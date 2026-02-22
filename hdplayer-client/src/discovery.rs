//! UDP device discovery on port 9527.
//!
//! ## Protocol
//!
//! HDPlayer broadcasts a search packet on UDP port 9527.
//! Devices respond with a packet containing:
//! ```text
//! [15 bytes] device_id (null-padded ASCII)
//! [ 4 bytes] IPv4 address (network byte order)
//! [ N bytes] player_name (null-terminated UTF-8)
//! [ M bytes] DeviceInfo XML (null-terminated or rest of packet)
//! ```

use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::{debug, warn};
use crate::error::{Error, Result};
use crate::xml;

pub const DISCOVERY_PORT: u16 = 9527;

/// A discovered Huidu device on the network.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    /// Hardware device ID (up to 15 ASCII chars)
    pub device_id: String,
    /// Device IP address
    pub addr: IpAddr,
    /// Player display name
    pub name: String,
    /// Raw DeviceInfo XML payload
    pub info_xml: String,
    /// Parsed device type (e.g. "D15", "C16H")
    pub device_type: Option<String>,
    /// Parsed firmware version
    pub firmware_version: Option<String>,
    /// Screen width in pixels
    pub screen_width: Option<u32>,
    /// Screen height in pixels
    pub screen_height: Option<u32>,
    /// Currently playing program GUID
    pub current_program: Option<String>,
    /// Current brightness (0–100)
    pub brightness: Option<u8>,
    /// Current rotation in degrees (0, 90, 180, 270)
    pub rotation: Option<u16>,
    /// Whether the screen is currently on
    pub screen_on: Option<bool>,
}

impl DeviceInfo {
    fn parse(data: &[u8]) -> Option<Self> {
        if data.len() < 20 {
            return None;
        }
        // Device ID: first 15 bytes, null-padded
        let device_id = std::str::from_utf8(&data[..15])
            .unwrap_or("")
            .trim_end_matches('\0')
            .to_string();

        // ext1 packets have XML directly at byte 15 (no IP address field).
        // Detect by checking for '<' at that position.
        if data[15] == b'<' {
            return None; // ext1 status packet — skip, not a device entry
        }

        // IP address: bytes 15–18 (network byte order = big endian)
        let ip = Ipv4Addr::new(data[15], data[16], data[17], data[18]);
        let addr = IpAddr::V4(ip);

        // Player name: null-terminated string starting at offset 19
        let rest = &data[19..];
        let name_end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        let name = std::str::from_utf8(&rest[..name_end])
            .unwrap_or("")
            .to_string();

        // DeviceInfo XML: everything after the null terminator
        let xml_start = name_end + 1;
        let info_xml = if xml_start < rest.len() {
            let xml_bytes = &rest[xml_start..];
            let xml_end = xml_bytes.iter().rposition(|&b| b == b'>').map(|p| p + 1)
                .unwrap_or(xml_bytes.len());
            std::str::from_utf8(&xml_bytes[..xml_end])
                .unwrap_or("")
                .to_string()
        } else {
            String::new()
        };

        // Parse optional fields from DeviceInfo XML.
        // huidu-player sends capitalized attribute names: SoftwareVersion, DeviceType, etc.
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
            device_id,
            addr,
            name,
            info_xml,
            device_type,
            firmware_version,
            screen_width,
            screen_height,
            current_program,
            brightness,
            rotation,
            screen_on,
        })
    }
}

/// UDP discovery scanner.
pub struct Discovery;

impl Discovery {
    /// Scan the local network for Huidu devices.
    ///
    /// Sends a broadcast search packet on UDP port 9527 and collects
    /// responses for the given duration.
    pub async fn scan(timeout: Duration) -> Result<Vec<DeviceInfo>> {
        let socket = UdpSocket::bind("0.0.0.0:0").await
            .map_err(|e| Error::Connection(format!("bind UDP: {e}")))?;
        socket.set_broadcast(true)
            .map_err(|e| Error::Connection(format!("set broadcast: {e}")))?;

        // Broadcast a search packet (simple "find device" payload)
        let search_packet = Self::build_search_packet();
        let broadcast = SocketAddr::new(IpAddr::V4(Ipv4Addr::BROADCAST), DISCOVERY_PORT);
        socket.send_to(&search_packet, broadcast).await
            .map_err(|e| Error::Connection(format!("send broadcast: {e}")))?;
        debug!("Broadcast discovery packet sent to 255.255.255.255:{DISCOVERY_PORT}");

        let mut devices = Vec::new();
        let deadline = tokio::time::Instant::now() + timeout;
        let mut buf = vec![0u8; 2048];

        loop {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, socket.recv_from(&mut buf)).await {
                Ok(Ok((n, from))) => {
                    debug!("Discovery response from {from}: {n} bytes");
                    if let Some(dev) = DeviceInfo::parse(&buf[..n]) {
                        debug!("Found device: {} @ {}", dev.name, dev.addr);
                        // Deduplicate by device_id
                        if !devices.iter().any(|d: &DeviceInfo| d.device_id == dev.device_id) {
                            devices.push(dev);
                        }
                    } else {
                        debug!("Skipped non-DeviceInfo packet from {from}");
                    }
                }
                Ok(Err(e)) => {
                    warn!("UDP recv error: {e}");
                }
                Err(_) => break, // timeout
            }
        }

        Ok(devices)
    }

    /// Build the UDP broadcast search packet.
    /// The exact payload triggers a `kSearchDeviceAnswer` / `kSearchAsk` response.
    fn build_search_packet() -> Vec<u8> {
        // Minimal device search: the device responds to any broadcast on port 9527.
        // The packet format is: [u16 length=2][u16 command=kSearchAsk]
        // Using a simple broadcast with just the UDP destination is sometimes
        // sufficient; some firmware versions respond to any packet on port 9527.
        //
        // Known search trigger payload from HDPlayer wire captures:
        // Empty payload with command 0x0001 (kSearchDeviceAsk)
        let cmd: u16 = 0x0001; // kSearchDeviceAsk (inferred)
        let len: u16 = 2;
        let mut pkt = Vec::with_capacity(4);
        pkt.extend_from_slice(&len.to_le_bytes());
        pkt.extend_from_slice(&cmd.to_le_bytes());
        pkt
    }
}
