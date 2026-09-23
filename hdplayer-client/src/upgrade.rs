//! Port 9528 binary upgrade protocol — firmware transfer and apply.
//!
//! Wire-confirmed against a capture of the stock HDPlayer 7.11.18.0 upgrading a
//! C15 from 7.4.61.0 → 7.11.18.0 (`hdplayer_firmware_upgrade_20260923_1609.pcapng`,
//! upgrade succeeded).  Framing is `[u16 LE total_length][u16 LE cmd][payload]`,
//! the same as BoxStream.  Times below are seconds from the first SYN.
//!
//! **Connection 1 (port 9528):**
//!
//! 1.  ConnectReq (0x000b) `[u32 LE 0x01000007]` → ConnectAck (0x000c) echo
//! 2.  ClientInfoReq (0x0410) null-terminated CSV → ClientInfoAck (0x0411) `[u16 0]`
//! 3.  NullCapQuery (0x0053) empty → `[u32 0]`;  CapQuery (0x040a) empty → `[u8 0]`
//! 4.  UpgradeCMD mode=1 (0x0055) `[u16 1]` → UpgradeStatus (0x0056)
//!     `[u16 1][u8 a][u8 b][u8 c][u8 d]` = the device's limit version a.b.c.d
//!     (7.4.59.0 on this box).
//! 5.  OpenFileAsk (0x0017) `[/tmp/Box.tar.gz\0][u64 LE size]` → `[u32 0]`.
//!     `size` is the WHOLE `.bin` size, although only the tar.gz after the
//!     header is streamed (see [`FirmwareParsed::declared_size`]).
//! 6.  FileContentAsk (0x0019) × N, 9212-byte payloads; the device answers EVERY
//!     chunk with FileContentAnswer (0x001a) `[u32 0]` (35835 / 35835).  During
//!     the transfer the device also sends a bare 0x0060 every ~5.7 s, and the PC
//!     sends a bare heartbeat 0x005f every ~6 s for the life of the connection.
//!     Neither side answers the other's keepalive.  330 MB took ~34 s.
//! 7.  CloseFileAsk (0x001b) EMPTY payload (`04 00 1b 00`) → CloseFileAnswer
//!     (0x001c) `[u32 0]` ~0.85 s later.
//! 8.  UpgradeCMD mode=3 `[u16 3]["killall -1 BoxDaemon; tar zxvf %s -C %s \0"]`
//!     → UpgradeStatus `[u16 3][u32 0]` immediately.
//! 9.  UpgradeCMD mode=2 `[u16 2]["upgrade.sh\0"]` — no reply.
//! 10. PC polls mode=0 every 5 s (+ heartbeat every 6 s).  The device never
//!     answers again on this connection (its services are being replaced).
//!     HDPlayer gives up and RSTs it ~70 s after mode=2.
//!
//! **Connection 2 (port 9528), opened ~85 s after mode=2:**
//!
//! 11. ConnectReq `[u32 0x01000007]` → ConnectAck `[u32 0x01000009]` — 14 s
//!     later, and version 9, not 7: it is the NEW firmware's BoxUpgrade replying.
//! 12. UpgradeExec (0x0730) `[u64 LE 8]` → ExecAck (0x0731) `[u16 0]` (~10 s)
//! 13. ClientInfoReq → ClientInfoAck (~10 s);  NullCapQuery → `[u32 0]` (~10 s)
//! 14. Poll UpgradeCMD mode=0 every 5 s → UpgradeStatus `[u16 0][i32 result]`,
//!     where result comes from `/root/upgrade.status` (`SendGetUpgradeResultAnswer`):
//!     `'0'`→0 still running, `'1'`→1 SUCCESS, `'2'`→2 failed, else −1.
//!     Got result=1 ~41 s after UpgradeExec; HDPlayer then closed (FIN).
//!
//! NOTE: an UpgradeStatus payload's first u16 is the ECHOED MODE, not a status.
//! Treating it as the status (as this client used to) made every mode=0 reply
//! look like "done".  HDPlayer also opened a second, unused socket at the same
//! moment as connection 2; it never sent on it.

