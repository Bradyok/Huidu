/// UDP device discovery protocol on port 9527.
/// The real BoxPlayer protocol uses binary packet headers followed by XML payloads.
/// The device both responds to HDPlayer search requests AND periodically broadcasts
/// its device info to 255.255.255.255:9527.
use anyhow::Result;
use tokio::net::UdpSocket;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

/// Discovery port used by Huidu protocol (confirmed in both HDPlayer.exe and BoxPlayer binaries)
pub const DISCOVERY_PORT: u16 = 9527;

/// Device info for discovery responses
#[derive(Clone)]
pub struct DeviceInfo {
    pub device_id: String,
    pub ip_address: String,
    pub screen_width: u16,
    pub screen_height: u16,
    pub player_name: String,
}

/// Run the UDP discovery service — listens for search requests AND broadcasts periodically
pub async fn run(device_info: DeviceInfo) -> Result<()> {
    let addr = format!("0.0.0.0:{}", DISCOVERY_PORT);
    let socket = UdpSocket::bind(&addr).await?;
    socket.set_broadcast(true)?;
    info!("UDP discovery listening on {}", addr);

    let mut buf = [0u8; 2048];
    let mut broadcast_interval = time::interval(Duration::from_secs(3));

    // Build the broadcast packets
    let dev_info_packet = build_device_info_packet(&device_info);
    let ext1_packet = build_ext1_packet(&device_info);
    let broadcast_addr = "255.255.255.255:9527";

    loop {
        tokio::select! {
            result = socket.recv_from(&mut buf) => {
                match result {
                    Ok((len, peer)) => {
                        debug!("UDP recv {} bytes from {}", len, peer);

                        // Try to detect if this is a search request
                        // HDPlayer may send various packet formats; respond to anything
                        // that arrives on our discovery port
                        if len >= 2 {
                            // Log the first bytes for debugging
                            let hex: String = buf[..len.min(32)].iter()
                                .map(|b| format!("{:02x}", b))
                                .collect::<Vec<_>>()
                                .join(" ");
                            debug!("UDP packet: {}", hex);

                            // Respond with both DeviceInfo and Ext1
                            if let Err(e) = socket.send_to(&dev_info_packet, peer).await {
                                warn!("Failed to send DeviceInfo response: {}", e);
                            }
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            if let Err(e) = socket.send_to(&ext1_packet, peer).await {
                                warn!("Failed to send Ext1 response: {}", e);
                            }
                            info!("Responded to search from {}", peer);
                        }
                    }
                    Err(e) => {
                        warn!("UDP receive error: {}", e);
                    }
                }
            }
            _ = broadcast_interval.tick() => {
                // Periodically broadcast device info (like RespondDevInfoTimer in BoxPlayer)
                if let Err(e) = socket.send_to(&dev_info_packet, broadcast_addr).await {
                    debug!("Broadcast DeviceInfo failed: {}", e);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Err(e) = socket.send_to(&ext1_packet, broadcast_addr).await {
                    debug!("Broadcast Ext1 failed: {}", e);
                }
                debug!("Broadcast device info");
            }
        }
    }
}

/// Build the DeviceInfo packet matching the BoxPlayer format.
/// Format observed in Wireshark: [device_id bytes][binary header][BoxPlayer name][DeviceInfo XML]
fn build_device_info_packet(info: &DeviceInfo) -> Vec<u8> {
    let mut packet = Vec::new();

    // Device ID (padded to 15 bytes with nulls, matching BoxPlayer)
    let id_bytes = info.device_id.as_bytes();
    let mut id_buf = [0u8; 15];
    let copy_len = id_bytes.len().min(15);
    id_buf[..copy_len].copy_from_slice(&id_bytes[..copy_len]);
    packet.extend_from_slice(&id_buf);

    // IP address as 4 bytes
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

    // Player name (null-terminated)
    packet.extend_from_slice(info.player_name.as_bytes());
    packet.push(0);

    // DeviceInfo XML
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <DeviceInfo>\
         <CPUType Value=\"5\"/>\
         <ScreenOnOff Value=\"1\"/>\
         <ScreenR Value=\"0\"/>\
         <HardwareVersion Value=\"1.0\"/>\
         </DeviceInfo>"
    );
    packet.extend_from_slice(xml.as_bytes());

    packet
}

/// Build the ext1 status packet.
/// Format: [device_id bytes][ext1 XML with play status]
fn build_ext1_packet(info: &DeviceInfo) -> Vec<u8> {
    let mut packet = Vec::new();

    // Device ID (padded to 15 bytes)
    let id_bytes = info.device_id.as_bytes();
    let mut id_buf = [0u8; 15];
    let copy_len = id_bytes.len().min(15);
    id_buf[..copy_len].copy_from_slice(&id_bytes[..copy_len]);
    packet.extend_from_slice(&id_buf);

    // ext1 XML with status info
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
         <ext1>\
         <PlayStatus value=\"1\"/>\
         <ProgramIndex index=\"0\"/>\
         <ProgramCount count=\"1\" normalCount=\"1\" intercutCount=\"0\"/>\
         <DeviceLocker enable=\"0\"/>\
         <WifiApPasswd simple=\"1\"/>\
         </ext1>"
    );
    packet.extend_from_slice(xml.as_bytes());

    packet
}

/// Get the local IP address for the discovery response
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
