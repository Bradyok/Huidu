//! Serialize / deserialize BoxHwConfig to/from the native Huidu XML format.
//!
//! The XML format is used by GetBoxHwConfig / SetBoxHwConfig SDK methods,
//! and is also the format of `send_card_cfg.xml` on the device.

use super::model::*;

/// Serialize a BoxHwConfig to Huidu XML string.
pub fn to_xml(cfg: &BoxHwConfig) -> String {
    let mut out = String::from("<BoxHwConfig>");
    out.push_str("<CardInfo>");
    for card in &cfg.card_info {
        out.push_str(&card_to_xml(card));
    }
    out.push_str("</CardInfo>");
    out.push_str(&format!(
        "<NetcardCtrlRect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
        cfg.netcard_ctrl_rect.x, cfg.netcard_ctrl_rect.y,
        cfg.netcard_ctrl_rect.width, cfg.netcard_ctrl_rect.height
    ));
    out.push_str(&val("ModeSwitchPlan", cfg.mode_switch_plan));
    out.push_str(&val("Rotation", cfg.rotation));
    out.push_str(&val("SendCardMode", cfg.send_card_mode));
    out.push_str(&val("NetCardCtrlMode", cfg.net_card_ctrl_mode));
    out.push_str(&val("AsyncPriorityMode", cfg.async_priority_mode));
    out.push_str(&val("EnSendcardOnly", cfg.en_send_card_only as u32));
    out.push_str(&val("RecvCardChoose", cfg.recv_card_choose));
    out.push_str(&val("RgbCtrlWidth", cfg.rgb_ctrl_width));
    out.push_str(&val("RgbCtrlHeight", cfg.rgb_ctrl_height));
    out.push_str(&val("BrightnessSetMode", cfg.brightness_set_mode));
    out.push_str("</BoxHwConfig>");
    out
}

