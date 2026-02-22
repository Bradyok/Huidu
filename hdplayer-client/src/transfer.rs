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
pub fn build_start_packet(filename: &str, total_size: u64, md5: &str) -> Packet {
    let mut payload = Vec::new();
    payload.extend_from_slice(filename.as_bytes());
    payload.push(0);
    payload.extend_from_slice(&total_size.to_le_bytes());
    payload.extend_from_slice(md5.as_bytes());
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
