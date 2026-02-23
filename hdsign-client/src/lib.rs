//! # hdsign
//!
//! Rust reproduction of Huidu HDSign V2.0.2 — the PC-side tool for ultra-cheap
//! LED display cards (HD-U/W/E/S/A series).
//!
//! Unlike HDPlayer (LAN Ethernet only), HDSign supports multiple connectivity
//! modes: serial, USB, WiFi AP, and Ethernet — encoded per-card as a bitmask
//! in `CardList.xml`.
//!
//! ## Connection dispatch
//!
//! | `Communications` bit | Mode      | Protocol                      |
//! |----------------------|-----------|-------------------------------|
//! | bit3 (value 4)       | Ethernet  | TCP port 10001 (HDPlayer SDK) |
//! | bit4 (value 8)       | WiFi AP   | HTTP port 6104                |
//! | bit1 (value 1)       | Serial    | UART (pending wire capture)   |
//! | bit2 (value 2)       | USB       | File copy (pending)           |

pub mod client;
pub mod discovery;
pub mod error;
pub mod firmware;
pub mod hardware;
pub mod protocol;
pub mod transfer;
pub mod weather;
pub mod xml;

pub use client::{HdsignClient, TcpClient, HttpClient, DeviceDetails, ProgramInfo, FileEntry, EthConfig};
pub use discovery::{DeviceInfo, Discovery, WIFI_AP_DEFAULT_IP, WIFI_AP_HTTP_PORT};
pub use error::Error;
pub use firmware::{FirmwareEntry, FirmwareVersion, find_firmware, load_firmware_catalog};
pub use hardware::{HardwareCard, load_cards};
