//! Port 9528 binary upgrade protocol — firmware transfer and apply.
//!
//! This module implements the upgrade protocol confirmed from `Upgrade Huidu.pcapng`.
//! The protocol is entirely binary (no XML), using the same
//! `[u16 LE length][u16 LE cmd][payload]` framing as the BoxStream protocol.
//!
//! ## Wire-confirmed packet sequence
//!
//! **First TCP connection (port 9528):**
//!
//! 1. ConnectReq (0x000b): `[u32 LE 0x01000007]`
//! 2. ConnectAck (0x000c): version echo
//! 3. ClientInfoReq (0x0410): null-terminated CSV
//! 4. ClientInfoAck (0x0411): `[u16 LE 0]`
//! 5. NullCapQuery (0x0053): empty
//! 6. NullCapResp (0x0054): `[u32 LE 0]`
//! 7. CapQuery (0x040a): `[u16 LE 0]`
//! 8. CapResp (0x040b): `[u8 0]`
//! 9. UpgradeControl mode=1 (0x0055): `[u16 LE 1]`
//! 10. UpgradeStatus (0x0056): 6-byte status payload
//! 11. FileTransferReq (0x0017): `[/tmp/Box.tar.gz\0][u64 LE size]`
//! 12. FileTransferAck (0x0018): `[u32 LE 0]`
//! 13. FileDataChunks (0x0019): 9212-byte chunks, streaming
//!     - Device sends periodic acks (0x001a) and heartbeat responses (0x001c)
//! 14. Device sends FileComplete (0x0060): empty payload
//! 15. UpgradeControl mode=3 (0x0055): `[u16 LE 3]["killall -1 BoxDaemon; tar zxvf %s -C %s \0"]`
//! 16. UpgradeStatus (0x0056): status=3 (in progress)
//! 17. UpgradeControl mode=2 (0x0055): `[u16 LE 2]["upgrade.sh\0"]`
//!
//! **Second TCP connection (port 9528):**
//!
//! 18. ConnectReq (0x000b): `[u32 LE 0x01000007]`
//! 19. ConnectAck (0x000c): version echo (device may return different version)
//! 20. UpgradeExec (0x0730): `[u64 LE 8]`
//! 21. UpgradeExecAck (0x0731): `[u16 LE 0]`
//!
//! **Back on first connection:**
//!
//! 22. Poll UpgradeControl mode=0 (0x0055): `[u16 LE 0]`
//! 23. Heartbeat (0x005f): empty
//! 24. UpgradeStatus (0x0056): repeat until status=0 (success)

use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info, warn};

// ── Command codes confirmed from Upgrade Huidu.pcapng ────────────────────────

const CMD_CONNECT_REQ: u16 = 0x000b;
const CMD_CONNECT_ACK: u16 = 0x000c;
const CMD_CLIENT_INFO_REQ: u16 = 0x0410;
const CMD_CLIENT_INFO_ACK: u16 = 0x0411;
const CMD_NULL_CAP_QUERY: u16 = 0x0053;
const CMD_NULL_CAP_RESP: u16 = 0x0054;
const CMD_CAP_A: u16 = 0x040a;
const CMD_CAP_B: u16 = 0x040b;
const CMD_UPGRADE_CTRL: u16 = 0x0055;
const CMD_UPGRADE_STATUS: u16 = 0x0056;
const CMD_FILE_TRANSFER_REQ: u16 = 0x0017;
const CMD_FILE_TRANSFER_ACK: u16 = 0x0018;
const CMD_FILE_DATA_CHUNK: u16 = 0x0019;
const CMD_FILE_DATA_ACK: u16 = 0x001a; // periodic ack from device during transfer
const CMD_FILE_COMPLETE: u16 = 0x0060; // device signals file fully received
const CMD_HEARTBEAT: u16 = 0x005f;     // PC ping
const CMD_HEARTBEAT_ACK: u16 = 0x001c; // device pong
const CMD_UPGRADE_EXEC: u16 = 0x0730;
const CMD_UPGRADE_EXEC_ACK: u16 = 0x0731;

