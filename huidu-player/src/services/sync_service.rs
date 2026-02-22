/// Multi-device sync service — UDP multicast master/slave synchronization.
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::Arc;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

use crate::core::sync_clock::SyncClock;

const MAGIC: &[u8; 4] = b"HDSY";
const VERSION: u8 = 1;
const PACKET_SIZE: usize = 57;

#[derive(Debug, Clone, PartialEq)]
pub enum SyncMode {
    None,
    Master,
    Slave,
}

impl std::str::FromStr for SyncMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "master" => Ok(SyncMode::Master),
            "slave" => Ok(SyncMode::Slave),
            _ => Ok(SyncMode::None),
        }
    }
}

struct SyncPacket {
    frame_number: u64,
    program_guid: [u8; 36],
    timestamp_us: u64,
}

impl SyncPacket {
    fn encode(&self) -> [u8; PACKET_SIZE] {
        let mut buf = [0u8; PACKET_SIZE];
        buf[0..4].copy_from_slice(MAGIC);
        buf[4] = VERSION;
        buf[5..13].copy_from_slice(&self.frame_number.to_be_bytes());
        buf[13..49].copy_from_slice(&self.program_guid);
        buf[49..57].copy_from_slice(&self.timestamp_us.to_be_bytes());
        buf
    }

    fn decode(buf: &[u8]) -> Option<Self> {
        if buf.len() < PACKET_SIZE { return None; }
        if &buf[0..4] != MAGIC { return None; }
        if buf[4] != VERSION { return None; }
        let frame_number = u64::from_be_bytes(buf[5..13].try_into().ok()?);
        let mut program_guid = [0u8; 36];
        program_guid.copy_from_slice(&buf[13..49]);
        let timestamp_us = u64::from_be_bytes(buf[49..57].try_into().ok()?);
        Some(Self { frame_number, program_guid, timestamp_us })
    }
}

pub struct SyncService;

impl SyncService {
    pub fn start_master(
        clock: Arc<SyncClock>,
        multicast_group: &str,
        port: u16,
        cancel: CancellationToken,
    ) {
        let group = multicast_group.to_string();
        let cancel2 = cancel.clone();
        std::thread::spawn(move || {
            let socket = match UdpSocket::bind("0.0.0.0:0") {
                Ok(s) => s,
                Err(e) => { warn!("Sync master bind failed: {}", e); return; }
            };
            let dest: SocketAddr = format!("{}:{}", group, port).parse().unwrap();
            let _ = socket.set_multicast_ttl_v4(4);
            info!("Sync master started → {}:{}", group, port);

            loop {
                if cancel2.is_cancelled() { break; }
                let frame = clock.current_frame();
                let guid_str = clock.get_program_guid();
                let mut guid_bytes = [b' '; 36];
                let g = guid_str.as_bytes();
                let len = g.len().min(36);
                guid_bytes[..len].copy_from_slice(&g[..len]);

                let now_us = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64;

                let pkt = SyncPacket { frame_number: frame, program_guid: guid_bytes, timestamp_us: now_us };
                let _ = socket.send_to(&pkt.encode(), dest);
                std::thread::sleep(std::time::Duration::from_millis(40)); // 25fps
            }
        });
    }

    pub fn start_slave(
        clock: Arc<SyncClock>,
        multicast_group: &str,
        port: u16,
        cancel: CancellationToken,
        program_tx: tokio::sync::mpsc::Sender<crate::core::player::PlayerCommand>,
    ) {
        let group = multicast_group.to_string();
        let cancel2 = cancel.clone();
        std::thread::spawn(move || {
            let socket = match UdpSocket::bind(format!("0.0.0.0:{}", port)) {
                Ok(s) => s,
                Err(e) => { warn!("Sync slave bind failed: {}", e); return; }
            };
            let _ = socket.set_read_timeout(Some(std::time::Duration::from_millis(100)));

            // Join multicast group
            if let Ok(mcast) = group.parse::<Ipv4Addr>() {
                let _ = socket.join_multicast_v4(&mcast, &Ipv4Addr::UNSPECIFIED);
            }

            info!("Sync slave listening on {}:{}", group, port);
            let mut buf = [0u8; 128];

            loop {
                if cancel2.is_cancelled() { break; }
                match socket.recv_from(&mut buf) {
                    Ok((n, _)) => {
                        if let Some(pkt) = SyncPacket::decode(&buf[..n]) {
                            clock.apply_sync(pkt.frame_number);
                            // Check program GUID match
                            let recv_guid = String::from_utf8_lossy(&pkt.program_guid).trim().to_string();
                            let local_guid = clock.get_program_guid();
                            if !recv_guid.is_empty() && recv_guid != local_guid {
                                let _ = program_tx.blocking_send(
                                    crate::core::player::PlayerCommand::SwitchProgram(recv_guid)
                                );
                            }
                        }
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => {}
                    Err(e) => { warn!("Sync slave recv error: {}", e); }
                }
            }
        });
    }
}
