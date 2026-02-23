//! UDP device discovery on port 9527.
//!
//! ## Protocol (confirmed by live packet capture — BoxPlayer 7.11.18.0)
//!
//! ### Ports
//! - **9527**: Used by BOTH PC and device.  The PC binds to port 9527,
//!   sends triggers to `broadcast:9527`, and all device responses also arrive
//!   on port 9527.
//!
//! ### Discovery flow
//!
//! 1. PC binds to `0.0.0.0:9527`.
//! 2. PC sends a search trigger broadcast to `255.255.255.255:9527`.
//! 3. Device replies with a **21-byte announce** packet FROM device port 9527:
//!    ```text
//!    [4 bytes]  header    — [counter, 0x00, 0x00, 0x01]
//!    [2 bytes]  cmd=0x02  — announce type (LE u16)
//!    [15 bytes] device_id — null-padded ASCII
//!    ```
//! 4. PC **echoes** the 21-byte packet back to `device_ip:9527`.
//! 5. Device sends a **full info** packet back to PC:9527.
//!
//! ### Full info packet layout (cmd=0x04, legacy)
//!
//! ```text
//!  0- 5: header  [counter, 0x00, 0x00, 0x01, 0x04, 0x00]
//!  6-20: device_id (15 bytes, null-padded ASCII)
//!    21: flag  (0x20 = static IP)
//! 22-25: IPv4 (big-endian)
//! 26-31: 6 bytes unknown
//! 32-35: subnet mask
//! 36-39: gateway
//! 40-43: DNS
//! 44-47: zeros
//!    48: cpu_type
//!    49: hw_version
//! 50-53: cap_flags (LE u32)
//! 54-70: zeros
//!    71: block_tag 0x07
//!    72: block_sub 0x04
//! 73-74: metric (LE u16)
//! 75-76: screen_width (LE u16)
//! 77-78: screen_height (LE u16)
//!    79: zero
//!    80: name_len (u8)
//! 81..(81+name_len): player name (ASCII)
//! (81+name_len): null
//! (82+name_len): xml_len (u8)
//! (83+name_len)..(83+name_len+xml_len): DeviceInfo XML
//! ```
//!
//! ### Full info packet layout (cmd=0x05, current firmware)
//!
//! Same as cmd=0x04 but with **4 extra bytes** at offset 6-9 (`[0x21,0,0,0]`),
//! which shifts device_id and everything after it by +4:
//! ```text
//!  0- 5: header  [counter, 0x00, 0x00, 0x01, 0x05, 0x00]
//!  6- 9: extra   [0x21, 0x00, 0x00, 0x00]
//! 10-24: device_id (15 bytes)
//!    25: flag
//! 26-29: IPv4
//!   …all other fields +4 vs cmd=0x04…
//!    84: name_len
//! 85..(85+name_len): player name
//! ```
//!
//! Devices also broadcast the announce packet every ~3 seconds so passive
//! listening also works without sending a search trigger.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, warn};
use crate::error::{Error, Result};
use crate::xml;

/// UDP port that **devices listen on** (PC sends search triggers here).
pub const DISCOVERY_PORT: u16 = 9527;

/// UDP port that the **PC (controller) listens on** for device unicast broadcasts.
///
/// Confirmed from hdplayer_real.pcapng:
///   - Device sends FROM port 9527 TO port 9526 (controller's listen port)
///   - PC responds FROM port 9526 TO device port 9527
/// The real HDPlayer binds to UDP port 9526 for receiving device announcements.
pub const CONTROLLER_UDP_PORT: u16 = 9526;

/// Fixed BoxPlayer firmware version string reported in discovery packets.
///
/// HDPlayer uses this to recognise the device type / firmware line.  It is
/// deliberately pinned to a known compatible release and is NOT the Rust crate
/// version.
pub const BOXPLAYER_VERSION: &str = "7.11.18.0";

// ── Client-side: scan and parse ───────────────────────────────────────────────

