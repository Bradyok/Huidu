//! File transfer — thin wrapper around `huidu_protocol::transfer`.
pub use huidu_protocol::transfer::{
    CHUNK_SIZE, ProgressFn,
    md5_hex,
    build_start_packet, build_content_packet, build_end_packet,
    FileTransfer,
    TransferStatus, TransferLogEntry,
};
