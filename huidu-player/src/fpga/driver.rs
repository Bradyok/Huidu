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
        if let Some(ref mut serial) = self.serial {
            // Serialize config into a best-effort binary payload.
            // Format: each register as (id: u8, value: u32 LE) pairs.
            // Register IDs (best-effort from binary analysis):
            //   0x01 = display width     0x02 = display height
            //   0x03 = brightness        0x04 = rotation
            //   0x05 = scan_type         0x06 = pixel_frequency (MHz)
            //   0x07 = gray_level        0x08 = refresh_rate
            //   0x09 = red_correction    0x0A = green_correction
            //   0x0B = blue_correction
            let sc = &self.config.send_card;
            let rv = self.config.recv_cards.first();
            let mut payload: Vec<u8> = Vec::with_capacity(64);
            let mut push_reg = |id: u8, val: u32| {
                payload.push(id);
                payload.extend_from_slice(&val.to_le_bytes());
            };
            push_reg(0x01, sc.width);
            push_reg(0x02, sc.height);
            push_reg(0x03, sc.brightness as u32);
            push_reg(0x04, sc.rotation as u32);
            push_reg(0x09, sc.red_correction as u32);
            push_reg(0x0A, sc.green_correction as u32);
            push_reg(0x0B, sc.blue_correction as u32);
            if let Some(rc) = rv {
                push_reg(0x05, rc.scan_type as u32);
                push_reg(0x06, rc.frequency as u32);
                push_reg(0x07, rc.gray_level as u32);
                push_reg(0x08, rc.refresh_rate as u32);
            }
            if let Err(e) = serial.send_config(&payload) {
                warn!("FPGA apply_config error: {}", e);
            }
        }
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
