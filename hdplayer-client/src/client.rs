//! TCP client for communicating with Huidu BoxPlayer devices.
//!
//! Implements the full SDK XML command protocol plus file transfer.

use std::net::Ipv4Addr;
use std::str::FromStr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::Arc;
use tracing::{debug, info, warn};


use crate::command;
use crate::error::{Error, Result};
use crate::protocol::{
    Command, Packet,
    SDK_CLIENT_VERSION, sdk_service_ask_payload, sdk_cmd_ask_payload, parse_sdk_cmd_payload,
};
use crate::transfer::FileTransfer;
use crate::xml;

/// Default TCP port for Huidu BoxPlayer — the BoxStream protocol on port 9527.
/// Confirmed from Huidu.pcapng: HDPlayer.exe connects to port 9527, not 10001.
pub const DEFAULT_PORT: u16 = huidu_protocol::packet::BOX_PLAYER_PORT;

/// SDK protocol version sent in SdkServiceAsk (confirmed from HCatNet.dll analysis).
pub const SDK_VERSION: u32 = huidu_protocol::packet::SDK_CLIENT_VERSION;

/// Heartbeat interval.
pub const HEARTBEAT_INTERVAL: Duration = huidu_protocol::packet::HEARTBEAT_INTERVAL;

/// Response timeout for SDK commands.
pub const CMD_TIMEOUT: Duration = Duration::from_secs(15);

/// Information returned by the full device status batch request.
#[derive(Debug, Clone, Default)]
pub struct DeviceDetails {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    pub firmware_version: String,
    pub fpga_version: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub storage_total: u64,
    pub storage_free: u64,
    pub ip_address: String,
    pub mac_address: String,
    pub dhcp: bool,
    pub volume: u8,
    pub brightness: u8,
    pub rotation: u32,
    pub admin_mode: bool,
    pub current_program_guid: Option<String>,
    /// Play status: 0 = stopped, 1 = playing (from GetPlayStatus).
    pub play_status: u8,
    /// Device time string from GetTimeInfo (e.g. "2026-02-23 05:01:32").
    pub device_time: String,
    /// NTP server list from GetTimeInfo.
    pub ntp_server: String,
    /// Screen-on scheduling enabled (from GetSwitchTime).
    pub switch_time_on: bool,
    /// Device locker feature enabled (from GetDeviceLockerEnable).
    pub locker_enabled: bool,
    /// Boot logo present on device (from GetBootLogo).
    pub boot_logo_exists: bool,
    pub raw_xml: String,
}

/// Program metadata returned by GetAllProgram.
#[derive(Debug, Clone)]
pub struct ProgramInfo {
    pub guid: String,
    pub name: String,
    pub program_type: String,
    pub is_current: bool,
}

/// File entry returned by GetFileChecklist.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub md5: String,
    pub size: u64,
}

/// Ethernet configuration.
#[derive(Debug, Clone)]
pub struct EthConfig {
    pub dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
}

/// Client for a single connected Huidu device.
pub struct Client {
    stream: Arc<Mutex<TcpStream>>,
    client_guid: String,
    read_buf: Vec<u8>,
    /// True when using the legacy port-10001 SdkService/SdkCmd protocol instead of BoxStream.
    use_legacy: bool,
    /// Session token from UDP registration (cmd=0x0005), used as BoxStreamInit payload.
    /// Devices with newer firmware reject BoxStreamInit [0,0,0,0] and require the token.
    session_token: [u8; 4],
    /// Port 9528 management connection — kept alive to maintain authorization for port 9527.
    ///
    /// Real HDPlayer always establishes a port 9528 TCP session (ConnectReq → ConnectAck →
    /// ClientInfoReq → ClientInfoAck handshake) before opening port 9527.  The device tracks
    /// authorized controllers per port-9528 connection; dropping it causes BoxStreamInit on
    /// port 9527 to be rejected with FIN.
    #[allow(dead_code)] // intentionally held alive; value is never read back
    mgmt_stream: Option<TcpStream>,
    /// Target host IP/hostname (stored for use as fallback ip_address in DeviceDetails).
    host: String,
    /// Firmware version string from port-9528 VersionResp (e.g. "7.4.59.0").
    /// Used as fallback when legacy TCP query returns empty.
    mgmt_firmware: String,
    /// Device info captured from UDP registration (cmd=0x0004/0x0005).
    /// Used as fallback for name, MAC, screen size when TCP queries return empty.
    udp_device_info: Option<huidu_protocol::DeviceInfo>,
}

// ── Port 9528 management login helpers ───────────────────────────────────────
//
// Real HDPlayer establishes a port 9528 TCP session and authenticates with a
// client-info CSV string BEFORE opening port 9527.  The device uses this to
// track "registered controllers"; any BoxStreamInit received on port 9527 from
// a client that has not completed the port 9528 handshake is rejected with FIN.
//
// Confirmed from Upgrade Huidu.pcapng (frame-by-frame analysis):
//   t=23.264 s : PC → port 9528 ConnectReq (0x000B)
//   t=23.264 s : ConnectAck (0x000C) received
//   t=23.264 s : ClientInfoReq (0x0410) sent
//   t=23.265 s : ClientInfoAck (0x0411) received
//   ...
//   t=195.873 s: PC → port 9527 TCP SYN (device was rebooting — FIN expected)
//
// Sequence:
//   PC→Dev: ConnectReq  (0x000B) [u32 LE 0x01000007]
//   Dev→PC: ConnectAck  (0x000C) [version echo]
//   PC→Dev: ClientInfoReq (0x0410) [OS,App,User,Host,...\0]
//   Dev→PC: ClientInfoAck (0x0411) [u16 LE 0]
//   PC→Dev: NullCapQuery  (0x0053) []
//   Dev→PC: NullCapResp   (0x0054) [u32 LE 0]
//   PC→Dev: CapQuery      (0x040A) []
//   Dev→PC: CapResp       (0x040B) [u8 0]

/// Build a raw port-9528 framed packet.
/// Wire format: `[u16 LE total][u16 LE cmd][payload]`
fn mgmt_build_packet(cmd: u16, payload: &[u8]) -> Vec<u8> {
    let total = (4u32 + payload.len() as u32) as u16;
    let mut pkt = Vec::with_capacity(total as usize);
    pkt.extend_from_slice(&total.to_le_bytes());
    pkt.extend_from_slice(&cmd.to_le_bytes());
    pkt.extend_from_slice(payload);
    pkt
}

