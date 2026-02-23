//! Hardware card database parsed from bundled CardList.xml.
//!
//! CardList.xml ships with HDSign and enumerates all supported LED controller
//! cards with their capabilities.  The `Communications` bitmask encodes which
//! connection methods the card supports:
//!
//! | Bit | Value | Mode     |
//! |-----|-------|----------|
//! |  1  |   1   | Serial   |
//! |  2  |   2   | USB disk |
//! |  3  |   4   | Ethernet |
//! |  4  |   8   | WiFi AP  |
//! |  5  |  16   | RF       |
//! |  6  |  32   | GPRS     |
//!
//! The `Interface` field is a bitmask of supported LED panel connector types:
//! bit1=HUB75, bit2=50PIN, bit3=HUB08, bit4=HUB12.

const CARD_LIST_XML: &str = include_str!("../../hdsign-extracted/CardList.xml");

/// A single hardware card entry from CardList.xml.
#[derive(Debug, Clone)]
pub struct HardwareCard {
    pub id: u32,
    pub name: String,
    pub max_pixels: u32,
    pub max_width: u32,
    pub max_height: u32,
    /// Connection bitmask: bit1=Serial, bit2=USB, bit3=Ethernet, bit4=WiFi, bit5=RF, bit6=GPRS
    pub communications: u8,
    /// Maximum number of display regions (content areas)
    pub region: u8,
    /// Maximum gray levels (1=mono, 3=4-level, 7=256-level etc.)
    pub max_gray: u8,
    pub default_gray: u8,
    /// Smart Setting type: 0=basic, 1=FPGA-assisted
    pub smart_setting: u8,
    /// Feature flags: bit1=button-switch, bit2=full-color, bit3=custom-options, bit4=no-UI-select
    pub support_flag: u8,
    /// Interface connector bitmask: bit1=HUB75, bit2=50PIN, bit3=HUB08, bit4=HUB12
    pub interface_type: u32,
    /// High-speed serial index: 5 = 1 843 200 baud, 6 = 230 400 baud
    pub high_speed_index: Option<u8>,
    /// Card image filename for the UI (e.g. "Dev/E62.png")
    pub picture_name: String,
    /// Supports the new realtime-area protocol
    pub support_new_realtime: bool,
    /// Hardware platform ID (0=unknown/legacy, 1=AD210, 2=XR809, 3=Taizhen, 4=XR808)
    pub platform_id: u8,
    /// Buffer size in KB for flash storage
    pub buf_size: u32,
    /// Supports cloud platform connection
    pub support_cloud: bool,
}

impl HardwareCard {
    pub fn has_serial(&self)   -> bool { self.communications & 1  != 0 }
    pub fn has_usb(&self)      -> bool { self.communications & 2  != 0 }
    pub fn has_ethernet(&self) -> bool { self.communications & 4  != 0 }
    pub fn has_wifi(&self)     -> bool { self.communications & 8  != 0 }
    pub fn has_rf(&self)       -> bool { self.communications & 16 != 0 }
    pub fn has_gprs(&self)     -> bool { self.communications & 32 != 0 }

    pub fn has_hub75(&self)  -> bool { self.interface_type &  1 != 0 }
    pub fn has_50pin(&self)  -> bool { self.interface_type &  2 != 0 }
    pub fn has_hub08(&self)  -> bool { self.interface_type &  4 != 0 }
    pub fn has_hub12(&self)  -> bool { self.interface_type &  8 != 0 }

    /// Returns a short string describing the connection options, e.g. "USB/WiFi"
    pub fn comm_summary(&self) -> String {
        let mut parts = Vec::new();
        if self.has_serial()   { parts.push("Serial"); }
        if self.has_usb()      { parts.push("USB"); }
        if self.has_ethernet() { parts.push("Ethernet"); }
        if self.has_wifi()     { parts.push("WiFi"); }
        if self.has_rf()       { parts.push("RF"); }
        if self.has_gprs()     { parts.push("GPRS"); }
        if parts.is_empty()    { "—".into() } else { parts.join("/") }
    }

    /// Returns a short string listing interface connector types, e.g. "HUB75/HUB12"
    pub fn interface_summary(&self) -> String {
        if self.interface_type == 0 { return "—".into(); }
        let mut parts = Vec::new();
        if self.has_hub75()  { parts.push("HUB75"); }
        if self.has_50pin()  { parts.push("50PIN"); }
        if self.has_hub08()  { parts.push("HUB08"); }
        if self.has_hub12()  { parts.push("HUB12"); }
        parts.join("/")
    }
}