/// Protocol version sent in ConnectReq (frame 1420: 07 00 00 01 → LE = 0x01000007).
const CONNECT_VERSION: u32 = 0x01000007;
/// Data payload bytes per FileDataChunk packet (total packet = 4 header + 9212 = 9216).
const CHUNK_SIZE: usize = 9212;
/// Device target path for the uploaded firmware archive.
/// The real HDPlayer client always uses this fixed path regardless of the local
/// filename — confirmed in PCAP where the source was a `.zbin` file but the
/// device received it as `/tmp/Box.tar.gz`.
const DEVICE_FIRMWARE_PATH: &[u8] = b"/tmp/Box.tar.gz\0";

// ── Firmware file parsing ─────────────────────────────────────────────────────

/// The Huidu HDPLAYER .bin file has a 678-byte header before the tar.gz payload.
/// Confirmed from ZBIN_Firmware_Analysis.md: payload starts at offset 0x2A6.
const BIN_PAYLOAD_OFFSET: usize = 678;

/// Parsed firmware ready for upload.
struct FirmwareParsed {
    /// The raw tar.gz bytes to stream to the device.
    payload: Vec<u8>,
    /// UpgradeControl mode=3 payload: `[u16 LE 3][decompress_cmd\0]`.
    /// Read from the BIN XML header `<Decompress>` field.
    decompress_cmd: Vec<u8>,
    /// UpgradeControl mode=2 payload: `[u16 LE 2][script_name\0]`.
    /// Read from the BIN XML header `<Script>` field.
    script_name: Vec<u8>,
}

/// Parse a firmware file and extract the uploadable payload + device commands.
///
/// Handles three formats:
/// - `.zbin` (ZIP bundle): unzips, finds `BoxPlayer*.bin`, then strips BIN header
/// - `HDPLAYER` `.bin`: strips the 678-byte custom header, returns tar.gz payload
/// - Anything else: treated as a raw tar.gz (passed through unchanged)
fn parse_firmware_file(data: &[u8]) -> Result<FirmwareParsed> {
    // ZIP magic → .zbin bundle
    if data.starts_with(b"PK\x03\x04") {
        return parse_zbin(data);
    }
    // HDPLAYER binary header
    if data.starts_with(b"HDPLAYER") {
        return parse_bin(data);
    }
    // Raw payload — use as-is with the defaults from the BIN XML
    Ok(FirmwareParsed {
        payload: data.to_vec(),
        decompress_cmd: b"killall -1 BoxDaemon; tar zxvf %s -C %s \0".to_vec(),
        script_name: b"upgrade.sh\0".to_vec(),
    })
}

