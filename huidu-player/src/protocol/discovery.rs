/// UDP device discovery protocol on port 9527.
/// The real BoxPlayer protocol uses binary packet headers followed by XML payloads.
/// The device both responds to HDPlayer search requests AND periodically broadcasts
/// its device info to 255.255.255.255:9527.
use anyhow::Result;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

use crate::services::manager::ServicesState;

/// Discovery port used by Huidu protocol (confirmed in both HDPlayer.exe and BoxPlayer binaries)
pub const DISCOVERY_PORT: u16 = 9527;

/// BoxPlayer firmware version reported in discovery packets.
/// This value is deliberately fixed at the compatible BoxPlayer release; it is NOT the
/// Rust crate version.  HDPlayer.exe uses it to identify the device type/firmware line.
const BOXPLAYER_VERSION: &str = "7.11.18.0";

/// Fixed device info for discovery responses (screen dimensions, hardware identity).
#[derive(Clone)]
pub struct DeviceInfo {
    pub device_id: String,
    pub ip_address: String,
    pub screen_width: u16,
    pub screen_height: u16,
    pub player_name: String,
}

/// Run the UDP discovery service — listens for search requests AND broadcasts periodically.
/// `services` provides live state (brightness, rotation, screen power, programs).
pub async fn run(device_info: DeviceInfo, services: Arc<RwLock<ServicesState>>) -> Result<()> {
    let addr = format!("0.0.0.0:{}", DISCOVERY_PORT);
    let socket = UdpSocket::bind(&addr).await?;
    socket.set_broadcast(true)?;
    info!("UDP discovery listening on {}", addr);

    let mut buf = [0u8; 2048];
    let mut broadcast_interval = time::interval(Duration::from_secs(3));
    let broadcast_addr = "255.255.255.255:9527";

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, peer)) => {
                        debug!("UDP recv {} bytes from {}", len, peer);

                        if len >= 2 {
                            let hex: String = buf[..len.min(32)].iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            debug!("UDP packet: {}", hex);

                            // Ignore our own periodic broadcasts looping back on port 9527
                            if peer.port() == DISCOVERY_PORT {
                                debug!("Ignoring own broadcast from {}", peer);
                            } else {
                                // Read live state for the unicast response
                                let snap = read_state_snapshot(&services).await;
                                let dev_info_pkt = build_device_info_packet(&device_info, &snap);
                                let ext1_pkt = build_ext1_packet(&device_info, &snap);

                                if let Err(e) = socket.send_to(&dev_info_pkt, peer).await {
                                    warn!("Failed to send DeviceInfo response: {}", e);
                                }
                                tokio::time::sleep(Duration::from_millis(50)).await;
                                if let Err(e) = socket.send_to(&ext1_pkt, peer).await {
                                    warn!("Failed to send Ext1 response: {}", e);
                                }
                                info!("Responded to search from {}", peer);
                            }
                        }
                    }
                    Err(e) => {
                        warn!("UDP receive error: {}", e);
                    }
                }
            }
            _ = broadcast_interval.tick() => {
                // Rebuild with fresh state on every broadcast tick
                let snap = read_state_snapshot(&services).await;
                let dev_info_pkt = build_device_info_packet(&device_info, &snap);
                let ext1_pkt = build_ext1_packet(&device_info, &snap);

                if let Err(e) = socket.send_to(&dev_info_pkt, broadcast_addr).await {
                    debug!("Broadcast DeviceInfo failed: {}", e);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Err(e) = socket.send_to(&ext1_pkt, broadcast_addr).await {
                    debug!("Broadcast Ext1 failed: {}", e);
                }
                debug!("Broadcast device info (brightness={}, rotation={}, screen_on={})",
                    snap.brightness, snap.rotation, snap.screen_on);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Dynamic state snapshot
// ---------------------------------------------------------------------------

struct StateSnapshot {
    brightness: u8,
    rotation: u16,
    volume: u8,
    screen_on: bool,
    program_count: usize,
    program_index: usize,
}

async fn read_state_snapshot(services: &Arc<RwLock<ServicesState>>) -> StateSnapshot {
    let s = services.read().await;
    let idx = s.programs
        .iter()
        .position(|p| p.guid == s.current_program_guid)
        .unwrap_or(0);
    StateSnapshot {
        brightness: s.brightness.get_level(),
        rotation: s.rotation,
        volume: s.volume,
        screen_on: s.screen_on,
        program_count: s.programs.len().max(1),
        program_index: idx,
    }
}

// ---------------------------------------------------------------------------
// Packet builders
// ---------------------------------------------------------------------------

/// Build the DeviceInfo packet matching the BoxPlayer format.
///
/// Wire format (confirmed from Huidu protocol analysis):
///   [15 bytes] device_id (null-padded ASCII)
///   [ 4 bytes] IPv4 address (big-endian / network order)
///   [ N bytes] player_name (null-terminated UTF-8)
///   [ M bytes] DeviceInfo XML — attributes on the root element, NOT child elements
fn build_device_info_packet(info: &DeviceInfo, snap: &StateSnapshot) -> Vec<u8> {
    let mut packet = Vec::new();

    // Device ID: exactly 15 bytes, null-padded
    let id_bytes = info.device_id.as_bytes();
    let mut id_buf = [0u8; 15];
    let copy_len = id_bytes.len().min(15);
    id_buf[..copy_len].copy_from_slice(&id_bytes[..copy_len]);
    packet.extend_from_slice(&id_buf);

    // IPv4 address — 4 bytes, big-endian (network byte order)
    let ip_parts: Vec<u8> = info
        .ip_address
        .split('.')
        .filter_map(|s| s.parse().ok())
        .collect();
    if ip_parts.len() == 4 {
        packet.extend_from_slice(&ip_parts);
    } else {
        packet.extend_from_slice(&[0, 0, 0, 0]);
    }

    // Player name: null-terminated
    packet.extend_from_slice(info.player_name.as_bytes());
    packet.push(0);

    // DeviceInfo XML — HDPlayer expects INLINE ATTRIBUTES on <DeviceInfo/>.
    // SoftwareVersion must be present or HDPlayer will silently discard the packet.
    // DeviceType 52 == 0x34 == D15 (async architecture).
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <DeviceInfo \
           SoftwareVersion=\"{ver}\" \
           DeviceType=\"52\" \
           CPUType=\"6\" \
           ScreenWidth=\"{w}\" \
           ScreenHeight=\"{h}\" \
           ScreenOnOff=\"{on}\" \
           ScreenR=\"{rot}\" \
           HardwareVersion=\"1.0\" \
           Volume=\"{vol}\" \
           Brightness=\"{bright}\" \
           Rotation=\"{rot}\" \
           AdminMode=\"0\"/>",
        ver = BOXPLAYER_VERSION,
        w = info.screen_width,
        h = info.screen_height,
        on = if snap.screen_on { 1 } else { 0 },
        rot = snap.rotation,
        vol = snap.volume,
        bright = snap.brightness,
    );
    packet.extend_from_slice(xml.as_bytes());

    packet
}

/// Build the ext1 status packet.
///
/// Wire format: [15 bytes device_id][ext1 XML]
/// (no IP address field — ext1 omits it)
fn build_ext1_packet(info: &DeviceInfo, snap: &StateSnapshot) -> Vec<u8> {
    let mut packet = Vec::new();

    // Device ID: exactly 15 bytes, null-padded
    let id_bytes = info.device_id.as_bytes();
    let mut id_buf = [0u8; 15];
    let copy_len = id_bytes.len().min(15);
    id_buf[..copy_len].copy_from_slice(&id_bytes[..copy_len]);
    packet.extend_from_slice(&id_buf);

    // ext1 XML — uses inline attributes
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <ext1 \
           PlayStatus=\"{play}\" \
           ProgramIndex=\"{idx}\" \
           ProgramCount=\"{cnt}\" \
           NormalCount=\"{cnt}\" \
           IntercutCount=\"0\" \
           DeviceLocker=\"0\" \
           WifiApPasswd=\"1\"/>",
        play = if snap.screen_on { 1 } else { 0 },
        idx  = snap.program_index,
        cnt  = snap.program_count,
    );
    packet.extend_from_slice(xml.as_bytes());

    packet
}

/// Get the local IP address for the discovery response
pub fn get_local_ip() -> String {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok();
    if let Some(sock) = socket
        && sock.connect("8.8.8.8:80").is_ok()
            && let Ok(addr) = sock.local_addr() {
                return addr.ip().to_string();
            }
    "0.0.0.0".to_string()
}
