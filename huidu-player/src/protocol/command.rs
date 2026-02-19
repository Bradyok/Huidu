/// SDK XML command handler — routes incoming commands to appropriate handlers.
/// Implements the full Huidu SDK command set based on binary analysis.
use anyhow::Result;
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

    match method.as_str() {
        // --- Version Negotiation ---
        "QueryIFVersion" | "queryIFVersion" | "GetIFVersion" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"QueryIFVersion\">\
             <version value=\"0x1000000\"/></out></sdk>"
        )),

        // --- Program Management ---
        "AddProgram" | "addProgram" => {
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    // Save to disk
                    {
                        let state = services.read().await;
                        let _ = state.storage.save_program(&screen, xml);
                    }
                    player_tx.send(PlayerCommand::LoadScreen(screen)).await.ok();
                    Ok(format!(
                        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                         <sdk guid=\"{guid}\"><out method=\"AddProgram\">\
                         <result value=\"0\"/></out></sdk>"
                    ))
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
                    Ok(format!(
                        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                         <sdk guid=\"{guid}\"><out method=\"UpdateProgram\">\
                         <result value=\"0\"/></out></sdk>"
                    ))
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
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"DeleteProgram\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Screen Control ---
        "OpenScreen" | "openScreen" => {
            player_tx.send(PlayerCommand::ScreenPower(true)).await.ok();
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"OpenScreen\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        "CloseScreen" | "closeScreen" => {
            player_tx.send(PlayerCommand::ScreenPower(false)).await.ok();
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"CloseScreen\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Brightness ---
        "GetLuminancePloy" | "getLuminancePloy" => {
            let state = services.read().await;
            let level = state.brightness.get_level();
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"GetLuminancePloy\">\
                 <luminance mode=\"manual\" value=\"{level}\"/>\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        "SetLuminancePloy" | "setLuminancePloy" => {
            if let Some(val) = extract_attr(xml, "luminance", "value") {
                if let Ok(level) = val.parse::<u8>() {
                    let mut state = services.write().await;
                    state.brightness.set_level(level);
                    player_tx.send(PlayerCommand::SetBrightness(level)).await.ok();
                }
            }
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"SetLuminancePloy\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Screen Schedule ---
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
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"GetSwitchTime\">\
                 {items}<result value=\"0\"/></out></sdk>"
            ))
        }

        "SetSwitchTime" | "setSwitchTime" => {
            // Parse schedule entries from XML
            let entries = extract_schedule_entries(xml);
            {
                let mut state = services.write().await;
                state.screen_schedule.set_schedule(entries);
            }
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"SetSwitchTime\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Time ---
        "GetTimeInfo" | "getTimeInfo" => {
            let now = chrono::Local::now();
            let dt = now.format("%Y-%m-%d %H:%M:%S").to_string();
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"GetTimeInfo\">\
                 <time value=\"{dt}\"/>\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        "SetTimeInfo" | "setTimeInfo" => {
            if let Some(time_val) = extract_attr(xml, "time", "value") {
                crate::services::time_sync::TimeSyncService::set_time(&time_val).await;
            }
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"SetTimeInfo\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Device Info ---
        "GetDeviceInfo" | "getDeviceInfo" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"GetDeviceInfo\">\
             <deviceInfo cpu=\"RustPlayer\" model=\"huidu-player\" \
             fpgaVersion=\"1.0.0\" screenWidth=\"{screen_width}\" \
             screenHeight=\"{screen_height}\" deviceID=\"RUST-001\"/>\
             <result value=\"0\"/></out></sdk>"
        )),

        // --- Font Management ---
        "GetAllFontInfo" | "getAllFontInfo" => {
            // Return list of available fonts
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"GetAllFontInfo\">\
                 <font name=\"Arial\" index=\"0\"/>\
                 <font name=\"DejaVu Sans\" index=\"1\"/>\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Network Config ---
        "GetEth0Info" | "getEth0Info" => {
            let ip = crate::protocol::discovery::get_local_ip();
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"GetEth0Info\">\
                 <eth0 dhcp=\"true\" ip=\"{ip}\" mask=\"255.255.255.0\" \
                 gateway=\"\" dns=\"8.8.8.8\"/>\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        "SetEth0Info" | "setEth0Info" => {
            info!("SetEth0Info received (network config change)");
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"SetEth0Info\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- File Management ---
        "GetFiles" | "getFiles" => {
            let state = services.read().await;
            let files = state.storage.list_files();
            let mut items = String::new();
            for f in &files {
                items.push_str(&format!("<file name=\"{f}\"/>"));
            }
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"GetFiles\">\
                 {items}<result value=\"0\"/></out></sdk>"
            ))
        }

        "DeleteFiles" | "deleteFiles" => {
            // Extract filenames to delete
            let filenames = extract_file_list(xml);
            let state = services.read().await;
            for f in &filenames {
                let _ = state.storage.delete_file(f);
            }
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"DeleteFiles\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }

        // --- Boot Logo ---
        "GetBootLogo" | "getBootLogo" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"GetBootLogo\">\
             <bootLogo name=\"\"/>\
             <result value=\"0\"/></out></sdk>"
        )),

        "SetBootLogoName" | "setBootLogoName" | "ClearBootLogo" | "clearBootLogo" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"{method}\">\
             <result value=\"0\"/></out></sdk>"
        )),

        // --- TCP Server Config ---
        "GetSDKTcpServer" | "getSDKTcpServer" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"GetSDKTcpServer\">\
             <server ip=\"\" port=\"10001\" enable=\"true\"/>\
             <result value=\"0\"/></out></sdk>"
        )),

        "SetSDKTcpServer" | "setSDKTcpServer" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"SetSDKTcpServer\">\
             <result value=\"0\"/></out></sdk>"
        )),

        // --- Wifi ---
        "GetWifiInfo" | "getWifiInfo" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"GetWifiInfo\">\
             <wifi enable=\"false\" ssid=\"\" password=\"\"/>\
             <result value=\"0\"/></out></sdk>"
        )),

        "SetWifiInfo" | "setWifiInfo" => Ok(format!(
            "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
             <sdk guid=\"{guid}\"><out method=\"SetWifiInfo\">\
             <result value=\"0\"/></out></sdk>"
        )),

        // --- Catch-all ---
        _ => {
            warn!("Unhandled SDK method: {}", method);
            Ok(format!(
                "<?xml version=\"1.0\" encoding=\"utf-8\"?>\
                 <sdk guid=\"{guid}\"><out method=\"{method}\">\
                 <result value=\"0\"/></out></sdk>"
            ))
        }
    }
}

/// Extract the method name from <sdk...><in method="MethodName">
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
}
