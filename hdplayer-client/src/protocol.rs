//! Huidu TCP binary protocol layer.
//!
//! ## Packet Format
//! ```text
//! [u16 length]   — total bytes including the command word (payload_len + 2)
//! [u16 command]  — command code
//! [N bytes]      — payload (length - 2 bytes)
//! ```

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use crate::error::{Error, Result};

/// All known command codes in the Huidu protocol.
/// Codes marked 0x???? are inferred from binary analysis; the exact values
/// for many non-SDK commands are not yet confirmed from wire captures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Command {
    // ── Core connection ───────────────────────────────────────────────────
    TcpHeartbeatAsk    = 0x005F,
    TcpHeartbeatAnswer = 0x0060,

    // ── SDK XML command channel ───────────────────────────────────────────
    SdkServiceAsk    = 0x2001,
    SdkServiceAnswer = 0x2002,
    SdkCmdAsk        = 0x2003,
    SdkCmdAnswer     = 0x2004,

    // ── File transfer ─────────────────────────────────────────────────────
    FileStartAsk      = 0x8001,
    FileStartAnswer   = 0x8002,
    FileContentAsk    = 0x8003,
    FileContentAnswer = 0x8004,
    FileEndAsk        = 0x8005,
    FileEndAnswer     = 0x8006,

    // ── Remaining commands (numeric values TBD from wire capture) ─────────
    // These names come from HCatNet.dll string analysis.
    Unknown           = 0xFFFF,
}

impl Command {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x005F => Self::TcpHeartbeatAsk,
            0x0060 => Self::TcpHeartbeatAnswer,
            0x2001 => Self::SdkServiceAsk,
            0x2002 => Self::SdkServiceAnswer,
            0x2003 => Self::SdkCmdAsk,
            0x2004 => Self::SdkCmdAnswer,
            0x8001 => Self::FileStartAsk,
            0x8002 => Self::FileStartAnswer,
            0x8003 => Self::FileContentAsk,
            0x8004 => Self::FileContentAnswer,
            0x8005 => Self::FileEndAsk,
            0x8006 => Self::FileEndAnswer,
            _ => Self::Unknown,
        }
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

/// A decoded protocol packet.
#[derive(Debug, Clone)]
pub struct Packet {
    pub command: Command,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(command: Command, payload: Vec<u8>) -> Self {
        Self { command, payload }
    }

    pub fn heartbeat() -> Self {
        Self::new(Command::TcpHeartbeatAsk, vec![])
    }

    /// Serialize to wire format: [u16 len][u16 cmd][payload]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.payload.len());
        let length = (self.payload.len() + 2) as u16;
        buf.write_u16::<LittleEndian>(length).unwrap();
        buf.write_u16::<LittleEndian>(self.command.as_u16()).unwrap();
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Parse from wire bytes. Returns (packet, bytes_consumed).
    pub fn from_bytes(data: &[u8]) -> Result<Option<(Packet, usize)>> {
        if data.len() < 4 {
            return Ok(None); // Need more data
        }
        let mut cur = Cursor::new(data);
        let length = cur.read_u16::<LittleEndian>()
            .map_err(|e| Error::Protocol(e.to_string()))? as usize;
        if length < 2 {
            return Err(Error::Protocol("packet length < 2".into()));
        }
        let payload_len = length - 2;
        if data.len() < 4 + payload_len {
            return Ok(None); // Need more data
        }
        let command_raw = cur.read_u16::<LittleEndian>()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let command = Command::from_u16(command_raw);
        let payload = data[4..4 + payload_len].to_vec();
        Ok(Some((Packet { command, payload }, 4 + payload_len)))
    }
}

/// SDK service negotiation payload (kSDKServiceAsk).
/// Sent once at connection start to agree on protocol version.
pub fn sdk_service_ask_payload(version: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(4);
    v.write_u32::<LittleEndian>(version).unwrap();
    v
}

/// File transfer start payload (kFileStartAsk).
///
/// Wire format (confirmed from server.rs):
///   [32 bytes] MD5 hex string (null-padded to exactly 32 bytes)
///   [ 8 bytes] file size (u64 little-endian)
///   [ 2 bytes] file type (u16 little-endian; 0 = generic)
///   [ N bytes] filename (null-terminated UTF-8)
pub fn file_start_payload(filename: &str, total_size: u64, md5: &str) -> Vec<u8> {
    let mut v = Vec::new();
    // MD5 hex: exactly 32 bytes, zero-padded
    let mut md5_buf = [0u8; 32];
    let md5_bytes = md5.as_bytes();
    let copy_len = md5_bytes.len().min(32);
    md5_buf[..copy_len].copy_from_slice(&md5_bytes[..copy_len]);
    v.extend_from_slice(&md5_buf);
    // File size (u64 LE)
    v.write_u64::<LittleEndian>(total_size).unwrap();
    // File type (u16 LE; 0 = generic binary)
    v.write_u16::<LittleEndian>(0).unwrap();
    // Filename: null-terminated
    v.extend_from_slice(filename.as_bytes());
    v.push(0);
    v
}

/// File content chunk payload (kFileContentAsk).
pub fn file_content_payload(offset: u64, data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(8 + data.len());
    v.write_u64::<LittleEndian>(offset).unwrap();
    v.extend_from_slice(data);
    v
}
