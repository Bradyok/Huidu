//! # huidu-protocol
//!
//! Shared Huidu LED display wire protocol layer.
//!
//! Used by all three Huidu Rust crates:
//! - `hdplayer` — PC content management tool (reproducing Huidu HDPlayer)
//! - `hdset`    — PC hardware configuration tool (reproducing Huidu HDSet)
//! - `huidu-player` — Device-side player (reproducing BoxPlayer for ARM Linux)
//!
//! ## Protocol Overview
//!
//! All TCP communication uses the same binary framing:
//! ```text
//! [u16 LE length]   — total bytes including the command word (payload_len + 2)
//! [u16 LE command]  — command code (see Command enum)
//! [N bytes payload] — command-specific data
//! ```
//!
//! Device discovery uses UDP on port 9527.  File transfer uses a three-phase
//! Start / Content / End sequence over the same TCP connection.

pub mod discovery;
pub mod error;
pub mod packet;
pub mod transfer;
pub mod xml;

// Convenience re-exports at the crate root
pub use error::{Error, Result, DeviceError};
pub use discovery::{udp_register, SessionToken, DeviceInfo};