fn parse_zbin(data: &[u8]) -> Result<FirmwareParsed> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)?;

    // Find BoxPlayer*.bin (the Linux controller firmware)
    let bin_name = (0..archive.len())
        .find_map(|i| {
            archive.by_index(i).ok().and_then(|f| {
                let name = f.name().to_string();
                if name.starts_with("BoxPlayer") && name.ends_with(".bin") {
                    Some(name)
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| anyhow::anyhow!("No BoxPlayer*.bin found inside .zbin"))?;

    info!("Extracting {} from .zbin…", bin_name);
    let mut entry = archive.by_name(&bin_name)?;
    let mut bin_data = Vec::new();
    entry.read_to_end(&mut bin_data)?;
    parse_bin(&bin_data)
}

fn parse_bin(data: &[u8]) -> Result<FirmwareParsed> {
    if !data.starts_with(b"HDPLAYER") {
        bail!("Expected HDPLAYER magic in .bin file");
    }
    if data.len() <= BIN_PAYLOAD_OFFSET {
        bail!(".bin too short ({} bytes)", data.len());
    }

    // XML metadata occupies bytes 18..678; search it for Decompress and Script tags.
    let xml_end = BIN_PAYLOAD_OFFSET.min(data.len());
    let xml_region = std::str::from_utf8(&data[18..xml_end]).unwrap_or("");
    let decompress = xml_text(xml_region, "Decompress")
        .unwrap_or("killall -1 BoxDaemon; tar zxvf %s -C %s");
    let script = xml_text(xml_region, "Script").unwrap_or("upgrade.sh");
    info!("BIN Decompress: {}", decompress);
    info!("BIN Script: {}", script);

    // Build null-terminated byte vectors (trailing space matches PCAP for Decompress)
    let mut decompress_cmd = format!("{} ", decompress).into_bytes();
    decompress_cmd.push(0);
    let mut script_name = script.as_bytes().to_vec();
    script_name.push(0);

    // IMPORTANT: send the FULL .bin file including the 678-byte HDPLAYER header.
    // The device's BoxUpgrade service understands the HDPLAYER binary format natively.
    // Confirmed from PCAP: FileTransferReq size = 330,110,795 = full .bin size,
    // NOT 330,110,117 (which would be the size with the header stripped).
    Ok(FirmwareParsed {
        payload: data.to_vec(),
        decompress_cmd,
        script_name,
    })
}

/// Extract the text content of a simple XML tag (no attributes, no nesting).
fn xml_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)?;
    Some(xml[start..start + end].trim())
}
/// UpgradeExec parameter observed in capture (frame 103931: 08 00 00 00 00 00 00 00 → u64=8).
const UPGRADE_EXEC_PARAM: u64 = 8;

// ── Packet I/O ────────────────────────────────────────────────────────────────

/// Build a framed binary packet.
///
/// Wire format: `[u16 LE total_length][u16 LE cmd][payload]`
/// where `total_length = 4 + payload.len()`.
fn build_packet(cmd: u16, payload: &[u8]) -> Vec<u8> {
    let total = (4u32 + payload.len() as u32) as u16;
    let mut pkt = Vec::with_capacity(total as usize);
    pkt.extend_from_slice(&total.to_le_bytes());
    pkt.extend_from_slice(&cmd.to_le_bytes());
    pkt.extend_from_slice(payload);
    pkt
}

struct Conn {
    stream: TcpStream,
    buf: Vec<u8>,
}

impl Conn {
    async fn connect(addr: &str, port: u16) -> Result<Self> {
        let stream = TcpStream::connect(format!("{}:{}", addr, port)).await?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            buf: Vec::with_capacity(65536),
        })
    }

    async fn send(&mut self, cmd: u16, payload: &[u8]) -> Result<()> {
        let pkt = build_packet(cmd, payload);
        self.stream.write_all(&pkt).await?;
        Ok(())
    }

    /// Read the next complete application-level packet.
    /// Returns `(cmd, payload)`.
    async fn recv(&mut self) -> Result<(u16, Vec<u8>)> {
        loop {
            if self.buf.len() >= 4 {
                let total = u16::from_le_bytes([self.buf[0], self.buf[1]]) as usize;
                if total < 4 {
                    bail!("Malformed packet: length field {} < 4", total);
                }
                let cmd = u16::from_le_bytes([self.buf[2], self.buf[3]]);
                if self.buf.len() >= total {
                    let payload = self.buf[4..total].to_vec();
                    self.buf.drain(..total);
                    return Ok((cmd, payload));
                }
            }
            let mut tmp = [0u8; 32768];
            let n = self.stream.read(&mut tmp).await?;
            if n == 0 {
                bail!("Connection closed by device");
            }
            self.buf.extend_from_slice(&tmp[..n]);
        }
    }

    /// Receive next packet, skipping transparent periodic acks from device.
    async fn recv_skip_acks(&mut self) -> Result<(u16, Vec<u8>)> {
        loop {
            let (cmd, payload) = self.recv().await?;
            match cmd {
                CMD_FILE_DATA_ACK | CMD_HEARTBEAT_ACK => {
                    debug!("skip device ack cmd=0x{:04x}", cmd);
                }
                _ => return Ok((cmd, payload)),
            }
        }
    }

    /// Receive the next packet and assert it has the expected command.
    /// Periodic device acks (0x001a, 0x001c) are silently skipped.
    async fn expect(&mut self, expected: u16) -> Result<Vec<u8>> {
        let (cmd, payload) = self.recv_skip_acks().await?;
        if cmd != expected {
            bail!("expected cmd=0x{:04x} got cmd=0x{:04x}", expected, cmd);
        }
        Ok(payload)
    }
}

