//! BoxHwConfig data model — the complete LED hardware configuration.
//!
//! This structure is written to/read from the device via `GetBoxHwConfig` /
//! `SetBoxHwConfig` SDK XML methods. On the device, it is stored as
//! `send_card_cfg.xml` and directly controls the FPGA send-card parameters.
//!
//! Field names and semantics are reverse-engineered from the BoxHwConfig XML
//! tag list in the device firmware (libFPGADriver.so) and the HDSet.exe binary.

use serde::{Deserialize, Serialize};

/// Root hardware configuration.
/// Contains per-card parameters plus global screen geometry.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BoxHwConfig {
    pub card_info: Vec<CardConfig>,
    /// Screen control rectangle (which portion of the physical canvas to display)
    pub netcard_ctrl_rect: Rect,
    /// Mode switch plan (0 = normal)
    pub mode_switch_plan: u32,
    /// Screen rotation: 0=0°, 1=90°, 2=180°, 3=270°
    pub rotation: u32,
    /// Send card operating mode (0 = normal synchronous)
    pub send_card_mode: u32,
    /// Network card control mode
    pub net_card_ctrl_mode: u32,
    /// Async priority mode
    pub async_priority_mode: u32,
    /// Enable send card only (bypass BoxPlayer for direct FPGA control)
    pub en_send_card_only: bool,
    /// Receiver card selection index
    pub recv_card_choose: u32,
    /// RGB output pixel width
    pub rgb_ctrl_width: u32,
    /// RGB output pixel height
    pub rgb_ctrl_height: u32,
    /// Brightness set mode (0 = software, 1 = hardware PWM)
    pub brightness_set_mode: u32,
}

/// Configuration for one sender card (one FPGA channel).
/// The most important parameters are the per-LED-module physical parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardConfig {
    /// Card index (0-based)
    pub index: u32,
    /// LED module type code (module model ID)
    pub module_type: u32,
    /// LED driver chip type code (maps to ICN/SM/MBI/DP/FM/etc.)
    pub drive_chip_type: u32,
    /// Module pixel width (e.g. 32, 64)
    pub cell_width: u32,
    /// Module pixel height (e.g. 16, 32)  [note: "Hight" spelling matches original XML]
    pub cell_height: u32,
    /// Scan rows per module (the denominator of the scan ratio)
    pub cell_scan_row: u32,
    /// Scan mode: 0=static(1:1), 1=1:2, 2=1:4, 3=1:8, 4=1:16, 5=1:32
    pub scan_mode: u32,
    /// More than 16 scan rows flag
    pub more_than_16_scan: bool,
    /// E-signal (4th address signal) enable
    pub e_signal: bool,
    /// 74HC595 shift register variant
    pub chip_595: u32,
    /// 74HC5958 shift register variant
    pub chip_5958: u32,
    /// Address decoding mode (0=138 decoder, 1=595 decoder, etc.)
    pub decoding_mode: u32,
    /// RGB data polarity (0=high active, 1=low active)
    pub data_polarity: u32,
    /// OE signal polarity (0=low active, 1=high active)
    pub oe_polarity: u32,
    /// Signal color ordering: 0=RGB, 1=RBG, 2=GRB, 3=GBR, 4=BRG, 5=BGR
    pub signal_color: u32,
    /// Null pixel count per module cell
    pub cell_null_num: u32,
    /// Gamma lookup table type
    pub look_up_tab: u32,
    /// Target refresh rate in Hz (e.g. 1200, 2400, 3840)
    pub refresh_rate: u32,
    /// R accumulate bits (for ICN2053 and similar chips)
    pub r_acc: u32,
    /// Grayscale levels: 16, 64, 256, 1024, 4096
    pub gray_level: u32,
    /// Luminance levels
    pub luminance_level: u32,
    /// Pixel clock frequency in MHz (e.g. 25, 50)
    pub frequency: u32,
    /// Priority mode: 0=gray, 1=refresh, 2=automatic
    pub priority_mode: u32,
    /// RGB bit depth (exactly one of rgb20/rgb24/rgb28/rgb32 is set to 1)
    pub rgb_bit_depth: RgbBitDepth,
    /// Gamma correction value (0 = default)
    pub gamma_value: u32,
    /// Red channel correction factor (×1000, 1000 = no correction)
    pub red_correction: u32,
    /// Green channel correction factor (×1000)
    pub green_correction: u32,
    /// Blue channel correction factor (×1000)
    pub blue_correction: u32,
    /// OE duty cycle (×1000; 500 = 50%)
    pub duty_cycle: u32,
    /// Blanking value
    pub bv: u32,
    /// Clock phase
    pub phase: u32,
    /// Afterglow reduction value
    pub afterglow: u32,
    /// Oscillation mode
    pub om: u32,
    /// PWM red current
    pub pwm_red_current: u32,
    /// PWM green current
    pub pwm_green_current: u32,
    /// PWM blue current
    pub pwm_blue_current: u32,
    /// S-PWM mode
    pub spwm_mode: u32,
    /// FM-PWM multiplier
    pub fm_pwm_multiplier: u32,
    /// Double refresh rate enable
    pub double_refresh_rate: bool,
    /// GCLK multiplier (1, 2, 4, 8, ...)
    pub gclk_multiplier: u32,
    /// PWM frequency
    pub pwm_frequency: u32,
    /// Frame frequency
    pub f_frame: u32,
    /// Current brightness (0–100)
    pub brightness: u32,
    /// User-defined gamma table (optional)
    pub gamma_tab: Option<Vec<u8>>,
    /// Output definition enable
    pub en_output_definition: bool,
    /// Output definition value
    pub output_definition: u32,
    /// PWM chip type
    pub pwm_chip_type: u32,
    /// PWM IC register values for R/G/B channels (3 regs each)
    pub pwm_ic_red_regs: [u32; 3],
    pub pwm_ic_green_regs: [u32; 3],
    pub pwm_ic_blue_regs: [u32; 3],
}

