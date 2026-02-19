/// TCP protocol server — accepts connections from HDPlayer software.
/// Implements the Huidu SDK TCP protocol for receiving programs and commands.
use anyhow::Result;
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::core::player::PlayerCommand;
use crate::protocol::command;
use crate::protocol::session::Session;

// Protocol constants (from binary analysis)
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

/// Run the TCP protocol server
pub async fn run(
    port: u16,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: String,
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
                tokio::spawn(async move {
                    if let Err(e) = handle_connection(stream, tx, dir).await {
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
) -> Result<()> {
    let mut session = Session::new();
    let mut buf = vec![0u8; MAX_PACKET_SIZE];

    loop {
        // Read packet: [length: u16 LE] [command: u16 LE] [data...]
        let length = match stream.read_u16_le().await {
            Ok(l) => l as usize,
            Err(_) => break, // Connection closed
        };

        if length < 2 || length > MAX_PACKET_SIZE {
            warn!("Invalid packet length: {}", length);
            break;
        }

        // Length includes the command bytes
        let data_len = length - 2;
        let cmd = stream.read_u16_le().await?;

        // Read remaining data
        if data_len > 0 {
            if data_len > buf.len() {
                buf.resize(data_len, 0);
            }
            stream.read_exact(&mut buf[..data_len]).await?;
        }

        // Handle command
        let response = match cmd {
            CMD_TCP_HEARTBEAT_ASK => {
                // Heartbeat — respond immediately
                Some(make_packet(CMD_TCP_HEARTBEAT_ANSWER, &[]))
            }
            CMD_SDK_SERVICE_ASK => {
                // Transport version negotiation
                let mut resp_data = Vec::new();
                WriteBytesExt::write_u32::<LittleEndian>(&mut resp_data, TRANSPORT_VERSION)
                    .unwrap();
                Some(make_packet(CMD_SDK_SERVICE_ANSWER, &resp_data))
            }
            CMD_SDK_CMD_ASK => {
                // SDK XML command
                if data_len >= 8 {
                    let mut cursor = Cursor::new(&buf[..data_len]);
                    let total_len =
                        ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)? as usize;
                    let index =
                        ReadBytesExt::read_u32::<LittleEndian>(&mut cursor)? as usize;
                    let xml_chunk = &buf[8..data_len];

                    // Accumulate XML data (may span multiple packets)
                    session.accumulate_xml(xml_chunk, total_len, index);

                    if session.xml_complete() {
                        let xml = session.take_xml();
                        let xml_str = String::from_utf8_lossy(&xml);
                        info!("Received SDK command ({} bytes)", xml_str.len());

                        match command::handle_sdk_command(
                            &xml_str,
                            &session,
                            &player_tx,
                            &program_dir,
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
                        None // Still accumulating
                    }
                } else {
                    warn!("SDK command too short: {} bytes", data_len);
                    None
                }
            }
            CMD_FILE_START_ASK => {
                // File transfer start
                if data_len >= 42 {
                    let md5_str = String::from_utf8_lossy(&buf[..32]).to_string();
                    let mut cursor = Cursor::new(&buf[32..]);
                    let file_size =
                        ReadBytesExt::read_u64::<LittleEndian>(&mut cursor)?;
                    let file_type =
                        ReadBytesExt::read_u16::<LittleEndian>(&mut cursor)?;
                    let filename_bytes = &buf[42..data_len];
                    let filename = String::from_utf8_lossy(filename_bytes)
                        .trim_end_matches('\0')
                        .to_string();

                    info!(
                        "File transfer start: {} ({} bytes, type {}, md5={})",
                        filename, file_size, file_type, md5_str
                    );

                    session.start_file_transfer(filename, file_size, file_type, md5_str);

                    // Respond with error=0, existSize=0
                    let mut resp = Vec::new();
                    WriteBytesExt::write_u32::<LittleEndian>(&mut resp, 0).unwrap();
                    WriteBytesExt::write_u64::<LittleEndian>(&mut resp, 0).unwrap();
                    Some(make_packet(CMD_FILE_START_ANSWER, &resp))
                } else {
                    warn!("File start packet too short");
                    None
                }
            }
            CMD_FILE_CONTENT_ASK => {
                // File content chunk
                if data_len > 0 {
                    session.append_file_data(&buf[..data_len]);
                }
                None // No response for content packets
            }
            CMD_FILE_END_ASK => {
                // File transfer complete
                if let Some(transfer) = session.complete_file_transfer() {
                    let dest_path =
                        std::path::Path::new(&program_dir).join(&transfer.filename);
                    info!(
                        "Saving file: {} ({} bytes) -> {}",
                        transfer.filename,
                        transfer.data.len(),
                        dest_path.display()
                    );

                    if let Err(e) = std::fs::create_dir_all(&program_dir) {
                        warn!("Failed to create dir {}: {}", program_dir, e);
                    }
                    if let Err(e) = std::fs::write(&dest_path, &transfer.data) {
                        warn!("Failed to write file: {}", e);
                    }
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

/// Build a response packet: [length: u16 LE] [command: u16 LE] [data...]
fn make_packet(cmd: u16, data: &[u8]) -> Vec<u8> {
    let length = (data.len() + 2) as u16; // +2 for the command field
    let mut packet = Vec::with_capacity(4 + data.len());
    WriteBytesExt::write_u16::<LittleEndian>(&mut packet, length).unwrap();
    WriteBytesExt::write_u16::<LittleEndian>(&mut packet, cmd).unwrap();
    packet.extend_from_slice(data);
    packet
}
