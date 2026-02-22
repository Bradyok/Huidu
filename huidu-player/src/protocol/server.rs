/// TCP protocol server — accepts connections from HDPlayer / HDSet software.
use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use huidu_protocol::packet::{Command, Packet, SDK_TRANSPORT_VERSION};

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

        if !(2..=MAX_PACKET_SIZE).contains(&length) {
            warn!("Invalid packet length: {}", length);
            break;
        }

        let data_len = length - 2;
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