/// A Huidu device discovered on the local network.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Hardware device ID (up to 15 ASCII chars).
    pub device_id: String,
    /// Device's reported IP address.
    pub addr: IpAddr,
    /// BoxPlayer display name.
    pub name: String,
    /// Raw DeviceInfo XML payload for further parsing.
    pub info_xml: String,
    /// e.g. "D15", "C16H"
    pub device_type: Option<String>,
    pub firmware_version: Option<String>,
    pub screen_width: Option<u32>,
    pub screen_height: Option<u32>,
    /// Currently-playing program GUID (from HDPlayer DeviceInfo responses).
    pub current_program: Option<String>,
    /// Brightness 0–100.
    pub brightness: Option<u8>,
    /// Rotation in degrees (0, 90, 180, 270).
    pub rotation: Option<u16>,
    pub screen_on: Option<bool>,
}

impl DeviceInfo {
    /// Try to parse a full DeviceInfo packet received via UDP on port 9527.
    ///
    /// Handles both:
    /// - **New format** (≥85 bytes, 6-byte header, binary + XML):
    ///   confirmed by live packet capture of BoxPlayer 7.11.18.0
    /// - **Old format** (≥19 bytes, no header, starts with device_id):
    ///   documented in HCatNet.dll / NetIOServices.dll
    ///
    /// Returns `None` for short announce packets (21-byte, cmd 0x02) or
    /// unrecognised data.
    pub fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }

        // ── Detect new format ─────────────────────────────────────────────
        // New format: bytes [0..5] = [counter, 0, 0, 0x01, cmd, 0x00]
        // Full response has cmd = 0x04; announce has cmd = 0x02.
        let is_new_format = data.len() >= 6
            && data[1] == 0x00
            && data[2] == 0x00
            && data[3] == 0x01;

        if is_new_format {
            let cmd = u16::from_le_bytes([data[4], data[5]]);
            match cmd {
                0x04 | 0x05 => return Self::parse_new_format(data),
                _ => return None, // Short announce (0x02), ext1 (0x0340), etc.
            }
        }

        // ── Old format ────────────────────────────────────────────────────
        Self::parse_old_format(data)
    }

    /// Parse the new binary+XML format introduced in BoxPlayer ≥7.11.x.
    ///
    /// cmd=0x04: device_id at offset 6.
    /// cmd=0x05: 4 extra bytes at offset 6–9, device_id starts at offset 10.
    ///
    /// Confirmed offsets for cmd=0x05 (221-byte live capture):
    /// - device_id: [10..25)
    /// - flag:      [25]
    /// - IPv4:      [26..30)
    /// - MAC/unk6:  [30..36)
    /// - subnet:    [36..40)
    /// - gateway:   [40..44)
    /// - dns:       [44..48)
    /// - zeros4:    [48..52)
    /// - cpu:       [52], hw: [53]
    /// - cap_flags: [54..58)
    /// - zeros14:   [58..72)
    /// - block_tag: [72], block_sub: [73]
    /// - metric:    [74..76)
    /// - width:     [76..78), height: [78..80)
    /// - zero:      [80], name_len: [81], name: [82..)
    fn parse_new_format(data: &[u8]) -> Option<DeviceInfo> {
        let cmd = u16::from_le_bytes([data[4], data[5]]);
        // cmd=0x05 has 4 extra bytes before device_id; shift everything by 4.
        let extra: usize = if cmd == 0x05 { 4 } else { 0 };

        // Device ID: 15 bytes at 6+extra
        let id_start = 6 + extra;
        let id_end = id_start + 15;
        let device_id = std::str::from_utf8(data.get(id_start..id_end)?)
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();

        // IPv4: 4 bytes at 22+extra
        let ip_start = 22 + extra;
        let ip = data.get(ip_start..ip_start + 4)?;
        let addr = IpAddr::V4(Ipv4Addr::new(ip[0], ip[1], ip[2], ip[3]));

        // Screen dimensions: width at 76+extra (base 72 for cmd=0x04 + 4 extra for cmd=0x05).
        // Confirmed offsets from 221-byte live capture: width=[76..78), height=[78..80).
        let sw_start = 72 + extra; // = 76 for cmd=0x05, 72 for cmd=0x04
        let screen_width = data.get(sw_start..sw_start + 2).map(|b| {
            u16::from_le_bytes([b[0], b[1]]) as u32
        });
        let screen_height = data.get(sw_start + 2..sw_start + 4).map(|b| {
            u16::from_le_bytes([b[0], b[1]]) as u32
        });

        // Player name: length-prefixed at 77+extra (= 81 for cmd=0x05)
        let name_len_pos = 77 + extra;
        let name_len = *data.get(name_len_pos)? as usize;
        let name_start = name_len_pos + 1;
        let name_end = name_start.checked_add(name_len)?;
        let name = std::str::from_utf8(data.get(name_start..name_end)?)
            .unwrap_or("")
            .to_string();

        // XML: length-prefixed after name + null terminator
        let xml_len_pos = name_end + 1; // skip null terminator after name
        let xml_len = *data.get(xml_len_pos)? as usize;
        let xml_start = xml_len_pos + 1;
        let xml_end = xml_start.checked_add(xml_len)?;
        let info_xml = if xml_len > 0 {
            std::str::from_utf8(data.get(xml_start..xml_end)?)
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        // Parse optional fields from XML (may be limited in the compact XML)
        let device_type = xml::get_attr(&info_xml, "DeviceType").map(|s| s.to_string())
            .or_else(|| xml::get_attr(&info_xml, "deviceType").map(|s| s.to_string()));
        let firmware_version = xml::get_attr(&info_xml, "SoftwareVersion").map(|s| s.to_string());

        // Screen dimensions: prefer XML values (more authoritative), fall back to binary
        let screen_width = xml::get_attr(&info_xml, "ScreenWidth")
            .and_then(|s| s.parse().ok())
            .or(screen_width);
        let screen_height = xml::get_attr(&info_xml, "ScreenHeight")
            .and_then(|s| s.parse().ok())
            .or(screen_height);

        let current_program = xml::get_attr(&info_xml, "programGuid").map(|s| s.to_string());
        let brightness = xml::get_attr(&info_xml, "Brightness")
            .or_else(|| xml::get_attr(&info_xml, "brightness"))
            .and_then(|s| s.parse().ok());
        let rotation = xml::get_attr(&info_xml, "ScreenR")
            .or_else(|| xml::get_attr(&info_xml, "Rotation"))
            .and_then(|s| s.parse().ok());
        let screen_on = xml::get_attr(&info_xml, "ScreenOnOff")
            .map(|s| s != "0" && !s.eq_ignore_ascii_case("false"));

        Some(DeviceInfo {
            device_id, addr, name, info_xml, device_type, firmware_version,
            screen_width, screen_height, current_program, brightness, rotation, screen_on,
        })
    }

    /// Parse the older format (no header, device_id starts at byte 0).
    ///
    /// ```text
    ///  0-14: device_id (15 bytes, null-padded ASCII)
    /// 15-18: IPv4 address (big-endian)
    ///   19+: null-terminated player name
    ///     +: DeviceInfo XML (ends before final '>')
    /// ```
    fn parse_old_format(data: &[u8]) -> Option<DeviceInfo> {
        if data.len() < 20 {
            return None;
        }

        let device_id = std::str::from_utf8(&data[..15])
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();

        // ext1 status packets start with XML at byte 15 — skip them.
        if data[15] == b'<' {
            return None;
        }

        let addr = IpAddr::V4(Ipv4Addr::new(data[15], data[16], data[17], data[18]));

        let rest = &data[19..];
        let name_end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let name = std::str::from_utf8(&rest[..name_end]).unwrap_or("").to_string();

        let xml_start = name_end + 1;
        let info_xml = if xml_start < rest.len() {
            let xml_bytes = &rest[xml_start..];
            let xml_end = xml_bytes.iter().rposition(|&b| b == b'>').map(|p| p + 1)
                .unwrap_or(xml_bytes.len());
            std::str::from_utf8(&xml_bytes[..xml_end]).unwrap_or("").to_string()
        } else {
            String::new()
        };

        let device_type = xml::get_attr(&info_xml, "DeviceType").map(|s| s.to_string())
            .or_else(|| xml::get_attr(&info_xml, "deviceType").map(|s| s.to_string()))
            .or_else(|| xml::get_attr(&info_xml, "type").map(|s| s.to_string()));
        let firmware_version = xml::get_attr(&info_xml, "SoftwareVersion").map(|s| s.to_string())
            .or_else(|| xml::get_attr(&info_xml, "version").map(|s| s.to_string()));
        let screen_width = xml::get_attr(&info_xml, "ScreenWidth")
            .or_else(|| xml::get_attr(&info_xml, "screenWidth"))
            .and_then(|s| s.parse().ok());
        let screen_height = xml::get_attr(&info_xml, "ScreenHeight")
            .or_else(|| xml::get_attr(&info_xml, "screenHeight"))
            .and_then(|s| s.parse().ok());
        let current_program = xml::get_attr(&info_xml, "programGuid").map(|s| s.to_string());
        let brightness = xml::get_attr(&info_xml, "Brightness")
            .or_else(|| xml::get_attr(&info_xml, "brightness"))
            .and_then(|s| s.parse().ok());
        let rotation = xml::get_attr(&info_xml, "Rotation")
            .or_else(|| xml::get_attr(&info_xml, "rotation"))
            .or_else(|| xml::get_attr(&info_xml, "ScreenR"))
            .and_then(|s| s.parse().ok());
        let screen_on = xml::get_attr(&info_xml, "ScreenOnOff")
            .map(|s| s != "0" && !s.eq_ignore_ascii_case("false"));

        Some(DeviceInfo {
            device_id, addr, name, info_xml, device_type, firmware_version,
            screen_width, screen_height, current_program, brightness, rotation, screen_on,
        })
    }
}

