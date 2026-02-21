/// SDK XML command handler — routes incoming commands to appropriate handlers.
/// Implements the full Huidu SDK command set based on binary analysis.
use anyhow::Result;
use base64::Engine as _;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{info, warn};

use crate::core::player::PlayerCommand;
use crate::program::parser;
use crate::protocol::session::Session;
use crate::services::manager::ServicesState;

/// Handle an incoming SDK XML command and return the response XML
pub async fn handle_sdk_command(
    xml: &str,
    session: &Session,
    player_tx: &mpsc::Sender<PlayerCommand>,
    _program_dir: &str,
    services: &Arc<RwLock<ServicesState>>,
    screen_width: u32,
    screen_height: u32,
) -> Result<String> {
    let method = extract_method(xml).unwrap_or_default();
    info!("SDK command: {}", method);
    let guid = &session.guid;

    macro_rules! ok {
        ($body:expr) => {
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"{method}\">{}<result value=\"0\"/></out></sdk>",
                $body
            ))
        };
    }

    match method.as_str() {
        // ── Version Negotiation ────────────────────────────────────────────────
        "QueryIFVersion" | "queryIFVersion" | "GetIFVersion" => ok!(
            "<version value=\"0x1000000\"/>"
        ),

        // ── Program Management ─────────────────────────────────────────────────
        "AddProgram" | "addProgram" => {
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    {
                        let state = services.read().await;
                        let _ = state.storage.save_program(&screen, xml);
                    }
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    ok!("")
                }
                Err(e) => {
                    warn!("Failed to parse AddProgram: {}", e);
                    let msg = e.to_string().replace('"', "'").replace('<', "&lt;");
                    Ok(format!(
                        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                         <sdk guid=\"{guid}\"><out method=\"AddProgram\">\
                         <result value=\"1\"/><error message=\"{msg}\"/></out></sdk>"
                    ))
                }
            }
        }

        "UpdateProgram" | "updateProgram" => {
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    {
                        let state = services.read().await;
                        let _ = state.storage.save_program(&screen, xml);
                    }
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    ok!("")
                }
                Err(e) => {
                    warn!("Failed to parse UpdateProgram: {}", e);
                    Ok(format!(
                        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                         <sdk guid=\"{guid}\"><out method=\"UpdateProgram\">\
                         <result value=\"1\"/></out></sdk>"
                    ))
                }
            }
        }

        "DeleteProgram" | "deleteProgram" => {
            {
                let state = services.read().await;
                let _ = state.storage.clear();
            }
            player_tx
                .send(PlayerCommand::LoadScreen(crate::program::model::Screen {
                    timestamps: String::new(),
                    programs: Vec::new(),
                }))
                .await
                .ok();
            ok!("")
        }

        "GetAllProgram" | "getAllProgram" => {
            let state = services.read().await;
            let mut items = String::new();
            for p in &state.programs {
                items.push_str(&format!(
                    "<program guid=\"{}\" name=\"{}\"/>",
                    p.guid, p.name
                ));
            }
            ok!(&items)
        }

        "GetProgram" | "getProgram" => {
            // Return the XML for the requested program, or empty if not found
            let req_guid = extract_attr(xml, "program", "guid").unwrap_or_default();
            let state = services.read().await;
            let found = state.programs.iter().any(|p| p.guid == req_guid);
            if found {
                // Load the raw XML from disk
                let raw = state
                    .storage
                    .load_current_program()
                    .map(|_| {
                        // Re-read the file for the raw XML
                        let path = state.storage.program_dir().join("current_program.xml");
                        std::fs::read_to_string(path).unwrap_or_default()
                    })
                    .unwrap_or_default();
                ok!(&raw)
            } else {
                ok!("")
            }
        }

        "SwitchProgram" | "switchProgram" => {
            let target_guid = extract_attr(xml, "program", "guid").unwrap_or_default();
            if !target_guid.is_empty() {
                player_tx
                    .send(PlayerCommand::SwitchProgram(target_guid))
                    .await
                    .ok();
            }
            ok!("")
        }

        "GetCurrentPlayProgramGUID" | "getCurrentPlayProgramGUID" => {
            let state = services.read().await;
            let g = &state.current_program_guid;
            ok!(&format!("<program guid=\"{g}\"/>"))
        }

        "RealTimeUpdate" | "realTimeUpdate" => {
            // Treated the same as UpdateProgram for now
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    ok!("")
                }
                Err(e) => {
                    warn!("RealTimeUpdate parse error: {}", e);
                    ok!("")
                }
            }
        }

        "InsertPlayProgram" | "insertPlayProgram" => {
            // Priority insert — treated as LoadScreen for now
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    ok!("")
                }
                Err(_) => ok!(""),
            }
        }

        "ModifyProgram" | "modifyProgram" => {
            // In-place modification — treated as UpdateProgram for now
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    ok!("")
                }
                Err(_) => ok!(""),
            }
        }

        "DeleteNotCiteFile" | "deleteNotCiteFile" => {
            // Delete files in the program dir that are not referenced by current programs.
            // For simplicity: list all files and keep only current_program.xml.
            let state = services.read().await;
            let files = state.storage.list_files();
            for f in &files {
                if f != "current_program.xml" && f != "screenshot.png" {
                    let _ = state.storage.delete_file(f);
                }
            }
            ok!("")
        }

        // ── Screen Control ─────────────────────────────────────────────────────
        "OpenScreen" | "openScreen" => {
            player_tx.send(PlayerCommand::ScreenPower(true)).await.ok();
            ok!("")
        }

        "CloseScreen" | "closeScreen" => {
            player_tx.send(PlayerCommand::ScreenPower(false)).await.ok();
            ok!("")
        }

        "ScreenRotation" | "screenRotation" => {
            // <rotate value="90"/> or <rotation value="90"/>
            let deg = extract_attr(xml, "rotate", "value")
                .or_else(|| extract_attr(xml, "rotation", "value"))
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(0);
            player_tx.send(PlayerCommand::SetRotation(deg)).await.ok();
            ok!("")
        }

        "GetScreenRotation" | "getScreenRotation" => {
            let state = services.read().await;
            let r = state.rotation;
            ok!(&format!("<rotation value=\"{r}\"/>"))
        }

        // ── Brightness ─────────────────────────────────────────────────────────
        "SetLuminancePloy" | "setLuminancePloy" => {
            let mode = extract_attr(xml, "luminance", "mode").unwrap_or_default();
            if mode == "auto" {
                // Extract brightness schedule: <item time="HH:MM" level="N"/>
                let entries = extract_brightness_schedule(xml);
                let mut state = services.write().await;
                state.brightness.set_schedule(entries);
            } else {
                // Manual mode: <luminance mode="manual" value="N"/>
                if let Some(val) = extract_attr(xml, "luminance", "value") {
                    if let Ok(level) = val.parse::<u8>() {
                        let mut state = services.write().await;
                        state.brightness.set_level(level);
                        drop(state);
                        player_tx.send(PlayerCommand::SetBrightness(level)).await.ok();
                    }
                }
            }
            ok!("")
        }

        "GetLuminancePloy" | "getLuminancePloy" => {
            let state = services.read().await;
            let level = state.brightness.get_level();
            let schedule = state.brightness.get_schedule();
            if schedule.is_empty() {
                ok!(&format!("<luminance mode=\"manual\" value=\"{level}\"/>"))
            } else {
                let mut items = String::from("<luminance mode=\"auto\">");
                for e in schedule {
                    items.push_str(&format!(
                        "<item time=\"{:02}:{:02}\" level=\"{}\"/>",
                        e.hour, e.minute, e.level
                    ));
                }
                items.push_str("</luminance>");
                ok!(&items)
            }
        }

        // ── Screen Schedule ────────────────────────────────────────────────────
        "GetSwitchTime" | "getSwitchTime" => {
            let state = services.read().await;
            let entries = state.screen_schedule.get_schedule();
            let mut items = String::new();
            for (i, entry) in entries.iter().enumerate() {
                items.push_str(&format!(
                    "<item index=\"{}\" onTime=\"{}\" offTime=\"{}\" days=\"{}\"/>",
                    i, entry.on_time, entry.off_time, entry.days
                ));
            }
            ok!(&items)
        }

        "SetSwitchTime" | "setSwitchTime" => {
            let entries = extract_schedule_entries(xml);
            {
                let mut state = services.write().await;
                state.screen_schedule.set_schedule(entries);
            }
            ok!("")
        }

        // ── Time ───────────────────────────────────────────────────────────────
        "GetTimeInfo" | "getTimeInfo" => {
            let now = chrono::Local::now();
            let dt = now.format("%Y-%m-%d %H:%M:%S").to_string();
            let state = services.read().await;
            let ntp = &state.ntp_server;
            ok!(&format!(
                "<time value=\"{dt}\"/><ntp enable=\"true\" server=\"{ntp}\"/>"
            ))
        }

        "SetTimeInfo" | "setTimeInfo" => {
            if let Some(time_val) = extract_attr(xml, "time", "value") {
                crate::services::time_sync::TimeSyncService::set_time(&time_val).await;
            }
            if let Some(ntp_server) = extract_attr(xml, "ntp", "server") {
                let mut state = services.write().await;
                state.ntp_server = ntp_server;
            }
            ok!("")
        }

        // ── Device Info ────────────────────────────────────────────────────────
        "GetDeviceInfo" | "getDeviceInfo" => {
            let state = services.read().await;
            let name = &state.device_name;
            let dev_id = if state.device_id.is_empty() { "RUST-001" } else { &state.device_id };
            ok!(&format!(
                "<deviceInfo cpu=\"RustPlayer\" model=\"huidu-player\" \
                 fpgaVersion=\"1.0.0\" screenWidth=\"{screen_width}\" \
                 screenHeight=\"{screen_height}\" deviceID=\"{dev_id}\" name=\"{name}\"/>"
            ))
        }

        "GetDeviceName" | "getDeviceName" => {
            let state = services.read().await;
            let name = &state.device_name;
            ok!(&format!("<name value=\"{name}\"/>"))
        }

        "SetDeviceName" | "setDeviceName" => {
            if let Some(name) = extract_attr(xml, "name", "value") {
                let mut state = services.write().await;
                state.device_name = name;
            }
            ok!("")
        }

        "GetHardwareInfo" | "getHardwareInfo" => {
            ok!(
                "<hardware cpu=\"aarch64\" ram=\"512\" storage=\"8192\" \
                 cpuUsage=\"5\" memUsage=\"15\" temperature=\"40\"/>"
            )
        }

        // ── Screenshot ─────────────────────────────────────────────────────────
        "GetScreenshot2" | "getScreenshot2" | "GetScreenshot" | "getScreenshot" => {
            let state = services.read().await;
            let buf = state.screenshot.lock().await;
            if buf.is_empty() {
                ok!("<screenshot format=\"png\" data=\"\"/>")
            } else {
                let b64 = base64::engine::general_purpose::STANDARD.encode(&*buf);
                ok!(&format!("<screenshot format=\"png\" data=\"{b64}\"/>"))
            }
        }

        // ── Font Management ────────────────────────────────────────────────────
        "GetAllFontInfo" | "getAllFontInfo" => {
            ok!(
                "<font name=\"Arial\" index=\"0\"/>\
                 <font name=\"DejaVu Sans\" index=\"1\"/>"
            )
        }

        // ── Network Config ─────────────────────────────────────────────────────
        "GetEth0Info" | "getEth0Info" => {
            let ip = crate::protocol::discovery::get_local_ip();
            ok!(&format!(
                "<eth0 dhcp=\"true\" ip=\"{ip}\" mask=\"255.255.255.0\" \
                 gateway=\"\" dns=\"8.8.8.8\"/>"
            ))
        }

        "SetEth0Info" | "setEth0Info" => {
            info!("SetEth0Info received (network config change, not applied)");
            ok!("")
        }

        "GetPppoeInfo" | "getPppoeInfo" => {
            ok!("<pppoe enable=\"false\" user=\"\" password=\"\" status=\"disconnected\"/>")
        }

        "GetWifiInfo" | "getWifiInfo" => {
            ok!("<wifi enable=\"false\" ssid=\"\" password=\"\" status=\"disconnected\"/>")
        }

        "SetWifiInfo" | "setWifiInfo" => {
            info!("SetWifiInfo received (not applied on this platform)");
            ok!("")
        }

        "GetNetworkInfo" | "getNetworkInfo" => {
            let ip = crate::protocol::discovery::get_local_ip();
            ok!(&format!(
                "<network eth0Connected=\"true\" wifiConnected=\"false\" internet=\"false\" \
                 ip=\"{ip}\"/>"
            ))
        }

        // ── File Management ────────────────────────────────────────────────────
        "GetFiles" | "getFiles" => {
            let state = services.read().await;
            let files = state.storage.list_files();
            let mut items = String::new();
            for f in &files {
                items.push_str(&format!("<file name=\"{f}\"/>"));
            }
            ok!(&items)
        }

        "DeleteFiles" | "deleteFiles" => {
            let filenames = extract_file_list(xml);
            let state = services.read().await;
            for f in &filenames {
                let _ = state.storage.delete_file(f);
            }
            ok!("")
        }

        // ── Boot Logo ──────────────────────────────────────────────────────────
        "GetBootLogo" | "getBootLogo" => {
            ok!("<bootLogo name=\"\"/>")
        }

        "SetBootLogoName" | "setBootLogoName" | "ClearBootLogo" | "clearBootLogo" => ok!(""),

        // ── TCP Server Config ──────────────────────────────────────────────────
        "GetSDKTcpServer" | "getSDKTcpServer" => {
            ok!("<server ip=\"\" port=\"10001\" enable=\"true\"/>")
        }

        "SetSDKTcpServer" | "setSDKTcpServer" => ok!(""),

        // ── FPGA Hardware Config ───────────────────────────────────────────────
        "GetBoxHwConfig" | "getBoxHwConfig" | "GetSDKFPGAConfig" | "getSDKFPGAConfig" => {
            let state = services.read().await;
            let cfg = state.fpga_config.clone();
            ok!(&cfg)
        }

        "SetBoxHwConfig" | "setBoxHwConfig" | "SaveBoxHwConfig" | "saveBoxHwConfig"
        | "ReplaceBoxHwConfig" | "replaceBoxHwConfig" => {
            // Extract the BoxHwConfig element if present
            if let Some(start) = xml.find("<BoxHwConfig") {
                if let Some(end) = xml[start..].find("</BoxHwConfig>") {
                    let config_xml = &xml[start..start + end + "</BoxHwConfig>".len()];
                    let mut state = services.write().await;
                    state.fpga_config = config_xml.to_string();
                }
            }
            ok!("")
        }

        "SetSDKFPGAConfig" | "setSDKFPGAConfig" => {
            info!("SetSDKFPGAConfig received (FPGA hardware driver not yet implemented)");
            ok!("")
        }

        "SmartSetting" | "smartSetting" | "SmartDrawLine" | "smartDrawLine" => ok!(""),

        // ── License ────────────────────────────────────────────────────────────
        "GetLicense" | "getLicense" => {
            let state = services.read().await;
            let lic = &state.license;
            let valid = !lic.is_empty();
            ok!(&format!(
                "<license value=\"{lic}\" valid=\"{valid}\"/>"
            ))
        }

        "SetLicense" | "setLicense" => {
            if let Some(lic) = extract_attr(xml, "license", "value") {
                let mut state = services.write().await;
                state.license = lic;
            }
            ok!("")
        }

        "ClearLicense" | "clearLicense" => {
            let mut state = services.write().await;
            state.license = String::new();
            ok!("")
        }

        "CheckSuperCode" | "checkSuperCode" => {
            // Always accept the super code in this open-source reproduction
            ok!("<superCode valid=\"true\"/>")
        }

        // ── Admin Mode ─────────────────────────────────────────────────────────
        "GetAdminModeInfo" | "getAdminModeInfo" => {
            let state = services.read().await;
            let admin = state.admin_mode;
            ok!(&format!("<adminMode enable=\"{admin}\"/>"))
        }

        "SetAdminModeInfo" | "setAdminModeInfo" => {
            if let Some(v) = extract_attr(xml, "adminMode", "enable") {
                let mut state = services.write().await;
                state.admin_mode = v == "true" || v == "1";
            }
            ok!("")
        }

        "UnlockAdminModePassword" | "unlockAdminModePassword" => ok!(""),

        // ── Volume ─────────────────────────────────────────────────────────────
        "GetSystemVolume" | "getSystemVolume" => {
            let state = services.read().await;
            let vol = state.volume;
            ok!(&format!("<volume value=\"{vol}\"/>"))
        }

        "SetSystemVolume" | "setSystemVolume" => {
            if let Some(v) = extract_attr(xml, "volume", "value") {
                if let Ok(vol) = v.parse::<u8>() {
                    let mut state = services.write().await;
                    state.volume = vol.min(100);
                }
            }
            ok!("")
        }

        // ── Sensors / Modbus ───────────────────────────────────────────────────
        "GetSensorInfo" | "getSensorInfo" => {
            ok!("<sensors count=\"0\"/>")
        }

        "GetCurrentSensorValue" | "getCurrentSensorValue" => {
            ok!("<sensors count=\"0\"/>")
        }

        "GetGPSInfo" | "getGPSInfo" => {
            ok!("<gps enable=\"false\" lat=\"0.0\" lon=\"0.0\"/>")
        }

        "GetRelayInfo" | "getRelayInfo" => {
            ok!("<relays count=\"0\"/>")
        }

        "SetRelayInfo" | "setRelayInfo" | "SetRelayStatusInfo" | "setRelayStatusInfo" => ok!(""),

        "GetSerialSDK" | "getSerialSDK" => {
            ok!("<serialSDK enable=\"false\" port=\"\" baud=\"9600\"/>")
        }

        "SetSerialSDK" | "setSerialSDK" => ok!(""),

        // ── System Control ─────────────────────────────────────────────────────
        "Reboot" | "reboot" => {
            info!("Reboot requested — initiating system reboot");
            // On Linux, spawn `reboot` in the background
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("reboot").spawn();
            }
            ok!("")
        }

        "FirmwareUpgrade" | "firmwareUpgrade" => {
            info!("FirmwareUpgrade requested (not implemented)");
            ok!("<upgradeStatus value=\"unsupported\"/>")
        }

        "ExcuteUpgradeShell" | "excuteUpgradeShell" => {
            info!("ExcuteUpgradeShell requested (not implemented)");
            ok!("")
        }

        "GetUpgradeResult" | "getUpgradeResult" => {
            ok!("<upgradeResult value=\"1\" message=\"no upgrade in progress\"/>")
        }

        // ── Data Sources ───────────────────────────────────────────────────────
        "GetDataSourceInfo" | "getDataSourceInfo" => {
            ok!("<dataSources count=\"0\"/>")
        }

        "SetDataSourceInfo" | "setDataSourceInfo" => ok!(""),

        "ReloadDeviceID" | "reloadDeviceID" => ok!(""),

        // ── Catch-all ──────────────────────────────────────────────────────────
        _ => {
            warn!("Unhandled SDK method: {}", method);
            ok!("")
        }
    }
}