fn card_to_xml(c: &CardConfig) -> String {
    let mut out = format!("<Card index=\"{}\">", c.index);
    out.push_str(&val("ModuleType",        c.module_type));
    out.push_str(&val("DriveChipType",     c.drive_chip_type));
    out.push_str(&val("CellWidth",         c.cell_width));
    out.push_str(&val("CellHight",         c.cell_height)); // note: "Hight" matches original
    out.push_str(&val("CellScanRow",       c.cell_scan_row));
    out.push_str(&val("ScanMode",          c.scan_mode));
    out.push_str(&val("MoreThan16Scan",    c.more_than_16_scan as u32));
    out.push_str(&val("ESignal",           c.e_signal as u32));
    out.push_str(&val("Chip595",           c.chip_595));
    out.push_str(&val("Chip5958",          c.chip_5958));
    out.push_str(&val("DecodingMode",      c.decoding_mode));
    out.push_str(&val("DataPolarity",      c.data_polarity));
    out.push_str(&val("OEPolarity",        c.oe_polarity));
    out.push_str(&val("SignalColor",       c.signal_color));
    out.push_str(&val("CellNullNum",       c.cell_null_num));
    out.push_str(&val("LookUpTab",         c.look_up_tab));
    out.push_str(&val("RefreshRate",       c.refresh_rate));
    out.push_str(&val("R_Acc",             c.r_acc));
    out.push_str(&val("GrayLevel",         c.gray_level));
    out.push_str(&val("LuminanceLevel",    c.luminance_level));
    out.push_str(&val("Frequency",         c.frequency));
    out.push_str(&val("PriorityMode",      c.priority_mode));
    // RGB bit depth: only the matching tag has value="1"
    out.push_str(&val("RGB20", (c.rgb_bit_depth == RgbBitDepth::Rgb20) as u32));
    out.push_str(&val("RGB24", (c.rgb_bit_depth == RgbBitDepth::Rgb24) as u32));
    out.push_str(&val("RGB28", (c.rgb_bit_depth == RgbBitDepth::Rgb28) as u32));
    out.push_str(&val("RGB32", (c.rgb_bit_depth == RgbBitDepth::Rgb32) as u32));
    out.push_str(&val("GamaValue",         c.gamma_value));
    out.push_str(&val("RedCorrection",     c.red_correction));
    out.push_str(&val("GreenCorrection",   c.green_correction));
    out.push_str(&val("BlueCorrection",    c.blue_correction));
    out.push_str(&val("DutyCycle",         c.duty_cycle));
    out.push_str(&val("BV",               c.bv));
    out.push_str(&val("Phase",            c.phase));
    out.push_str(&val("Afterglow",        c.afterglow));
    out.push_str(&val("OM",              c.om));
    out.push_str(&val("PwmRedCurrent",   c.pwm_red_current));
    out.push_str(&val("PwmGreenCurrent", c.pwm_green_current));
    out.push_str(&val("PwmBlueCurrent",  c.pwm_blue_current));
    out.push_str(&val("SPWMMode",        c.spwm_mode));
    out.push_str(&val("FMPWMMultiplier", c.fm_pwm_multiplier));
    out.push_str(&val("DoubleRefreshRate", c.double_refresh_rate as u32));
    out.push_str(&val("GCLKMultiplier",  c.gclk_multiplier));
    out.push_str(&val("PwmFrequency",    c.pwm_frequency));
    out.push_str(&val("F_Frame",         c.f_frame));
    out.push_str(&val("Brightness",      c.brightness));
    // Optional gamma table
    if let Some(tab) = &c.gamma_tab {
        let hex: String = tab.iter().map(|b| format!("{:02X}", b)).collect::<Vec<_>>().join(" ");
        out.push_str(&format!("<GamaTab value=\"{}\"/>", hex));
    }
    out.push_str(&val("EnOutputDefinition", c.en_output_definition as u32));
    out.push_str(&val("OutputDefinition",   c.output_definition));
    out.push_str(&val("PwmChipType",        c.pwm_chip_type));
    for (i, v) in c.pwm_ic_red_regs.iter().enumerate() {
        out.push_str(&val(&format!("PWMICRedReg{}", i + 1), *v));
    }
    for (i, v) in c.pwm_ic_green_regs.iter().enumerate() {
        out.push_str(&val(&format!("PWMICGreenReg{}", i + 1), *v));
    }
    for (i, v) in c.pwm_ic_blue_regs.iter().enumerate() {
        out.push_str(&val(&format!("PWMICBlueReg{}", i + 1), *v));
    }
    out.push_str("</Card>");
    out
}

fn val(tag: &str, v: impl std::fmt::Display) -> String {
    format!("<{} value=\"{}\"/>", tag, v)
}