/// Perform the UDP controller-registration handshake required before TCP BoxStream.
///
/// ## Why this is needed
///
/// Confirmed from `hdplayer_real.pcapng`: the real HDPlayer performs a UDP handshake
/// on port 9526 BEFORE establishing a TCP BoxStream connection.  Without this, the
/// device accepts the TCP connection and sends TcpHeartbeatAnswer, but then responds to
/// BoxStreamInit with a TCP FIN (graceful rejection).
///
/// ## Protocol (confirmed from real HDPlayer capture)
///
/// The device sends unicast UDP to the controller's port **9526**:
///
/// 1. **Device → PC cmd=0x0002** (21-byte announce; periodic, ~200 ms apart)
///    `[00 00 00 01][02 00][device_id_15]`
///
/// 2. **PC → Device cmd=0x0003** (acknowledgment)
///    `[03 00 00 01][03 00][device_id_15]`
///
/// 3. **Device → PC cmd=0x0004** (extended info, ~1 ms after cmd=0x0003)
///
/// ...then after ~2 seconds:
///
/// 4. **Device → PC cmd=0x0005** (token + full device info)
///    `[03 00 00 01][05 00][token_4][device_id_15][...]`
///
/// 5. **Device → PC cmd=0x0340** (ext1 status XML)
///    `[04 00 00 01][40 03][device_id_15][token_4][xml]`
///
/// 6. **PC → Device cmd=0x0006** (acknowledge with token)
///    `[00 00 00 01][06 00][device_id_15][token_4]`
///
/// 7. **PC → Device cmd=0x0341** (extended acknowledgment)
///    `[04 00 00 01][41 03][device_id_15][token_4]`
///
/// After step 7, the device accepts TCP BoxStream connections.
///
/// ## Usage
///
/// Call this before [`crate::packet::BOX_PLAYER_PORT`] TCP connection:
/// ```rust,ignore
/// udp_register(device_ip, Duration::from_secs(12)).await?;
/// // now safe to TcpStream::connect(device_ip:9527)
/// ```
pub async fn udp_register(device_ip: Ipv4Addr, timeout: Duration) -> Result<()> {
    use tracing::info;

    // Bind to the controller's UDP port that the device sends unicast to.
    let socket = UdpSocket::bind(format!("0.0.0.0:{CONTROLLER_UDP_PORT}")).await
        .map_err(|e| Error::Connection(
            format!("bind UDP controller port {CONTROLLER_UDP_PORT}: {e}")
        ))?;

    let device_addr = SocketAddr::new(IpAddr::V4(device_ip), DISCOVERY_PORT);
    let mut buf = vec![0u8; 2048];
    let deadline = tokio::time::Instant::now() + timeout;
    let mut sent_cmd3 = false;
    let mut pending_token: Option<[u8; 4]> = None;
    let mut pending_device_id: Option<[u8; 15]> = None;
    let mut got_cmd340 = false;

    info!("UDP registration: waiting for device announce on port {CONTROLLER_UDP_PORT}...");

    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            if sent_cmd3 {
                // Sent cmd=0x0003, treat as registered even if we didn't get cmd=0x0005/0x0340
                info!("UDP registration: timeout waiting for extended handshake — proceeding");
                return Ok(());
            }
            return Err(Error::Connection(
                format!("UDP registration: no announce from {device_ip} within {timeout:?}")
            ));
        }

        let Ok(Ok((n, from))) = tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await
        else {
            if sent_cmd3 {
                info!("UDP registration: socket timeout — proceeding with partial handshake");
                return Ok(());
            }
            break;
        };

        // Only handle packets from our target device
        if from.ip() != IpAddr::V4(device_ip) {
            debug!("UDP registration: ignoring packet from {from}");
            continue;
        }

        let data = &buf[..n];
        if data.len() < 6 {
            continue;
        }

        let cmd = u16::from_le_bytes([data[4], data[5]]);
        debug!("UDP registration: received cmd=0x{cmd:04x} ({n} bytes) from {from}");

        match cmd {
            0x0002 if n == 21 => {
                // Short announce — respond with cmd=0x0003.
                // PC response format: [03 00 00 01][03 00][device_id_15]
                let mut resp = [0u8; 21];
                resp[0] = 0x03;
                resp[1] = 0x00;
                resp[2] = 0x00;
                resp[3] = 0x01;
                resp[4] = 0x03; // cmd=0x0003
                resp[5] = 0x00;
                resp[6..21].copy_from_slice(&data[6..21]); // device_id
                socket.send_to(&resp, device_addr).await
                    .map_err(|e| Error::Connection(format!("send cmd=0x0003: {e}")))?;
                sent_cmd3 = true;
                info!("UDP registration: sent cmd=0x0003 acknowledgment");
            }
            0x0004 => {
                // Extended info response after cmd=0x0003 — acknowledged, keep waiting
                debug!("UDP registration: received cmd=0x0004 (extended info)");
            }
            0x0005 if n >= 25 => {
                // Token + device info.  Token is at bytes [6..10], device_id at [10..25].
                let mut token = [0u8; 4];
                token.copy_from_slice(&data[6..10]);
                let mut device_id = [0u8; 15];
                device_id.copy_from_slice(&data[10..25]);
                debug!("UDP registration: received cmd=0x0005, token={token:?}");
                pending_token = Some(token);
                pending_device_id = Some(device_id);
            }
            0x0340 => {
                // Ext1 status XML — sent together with cmd=0x0005.
                // Extract device_id [6..21] and token [21..25] from this packet.
                if n >= 25 && pending_token.is_none() {
                    let mut device_id = [0u8; 15];
                    device_id.copy_from_slice(&data[6..21]);
                    let mut token = [0u8; 4];
                    token.copy_from_slice(&data[21..25]);
                    pending_device_id = Some(device_id);
                    pending_token = Some(token);
                }
                got_cmd340 = true;
            }
            _ => {
                debug!("UDP registration: ignoring cmd=0x{cmd:04x}");
            }
        }

        // Once we have both cmd=0x0005 token AND cmd=0x0340, send the acks.
        if let (Some(token), Some(device_id), true) = (pending_token, pending_device_id, got_cmd340) {
            // cmd=0x0006: [00 00 00 01][06 00][device_id_15][token_4]
            let mut pkt6 = [0u8; 25];
            pkt6[0] = 0x00; pkt6[1] = 0x00; pkt6[2] = 0x00; pkt6[3] = 0x01;
            pkt6[4] = 0x06; pkt6[5] = 0x00;
            pkt6[6..21].copy_from_slice(&device_id);
            pkt6[21..25].copy_from_slice(&token);
            socket.send_to(&pkt6, device_addr).await
                .map_err(|e| Error::Connection(format!("send cmd=0x0006: {e}")))?;

            // cmd=0x0341: [04 00 00 01][41 03][device_id_15][token_4]
            let mut pkt341 = [0u8; 25];
            pkt341[0] = 0x04; pkt341[1] = 0x00; pkt341[2] = 0x00; pkt341[3] = 0x01;
            pkt341[4] = 0x41; pkt341[5] = 0x03;
            pkt341[6..21].copy_from_slice(&device_id);
            pkt341[21..25].copy_from_slice(&token);
            socket.send_to(&pkt341, device_addr).await
                .map_err(|e| Error::Connection(format!("send cmd=0x0341: {e}")))?;

            info!("UDP registration: sent cmd=0x0006 + cmd=0x0341 — registration complete");
            return Ok(());
        }
    }

    Err(Error::Connection(format!(
        "UDP registration failed: device {device_ip} did not complete handshake"
    )))
}

