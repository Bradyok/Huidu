/// TCP protocol server — accepts connections from HDPlayer / HDSet software.
use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};

use huidu_protocol::packet::{Command, Packet, SDK_TRANSPORT_VERSION, parse_box_stream_data};

use crate::core::player::PlayerCommand;
use crate::protocol::command;
use crate::protocol::session::Session;
use crate::services::manager::ServicesState;

const MAX_PACKET_SIZE: usize = 9 * 1024;

pub async fn run(
    port: u16,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: String,
    services: Arc<RwLock<ServicesState>>,
    screen_width: u32,
    screen_height: u32,
) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!("Protocol server listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                info!("New connection from {}", peer);
                let tx = player_tx.clone();
                let dir = program_dir.clone();
                let svc = services.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        handle_connection(stream, tx, dir, svc, screen_width, screen_height).await
                    {
                        warn!("Connection error from {}: {}", peer, e);
                    }
                    info!("Connection closed: {}", peer);
                });
            }
            Err(e) => {
                error!("Accept error: {}", e);
            }
        }
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: String,
    services: Arc<RwLock<ServicesState>>,
    screen_width: u32,
    screen_height: u32,
) -> Result<()> {
    let mut session = Session::new();
    let mut buf = vec![0u8; MAX_PACKET_SIZE];

    while let Ok(l) = stream.read_u16_le().await {
        let length = l as usize;

        if !(4..=MAX_PACKET_SIZE).contains(&length) {
            warn!("Invalid packet length: {}", length);
            break;
        }

        let data_len = length - 4;
        let cmd_raw = stream.read_u16_le().await?;

        if data_len > 0 {
            if data_len > buf.len() {
                buf.resize(data_len, 0);
            }
            stream.read_exact(&mut buf[..data_len]).await?;
        }

        let cmd = Command::from_u16(cmd_raw);
        let response: Option<Vec<u8>> = match cmd {
            Command::TcpHeartbeatAsk => {
                Some(Packet::new(Command::TcpHeartbeatAnswer, vec![]).to_bytes())
            }

            Command::SdkServiceAsk => {
                let mut resp_data = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp_data, SDK_TRANSPORT_VERSION)
                    .unwrap();
                Some(Packet::new(Command::SdkServiceAnswer, resp_data).to_bytes())
            }

            Command::SdkCmdAsk => {
                // Payload format: [u32 total_len][u32 chunk_index][xml_bytes...]
                if data_len >= 8 {
                    let mut cursor = Cursor::new(&buf[..data_len]);
                    let total_len =
                        ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)? as usize;
                    let index = ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)? as usize;
                    let xml_chunk = &buf[8..data_len];

                    session.accumulate_xml(xml_chunk, total_len, index);

                    if session.xml_complete() {
                        let xml = session.take_xml();
                        let xml_str = String::from_utf8_lossy(&xml);
                        info!("SDK command ({} bytes)", xml_str.len());

                        match command::handle_sdk_command(
                            &xml_str,
                            &session,
                            &player_tx,
                            &program_dir,
                            &services,
                            screen_width,
                            screen_height,
                        )
                        .await
                        {
                            Ok(response_xml) => {
                                // Response payload: [u32 total_len][u32 index=0][xml_bytes]
                                let xml_bytes = response_xml.as_bytes();
                                let mut resp = Vec::new();
                                WriteBytesExt::write_u32::<LittleEndian>(
                                    &mut resp,
                                    xml_bytes.len() as u32,
                                )
                                .unwrap();
                                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                                resp.extend_from_slice(xml_bytes);
                                Some(Packet::new(Command::SdkCmdAnswer, resp).to_bytes())
                            }
                            Err(e) => {
                                warn!("SDK command error: {}", e);
                                None
                            }
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            }

            Command::FileStartAsk => {
                // Payload: [32 bytes MD5 hex][u64 file_size][u16 file_type][filename\0]
                if data_len >= 42 {
                    let md5_str = String::from_utf8_lossy(&buf[..32]).to_string();
                    let mut cursor = Cursor::new(&buf[32..]);
                    let file_size = ReadBytesExt::read_u64::<LittleEndian>(&mut cursor)?;
                    let file_type = ReadBytesExt::read_u16::<LittleEndian>(&mut cursor)?;
                    let filename = String::from_utf8_lossy(&buf[42..data_len])
                        .trim_end_matches('\0')
                        .to_string();

                    info!("File start: {} ({} bytes, type {})", filename, file_size, file_type);
                    session.start_file_transfer(filename, file_size, file_type, md5_str);

                    // Response: [u32 result=0][u64 resume_offset=0]
                    let mut resp = Vec::new();
                    WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                    WriteBytesExt::write_u64::<LittleEndian>(&mut resp, 0).unwrap();
                    Some(Packet::new(Command::FileStartAnswer, resp).to_bytes())
                } else {
                    None
                }
            }

            Command::FileContentAsk => {
                // Payload: [u64 offset][chunk data]
                const OFFSET_LEN: usize = 8;
                if data_len > OFFSET_LEN {
                    session.append_file_data(&buf[OFFSET_LEN..data_len]);
                } else if data_len > 0 {
                    // Older protocol variant: no offset prefix
                    session.append_file_data(&buf[..data_len]);
                }
                // Per-chunk ack: [u32 result=0]
                let mut resp = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                Some(Packet::new(Command::FileContentAnswer, resp).to_bytes())
            }

            Command::FileEndAsk => {
                let result: u32 = if let Some(transfer) = session.complete_file_transfer() {
                    let md5_ok = if !transfer.md5.is_empty() {
                        let computed = format!("{:x}", md5::compute(&transfer.data));
                        if computed != transfer.md5.to_lowercase() {
                            warn!(
                                "MD5 mismatch for {}: expected={} computed={}",
                                transfer.filename, transfer.md5, computed
                            );
                            false
                        } else {
                            true
                        }
                    } else {
                        true
                    };

                    let dest = std::path::Path::new(&program_dir).join(&transfer.filename);
                    info!(
                        "File transfer complete: {} ({} bytes, md5_ok={})",
                        transfer.filename, transfer.data.len(), md5_ok
                    );
                    let _ = std::fs::create_dir_all(&program_dir);
                    let _ = std::fs::write(&dest, &transfer.data);
                    0u32
                } else {
                    0u32
                };
                let mut resp = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, result).unwrap();
                Some(Packet::new(Command::FileEndAnswer, resp).to_bytes())
            }

            _ => {
                warn!("Unknown command: 0x{:04X}", cmd_raw);
                None
            }
        };

        if let Some(resp) = response {
            stream.write_all(&resp).await?;
        }
    }

    Ok(())
}

