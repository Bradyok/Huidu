/// FPGA hardware parameters — reconstructed from libFPGADriver.so binary analysis
/// and the BoxHwConfig XML format used by the Huidu SDK.
///
/// These structures mirror HFPGAParam, HSendCard, HRecvCard, etc. in the original.

/// LED module / receive-card scan type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ScanType {
    Static = 0,
    Half    = 1,  // 1/2 scan
    Quarter = 2,  // 1/4 scan
    Eighth  = 3,  // 1/8 scan
    Sixteenth = 4, // 1/16 scan
    ThirtySecond = 5, // 1/32 scan
}

impl Default for ScanType {
    fn default() -> Self {
        ScanType::Sixteenth
    }
}

/// Data polarity (OE / LAT active level)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Polarity {
    #[default]
    ActiveHigh,
    ActiveLow,
}

/// Receiving-card (LED module) configuration.
/// Maps to BoxHwConfig/CardInfo/Card XML element.
#[derive(Debug, Clone)]
pub struct RecvCardParam {
    /// Module pixel width
    pub cell_width: u16,
    /// Module pixel height
    pub cell_height: u16,
    /// Number of scan rows (1/N scan → N rows per output pin)
    pub cell_scan_rows: u8,
    /// Scan type
    pub scan_type: ScanType,
    /// Drive chip type index (0 = generic shift register)
    pub drive_chip_type: u8,
    /// Data polarity
    pub data_polarity: Polarity,
    /// OE (output enable) polarity
    pub oe_polarity: Polarity,
    /// Pixel clock frequency (MHz)
    pub frequency: u16,
    /// Gray-scale level (8 / 16)
    pub gray_level: u8,
    /// Target refresh rate (Hz)
    pub refresh_rate: u16,
}

impl Default for RecvCardParam {
    fn default() -> Self {
        Self {
            cell_width: 32,
            cell_height: 16,
            cell_scan_rows: 8,
            scan_type: ScanType::default(),
            drive_chip_type: 0,
            data_polarity: Polarity::ActiveHigh,
            oe_polarity: Polarity::ActiveLow,
            frequency: 12,
            gray_level: 16,
            refresh_rate: 60,
        }
    }
}

/// Send-card (FPGA) configuration.
/// Controls the top-level display geometry and gamma correction.
#[derive(Debug, Clone)]
pub struct SendCardParam {
    /// Total display width (pixels)
    pub width: u32,
    /// Total display height (pixels)
    pub height: u32,
    /// Global brightness (0–100)
    pub brightness: u8,
    /// Screen rotation (0 / 90 / 180 / 270)
    pub rotation: u16,
    /// Red gamma correction table (256 entries) — None = identity
    pub gamma_r: Option<Vec<u8>>,
    /// Green gamma correction table
    pub gamma_g: Option<Vec<u8>>,
    /// Blue gamma correction table
    pub gamma_b: Option<Vec<u8>>,
    /// Red channel gain (0–100, default 100)
    pub red_correction: u8,
    /// Green channel gain
    pub green_correction: u8,
    /// Blue channel gain
    pub blue_correction: u8,
}

impl Default for SendCardParam {
    fn default() -> Self {
        Self {
            width: 128,
            height: 64,
            brightness: 100,
            rotation: 0,
            gamma_r: None,
            gamma_g: None,
            gamma_b: None,
            red_correction: 100,
            green_correction: 100,
            blue_correction: 100,
        }
    }
}

/// Full FPGA configuration bundle
#[derive(Debug, Clone, Default)]
pub struct FpgaConfig {
    pub send_card: SendCardParam,
    pub recv_cards: Vec<RecvCardParam>,
    /// Dual-scan table (s_dualScanTab in binary — maps logical rows to physical)
    pub dual_scan_tab: Vec<u8>,
    /// Gray-priority table (s_grayPriority — BCM bit ordering)
    pub gray_priority: Vec<u8>,
    /// Light-priority table (s_lightPriority — brightness stepping)
    pub light_priority: Vec<u8>,
    /// Refresh-priority table (s_refreshPriority)
    pub refresh_priority: Vec<u8>,
}

impl FpgaConfig {
    /// Parse from a BoxHwConfig XML string (subset of fields).
    pub fn from_xml(xml: &str) -> Self {
        let mut cfg = Self::default();

        // Helper: extract a u32 from an XML attribute
        fn attr_u32(xml: &str, elem: &str, attr: &str) -> Option<u32> {
            let tag = format!("<{}", elem);
            let pos = xml.find(&tag)?;
            let a = format!("{}=\"", attr);
            let ap = xml[pos..].find(&a)?;
            let s = pos + ap + a.len();
            let e = xml[s..].find('"')? + s;
            xml[s..e].parse().ok()
        }

        if let Some(w) = attr_u32(xml, "Card", "width") {
            cfg.send_card.width = w;
        }
        if let Some(h) = attr_u32(xml, "Card", "height") {
            cfg.send_card.height = h;
        }
        if let Some(b) = attr_u32(xml, "Brightness", "value")
            .or_else(|| attr_u32(xml, "Brightness", "v"))
        {
            cfg.send_card.brightness = b.min(100) as u8;
        }
        if let Some(rr) = attr_u32(xml, "RefreshRate", "value") {
            for card in &mut cfg.recv_cards {
                card.refresh_rate = rr as u16;
            }
        }

        cfg
    }
}