/// UDP discovery scanner (client-side).
pub struct Discovery;

impl Discovery {
    /// Broadcast a search packet and collect responding devices for `timeout`.
    ///
    /// ## Protocol
    ///
    /// 1. Binds to `0.0.0.0:9527` (same port used by both PC and device).
    /// 2. Sends a search trigger broadcast to `255.255.255.255:9527`.
    /// 3. When a 21-byte announce arrives, echoes it to `device_ip:9527`.
    /// 4. Parses subsequent full-info packets into [`DeviceInfo`].
    ///
    /// Deduplicates by `device_id`.
    pub async fn scan(timeout: Duration) -> Result<Vec<DeviceInfo>> {
        // Both PC and device use port 9527 — confirmed by live packet capture.
        let socket = UdpSocket::bind(format!("0.0.0.0:{DISCOVERY_PORT}")).await
            .map_err(|e| Error::Connection(
                format!("bind UDP port {DISCOVERY_PORT}: {e}")
            ))?;
        socket.set_broadcast(true)
            .map_err(|e| Error::Connection(format!("set broadcast: {e}")))?;

        // Send search trigger to broadcast:9527.
        // Use both old and new format to maximise compatibility.
        let broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT);

        // Old format: [u16 LE length=2][u16 LE cmd=0x0001] — confirmed to trigger response.
        let trigger_old = [2u8, 0, 1, 0];
        socket.send_to(&trigger_old, broadcast).await
            .map_err(|e| Error::Connection(format!("send broadcast (old): {e}")))?;

