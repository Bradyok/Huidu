/// UDP device discovery protocol.
/// Responds to HDPlayer search broadcasts on port 10001.
use anyhow::Result;
use byteorder::{LittleEndian, WriteBytesExt};
use tokio::net::UdpSocket;
use tracing::{debug, info, warn};

const CMD_SEARCH_DEVICE_ASK: u16 = 0x1001;
const CMD_SEARCH_DEVICE_ANSWER: u16 = 0x1002;
const UDP_PROTOCOL_VERSION: u32 = 0x0100_0005;

/// Device info for discovery responses
pub struct DeviceInfo {
    pub device_id: String,
    pub ip_address: String,
    pub screen_width: u16,
    pub screen_height: u16,
}

/// Run the UDP discovery responder
pub async fn run(port: u16, device_info: DeviceInfo) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let socket = UdpSocket::bind(&addr).await?;
    socket.set_broadcast(true)?;
    info!("UDP discovery listening on {}", addr);

    let mut buf = [0u8; 1024];

    loop {
        match socket.recv_from(&mut buf).await {
            Ok((len, peer)) => {
                if len < 6 {
                    continue;
                }

                // Parse: [version: u32 LE] [command: u16 LE] [data...]
                let version = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
                let cmd = u16::from_le_bytes([buf[4], buf[5]]);

                debug!("UDP from {}: cmd=0x{:04X}, version=0x{:08X}", peer, cmd, version);

                if cmd == CMD_SEARCH_DEVICE_ASK {
                    let response = build_search_response(&device_info);
                    if let Err(e) = socket.send_to(&response, peer).await {
                        warn!("Failed to send discovery response: {}", e);
                    } else {
                        info!("Responded to device search from {}", peer);
                    }
                }
            }
            Err(e) => {
                warn!("UDP receive error: {}", e);
            }
        }
    }
}

fn build_search_response(info: &DeviceInfo) -> Vec<u8> {
    let mut packet = Vec::new();

    // Protocol version
    packet.write_u32::<LittleEndian>(UDP_PROTOCOL_VERSION).unwrap();

    // Command: search answer
    packet.write_u16::<LittleEndian>(CMD_SEARCH_DEVICE_ANSWER).unwrap();

    // Device ID (15 bytes, null-padded)
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

    // Screen dimensions
    packet.write_u16::<LittleEndian>(info.screen_width).unwrap();
    packet.write_u16::<LittleEndian>(info.screen_height).unwrap();

    packet
}

/// Get the local IP address for the discovery response
pub fn get_local_ip() -> String {
    // Try to get the local IP by connecting to a public address
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
