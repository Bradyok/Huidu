/// TCP protocol server — accepts connections from HDPlayer software.
use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use crate::core::player::PlayerCommand;
use crate::protocol::command;
use crate::protocol::session::Session;
use crate::services::manager::ServicesState;

const CMD_TCP_HEARTBEAT_ASK: u16 = 0x005F;
const CMD_TCP_HEARTBEAT_ANSWER: u16 = 0x0060;
const CMD_SDK_SERVICE_ASK: u16 = 0x2001;
const CMD_SDK_SERVICE_ANSWER: u16 = 0x2002;
const CMD_SDK_CMD_ASK: u16 = 0x2003;
const CMD_SDK_CMD_ANSWER: u16 = 0x2004;
const CMD_FILE_START_ASK: u16 = 0x8001;
const CMD_FILE_START_ANSWER: u16 = 0x8002;
const CMD_FILE_CONTENT_ASK: u16 = 0x8003;
const CMD_FILE_END_ASK: u16 = 0x8005;
const CMD_FILE_END_ANSWER: u16 = 0x8006;

const TRANSPORT_VERSION: u32 = 0x0100_0005;
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

    loop {
        let length = match stream.read_u16_le().await {
            Ok(l) => l as usize,
            Err(_) => break,
        };

        if length < 2 || length > MAX_PACKET_SIZE {
            warn!("Invalid packet length: {}", length);
            break;
        }

        let data_len = length - 2;
        let cmd = stream.read_u16_le().await?;

        if data_len > 0 {
            if data_len > buf.len() {
                buf.resize(data_len, 0);
            }
            stream.read_exact(&mut buf[..data_len]).await?;
        }

        let response = match cmd {
            CMD_TCP_HEARTBEAT_ASK => Some(make_packet(CMD_TCP_HEARTBEAT_ANSWER, &[])),

            CMD_SDK_SERVICE_ASK => {
                let mut resp_data = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp_data, TRANSPORT_VERSION)
                    .unwrap();
                Some(make_packet(CMD_SDK_SERVICE_ANSWER, &resp_data))
            }

            CMD_SDK_CMD_ASK => {
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
                                let xml_bytes = response_xml.as_bytes();
                                let mut resp = Vec::new();
                                WriteBytesExt::write_u32::<LittleEndian>(
                                    &mut resp,
                                    xml_bytes.len() as u32,
                                )
                                .unwrap();
                                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                                resp.extend_from_slice(xml_bytes);
                                Some(make_packet(CMD_SDK_CMD_ANSWER, &resp))
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

            CMD_FILE_START_ASK => {
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

                    let mut resp = Vec::new();
                    WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                    WriteBytesExt::write_u64::<LittleEndian>(&mut resp, 0).unwrap();
                    Some(make_packet(CMD_FILE_START_ANSWER, &resp))
                } else {
                    None
                }
            }

            CMD_FILE_CONTENT_ASK => {
                if data_len > 0 {
                    session.append_file_data(&buf[..data_len]);
                }
                None
            }

            CMD_FILE_END_ASK => {
                if let Some(transfer) = session.complete_file_transfer() {
                    let dest = std::path::Path::new(&program_dir).join(&transfer.filename);
                    info!("File saved: {} ({} bytes)", transfer.filename, transfer.data.len());
                    let _ = std::fs::create_dir_all(&program_dir);
                    let _ = std::fs::write(&dest, &transfer.data);
                }
                let mut resp = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                Some(make_packet(CMD_FILE_END_ANSWER, &resp))
            }

            _ => {
                warn!("Unknown command: 0x{:04X}", cmd);
                None
            }
        };

        if let Some(resp) = response {
            stream.write_all(&resp).await?;
        }
    }

    Ok(())
}

fn make_packet(cmd: u16, data: &[u8]) -> Vec<u8> {
    let length = (data.len() + 2) as u16;
    let mut packet = Vec::with_capacity(4 + data.len());
    WriteBytesExt::write_u16::<LittleEndian>(&mut packet, length).unwrap();
    WriteBytesExt::write_u16::<LittleEndian>(&mut packet, cmd).unwrap();
    packet.extend_from_slice(data);
    packet
}
