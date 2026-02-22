/// UDP device discovery protocol on port 9527.
///
/// The player both responds to HDPlayer / HDSet search requests AND periodically
/// broadcasts its device info to 255.255.255.255:9527.
use anyhow::Result;
use std::sync::Arc;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

use huidu_protocol::discovery::{
    BOXPLAYER_VERSION, DISCOVERY_PORT,
    build_device_info_packet, build_ext1_packet,
};
pub use huidu_protocol::discovery::get_local_ip;

use crate::services::manager::ServicesState;

/// Fixed device info for discovery responses.
#[derive(Clone)]
pub struct DeviceInfo {
    pub device_id: String,
    pub ip_address: String,
    pub screen_width: u16,
    pub screen_height: u16,
    pub player_name: String,
}

/// Run the UDP discovery service — listens for search requests AND broadcasts periodically.
pub async fn run(device_info: DeviceInfo, services: Arc<RwLock<ServicesState>>) -> Result<()> {
    let addr = format!("0.0.0.0:{}", DISCOVERY_PORT);
    let socket = UdpSocket::bind(&addr).await?;
    socket.set_broadcast(true)?;
    info!("UDP discovery listening on {}", addr);

    let mut buf = [0u8; 2048];
    let mut broadcast_interval = time::interval(Duration::from_secs(3));
    let broadcast_addr = format!("255.255.255.255:{}", DISCOVERY_PORT);

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
                                let snap = read_state_snapshot(&services).await;
                                let dev_info_pkt = build_response_packets(&device_info, &snap);
                                let ext1_pkt = build_ext1(&device_info, &snap);

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
                    Err(e) => warn!("UDP receive error: {}", e),
                }
            }
            _ = broadcast_interval.tick() => {
                let snap = read_state_snapshot(&services).await;
                let dev_info_pkt = build_response_packets(&device_info, &snap);
                let ext1_pkt = build_ext1(&device_info, &snap);

                if let Err(e) = socket.send_to(&dev_info_pkt, &*broadcast_addr).await {
                    debug!("Broadcast DeviceInfo failed: {}", e);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
                if let Err(e) = socket.send_to(&ext1_pkt, &*broadcast_addr).await {
                    debug!("Broadcast Ext1 failed: {}", e);
                }
                debug!("Broadcast device info (brightness={}, rotation={}, screen_on={})",
                    snap.brightness, snap.rotation, snap.screen_on);
            }
        }
    }
}

// ── State snapshot ────────────────────────────────────────────────────────────

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

// ── Packet builder wrappers ───────────────────────────────────────────────────

fn build_response_packets(info: &DeviceInfo, snap: &StateSnapshot) -> Vec<u8> {
    build_device_info_packet(
        &info.device_id,
        &info.ip_address,
        &info.player_name,
        info.screen_width,
        info.screen_height,
        BOXPLAYER_VERSION,
        snap.screen_on,
        snap.rotation,
        snap.brightness,
        snap.volume,
    )
}

fn build_ext1(info: &DeviceInfo, snap: &StateSnapshot) -> Vec<u8> {
    build_ext1_packet(
        &info.device_id,
        snap.screen_on,
        snap.program_index,
        snap.program_count,
    )
}