use std::io::Read as _;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tracing::{debug, info};

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
const CMD_FILE_TRANSFER_REQ: u16 = 0x0017; // OpenFileAsk
const CMD_FILE_TRANSFER_ACK: u16 = 0x0018;
const CMD_FILE_DATA_CHUNK: u16 = 0x0019;    // FileContentAsk
const CMD_CLOSE_FILE: u16 = 0x001b;         // CloseFileAsk — device close()s + renames the file. MUST be sent with an EMPTY payload (device requires total_length == 4).
const CMD_CLOSE_FILE_ACK: u16 = 0x001c;     // CloseFileAnswer — on port 9528 this code is ONLY ever the close answer (verified: emitted from exactly one site in BoxUpgrade).
const CMD_FILE_DATA_ACK: u16 = 0x001a; // FileContentAnswer — device acks every data chunk
const CMD_DEVICE_KEEPALIVE: u16 = 0x0060; // bare device keepalive, sent every ~5.7 s during the transfer; never answered
const CMD_HEARTBEAT: u16 = 0x005f;     // bare PC keepalive; HDPlayer sends it every ~6 s for the whole session; never answered
const CMD_UPGRADE_EXEC: u16 = 0x0730;
const CMD_UPGRADE_EXEC_ACK: u16 = 0x0731;

/// Protocol version sent in ConnectReq (frame 1420: 07 00 00 01 → LE = 0x01000007).
const CONNECT_VERSION: u32 = 0x01000007;
/// HDPlayer's keepalive period (0x005f), measured in the 2026-09-23 capture.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(6);
/// Silence on connection 1 after mode=2 before we give up on it and reconnect.
/// The device answers nothing there once `killall -1 BoxDaemon` has run.
/// HDPlayer RST'd the connection ~70 s after mode=2.
const CONN1_SILENCE: Duration = Duration::from_secs(60);
/// The reconnect handshake is slow while the new firmware comes up: ConnectAck
/// took 14 s and each later reply ~10 s in the capture.
const CONN2_STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// `/root/upgrade.status` as reported in UpgradeStatus(mode=0) — see
/// `SendGetUpgradeResultAnswer`.
const RESULT_RUNNING: i32 = 0;
const RESULT_SUCCESS: i32 = 1;
const RESULT_FAILED: i32 = 2;

