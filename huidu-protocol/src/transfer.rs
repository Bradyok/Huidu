//! Binary file transfer protocol — shared by HDPlayer, HDSet, and BoxPlayer.
//!
//! ## Transfer flow (client → device)
//! ```text
//! Client                                   Device
//!   |                                         |
//!   |─[FileStartAsk: MD5 + size + name]──────>|
//!   |<─[FileStartAnswer: result + resume_off]─|
//!   |─[FileContentAsk: offset + chunk]────────| (repeat)
//!   |<─[FileContentAnswer: result]────────────|
//!   |─[FileEndAsk]───────────────────────────>|
//!   |<─[FileEndAnswer: result]────────────────|
//! ```
//!
//! ## FileStartAsk payload layout (confirmed from BoxPlayer server.rs)
//! ```text
//! [32 bytes] MD5 hex string (null-padded, not null-terminated)
//! [ 8 bytes] file size (u64 little-endian)
//! [ 2 bytes] file type (u16 little-endian; 0 = generic binary)
//! [ N bytes] filename (null-terminated UTF-8)
//! ```
//!
//! ## FileStartAnswer payload layout
//! ```text
//! [4 bytes] result code (u32 little-endian; 0 = OK)
//! [8 bytes] resume offset (u64 little-endian; 0 = start from beginning)
//! ```

use std::path::Path;
use tokio::fs::File;
use tokio::io::AsyncReadExt;
use crate::error::Result;
use crate::packet::{Command, Packet};

/// Default chunk size for file transfers (matches BoxPlayer's 4 KB UART buffer).
pub const CHUNK_SIZE: usize = 4096;

/// File transfer progress callback: called with `(bytes_sent, total_bytes)`.
pub type ProgressFn = Box<dyn Fn(u64, u64) + Send + Sync>;

// ── MD5 helper ────────────────────────────────────────────────────────────────

/// Compute the MD5 digest of `data` and return it as a lowercase hex string.
pub fn md5_hex(data: &[u8]) -> String {
    format!("{:x}", md5::compute(data))
}

// ── Packet builders ───────────────────────────────────────────────────────────

/// Build a `FileStartAsk` packet.
pub fn build_start_packet(filename: &str, total_size: u64, md5: &str) -> Packet {
    let mut payload = Vec::new();

    // MD5 hex: exactly 32 bytes, zero-padded (NOT null-terminated)
    let mut md5_buf = [0u8; 32];
    let src = md5.as_bytes();
    md5_buf[..src.len().min(32)].copy_from_slice(&src[..src.len().min(32)]);
    payload.extend_from_slice(&md5_buf);

    // File size: u64 LE
    payload.extend_from_slice(&total_size.to_le_bytes());

    // File type: u16 LE (0 = generic binary)
    payload.extend_from_slice(&0u16.to_le_bytes());

    // Filename: null-terminated UTF-8
    payload.extend_from_slice(filename.as_bytes());
    payload.push(0);

    Packet::new(Command::FileStartAsk, payload)
}

/// Build a `FileContentAsk` packet for one data chunk.
pub fn build_content_packet(offset: u64, chunk: &[u8]) -> Packet {
    let mut payload = Vec::with_capacity(8 + chunk.len());
    payload.extend_from_slice(&offset.to_le_bytes());
    payload.extend_from_slice(chunk);
    Packet::new(Command::FileContentAsk, payload)
}

/// Build a `FileEndAsk` packet (empty payload).
pub fn build_end_packet() -> Packet {
    Packet::new(Command::FileEndAsk, vec![])
}

// ── High-level FileTransfer struct ────────────────────────────────────────────

/// Encapsulates an in-memory file ready for transfer.
pub struct FileTransfer {
    pub filename: String,
    pub data: Vec<u8>,
    pub md5: String,
}

impl FileTransfer {
    /// Create a transfer from in-memory bytes.  MD5 is computed automatically.
    pub fn from_data(filename: impl Into<String>, data: Vec<u8>) -> Self {
        let md5 = md5_hex(&data);
        Self { filename: filename.into(), data, md5 }
    }

    /// Load a file from disk for transfer.  MD5 is computed automatically.
    pub async fn from_file(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();
        let mut file: File = File::open(path).await?;
        let mut data = Vec::new();
        file.read_to_end(&mut data).await?;
        let md5 = md5_hex(&data);
        Ok(Self { filename, data, md5 })
    }

    pub fn total_size(&self) -> u64 { self.data.len() as u64 }

    pub fn chunk_count(&self) -> usize {
        self.data.len().div_ceil(CHUNK_SIZE)
    }

    /// Iterate over `(byte_offset, chunk_slice)` pairs.
    pub fn chunks(&self) -> impl Iterator<Item = (u64, &[u8])> {
        self.data.chunks(CHUNK_SIZE)
            .scan(0u64, |offset, chunk| {
                let off = *offset;
                *offset += chunk.len() as u64;
                Some((off, chunk))
            })
    }

    pub fn start_packet(&self) -> Packet {
        build_start_packet(&self.filename, self.total_size(), &self.md5)
    }

    pub fn end_packet(&self) -> Packet {
        build_end_packet()
    }
}

// ── Transfer log entry ────────────────────────────────────────────────────────

/// Status of a single file in the Huidu transfer log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferStatus {
    InProgress = 0,
    Complete   = 1,
    Read       = 2,
    Delay      = 3,
}

/// One entry in the `;;`-separated Huidu file transfer log format.
pub struct TransferLogEntry {
    pub name: String,
    pub size: u64,
    pub md5: String,
    pub status: TransferStatus,
}

impl TransferLogEntry {
    /// Serialize as `name;;size;;md5;;status` (Huidu wire format).
    pub fn to_log_string(&self) -> String {
        format!("{};;{};;{};;{}", self.name, self.size, self.md5, self.status as u8)
    }
}
