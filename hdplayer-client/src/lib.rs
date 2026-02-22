//! # hdplayer-client
//!
//! Rust reproduction of Huidu HDPlayer control software.
//!
//! HDPlayer is the Windows PC-side application that communicates with Huidu
//! BoxPlayer devices (LED display controllers running on Linux ARM hardware).
//!
//! ## Architecture
//!
//! This library implements the full client-side protocol:
//! - UDP device discovery (port 9527)
//! - TCP command protocol (default port 10001)
//! - SDK XML command set (70+ methods)
//! - Binary file transfer protocol
//!
//! ## Quick Start
//!
//! ```rust,no_run
//! use hdplayer_client::{Client, Discovery};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Discover devices on the local network
//!     let devices = Discovery::scan(std::time::Duration::from_secs(3)).await?;
//!
//!     for device in &devices {
//!         println!("Found: {} @ {}", device.name, device.addr);
//!     }
//!
//!     // Connect to first device
//!     if let Some(device) = devices.first() {
//!         let mut client = Client::connect(&device.addr.to_string(), 10001).await?;
//!         let info = client.get_device_info().await?;
//!         println!("Device: {:?}", info);
//!     }
//!
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