// ── Client info CSV ───────────────────────────────────────────────────────────

/// Build the null-terminated ClientInfoReq CSV payload.
///
/// Matches the format observed in frame 1423:
/// `OS,App,User,Hostname,,,_,YYYY-MM-DD_HH:MM:SS,<net>,<uuid>,YYYY/MM/DD HH:MM:SS\0`
fn build_client_info() -> Vec<u8> {
    use chrono::Local;
    let now = Local::now();
    let date1 = now.format("%Y-%m-%d_%H:%M:%S").to_string();
    let date2 = now.format("%Y/%m/%d %H:%M:%S").to_string();
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string());
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "HDPLAYER".to_string());
    let session_id = uuid::Uuid::new_v4().to_string();
    let csv = format!(
        "Windows,HDPlayer,{},{},,,_,{},Ethernet 00-00-00-00-00-00,{},{}",
        username, hostname, date1, session_id, date2,
    );
    let mut bytes = csv.into_bytes();
    bytes.push(0); // null terminator
    bytes
}

// ── Handshake ─────────────────────────────────────────────────────────────────

/// Perform the initial protocol handshake on a freshly-connected socket.
///
/// Sequence: ConnectReq → ConnectAck → ClientInfoReq → ClientInfoAck
///           → NullCapQuery → NullCapResp → CapQuery → CapResp
async fn handshake(conn: &mut Conn) -> Result<()> {
    conn.send(CMD_CONNECT_REQ, &CONNECT_VERSION.to_le_bytes())
        .await?;
    conn.expect(CMD_CONNECT_ACK).await?;
    debug!("ConnectAck ok");

    let info = build_client_info();
    conn.send(CMD_CLIENT_INFO_REQ, &info).await?;
    conn.expect(CMD_CLIENT_INFO_ACK).await?;
    debug!("ClientInfoAck ok");

    conn.send(CMD_NULL_CAP_QUERY, &[]).await?;
    conn.expect(CMD_NULL_CAP_RESP).await?;
    debug!("NullCapResp ok");

    // CapQuery: empty payload (PCAP frame 1427: 04000a04 = length 4, cmd 0x040a, no payload)
    conn.send(CMD_CAP_A, &[]).await?;
    conn.expect(CMD_CAP_B).await?;
    debug!("CapB ok");

    Ok(())
}

// ── Upgrade options ───────────────────────────────────────────────────────────

/// Options for the firmware upgrade process.
pub struct UpgradeOptions {
    /// How often to send UpgradeControl(mode=0) status polls after the upgrade executes.
    pub poll_interval: Duration,
    /// Maximum time to wait for the device to report upgrade complete.
    pub poll_timeout: Duration,
    /// Progress callback: receives (bytes_sent, total_bytes).
    pub progress: Option<Box<dyn Fn(u64, u64) + Send>>,
    /// Phase callback: called with a short human-readable status string at each
    /// major step (e.g. "Connecting", "Transferring", "Extracting", "Polling").
    pub phase: Option<Box<dyn Fn(&str) + Send>>,
}

impl Default for UpgradeOptions {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            poll_timeout: Duration::from_secs(600),
            progress: None,
            phase: None,
        }
    }
}

/// Helper: call the phase callback if set.
fn report_phase(opts: &UpgradeOptions, msg: &str) {
    if let Some(ref cb) = opts.phase {
        cb(msg);
    }
    info!("{}", msg);
}

// ── Main upgrade entry point ──────────────────────────────────────────────────