// ── BoxStream server (port 9527) ────────────────────────────────────────────
//
// HDPlayer.exe connects here using the 12-step BoxStream protocol documented
// in huidu_protocol::packet.  File transfers share the same TCP connection
// and use the same FileStart/Content/End packet framing as the legacy server.

/// Start the BoxStream server on the given port (typically 9527).
/// This is what real HDPlayer.exe connects to — it does NOT use port 10001.
pub async fn run_box_stream(
    port: u16,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: String,
    services: Arc<RwLock<ServicesState>>,
    screen_width: u32,
    screen_height: u32,
) -> Result<()> {
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await?;
    info!("BoxStream server listening on {} (HDPlayer.exe protocol)", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer)) => {
                info!("BoxStream connection from {}", peer);
                let tx = player_tx.clone();
                let dir = program_dir.clone();
                let svc = services.clone();
                tokio::spawn(async move {
                    if let Err(e) =
                        handle_box_stream(stream, tx, dir, svc, screen_width, screen_height).await
                    {
                        warn!("BoxStream connection error from {}: {}", peer, e);
                    }
                    info!("BoxStream connection closed: {}", peer);
                });
            }
            Err(e) => {
                error!("BoxStream accept error: {}", e);
            }
        }
    }
}

/// Handle a single BoxStream connection (HDPlayer.exe → device).
///
/// Protocol flow per exchange (12 steps, confirmed from Huidu.pcapng):
///  1  PC → Device: BoxStreamInit [0x00,0x00,0x00,0x00]  (request)
///  2  Device → PC: BoxStreamInitAck [0x00,0x00]
///  3  PC → Device: BoxStreamData [0x00,0x00]+xml
///  4  Device → PC: BoxStreamRxAck [0x00,0x00]
///  5  PC → Device: BoxStreamTxAck [0x00,0x00]
///  6  Device → PC: BoxStreamFinalAck [0x00,0x00]
///  7  Device → PC: BoxStreamInit [0x01,0x00,0x00,0x00]  ("response ready")
///  8  PC → Device: BoxStreamInitAck [0x00,0x00]
///  9  Device → PC: BoxStreamData [0x01,0x00]+xml
/// 10  PC → Device: BoxStreamRxAck [0x00,0x00]
/// 11  Device → PC: BoxStreamTxAck [0x01,0x00]
/// 12  PC → Device: BoxStreamFinalAck [0x00,0x00]
///
/// File transfers (FileStartAsk/FileContentAsk/FileEndAsk) use the same TCP
/// connection and the same packet framing; they arrive between exchanges.
async fn handle_box_stream(
    mut stream: TcpStream,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: String,
    services: Arc<RwLock<ServicesState>>,
    screen_width: u32,
    screen_height: u32,
) -> Result<()> {
    let mut session = Session::new();
    let mut buf: Vec<u8> = Vec::with_capacity(MAX_PACKET_SIZE * 4);

    // Device sends TcpHeartbeatAnswer immediately — HDPlayer.exe waits for two
    // heartbeat cycles before sending the first BoxStreamInit.
    stream
        .write_all(&Packet::new(Command::TcpHeartbeatAnswer, vec![]).to_bytes())
        .await?;

    let mut hb_tick =
        tokio::time::interval(std::time::Duration::from_secs(5));
    hb_tick.tick().await; // discard the immediate first tick

    loop {
        // Send a periodic heartbeat or read the next incoming packet.
        let pkt = tokio::select! {
            _ = hb_tick.tick() => {
                stream
                    .write_all(
                        &Packet::new(Command::TcpHeartbeatAnswer, vec![]).to_bytes(),
                    )
                    .await?;
                continue;
            }
            r = bs_recv(&mut stream, &mut buf) => r?,
        };

        match pkt.command {
            // HDPlayer.exe sends TcpHeartbeatAsk to ack each of our heartbeats.
            Command::TcpHeartbeatAsk | Command::TcpHeartbeatAnswer => {
                debug!("BoxStream: heartbeat {:?}", pkt.command);
            }

            // ── XML command exchange ─────────────────────────────────────────
            Command::BoxStreamInit if pkt.payload.first() == Some(&0x00) => {
                // Step 2: ack the client's init
                stream
                    .write_all(
                        &Packet::new(Command::BoxStreamInitAck, vec![0x00, 0x00]).to_bytes(),
                    )
                    .await?;

                // Step 3: receive BoxStreamData carrying the request XML
                let xml_str = loop {
                    let p = bs_recv_skip_hb(&mut stream, &mut buf).await?;
                    if p.command == Command::BoxStreamData {
                        let xml_bytes = parse_box_stream_data(&p.payload)
                            .ok_or_else(|| anyhow::anyhow!("BoxStreamData payload too short"))?;
                        break String::from_utf8_lossy(xml_bytes).into_owned();
                    }
                    warn!(
                        "BoxStream: expected BoxStreamData, got {:?} — ignoring",
                        p.command
                    );
                };
                info!("BoxStream XML command ({} bytes)", xml_str.len());

                // Step 4: acknowledge receipt of the XML
                stream
                    .write_all(
                        &Packet::new(Command::BoxStreamRxAck, vec![0x00, 0x00]).to_bytes(),
                    )
                    .await?;

                // Step 5: wait for client BoxStreamTxAck
                bs_skip_to(&mut stream, &mut buf, Command::BoxStreamTxAck).await?;

                // Step 6: send BoxStreamFinalAck
                stream
                    .write_all(
                        &Packet::new(Command::BoxStreamFinalAck, vec![0x00, 0x00]).to_bytes(),
                    )
                    .await?;

                // Process the XML command
                let resp_xml = match command::handle_sdk_command(
                    &xml_str,
                    &session,
                    &player_tx,
                    &program_dir,
                    &services,
                    screen_width,
                    screen_height,
                )
                .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        warn!("BoxStream command error: {}", e);
                        format!(
                            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                             <sdk guid=\"##GUID\"><out result=\"kError\"/></sdk>"
                        )
                    }
                };

                // Step 7: signal that the response is ready
                stream
                    .write_all(
                        &Packet::new(
                            Command::BoxStreamInit,
                            vec![0x01, 0x00, 0x00, 0x00],
                        )
                        .to_bytes(),
                    )
                    .await?;

                // Step 8: wait for client BoxStreamInitAck
                bs_skip_to(&mut stream, &mut buf, Command::BoxStreamInitAck).await?;

                // Step 9: send response XML in BoxStreamData [dir=1]
                let mut resp_payload = vec![0x01u8, 0x00];
                resp_payload.extend_from_slice(resp_xml.as_bytes());
                stream
                    .write_all(
                        &Packet::new(Command::BoxStreamData, resp_payload).to_bytes(),
                    )
                    .await?;

                // Step 10: wait for client BoxStreamRxAck
                bs_skip_to(&mut stream, &mut buf, Command::BoxStreamRxAck).await?;

                // Step 11: send BoxStreamTxAck [dir=1]
                stream
                    .write_all(
                        &Packet::new(Command::BoxStreamTxAck, vec![0x01, 0x00]).to_bytes(),
                    )
                    .await?;

                // Step 12: wait for client BoxStreamFinalAck
                bs_skip_to(&mut stream, &mut buf, Command::BoxStreamFinalAck).await?;

                // Exchange complete — loop for the next command.
            }

            // ── File transfer (same framing as legacy port 10001 server) ────
            Command::FileStartAsk => {
                if pkt.payload.len() >= 42 {
                    let md5_str =
                        String::from_utf8_lossy(&pkt.payload[..32]).to_string();
                    let mut cursor = Cursor::new(&pkt.payload[32..]);
                    let file_size =
                        ReadBytesExt::read_u64::<LittleEndian>(&mut cursor)?;
                    let file_type =
                        ReadBytesExt::read_u16::<LittleEndian>(&mut cursor)?;
                    let filename = String::from_utf8_lossy(&pkt.payload[42..])
                        .trim_end_matches('\0')
                        .to_string();
                    info!(
                        "BoxStream file start: {} ({} bytes, type {})",
                        filename, file_size, file_type
                    );
                    session.start_file_transfer(filename, file_size, file_type, md5_str);
                }
                let mut resp = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                WriteBytesExt::write_u64::<LittleEndian>(&mut resp, 0).unwrap();
                stream
                    .write_all(
                        &Packet::new(Command::FileStartAnswer, resp).to_bytes(),
                    )
                    .await?;
            }

            Command::FileContentAsk => {
                const OFFSET_LEN: usize = 8;
                if pkt.payload.len() > OFFSET_LEN {
                    session.append_file_data(&pkt.payload[OFFSET_LEN..]);
                } else if !pkt.payload.is_empty() {
                    session.append_file_data(&pkt.payload);
                }
                let mut resp = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                stream
                    .write_all(
                        &Packet::new(Command::FileContentAnswer, resp).to_bytes(),
                    )
                    .await?;
            }

            Command::FileEndAsk => {
                let result: u32 =
                    if let Some(transfer) = session.complete_file_transfer() {
                        let md5_ok = if !transfer.md5.is_empty() {
                            let computed =
                                format!("{:x}", md5::compute(&transfer.data));
                            if computed != transfer.md5.to_lowercase() {
                                warn!(
                                    "BoxStream MD5 mismatch for {}: expected={} computed={}",
                                    transfer.filename, transfer.md5, computed
                                );
                                false
                            } else {
                                true
                            }
                        } else {
                            true
                        };
                        let dest =
                            std::path::Path::new(&program_dir).join(&transfer.filename);
                        info!(
                            "BoxStream file complete: {} ({} bytes, md5_ok={})",
                            transfer.filename,
                            transfer.data.len(),
                            md5_ok
                        );
                        let _ = std::fs::create_dir_all(&program_dir);
                        let _ = std::fs::write(&dest, &transfer.data);
                        0u32
                    } else {
                        0u32
                    };
                let mut resp = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, result).unwrap();
                stream
                    .write_all(
                        &Packet::new(Command::FileEndAnswer, resp).to_bytes(),
                    )
                    .await?;
            }

            other => {
                warn!(
                    "BoxStream: unexpected command {:?} (0x{:04X})",
                    other,
                    other as u16
                );
            }
        }
    }
}