/// Read the next complete framed packet from a raw TcpStream.
async fn mgmt_recv_packet(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
) -> Result<(u16, Vec<u8>)> {
    loop {
        if buf.len() >= 4 {
            let total = u16::from_le_bytes([buf[0], buf[1]]) as usize;
            if total < 4 {
                return Err(Error::Protocol(format!(
                    "port 9528: bad packet length {total}"
                )));
            }
            if buf.len() >= total {
                let cmd = u16::from_le_bytes([buf[2], buf[3]]);
                let payload = buf[4..total].to_vec();
                buf.drain(..total);
                return Ok((cmd, payload));
            }
        }
        let mut tmp = [0u8; 4096];
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(Error::Connection(
                "port 9528: connection closed by device".into(),
            ));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

/// Build the null-terminated CSV payload for ClientInfoReq (0x0410).
///
/// Format confirmed from Upgrade Huidu.pcapng frame 1423:
/// `OS,App,User,Hostname,,,_,YYYY-MM-DD_HH:MM:SS,<net>,<uuid>,YYYY/MM/DD HH:MM:SS\0`
fn mgmt_build_client_info() -> Vec<u8> {
    use chrono::Local;
    use uuid::Uuid;
    let now = Local::now();
    let date1 = now.format("%Y-%m-%d_%H:%M:%S").to_string();
    let date2 = now.format("%Y/%m/%d %H:%M:%S").to_string();
    let username = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".to_string());
    let hostname = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "HDPLAYER".to_string());
    let session_id = Uuid::new_v4().to_string();
    let csv = format!(
        "Windows,HDPlayer,{},{},,,_,{},Ethernet 00-00-00-00-00-00,{},{}",
        username, hostname, date1, session_id, date2,
    );
    let mut bytes = csv.into_bytes();
    bytes.push(0); // null terminator
    bytes
}

/// Connect to port 9528 and complete the management login handshake.
///
/// Returns `(TcpStream, firmware_version)`.  The `TcpStream` MUST be kept
/// alive for the duration of the port 9527 session — the device tracks
/// authorized controllers per port-9528 connection and rejects BoxStreamInit
/// when no such session is active.  The firmware version string (e.g.
/// "7.4.59.0") is parsed from the VersionResp packet.
async fn mgmt_login(host: &str) -> Result<(TcpStream, String)> {
    const MGMT_PORT: u16 = 9528;
    const CONNECT_VERSION: u32 = 0x01000007;

    let addr = format!("{host}:{MGMT_PORT}");
    info!("Port 9528: connecting for management login…");
    let mut stream = TcpStream::connect(&addr)
        .await
        .map_err(|e| Error::Connection(format!("TCP connect to {addr}: {e}")))?;
    stream.set_nodelay(true)?;
    let mut buf = Vec::with_capacity(1024);
    let mut firmware_version = String::new();

    // Step 1-2: ConnectReq → ConnectAck
    stream
        .write_all(&mgmt_build_packet(0x000b, &CONNECT_VERSION.to_le_bytes()))
        .await?;
    stream.flush().await?;
    let (cmd, _) = tokio::time::timeout(
        Duration::from_secs(10),
        mgmt_recv_packet(&mut stream, &mut buf),
    )
    .await
    .map_err(|_| Error::Timeout)??;
    if cmd != 0x000c {
        return Err(Error::Protocol(format!(
            "port 9528: expected ConnectAck (0x000c), got 0x{cmd:04x}"
        )));
    }
    debug!("Port 9528 ConnectAck ok");

    // Step 3-4: ClientInfoReq → ClientInfoAck
    let info = mgmt_build_client_info();
    stream.write_all(&mgmt_build_packet(0x0410, &info)).await?;
    stream.flush().await?;
    let (cmd, _) = tokio::time::timeout(
        Duration::from_secs(10),
        mgmt_recv_packet(&mut stream, &mut buf),
    )
    .await
    .map_err(|_| Error::Timeout)??;
    if cmd != 0x0411 {
        return Err(Error::Protocol(format!(
            "port 9528: expected ClientInfoAck (0x0411), got 0x{cmd:04x}"
        )));
    }
    info!("Port 9528 management login authorised (ClientInfoAck ok)");

    // Step 5-6: NullCapQuery → NullCapResp
    stream.write_all(&mgmt_build_packet(0x0053, &[])).await?;
    stream.flush().await?;
    match tokio::time::timeout(
        Duration::from_secs(10),
        mgmt_recv_packet(&mut stream, &mut buf),
    )
    .await
    {
        Ok(Ok((0x0054, _))) => debug!("Port 9528 NullCapResp ok"),
        Ok(Ok((c, _))) => warn!("Port 9528: expected NullCapResp (0x0054), got 0x{c:04x}"),
        Ok(Err(e)) => warn!("Port 9528 NullCapResp error: {e}"),
        Err(_) => warn!("Port 9528 NullCapResp timeout"),
    }

    // Step 7-8: CapQuery → CapResp
    stream.write_all(&mgmt_build_packet(0x040a, &[])).await?;
    stream.flush().await?;
    match tokio::time::timeout(
        Duration::from_secs(10),
        mgmt_recv_packet(&mut stream, &mut buf),
    )
    .await
    {
        Ok(Ok((0x040b, _))) => debug!("Port 9528 CapResp ok"),
        Ok(Ok((c, _))) => warn!("Port 9528: expected CapResp (0x040b), got 0x{c:04x}"),
        Ok(Err(e)) => warn!("Port 9528 CapResp error: {e}"),
        Err(_) => warn!("Port 9528 CapResp timeout"),
    }

    // Step 9-10: VersionQuery (0x0055) → VersionResp (0x0056)
    //
    // Confirmed from Upgrade Huidu.pcapng frames 1429-1430: after CapResp the real
    // HDPlayer sends cmd=0x0055 [0x01, 0x00] and the device replies with cmd=0x0056
    // [0x01, 0x00, fw_major, fw_minor, fw_patch, fw_build].
    //
    // Frame 1429: PC→Dev  06 00 55 00 01 00
    // Frame 1430: Dev→PC  0a 00 56 00 01 00 07 04 3b 00  (firmware 7.4.59.0)
    stream.write_all(&mgmt_build_packet(0x0055, &[0x01, 0x00])).await?;
    stream.flush().await?;
    match tokio::time::timeout(
        Duration::from_secs(10),
        mgmt_recv_packet(&mut stream, &mut buf),
    )
    .await
    {
        Ok(Ok((0x0056, ref p))) if p.len() >= 6 => {
            firmware_version = format!("{}.{}.{}.{}", p[2], p[3], p[4], p[5]);
            info!("Port 9528 VersionResp: firmware={firmware_version}");
        }
        Ok(Ok((0x0056, _))) => debug!("Port 9528 VersionResp ok (short payload)"),
        Ok(Ok((c, _))) => warn!("Port 9528: expected VersionResp (0x0056), got 0x{c:04x}"),
        Ok(Err(e)) => warn!("Port 9528 VersionResp error: {e}"),
        Err(_) => warn!("Port 9528 VersionResp timeout"),
    }

    Ok((stream, firmware_version))
}

/// Query the firmware version from a device's port-9528 management port.
///
/// Faster than a full BoxStream connection and works even when port 9527
/// rejects BoxStreamInit (e.g. firmware 7.4.59.0).
pub async fn firmware_version_via_9528(host: &str) -> Result<String> {
    let (_stream, version) = mgmt_login(host).await?;
    Ok(version)
}

impl Client {
    /// Connect to a Huidu BoxPlayer at the given host and port.
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        // UDP registration MUST happen before TCP BoxStream connection.
        //
        // Confirmed from hdplayer_real.pcapng: real HDPlayer performs a UDP handshake on
        // port 9526 before connecting TCP.  Without this, the device accepts the TCP
        // connection, sends TcpHeartbeatAnswer, but then closes the connection (FIN) after
        // receiving BoxStreamInit — regardless of timing.
        //
        // The device sends unicast UDP from port 9527 TO port 9526 on the controller.
        // We must respond with cmd=0x0003 (and optionally cmd=0x0006/0x0341) to be
        // "registered" as an authorised controller.
        // Perform UDP registration and capture the session token.
        // Newer firmware (≥ ~7.2) requires the session token from cmd=0x0005 to be sent
        // as the BoxStreamInit payload; older devices accept [0,0,0,0].
        let mut session_token = [0u8; 4];
        let mut udp_device_info: Option<huidu_protocol::DeviceInfo> = None;
        if let Ok(ip) = Ipv4Addr::from_str(host) {
            match huidu_protocol::udp_register(ip, Duration::from_secs(12)).await {
                Ok((Some(token), dev)) => {
                    info!("UDP registration complete — session token: {:02x?}", token);
                    session_token = token;
                    udp_device_info = dev;
                }
                Ok((None, dev)) => {
                    info!("UDP registration complete (no session token)");
                    udp_device_info = dev;
                }
                Err(e) => warn!("UDP registration failed ({e}) — attempting TCP anyway"),
            }
            // Wait for the device to process our cmd=0x0006 registration before TCP.
            //
            // From Huidu.pcapng analysis: in a working session the PC was already
            // registered (cmd=0x0006 sent in a previous session), so TcpHeartbeatAnswer
            // arrived immediately (<1ms) as a connection greeting.  When we register
            // fresh and connect TCP 1ms later, the device hasn't finished processing the
            // registration yet — TcpHeartbeatAnswer only arrives ~6s later (periodic
            // heartbeat, not a greeting), and BoxStreamInit is rejected with FIN.
            //
            // Giving the device 5s after cmd=0x0006 before TCP connect lets it finish
            // registration so it recognises us as an authorised controller.
            info!("Waiting 5s for device to process UDP registration...");
            tokio::time::sleep(Duration::from_secs(5)).await;
        } else {
            warn!("Host '{host}' is not an IPv4 address — skipping UDP registration");
        }

        // Port 9528 management login — MUST happen before port 9527 BoxStreamInit.
        //
        // Real HDPlayer authenticates on port 9528 before connecting to port 9527.
        // Without the prior ClientInfoAck handshake the device rejects BoxStreamInit
        // with a FIN regardless of timing or UDP registration state.
        let (mgmt_stream, mgmt_firmware) = match mgmt_login(host).await {
            Ok((s, fw)) => {
                info!("Port 9528 management login complete — device will accept BoxStreamInit");
                (Some(s), fw)
            }
            Err(e) => {
                warn!("Port 9528 management login failed ({e}) — attempting port 9527 anyway");
                (None, String::new())
            }
        };

        let addr = format!("{host}:{port}");
        info!("Connecting to {addr}");
        let stream = TcpStream::connect(&addr).await
            .map_err(|e| Error::Connection(format!("TCP connect to {addr}: {e}")))?;
        stream.set_nodelay(true)?;
        info!("Connected to {addr}");

        let client = Self {
            stream: Arc::new(Mutex::new(stream)),
            // BoxStream protocol uses the literal "##GUID" placeholder (not a real UUID).
            // Confirmed from Huidu.pcapng: all SDK requests and responses use guid="##GUID".
            client_guid: "##GUID".to_string(),
            read_buf: Vec::with_capacity(65536),
            use_legacy: false,
            session_token,
            mgmt_stream,
            host: host.to_string(),
            mgmt_firmware,
            udp_device_info,
        };

        // BoxStream connect sequence (confirmed from More Huidu.pcapng):
        //
        //  1. TCP connect to port 9527
        //  2. PC → Device: BoxStreamInit (0x0200) — sent IMMEDIATELY as first data packet
        //  3. Device → PC: BoxStreamInitAck (0x0201)
        //
        // IMPORTANT: The device has a short acceptance window after TCP connect.  If
        // BoxStreamInit does not arrive within ~2–3 seconds the device closes the
        // connection (FIN) on the next periodic heartbeat tick (~5–6 s).
        //
        // Earlier tests showed RST when BoxStreamInit arrived immediately, but those
        // tests were run WITHOUT the port 9528 management login.  With port 9528 login
        // completed first, immediate BoxStreamInit is accepted.
        //
        // Historical note — these all failed when port 9528 login was NOT done:
        //   BoxStreamInit before heartbeat (no port-9528) → RST
        //   BoxStreamInit after heartbeat  (no port-9528) → FIN
        //   BoxStreamInit after heartbeat  (with port-9528, 5.9 s delay) → FIN
        //
        // Brief settle delay to ensure the TCP connection is fully established on
        // both ends before the first data write.
        tokio::time::sleep(Duration::from_millis(200)).await;
        info!("TCP connection settled — ready for BoxStreamInit");

        Ok(client)
    }

    /// Connect using the default port (9527).
    pub async fn connect_default(host: &str) -> Result<Self> {
        Self::connect(host, DEFAULT_PORT).await
    }

    /// Connect using the legacy port-10001 SdkService/SdkCmd protocol.
    ///
    /// Older Huidu devices (and some C-series) use this protocol instead of BoxStream.
    /// The handshake is client-initiated:
    ///  1. Client → Device: SdkServiceAsk [u32 LE version]
    ///  2. Device → Client: SdkServiceAnswer [u32 LE device_version]
    ///
    /// Some devices (e.g. C15 with SDK 6) reject SdkCmdAsk with kVersionNotSupport (22)
    /// if the client claims a version higher than the device's own.  We handle this with
    /// a two-pass strategy: probe with our full version, then reconnect with
    /// min(SDK_CLIENT_VERSION, device_version) if needed.
    ///
    /// XML is then sent as SdkCmdAsk and received as SdkCmdAnswer, both with
    /// an 8-byte `[u32 total][u32 chunk_index]` framing header before the XML bytes.
    pub async fn connect_legacy(host: &str, port: u16) -> Result<Self> {
        // Try port 9528 login to capture firmware version (non-fatal).
        // Older C-series devices may not have port 9528 — connection will fail quickly.
        let mgmt_firmware = match mgmt_login(host).await {
            Ok((_, fw)) => {
                info!("Port 9528 firmware version on legacy path: {fw}");
                fw
            }
            Err(e) => {
                debug!("Port 9528 login failed on legacy path ({e}) — no firmware fallback");
                String::new()
            }
        };

        // Probe to discover the device's SDK version.
        let device_version = Self::probe_legacy_version(host, port).await?;
        let use_version = device_version.min(SDK_CLIENT_VERSION);

        let addr = format!("{host}:{port}");
        info!(
            "Connecting (legacy SDK) to {addr} with version 0x{use_version:08X} \
             (device=0x{device_version:08X})"
        );
        let stream = TcpStream::connect(&addr).await
            .map_err(|e| Error::Connection(format!("TCP connect to {addr}: {e}")))?;
        stream.set_nodelay(true)?;

        let mut client = Self {
            stream: Arc::new(Mutex::new(stream)),
            // Legacy port-10001 protocol also uses "##GUID" as the literal GUID placeholder.
            // Confirmed from More Huidu.pcapng frame 189: real HDPlayer uses ##GUID on port 10001.
            client_guid: "##GUID".to_string(),
            read_buf: Vec::with_capacity(65536),
            use_legacy: true,
            session_token: [0u8; 4], // port 10001 doesn't use BoxStreamInit
            mgmt_stream: None,       // legacy protocol doesn't hold port 9528 open
            host: host.to_string(),
            mgmt_firmware,
            udp_device_info: None,   // no UDP registration on legacy path
        };

        info!("Sending SdkServiceAsk (version 0x{use_version:08X})...");
        let payload = sdk_service_ask_payload(use_version);
        client.send_packet(&Packet::new(Command::SdkServiceAsk, payload)).await?;

        // Wait for SdkServiceAnswer
        loop {
            match client.recv_with_timeout(Duration::from_secs(10)).await? {
                pkt if pkt.command == Command::SdkServiceAnswer => {
                    break;
                }
                pkt if pkt.command == Command::TcpHeartbeatAnswer => {
                    debug!("TcpHeartbeatAnswer during legacy handshake — ignoring");
                }
                pkt if pkt.command == Command::SdkErrorAnswer => {
                    let code = if pkt.payload.len() >= 2 {
                        u16::from_le_bytes([pkt.payload[0], pkt.payload[1]])
                    } else { 0 };
                    return Err(Error::Protocol(format!(
                        "Device rejected SDK version at handshake (SdkErrorAnswer code={code})"
                    )));
                }
                pkt => {
                    return Err(Error::Protocol(format!(
                        "Expected SdkServiceAnswer, got {:?}", pkt.command
                    )));
                }
            }
        }

        // GetIFVersion is a mandatory capability-negotiation step (confirmed from
        // More Huidu.pcapng frame 189: always the first SdkCmdAsk after SdkServiceAnswer).
        //
        // The device's response contains the session GUID in <sdk guid="...">  which MUST
        // be used for all subsequent SdkCmdAsk calls.  Using ##GUID or any other value
        // returns kInvalidGUID (error 45).
        info!("Sending GetIFVersion capability negotiation...");
        let xml_str = xml::sdk_request(&client.client_guid, "GetIFVersion", &command::get_if_version());
        let response = client.send_xml_request_legacy(&xml_str).await?;
        // Extract session GUID assigned by the device.
        if let Some(guid) = xml::extract_guid(&response) {
            info!("Device assigned session GUID: {}", guid);
            client.client_guid = guid.to_string();
        }
        let if_ver = response.find("<version ")
            .and_then(|p| xml::get_attr(&response[p..], "value"))
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(0);
        info!("Legacy SDK connected (interface version {})", if_ver);
        Ok(client)
    }

    /// Open a short-lived connection to discover the device's SDK version, then close it.
    async fn probe_legacy_version(host: &str, port: u16) -> Result<u32> {
        let addr = format!("{host}:{port}");
        let stream = TcpStream::connect(&addr).await
            .map_err(|e| Error::Connection(format!("TCP connect to {addr}: {e}")))?;
        stream.set_nodelay(true)?;

        let mut probe = Self {
            stream: Arc::new(Mutex::new(stream)),
            client_guid: String::new(),
            read_buf: Vec::with_capacity(256),
            use_legacy: true,
            session_token: [0u8; 4],
            mgmt_stream: None,
            host: String::new(),
            mgmt_firmware: String::new(),
            udp_device_info: None,
        };

        let payload = sdk_service_ask_payload(SDK_CLIENT_VERSION);
        probe.send_packet(&Packet::new(Command::SdkServiceAsk, payload)).await?;

        let device_version = loop {
            match probe.recv_with_timeout(Duration::from_secs(5)).await? {
                pkt if pkt.command == Command::SdkServiceAnswer => {
                    let ver = if pkt.payload.len() >= 4 {
                        u32::from_le_bytes([
                            pkt.payload[0], pkt.payload[1],
                            pkt.payload[2], pkt.payload[3],
                        ])
                    } else { 0 };
                    break ver;
                }
                pkt if pkt.command == Command::TcpHeartbeatAnswer => {
                    debug!("TcpHeartbeatAnswer during probe — ignoring");
                }
                pkt if pkt.command == Command::SdkErrorAnswer => {
                    // Device rejected version probe — default to SDK_CLIENT_VERSION
                    break SDK_CLIENT_VERSION;
                }
                _ => {
                    break SDK_CLIENT_VERSION;
                }
            }
        };
        // probe drops here, closing the TCP connection
        info!("Version probe: device version = 0x{device_version:08X}");
        Ok(device_version)
    }

    // ── Internal helpers ─────────────────────────────────────────────────

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("-");
        debug!("Sent {:?} ({} bytes): {}", packet.command, bytes.len(), hex);
        let mut stream = self.stream.lock().await;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Packet> {
        loop {
            // Try to parse from buffer
            if let Some((packet, consumed)) = Packet::from_bytes(&self.read_buf)? {
                self.read_buf.drain(..consumed);
                debug!("Recv {:?} ({} bytes payload)", packet.command, packet.payload.len());
                return Ok(packet);
            }
            // Need more data
            let mut tmp = vec![0u8; 4096];
            let mut stream = self.stream.lock().await;
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(Error::Connection("connection closed by device".into()));
            }
            drop(stream);
            let hex: String = tmp[..n].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("-");
            debug!("Recv raw {} bytes: {}", n, hex);
            self.read_buf.extend_from_slice(&tmp[..n]);
        }
    }

    async fn recv_with_timeout(&mut self, timeout: Duration) -> Result<Packet> {
        tokio::time::timeout(timeout, self.recv_packet())
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Initiate a BoxStream session (port 9527 protocol).
    ///
    /// Sends `BoxStreamInit` with the session token as payload and waits for
    /// `BoxStreamInitAck`.
    ///
    /// Payload layout (4 bytes = 8-byte total packet):
    ///   bytes 0-3: session token from UDP cmd=0x0005 (or `[0,0,0,0]` for older devices).
    ///
    /// Confirmed from DLL analysis (NetIOServices.dll 0x044700): the 8-byte BoxStreamInit
    /// packet is `[len=0x0008][cmd=0x0200][channel_u16][0x0000]` where `channel` is the
    /// value from the XML `[channel]` tag / UDP registration token.  Devices running
    /// firmware ≥ ~7.2 reject `[0,0,0,0]` and require the token obtained from UDP
    /// cmd=0x0005 during registration.
    async fn box_stream_init(&mut self) -> Result<()> {
        let token = self.session_token;
        info!("Sending BoxStreamInit token={:02x?}...", token);
        let pkt = Packet::new(Command::BoxStreamInit, token.to_vec());
        self.send_packet(&pkt).await?;
        info!("BoxStreamInit sent — waiting for BoxStreamInitAck (0x0201)");
        loop {
            let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
            info!("box_stream_init recv: {:?} payload_len={}", resp.command, resp.payload.len());
            match resp.command {
                Command::BoxStreamInitAck => {
                    info!("BoxStream session established");
                    return Ok(());
                }
                // Device may send periodic keepalives while we wait.
                Command::TcpHeartbeatAnswer => {
                    info!("TcpHeartbeatAnswer while waiting for BoxStreamInitAck — sending TcpHeartbeatAsk");
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "Expected BoxStreamInitAck, got {other:?}"
                    )));
                }
            }
        }
    }

    /// Wait for a specific command, discarding heartbeat pings in between.
    async fn expect_cmd(&mut self, expected: Command, timeout: Duration) -> Result<Packet> {
        loop {
            let pkt = self.recv_with_timeout(timeout).await?;
            if pkt.command == expected {
                return Ok(pkt);
            }
            match pkt.command {
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                // Device sends TcpHeartbeatAnswer periodically; ignore mid-exchange.
                Command::TcpHeartbeatAnswer => {
                    debug!("Received TcpHeartbeatAnswer mid-exchange — ignoring");
                }
                other => {
                    warn!("Expected {:?}, got {:?} — ignoring", expected, other);
                }
            }
        }
    }

    /// Send XML using the legacy SdkCmdAsk/SdkCmdAnswer protocol (port 10001).
    async fn send_xml_request_legacy(&mut self, xml_str: &str) -> Result<String> {
        debug!("Legacy XML request: {}", xml_str);
        // 8-byte framing header confirmed from More Huidu.pcapng frame 189:
        //   u32[0] = total XML length, u32[1] = chunk index (0 for single-chunk)
        let payload = sdk_cmd_ask_payload(xml_str);
        self.send_packet(&Packet::new(Command::SdkCmdAsk, payload)).await?;

        loop {
            let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
            match resp.command {
                Command::SdkCmdAnswer => {
                    let xml_bytes = parse_sdk_cmd_payload(&resp.payload)
                        .ok_or_else(|| Error::Protocol(
                            format!("SdkCmdAnswer payload too short ({} bytes)", resp.payload.len())
                        ))?;
                    return Ok(String::from_utf8_lossy(xml_bytes).into_owned());
                }
                Command::SdkErrorAnswer => {
                    let code = if resp.payload.len() >= 2 {
                        u16::from_le_bytes([resp.payload[0], resp.payload[1]])
                    } else {
                        0
                    };
                    let code_name = match code {
                        3  => "kVersionTooLow",
                        4  => "kDeviceOccupa",
                        20 => "kXmlCmdTooLong",
                        21 => "kInvalidXmlIndex",
                        22 => "kParseXmlFailed",
                        23 => "kInvalidMethod",
                        44 => "kUnsupportMethod",
                        45 => "kInvalidGUID",
                        _  => "unknown",
                    };
                    warn!("SdkErrorAnswer code={} ({}) for XML: {}", code, code_name, xml_str);
                    return Err(Error::Protocol(format!(
                        "SdkErrorAnswer code={} ({})", code, code_name
                    )));
                }
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                Command::TcpHeartbeatAnswer => {
                    debug!("TcpHeartbeatAnswer mid-legacy-exchange — ignoring");
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "Expected SdkCmdAnswer, got {:?}", other
                    )));
                }
            }
        }
    }

    /// Send XML over BoxStream and receive the response XML.
    ///
    /// Full 10-step handshake confirmed from Huidu.pcapng:
    ///
    /// **Request delivery:**
    /// 1. PC→Dev: BoxStreamData `[0x00,0x00] + xml`
    /// 2. Dev→PC: BoxStreamRxAck `[0x00,0x00]`
    /// 3. PC→Dev: BoxStreamTxAck `[0x00,0x00]`
    /// 4. Dev→PC: BoxStreamFinalAck `[0x00,0x00]`
    ///
    /// **Response delivery:**
    /// 5. Dev→PC: BoxStreamInit `[0x01,0x00,0x00,0x00]`  ← "response ready"
    /// 6. PC→Dev: BoxStreamInitAck `[0x00,0x00]`
    /// 7. Dev→PC: BoxStreamData `[0x01,0x00] + xml`      ← actual response
    /// 8. PC→Dev: BoxStreamRxAck `[0x00,0x00]`
    /// 9. Dev→PC: BoxStreamTxAck `[0x01,0x00]`
    /// 10. PC→Dev: BoxStreamFinalAck `[0x00,0x00]`
    async fn send_xml_request(&mut self, xml_str: &str) -> Result<String> {
        if self.use_legacy {
            return self.send_xml_request_legacy(xml_str).await;
        }
        use crate::protocol::{box_stream_data_request, parse_box_stream_data};

        // Step 0 (pre-request): BoxStreamInit → BoxStreamInitAck handshake.
        // Required before EVERY XML exchange (confirmed from More Huidu.pcapng:
        // every subsequent call starts with BoxStreamInit [0,0,0,0] → BoxStreamInitAck).
        self.box_stream_init().await?;

        // Step 1: send BoxStreamData with [0x00,0x00] + xml
        let payload = box_stream_data_request(xml_str);
        self.send_packet(&Packet::new(Command::BoxStreamData, payload)).await?;

        // Step 2: wait for BoxStreamRxAck
        self.expect_cmd(Command::BoxStreamRxAck, CMD_TIMEOUT).await?;

        // Step 3: send BoxStreamTxAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamTxAck, vec![0x00, 0x00])).await?;

        // Step 4: wait for BoxStreamFinalAck
        self.expect_cmd(Command::BoxStreamFinalAck, CMD_TIMEOUT).await?;

        // Step 5: wait for BoxStreamInit with payload [0x01,0x00,0x00,0x00] ("response ready")
        loop {
            let pkt = self.recv_with_timeout(CMD_TIMEOUT).await?;
            match pkt.command {
                Command::BoxStreamInit => {
                    // payload[0] == 0x01 means device has a response queued
                    if pkt.payload.first() == Some(&0x01) {
                        break;
                    }
                    warn!("BoxStreamInit with unexpected payload {:?}", pkt.payload);
                }
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                other => {
                    warn!("Unexpected {:?} while waiting for BoxStreamInit(ready)", other);
                }
            }
        }

        // Step 6: send BoxStreamInitAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamInitAck, vec![0x00, 0x00])).await?;

        // Step 7: wait for BoxStreamData response
        let resp = self.expect_cmd(Command::BoxStreamData, CMD_TIMEOUT).await?;
        let xml_bytes = parse_box_stream_data(&resp.payload)
            .ok_or_else(|| Error::Protocol(
                format!("BoxStreamData response too short: {} bytes", resp.payload.len())
            ))?;
        let response_xml = String::from_utf8_lossy(xml_bytes).into_owned();
        debug!("BoxStream response: {} bytes", response_xml.len());

        // Step 8: send BoxStreamRxAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamRxAck, vec![0x00, 0x00])).await?;

        // Step 9: wait for BoxStreamTxAck from device
        self.expect_cmd(Command::BoxStreamTxAck, CMD_TIMEOUT).await?;

        // Step 10: send BoxStreamFinalAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamFinalAck, vec![0x00, 0x00])).await?;

        Ok(response_xml)
    }

    /// Send a single SDK XML method call and check the result.
    async fn sdk_cmd(&mut self, method: &str, body: &str) -> Result<String> {
        let xml_str = xml::sdk_request(&self.client_guid, method, body);
        let response = self.send_xml_request(&xml_str).await?;
        debug!("SDK response for {method}: {} bytes", response.len());
        xml::parse_result(&response)?;
        Ok(response)
    }

    /// Send a heartbeat packet.
    pub async fn heartbeat(&mut self) -> Result<()> {
        let pkt = Packet::heartbeat();
        self.send_packet(&pkt).await
    }

    // ── Device Info ──────────────────────────────────────────────────────

    /// Get full device status via a 23-method batch request.
    ///
    /// Sends the same methods HDPlayer.exe requests on initial connection
    /// (confirmed from Huidu.pcapng and More Huidu.pcapng):
    /// GetDeviceName, GetFirewareVersion, GetKeyDefine, GetPlayStatus,
    /// GetSystemVolume, GetBootLogo, GetSensorInfo, GetGPSInfo,
    /// GetCurrentLuminance, GetCurrentTemperature, GetCurrentHumity,
    /// GetSensorType, GetSwitchTime, GetTimeInfo, GetLuminancePloy,
    /// GetScreenInfo, GetLicense, GetEth0Info, GetWifiInfo, GetPppoeInfo,
    /// GetDeviceInfo, GetDataSourceInfo, GetRelay.
    ///
    /// Methods not supported by the device (e.g. GetSensorType, GetDeviceInfo,
    /// GetDataSourceInfo) return kUnsupportMethod and are silently skipped.
    const INFO_METHODS: &'static [(&'static str, &'static str)] = &[
        ("GetDeviceName",           ""),
        ("GetFirewareVersion",      ""),
        ("GetKeyDefine",            ""),
        ("GetPlayStatus",           ""),
        ("GetSystemVolume",         ""),
        ("GetBootLogo",             ""),
        ("GetSensorInfo",           ""),
        ("GetGPSInfo",              ""),
        ("GetCurrentLuminance",     ""),
        ("GetCurrentTemperature",   ""),
        ("GetCurrentHumity",        ""),  // note: protocol typo "Humity"
        ("GetSensorType",           ""),
        ("GetSwitchTime",           ""),
        ("GetTimeInfo",             ""),
        ("GetLuminancePloy",        ""),
        ("GetScreenInfo",           ""),
        ("GetLicense",              ""),
        ("GetEth0Info",             ""),
        ("GetWifiInfo",             ""),
        ("GetPppoeInfo",            ""),
        ("GetDeviceInfo",           ""),  // returns kUnsupportMethod on C-series
        ("GetDataSourceInfo",       ""),  // returns kUnsupportMethod on C-series
        ("GetRelay",                ""),
    ];

    /// Send each of the INFO_METHODS individually (legacy port-10001 path) and
    /// concatenate the successful responses.  Failed calls (kUnsupportMethod,
    /// etc.) are silently skipped — the caller's `if let Some(body)` guards
    /// handle missing methods gracefully.
    async fn get_device_info_legacy(&mut self) -> Result<String> {
        let mut combined = String::new();
        for (method, body) in Self::INFO_METHODS {
            match self.sdk_cmd(method, body).await {
                Ok(xml) => combined.push_str(&xml),
                Err(e) => {
                    debug!("Legacy info: {} skipped ({})", method, e);
                }
            }
        }
        Ok(combined)
    }

    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        // Legacy protocol (port 10001) only accepts one <in> per SdkCmdAsk.
        // BoxStream (port 9527) supports a multi-method batch in one request.
        let response = if self.use_legacy {
            self.get_device_info_legacy().await?
        } else {
            use huidu_protocol::xml::sdk_batch_request;
            let request_xml = sdk_batch_request(Self::INFO_METHODS);
            self.send_xml_request(&request_xml).await?
        };
        let mut info = DeviceDetails { raw_xml: response.clone(), ..Default::default() };
        info.device_type = "BoxPlayer".to_string();

        // GetDeviceName → <name value="BoxPlayer"/>
        if let Some(body) = extract_out_body(&response, "GetDeviceName") {
            info.device_name = xml::get_attr(&body, "value")
                .map(xml::xml_unescape).unwrap_or_default();
        }

        // GetFirewareVersion → <app version="7.4.61.0"/><fpga version="6.3.70.0"/>
        if let Some(body) = extract_out_body(&response, "GetFirewareVersion") {
            if let Some(app_pos) = body.find("<app ") {
                info.firmware_version = xml::get_attr(&body[app_pos..], "version")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(fpga_pos) = body.find("<fpga ") {
                info.fpga_version = xml::get_attr(&body[fpga_pos..], "version")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
        }

        // GetPlayStatus → <status value="1"/> (1=playing, 0=stopped)
        if let Some(body) = extract_out_body(&response, "GetPlayStatus") {
            if let Some(p) = body.find("<status ") {
                info.play_status = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }

        // GetSystemVolume → <volume precent="100"/> (note: protocol typo "precent")
        if let Some(body) = extract_out_body(&response, "GetSystemVolume") {
            if let Some(p) = body.find("<volume ") {
                info.volume = xml::get_attr(&body[p..], "precent")
                    .and_then(|v| v.parse().ok()).unwrap_or(100);
            }
        }

        // GetBootLogo → <logo exist="false"/>
        if let Some(body) = extract_out_body(&response, "GetBootLogo") {
            info.boot_logo_exists = xml::get_attr(&body, "exist")
                .map(|v| v == "true")
                .unwrap_or(false);
        }

        // GetCurrentLuminance → <percent value="100"/>
        if let Some(body) = extract_out_body(&response, "GetCurrentLuminance") {
            if let Some(p) = body.find("<percent ") {
                info.brightness = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(100);
            }
        }

        // GetSwitchTime → <open enable="true"/>
        if let Some(body) = extract_out_body(&response, "GetSwitchTime") {
            info.switch_time_on = body.find("<open ")
                .and_then(|p| xml::get_attr(&body[p..], "enable"))
                .map(|v| v == "true")
                .unwrap_or(false);
        }

        // GetTimeInfo → <time value="..."/><server list="..."/>
        if let Some(body) = extract_out_body(&response, "GetTimeInfo") {
            if let Some(p) = body.find("<time ") {
                info.device_time = xml::get_attr(&body[p..], "value")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(p) = body.find("<server ") {
                info.ntp_server = xml::get_attr(&body[p..], "list")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
        }

        // GetScreenInfo → <width value="128"/><height value="128"/>
        //                  <space value="..."/><total value="..."/>
        if let Some(body) = extract_out_body(&response, "GetScreenInfo") {
            if let Some(p) = body.find("<width ") {
                info.screen_width = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<height ") {
                info.screen_height = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<space ") {
                info.storage_free = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<total ") {
                info.storage_total = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<rotation ") {
                info.rotation = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }

        // GetEth0Info → <enable value="true"/><dhcp auto="1"/>
        //               <ip addr="..."/><netmask addr="..."/>
        //               <gateway addr="..."/><dns addr="..."/>
        //               <mac addr="28:32:fd:be:36:40"/>
        if let Some(body) = extract_out_body(&response, "GetEth0Info") {
            if let Some(p) = body.find("<ip ") {
                info.ip_address = xml::get_attr(&body[p..], "addr")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(p) = body.find("<mac ") {
                info.mac_address = xml::get_attr(&body[p..], "addr")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(p) = body.find("<dhcp ") {
                let auto_val = xml::get_attr(&body[p..], "auto").unwrap_or("0");
                info.dhcp = auto_val == "1" || auto_val == "true";
            }
            // Use MAC as device_id — the most stable unique identifier.
            if !info.mac_address.is_empty() {
                info.device_id = info.mac_address.replace(':', "").to_uppercase();
            }
        }

        // Fallback: populate empty fields from alternate data sources.
        //
        // Firmware 7.4.59.0 returns empty self-closing <out method="X"/> for ALL TCP queries
        // on port 10001.  Fill in what we can from UDP registration (cmd=0x0004/0x0005) and
        // port-9528 VersionResp.
        if info.firmware_version.is_empty() && !self.mgmt_firmware.is_empty() {
            info.firmware_version = self.mgmt_firmware.clone();
        }
        if info.ip_address.is_empty() && !self.host.is_empty() {
            info.ip_address = self.host.clone();
        }
        if let Some(ref udp) = self.udp_device_info {
            if info.device_name.is_empty() && !udp.name.is_empty() {
                info.device_name = udp.name.clone();
            }
            if info.screen_width == 0 {
                if let Some(w) = udp.screen_width { info.screen_width = w; }
            }
            if info.screen_height == 0 {
                if let Some(h) = udp.screen_height { info.screen_height = h; }
            }
            if info.mac_address.is_empty() {
                if let Some(ref mac) = udp.mac_address {
                    info.mac_address = mac.clone();
                    info.device_id = mac.replace(':', "").to_uppercase();
                }
            }
            if info.firmware_version.is_empty() {
                if let Some(ref fw) = udp.firmware_version { info.firmware_version = fw.clone(); }
            }
        }

        Ok(info)
    }

    pub async fn get_hardware_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetHardwareInfo", &command::get_hardware_info()).await
    }

    pub async fn get_sdk_version(&mut self) -> Result<u32> {
        let xml = self.sdk_cmd("GetIFVersion", &command::get_if_version()).await?;
        // Response body is `<version value="1000000"/>` (confirmed from More Huidu.pcapng frame 190).
        // Must find the <version> tag first, then read its `value` attribute.
        let v = xml.find("<version ")
            .and_then(|p| xml::get_attr(&xml[p..], "value"))
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(v)
    }

    /// Capture a screenshot. Returns PNG bytes.
    pub async fn screenshot(&mut self) -> Result<Vec<u8>> {
        let xml = self.sdk_cmd("GetScreenshot2", &command::get_screenshot2(0, 0)).await?;
        // Response contains base64-encoded image in <data> tag or attribute
        let data_b64 = xml::get_attr(&xml, "data")
            .or_else(|| xml::get_tag_text(&xml, "data"))
            .unwrap_or("");
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_b64.trim())
            .map_err(|e| Error::InvalidResponse(format!("base64 decode: {e}")))?;
        Ok(bytes)
    }

    // ── Program Management ───────────────────────────────────────────────

    /// Upload a complete program to the device.
    /// `screen_xml` is the `<screen>...</screen>` block (without SDK envelope).
    pub async fn add_program(&mut self, screen_xml: &str) -> Result<()> {
        self.sdk_cmd("AddProgram", &command::add_program(screen_xml)).await?;
        Ok(())
    }

    /// Update an existing program.
    pub async fn update_program(&mut self, screen_xml: &str) -> Result<()> {
        self.sdk_cmd("UpdateProgram", &command::update_program(screen_xml)).await?;
        Ok(())
    }

    /// Delete a program by GUID.
    pub async fn delete_program(&mut self, guid: &str) -> Result<()> {
        self.sdk_cmd("DeleteProgram", &command::delete_program(guid)).await?;
        Ok(())
    }

    /// Switch to a program by GUID.
    pub async fn switch_program(&mut self, guid: &str) -> Result<()> {
        self.sdk_cmd("SwitchProgram", &command::switch_program(guid)).await?;
        Ok(())
    }

    /// Switch to a program by index.
    pub async fn switch_program_index(&mut self, index: u32) -> Result<()> {
        self.sdk_cmd("SwitchProgramIndex", &command::switch_program_index(index)).await?;
        Ok(())
    }

    /// Get all programs on the device.
    pub async fn get_all_programs(&mut self) -> Result<Vec<ProgramInfo>> {
        let xml = self.sdk_cmd("GetAllProgram", &command::get_all_program()).await?;
        let mut programs = Vec::new();
        // Parse <program guid="..." name="..." type="..." current="..."/>
        let mut search = xml.as_str();
        while let Some(start) = search.find("<program ") {
            let end = search[start..].find("/>")
                .map(|e| start + e + 2)
                .or_else(|| search[start..].find("</program>").map(|e| start + e + 10))
                .unwrap_or(search.len());
            let tag = &search[start..end];
            programs.push(ProgramInfo {
                guid: xml::get_attr(tag, "guid").unwrap_or("").to_string(),
                name: xml::get_attr(tag, "name").map(xml::xml_unescape).unwrap_or_default(),
                program_type: xml::get_attr(tag, "type").map(xml::xml_unescape)
                    .unwrap_or_else(|| "normal".to_string()),
                is_current: xml::get_attr(tag, "current")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false),
            });
            search = &search[end.min(search.len())..];
        }
        Ok(programs)
    }

    // ── Brightness / Screen ──────────────────────────────────────────────

    /// Set display brightness (0–100).
    pub async fn set_brightness(&mut self, level: u8) -> Result<()> {
        self.sdk_cmd("SetBrightness", &command::set_brightness(level)).await?;
        Ok(())
    }

    /// Get brightness schedule.
    pub async fn get_luminance_ploy(&mut self) -> Result<String> {
        self.sdk_cmd("GetLuminancePloy", &command::get_luminance_ploy()).await
    }

    /// Set brightness schedule.
    pub async fn set_luminance_ploy(&mut self, entries: &[(u8, u8, u8)]) -> Result<()> {
        self.sdk_cmd("SetLuminancePloy", &command::set_luminance_ploy(entries)).await?;
        Ok(())
    }

    /// Turn the screen on.
    pub async fn screen_on(&mut self) -> Result<()> {
        self.sdk_cmd("OpenScreen", &command::open_screen()).await?;
        Ok(())
    }

    /// Turn the screen off.
    pub async fn screen_off(&mut self) -> Result<()> {
        self.sdk_cmd("CloseScreen", &command::close_screen()).await?;
        Ok(())
    }

    /// Get screen on/off schedule.
    pub async fn get_switch_time(&mut self) -> Result<String> {
        self.sdk_cmd("GetSwitchTime", &command::get_switch_time()).await
    }

    /// Set screen on/off schedule.
    ///
    /// Each entry is `(on_time, off_time, days)` where `days` is a 7-char '0'/'1' string
    /// for Mon–Sun (e.g. `"1111100"` = weekdays only). Pass an empty slice to clear.
    pub async fn set_switch_time(&mut self, entries: &[(&str, &str, &str)]) -> Result<()> {
        self.sdk_cmd("SetSwitchTime", &command::set_switch_time(entries)).await?;
        Ok(())
    }

    // ── Network ──────────────────────────────────────────────────────────

    pub async fn get_eth0_info(&mut self) -> Result<EthConfig> {
        let xml = self.sdk_cmd("GetEth0Info", &command::get_eth0_info()).await?;
        // Real device response (confirmed from Huidu.pcapng):
        //   <enable value="true"/>
        //   <dhcp auto="1"/>
        //   <ip addr="192.168.1.104"/>
        //   <netmask addr="255.255.255.0"/>
        //   <gateway addr="192.168.1.1"/>
        //   <dns addr="192.168.1.1"/>
        //   <mac addr="28:32:fd:be:36:40"/>
        let addr_of = |tag: &str| -> String {
            xml.find(&format!("<{tag} "))
                .and_then(|p| crate::xml::get_attr(&xml[p..], "addr"))
                .map(str::to_string)
                .unwrap_or_default()
        };
        let dhcp = xml.find("<dhcp ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "auto"))
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
        Ok(EthConfig {
            dhcp,
            ip:      addr_of("ip"),
            mask:    addr_of("netmask"),
            gateway: addr_of("gateway"),
            dns:     addr_of("dns"),
        })
    }

    pub async fn set_eth0_info(&mut self, cfg: &EthConfig) -> Result<()> {
        let body = command::set_eth0_info(cfg.dhcp, &cfg.ip, &cfg.mask, &cfg.gateway, &cfg.dns);
        self.sdk_cmd("SetEth0Info", &body).await?;
        Ok(())
    }

    // ── Time ─────────────────────────────────────────────────────────────

    pub async fn get_time_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetTimeInfo", &command::get_time_info()).await
    }

    pub async fn sync_time(&mut self) -> Result<()> {
        self.sdk_cmd("SetTimeInfo", &command::sync_time_now()).await?;
        Ok(())
    }

    /// Set the NTP server used for automatic time synchronisation.
    pub async fn set_ntp_server(&mut self, server: &str) -> Result<()> {
        self.sdk_cmd("SetNtpServer", &command::set_ntp_server(server)).await?;
        Ok(())
    }

    /// Set device timezone (UTC offset in whole hours, e.g. 8 for UTC+8).
    pub async fn set_timezone(&mut self, offset: i8) -> Result<()> {
        self.sdk_cmd("SetTimeInfo", &command::set_time_zone(offset)).await?;
        Ok(())
    }

    // ── FPGA Config ───────────────────────────────────────────────────────

    pub async fn get_fpga_config(&mut self) -> Result<String> {
        self.sdk_cmd("GetSDKFPGAConfig", &command::get_sdk_fpga_config()).await
    }

    pub async fn set_fpga_config(&mut self, config_xml: &str) -> Result<()> {
        self.sdk_cmd("SetSDKFPGAConfig", &command::set_sdk_fpga_config(config_xml)).await?;
        Ok(())
    }

    pub async fn get_box_hw_config(&mut self) -> Result<String> {
        self.sdk_cmd("GetBoxHwConfig", &command::get_box_hw_config()).await
    }

    /// Persist the current BoxHwConfig to flash storage.
    pub async fn save_box_hw_config(&mut self) -> Result<()> {
        self.sdk_cmd("SaveBoxHwConfig", &command::save_box_hw_config()).await?;
        Ok(())
    }

    /// Request automatic FPGA configuration for the attached LED module type.
    pub async fn smart_setting(&mut self) -> Result<()> {
        self.sdk_cmd("SmartSetting", &command::smart_setting()).await?;
        Ok(())
    }

    // ── Device Control ────────────────────────────────────────────────────

    pub async fn reboot(&mut self) -> Result<()> {
        self.sdk_cmd("RebootDevice", &command::reboot_device()).await?;
        Ok(())
    }

    pub async fn set_device_name(&mut self, name: &str) -> Result<()> {
        self.sdk_cmd("UpdateDevName", &command::set_device_name(name)).await?;
        Ok(())
    }

    pub async fn set_rotation(&mut self, angle: u16) -> Result<()> {
        self.sdk_cmd("SetRotation", &command::set_rotation(angle)).await?;
        Ok(())
    }

    pub async fn set_volume(&mut self, level: u8) -> Result<()> {
        self.sdk_cmd("SetVolume", &command::set_volume(level)).await?;
        Ok(())
    }

    // ── Extra queries ─────────────────────────────────────────────────────

    /// Get current brightness level (0–100).
    ///
    /// Uses `GetCurrentLuminance` which returns `<percent value="N"/>`.
    /// Confirmed from More Huidu.pcapng: response is `<percent value="100"/>`.
    pub async fn get_brightness(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetCurrentLuminance", "").await?;
        // Response: <out result="kSuccess" method="GetCurrentLuminance"><percent value="100"/></out>
        let level = xml.find("<percent ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        Ok(level)
    }

    // ── Sensor / Status queries ───────────────────────────────────────────

    /// Get current play status (0 = stopped, 1 = playing).
    ///
    /// Confirmed from More Huidu.pcapng: response is `<status value="1"/>`.
    pub async fn get_play_status(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetPlayStatus", "").await?;
        Ok(xml.find("<status ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// Get current audio volume (0–100).
    ///
    /// Confirmed from More Huidu.pcapng: response is `<volume precent="100"/>`.
    /// Note: "precent" is the protocol's typo for "percent".
    pub async fn get_volume(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetSystemVolume", "").await?;
        Ok(xml.find("<volume ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "precent"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(100))
    }

    /// Get sensor connectivity info (luminance, temperature, GPS, humidity, etc.)
    ///
    /// Confirmed from More Huidu.pcapng: each sensor has a `<connect enable="0"/>` child.
    pub async fn get_sensor_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetSensorInfo", "").await
    }

    /// Get GPS coordinates.
    ///
    /// Returns `<latitude value="-1"/><longitude value="-1"/>` when GPS is not connected.
    pub async fn get_gps_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetGPSInfo", "").await
    }

    /// Get current ambient temperature from sensor (-1 = no sensor connected).
    pub async fn get_current_temperature(&mut self) -> Result<i32> {
        let xml = self.sdk_cmd("GetCurrentTemperature", "").await?;
        Ok(xml.find("<temperature ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1))
    }

    /// Get current ambient humidity from sensor (-1 = no sensor connected).
    ///
    /// Note: the protocol spells this "Humity" (missing an 'i') — that is intentional
    /// and matches the real device firmware.
    pub async fn get_current_humidity(&mut self) -> Result<i32> {
        let xml = self.sdk_cmd("GetCurrentHumity", "").await?;
        Ok(xml.find("<humity ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1))
    }

    /// Get key definition (returned as `<key value="0"/>` on most devices).
    pub async fn get_key_define(&mut self) -> Result<u32> {
        let xml = self.sdk_cmd("GetKeyDefine", "").await?;
        Ok(crate::xml::get_attr(&xml, "value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// Get relay status list.
    ///
    /// Returns a Vec of `(use_switch, name, relay_status)` tuples — one per relay.
    /// Confirmed from More Huidu.pcapng: 6 relay items, all disabled.
    pub async fn get_relay(&mut self) -> Result<Vec<(bool, String, u8)>> {
        let xml = self.sdk_cmd("GetRelay", "").await?;
        let mut relays = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<item ") {
            let end = search[start..].find("/>")
                .map(|e| start + e + 2)
                .unwrap_or(search.len());
            let tag = &search[start..end];
            let use_switch = crate::xml::get_attr(tag, "useSwitch")
                .map(|v| v == "1")
                .unwrap_or(false);
            let name = crate::xml::get_attr(tag, "name")
                .map(crate::xml::xml_unescape)
                .unwrap_or_default();
            let status = crate::xml::get_attr(tag, "relayStatus")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            relays.push((use_switch, name, status));
            search = &search[end.min(search.len())..];
        }
        Ok(relays)
    }

    /// Get sensor measurement range.
    ///
    /// Returns `Ok(Some(xml))` when supported, `Ok(None)` when the device responds
    /// with kUnsupportMethod (confirmed: C-series BoxPlayer does not support this).
    pub async fn get_sensor_range(&mut self) -> Result<Option<String>> {
        let request_xml = xml::sdk_request(&self.client_guid, "GetSensorRange", "");
        let response = self.send_xml_request(&request_xml).await?;
        if response.contains("kUnsupportMethod") {
            return Ok(None);
        }
        Ok(Some(response))
    }

    /// Get device locker enabled status.
    ///
    /// Confirmed from More Huidu.pcapng: response is `<enable value="false"/>`.
    pub async fn get_device_locker_enable(&mut self) -> Result<bool> {
        let request_xml = xml::sdk_request(&self.client_guid, "GetDeviceLockerEnable", "");
        let response = self.send_xml_request(&request_xml).await?;
        if response.contains("kUnsupportMethod") {
            return Ok(false);
        }
        Ok(response.find("<enable ")
            .and_then(|p| crate::xml::get_attr(&response[p..], "value"))
            .map(|v| v == "true")
            .unwrap_or(false))
    }

    /// List all files stored on the device.
    pub async fn list_files(&mut self) -> Result<Vec<String>> {
        let xml = self.sdk_cmd("GetFiles", "").await?;
        let mut files = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<file ") {
            let end = search[start..]
                .find("/>")
                .map(|e| start + e + 2)
                .unwrap_or(search.len());
            let tag = &search[start..end];
            if let Some(name) = crate::xml::get_attr(tag, "name") {
                files.push(crate::xml::xml_unescape(name));
            }
            search = &search[end.min(search.len())..];
        }
        Ok(files)
    }

    /// Get file list with MD5 hashes and sizes.
    pub async fn get_file_checklist(&mut self) -> Result<Vec<FileEntry>> {
        let xml = self.sdk_cmd("GetFileChecklist", &command::get_file_checklist()).await?;
        let mut files = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<file ") {
            let end = search[start..]
                .find("/>")
                .map(|e| start + e + 2)
                .unwrap_or(search.len());
            let tag = &search[start..end];
            files.push(FileEntry {
                name: crate::xml::get_attr(tag, "name").map(crate::xml::xml_unescape).unwrap_or_default(),
                md5:  crate::xml::get_attr(tag, "md5").unwrap_or("").to_string(),
                size: crate::xml::get_attr(tag, "size")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            });
            search = &search[end.min(search.len())..];
        }
        Ok(files)
    }

    /// Set multiple data source key/value pairs in a single command.
    pub async fn set_data_sources(&mut self, entries: &[(&str, &str)]) -> Result<()> {
        self.sdk_cmd("SetDataSourceInfo", &command::set_data_source_info(entries)).await?;
        Ok(())
    }

    /// Get the GUID of the currently playing program (empty string if none).
    pub async fn get_current_program_guid(&mut self) -> Result<String> {
        let xml = self
            .sdk_cmd("GetCurrentPlayProgramGUID", "")
            .await?;
        Ok(crate::xml::get_attr(&xml, "guid").unwrap_or("").to_string())
    }

    /// Get current screen rotation in degrees (0, 90, 180, 270).
    pub async fn get_rotation(&mut self) -> Result<u16> {
        let xml = self.sdk_cmd("GetScreenRotation", &command::get_screen_rotation()).await?;
        Ok(crate::xml::get_attr(&xml, "value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// Get the boot logo filename (empty = none set).
    pub async fn get_boot_logo(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetBootLogo", &command::get_boot_logo()).await?;
        Ok(crate::xml::get_attr(&xml, "name").map(crate::xml::xml_unescape).unwrap_or_default())
    }

    /// Set the boot logo by filename (must already be on the device).
    pub async fn set_boot_logo(&mut self, filename: &str) -> Result<()> {
        self.sdk_cmd("SetBootLogoName", &command::set_boot_logo_name(filename)).await?;
        Ok(())
    }

    /// Clear the boot logo.
    pub async fn clear_boot_logo(&mut self) -> Result<()> {
        self.sdk_cmd("ClearBootLogo", &command::clear_boot_logo()).await?;
        Ok(())
    }

    /// Delete named files from device storage.
    pub async fn delete_files(&mut self, filenames: &[&str]) -> Result<()> {
        self.sdk_cmd("DeleteFiles", &command::delete_files(filenames)).await?;
        Ok(())
    }

    /// Get network connectivity status (eth0, wifi, internet).
    pub async fn get_network_info(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetNetworkInfo", &command::get_network_info()).await?;
        Ok(xml)
    }

    /// Get WiFi status (SSID and connection state).
    pub async fn get_wifi_info(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetWifiInfo", &command::get_wifi_info()).await?;
        Ok(xml)
    }

    /// Connect to a WiFi network.
    pub async fn set_wifi(&mut self, ssid: &str, password: &str) -> Result<()> {
        self.sdk_cmd("SetWifiInfo", &command::set_wifi(ssid, password, false)).await?;
        Ok(())
    }

    /// Get PPPoE connection configuration.
    pub async fn get_pppoe_info(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetPppoeInfo", &command::get_pppoe_info()).await?;
        Ok(xml)
    }

    /// Set PPPoE connection configuration.
    pub async fn set_pppoe_info(&mut self, enable: bool, user: &str, password: &str) -> Result<()> {
        self.sdk_cmd("SetPppoeInfo", &command::set_pppoe_info(enable, user, password)).await?;
        Ok(())
    }

    /// Get admin mode status: `(enabled, locker_on)`.
    pub async fn get_admin_mode_info(&mut self) -> Result<(bool, bool)> {
        let xml = self.sdk_cmd("GetAdminModeInfo", &command::get_admin_mode_info()).await?;
        let enabled = crate::xml::get_attr(&xml, "enable")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let locker = crate::xml::get_attr(&xml, "locker")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        Ok((enabled, locker))
    }

    /// Toggle a single relay output (SetRelayStatusInfo).
    ///
    /// `index`: 0-based relay number; `state`: true = on, false = off.
    pub async fn set_relay_status(&mut self, index: u8, state: bool) -> Result<()> {
        self.sdk_cmd("SetRelayStatusInfo", &command::set_relay_status(index, state)).await?;
        Ok(())
    }

    /// Remove the device license key.
    pub async fn clear_license(&mut self) -> Result<()> {
        self.sdk_cmd("ClearLicense", &command::clear_license()).await?;
        Ok(())
    }

    /// Get live sensor readings (temperature, humidity, luminance, etc.).
    pub async fn get_current_sensor_value(&mut self) -> Result<String> {
        self.sdk_cmd("GetCurrentSensorValue", &command::get_current_sensor_value()).await
    }

    /// Unlock admin mode using the device's configured password.
    /// Returns `true` if the password was accepted.
    pub async fn unlock_admin_password(&mut self, password: &str) -> Result<bool> {
        let xml = self.sdk_cmd(
            "UnlockAdminModePassword",
            &command::unlock_admin_password(password),
        ).await?;
        // Success: <adminMode enable="true"/>  Failure: <result value="1" .../>
        let accepted = xml.contains("adminMode") || !xml.contains("value=\"1\"");
        Ok(accepted)
    }

    /// Set or clear the admin unlock password.
    /// Pass an empty string to remove the password requirement.
    pub async fn set_admin_password(&mut self, password: &str) -> Result<()> {
        self.sdk_cmd("SetAdminModePassword", &command::set_admin_password(password)).await?;
        Ok(())
    }

    /// Trigger a SmartDrawLine color-bar test pattern on the display.
    pub async fn screen_test(&mut self) -> Result<()> {
        self.sdk_cmd("SmartDrawLine", &command::smart_draw_line()).await?;
        Ok(())
    }

    /// Get the firmware upgrade result.
    pub async fn get_upgrade_result(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetUpgradeResult", &command::get_upgrade_result()).await?;
        Ok(crate::xml::get_attr(&xml, "message").map(crate::xml::xml_unescape).unwrap_or_default())
    }

    /// Trigger a firmware upgrade from a file already on the device.
    pub async fn firmware_upgrade(&mut self, filename: &str) -> Result<()> {
        self.sdk_cmd("FirmwareUpgrade", &command::firmware_upgrade(filename)).await?;
        Ok(())
    }

    /// Get all data source key/value pairs from the device.
    pub async fn get_data_sources(&mut self) -> Result<Vec<(String, String)>> {
        let xml = self.sdk_cmd("GetDataSourceInfo", &command::get_data_source_info()).await?;
        let mut entries = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<dataSource ") {
            let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
            let tag = &search[start..end];
            if let (Some(name), Some(value)) = (
                crate::xml::get_attr(tag, "name"),
                crate::xml::get_attr(tag, "value"),
            ) {
                entries.push((
                    crate::xml::xml_unescape(name),
                    crate::xml::xml_unescape(value),
                ));
            }
            search = &search[end.min(search.len())..];
        }
        Ok(entries)
    }

    /// Set a single data source value.
    pub async fn set_data_source(&mut self, name: &str, value: &str) -> Result<()> {
        self.sdk_cmd("SetDataSourceInfo", &command::set_data_source_info(&[(name, value)])).await?;
        Ok(())
    }

    /// Delete all orphaned program XML files from device storage.
    /// Media files and configuration are preserved.
    pub async fn cleanup(&mut self) -> Result<()> {
        self.sdk_cmd("DeleteNotCiteFile", &command::delete_not_cite_file()).await?;
        Ok(())
    }

    /// Get the device license string. Returns `(license, valid)`.
    pub async fn get_license(&mut self) -> Result<(String, bool)> {
        let xml = self.sdk_cmd("GetLicense", &command::get_license()).await?;
        let value = crate::xml::get_attr(&xml, "value").unwrap_or("").to_string();
        let valid = crate::xml::get_attr(&xml, "valid")
            .map(|v| v == "true")
            .unwrap_or(!value.is_empty());
        Ok((value, valid))
    }

    /// Set the device license string.
    pub async fn set_license(&mut self, value: &str) -> Result<()> {
        self.sdk_cmd("SetLicense", &command::set_license(value)).await?;
        Ok(())
    }

    // ── File Transfer ─────────────────────────────────────────────────────

    /// Transfer a file to the device using the binary file transfer protocol.
    pub async fn upload_file(
        &mut self,
        transfer: &FileTransfer,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<()> {
        info!("Uploading '{}' ({} bytes, md5={})",
            transfer.filename, transfer.total_size(), transfer.md5);

        // Send FileStartAsk
        self.send_packet(&transfer.start_packet()).await?;
        let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
        if ack.command != Command::FileStartAnswer {
            return Err(Error::Transfer(format!("Expected FileStartAnswer, got {:?}", ack.command)));
        }
        self.check_file_answer(&ack)?;

        // Send chunks
        let total = transfer.total_size();
        for (offset, chunk) in transfer.chunks() {
            let pkt = crate::transfer::build_content_packet(offset, chunk);
            self.send_packet(&pkt).await?;
            let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
            if ack.command != Command::FileContentAnswer {
                return Err(Error::Transfer(
                    format!("Expected FileContentAnswer, got {:?}", ack.command)
                ));
            }
            self.check_file_answer(&ack)?;
            if let Some(cb) = progress {
                cb(offset + chunk.len() as u64, total);
            }
        }

        // Send FileEndAsk
        self.send_packet(&transfer.end_packet()).await?;
        let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
        if ack.command != Command::FileEndAnswer {
            return Err(Error::Transfer(format!("Expected FileEndAnswer, got {:?}", ack.command)));
        }
        self.check_file_answer(&ack)?;

        info!("Upload complete: '{}'", transfer.filename);
        Ok(())
    }

    fn check_file_answer(&self, pkt: &Packet) -> Result<()> {
        if pkt.payload.len() < 4 {
            return Ok(()); // Too short to contain a result code — treat as OK
        }
        let code = i32::from_le_bytes(pkt.payload[..4].try_into().unwrap_or([0; 4]));
        if code == 0 {
            Ok(())
        } else {
            Err(Error::Device {
                code,
                message: crate::error::DeviceError::from_code(code).message().into(),
            })
        }
    }
}

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Extract the inner body of `<out method="name">...</out>` from a batch response XML.
///
/// Returns `Some(body_xml)` — the content between the opening and closing `</out>` tags.
/// Returns `Some("")` for self-closing tags (e.g. `<out result="kUnsupportMethod" .../>`).
/// Returns `None` if the method's `<out>` block is not present in the XML.
fn extract_out_body(xml: &str, method: &str) -> Option<String> {
    let needle = format!("method=\"{method}\"");
    let method_pos = xml.find(&needle)?;
    // Walk back to the start of the enclosing <out element
    let out_start = xml[..method_pos].rfind("<out")?;
    let rest = &xml[out_start..];
    // Find the end of the opening tag
    let tag_end = rest.find('>')?;
    // Self-closing? e.g. <out result="kUnsupportMethod" method="X"/>
    if rest.as_bytes().get(tag_end.saturating_sub(1)) == Some(&b'/') {
        return Some(String::new());
    }
    let body_start = tag_end + 1;
    let close_off = rest[body_start..].find("</out>")?;
    Some(rest[body_start..body_start + close_off].to_string())
}
