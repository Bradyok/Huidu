/// FPGA driver module — interfaces with the Altera Cyclone IV FPGA that
/// drives the LED panel send cards.
///
/// Hardware overview (from binary decompilation of libFPGADriver.so):
///
///   SoC (PX30 / RK3288)
///     │  /dev/ttyS1 (PX30) or /dev/cyclone4-0 (RK3288)  serial
///     ▼
///   Altera Cyclone IV FPGA (on-board "send card")
///     │  proprietary parallel bus
///     ▼
///   HUB75-like receiving cards (one per LED cabinet)
///     │
///     ▼
///   LED modules
///
/// The FPGA communicates via a binary serial protocol that is
/// currently being reverse-engineered (see RUST_REPRODUCTION_PLAN.md).
/// This module provides:
///   - Known data structures (SendCardParam, RecvCardParam, FPGAParam)
///   - Serial transport scaffolding
///   - Pixel data output (stub — actual framing TBD from wire capture)
///   - Hardware configuration (brightness, gamma, scan tables)
pub mod driver;
pub mod params;
pub mod serial;

#[allow(unused_imports)]
pub use driver::FpgaDriver;
#[allow(unused_imports)]
pub use params::{FpgaConfig, RecvCardParam, SendCardParam};