/// Run the full port-9528 firmware upgrade.
///
/// `addr` — device IP address (without port).
/// `file_path` — local path to the firmware archive (`.zbin` as released by Huidu,
///               e.g. `BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin`).
pub async fn run_upgrade(addr: &str, file_path: &Path, opts: UpgradeOptions) -> Result<()> {
    // ── Phase 0: parse firmware file ─────────────────────────────────────────
    // .zbin is a ZIP bundle → extract BoxPlayer*.bin → strip 678-byte header
    // → send only the inner tar.gz payload.  The real HDPlayer client does this
    // extraction before opening the 9528 connection.
    report_phase(&opts, "Parsing firmware archive…");
    let raw = tokio::fs::read(file_path).await?;
    let fw = tokio::task::spawn_blocking(move || parse_firmware_file(&raw)).await??;
    let file_data = fw.payload;
    let file_size = file_data.len() as u64;
    info!(
        "Firmware payload: {} bytes ({} chunks) extracted from {}",
        file_size,
        (file_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE,
        file_path.display(),
    );

    // ── Phase 1: connect and handshake ────────────────────────────────────────
    report_phase(&opts, "Connecting…");
    let mut conn = Conn::connect(addr, 9528).await?;
    report_phase(&opts, "Handshaking…");
    handshake(&mut conn).await?;

    // ── Phase 2: enter upgrade mode ───────────────────────────────────────────
    report_phase(&opts, "Entering upgrade mode…");
    conn.send(CMD_UPGRADE_CTRL, &1u16.to_le_bytes()).await?;
    let st = conn.expect(CMD_UPGRADE_STATUS).await?;
    let status_code = if st.len() >= 2 {
        u16::from_le_bytes([st[0], st[1]])
    } else {
        0
    };
    debug!("UpgradeStatus after enter-upgrade: {}", status_code);
    info!("Device entered upgrade mode");

    // ── Phase 3: initiate file transfer ───────────────────────────────────────
    let mut req = Vec::with_capacity(DEVICE_FIRMWARE_PATH.len() + 8);
    req.extend_from_slice(DEVICE_FIRMWARE_PATH);
    req.extend_from_slice(&file_size.to_le_bytes());
    conn.send(CMD_FILE_TRANSFER_REQ, &req).await?;

    let ack = conn.expect(CMD_FILE_TRANSFER_ACK).await?;
    if ack.len() >= 4 {
        let code = u32::from_le_bytes([ack[0], ack[1], ack[2], ack[3]]);
        if code != 0 {
            bail!("FileTransferAck returned error code {}", code);
        }
    }
    report_phase(&opts, &format!("Transferring {:.1} MB…", file_size as f64 / 1_000_000.0));

    // ── Phase 4: stream file data chunks ─────────────────────────────────────
    let total_chunks = (file_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let mut bytes_sent: u64 = 0;
    for (i, chunk) in file_data.chunks(CHUNK_SIZE).enumerate() {
        conn.send(CMD_FILE_DATA_CHUNK, chunk).await?;
        bytes_sent += chunk.len() as u64;
        if let Some(ref cb) = opts.progress {
            cb(bytes_sent, file_size);
        }
        if (i + 1) % 500 == 0 || i + 1 == total_chunks {
            info!(
                "Transfer: {}/{} chunks ({:.1}%)",
                i + 1,
                total_chunks,
                bytes_sent as f64 / file_size as f64 * 100.0
            );
        }
    }
    report_phase(&opts, "Transfer complete — waiting for device…");

    // ── Phase 5: wait for file-complete notification (cmd 0x0060) ─────────────
    report_phase(&opts, "Waiting for device to confirm receipt…");
    loop {
        let (cmd, _) = conn.recv().await?;
        match cmd {
            CMD_FILE_DATA_ACK | CMD_HEARTBEAT_ACK => {
                // Periodic ack during/after transfer — keep waiting
            }
            CMD_FILE_COMPLETE => {
                info!("Device confirmed file fully received (cmd 0x0060)");
                break;
            }
            other => {
                warn!("Unexpected cmd=0x{:04x} while waiting for FileComplete", other);
            }
        }
    }

    // ── Phase 6: trigger extraction ───────────────────────────────────────────
    report_phase(&opts, "Extracting firmware…");
    // Heartbeat exchange before extract command (matches real client behaviour)
    conn.send(CMD_HEARTBEAT, &[]).await?;
    conn.expect(CMD_HEARTBEAT_ACK).await?;

    // UpgradeControl mode=3: [u16 LE 3][decompress_cmd\0]
    // The decompress_cmd comes from the BIN header's <Decompress> XML field.
    let mut ctrl3 = Vec::with_capacity(2 + fw.decompress_cmd.len());
    ctrl3.extend_from_slice(&3u16.to_le_bytes());
    ctrl3.extend_from_slice(&fw.decompress_cmd);
    conn.send(CMD_UPGRADE_CTRL, &ctrl3).await?;
    let st = conn.expect(CMD_UPGRADE_STATUS).await?;
    let sc = if st.len() >= 2 { u16::from_le_bytes([st[0], st[1]]) } else { 0 };
    debug!("UpgradeStatus after extract cmd: {}", sc);
    info!("Extract command accepted");

    // UpgradeControl mode=2: [u16 LE 2][script_name\0]
    // The script_name comes from the BIN header's <Script> XML field.
    let mut ctrl2 = Vec::with_capacity(2 + fw.script_name.len());
    ctrl2.extend_from_slice(&2u16.to_le_bytes());
    ctrl2.extend_from_slice(&fw.script_name);
    conn.send(CMD_UPGRADE_CTRL, &ctrl2).await?;
    info!("Upgrade script queued");

    // ── Phase 7: second connection → UpgradeExec ─────────────────────────────
    report_phase(&opts, "Applying upgrade…");
    info!("Opening second connection for UpgradeExec ...");
    let mut conn2 = Conn::connect(addr, 9528).await?;
    // Full handshake on second connection (PCAP shows ClientInfoAck arrives after
    // UpgradeExecAck, suggesting the device expects the full exchange).
    handshake(&mut conn2).await?;
    debug!("Second connection handshake ok");

    // UpgradeExec: payload = u64 LE 8 (confirmed from frame 103931)
    conn2
        .send(CMD_UPGRADE_EXEC, &UPGRADE_EXEC_PARAM.to_le_bytes())
        .await?;
    conn2.expect(CMD_UPGRADE_EXEC_ACK).await?;
    info!("UpgradeExec acknowledged — device is applying firmware");

    // ── Phase 8: poll for completion on original connection ───────────────────
    report_phase(&opts, "Polling for completion…");
    info!(
        "Polling for upgrade completion (interval={:?}, timeout={:?}) ...",
        opts.poll_interval, opts.poll_timeout,
    );
    let deadline = Instant::now() + opts.poll_timeout;

    loop {
        if Instant::now() > deadline {
            bail!(
                "Upgrade timed out after {:?} — device did not report completion",
                opts.poll_timeout
            );
        }

        // Send status poll
        if conn.send(CMD_UPGRADE_CTRL, &0u16.to_le_bytes()).await.is_err() {
            report_phase(&opts, "Device rebooting — upgrade applied");
            return Ok(());
        }
        // Interleave heartbeat (matches real client)
        let _ = conn.send(CMD_HEARTBEAT, &[]).await;

        // Wait up to poll_interval for UpgradeStatus
        let wait_until = Instant::now() + opts.poll_interval;
        loop {
            let remaining = wait_until.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, conn.recv()).await {
                Ok(Ok((CMD_UPGRADE_STATUS, payload))) => {
                    let status = if payload.len() >= 2 {
                        u16::from_le_bytes([payload[0], payload[1]])
                    } else {
                        0
                    };
                    info!("UpgradeStatus poll: {}", status);
                    if status == 0 {
                        report_phase(&opts, "Upgrade complete!");
                        return Ok(());
                    }
                    report_phase(&opts, &format!("Applying… (status {})", status));
                    // Non-zero status → still in progress; break inner loop to re-poll
                    break;
                }
                Ok(Ok((CMD_FILE_DATA_ACK, _)))
                | Ok(Ok((CMD_HEARTBEAT_ACK, _))) => {
                    // Ignore transparent acks
                }
                Ok(Ok((cmd, _))) => {
                    debug!("Poll: unhandled cmd=0x{:04x}", cmd);
                }
                Ok(Err(_)) => {
                    // Connection dropped — device rebooted
                    report_phase(&opts, "Device rebooting — upgrade applied");
                    return Ok(());
                }
                Err(_) => {
                    // Timeout waiting for response — send another poll
                    break;
                }
            }
        }
    }
}
