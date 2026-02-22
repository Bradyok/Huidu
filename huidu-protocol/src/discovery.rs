//! UDP device discovery on port 9527.
//!
//! ## Protocol (both directions)
//!
//! ### PC → Device (search request)
//! Any UDP packet sent to port 9527 triggers a response.  The conventional
//! search trigger uses command `0x0001` (kSearchDeviceAsk):
//! ```text
//! [u16 LE length=2][u16 LE command=0x0001]
//! ```
//!
//! ### Device → PC (two-packet response)
//!
//! **Packet 1 — DeviceInfo** (client parses `DeviceInfo` from this):
//! ```text
//! [15 bytes] device_id  — null-padded ASCII
//! [ 4 bytes] IPv4 addr  — big-endian (network byte order)
//! [ N bytes] player_name — null-terminated UTF-8
//! [ M bytes] DeviceInfo XML — attribute-only root element
//! ```
//!
//! **Packet 2 — ext1** (status overlay, no IP field):
//! ```text
//! [15 bytes] device_id  — null-padded ASCII (same as packet 1)
//! [ M bytes] ext1 XML   — attribute-only root element
//! ```
//!
//! The device also broadcasts both packets every ~3 seconds.

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, warn};
use crate::error::{Error, Result};
use crate::xml;

/// UDP port used by all Huidu device discovery traffic.
pub const DISCOVERY_PORT: u16 = 9527;

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
    /// Try to parse a DeviceInfo packet received via UDP.
    ///
    /// Returns `None` for ext1 status packets (which have XML directly at byte 15).
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }

        // Device ID: first 15 bytes, null-padded
        let device_id = std::str::from_utf8(&data[..15])
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();

        // ext1 packets have XML starting at byte 15 (no IP field).
        // Detect by looking for '<' at that position.
        if data[15] == b'<' {
            return None;
        }

        // IP: bytes 15–18 in big-endian (network byte order)
        let addr = IpAddr::V4(Ipv4Addr::new(data[15], data[16], data[17], data[18]));

        // Player name: null-terminated string starting at byte 19
        let rest = &data[19..];
        let name_end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let name = std::str::from_utf8(&rest[..name_end]).unwrap_or("").to_string();

        // DeviceInfo XML: everything after the null terminator
        let xml_start = name_end + 1;
        let info_xml = if xml_start < rest.len() {
            let xml_bytes = &rest[xml_start..];
            let xml_end = xml_bytes.iter().rposition(|&b| b == b'>').map(|p| p + 1)
                .unwrap_or(xml_bytes.len());
            std::str::from_utf8(&xml_bytes[..xml_end]).unwrap_or("").to_string()
        } else {
            String::new()
        };

        // Parse optional fields from DeviceInfo XML
        let device_type = xml::get_attr(&info_xml, "DeviceType").map(|s| s.to_string())
            .or_else(|| xml::get_attr(&info_xml, "deviceType").map(|s| s.to_string()))
            .or_else(|| xml::get_attr(&info_xml, "type").map(|s| s.to_string()));
        let firmware_version = xml::get_attr(&info_xml, "SoftwareVersion").map(|s| s.to_string())
            .or_else(|| xml::get_attr(&info_xml, "version").map(|s| s.to_string()));
        let screen_width = xml::get_attr(&info_xml, "ScreenWidth")
            .or_else(|| xml::get_attr(&info_xml, "screenWidth"))
            .or_else(|| xml::get_attr(&info_xml, "width"))
            .and_then(|s| s.parse().ok());
        let screen_height = xml::get_attr(&info_xml, "ScreenHeight")
            .or_else(|| xml::get_attr(&info_xml, "screenHeight"))
            .or_else(|| xml::get_attr(&info_xml, "height"))
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

/// UDP discovery scanner (client-side).
pub struct Discovery;

impl Discovery {
    /// Broadcast a search packet and collect responses for `timeout`.
    ///
    /// Deduplicates by `device_id`.  ext1 status packets are silently skipped.
    pub async fn scan(timeout: Duration) -> Result<Vec<DeviceInfo>> {
        let socket = UdpSocket::bind("0.0.0.0:0").await
            .map_err(|e| Error::Connection(format!("bind UDP: {e}")))?;
        socket.set_broadcast(true)
            .map_err(|e| Error::Connection(format!("set broadcast: {e}")))?;

        // kSearchDeviceAsk: [u16 length=2][u16 cmd=0x0001]
        let search_pkt = [2u8, 0, 1, 0];
        let broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT);
        socket.send_to(&search_pkt, broadcast).await
            .map_err(|e| Error::Connection(format!("send broadcast: {e}")))?;
        debug!("Discovery broadcast sent to 255.255.255.255:{DISCOVERY_PORT}");

        let mut devices = Vec::new();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut buf = vec![0u8; 2048];

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() { break; }
            match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
                Ok(Ok((n, from))) => {
                    debug!("Discovery response from {from}: {n} bytes");
                    if let Some(dev) = DeviceInfo::parse(&buf[..n]) {
                        debug!("Found: {} @ {}", dev.name, dev.addr);
                        if !devices.iter().any(|d: &DeviceInfo| d.device_id == dev.device_id) {
                            devices.push(dev);
                        }
                    } else {
                        debug!("Skipped non-DeviceInfo packet from {from}");
                    }
                }
                Ok(Err(e)) => warn!("UDP recv error: {e}"),
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
