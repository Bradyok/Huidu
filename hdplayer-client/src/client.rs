//! TCP client for communicating with Huidu BoxPlayer devices.
//!
//! Implements the full SDK XML command protocol plus file transfer.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;


use crate::command;
use crate::error::{Error, Result};
use crate::protocol::{Command, Packet};
use crate::transfer::FileTransfer;
use crate::xml;

/// Default TCP port for Huidu BoxPlayer SDK protocol.
pub const DEFAULT_PORT: u16 = huidu_protocol::packet::DEFAULT_PORT;

/// SDK protocol version sent in SdkServiceAsk (confirmed from HCatNet.dll analysis).
pub const SDK_VERSION: u32 = huidu_protocol::packet::SDK_CLIENT_VERSION;

/// Heartbeat interval.
pub const HEARTBEAT_INTERVAL: Duration = huidu_protocol::packet::HEARTBEAT_INTERVAL;

/// Response timeout for SDK commands.
pub const CMD_TIMEOUT: Duration = Duration::from_secs(15);

/// Information returned by GetDeviceInfo.
#[derive(Debug, Clone, Default)]
pub struct DeviceDetails {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    pub firmware_version: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub storage_total: u64,
    pub storage_free: u64,
    pub ip_address: String,
    pub mac_address: String,
    pub volume: u8,
    pub brightness: u8,
    pub rotation: u32,
    pub admin_mode: bool,
    pub current_program_guid: Option<String>,
    pub raw_xml: String,
}

/// Program metadata returned by GetAllProgram.
#[derive(Debug, Clone)]
pub struct ProgramInfo {
    pub guid: String,
    pub name: String,
    pub program_type: String,
    pub is_current: bool,
}

/// File entry returned by GetFileChecklist.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub md5: String,
    pub size: u64,
}

/// Ethernet configuration.
#[derive(Debug, Clone)]
pub struct EthConfig {
    pub dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
}

/// Client for a single connected Huidu device.
pub struct Client {
    stream: Arc<Mutex<TcpStream>>,
    client_guid: String,
    read_buf: Vec<u8>,
}

impl Client {
    /// Connect to a Huidu BoxPlayer at the given host and port.
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        let addr = format!("{host}:{port}");
        info!("Connecting to {addr}");
        let stream = TcpStream::connect(&addr).await
            .map_err(|e| Error::Connection(format!("TCP connect to {addr}: {e}")))?;
        stream.set_nodelay(true)?;
        info!("Connected to {addr}");

        let mut client = Self {
            stream: Arc::new(Mutex::new(stream)),
            client_guid: Uuid::new_v4().to_string().to_uppercase(),
            read_buf: Vec::with_capacity(65536),
        };