/// Split an UpgradeStatus (0x0056) payload into `(echoed mode, i32 value)`.
///
/// The payload is `[u16 LE mode][4 bytes]`.  For mode=0 the 4 bytes are the
/// i32 result (see `RESULT_*`); for mode=1 they are the limit version, one byte
/// per dotted-quad field; for mode=3 they are 0.
fn parse_upgrade_status(p: &[u8]) -> Option<(u16, [u8; 4])> {
    if p.len() < 6 {
        return None;
    }
    Some((u16::from_le_bytes([p[0], p[1]]), [p[2], p[3], p[4], p[5]]))
}
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
    /// Size announced in OpenFileAsk (0x0017).  HDPlayer announces the size of
    /// the WHOLE `.bin` (header included) even though it only streams the tar.gz
    /// payload — 2026-09-23 capture: declared 330110795, sent 330110117 (diff =
    /// the 678-byte header).  We mirror that; for raw archives it is the payload size.
    declared_size: u64,
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
    // Raw payload (custom tar.gz, not a .zbin/.bin with BIN XML).
    // Do NOT include "killall -1 BoxDaemon" in the decompress command: sending
    // SIGHUP to BoxDaemon causes it to restart, losing the upgrade state it needs
    // to accept UpgradeExec.  For raw archives we keep BoxDaemon alive so it can
    // respond to UpgradeExec on the same connection that delivered the file.
    Ok(FirmwareParsed {
        payload: data.to_vec(),
        declared_size: data.len() as u64,
        decompress_cmd: b"tar zxvf %s -C %s \0".to_vec(),
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
    if data.len() < 28 {
        bail!(".bin too short ({} bytes)", data.len());
    }

    // Header: magic(8) + md5(16) + u32 LE xml_len(4) + xml + payload.
    // Payload offset = 28 + xml_len (this is 678 for the stock BoxPlayer .bin,
    // but varies for smaller packages we build, so compute it rather than
    // hardcoding BIN_PAYLOAD_OFFSET).  Fall back to the legacy constant if the
    // computed region doesn't look like the firmware XML.
    let xml_len = u32::from_le_bytes([data[24], data[25], data[26], data[27]]) as usize;
    let computed = 28usize.checked_add(xml_len).unwrap_or(usize::MAX);
    let payload_offset = if computed <= data.len()
        && std::str::from_utf8(&data[28..computed]).map_or(false, |s| s.contains("<FirmwareInfo"))
    {
        computed
    } else if data.len() > BIN_PAYLOAD_OFFSET {
        BIN_PAYLOAD_OFFSET
    } else {
        bail!(".bin header unrecognised (xml_len={xml_len}, size={})", data.len());
    };

    // Search the XML region for Decompress and Script tags.
    let xml_region = std::str::from_utf8(&data[28..payload_offset]).unwrap_or("");
    let decompress = xml_text(xml_region, "Decompress")
        .unwrap_or("killall -1 BoxDaemon; tar zxvf %s -C %s");
    // Use the <Decompress> command VERBATIM, including the "killall -1 BoxDaemon;"
    // prefix. A capture of the real HDPlayer.exe upgrade shows it sends exactly
    // "killall -1 BoxDaemon; tar zxvf %s -C %s " as UpgradeControl mode=3, and the
    // upgrade succeeds. Our earlier code stripped the killall (and then relied on a
    // bogus UpgradeExec on a second connection) — that was wrong.
    let script = xml_text(xml_region, "Script").unwrap_or("upgrade.sh");
    info!("BIN Decompress: {}", decompress);
    info!("BIN Script: {}", script);

    // Build null-terminated byte vectors (trailing space matches the HDPlayer capture)
    let mut decompress_cmd = format!("{} ", decompress).into_bytes();
    decompress_cmd.push(0);
    let mut script_name = script.as_bytes().to_vec();
    script_name.push(0);

    // IMPORTANT: strip the 678-byte HDPLAYER header and send ONLY the raw tar.gz payload.
    // Confirmed from PCAP (Upgrade Huidu.pcapng frame 1463): the first FileDataChunk
    // starts with bytes 1f 8b 08 00 (gzip magic), meaning the real HDPlayer client
    // strips the header before uploading.  Sending the full .bin (starting with
    // "HDPLAYER") causes `tar zxvf` on the device to fail immediately, which puts
    // the device back to idle state and causes UpgradeExec to be rejected.
    Ok(FirmwareParsed {
        payload: data[payload_offset..].to_vec(),
        declared_size: data.len() as u64,
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

/// Extract the BoxPlayer firmware version string from in-memory firmware data.
///
/// Handles `.zbin` (ZIP bundle containing `BoxPlayer*.bin`) and plain HDPLAYER `.bin`
/// formats.  Returns `None` if the format is unrecognised or the `<Version>` tag is absent.
pub fn firmware_file_version(data: &[u8]) -> Option<String> {
    if data.starts_with(b"PK\x03\x04") {
        // .zbin → find BoxPlayer*.bin inside and recurse on its contents
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).ok()?;
        let bin_name = (0..archive.len()).find_map(|i| {
            archive.by_index(i).ok().and_then(|f| {
                let name = f.name().to_string();
                if name.starts_with("BoxPlayer") && name.ends_with(".bin") {
                    Some(name)
                } else {
                    None
                }
            })
        })?;
        // Extract version from the filename — no decompression required.
        // "BoxPlayer_7_11_18_0.bin"  → "7.11.18.0"
        // "BoxPlayer_V7.11.18.0.bin" → "7.11.18.0"
        let stem = bin_name
            .strip_prefix("BoxPlayer_").unwrap_or(&bin_name)
            .strip_suffix(".bin").unwrap_or(&bin_name);
        let stem = stem.strip_prefix('V').unwrap_or(stem);
        return Some(stem.replace('_', "."));
    }
    if data.starts_with(b"HDPLAYER") && data.len() > 18 {
        let xml_end = BIN_PAYLOAD_OFFSET.min(data.len());
        let xml_region = std::str::from_utf8(&data[18..xml_end]).ok()?;
        return xml_text(xml_region, "Version").map(str::to_string);
    }
    None
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

    /// Receive next packet, skipping the device's transparent traffic:
    /// per-chunk FileContentAnswers (0x001a) and bare keepalives (0x0060).
    async fn recv_skip_acks(&mut self) -> Result<(u16, Vec<u8>)> {
        loop {
            let (cmd, payload) = self.recv().await?;
            match cmd {
                CMD_FILE_DATA_ACK | CMD_DEVICE_KEEPALIVE => {
                    debug!("skip transparent cmd=0x{:04x}", cmd);
                }
                _ => return Ok((cmd, payload)),
            }
        }
    }

    /// Receive the next packet and assert it has the expected command.
    /// Periodic device traffic (0x001a, 0x0060) is silently skipped.
    async fn expect(&mut self, expected: u16) -> Result<Vec<u8>> {
        let (cmd, payload) = self.recv_skip_acks().await?;
        if cmd != expected {
            bail!("expected cmd=0x{:04x} got cmd=0x{:04x}", expected, cmd);
        }
        Ok(payload)
    }

    /// Like [`Conn::expect`], but gives up after `limit` and keeps HDPlayer's
    /// 6 s heartbeat going while it waits (the device can take 10–15 s per reply
    /// while the new firmware comes up).
    async fn expect_within(&mut self, expected: u16, limit: Duration) -> Result<Vec<u8>> {
        let deadline = Instant::now() + limit;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("no cmd=0x{:04x} within {:?}", expected, limit);
            }
            match tokio::time::timeout(remaining.min(HEARTBEAT_EVERY), self.expect(expected)).await {
                Ok(r) => return r,
                // Cancelling mid-read is safe: recv() only drains self.buf once a
                // whole packet is buffered, so partial reads stay in self.buf.
                Err(_) => self.send(CMD_HEARTBEAT, &[]).await?,
            }
        }
    }

    /// Pull whatever the socket already has into `self.buf` without blocking.
    fn drain_nonblocking(&mut self) -> Result<()> {
        let mut tmp = [0u8; 65536];
        loop {
            match self.stream.try_read(&mut tmp) {
                Ok(0) => bail!("Connection closed by device"),
                Ok(n) => self.buf.extend_from_slice(&tmp[..n]),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e.into()),
            }
        }
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
    /// Upper bound on how long to keep polling connection 1 after mode=2
    /// before reconnecting (the effective wait is min(this, 60 s of silence)).
    pub decompress_wait: Duration,
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
            decompress_wait: Duration::from_secs(600),
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

/// Run the full firmware upgrade via the binary streaming protocol.
///
/// `addr` — device IP address (without port).
/// `port` — TCP port for the upgrade protocol (normally 9528; use 10001 for legacy devices).
/// `file_path` — local path to the firmware archive (`.zbin` as released by Huidu,
///               e.g. `BoxPlayer_V7.11.18.0_MagicPlayer_V2.12.8.0.zbin`).
pub async fn run_upgrade(addr: &str, port: u16, file_path: &Path, opts: UpgradeOptions) -> Result<()> {
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
    let mut conn = Conn::connect(addr, port).await?;
    report_phase(&opts, "Handshaking…");
    handshake(&mut conn).await?;

    // ── Phase 2: limit-version query (UpgradeCMD mode=1) ─────────────────────
    report_phase(&opts, "Querying limit version…");
    conn.send(CMD_UPGRADE_CTRL, &1u16.to_le_bytes()).await?;
    let st = conn.expect(CMD_UPGRADE_STATUS).await?;
    match parse_upgrade_status(&st) {
        Some((1, v)) => info!("Device limit version: {}.{}.{}.{}", v[0], v[1], v[2], v[3]),
        _ => info!("Unexpected limit-version answer: {:02x?}", st),
    }

    // ── Phase 3: open the file on the device ─────────────────────────────────
    let mut req = Vec::with_capacity(DEVICE_FIRMWARE_PATH.len() + 8);
    req.extend_from_slice(DEVICE_FIRMWARE_PATH);
    req.extend_from_slice(&fw.declared_size.to_le_bytes());
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
    // After each chunk, drain incoming device traffic with a non-blocking read.
    // The device answers every chunk with 0x001a and sends 0x0060 keepalives;
    // if we never read them our receive window fills, the single-threaded
    // device blocks on its send, stops reading, and both sides deadlock.
    // HDPlayer keeps its 6 s heartbeat going throughout, so we do too.
    let total_chunks = (file_data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let mut bytes_sent: u64 = 0;
    let mut next_heartbeat = Instant::now() + HEARTBEAT_EVERY;
    for (i, chunk) in file_data.chunks(CHUNK_SIZE).enumerate() {
        conn.send(CMD_FILE_DATA_CHUNK, chunk).await?;
        bytes_sent += chunk.len() as u64;
        conn.drain_nonblocking()?;
        if Instant::now() >= next_heartbeat {
            conn.send(CMD_HEARTBEAT, &[]).await?;
            next_heartbeat = Instant::now() + HEARTBEAT_EVERY;
        }

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

    // ── Phase 4b: CloseFile (empty payload) → CloseFileAnswer ────────────────
    // CloseFile (0x001b) MUST carry an EMPTY payload: BoxUpgrade's
    // `RecvCloseFileAsk` requires total_length == 4 (PX30 BoxUpgrade @ 0x420798
    // `ccmp w2,#4`; RK3288 libBoxUpgrade.so @ 0x138bc `cmp r2,#4 ; bne`) and
    // silently drops anything else, leaving /tmp/Box.tar.gz uncommitted.
    // HDPlayer sends exactly `04 00 1b 00` and gets 0x001c back in ~0.85 s.
    // Leftover 0x001a answers are skipped by recv_skip_acks.
    info!("Sending CloseFile (0x001b, empty payload) to flush + commit the archive…");
    conn.send(CMD_CLOSE_FILE, &[]).await?;
    match tokio::time::timeout(Duration::from_secs(15), conn.recv_skip_acks()).await {
        Ok(Ok((CMD_CLOSE_FILE_ACK, _))) => {
            info!("CloseFileAnswer (0x001c) — archive flushed + renamed on device")
        }
        Ok(Ok((cmd, _))) => {
            bail!("expected CloseFileAnswer (0x001c) after CloseFile, got cmd=0x{:04x}", cmd)
        }
        Ok(Err(e)) => bail!("connection closed after CloseFile: {}", e),
        // No 0x001c means SendCloseFileAnswer never ran — the file is NOT
        // committed.  Fail loudly instead of reporting a false success.
        Err(_) => bail!(
            "no CloseFileAnswer (0x001c) within 15 s — archive was not committed; \
             aborting so we don't report a phantom upgrade"
        ),
    }

    // ── Phase 5: UpgradeCMD mode=3 (unpack) and mode=2 (run script) ──────────
    // Same connection that delivered the file.  mode=3 answers `[u16 3][u32 0]`
    // immediately; mode=2 is never answered.
    report_phase(&opts, "Starting decompression…");
    let mut ctrl3 = Vec::with_capacity(2 + fw.decompress_cmd.len());
    ctrl3.extend_from_slice(&3u16.to_le_bytes());
    ctrl3.extend_from_slice(&fw.decompress_cmd);
    conn.send(CMD_UPGRADE_CTRL, &ctrl3).await?;
    let st = conn.expect(CMD_UPGRADE_STATUS).await?;
    info!("Decompress command accepted: {:02x?}", st);

    let mut ctrl2 = Vec::with_capacity(2 + fw.script_name.len());
    ctrl2.extend_from_slice(&2u16.to_le_bytes());
    ctrl2.extend_from_slice(&fw.script_name);
    conn.send(CMD_UPGRADE_CTRL, &ctrl2).await?;
    info!("Upgrade script queued (mode=2)");
    report_phase(&opts, "Upgrade script running…");

    // ── Phase 6: poll connection 1 until the device goes quiet ───────────────
    // The Decompress command starts with `killall -1 BoxDaemon`, and upgrade.sh
    // replaces the services, so in practice the device never answers here.
    // We still poll (as HDPlayer does) in case a quick package finishes in
    // place; once the socket closes, or is silent for CONN1_SILENCE (capped
    // by decompress_wait), we move on to a fresh connection.
    let deadline = Instant::now() + opts.poll_timeout;
    if let Some(result) = poll_result(&mut conn, &opts, CONN1_SILENCE.min(opts.decompress_wait)).await? {
        return finish(&opts, result);
    }
    drop(conn);

    // ── Phase 7: reconnect, UpgradeExec, poll for the result ─────────────────
    // HDPlayer reconnected ~85 s after mode=2.  The reply's ConnectAck carried
    // protocol version 9 (not 7): the NEW firmware's BoxUpgrade is answering.
    // Every reply took 10–15 s while it came up, hence CONN2_STEP_TIMEOUT.
    report_phase(&opts, "Reconnecting to the upgraded services…");
    let mut conn2 = loop {
        if Instant::now() > deadline {
            bail!("Upgrade timed out after {:?} waiting to reconnect", opts.poll_timeout);
        }
        match reconnect_exec(addr, port).await {
            Ok(c) => break c,
            Err(e) => {
                debug!("reconnect attempt failed: {} — retrying", e);
                tokio::time::sleep(opts.poll_interval).await;
            }
        }
    };

    report_phase(&opts, "Waiting for the upgrade result…");
    let remaining = deadline.saturating_duration_since(Instant::now());
    match poll_result(&mut conn2, &opts, remaining).await? {
        Some(result) => finish(&opts, result),
        None => bail!("Upgrade timed out after {:?} — no result from device", opts.poll_timeout),
    }
}

/// Connection 2 as HDPlayer does it: ConnectReq → ConnectAck, UpgradeExec
/// `[u64 8]` → ExecAck, ClientInfoReq → Ack, NullCapQuery → Resp.
async fn reconnect_exec(addr: &str, port: u16) -> Result<Conn> {
    let mut conn = Conn::connect(addr, port).await?;
    conn.send(CMD_CONNECT_REQ, &CONNECT_VERSION.to_le_bytes()).await?;
    let ack = conn.expect_within(CMD_CONNECT_ACK, CONN2_STEP_TIMEOUT).await?;
    info!("ConnectAck on reconnect: {:02x?}", ack);

    conn.send(CMD_UPGRADE_EXEC, &UPGRADE_EXEC_PARAM.to_le_bytes()).await?;
    let ack = conn.expect_within(CMD_UPGRADE_EXEC_ACK, CONN2_STEP_TIMEOUT).await?;
    info!("UpgradeExecAck: {:02x?}", ack);

    conn.send(CMD_CLIENT_INFO_REQ, &build_client_info()).await?;
    conn.expect_within(CMD_CLIENT_INFO_ACK, CONN2_STEP_TIMEOUT).await?;
    conn.send(CMD_NULL_CAP_QUERY, &[]).await?;
    conn.expect_within(CMD_NULL_CAP_RESP, CONN2_STEP_TIMEOUT).await?;
    Ok(conn)
}

/// Poll UpgradeCMD mode=0 every `poll_interval` (heartbeat every 6 s) until the
/// device reports a final result (success/failure).  Returns `Ok(None)` if the
/// connection closes or stays silent for `silence`.
async fn poll_result(conn: &mut Conn, opts: &UpgradeOptions, silence: Duration) -> Result<Option<i32>> {
    let mut last_heard = Instant::now();
    let mut next_poll = Instant::now();
    let mut next_heartbeat = Instant::now() + HEARTBEAT_EVERY;
    loop {
        let now = Instant::now();
        if now.duration_since(last_heard) >= silence {
            info!("No reply for {:?} — leaving this connection", silence);
            return Ok(None);
        }
        if now >= next_poll {
            if conn.send(CMD_UPGRADE_CTRL, &0u16.to_le_bytes()).await.is_err() {
                return Ok(None);
            }
            next_poll = now + opts.poll_interval;
        }
        if now >= next_heartbeat {
            if conn.send(CMD_HEARTBEAT, &[]).await.is_err() {
                return Ok(None);
            }
            next_heartbeat = now + HEARTBEAT_EVERY;
        }

        let wake = next_poll.min(next_heartbeat);
        match tokio::time::timeout_at(wake.into(), conn.recv()).await {
            Err(_) => {}
            Ok(Err(e)) => {
                info!("Connection ended: {}", e);
                return Ok(None);
            }
            Ok(Ok((cmd, payload))) => {
                last_heard = Instant::now();
                if cmd != CMD_UPGRADE_STATUS {
                    debug!("poll: ignoring cmd=0x{:04x}", cmd);
                    continue;
                }
                match parse_upgrade_status(&payload) {
                    Some((0, v)) => {
                        let result = i32::from_le_bytes(v);
                        if result == RESULT_SUCCESS || result == RESULT_FAILED {
                            return Ok(Some(result));
                        }
                        // 0 = script running; -1 = status file not there (yet).
                        let state = if result == RESULT_RUNNING { "running" } else { "unknown" };
                        report_phase(opts, &format!("Upgrade in progress ({state})…"));
                    }
                    _ => debug!("poll: other UpgradeStatus {:02x?}", payload),
                }
            }
        }
    }
}

/// Map a final `/root/upgrade.status` result to success or an error.
fn finish(opts: &UpgradeOptions, result: i32) -> Result<()> {
    match result {
        RESULT_SUCCESS => {
            report_phase(opts, "Upgrade complete!");
            Ok(())
        }
        _ => bail!("Device reported upgrade FAILED (upgrade.status = {})", result),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard for the port-9528 upgrade blocker.
    ///
    /// The device's BoxUpgrade `RecvCloseFileAsk` accepts CloseFile ONLY when the
    /// frame's total_length field == 4 (i.e. an empty payload).  `build_packet`
    /// sets total_length = 4 + payload.len(), so CloseFile MUST be built with an
    /// empty payload.  A 4-byte payload (the old bug) produced total_length = 8,
    /// which the device rejected silently — the archive was never flushed/renamed
    /// and `tar` extracted nothing.
    #[test]
    fn close_file_frame_has_total_length_4() {
        let pkt = build_packet(CMD_CLOSE_FILE, &[]);
        // Wire bytes: [u16 LE total_length=4][u16 LE cmd=0x001b]
        assert_eq!(pkt, vec![0x04, 0x00, 0x1b, 0x00], "CloseFile must be exactly `04 00 1B 00`");
        let total = u16::from_le_bytes([pkt[0], pkt[1]]);
        assert_eq!(total, 4, "device RecvCloseFileAsk requires total_length == 4");
    }

    /// UpgradeStatus payloads captured from HDPlayer 7.11.18.0 ↔ C15 (2026-09-23).
    /// The first u16 is the echoed mode, NOT the status.
    #[test]
    fn upgrade_status_payloads_from_capture() {
        // mode=1 → limit version 7.4.59.0
        assert_eq!(parse_upgrade_status(&[0x01, 0x00, 0x07, 0x04, 0x3b, 0x00]), Some((1, [7, 4, 59, 0])));
        // mode=3 → accepted
        assert_eq!(parse_upgrade_status(&[0x03, 0x00, 0, 0, 0, 0]), Some((3, [0; 4])));
        // mode=0 on connection 2 → result 1 = success
        let (mode, v) = parse_upgrade_status(&[0x00, 0x00, 0x01, 0x00, 0x00, 0x00]).unwrap();
        assert_eq!((mode, i32::from_le_bytes(v)), (0, RESULT_SUCCESS));
        assert_eq!(parse_upgrade_status(&[0x00, 0x00]), None);
    }

    /// OpenFileAsk announces the whole .bin size while only the tar.gz is streamed.
    #[test]
    fn declared_size_is_whole_bin() {
        let xml = b"<FirmwareInfo><Version>1.2.3.4</Version><Script>upgrade.sh</Script></FirmwareInfo>";
        let mut bin = b"HDPLAYER".to_vec();
        bin.extend_from_slice(&[0u8; 16]);
        bin.extend_from_slice(&(xml.len() as u32).to_le_bytes());
        bin.extend_from_slice(xml);
        bin.extend_from_slice(&[0x1f, 0x8b, 0x08, 0x00, 1, 2, 3]);
        let fw = parse_bin(&bin).unwrap();
        assert_eq!(fw.payload, vec![0x1f, 0x8b, 0x08, 0x00, 1, 2, 3]);
        assert_eq!(fw.declared_size, bin.len() as u64);
    }

    /// The old buggy encoding (4-byte payload) is exactly what the device rejects.
    /// This documents the failure so nobody reintroduces a payload here.
    #[test]
    fn close_file_with_payload_is_the_rejected_encoding() {
        let bad = build_packet(CMD_CLOSE_FILE, &0u32.to_le_bytes());
        let total = u16::from_le_bytes([bad[0], bad[1]]);
        assert_eq!(total, 8, "a 4-byte payload yields total_length=8 — the value the device silently drops");
    }
}