        // New format: [counter=0][0x00][0x00][0x01][cmd=0x01][0x00]
        let trigger_new = [0u8, 0, 0, 1, 1, 0];
        socket.send_to(&trigger_new, broadcast).await
            .map_err(|e| Error::Connection(format!("send broadcast (new): {e}")))?;

        debug!("Discovery: sent search broadcasts to 255.255.255.255:{DISCOVERY_PORT}, \
                listening on port {DISCOVERY_PORT}");

        let mut devices: Vec<DeviceInfo> = Vec::new();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut buf = vec![0u8; 2048];

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { break; }

            match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
                Ok(Ok((n, from))) => {
                    debug!("Discovery: received {n} bytes from {from}");
                    let data = &buf[..n];

                    // ── Detect and handle short announce (21-byte, cmd=0x02) ──────
                    let is_new_announce = n == 21
                        && data.get(3).copied() == Some(0x01)
                        && data.get(4).copied() == Some(0x02)
                        && data.get(5).copied() == Some(0x00);

                    if is_new_announce {
                        // Echo the announce back to device:9527 to trigger full response.
                        let device_addr = SocketAddr::new(from.ip(), DISCOVERY_PORT);
                        if let Err(e) = socket.send_to(data, device_addr).await {
                            warn!("Discovery: echo to {device_addr} failed: {e}");
                        } else {
                            debug!("Discovery: echoed announce to {device_addr}");
                        }
                        continue; // wait for the full response
                    }

                    // ── Try to parse as full DeviceInfo ───────────────────────────
                    match DeviceInfo::parse(data) {
                        Some(dev) => {
                            debug!("Discovery: found {} @ {}", dev.name, dev.addr);
                            if !devices.iter().any(|d: &DeviceInfo| d.device_id == dev.device_id) {
                                devices.push(dev);
                            }
                        }
                        None => debug!("Discovery: skipped unrecognised packet from {from}"),
                    }
                }
                Ok(Err(e)) => warn!("Discovery: UDP recv error: {e}"),
                Err(_) => break, // timeout
            }
        }

        Ok(devices)
    }
}