        // Negotiate SDK protocol version
        client.send_sdk_service().await?;
        Ok(client)
    }

    /// Connect using the default port (10001).
    pub async fn connect_default(host: &str) -> Result<Self> {
        Self::connect(host, DEFAULT_PORT).await
    }

    // ── Internal helpers ─────────────────────────────────────────────────

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes();
        let hex: String = bytes.iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("-");
        debug!("Sent {:?} ({} bytes): {}", packet.command, bytes.len(), hex);
        let mut stream = self.stream.lock().await;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Packet> {
        loop {
            // Try to parse from buffer
            if let Some((packet, consumed)) = Packet::from_bytes(&self.read_buf)? {
                self.read_buf.drain(..consumed);
                debug!("Recv {:?} ({} bytes payload)", packet.command, packet.payload.len());
                return Ok(packet);
            }
            // Need more data
            let mut tmp = vec![0u8; 4096];
            let mut stream = self.stream.lock().await;
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(Error::Connection("connection closed by device".into()));
            }
            drop(stream);
            let hex: String = tmp[..n].iter().map(|b| format!("{b:02X}")).collect::<Vec<_>>().join("-");
            debug!("Recv raw {} bytes: {}", n, hex);
            self.read_buf.extend_from_slice(&tmp[..n]);
        }
    }

    async fn recv_with_timeout(&mut self, timeout: Duration) -> Result<Packet> {
        tokio::time::timeout(timeout, self.recv_packet())
            .await
            .map_err(|_| Error::Timeout)?
    }

    /// Send SDK service negotiation packet.
    async fn send_sdk_service(&mut self) -> Result<()> {
        use crate::protocol::sdk_service_ask_payload;
        let payload = sdk_service_ask_payload(SDK_VERSION);
        let pkt = Packet::new(Command::SdkServiceAsk, payload);
        self.send_packet(&pkt).await?;
        // Wait for SdkServiceAnswer
        let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
        match resp.command {
            Command::SdkServiceAnswer => {}
            Command::SdkErrorAnswer => {
                let code = if resp.payload.len() >= 2 {
                    u16::from_le_bytes(resp.payload[..2].try_into().unwrap_or([0; 2])) as i32
                } else {
                    -1
                };
                let de = crate::error::DeviceError::from_code(code);
                return Err(Error::Device {
                    code,
                    message: format!("SDK version rejected: {}", de.message()),
                });
            }
            other => {
                warn!("Expected SdkServiceAnswer, got {:?}", other);
            }
        }
        Ok(())
    }

    /// Send an SDK XML command and receive the XML response.
    ///
    /// SDK CMD payload framing (confirmed from server.rs):
    ///   Request:  [u32 total_xml_len][u32 chunk_index=0][xml_bytes...]
    ///   Response: [u32 total_xml_len][u32 chunk_index=0][xml_bytes...]
    async fn sdk_cmd(&mut self, method: &str, body: &str) -> Result<String> {
        use crate::protocol::sdk_cmd_ask_payload;
        let xml_str = xml::sdk_request(&self.client_guid, method, body);
        let payload = sdk_cmd_ask_payload(&xml_str);
        let pkt = Packet::new(Command::SdkCmdAsk, payload);
        self.send_packet(&pkt).await?;

        // Receive response — may need to skip heartbeat packets
        loop {
            let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
            match resp.command {
                Command::SdkCmdAnswer => {
                    // Strip the [u32 total_len][u32 index] 8-byte prefix from response
                    let xml_bytes = crate::protocol::parse_sdk_cmd_payload(&resp.payload)
                        .ok_or_else(|| Error::Protocol(
                            format!("SdkCmdAnswer too short: {} bytes", resp.payload.len())
                        ))?;
                    let response_xml = String::from_utf8_lossy(xml_bytes).into_owned();
                    debug!("SDK response for {method}: {} bytes", response_xml.len());
                    xml::parse_result(&response_xml)?;
                    return Ok(response_xml);
                }
                Command::SdkErrorAnswer => {
                    let code = if resp.payload.len() >= 2 {
                        u16::from_le_bytes(resp.payload[..2].try_into().unwrap_or([0; 2])) as i32
                    } else {
                        -1
                    };
                    let de = crate::error::DeviceError::from_code(code);
                    return Err(Error::Device {
                        code,
                        message: de.message().into(),
                    });
                }
                Command::TcpHeartbeatAsk => {
                    // Auto-respond to heartbeat
                    let pong = Packet::heartbeat();
                    self.send_packet(&pong).await?;
                }
                other => {
                    warn!("Unexpected packet {:?} while waiting for SdkCmdAnswer", other);
                }
            }
        }
    }

    /// Send a heartbeat packet.
    pub async fn heartbeat(&mut self) -> Result<()> {
        let pkt = Packet::heartbeat();
        self.send_packet(&pkt).await
    }

    // ── Device Info ──────────────────────────────────────────────────────

    /// Get device information.
    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        let xml = self.sdk_cmd("GetDeviceInfo", &command::get_device_info()).await?;
        let mut info = DeviceDetails { raw_xml: xml.clone(), ..Default::default() };
        // Narrow to the <deviceInfo ...> element so get_attr("version") doesn't
        // hit <?xml version="1.0"?> in the envelope declaration.
        let scope = xml.find("<deviceInfo")
            .and_then(|s| xml[s..].find('>').map(|e| &xml[s..s + e + 1]))
            .unwrap_or(xml.as_str());
        let get = |a: &str| xml::get_attr(scope, a).map(xml::xml_unescape);
        info.device_id    = get("deviceId").or_else(|| get("deviceID")).unwrap_or_default();
        info.device_name  = get("deviceName").or_else(|| get("name")).unwrap_or_default();
        info.firmware_version = get("version").or_else(|| get("SoftwareVersion"))
                                    .or_else(|| get("softwareVersion")).unwrap_or_default();
        info.screen_width  = get("screenWidth").or_else(|| get("ScreenWidth"))
                                .or_else(|| get("width"))
                                .and_then(|v| v.parse().ok()).unwrap_or(0);
        info.screen_height = get("screenHeight").or_else(|| get("ScreenHeight"))
                                .or_else(|| get("height"))
                                .and_then(|v| v.parse().ok()).unwrap_or(0);
        info.device_type   = get("deviceType").or_else(|| get("DeviceType")).unwrap_or_default();
        info.ip_address    = get("ip").unwrap_or_default();
        info.mac_address   = get("mac").unwrap_or_default();
        info.brightness    = get("brightness").and_then(|v| v.parse().ok()).unwrap_or(100);
        info.volume        = get("volume").and_then(|v| v.parse().ok()).unwrap_or(50);
        info.rotation      = get("rotation").and_then(|v| v.parse().ok()).unwrap_or(0);
        info.admin_mode    = get("adminMode").map(|v| v == "true" || v == "1").unwrap_or(false);
        info.storage_total = get("storageTotal").and_then(|v| v.parse().ok()).unwrap_or(0);
        info.storage_free  = get("storageFree").and_then(|v| v.parse().ok()).unwrap_or(0);
        Ok(info)
    }

    pub async fn get_hardware_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetHardwareInfo", &command::get_hardware_info()).await
    }

    pub async fn get_sdk_version(&mut self) -> Result<u32> {
        let xml = self.sdk_cmd("GetIFVersion", &command::get_if_version()).await?;
        let v = xml::get_attr(&xml, "version")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        Ok(v)
    }

    /// Capture a screenshot. Returns PNG bytes.
    pub async fn screenshot(&mut self) -> Result<Vec<u8>> {
        let xml = self.sdk_cmd("GetScreenshot2", &command::get_screenshot2(0, 0)).await?;
        // Response contains base64-encoded image in <data> tag or attribute
        let data_b64 = xml::get_attr(&xml, "data")
            .or_else(|| xml::get_tag_text(&xml, "data"))
            .unwrap_or("");
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data_b64.trim())
            .map_err(|e| Error::InvalidResponse(format!("base64 decode: {e}")))?;
        Ok(bytes)
    }

    // ── Program Management ───────────────────────────────────────────────

    /// Upload a complete program to the device.
    /// `screen_xml` is the `<screen>...</screen>` block (without SDK envelope).
    pub async fn add_program(&mut self, screen_xml: &str) -> Result<()> {
        self.sdk_cmd("AddProgram", &command::add_program(screen_xml)).await?;
        Ok(())
    }

    /// Update an existing program.
    pub async fn update_program(&mut self, screen_xml: &str) -> Result<()> {
        self.sdk_cmd("UpdateProgram", &command::update_program(screen_xml)).await?;
        Ok(())
    }

    /// Delete a program by GUID.
    pub async fn delete_program(&mut self, guid: &str) -> Result<()> {
        self.sdk_cmd("DeleteProgram", &command::delete_program(guid)).await?;
        Ok(())
    }

    /// Switch to a program by GUID.
    pub async fn switch_program(&mut self, guid: &str) -> Result<()> {
        self.sdk_cmd("SwitchProgram", &command::switch_program(guid)).await?;
        Ok(())
    }

    /// Switch to a program by index.
    pub async fn switch_program_index(&mut self, index: u32) -> Result<()> {
        self.sdk_cmd("SwitchProgramIndex", &command::switch_program_index(index)).await?;
        Ok(())
    }

    /// Get all programs on the device.
    pub async fn get_all_programs(&mut self) -> Result<Vec<ProgramInfo>> {
        let xml = self.sdk_cmd("GetAllProgram", &command::get_all_program()).await?;
        let mut programs = Vec::new();
        // Parse <program guid="..." name="..." type="..." current="..."/>
        let mut search = xml.as_str();
        while let Some(start) = search.find("<program ") {
            let end = search[start..].find("/>")
                .map(|e| start + e + 2)
                .or_else(|| search[start..].find("</program>").map(|e| start + e + 10))
                .unwrap_or(search.len());
            let tag = &search[start..end];
            programs.push(ProgramInfo {
                guid: xml::get_attr(tag, "guid").unwrap_or("").to_string(),
                name: xml::get_attr(tag, "name").map(xml::xml_unescape).unwrap_or_default(),
                program_type: xml::get_attr(tag, "type").map(xml::xml_unescape)
                    .unwrap_or_else(|| "normal".to_string()),
                is_current: xml::get_attr(tag, "current")
                    .map(|v| v == "true" || v == "1")
                    .unwrap_or(false),
            });
            search = &search[end.min(search.len())..];
        }
        Ok(programs)
    }

    // ── Brightness / Screen ──────────────────────────────────────────────

    /// Set display brightness (0–100).
    pub async fn set_brightness(&mut self, level: u8) -> Result<()> {
        self.sdk_cmd("SetBrightness", &command::set_brightness(level)).await?;
        Ok(())
    }

    /// Get brightness schedule.
    pub async fn get_luminance_ploy(&mut self) -> Result<String> {
        self.sdk_cmd("GetLuminancePloy", &command::get_luminance_ploy()).await
    }

    /// Set brightness schedule.
    pub async fn set_luminance_ploy(&mut self, entries: &[(u8, u8, u8)]) -> Result<()> {
        self.sdk_cmd("SetLuminancePloy", &command::set_luminance_ploy(entries)).await?;
        Ok(())
    }

    /// Turn the screen on.
    pub async fn screen_on(&mut self) -> Result<()> {
        self.sdk_cmd("OpenScreen", &command::open_screen()).await?;
        Ok(())
    }

    /// Turn the screen off.
    pub async fn screen_off(&mut self) -> Result<()> {
        self.sdk_cmd("CloseScreen", &command::close_screen()).await?;
        Ok(())
    }

    /// Get screen on/off schedule.
    pub async fn get_switch_time(&mut self) -> Result<String> {
        self.sdk_cmd("GetSwitchTime", &command::get_switch_time()).await
    }

    /// Set screen on/off schedule.
    ///
    /// Each entry is `(on_time, off_time, days)` where `days` is a 7-char '0'/'1' string
    /// for Mon–Sun (e.g. `"1111100"` = weekdays only). Pass an empty slice to clear.
    pub async fn set_switch_time(&mut self, entries: &[(&str, &str, &str)]) -> Result<()> {
        self.sdk_cmd("SetSwitchTime", &command::set_switch_time(entries)).await?;
        Ok(())
    }

    // ── Network ──────────────────────────────────────────────────────────

    pub async fn get_eth0_info(&mut self) -> Result<EthConfig> {
        let xml = self.sdk_cmd("GetEth0Info", &command::get_eth0_info()).await?;
        // Response: <eth0 dhcp="true" ip="..." mask="..." gateway="..." dns="..."/>
        let get = |a: &str| crate::xml::get_attr(&xml, a).map(str::to_string);
        Ok(EthConfig {
            dhcp:    get("dhcp").map(|v| v == "true").unwrap_or(false),
            ip:      get("ip").unwrap_or_default(),
            mask:    get("mask").unwrap_or_default(),
            gateway: get("gateway").unwrap_or_default(),
            dns:     get("dns").unwrap_or_default(),
        })
    }

    pub async fn set_eth0_info(&mut self, cfg: &EthConfig) -> Result<()> {
        let body = command::set_eth0_info(cfg.dhcp, &cfg.ip, &cfg.mask, &cfg.gateway, &cfg.dns);
        self.sdk_cmd("SetEth0Info", &body).await?;
        Ok(())
    }

    // ── Time ─────────────────────────────────────────────────────────────

    pub async fn get_time_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetTimeInfo", &command::get_time_info()).await
    }

    pub async fn sync_time(&mut self) -> Result<()> {
        self.sdk_cmd("SetTimeInfo", &command::sync_time_now()).await?;
        Ok(())
    }

    /// Set device timezone (UTC offset in whole hours, e.g. 8 for UTC+8).
    pub async fn set_timezone(&mut self, offset: i8) -> Result<()> {
        self.sdk_cmd("SetTimeInfo", &command::set_time_zone(offset)).await?;
        Ok(())
    }

    // ── FPGA Config ───────────────────────────────────────────────────────

    pub async fn get_fpga_config(&mut self) -> Result<String> {
        self.sdk_cmd("GetSDKFPGAConfig", &command::get_sdk_fpga_config()).await
    }

    pub async fn set_fpga_config(&mut self, config_xml: &str) -> Result<()> {
        self.sdk_cmd("SetSDKFPGAConfig", &command::set_sdk_fpga_config(config_xml)).await?;
        Ok(())
    }

    pub async fn get_box_hw_config(&mut self) -> Result<String> {
        self.sdk_cmd("GetBoxHwConfig", &command::get_box_hw_config()).await
    }

    // ── Device Control ────────────────────────────────────────────────────

    pub async fn reboot(&mut self) -> Result<()> {
        self.sdk_cmd("RebootDevice", &command::reboot_device()).await?;
        Ok(())
    }

    pub async fn set_device_name(&mut self, name: &str) -> Result<()> {
        self.sdk_cmd("UpdateDevName", &command::set_device_name(name)).await?;
        Ok(())
    }

    pub async fn set_rotation(&mut self, angle: u16) -> Result<()> {
        self.sdk_cmd("SetRotation", &command::set_rotation(angle)).await?;
        Ok(())
    }

    pub async fn set_volume(&mut self, level: u8) -> Result<()> {
        self.sdk_cmd("SetVolume", &command::set_volume(level)).await?;
        Ok(())
    }

    // ── Extra queries ─────────────────────────────────────────────────────

    /// Get current brightness level (0–100).
    pub async fn get_brightness(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetLuminancePloy", &command::get_luminance_ploy()).await?;
        let level = crate::xml::get_attr(&xml, "value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        Ok(level)
    }

    /// List all files stored on the device.
    pub async fn list_files(&mut self) -> Result<Vec<String>> {
        let xml = self.sdk_cmd("GetFiles", "").await?;
        let mut files = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<file ") {
            let end = search[start..]
                .find("/>")
                .map(|e| start + e + 2)
                .unwrap_or(search.len());
            let tag = &search[start..end];
            if let Some(name) = crate::xml::get_attr(tag, "name") {
                files.push(crate::xml::xml_unescape(name));
            }
            search = &search[end.min(search.len())..];
        }
        Ok(files)
    }

    /// Get file list with MD5 hashes and sizes.
    pub async fn get_file_checklist(&mut self) -> Result<Vec<FileEntry>> {
        let xml = self.sdk_cmd("GetFileChecklist", &command::get_file_checklist()).await?;
        let mut files = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<file ") {
            let end = search[start..]
                .find("/>")
                .map(|e| start + e + 2)
                .unwrap_or(search.len());
            let tag = &search[start..end];
            files.push(FileEntry {
                name: crate::xml::get_attr(tag, "name").map(crate::xml::xml_unescape).unwrap_or_default(),
                md5:  crate::xml::get_attr(tag, "md5").unwrap_or("").to_string(),
                size: crate::xml::get_attr(tag, "size")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0),
            });
            search = &search[end.min(search.len())..];
        }
        Ok(files)
    }

    /// Set multiple data source key/value pairs in a single command.
    pub async fn set_data_sources(&mut self, entries: &[(&str, &str)]) -> Result<()> {
        self.sdk_cmd("SetDataSourceInfo", &command::set_data_source_info(entries)).await?;
        Ok(())
    }

    /// Get the GUID of the currently playing program (empty string if none).
    pub async fn get_current_program_guid(&mut self) -> Result<String> {
        let xml = self
            .sdk_cmd("GetCurrentPlayProgramGUID", "")
            .await?;
        Ok(crate::xml::get_attr(&xml, "guid").unwrap_or("").to_string())
    }

    /// Get current screen rotation in degrees (0, 90, 180, 270).
    pub async fn get_rotation(&mut self) -> Result<u16> {
        let xml = self.sdk_cmd("GetScreenRotation", &command::get_screen_rotation()).await?;
        Ok(crate::xml::get_attr(&xml, "value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// Get the boot logo filename (empty = none set).
    pub async fn get_boot_logo(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetBootLogo", &command::get_boot_logo()).await?;
        Ok(crate::xml::get_attr(&xml, "name").map(crate::xml::xml_unescape).unwrap_or_default())
    }

    /// Set the boot logo by filename (must already be on the device).
    pub async fn set_boot_logo(&mut self, filename: &str) -> Result<()> {
        self.sdk_cmd("SetBootLogoName", &command::set_boot_logo_name(filename)).await?;
        Ok(())
    }

    /// Clear the boot logo.
    pub async fn clear_boot_logo(&mut self) -> Result<()> {
        self.sdk_cmd("ClearBootLogo", &command::clear_boot_logo()).await?;
        Ok(())
    }

    /// Delete named files from device storage.
    pub async fn delete_files(&mut self, filenames: &[&str]) -> Result<()> {
        self.sdk_cmd("DeleteFiles", &command::delete_files(filenames)).await?;
        Ok(())
    }

    /// Get network connectivity status (eth0, wifi, internet).
    pub async fn get_network_info(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetNetworkInfo", &command::get_network_info()).await?;
        Ok(xml)
    }

    /// Get WiFi status (SSID and connection state).
    pub async fn get_wifi_info(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetWifiInfo", &command::get_wifi_info()).await?;
        Ok(xml)
    }

    /// Connect to a WiFi network.
    pub async fn set_wifi(&mut self, ssid: &str, password: &str) -> Result<()> {
        self.sdk_cmd("SetWifiInfo", &command::set_wifi(ssid, password, false)).await?;
        Ok(())
    }

    /// Get PPPoE connection configuration.
    pub async fn get_pppoe_info(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetPppoeInfo", &command::get_pppoe_info()).await?;
        Ok(xml)
    }

    /// Set PPPoE connection configuration.
    pub async fn set_pppoe_info(&mut self, enable: bool, user: &str, password: &str) -> Result<()> {
        self.sdk_cmd("SetPppoeInfo", &command::set_pppoe_info(enable, user, password)).await?;
        Ok(())
    }

    /// Unlock admin mode using the device's configured password.
    /// Returns `true` if the password was accepted.
    pub async fn unlock_admin_password(&mut self, password: &str) -> Result<bool> {
        let xml = self.sdk_cmd(
            "UnlockAdminModePassword",
            &command::unlock_admin_password(password),
        ).await?;
        // Success: <adminMode enable="true"/>  Failure: <result value="1" .../>
        let accepted = xml.contains("adminMode") || !xml.contains("value=\"1\"");
        Ok(accepted)
    }

    /// Set or clear the admin unlock password.
    /// Pass an empty string to remove the password requirement.
    pub async fn set_admin_password(&mut self, password: &str) -> Result<()> {
        self.sdk_cmd("SetAdminModePassword", &command::set_admin_password(password)).await?;
        Ok(())
    }

    /// Trigger a SmartDrawLine color-bar test pattern on the display.
    pub async fn screen_test(&mut self) -> Result<()> {
        self.sdk_cmd("SmartDrawLine", &command::smart_draw_line()).await?;
        Ok(())
    }

    /// Get the firmware upgrade result.
    pub async fn get_upgrade_result(&mut self) -> Result<String> {
        let xml = self.sdk_cmd("GetUpgradeResult", &command::get_upgrade_result()).await?;
        Ok(crate::xml::get_attr(&xml, "message").map(crate::xml::xml_unescape).unwrap_or_default())
    }

    /// Trigger a firmware upgrade from a file already on the device.
    pub async fn firmware_upgrade(&mut self, filename: &str) -> Result<()> {
        self.sdk_cmd("FirmwareUpgrade", &command::firmware_upgrade(filename)).await?;
        Ok(())
    }

    /// Get all data source key/value pairs from the device.
    pub async fn get_data_sources(&mut self) -> Result<Vec<(String, String)>> {
        let xml = self.sdk_cmd("GetDataSourceInfo", &command::get_data_source_info()).await?;
        let mut entries = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<dataSource ") {
            let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
            let tag = &search[start..end];
            if let (Some(name), Some(value)) = (
                crate::xml::get_attr(tag, "name"),
                crate::xml::get_attr(tag, "value"),
            ) {
                entries.push((
                    crate::xml::xml_unescape(name),
                    crate::xml::xml_unescape(value),
                ));
            }
            search = &search[end.min(search.len())..];
        }
        Ok(entries)
    }

    /// Set a single data source value.
    pub async fn set_data_source(&mut self, name: &str, value: &str) -> Result<()> {
        self.sdk_cmd("SetDataSourceInfo", &command::set_data_source_info(&[(name, value)])).await?;
        Ok(())
    }

    /// Delete all orphaned program XML files from device storage.
    /// Media files and configuration are preserved.
    pub async fn cleanup(&mut self) -> Result<()> {
        self.sdk_cmd("DeleteNotCiteFile", &command::delete_not_cite_file()).await?;
        Ok(())
    }

    /// Get the device license string. Returns `(license, valid)`.
    pub async fn get_license(&mut self) -> Result<(String, bool)> {
        let xml = self.sdk_cmd("GetLicense", &command::get_license()).await?;
        let value = crate::xml::get_attr(&xml, "value").unwrap_or("").to_string();
        let valid = crate::xml::get_attr(&xml, "valid")
            .map(|v| v == "true")
            .unwrap_or(!value.is_empty());
        Ok((value, valid))
    }

    /// Set the device license string.
    pub async fn set_license(&mut self, value: &str) -> Result<()> {
        self.sdk_cmd("SetLicense", &command::set_license(value)).await?;
        Ok(())
    }

    // ── File Transfer ─────────────────────────────────────────────────────

    /// Transfer a file to the device using the binary file transfer protocol.
    pub async fn upload_file(
        &mut self,
        transfer: &FileTransfer,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<()> {
        info!("Uploading '{}' ({} bytes, md5={})",
            transfer.filename, transfer.total_size(), transfer.md5);

        // Send FileStartAsk
        self.send_packet(&transfer.start_packet()).await?;
        let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
        if ack.command != Command::FileStartAnswer {
            return Err(Error::Transfer(format!("Expected FileStartAnswer, got {:?}", ack.command)));
        }
        self.check_file_answer(&ack)?;

        // Send chunks
        let total = transfer.total_size();
        for (offset, chunk) in transfer.chunks() {
            let pkt = crate::transfer::build_content_packet(offset, chunk);
            self.send_packet(&pkt).await?;
            let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
            if ack.command != Command::FileContentAnswer {
                return Err(Error::Transfer(
                    format!("Expected FileContentAnswer, got {:?}", ack.command)
                ));
            }
            self.check_file_answer(&ack)?;
            if let Some(cb) = progress {
                cb(offset + chunk.len() as u64, total);
            }
        }

        // Send FileEndAsk
        self.send_packet(&transfer.end_packet()).await?;
        let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
        if ack.command != Command::FileEndAnswer {
            return Err(Error::Transfer(format!("Expected FileEndAnswer, got {:?}", ack.command)));
        }
        self.check_file_answer(&ack)?;

        info!("Upload complete: '{}'", transfer.filename);
        Ok(())
    }

    fn check_file_answer(&self, pkt: &Packet) -> Result<()> {
        if pkt.payload.len() < 4 {
            return Ok(()); // Too short to contain a result code — treat as OK
        }
        let code = i32::from_le_bytes(pkt.payload[..4].try_into().unwrap_or([0; 4]));
        if code == 0 {
            Ok(())
        } else {
            Err(Error::Device {
                code,
                message: crate::error::DeviceError::from_code(code).message().into(),
            })
        }
    }
}