/// Extract the method name from `<sdk...><in method="MethodName">`
fn extract_method(xml: &str) -> Option<String> {
    let in_start = xml.find("<in ")?;
    let method_attr = xml[in_start..].find("method=\"")?;
    let start = in_start + method_attr + 8;
    let end = xml[start..].find('"')? + start;
    Some(xml[start..end].to_string())
}

/// Extract an attribute value from an element
fn extract_attr(xml: &str, element: &str, attr: &str) -> Option<String> {
    let tag = format!("<{}", element);
    let pos = xml.find(&tag)?;
    let search = format!("{}=\"", attr);
    let attr_pos = xml[pos..].find(&search)?;
    let start = pos + attr_pos + search.len();
    let end = xml[start..].find('"')? + start;
    Some(xml[start..end].to_string())
}

/// Extract screen schedule entries from XML
fn extract_schedule_entries(xml: &str) -> Vec<crate::services::screen_schedule::ScreenScheduleEntry> {
    let mut entries = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = xml[search_from..].find("<item ") {
        let abs_pos = search_from + pos;
        let on_time = extract_attr(&xml[abs_pos..], "item", "onTime").unwrap_or_default();
        let off_time = extract_attr(&xml[abs_pos..], "item", "offTime").unwrap_or_default();
        let days = extract_attr(&xml[abs_pos..], "item", "days").unwrap_or_default();
        entries.push(crate::services::screen_schedule::ScreenScheduleEntry {
            on_time,
            off_time,
            days,
        });
        search_from = abs_pos + 5;
    }
    entries
}