// ── Server-side: broadcast packet builders ────────────────────────────────────

/// Build the DeviceInfo broadcast packet (Packet 1 of the two-packet response).
///
/// ## Wire format
/// ```text
/// [15 bytes] device_id (null-padded ASCII)
/// [ 4 bytes] IPv4 address (big-endian)
/// [ N bytes] player_name (null-terminated UTF-8)
/// [ M bytes] DeviceInfo XML (attributes on root element)
/// ```
pub fn build_device_info_packet(
    device_id: &str,
    ip_address: &str,
    player_name: &str,
    screen_width: u16,
    screen_height: u16,
    software_version: &str,
    screen_on: bool,
    rotation: u16,
    brightness: u8,
    volume: u8,
) -> Vec<u8> {
    let mut packet = Vec::new();

    // Device ID: exactly 15 bytes, null-padded
    let mut id_buf = [0u8; 15];
    let id_src = device_id.as_bytes();
    id_buf[..id_src.len().min(15)].copy_from_slice(&id_src[..id_src.len().min(15)]);
    packet.extend_from_slice(&id_buf);

    // IPv4 address: 4 bytes, big-endian
    let ip_parts: Vec<u8> = ip_address.split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    if ip_parts.len() == 4 {
        packet.extend_from_slice(&ip_parts);
    } else {
        packet.extend_from_slice(&[0, 0, 0, 0]);
    }

    // Player name: null-terminated
    packet.extend_from_slice(player_name.as_bytes());
    packet.push(0);

    // DeviceInfo XML — HDPlayer expects inline attributes on <DeviceInfo/>.
    // SoftwareVersion must be present or HDPlayer silently discards the packet.
    // DeviceType 52 == 0x34 == D15 (async architecture).
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <DeviceInfo \
           SoftwareVersion=\"{software_version}\" \
           DeviceType=\"52\" \
           CPUType=\"6\" \
           ScreenWidth=\"{screen_width}\" \
           ScreenHeight=\"{screen_height}\" \
           ScreenOnOff=\"{on}\" \
           ScreenR=\"{rotation}\" \
           HardwareVersion=\"1.0\" \
           Volume=\"{volume}\" \
           Brightness=\"{brightness}\" \
           Rotation=\"{rotation}\" \
           AdminMode=\"0\"/>",
        on = if screen_on { 1 } else { 0 },
    );
    packet.extend_from_slice(xml.as_bytes());

    packet
}