// ── BoxStream packet helpers ─────────────────────────────────────────────────

/// Read the next complete packet from the TCP stream, accumulating data in `buf`.
async fn bs_recv(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<Packet> {
    loop {
        if buf.len() >= 4 {
            match Packet::from_bytes(buf) {
                Ok(Some((pkt, consumed))) => {
                    buf.drain(..consumed);
                    return Ok(pkt);
                }
                Ok(None) => {}
                Err(e) => {
                    return Err(anyhow::anyhow!("BoxStream packet framing error: {}", e))
                }
            }
        }
        let mut tmp = [0u8; 8192];
        let n = stream.read(&mut tmp).await?;
        if n == 0 {
            return Err(anyhow::anyhow!("BoxStream connection closed by peer"));
        }
        buf.extend_from_slice(&tmp[..n]);
    }
}

/// Like `bs_recv` but silently discards heartbeat packets.
async fn bs_recv_skip_hb(stream: &mut TcpStream, buf: &mut Vec<u8>) -> Result<Packet> {
    loop {
        let pkt = bs_recv(stream, buf).await?;
        if pkt.command != Command::TcpHeartbeatAsk
            && pkt.command != Command::TcpHeartbeatAnswer
        {
            return Ok(pkt);
        }
        debug!("BoxStream: skipped {:?} mid-exchange", pkt.command);
    }
}

/// Read packets until `expected` is received, skipping all heartbeats.
async fn bs_skip_to(
    stream: &mut TcpStream,
    buf: &mut Vec<u8>,
    expected: Command,
) -> Result<Packet> {
    loop {
        let pkt = bs_recv(stream, buf).await?;
        if pkt.command == expected {
            return Ok(pkt);
        }
        if pkt.command != Command::TcpHeartbeatAsk
            && pkt.command != Command::TcpHeartbeatAnswer
        {
            debug!(
                "BoxStream: got {:?} while waiting for {:?}, skipping",
                pkt.command, expected
            );
        }
    }
}
