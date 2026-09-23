//! `huidu-sender` — control the Huidu LED sender/receiving cards over `/dev/ttyS1`.
//!
//! This is the FPGA *control* link that BoxPlayer uses (scan tables, gamma, brightness,
//! geometry, card params, status). Pixels do NOT go over this link — they reach the LED
//! panel as the SoC VOP's parallel-RGB888 output. So a replacement player = "any KMS
//! client that produces the right RGB frame" + this daemon driving ttyS1.
//!
//! Reverse-engineered in `products/BoxPlayer/v7.11.18.0/PLAYER_PIPELINE.md` §2.
//! Modeled on NovaOS's `a200-sender` (a UART daemon for a non-NovaStar LED sender).
//!
//! Layers:
//! - [`frame`]    — the 8-byte preamble + payload + CRC-32 wire frame.
//! - [`protocol`] — the 9-byte sub-header, function codes, and command builders.
//! - [`serial`]   — the raw `/dev/ttyS1` port.
//! - [`blob`]     — the 512-byte param blobs (captured or relinked from libFPGADriver).
//! - [`fpga_load`]— our `write_fpga` replacement: parse `/boot/fpga.img` + the
//!   passive-serial load sequence (the LED FPGA bitstream).
//! - [`fpga_io`]  — spidev + sysfs-GPIO backend for `fpga_load` (Unix).

pub mod blob;
pub mod fpga_load;
pub mod frame;
pub mod protocol;

#[cfg(unix)]
pub mod fpga_io;
#[cfg(unix)]
pub mod serial;
