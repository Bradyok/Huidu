//! Binary file transfer protocol.
//!
//! Files are transferred in chunks over the TCP connection using the
//! kFileStartAsk / kFileContentAsk / kFileEndAsk command sequence.
//!
//! ## Transfer Flow
//! ```text
//! Client                             Device
//!   |                                  |
//!   |--[kFileStartAsk: name+size+md5]->|
//!   |<-[kFileStartAnswer: result]------|
//!   |--[kFileContentAsk: offset+data]->| (repeat)
//!   |<-[kFileContentAnswer: result]----|
//!   |--[kFileEndAsk]------------------>|
//!   |<-[kFileEndAnswer: result]--------|
//! ```

use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use crate::error::Result;
use crate::protocol::{Command, Packet};

/// Chunk size for file transfers (4 KB — matches UART buffer limit).
pub const CHUNK_SIZE: usize = 4096;

/// File transfer progress callback type.
pub type ProgressFn = Box<dyn Fn(u64, u64) + Send + Sync>;

/// Encodes an MD5 digest to hex string.
pub fn md5_hex(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

/// Build a kFileStartAsk packet.
///
/// Payload format (confirmed from huidu-player server.rs):
///   [32 bytes] MD5 hex string (zero-padded to exactly 32 bytes)
///   [ 8 bytes] file size (u64 little-endian)
///   [ 2 bytes] file type (u16 little-endian; 0 = generic)
///   [ N bytes] filename (null-terminated UTF-8)
pub fn build_start_packet(filename: &str, total_size: u64, md5: &str) -> Packet {
    let mut payload = Vec::new();

    // MD5 hex: exactly 32 bytes, zero-padded
    let mut md5_buf = [0u8; 32];
    let md5_bytes = md5.as_bytes();
    let copy_len = md5_bytes.len().min(32);
    md5_buf[..copy_len].copy_from_slice(&md5_bytes[..copy_len]);
    payload.extend_from_slice(&md5_buf);

    // File size: u64 LE
    payload.extend_from_slice(&total_size.to_le_bytes());

    // File type: u16 LE (0 = generic binary)
    payload.extend_from_slice(&0u16.to_le_bytes());

    // Filename: null-terminated
    payload.extend_from_slice(filename.as_bytes());
    payload.push(0);

    Packet::new(Command::FileStartAsk, payload)
}

/// Build a kFileContentAsk packet for one chunk.
pub fn build_content_packet(offset: u64, chunk: &[u8]) -> Packet {
    let mut payload = Vec::with_capacity(8 + chunk.len());
    payload.extend_from_slice(&offset.to_le_bytes());
    payload.extend_from_slice(chunk);
    Packet::new(Command::FileContentAsk, payload)
}

/// Build a kFileEndAsk packet.
pub fn build_end_packet() -> Packet {
    Packet::new(Command::FileEndAsk, vec![])
}

/// Encapsulates a pending file transfer.
pub struct FileTransfer {
    pub filename: String,
    pub data: Vec<u8>,
    pub md5: String,
}

impl FileTransfer {
    /// Create a transfer from in-memory data.
    pub fn from_data(filename: impl Into<String>, data: Vec<u8>) -> Self {
        let md5 = md5_hex(&data);
        Self {
            filename: filename.into(),
            data,
            md5,
        }
    }

    /// Create a transfer by reading a file from disk.
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let mut file = File::open(path).await?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).await?;
        let md5 = md5_hex(&data);
        Ok(Self { filename, data, md5 })
    }

    /// Return the number of chunks this transfer will be split into.
    pub fn chunk_count(&self) -> usize {
        (self.data.len() + CHUNK_SIZE - 1) / CHUNK_SIZE
    }

    /// Iterate over (offset, chunk) pairs.
    pub fn chunks(&self) -> impl Iterator<Item = (u64, &[u8])> {
        self.data.chunks(CHUNK_SIZE)
            .scan(0u64, |offset, chunk| {
                let off = *offset;
                *offset += chunk.len() as u64;
                Some((off, chunk))
            })
    }

    pub fn total_size(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn start_packet(&self) -> Packet {
        build_start_packet(&self.filename, self.total_size(), &self.md5)
    }

    pub fn end_packet(&self) -> Packet {
        build_end_packet()
    }
}

/// File transfer log entry (;;-separated format used in Huidu protocol).
pub struct TransferLogEntry {
    pub name: String,
    pub size: u64,
    pub md5: String,
    pub status: TransferStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    InProgress = 0,
    Complete = 1,
    Read = 2,
    Delay = 3,
}

impl TransferLogEntry {
    pub fn to_log_string(&self) -> String {
        let status = self.status as u8;
        format!("{};;{};;{};;{}", self.name, self.size, self.md5, status)
    }
}
