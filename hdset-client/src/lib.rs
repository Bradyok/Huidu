//! # hdset
//!
//! Rust reproduction of Huidu HDSet V4.0.16.0 — the PC-side LED hardware
//! configuration tool.
//!
//! HDSet configures how the display hardware works: FPGA scan parameters,
//! driver chip selection, grayscale depth, refresh rates, colour correction,
//! firmware flashing, and direct USB/serial communication with sender cards.
//!
//! | Tool     | Purpose                                                   |
//! |----------|-----------------------------------------------------------|
//! | hdplayer | Upload & manage display programs (what to show)           |
//! | hdset    | Configure LED hardware parameters (how hardware works)   |
//!
//! ## Key types
//!
//! - [`hw_config::BoxHwConfig`] — complete FPGA hardware configuration
//! - [`client::HdsetClient`] — TCP connection to a Huidu device
//! - [`discovery::DeviceInfo`] — device discovered via UDP broadcast

pub mod client;
pub mod discovery;
pub mod error;
pub mod hw_config;
pub mod protocol;
pub mod transfer;
pub mod xml;

pub use client::{HdsetClient, DeviceDetails, EthInfo};
pub use discovery::{Discovery, DeviceInfo};
pub use error::{Error, Result};
pub use hw_config::{BoxHwConfig, CardConfig};
