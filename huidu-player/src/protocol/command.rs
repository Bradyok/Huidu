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
            let guid = extract_attr(xml, "program", "guid").unwrap_or_default();
            if !guid.is_empty() {
                // Delete file from disk
                let state = services.read().await;
                let filename = crate::services::storage::StorageService::filename_for_guid(&guid);
                let _ = state.storage.delete_file(&filename);
                drop(state);
                // Remove from player's in-memory list
                player_tx.send(PlayerCommand::DeleteProgram(guid)).await.ok();
            }
            ok!("")
        }

        "GetAllProgram" | "getAllProgram" => {
            let state = services.read().await;
            let current = state.current_program_guid.clone();
            let mut items = String::new();
            for p in &state.programs {
                let is_current = if p.guid == current { "1" } else { "0" };
                items.push_str(&format!(
                    "<program guid=\"{}\" name=\"{}\" type=\"{}\" current=\"{}\"/>",
                    xml_esc(&p.guid), xml_esc(&p.name), xml_esc(&p.program_type), is_current
                ));
            }
            ok!(&items)
        }

        "GetProgram" | "getProgram" => {
            let req_guid = extract_attr(xml, "program", "guid").unwrap_or_default();
            let state = services.read().await;
            let raw = state.storage.load_program_xml_by_guid(&req_guid).unwrap_or_default();
            ok!(&raw)
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

        "SwitchProgramIndex" | "switchProgramIndex" => {
            let idx = extract_attr(xml, "index", "value")
                .or_else(|| extract_attr(xml, "program", "index"))
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(0);
            let state = services.read().await;
            if let Some(p) = state.programs.get(idx) {
                let guid = p.guid.clone();
                drop(state);
                player_tx.send(PlayerCommand::SwitchProgram(guid)).await.ok();
            }
            ok!("")
        }

        "GetCurrentPlayProgramGUID" | "getCurrentPlayProgramGUID" => {
            let state = services.read().await;
            let g = &state.current_program_guid;
            ok!(&format!("<program guid=\"{g}\"/>"))
        }

        "GetProgramList" | "getProgramList" => {
            let state = services.read().await;
            let count = state.programs.len();
            let mut items = String::new();
            for p in &state.programs {
                items.push_str(&format!(
                    "<program guid=\"{}\" name=\"{}\" type=\"{}\"/>",
                    xml_esc(&p.guid), xml_esc(&p.name), xml_esc(&p.program_type)
                ));
            }
            ok!(&format!("<programs count=\"{}\">{}</programs>", count, items))
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
            match parser::parse_program_xml(xml) {
                Ok(screen) => {
                    player_tx
                        .send(PlayerCommand::InsertProgram(screen.programs))
                        .await
                        .ok();
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
            // Delete XML program files not referenced by the currently loaded programs.
            // Media files (images, videos, audio) and config files are preserved.
            let state = services.read().await;
            let active: std::collections::HashSet<String> = state
                .programs
                .iter()
                .map(|p| crate::services::storage::StorageService::filename_for_guid(&p.guid))
                .collect();
            let files = state.storage.list_files();
            for f in &files {
                // Only remove XML program files; preserve device_state.json and media
                if f.ends_with(".xml") && f != "device_state.json" && !active.contains(f) {
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

        "ScreenRotation" | "screenRotation" | "SetRotation" | "setRotation" => {
            // <rotate value="90"/> or <rotation value="90"/>
            let deg = extract_attr(xml, "rotate", "value")
                .or_else(|| extract_attr(xml, "rotation", "value"))
                .and_then(|v| v.parse::<u16>().ok())
                .unwrap_or(0);
            {
                let mut state = services.write().await;
                state.rotation = deg;
                state.save_persisted();
            }
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
                let entries = extract_brightness_schedule(xml);
                let mut state = services.write().await;
                state.brightness.set_schedule(entries);
                state.save_persisted();
            } else {
                // Manual mode: <luminance mode="manual" value="N"/>
                if let Some(val) = extract_attr(xml, "luminance", "value")
                    && let Ok(level) = val.parse::<u8>() {
                        {
                            let mut state = services.write().await;
                            state.brightness.set_level(level);
                            state.save_persisted();
                        }
                        player_tx.send(PlayerCommand::SetBrightness(level)).await.ok();
                    }
            }
            ok!("")
        }

        // Simple single-value brightness setter (used by hdplayer-client SetBrightness)
        "SetBrightness" | "setBrightness" => {
            if let Some(val) = extract_attr(xml, "brightness", "value")
                && let Ok(level) = val.parse::<u8>() {
                    {
                        let mut state = services.write().await;
                        state.brightness.set_level(level);
                        state.save_persisted();
                    }
                    player_tx.send(PlayerCommand::SetBrightness(level)).await.ok();
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
            let mut state = services.write().await;
            state.screen_schedule.set_schedule(entries);
            state.save_persisted();
            ok!("")
        }

        // ── Time ───────────────────────────────────────────────────────────────
        "GetTimeInfo" | "getTimeInfo" => {
            let now = chrono::Local::now();
            let dt = now.format("%Y-%m-%d %H:%M:%S").to_string();
            let state = services.read().await;
            let ntp = xml_esc(&state.ntp_server);
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
                state.save_persisted();
            }
            ok!("")
        }

        // ── Device Info ────────────────────────────────────────────────────────
        "GetDeviceInfo" | "getDeviceInfo" => {
            let (name, dev_id, brightness, vol, rot, admin, prog_dir) = {
                let state = services.read().await;
                let name = state.device_name.clone();
                let dev_id = if state.device_id.is_empty() {
                    "RUST-001".to_string()
                } else {
                    state.device_id.clone()
                };
                let brightness = state.brightness.get_level();
                let vol = state.volume;
                let rot = state.rotation;
                let admin = state.admin_mode;
                let prog_dir = state.storage.program_dir().clone();
                (name, dev_id, brightness, vol, rot, admin, prog_dir)
            };
            let ip = crate::protocol::discovery::get_local_ip();
            let (storage_total, storage_free) = get_storage_bytes(&prog_dir);
            let mac = get_mac_address();
            let (dev_id, name) = (xml_esc(&dev_id), xml_esc(&name));
            ok!(&format!(
                "<deviceInfo \
                 deviceId=\"{dev_id}\" \
                 deviceName=\"{name}\" \
                 deviceType=\"52\" \
                 version=\"7.11.18.0\" \
                 screenWidth=\"{screen_width}\" \
                 screenHeight=\"{screen_height}\" \
                 ip=\"{ip}\" \
                 mac=\"{mac}\" \
                 brightness=\"{brightness}\" \
                 volume=\"{vol}\" \
                 rotation=\"{rot}\" \
                 adminMode=\"{admin}\" \
                 storageTotal=\"{storage_total}\" \
                 storageFree=\"{storage_free}\"/>"
            ))
        }

        "GetDeviceName" | "getDeviceName" => {
            let state = services.read().await;
            let name = xml_esc(&state.device_name);
            ok!(&format!("<name value=\"{name}\"/>"))
        }

        "SetDeviceName" | "setDeviceName" | "UpdateDevName" | "updateDevName" => {
            if let Some(name) = extract_attr(xml, "name", "value") {
                let mut state = services.write().await;
                state.device_name = name;
                state.save_persisted();
            }
            ok!("")
        }

        "GetHardwareInfo" | "getHardwareInfo" => {
            let arch = std::env::consts::ARCH;
            let os   = std::env::consts::OS;
            let prog_dir = {
                let state = services.read().await;
                state.storage.program_dir().clone()
            };
            let (storage_total, _) = get_storage_bytes(&prog_dir);
            let storage_mb = storage_total / 1024 / 1024;
            // Read real hardware stats in a blocking thread (file I/O)
            let (cpu_pct, mem_pct, temp_c, ram_mb) =
                tokio::task::spawn_blocking(get_system_stats).await.unwrap_or((5, 15, 40, 512));
            ok!(&format!(
                "<hardware cpu=\"{arch}\" os=\"{os}\" \
                 ram=\"{ram_mb}\" storage=\"{storage_mb}\" \
                 cpuUsage=\"{cpu_pct}\" memUsage=\"{mem_pct}\" temperature=\"{temp_c}\"/>"
            ))
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
            let (mask, gateway, dns) = tokio::task::spawn_blocking(read_eth0_full_config)
                .await
                .unwrap_or_else(|_| ("255.255.255.0".to_string(), String::new(), "8.8.8.8".to_string()));
            ok!(&format!(
                "<eth0 dhcp=\"true\" ip=\"{ip}\" mask=\"{mask}\" \
                 gateway=\"{gateway}\" dns=\"{dns}\"/>"
            ))
        }

        "SetEth0Info" | "setEth0Info" => {
            let dhcp    = extract_attr(xml, "eth0", "dhcp").map(|v| v == "true").unwrap_or(true);
            let ip      = extract_attr(xml, "eth0", "ip").unwrap_or_default();
            let mask    = extract_attr(xml, "eth0", "mask").unwrap_or_else(|| "255.255.255.0".to_string());
            let gateway = extract_attr(xml, "eth0", "gateway").unwrap_or_default();
            let dns     = extract_attr(xml, "eth0", "dns").unwrap_or_default();
            info!("SetEth0Info: dhcp={dhcp} ip={ip} mask={mask} gw={gateway} dns={dns}");
            tokio::task::spawn_blocking(move || apply_eth0_config(dhcp, &ip, &mask, &gateway, &dns)).await.ok();
            ok!("")
        }

        "GetPppoeInfo" | "getPppoeInfo" => {
            ok!("<pppoe enable=\"false\" user=\"\" password=\"\" status=\"disconnected\"/>")
        }

        "GetWifiInfo" | "getWifiInfo" => {
            let (ssid, status) = tokio::task::spawn_blocking(read_wifi_status)
                .await
                .unwrap_or_else(|_| (String::new(), "disconnected".to_string()));
            let enable = !ssid.is_empty();
            let ssid = xml_esc(&ssid);
            ok!(&format!(
                "<wifi enable=\"{enable}\" ssid=\"{ssid}\" password=\"\" status=\"{status}\"/>"
            ))
        }

        "SetWifiInfo" | "setWifiInfo" => {
            let ssid     = extract_attr(xml, "wifi", "ssid").unwrap_or_default();
            let password = extract_attr(xml, "wifi", "password").unwrap_or_default();
            if ssid.is_empty() {
                ok!("")
            } else {
                info!("SetWifiInfo: connecting to SSID '{ssid}'");
                tokio::task::spawn_blocking(move || apply_wifi_config(&ssid, &password)).await.ok();
                ok!("")
            }
        }

        "GetNetworkInfo" | "getNetworkInfo" => {
            let ip = crate::protocol::discovery::get_local_ip();
            let eth0 = ip != "0.0.0.0";
            // Quick internet check — try TCP connect to 8.8.8.8:53 (500 ms timeout)
            let internet = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                tokio::net::TcpStream::connect("8.8.8.8:53"),
            ).await.map(|r| r.is_ok()).unwrap_or(false);
            ok!(&format!(
                "<network eth0Connected=\"{eth0}\" wifiConnected=\"false\" \
                 internet=\"{internet}\" ip=\"{ip}\"/>",
                eth0 = if eth0 { "true" } else { "false" },
                internet = if internet { "true" } else { "false" },
            ))
        }

        // ── File Management ────────────────────────────────────────────────────
        "GetFiles" | "getFiles" => {
            let state = services.read().await;
            let files = state.storage.list_files();
            let mut items = String::new();
            for f in &files {
                items.push_str(&format!("<file name=\"{}\"/>", xml_esc(f)));
            }
            ok!(&items)
        }

        "GetFileChecklist" | "getFileChecklist" => {
            // Returns each file in the program directory with its MD5 hash and size.
            // The controller uses this to determine which files are already present
            // and intact before starting a file transfer session.
            let (files, program_dir) = {
                let state = services.read().await;
                (state.storage.list_files(), state.storage.program_dir().to_path_buf())
            };
            let count = files.len();
            let mut items = String::new();
            for f in &files {
                let path = program_dir.join(f);
                if let Ok(data) = std::fs::read(&path) {
                    let md5_hash = format!("{:x}", md5::compute(&data));
                    items.push_str(&format!(
                        "<file name=\"{}\" md5=\"{}\" size=\"{}\"/>",
                        xml_esc(f), md5_hash, data.len()
                    ));
                }
            }
            ok!(&format!("<files count=\"{}\">{}</files>", count, items))
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
            let state = services.read().await;
            let name = xml_esc(state.boot_logo.as_deref().unwrap_or(""));
            ok!(&format!("<bootLogo name=\"{name}\"/>"))
        }

        "SetBootLogoName" | "setBootLogoName" => {
            if let Some(name) = extract_attr(xml, "bootLogo", "name") {
                let mut state = services.write().await;
                state.boot_logo = if name.is_empty() { None } else { Some(name) };
                state.save_persisted();
            }
            ok!("")
        }

        "ClearBootLogo" | "clearBootLogo" => {
            let mut state = services.write().await;
            state.boot_logo = None;
            state.save_persisted();
            ok!("")
        }

        // ── TCP Server Config ──────────────────────────────────────────────────
        "GetSDKTcpServer" | "getSDKTcpServer" => {
            let state = services.read().await;
            let url = &state.cloud_url;
            let (ip, port) = if url.is_empty() {
                (String::new(), "10001".to_string())
            } else {
                let trimmed = url.trim_start_matches("http://").trim_start_matches("https://");
                let mut parts = trimmed.splitn(2, ':');
                let ip = parts.next().unwrap_or("").to_string();
                let port = parts.next().unwrap_or("10001").trim_end_matches('/').to_string();
                (ip, port)
            };
            let enable = !ip.is_empty();
            ok!(&format!("<server ip=\"{ip}\" port=\"{port}\" enable=\"{enable}\"/>"))
        }

        "SetSDKTcpServer" | "setSDKTcpServer" => {
            // <server ip="192.168.1.100" port="10001" enable="true"/>
            let ip   = extract_attr(xml, "server", "ip").unwrap_or_default();
            let port = extract_attr(xml, "server", "port").unwrap_or_else(|| "10001".to_string());
            if !ip.is_empty() {
                let url = format!("http://{}:{}", ip, port);
                info!("Cloud server URL updated to {}", url);
                let mut state = services.write().await;
                state.cloud_url = url;
            }
            ok!("")
        }

        // ── FPGA Hardware Config ───────────────────────────────────────────────
        "GetBoxHwConfig" | "getBoxHwConfig" | "GetSDKFPGAConfig" | "getSDKFPGAConfig" => {
            let state = services.read().await;
            let cfg = state.fpga_config.clone();
            ok!(&cfg)
        }

        "SetBoxHwConfig" | "setBoxHwConfig" | "SaveBoxHwConfig" | "saveBoxHwConfig"
        | "ReplaceBoxHwConfig" | "replaceBoxHwConfig"
        | "SetSDKFPGAConfig" | "setSDKFPGAConfig" => {
            // Extract the BoxHwConfig (or first XML element) and persist it.
            let config_xml = if let Some(start) = xml.find("<BoxHwConfig") {
                xml[start..].find("</BoxHwConfig>")
                    .map(|end| xml[start..start + end + "</BoxHwConfig>".len()].to_string())
            } else {
                None
            };
            if let Some(cfg) = config_xml {
                let mut state = services.write().await;
                state.fpga_config = cfg;
                info!("FPGA/BoxHwConfig updated");
            }
            ok!("")
        }

        "SmartSetting" | "smartSetting" | "SmartDrawLine" | "smartDrawLine" => ok!(""),

        // ── License ────────────────────────────────────────────────────────────
        "GetLicense" | "getLicense" => {
            let state = services.read().await;
            let valid = !state.license.is_empty();
            let lic = xml_esc(&state.license);
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
                state.save_persisted();
            }
            ok!("")
        }

        "UnlockAdminModePassword" | "unlockAdminModePassword" => ok!(""),

        // ── Volume ─────────────────────────────────────────────────────────────
        "GetSystemVolume" | "getSystemVolume" | "GetVolume" | "getVolume" => {
            let state = services.read().await;
            let vol = state.volume;
            ok!(&format!("<volume value=\"{vol}\"/>"))
        }

        "SetSystemVolume" | "setSystemVolume" | "SetVolume" | "setVolume" => {
            if let Some(v) = extract_attr(xml, "volume", "value")
                && let Ok(vol) = v.parse::<u8>() {
                    let clamped = vol.min(100);
                    {
                        let mut state = services.write().await;
                        state.volume = clamped;
                        state.save_persisted();
                    }
                    player_tx.send(PlayerCommand::SetVolume(clamped)).await.ok();
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
        "Reboot" | "reboot" | "RebootDevice" | "rebootDevice" => {
            info!("Reboot requested — initiating system reboot");
            // On Linux, spawn `reboot` in the background
            #[cfg(unix)]
            {
                let _ = std::process::Command::new("reboot").spawn();
            }
            ok!("")
        }

        "FirmwareUpgrade" | "firmwareUpgrade" => {
            let filename = extract_attr(xml, "upgrade", "file")
                .unwrap_or_else(|| "upgrade.zbin".to_string());
            let (upgrade_path, program_dir) = {
                let state = services.read().await;
                let dir = state.storage.program_dir().to_path_buf();
                (dir.join(&filename), dir)
            };
            info!("FirmwareUpgrade requested: {}", upgrade_path.display());

            if !upgrade_path.exists() {
                warn!("Upgrade file not found: {}", upgrade_path.display());
                ok!("<upgradeStatus value=\"error\" message=\"file not found\"/>")
            } else {
                // Stage the .zbin package (parse ZIP, extract files, write pending marker)
                let stage_result = tokio::task::spawn_blocking({
                    let p = upgrade_path.clone();
                    move || crate::services::upgrade::stage_upgrade(&p)
                })
                .await
                .unwrap_or_else(|e| Err(format!("spawn_blocking error: {e}")));

                match stage_result {
                    Ok(version) => {
                        info!("Upgrade staged: {filename} v{version}");
                        crate::services::upgrade::write_pending_marker(
                            &program_dir,
                            &filename,
                            &version,
                        );
                        {
                            let mut state = services.write().await;
                            state.upgrade_status =
                                crate::services::upgrade::UpgradeStatus::InProgress {
                                    filename: filename.clone(),
                                };
                            state.save_persisted();
                        }

                        // Trigger a graceful restart after 3 s so the TCP response
                        // is delivered before the connection drops.
                        let svc = services.clone();
                        tokio::spawn(async move {
                            tokio::time::sleep(tokio::time::Duration::from_secs(3)).await;
                            info!("Initiating graceful restart for firmware upgrade");
                            if let Some(ref tok) = svc.read().await.shutdown_token {
                                tok.cancel();
                            }
                        });

                        ok!("<upgradeStatus value=\"upgrading\"/>")
                    }
                    Err(e) => {
                        warn!("Upgrade staging failed: {e}");
                        {
                            let mut state = services.write().await;
                            state.upgrade_status =
                                crate::services::upgrade::UpgradeStatus::Failed {
                                    reason: e.clone(),
                                };
                            state.save_persisted();
                        }
                        let msg = e.replace('"', "&quot;");
                        ok!(&format!(
                            "<upgradeStatus value=\"error\" message=\"{msg}\"/>"
                        ))
                    }
                }
            }
        }

        "ExcuteUpgradeShell" | "excuteUpgradeShell" => {
            // <cmd value="/path/to/script.sh [args]"/>
            let cmd = extract_attr(xml, "cmd", "value")
                .or_else(|| extract_attr(xml, "shell", "value"))
                .unwrap_or_default();
            if cmd.is_empty() {
                ok!("")
            } else {
                // Only allow execution in admin mode as a basic safety check.
                let admin = services.read().await.admin_mode;
                if !admin {
                    warn!("ExcuteUpgradeShell rejected — admin mode not enabled");
                    ok!("<result value=\"error\" message=\"requires admin mode\"/>")
                } else {
                    info!("ExcuteUpgradeShell: {cmd}");
                    #[cfg(unix)]
                    {
                        let cmd_owned = cmd.clone();
                        tokio::task::spawn_blocking(move || {
                            let _ = std::process::Command::new("sh")
                                .args(["-c", &cmd_owned])
                                .spawn();
                        }).await.ok();
                    }
                    ok!("")
                }
            }
        }

        "GetUpgradeResult" | "getUpgradeResult" => {
            let (value, message) = services.read().await.upgrade_status.sdk_response();
            let msg = message.replace('"', "&quot;");
            ok!(&format!("<upgradeResult value=\"{value}\" message=\"{msg}\"/>"))
        }

        // ── Data Sources ───────────────────────────────────────────────────────
        "GetDataSourceInfo" | "getDataSourceInfo" => {
            let state = services.read().await;
            let count = state.data_sources.len();
            let mut body = format!("<dataSources count=\"{}\">", count);
            for (name, value) in &state.data_sources {
                body.push_str(&format!(
                    "<dataSource name=\"{}\" value=\"{}\"/>",
                    xml_esc(name), xml_esc(value)
                ));
            }
            body.push_str("</dataSources>");
            ok!(body)
        }

        "SetDataSourceInfo" | "setDataSourceInfo" => {
            let sources = extract_data_sources(xml);
            let count = sources.len();
            {
                let mut state = services.write().await;
                state.data_sources = sources;
                state.save_persisted();
            }
            info!("Data sources updated: {} entries", count);
            ok!("")
        }

        "ReloadDeviceID" | "reloadDeviceID" => {
            // Reads device_id.txt from the program directory and updates the device ID.
            let program_dir = {
                let state = services.read().await;
                state.storage.program_dir().to_path_buf()
            };
            let id_file = program_dir.join("device_id.txt");
            if let Ok(contents) = std::fs::read_to_string(&id_file) {
                let new_id = contents.trim().to_string();
                if !new_id.is_empty() {
                    info!("Device ID reloaded from file: {}", new_id);
                    let mut state = services.write().await;
                    state.device_id = new_id;
                }
            }
            ok!("")
        }

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

/// Try to read the real MAC address of the first non-loopback network interface.
/// Falls back to all-zeros if unavailable (e.g. on Windows or embedded boards
/// where /sys/class/net is not present).
fn get_mac_address() -> String {
    #[cfg(unix)]
    {
        if let Ok(entries) = std::fs::read_dir("/sys/class/net") {
            let mut names: Vec<String> = entries
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect();
            names.sort(); // deterministic order
            for name in &names {
                if name == "lo" {
                    continue;
                }
                let mac_path =
                    std::path::Path::new("/sys/class/net").join(name).join("address");
                if let Ok(mac) = std::fs::read_to_string(&mac_path) {
                    let mac = mac.trim().to_string();
                    if !mac.is_empty() && mac != "00:00:00:00:00:00" {
                        return mac;
                    }
                }
            }
        }
    }
    "00:00:00:00:00:00".to_string()
}

/// Read real hardware stats.
/// Returns `(cpu_usage_pct, mem_usage_pct, temperature_celsius, ram_total_mb)`.
/// On Linux these are read from /proc and /sys; on other platforms defaults are used.
fn get_system_stats() -> (u8, u8, i32, u64) {
    #[cfg(unix)]
    {
        let (mem_pct, ram_mb) = read_mem_info().unwrap_or((15, 512));
        let temp = read_cpu_temp_c().unwrap_or(40);
        let cpu = read_cpu_usage_pct().unwrap_or(5);
        return (cpu, mem_pct, temp, ram_mb);
    }
    #[allow(unreachable_code)]
    (5, 15, 40, 512)
}

/// Parse /proc/meminfo to get (used_percent, total_mb).
#[cfg(unix)]
fn read_mem_info() -> Option<(u8, u64)> {
    let s = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total_kb = 0u64;
    let mut avail_kb = 0u64;
    for line in s.lines() {
        if line.starts_with("MemTotal:") {
            total_kb = line.split_whitespace().nth(1)?.parse().ok()?;
        } else if line.starts_with("MemAvailable:") {
            avail_kb = line.split_whitespace().nth(1)?.parse().ok()?;
        }
    }
    if total_kb == 0 {
        return None;
    }
    let used_kb = total_kb.saturating_sub(avail_kb);
    let pct = ((used_kb * 100 / total_kb) as u8).min(100);
    Some((pct, total_kb / 1024))
}

/// Read CPU temperature in degrees Celsius from a thermal zone sysfs node.
#[cfg(unix)]
fn read_cpu_temp_c() -> Option<i32> {
    for zone in 0..4u8 {
        let path = format!("/sys/class/thermal/thermal_zone{}/temp", zone);
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(milli) = s.trim().parse::<i32>() {
                return Some(milli / 1000);
            }
        }
    }
    None
}

/// Estimate CPU usage from a single snapshot of /proc/stat.
/// Returns the percentage of time NOT spent idle since boot — a rough but
/// zero-overhead approximation that avoids needing two timed samples.
#[cfg(unix)]
fn read_cpu_usage_pct() -> Option<u8> {
    let s = std::fs::read_to_string("/proc/stat").ok()?;
    let first = s.lines().next()?; // "cpu  user nice system idle iowait irq softirq ..."
    let nums: Vec<u64> = first
        .split_whitespace()
        .skip(1) // skip "cpu" label
        .filter_map(|v| v.parse().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    let total: u64 = nums.iter().sum();
    let idle = nums[3];
    if total == 0 {
        return None;
    }
    Some((((total - idle) * 100 / total) as u8).min(100))
}

/// Read current eth0 network configuration from the OS.
/// Returns `(mask, gateway, dns)`.  Falls back to safe defaults if unavailable.
fn read_eth0_full_config() -> (String, String, String) {
    let mask    = read_eth0_mask().unwrap_or_else(|| "255.255.255.0".to_string());
    let gateway = read_default_gateway().unwrap_or_default();
    let dns     = read_first_dns().unwrap_or_else(|| "8.8.8.8".to_string());
    (mask, gateway, dns)
}

/// Apply eth0 network config.  On Linux, uses `ip` commands.
/// No-op on Windows (network managed externally).
fn apply_eth0_config(dhcp: bool, ip: &str, mask: &str, gateway: &str, dns: &str) {
    #[cfg(unix)]
    {
        if dhcp {
            // Try udhcpc (BusyBox/embedded) then dhclient (Debian/Ubuntu)
            let udhcpc = std::process::Command::new("udhcpc")
                .args(["-i", "eth0", "-n", "-q"])
                .status();
            if udhcpc.is_err() || udhcpc.map(|s| !s.success()).unwrap_or(true) {
                let _ = std::process::Command::new("dhclient")
                    .args(["eth0"])
                    .status();
            }
        } else if !ip.is_empty() {
            let prefix = mask_to_prefix_len(mask);
            let _ = std::process::Command::new("ip")
                .args(["addr", "flush", "dev", "eth0"])
                .status();
            let _ = std::process::Command::new("ip")
                .args(["addr", "add", &format!("{ip}/{prefix}"), "dev", "eth0"])
                .status();
            let _ = std::process::Command::new("ip")
                .args(["link", "set", "eth0", "up"])
                .status();
            if !gateway.is_empty() {
                // Delete existing default route first (ignore errors)
                let _ = std::process::Command::new("ip")
                    .args(["route", "del", "default"])
                    .status();
                let _ = std::process::Command::new("ip")
                    .args(["route", "add", "default", "via", gateway, "dev", "eth0"])
                    .status();
            }
            if !dns.is_empty() {
                let _ = std::fs::write("/etc/resolv.conf", format!("nameserver {dns}\n"));
            }
        }
    }
    // Windows: network configuration is handled by the OS; log only.
    #[cfg(not(unix))]
    {
        let _ = (dhcp, ip, mask, gateway, dns);
    }
}

/// Convert a dotted-decimal subnet mask to a CIDR prefix length.
/// e.g. "255.255.255.0" → 24
fn mask_to_prefix_len(mask: &str) -> u32 {
    let parts: Vec<u8> = mask.split('.').filter_map(|s| s.parse().ok()).collect();
    if parts.len() != 4 {
        return 24;
    }
    let n = u32::from_be_bytes([parts[0], parts[1], parts[2], parts[3]]);
    n.count_ones() // works because valid masks are contiguous leading 1s
}

/// Read the current WiFi SSID and connection status.
/// Returns `(ssid, status)` where status is "connected"/"disconnected".
/// Uses nmcli if available; falls back to iw; returns empty on non-Linux or if unavailable.
fn read_wifi_status() -> (String, String) {
    #[cfg(unix)]
    {
        // Try nmcli first (NetworkManager)
        if let Ok(out) = std::process::Command::new("nmcli")
            .args(["-t", "-f", "DEVICE,TYPE,STATE,CONNECTION", "device"])
            .output()
        {
            if let Ok(s) = std::str::from_utf8(&out.stdout) {
                for line in s.lines() {
                    let parts: Vec<&str> = line.split(':').collect();
                    if parts.len() >= 4 && parts[1] == "wifi" {
                        let status = if parts[2].contains("connected") { "connected" } else { "disconnected" };
                        let ssid = parts[3].trim().to_string();
                        if !ssid.is_empty() && ssid != "--" {
                            return (ssid, status.to_string());
                        }
                        return (String::new(), status.to_string());
                    }
                }
            }
        }

        // Fallback: iw dev
        if let Ok(out) = std::process::Command::new("iw").args(["dev"]).output() {
            if let Ok(s) = std::str::from_utf8(&out.stdout) {
                for line in s.lines() {
                    let line = line.trim();
                    if let Some(rest) = line.strip_prefix("ssid ") {
                        return (rest.trim().to_string(), "connected".to_string());
                    }
                }
            }
        }
    }
    (String::new(), "disconnected".to_string())
}

/// Apply WiFi configuration using nmcli.
/// On non-Linux systems or when nmcli is absent, this is a no-op.
fn apply_wifi_config(ssid: &str, password: &str) {
    #[cfg(unix)]
    {
        let mut args = vec!["device", "wifi", "connect", ssid];
        let pw_owned;
        if !password.is_empty() {
            pw_owned = password.to_string();
            args.extend_from_slice(&["password", &pw_owned]);
        }
        let status = std::process::Command::new("nmcli").args(&args).status();
        match status {
            Ok(s) if s.success() => tracing::info!("WiFi connected to '{ssid}'"),
            Ok(s) => tracing::warn!("nmcli wifi connect failed (exit {})", s),
            Err(_) => tracing::warn!("nmcli not available — WiFi config not applied"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (ssid, password);
    }
}

/// Read the subnet mask of the first non-loopback IPv4 interface from `ip addr`.
/// Returns a dotted-decimal mask string, e.g. "255.255.255.0".
#[cfg(unix)]
fn read_eth0_mask() -> Option<String> {
    let out = std::process::Command::new("ip")
        .args(["-4", "addr", "show"])
        .output()
        .ok()?;
    let s = std::str::from_utf8(&out.stdout).ok()?;
    for line in s.lines() {
        let line = line.trim();
        if line.starts_with("inet ") {
            // "inet 192.168.1.100/24 brd ..."
            let cidr = line.split_whitespace().nth(1)?;
            let prefix: u32 = cidr.split('/').nth(1)?.parse().ok()?;
            let addr: u32 = cidr.split('/').next()?.parse::<std::net::Ipv4Addr>().ok().map(u32::from)?;
            if addr == 0x7f000001 { continue; } // skip loopback
            // prefix-length → dotted-decimal mask
            let mask_bits = if prefix == 0 { 0u32 } else { u32::MAX << (32 - prefix) };
            let b = mask_bits.to_be_bytes();
            return Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]));
        }
    }
    None
}

#[cfg(not(unix))]
fn read_eth0_mask() -> Option<String> { None }

/// Parse /proc/net/route to find the default gateway.
/// The default route has Destination == 00000000.
/// The Gateway column is a little-endian 32-bit hex IPv4 address.
#[cfg(unix)]
fn read_default_gateway() -> Option<String> {
    let s = std::fs::read_to_string("/proc/net/route").ok()?;
    for line in s.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 3 {
            continue;
        }
        if cols[1] == "00000000" {
            // cols[2] = gateway as little-endian hex
            let hex = u32::from_str_radix(cols[2], 16).ok()?;
            let bytes = hex.to_le_bytes(); // little-endian in /proc/net/route
            return Some(format!("{}.{}.{}.{}", bytes[0], bytes[1], bytes[2], bytes[3]));
        }
    }
    None
}

#[cfg(not(unix))]
fn read_default_gateway() -> Option<String> {
    None
}

/// Parse /etc/resolv.conf for the first `nameserver` entry.
#[cfg(unix)]
fn read_first_dns() -> Option<String> {
    let s = std::fs::read_to_string("/etc/resolv.conf").ok()?;
    for line in s.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("nameserver") {
            let addr = rest.trim();
            if !addr.is_empty() {
                return Some(addr.to_string());
            }
        }
    }
    None
}

#[cfg(not(unix))]
fn read_first_dns() -> Option<String> {
    None
}

/// Return (total_bytes, free_bytes) for the filesystem containing `path`.
/// Falls back to (0, 0) if the info is unavailable.
fn get_storage_bytes(path: &std::path::Path) -> (u64, u64) {
    // Use statvfs on Linux; approximate with dir walk on other platforms.
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap_or_default();
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } == 0 {
            let block = stat.f_frsize as u64;
            let total = stat.f_blocks as u64 * block;
            let free = stat.f_bavail as u64 * block;
            return (total, free);
        }
    }
    // Windows / fallback: scan directory size
    let used: u64 = std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .map(|m| m.len())
        .sum();
    // Report 8 GB disk, used bytes consumed
    let total = 8u64 * 1024 * 1024 * 1024;
    let free = total.saturating_sub(used);
    (total, free)
}