/// Parse a BoxHwConfig from Huidu XML string.
pub fn from_xml(xml: &str) -> Option<BoxHwConfig> {
    use crate::xml::parse_value_attr;

    let mut cfg = BoxHwConfig::default();

    // Parse NetcardCtrlRect
    if let Some(pos) = xml.find("<NetcardCtrlRect") {
        let sub = &xml[pos..];
        cfg.netcard_ctrl_rect.x = crate::xml::get_attr(sub, "x").and_then(|s| s.parse().ok()).unwrap_or(0);
        cfg.netcard_ctrl_rect.y = crate::xml::get_attr(sub, "y").and_then(|s| s.parse().ok()).unwrap_or(0);
        cfg.netcard_ctrl_rect.width = crate::xml::get_attr(sub, "width").and_then(|s| s.parse().ok()).unwrap_or(0);
        cfg.netcard_ctrl_rect.height = crate::xml::get_attr(sub, "height").and_then(|s| s.parse().ok()).unwrap_or(0);
    }

    cfg.mode_switch_plan     = parse_value_attr(xml, "ModeSwitchPlan").unwrap_or(0);
    cfg.rotation             = parse_value_attr(xml, "Rotation").unwrap_or(0);
    cfg.send_card_mode       = parse_value_attr(xml, "SendCardMode").unwrap_or(0);
    cfg.net_card_ctrl_mode   = parse_value_attr(xml, "NetCardCtrlMode").unwrap_or(0);
    cfg.async_priority_mode  = parse_value_attr(xml, "AsyncPriorityMode").unwrap_or(0);
    cfg.en_send_card_only    = parse_value_attr(xml, "EnSendcardOnly").unwrap_or(0) != 0;
    cfg.recv_card_choose     = parse_value_attr(xml, "RecvCardChoose").unwrap_or(0);
    cfg.rgb_ctrl_width       = parse_value_attr(xml, "RgbCtrlWidth").unwrap_or(0);
    cfg.rgb_ctrl_height      = parse_value_attr(xml, "RgbCtrlHeight").unwrap_or(0);
    cfg.brightness_set_mode  = parse_value_attr(xml, "BrightnessSetMode").unwrap_or(0);

    // Parse each <Card index="N">...</Card> block
    let mut search = xml;
    while let Some(card_start) = search.find("<Card ") {
        let block_start = card_start;
        let rest = &search[block_start..];
        let end = rest.find("</Card>").map(|e| e + 7)?;
        let card_xml = &rest[..end];
        cfg.card_info.push(parse_card(card_xml));
        search = &rest[end..];
    }

    Some(cfg)
}