impl Default for CardConfig {
    fn default() -> Self {
        Self {
            index: 0,
            module_type: 0,
            drive_chip_type: 0,
            cell_width: 32,
            cell_height: 16,
            cell_scan_row: 4,
            scan_mode: 2, // 1:4 scan (common for outdoor P10)
            more_than_16_scan: false,
            e_signal: false,
            chip_595: 0,
            chip_5958: 0,
            decoding_mode: 0,
            data_polarity: 0,
            oe_polarity: 0,
            signal_color: 0,
            cell_null_num: 0,
            look_up_tab: 0,
            refresh_rate: 1200,
            r_acc: 0,
            gray_level: 256,
            luminance_level: 64,
            frequency: 25,
            priority_mode: 2,
            rgb_bit_depth: RgbBitDepth::Rgb24,
            gamma_value: 0,
            red_correction: 1000,
            green_correction: 1000,
            blue_correction: 1000,
            duty_cycle: 500,
            bv: 6,
            phase: 0,
            afterglow: 1,
            om: 0,
            pwm_red_current: 0,
            pwm_green_current: 0,
            pwm_blue_current: 0,
            spwm_mode: 0,
            fm_pwm_multiplier: 0,
            double_refresh_rate: false,
            gclk_multiplier: 4,
            pwm_frequency: 0,
            f_frame: 0,
            brightness: 100,
            gamma_tab: None,
            en_output_definition: false,
            output_definition: 0,
            pwm_chip_type: 0,
            pwm_ic_red_regs: [0; 3],
            pwm_ic_green_regs: [0; 3],
            pwm_ic_blue_regs: [0; 3],
        }
    }
}

/// RGB bit depth mode (exactly one active at a time).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RgbBitDepth {
    Rgb20,
    Rgb24,
    Rgb28,
    Rgb32,
}

impl Default for RgbBitDepth {
    fn default() -> Self { Self::Rgb24 }
}

impl RgbBitDepth {
    pub fn bits_per_channel(&self) -> u32 {
        match self {
            Self::Rgb20 => 20,
            Self::Rgb24 => 24,
            Self::Rgb28 => 28,
            Self::Rgb32 => 32,
        }
    }
}

/// Screen display rectangle.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Known LED driver chip type codes (from recvcardsupportinfo.xml and chip plugin DLLs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveChipType {
    // ICN family
    Icn2037 = 0,
    Icn2038 = 1,
    Icn2045 = 2,
    Icn2053 = 3,
    Icn2163 = 4,
    Icn2165 = 5,
    // SM family
    Sm16207 = 10,
    Sm16227 = 11,
    Sm16259 = 12,
    Sm16289 = 13,
    Sm16359 = 14,
    Sm16389 = 15,
    Sm16395 = 16,
    // MBI family
    Mbi5041 = 20,
    Mbi5153 = 21,
    Mbi5252 = 22,
    Mbi5268 = 23,
    // FM family
    Fm6124 = 30,
    Fm6127 = 31,
    Fm6153 = 32,
    Fm6363 = 33,
    Fm6565 = 34,
    // DP family
    Dp3264 = 40,
    Dp3265 = 41,
    Dp5125 = 42,
    // Other
    Unknown = 255,
}

/// Scan mode — the fraction of rows driven simultaneously.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanMode {
    Static = 0,    // 1:1
    Scan2  = 1,    // 1:2
    Scan4  = 2,    // 1:4
    Scan8  = 3,    // 1:8
    Scan16 = 4,    // 1:16
    Scan32 = 5,    // 1:32
}

impl ScanMode {
    pub fn ratio_str(&self) -> &'static str {
        match self {
            Self::Static => "1:1 (static)",
            Self::Scan2  => "1:2",
            Self::Scan4  => "1:4",
            Self::Scan8  => "1:8",
            Self::Scan16 => "1:16",
            Self::Scan32 => "1:32",
        }
    }
}