/// Parse CardList.xml and return all valid hardware cards.
pub fn load_cards() -> Vec<HardwareCard> {
    parse_card_list(CARD_LIST_XML)
}

/// Find an attribute value in an XML snippet; handles `attr = "value"` with
/// optional whitespace around `=`, as used in CardList.xml.
fn get_attr_relaxed<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    let suffixes: &[&str] = &[" = \"", " =\"", "= \"", "=\""];
    for suffix in suffixes {
        let needle = format!("{attr}{suffix}");
        let mut offset = 0;
        while let Some(rel) = xml[offset..].find(needle.as_str()) {
            let abs = offset + rel;
            // Require a word boundary before the attr name
            let ok = abs == 0 || {
                let b = xml.as_bytes()[abs - 1];
                !b.is_ascii_alphanumeric() && b != b'_'
            };
            if ok {
                let vs = abs + needle.len();
                if let Some(e) = xml[vs..].find('"') {
                    return Some(&xml[vs..vs + e]);
                }
            }
            offset = abs + 1;
        }
    }
    None
}

fn parse_card_list(xml: &str) -> Vec<HardwareCard> {
    let mut cards = Vec::new();
    let mut search = xml;

    while let Some(start) = search.find("<Item ") {
        let end = search[start..]
            .find("</Item>")
            .map(|e| start + e + 7)
            .or_else(|| search[start..].find("/>").map(|e| start + e + 2))
            .unwrap_or(search.len());

        let chunk = &search[start..end];

        // Card name is the inner text between > and </Item>
        let name = chunk
            .find('>')
            .map(|gt| {
                let after = &chunk[gt + 1..];
                let end = after.find('<').unwrap_or(after.len());
                after[..end].trim().to_string()
            })
            .unwrap_or_default();

        if !name.is_empty() {
            let ga = |a: &str| -> Option<&str> { get_attr_relaxed(chunk, a) };
            let gau = |a: &str| -> u32 { ga(a).and_then(|v| v.trim().parse().ok()).unwrap_or(0) };
            let gau8 = |a: &str| -> u8 { ga(a).and_then(|v| v.trim().parse().ok()).unwrap_or(0) };
            let gab = |a: &str| -> bool { ga(a).map(|v| v.trim() == "1").unwrap_or(false) };

            let high_speed_index = ga("HighSpeedIndex")
                .and_then(|v| v.trim().parse::<u8>().ok());

            cards.push(HardwareCard {
                id:           gau("id"),
                name,
                max_pixels:   gau("Pixel"),
                max_width:    gau("Widest"),
                max_height:   gau("Highest"),
                communications: gau8("Communications"),
                region:       gau8("Region"),
                max_gray:     gau8("MaxGray"),
                default_gray: gau8("DefaultGray"),
                smart_setting: gau8("SmartSetting"),
                support_flag: gau8("SupportFlag"),
                interface_type: gau("Interface"),
                high_speed_index,
                picture_name: ga("PictureName").unwrap_or("").to_string(),
                support_new_realtime: gab("SupportNewRealtimeArea"),
                platform_id:  gau8("PlatformID"),
                buf_size:     gau("Bufsize"),
                support_cloud: gab("SupportCloudPlatform"),
            });
        }

        search = &search[end.min(search.len())..];
    }

    cards
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_cards_parses_non_empty() {
        let cards = load_cards();
        assert!(!cards.is_empty(), "Should parse at least one card from CardList.xml");
    }

    #[test]
    fn ethernet_card_detected() {
        let cards = load_cards();
        let eth: Vec<_> = cards.iter().filter(|c| c.has_ethernet()).collect();
        assert!(!eth.is_empty(), "Should find at least one Ethernet card");
    }

    #[test]
    fn wifi_card_detected() {
        let cards = load_cards();
        let wifi: Vec<_> = cards.iter().filter(|c| c.has_wifi()).collect();
        assert!(!wifi.is_empty(), "Should find at least one WiFi card");
    }

    #[test]
    fn comm_summary_non_empty_for_all_cards() {
        for card in load_cards() {
            let s = card.comm_summary();
            assert!(!s.is_empty(), "comm_summary should never be empty (card {})", card.name);
        }
    }
}