fn parse_card(xml: &str) -> CardConfig {
    use crate::xml::{get_attr, parse_value_attr};
    let mut c = CardConfig::default();
    c.index         = get_attr(xml, "index").and_then(|s| s.parse().ok()).unwrap_or(0);
    c.module_type   = parse_value_attr(xml, "ModuleType").unwrap_or(0);
    c.drive_chip_type = parse_value_attr(xml, "DriveChipType").unwrap_or(0);
    c.cell_width    = parse_value_attr(xml, "CellWidth").unwrap_or(32);
    c.cell_height   = parse_value_attr(xml, "CellHight").unwrap_or(16);
    c.cell_scan_row = parse_value_attr(xml, "CellScanRow").unwrap_or(4);
    c.scan_mode     = parse_value_attr(xml, "ScanMode").unwrap_or(2);
    c.more_than_16_scan = parse_value_attr(xml, "MoreThan16Scan").unwrap_or(0) != 0;
    c.e_signal      = parse_value_attr(xml, "ESignal").unwrap_or(0) != 0;
    c.chip_595      = parse_value_attr(xml, "Chip595").unwrap_or(0);
    c.chip_5958     = parse_value_attr(xml, "Chip5958").unwrap_or(0);
    c.decoding_mode = parse_value_attr(xml, "DecodingMode").unwrap_or(0);
    c.data_polarity = parse_value_attr(xml, "DataPolarity").unwrap_or(0);
    c.oe_polarity   = parse_value_attr(xml, "OEPolarity").unwrap_or(0);
    c.signal_color  = parse_value_attr(xml, "SignalColor").unwrap_or(0);
    c.cell_null_num = parse_value_attr(xml, "CellNullNum").unwrap_or(0);
    c.look_up_tab   = parse_value_attr(xml, "LookUpTab").unwrap_or(0);
    c.refresh_rate  = parse_value_attr(xml, "RefreshRate").unwrap_or(1200);
    c.r_acc         = parse_value_attr(xml, "R_Acc").unwrap_or(0);
    c.gray_level    = parse_value_attr(xml, "GrayLevel").unwrap_or(256);
    c.luminance_level = parse_value_attr(xml, "LuminanceLevel").unwrap_or(64);
    c.frequency     = parse_value_attr(xml, "Frequency").unwrap_or(25);
    c.priority_mode = parse_value_attr(xml, "PriorityMode").unwrap_or(2);
    c.rgb_bit_depth = if parse_value_attr(xml, "RGB20").unwrap_or(0) == 1 { RgbBitDepth::Rgb20 }
                  else if parse_value_attr(xml, "RGB28").unwrap_or(0) == 1 { RgbBitDepth::Rgb28 }
                  else if parse_value_attr(xml, "RGB32").unwrap_or(0) == 1 { RgbBitDepth::Rgb32 }
                  else { RgbBitDepth::Rgb24 };
    c.gamma_value   = parse_value_attr(xml, "GamaValue").unwrap_or(0);
    c.red_correction   = parse_value_attr(xml, "RedCorrection").unwrap_or(1000);
    c.green_correction = parse_value_attr(xml, "GreenCorrection").unwrap_or(1000);
    c.blue_correction  = parse_value_attr(xml, "BlueCorrection").unwrap_or(1000);
    c.duty_cycle    = parse_value_attr(xml, "DutyCycle").unwrap_or(500);
    c.bv            = parse_value_attr(xml, "BV").unwrap_or(6);
    c.phase         = parse_value_attr(xml, "Phase").unwrap_or(0);
    c.afterglow     = parse_value_attr(xml, "Afterglow").unwrap_or(1);
    c.om            = parse_value_attr(xml, "OM").unwrap_or(0);
    c.pwm_red_current   = parse_value_attr(xml, "PwmRedCurrent").unwrap_or(0);
    c.pwm_green_current = parse_value_attr(xml, "PwmGreenCurrent").unwrap_or(0);
    c.pwm_blue_current  = parse_value_attr(xml, "PwmBlueCurrent").unwrap_or(0);
    c.spwm_mode         = parse_value_attr(xml, "SPWMMode").unwrap_or(0);
    c.fm_pwm_multiplier = parse_value_attr(xml, "FMPWMMultiplier").unwrap_or(0);
    c.double_refresh_rate = parse_value_attr(xml, "DoubleRefreshRate").unwrap_or(0) != 0;
    c.gclk_multiplier   = parse_value_attr(xml, "GCLKMultiplier").unwrap_or(4);
    c.pwm_frequency     = parse_value_attr(xml, "PwmFrequency").unwrap_or(0);
    c.f_frame           = parse_value_attr(xml, "F_Frame").unwrap_or(0);
    c.brightness        = parse_value_attr(xml, "Brightness").unwrap_or(100);
    c.en_output_definition = parse_value_attr(xml, "EnOutputDefinition").unwrap_or(0) != 0;
    c.output_definition = parse_value_attr(xml, "OutputDefinition").unwrap_or(0);
    c.pwm_chip_type     = parse_value_attr(xml, "PwmChipType").unwrap_or(0);
    for i in 0..3 {
        c.pwm_ic_red_regs[i]   = parse_value_attr(xml, &format!("PWMICRedReg{}",   i+1)).unwrap_or(0);
        c.pwm_ic_green_regs[i] = parse_value_attr(xml, &format!("PWMICGreenReg{}", i+1)).unwrap_or(0);
        c.pwm_ic_blue_regs[i]  = parse_value_attr(xml, &format!("PWMICBlueReg{}",  i+1)).unwrap_or(0);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_config() {
        let cfg = BoxHwConfig {
            card_info: vec![CardConfig::default()],
            rgb_ctrl_width: 128,
            rgb_ctrl_height: 64,
            ..Default::default()
        };
        let xml = to_xml(&cfg);
        assert!(xml.contains("<BoxHwConfig>"));
        assert!(xml.contains("<Card index=\"0\">"));
        let parsed = from_xml(&xml).unwrap();
        assert_eq!(parsed.card_info.len(), 1);
        assert_eq!(parsed.card_info[0].refresh_rate, 1200);
        assert_eq!(parsed.card_info[0].gray_level, 256);
        assert_eq!(parsed.rgb_ctrl_width, 128);
    }
}
