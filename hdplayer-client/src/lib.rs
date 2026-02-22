//! # hdplayer
//!
//! Rust reproduction of Huidu HDPlayer — the PC-side LED content management tool.
//!
//! HDPlayer manages what the display shows: uploading programs, switching content,
//! adjusting brightness/rotation, managing files, and monitoring device status.
//!
//! ## Architecture
//!
//! - UDP device discovery (port 9527) — via `huidu_protocol::discovery`
//! - TCP command protocol (port 10001) — via `huidu_protocol::packet`
//! - SDK XML command set (70+ methods) — see `command` module
//! - Binary file transfer — via `huidu_protocol::transfer`
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use hdplayer::{Client, Discovery};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let devices = Discovery::scan(std::time::Duration::from_secs(3)).await?;
//!     for device in &devices {
//!         println!("Found: {} @ {}", device.name, device.addr);
//!     }
//!     if let Some(device) = devices.first() {
//!         let mut client = Client::connect(&device.addr.to_string(), 10001).await?;
//!         let info = client.get_device_info().await?;
//!         println!("Device: {:?}", info);
//!     }
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod command;
pub mod discovery;
pub mod error;
pub mod protocol;
pub mod transfer;
pub mod xml;

pub use client::{Client, DeviceDetails, EthConfig, FileEntry, ProgramInfo};
pub use discovery::{DeviceInfo, Discovery};
pub use error::Error;