/// Extract all `<dataSource name="…" value="…"/>` elements from XML.
fn extract_data_sources(xml: &str) -> std::collections::HashMap<String, String> {
    let mut sources = std::collections::HashMap::new();
    let mut from = 0;
    while let Some(pos) = xml[from..].find("<dataSource ") {
        let abs = from + pos;
        let name = extract_attr(&xml[abs..], "dataSource", "name").unwrap_or_default();
        let value = extract_attr(&xml[abs..], "dataSource", "value").unwrap_or_default();
        if !name.is_empty() {
            sources.insert(name, value);
        }
        from = abs + 12;
    }
    sources
}

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

/// Escape the five predefined XML entities so that any string is safe to
/// embed inside an XML attribute value (double-quoted).
///
/// No-op if the string contains none of the special characters.
#[inline]
fn xml_esc(s: &str) -> String {
    if !s.contains(['&', '<', '>', '"', '\'']) {
        return s.to_string();
    }
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
     .replace('\'', "&apos;")
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

    #[test]
    fn test_extract_data_sources_empty() {
        let xml = r#"<sdk><in method="SetDataSourceInfo"></in></sdk>"#;
        let sources = extract_data_sources(xml);
        assert!(sources.is_empty());
    }

    #[test]
    fn test_extract_data_sources_single() {
        let xml = r#"<sdk><in method="SetDataSourceInfo"><dataSource name="ticker" value="AAPL 150.25"/></in></sdk>"#;
        let sources = extract_data_sources(xml);
        assert_eq!(sources.len(), 1);
        assert_eq!(sources.get("ticker").map(|s| s.as_str()), Some("AAPL 150.25"));
    }

    #[test]
    fn test_extract_data_sources_multiple() {
        let xml = r#"<sdk><in method="SetDataSourceInfo">
            <dataSource name="score" value="3-1"/>
            <dataSource name="team" value="Home"/>
        </in></sdk>"#;
        let sources = extract_data_sources(xml);
        assert_eq!(sources.len(), 2);
        assert_eq!(sources.get("score").map(|s| s.as_str()), Some("3-1"));
        assert_eq!(sources.get("team").map(|s| s.as_str()), Some("Home"));
    }

    // ── extract_brightness_schedule ──────────────────────────────────────────

    #[test]
    fn test_extract_brightness_schedule_single() {
        let xml = r#"<luminance mode="auto"><item time="08:00" level="100"/></luminance>"#;
        let entries = extract_brightness_schedule(xml);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].hour, 8);
        assert_eq!(entries[0].minute, 0);
        assert_eq!(entries[0].level, 100);
    }

    #[test]
    fn test_extract_brightness_schedule_multiple() {
        let xml = r#"<luminance mode="auto">
            <item time="07:30" level="80"/>
            <item time="22:00" level="30"/>
        </luminance>"#;
        let entries = extract_brightness_schedule(xml);
        assert_eq!(entries.len(), 2);
        assert_eq!((entries[0].hour, entries[0].minute, entries[0].level), (7, 30, 80));
        assert_eq!((entries[1].hour, entries[1].minute, entries[1].level), (22, 0, 30));
    }

    #[test]
    fn test_extract_brightness_schedule_empty() {
        let xml = r#"<luminance mode="manual"><brightness value="75"/></luminance>"#;
        let entries = extract_brightness_schedule(xml);
        assert!(entries.is_empty());
    }

    // ── extract_file_list ─────────────────────────────────────────────────────

    #[test]
    fn test_extract_file_list_single() {
        let xml = r#"<in method="DeleteFiles"><file name="logo.png"/></in>"#;
        let files = extract_file_list(xml);
        assert_eq!(files, vec!["logo.png"]);
    }

    #[test]
    fn test_extract_file_list_multiple() {
        let xml = r#"<in method="DeleteFiles"><file name="a.mp4"/><file name="b.jpg"/></in>"#;
        let files = extract_file_list(xml);
        assert_eq!(files, vec!["a.mp4", "b.jpg"]);
    }

    #[test]
    fn test_extract_file_list_empty() {
        let xml = r#"<in method="DeleteFiles"></in>"#;
        let files = extract_file_list(xml);
        assert!(files.is_empty());
    }

    // ── xml_esc ───────────────────────────────────────────────────────────────

    #[test]
    fn test_xml_esc_passthrough() {
        assert_eq!(xml_esc("hello world"), "hello world");
        assert_eq!(xml_esc(""), "");
        assert_eq!(xml_esc("LED Sign v2"), "LED Sign v2");
    }

    #[test]
    fn test_xml_esc_ampersand() {
        assert_eq!(xml_esc("A & B"), "A &amp; B");
    }

    #[test]
    fn test_xml_esc_lt_gt() {
        assert_eq!(xml_esc("<tag>"), "&lt;tag&gt;");
    }

    #[test]
    fn test_xml_esc_quote() {
        assert_eq!(xml_esc("say \"hello\""), "say &quot;hello&quot;");
    }

    #[test]
    fn test_xml_esc_all_entities() {
        assert_eq!(
            xml_esc("<\"it's\" & fine>"),
            "&lt;&quot;it&apos;s&quot; &amp; fine&gt;"
        );
    }

    // ── mask_to_prefix_len ────────────────────────────────────────────────────

    #[test]
    fn test_mask_to_prefix_len_class_c() {
        assert_eq!(mask_to_prefix_len("255.255.255.0"), 24);
    }

    #[test]
    fn test_mask_to_prefix_len_class_b() {
        assert_eq!(mask_to_prefix_len("255.255.0.0"), 16);
    }

    #[test]
    fn test_mask_to_prefix_len_class_a() {
        assert_eq!(mask_to_prefix_len("255.0.0.0"), 8);
    }

    #[test]
    fn test_mask_to_prefix_len_full() {
        assert_eq!(mask_to_prefix_len("255.255.255.255"), 32);
    }

    #[test]
    fn test_mask_to_prefix_len_slash28() {
        assert_eq!(mask_to_prefix_len("255.255.255.240"), 28);
    }

    #[test]
    fn test_mask_to_prefix_len_invalid_defaults_24() {
        assert_eq!(mask_to_prefix_len("not.a.mask"), 24);
    }
}