/// Extract brightness schedule entries from SetLuminancePloy XML.
/// Expected: `<item time="HH:MM" level="N"/>` elements inside `<luminance mode="auto">`.
fn extract_brightness_schedule(
    xml: &str,
) -> Vec<crate::services::brightness::BrightnessScheduleEntry> {
    let mut entries = Vec::new();
    let mut from = 0;
    while let Some(pos) = xml[from..].find("<item ") {
        let abs = from + pos;
        let time_str = extract_attr(&xml[abs..], "item", "time").unwrap_or_default();
        let level_str = extract_attr(&xml[abs..], "item", "level").unwrap_or_default();
        let level: u8 = level_str.parse().unwrap_or(100);
        let parts: Vec<&str> = time_str.split(':').collect();
        let hour: u8 = parts.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let minute: u8 = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        entries.push(crate::services::brightness::BrightnessScheduleEntry { hour, minute, level });
        from = abs + 6;
    }
    entries
}

/// Extract file list from DeleteFiles XML
fn extract_file_list(xml: &str) -> Vec<String> {
    let mut files = Vec::new();
    let mut search_from = 0;
    while let Some(pos) = xml[search_from..].find("<file ") {
        let abs_pos = search_from + pos;
        if let Some(name) = extract_attr(&xml[abs_pos..], "file", "name") {
            files.push(name);
        }
        search_from = abs_pos + 5;
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_method() {
        let xml = r#"<sdk guid="abc"><in method="AddProgram"><screen></screen></in></sdk>"#;
        assert_eq!(extract_method(xml), Some("AddProgram".to_string()));
    }

    #[test]
    fn test_extract_attr() {
        let xml = r#"<luminance mode="manual" value="75"/>"#;
        assert_eq!(extract_attr(xml, "luminance", "value"), Some("75".to_string()));
        assert_eq!(extract_attr(xml, "luminance", "mode"), Some("manual".to_string()));
    }

    #[test]
    fn test_extract_schedule_entries() {
        let xml = r#"<sdk><in method="SetSwitchTime"><item onTime="08:00" offTime="22:00" days="1111111"/></in></sdk>"#;
        let entries = extract_schedule_entries(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].on_time, "08:00");
        assert_eq!(entries[0].off_time, "22:00");
    }
}
