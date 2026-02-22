//! HDSet GUI — egui-based hardware configuration interface.
//!
//! Build:  cargo build --features gui --bin hdset-gui
//! Run:    ./target/debug/hdset-gui
//!
//! This GUI reproduces the main panels of HDSet V4.0.16.0:
//!   - Device discovery & connection
//!   - BoxHwConfig editor (scan mode, chip type, refresh rate, etc.)
//!   - Screen test controls
//!   - Boot logo management
//!   - Brightness & screen power
//!   - Firmware upgrade

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // TODO: Implement egui GUI for HDSet hardware configuration.
    // This stub exists to satisfy the [[bin]] declaration in Cargo.toml.
    // See hdplayer-client/src/bin/hdplayer_gui.rs for an example of the
    // egui application structure used in this project.
    eprintln!("hdset-gui: GUI not yet implemented. Use hdset-client CLI.");
    std::process::exit(1);
}