/// Build the ext1 status broadcast packet (Packet 2 of the two-packet response).
///
/// ## Wire format
/// ```text
/// [15 bytes] device_id (null-padded ASCII)
/// [ M bytes] ext1 XML  (no IP address field — differs from DeviceInfo packet)
/// ```
pub fn build_ext1_packet(
    device_id: &str,
    screen_on: bool,
    program_index: usize,
    program_count: usize,
) -> Vec<u8> {
    let mut packet = Vec::new();

    // Device ID: exactly 15 bytes, null-padded
    let mut id_buf = [0u8; 15];
    let id_src = device_id.as_bytes();
    id_buf[..id_src.len().min(15)].copy_from_slice(&id_src[..id_src.len().min(15)]);
    packet.extend_from_slice(&id_buf);

    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <ext1 \
           PlayStatus=\"{play}\" \
           ProgramIndex=\"{program_index}\" \
           ProgramCount=\"{program_count}\" \
           NormalCount=\"{program_count}\" \
           IntercutCount=\"0\" \
           DeviceLocker=\"0\" \
           WifiApPasswd=\"1\"/>",
        play = if screen_on { 1 } else { 0 },
    );
    packet.extend_from_slice(xml.as_bytes());

    packet
}

// ── Network utility ───────────────────────────────────────────────────────────

/// Determine the local IP address by connecting to an external host.
///
/// Returns `"0.0.0.0"` if the local IP cannot be determined.
pub fn get_local_ip() -> String {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(sock) = socket {
        if sock.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = sock.local_addr() {
                return addr.ip().to_string();
            }
        }
    }
    "0.0.0.0".to_string()
}
