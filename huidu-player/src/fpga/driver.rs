/// FpgaDriver — high-level FPGA interface.
///
/// Ties together the serial transport (`FpgaSerial`) and hardware parameters
/// (`FpgaConfig`) to provide the rendering engine with a simple API:
///
///   - `send_frame(rgba, width, height)` — push a rendered frame to the LED panels
///   - `set_brightness(level)`            — hardware brightness (0–100)
///   - `apply_config(cfg)`                — update scan tables, gamma, etc.
///
/// When the FPGA serial device is unavailable (e.g. during development on a
/// PC) the driver silently no-ops rather than panicking.
use tracing::{debug, info, warn};

use super::params::FpgaConfig;
use super::serial::FpgaSerial;

pub struct FpgaDriver {
    serial: Option<FpgaSerial>,
    config: FpgaConfig,
    fpga_version: String,
}

impl FpgaDriver {
    /// Open the FPGA serial device.  Returns a driver even if no FPGA is
    /// present — in that case all operations are silently no-ops.
    pub fn open() -> Self {
        match FpgaSerial::open() {
            Ok(mut serial) => {
                let ver = serial.get_version().unwrap_or_else(|_| "?.?.?".to_string());
                info!("FPGA firmware version: {}", ver);
                Self {
                    serial: Some(serial),
                    config: FpgaConfig::default(),
                    fpga_version: ver,
                }
            }
            Err(e) => {
                warn!("FPGA unavailable ({}). Pixel output disabled.", e);
                Self {
                    serial: None,
                    config: FpgaConfig::default(),
                    fpga_version: "N/A".to_string(),
                }
            }
        }
    }

    pub fn is_available(&self) -> bool {
        self.serial.is_some()
    }

    pub fn fpga_version(&self) -> &str {
        &self.fpga_version
    }

    /// Push a rendered RGBA framebuffer to the LED panels via the FPGA.
    pub fn send_frame(&mut self, rgba: &[u8], width: u32, height: u32) {
        if let Some(ref mut serial) = self.serial {
            if let Err(e) = serial.send_frame(rgba, width, height) {
                warn!("FPGA send_frame error: {}", e);
            } else {
                debug!("FPGA frame sent ({}x{})", width, height);
            }
        }
    }

    /// Set hardware brightness (0–100).
    pub fn set_brightness(&mut self, level: u8) {
        if let Some(ref mut serial) = self.serial
            && let Err(e) = serial.set_brightness(level) {
                warn!("FPGA set_brightness error: {}", e);
            }
        self.config.send_card.brightness = level;
    }

    /// Apply a new hardware configuration (e.g. from SetBoxHwConfig command).
    pub fn apply_config(&mut self, cfg: FpgaConfig) {
        self.config = cfg;
        // TODO: send config registers to FPGA over serial once protocol is known
        info!(
            "FPGA config applied ({}x{}, brightness {}%)",
            self.config.send_card.width,
            self.config.send_card.height,
            self.config.send_card.brightness
        );
    }

    pub fn config(&self) -> &FpgaConfig {
        &self.config
    }
}
