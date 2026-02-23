//! HDSign device client: dispatches to TCP (Ethernet) or HTTP (WiFi AP).
//!
//! Ethernet cards use the same binary SDK protocol as HDPlayer (TCP port 10001).
//! WiFi AP cards use HTTP on port 6104; most endpoints are stubs pending
//! wire capture.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::Arc;
use tracing::{debug, info, warn};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::protocol::{Command, Packet};
use crate::transfer::FileTransfer;
use crate::xml;

pub const DEFAULT_PORT: u16 = huidu_protocol::packet::DEFAULT_PORT;
const SDK_VERSION: u32 = huidu_protocol::packet::SDK_CLIENT_VERSION;
const CMD_TIMEOUT: Duration = Duration::from_secs(15);

// ── Shared data structures ────────────────────────────────────────────────────

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
    pub brightness: u8,
    pub volume: u8,
    pub rotation: u32,
    pub admin_mode: bool,
    pub current_program_guid: Option<String>,
    pub raw_xml: String,
}

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
#[derive(Debug, Clone, Default)]
pub struct EthConfig {
    pub dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
}

// ── TCP client (Ethernet cards) ───────────────────────────────────────────────

/// Low-level TCP client implementing the Huidu SDK binary protocol.
pub struct TcpClient {
    stream: Arc<Mutex<TcpStream>>,
    client_guid: String,
    read_buf: Vec<u8>,
}

impl TcpClient {
    pub async fn connect(host: &str, port: u16) -> Result<Self> {
        let addr = format!("{host}:{port}");
        info!("Connecting to {addr}");
        let stream = TcpStream::connect(&addr).await
            .map_err(|e| Error::Connection(format!("TCP connect to {addr}: {e}")))?;
        stream.set_nodelay(true)?;

        let mut client = Self {
            stream: Arc::new(Mutex::new(stream)),
            client_guid: Uuid::new_v4().to_string().to_uppercase(),
            read_buf: Vec::with_capacity(65536),
        };
        client.send_sdk_service().await?;
        Ok(client)
    }

    // ── Internal helpers ──────────────────────────────────────────────────

    async fn send_packet(&mut self, packet: &Packet) -> Result<()> {
        let bytes = packet.to_bytes();
        let mut stream = self.stream.lock().await;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        debug!("Sent {:?} ({} bytes)", packet.command, bytes.len());
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Packet> {
        loop {
            if let Some((packet, consumed)) = Packet::from_bytes(&self.read_buf)? {
                self.read_buf.drain(..consumed);
                debug!("Recv {:?}", packet.command);
                return Ok(packet);
            }
            let mut tmp = vec![0u8; 4096];
            let mut stream = self.stream.lock().await;
            let n = stream.read(&mut tmp).await?;
            if n == 0 {
                return Err(Error::Connection("connection closed".into()));
            }
            drop(stream);
            self.read_buf.extend_from_slice(&tmp[..n]);
        }
    }

    async fn recv_with_timeout(&mut self, timeout: Duration) -> Result<Packet> {
        tokio::time::timeout(timeout, self.recv_packet())
            .await
            .map_err(|_| Error::Timeout)?
    }

    async fn send_sdk_service(&mut self) -> Result<()> {
        use crate::protocol::sdk_service_ask_payload;
        let payload = sdk_service_ask_payload(SDK_VERSION);
        let pkt = Packet::new(Command::SdkServiceAsk, payload);
        self.send_packet(&pkt).await?;
        let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
        if resp.command != Command::SdkServiceAnswer {
            warn!("Expected SdkServiceAnswer, got {:?}", resp.command);
        }
        Ok(())
    }

    async fn sdk_cmd(&mut self, method: &str, body: &str) -> Result<String> {
        use crate::protocol::sdk_cmd_ask_payload;
        let xml_str = xml::sdk_request(&self.client_guid, method, body);
        let payload = sdk_cmd_ask_payload(&xml_str);
        let pkt = Packet::new(Command::SdkCmdAsk, payload);
        self.send_packet(&pkt).await?;

        loop {
            let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
            match resp.command {
                Command::SdkCmdAnswer => {
                    let xml_bytes = crate::protocol::parse_sdk_cmd_payload(&resp.payload)
                        .ok_or_else(|| Error::Protocol(
                            format!("SdkCmdAnswer too short: {} bytes", resp.payload.len())
                        ))?;
                    let response_xml = String::from_utf8_lossy(xml_bytes).into_owned();
                    xml::parse_result(&response_xml)?;
                    return Ok(response_xml);
                }
                Command::TcpHeartbeatAsk => {
                    let pong = Packet::heartbeat();
                    self.send_packet(&pong).await?;
                }
                other => {
                    warn!("Unexpected packet {:?} while waiting for SdkCmdAnswer", other);
                }
            }
        }
    }

    // ── Public methods ────────────────────────────────────────────────────

    pub async fn heartbeat(&mut self) -> Result<()> {
        self.send_packet(&Packet::heartbeat()).await
    }

    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        let xml = self.sdk_cmd("GetDeviceInfo", "").await?;
        let mut info = DeviceDetails { raw_xml: xml.clone(), ..Default::default() };
        let scope = xml.find("<deviceInfo")
            .and_then(|s| xml[s..].find('>').map(|e| &xml[s..s + e + 1]))
            .unwrap_or(xml.as_str());
        let get = |a: &str| xml::get_attr(scope, a).map(xml::xml_unescape);
        info.device_id    = get("deviceId").or_else(|| get("deviceID")).unwrap_or_default();
        info.device_name  = get("deviceName").or_else(|| get("name")).unwrap_or_default();
        info.firmware_version = get("version").or_else(|| get("SoftwareVersion")).unwrap_or_default();
        info.screen_width  = get("screenWidth").or_else(|| get("width"))
            .and_then(|v| v.parse().ok()).unwrap_or(0);
        info.screen_height = get("screenHeight").or_else(|| get("height"))
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

    pub async fn get_all_programs(&mut self) -> Result<Vec<ProgramInfo>> {
        let xml = self.sdk_cmd("GetAllProgram", "").await?;
        let mut programs = Vec::new();
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
                    .map(|v| v == "true" || v == "1").unwrap_or(false),
            });
            search = &search[end.min(search.len())..];
        }
        Ok(programs)
    }

    pub async fn add_program(&mut self, screen_xml: &str) -> Result<()> {
        self.sdk_cmd("AddProgram", screen_xml).await?;
        Ok(())
    }

    pub async fn update_program(&mut self, screen_xml: &str) -> Result<()> {
        self.sdk_cmd("UpdateProgram", screen_xml).await?;
        Ok(())
    }

    pub async fn delete_program(&mut self, guid: &str) -> Result<()> {
        self.sdk_cmd("DeleteProgram", &format!("<program guid=\"{guid}\"/>")).await?;
        Ok(())
    }

    pub async fn switch_program(&mut self, guid: &str) -> Result<()> {
        self.sdk_cmd("SwitchProgram", &format!("<program guid=\"{guid}\"/>")).await?;
        Ok(())
    }

    pub async fn set_brightness(&mut self, level: u8) -> Result<()> {
        self.sdk_cmd("SetBrightness", &format!("<brightness value=\"{}\"/>", level.min(100))).await?;
        Ok(())
    }

    /// Set brightness schedule.  Each entry is `(hour, minute, level)`.
    pub async fn set_luminance_ploy(&mut self, entries: &[(u8, u8, u8)]) -> Result<()> {
        let mut body = String::from("<luminancePloy>");
        for (h, m, lv) in entries {
            body.push_str(&format!("<item hour=\"{}\" minute=\"{}\" luminance=\"{}\"/>", h, m, lv));
        }
        body.push_str("</luminancePloy>");
        self.sdk_cmd("SetLuminancePloy", &body).await?;
        Ok(())
    }

    /// Set screen on/off schedule.
    /// Each entry is `(on_time, off_time, days)` where `days` is a 7-char '0'/'1' string
    /// for Mon–Sun (e.g. `"1111100"` = weekdays only). Pass an empty slice to clear.
    pub async fn set_switch_time(&mut self, entries: &[(&str, &str, &str)]) -> Result<()> {
        let mut body = String::from("<switchTime>");
        for (on, off, days) in entries {
            body.push_str(&format!("<item on=\"{}\" off=\"{}\" week=\"{}\"/>", on, off, days));
        }
        body.push_str("</switchTime>");
        self.sdk_cmd("SetSwitchTime", &body).await?;
        Ok(())
    }

    pub async fn screen_on(&mut self) -> Result<()> {
        self.sdk_cmd("OpenScreen", "").await?;
        Ok(())
    }

    pub async fn screen_off(&mut self) -> Result<()> {
        self.sdk_cmd("CloseScreen", "").await?;
        Ok(())
    }

    pub async fn reboot(&mut self) -> Result<()> {
        self.sdk_cmd("RebootDevice", "").await?;
        Ok(())
    }

    pub async fn screen_test(&mut self) -> Result<()> {
        self.sdk_cmd("SmartDrawLine", "").await?;
        Ok(())
    }

    pub async fn set_volume(&mut self, level: u8) -> Result<()> {
        self.sdk_cmd("SetVolume", &format!("<volume value=\"{}\"/>", level.min(100))).await?;
        Ok(())
    }

    pub async fn set_rotation(&mut self, angle: u16) -> Result<()> {
        self.sdk_cmd("SetRotation", &format!("<rotation angle=\"{}\"/>", angle)).await?;
        Ok(())
    }

    pub async fn sync_time(&mut self) -> Result<()> {
        let now = chrono::Local::now();
        let ts = now.format("%Y-%m-%d %H:%M:%S").to_string();
        self.sdk_cmd("SyncTime", &format!("<time value=\"{}\"/>", ts)).await?;
        Ok(())
    }

    pub async fn get_luminance_ploy(&mut self) -> Result<String> {
        self.sdk_cmd("GetLuminancePloy", "").await
    }

    pub async fn get_switch_time(&mut self) -> Result<String> {
        self.sdk_cmd("GetSwitchTime", "").await
    }

    pub async fn get_eth0_info(&mut self) -> Result<EthConfig> {
        let xml_resp = self.sdk_cmd("GetEth0Info", "").await?;
        let get = |a: &str| xml::get_attr(&xml_resp, a).map(str::to_string);
        Ok(EthConfig {
            dhcp:    get("dhcp").map(|v| v == "true" || v == "1").unwrap_or(false),
            ip:      get("ip").unwrap_or_default(),
            mask:    get("mask").unwrap_or_default(),
            gateway: get("gateway").unwrap_or_default(),
            dns:     get("dns").unwrap_or_default(),
        })
    }

    pub async fn set_eth0_info(&mut self, cfg: &EthConfig) -> Result<()> {
        let body = format!(
            "<eth0 dhcp=\"{}\" ip=\"{}\" mask=\"{}\" gateway=\"{}\" dns=\"{}\"/>",
            if cfg.dhcp { "true" } else { "false" },
            xml::xml_escape(&cfg.ip),
            xml::xml_escape(&cfg.mask),
            xml::xml_escape(&cfg.gateway),
            xml::xml_escape(&cfg.dns),
        );
        self.sdk_cmd("SetEth0Info", &body).await?;
        Ok(())
    }

    pub async fn set_timezone(&mut self, offset: i8) -> Result<()> {
        self.sdk_cmd("SetTimeInfo", &format!("<timeInfo timezone=\"{offset}\"/>")).await?;
        Ok(())
    }

    pub async fn set_device_name(&mut self, name: &str) -> Result<()> {
        self.sdk_cmd("UpdateDevName",
            &format!("<device name=\"{}\"/>", xml::xml_escape(name))).await?;
        Ok(())
    }

    pub async fn get_rotation(&mut self) -> Result<u16> {
        let xml_resp = self.sdk_cmd("GetScreenRotation", "").await?;
        Ok(xml::get_attr(&xml_resp, "value").and_then(|v| v.parse().ok()).unwrap_or(0))
    }

    pub async fn get_boot_logo(&mut self) -> Result<String> {
        let xml_resp = self.sdk_cmd("GetBootLogo", "").await?;
        Ok(xml::get_attr(&xml_resp, "name").map(xml::xml_unescape).unwrap_or_default())
    }

    pub async fn set_boot_logo(&mut self, filename: &str) -> Result<()> {
        self.sdk_cmd("SetBootLogoName",
            &format!("<bootLogo name=\"{}\"/>", xml::xml_escape(filename))).await?;
        Ok(())
    }

    pub async fn clear_boot_logo(&mut self) -> Result<()> {
        self.sdk_cmd("ClearBootLogo", "").await?;
        Ok(())
    }

    pub async fn list_files(&mut self) -> Result<Vec<String>> {
        let xml_resp = self.sdk_cmd("GetFiles", "").await?;
        let mut files = Vec::new();
        let mut search = xml_resp.as_str();
        while let Some(start) = search.find("<file ") {
            let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
            let tag = &search[start..end];
            if let Some(name) = xml::get_attr(tag, "name") {
                files.push(xml::xml_unescape(name));
            }
            search = &search[end.min(search.len())..];
        }
        Ok(files)
    }

    pub async fn get_file_checklist(&mut self) -> Result<Vec<FileEntry>> {
        let xml_resp = self.sdk_cmd("GetFileChecklist", "").await?;
        let mut files = Vec::new();
        let mut search = xml_resp.as_str();
        while let Some(start) = search.find("<file ") {
            let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
            let tag = &search[start..end];
            files.push(FileEntry {
                name: xml::get_attr(tag, "name").map(xml::xml_unescape).unwrap_or_default(),
                md5:  xml::get_attr(tag, "md5").unwrap_or("").to_string(),
                size: xml::get_attr(tag, "size").and_then(|v| v.parse().ok()).unwrap_or(0),
            });
            search = &search[end.min(search.len())..];
        }
        Ok(files)
    }

    pub async fn delete_files(&mut self, filenames: &[&str]) -> Result<()> {
        let mut body = String::from("<files>");
        for name in filenames {
            body.push_str(&format!("<file name=\"{}\"/>", xml::xml_escape(name)));
        }
        body.push_str("</files>");
        self.sdk_cmd("DeleteFiles", &body).await?;
        Ok(())
    }

    pub async fn get_network_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetNetworkInfo", "").await
    }

    pub async fn get_wifi_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetWifiInfo", "").await
    }

    pub async fn set_wifi(&mut self, ssid: &str, password: &str) -> Result<()> {
        self.sdk_cmd("SetWifiInfo",
            &format!("<wifi ssid=\"{}\" password=\"{}\"/>",
                xml::xml_escape(ssid), xml::xml_escape(password))).await?;
        Ok(())
    }

    pub async fn get_current_program_guid(&mut self) -> Result<String> {
        let xml_resp = self.sdk_cmd("GetCurrentPlayProgramGUID", "").await?;
        Ok(xml::get_attr(&xml_resp, "guid").unwrap_or("").to_string())
    }

    pub async fn get_data_sources(&mut self) -> Result<Vec<(String, String)>> {
        let xml_resp = self.sdk_cmd("GetDataSourceInfo", "").await?;
        let mut entries = Vec::new();
        let mut search = xml_resp.as_str();
        while let Some(start) = search.find("<dataSource ") {
            let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
            let tag = &search[start..end];
            if let (Some(name), Some(value)) = (xml::get_attr(tag, "name"), xml::get_attr(tag, "value")) {
                entries.push((xml::xml_unescape(name), xml::xml_unescape(value)));
            }
            search = &search[end.min(search.len())..];
        }
        Ok(entries)
    }

    pub async fn set_data_sources(&mut self, entries: &[(&str, &str)]) -> Result<()> {
        let mut body = String::from("<dataSources>");
        for (name, value) in entries {
            body.push_str(&format!("<dataSource name=\"{}\" value=\"{}\"/>",
                xml::xml_escape(name), xml::xml_escape(value)));
        }
        body.push_str("</dataSources>");
        self.sdk_cmd("SetDataSourceInfo", &body).await?;
        Ok(())
    }

    pub async fn unlock_admin_password(&mut self, password: &str) -> Result<bool> {
        let xml_resp = self.sdk_cmd("UnlockAdminModePassword",
            &format!("<adminMode password=\"{}\"/>", xml::xml_escape(password))).await?;
        let accepted = xml_resp.contains("adminMode") || !xml_resp.contains("value=\"1\"");
        Ok(accepted)
    }

    pub async fn set_admin_password(&mut self, password: &str) -> Result<()> {
        self.sdk_cmd("SetAdminModePassword",
            &format!("<adminMode password=\"{}\"/>", xml::xml_escape(password))).await?;
        Ok(())
    }

    pub async fn firmware_upgrade(&mut self, filename: &str) -> Result<()> {
        self.sdk_cmd("FirmwareUpgrade",
            &format!("<upgrade file=\"{}\"/>", xml::xml_escape(filename))).await?;
        Ok(())
    }

    pub async fn upload_file(
        &mut self,
        transfer: &FileTransfer,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<()> {
        info!("Uploading '{}' ({} bytes)", transfer.filename, transfer.total_size());

        self.send_packet(&transfer.start_packet()).await?;
        let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
        if ack.command != Command::FileStartAnswer {
            return Err(Error::Transfer(format!("Expected FileStartAnswer, got {:?}", ack.command)));
        }
        self.check_file_answer(&ack)?;

        let total = transfer.total_size();
        for (offset, chunk) in transfer.chunks() {
            let pkt = crate::transfer::build_content_packet(offset, chunk);
            self.send_packet(&pkt).await?;
            let ack = self.recv_with_timeout(CMD_TIMEOUT).await?;
            if ack.command != Command::FileContentAnswer {
                return Err(Error::Transfer(format!("Expected FileContentAnswer, got {:?}", ack.command)));
            }
            self.check_file_answer(&ack)?;
            if let Some(cb) = progress { cb(offset + chunk.len() as u64, total); }
        }

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
        if pkt.payload.len() < 4 { return Ok(()); }
        let code = i32::from_le_bytes(pkt.payload[..4].try_into().unwrap_or([0; 4]));
        if code == 0 { Ok(()) } else {
            Err(Error::Device {
                code,
                message: crate::error::DeviceError::from_code(code).message().into(),
            })
        }
    }
}

// ── HTTP client (WiFi AP cards) ───────────────────────────────────────────────

/// HTTP-based client for cards in WiFi AP mode (e.g. HD-Wxxx-75).
///
/// The device runs an HTTP server on port 6104 at 192.168.4.1 when in AP mode.
/// Most endpoints are stubs — only WiFi provisioning (`POST /station`) is
/// confirmed from binary analysis.
pub struct HttpClient {
    pub base_url: String,
    http: reqwest::Client,
}

impl HttpClient {
    pub fn new(device_ip: &str) -> Self {
        Self {
            base_url: format!("http://{device_ip}:6104"),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
        }
    }

    /// Configure the card to connect to a WiFi network (POST /station).
    /// This is the only confirmed HTTP endpoint from binary analysis.
    pub async fn set_wifi_credentials(
        &self,
        ssid: &str,
        password: &str,
        netmask: &str,
        gateway: &str,
    ) -> Result<()> {
        let body = serde_json::json!({
            "ssid":     ssid,
            "password": password,
            "netmask":  netmask,
            "gateway":  gateway,
        });
        self.http
            .post(format!("{}/station", self.base_url))
            .json(&body)
            .send()
            .await?;
        Ok(())
    }

    /// Set brightness.  Endpoint/format unknown — stub.
    pub async fn set_brightness(&self, _level: u8) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Set brightness schedule.  Endpoint/format unknown — stub.
    pub async fn set_luminance_ploy(&self, _entries: &[(u8, u8, u8)]) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Set screen on/off schedule.  Endpoint/format unknown — stub.
    pub async fn set_switch_time(&self, _entries: &[(&str, &str, &str)]) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Set volume.  Endpoint/format unknown — stub.
    pub async fn set_volume(&self, _level: u8) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Set rotation.  Endpoint/format unknown — stub.
    pub async fn set_rotation(&self, _angle: u16) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Sync device time.  Endpoint/format unknown — stub.
    pub async fn sync_time(&self) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Turn screen on.  Endpoint/format unknown — stub.
    pub async fn screen_on(&self) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Turn screen off.  Endpoint/format unknown — stub.
    pub async fn screen_off(&self) -> Result<()> {
        // TODO: confirm via wire capture
        Ok(())
    }

    /// Upload a program to the device.  Endpoint/format unknown — stub.
    pub async fn upload_program(&self, _xml: &str) -> Result<()> {
        // TODO: confirm via wire capture
        Err(Error::Protocol("WiFi AP program upload not yet implemented (pending wire capture)".into()))
    }

    pub async fn get_luminance_ploy(&self) -> Result<String> { Ok(String::new()) }
    pub async fn get_switch_time(&self) -> Result<String> { Ok(String::new()) }
    pub async fn get_eth0_info(&self) -> Result<EthConfig> { Ok(EthConfig::default()) }
    pub async fn set_eth0_info(&self, _cfg: &EthConfig) -> Result<()> { Ok(()) }
    pub async fn set_timezone(&self, _offset: i8) -> Result<()> { Ok(()) }
    pub async fn set_device_name(&self, _name: &str) -> Result<()> { Ok(()) }
    pub async fn get_rotation(&self) -> Result<u16> { Ok(0) }
    pub async fn get_boot_logo(&self) -> Result<String> { Ok(String::new()) }
    pub async fn set_boot_logo(&self, _filename: &str) -> Result<()> { Ok(()) }
    pub async fn clear_boot_logo(&self) -> Result<()> { Ok(()) }
    pub async fn list_files(&self) -> Result<Vec<String>> { Ok(Vec::new()) }
    pub async fn get_file_checklist(&self) -> Result<Vec<FileEntry>> { Ok(Vec::new()) }
    pub async fn delete_files(&self, _filenames: &[&str]) -> Result<()> { Ok(()) }
    pub async fn get_network_info(&self) -> Result<String> { Ok(String::new()) }
    pub async fn get_wifi_info(&self) -> Result<String> { Ok(String::new()) }
    pub async fn set_wifi(&self, _ssid: &str, _password: &str) -> Result<()> { Ok(()) }
    pub async fn get_current_program_guid(&self) -> Result<String> { Ok(String::new()) }
    pub async fn get_data_sources(&self) -> Result<Vec<(String, String)>> { Ok(Vec::new()) }
    pub async fn set_data_sources(&self, _entries: &[(&str, &str)]) -> Result<()> { Ok(()) }
    pub async fn unlock_admin_password(&self, _password: &str) -> Result<bool> { Ok(false) }
    pub async fn set_admin_password(&self, _password: &str) -> Result<()> { Ok(()) }
}

// ── Unified client ────────────────────────────────────────────────────────────

/// Unified HDSign device client.  Dispatches to TCP (Ethernet) or HTTP (WiFi AP).
pub enum HdsignClient {
    Tcp(TcpClient),
    Http(HttpClient),
}

impl HdsignClient {
    pub async fn connect_ethernet(host: &str, port: u16) -> Result<Self> {
        Ok(Self::Tcp(TcpClient::connect(host, port).await?))
    }

    pub fn connect_wifi_ap(device_ip: &str) -> Self {
        Self::Http(HttpClient::new(device_ip))
    }

    pub async fn heartbeat(&mut self) -> Result<()> {
        match self {
            Self::Tcp(c) => c.heartbeat().await,
            Self::Http(_) => Ok(()),
        }
    }

    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        match self {
            Self::Tcp(c) => c.get_device_info().await,
            Self::Http(_) => Ok(DeviceDetails::default()),
        }
    }

    pub async fn get_all_programs(&mut self) -> Result<Vec<ProgramInfo>> {
        match self {
            Self::Tcp(c) => c.get_all_programs().await,
            Self::Http(_) => Ok(Vec::new()),
        }
    }

    pub async fn add_program(&mut self, xml: &str) -> Result<()> {
        match self {
            Self::Tcp(c) => c.add_program(xml).await,
            Self::Http(h) => h.upload_program(xml).await,
        }
    }

    pub async fn update_program(&mut self, xml: &str) -> Result<()> {
        match self {
            Self::Tcp(c) => c.update_program(xml).await,
            Self::Http(h) => h.upload_program(xml).await,
        }
    }

    pub async fn delete_program(&mut self, guid: &str) -> Result<()> {
        match self {
            Self::Tcp(c) => c.delete_program(guid).await,
            Self::Http(_) => Ok(()),
        }
    }

    pub async fn switch_program(&mut self, guid: &str) -> Result<()> {
        match self {
            Self::Tcp(c) => c.switch_program(guid).await,
            Self::Http(_) => Ok(()),
        }
    }

    pub async fn set_brightness(&mut self, level: u8) -> Result<()> {
        match self {
            Self::Tcp(c) => c.set_brightness(level).await,
            Self::Http(h) => h.set_brightness(level).await,
        }
    }

    pub async fn screen_on(&mut self) -> Result<()> {
        match self {
            Self::Tcp(c) => c.screen_on().await,
            Self::Http(h) => h.screen_on().await,
        }
    }

    pub async fn screen_off(&mut self) -> Result<()> {
        match self {
            Self::Tcp(c) => c.screen_off().await,
            Self::Http(h) => h.screen_off().await,
        }
    }

    pub async fn reboot(&mut self) -> Result<()> {
        match self {
            Self::Tcp(c) => c.reboot().await,
            Self::Http(_) => Ok(()),
        }
    }

    pub async fn screen_test(&mut self) -> Result<()> {
        match self {
            Self::Tcp(c) => c.screen_test().await,
            Self::Http(_) => Err(Error::Protocol("Screen test not supported in WiFi AP mode".into())),
        }
    }

    pub async fn firmware_upgrade(&mut self, filename: &str) -> Result<()> {
        match self {
            Self::Tcp(c) => c.firmware_upgrade(filename).await,
            Self::Http(_) => Err(Error::Protocol("Firmware upgrade via HTTP not yet implemented".into())),
        }
    }

    pub async fn set_luminance_ploy(&mut self, entries: &[(u8, u8, u8)]) -> Result<()> {
        match self {
            Self::Tcp(c) => c.set_luminance_ploy(entries).await,
            Self::Http(h) => h.set_luminance_ploy(entries).await,
        }
    }

    pub async fn set_switch_time(&mut self, entries: &[(&str, &str, &str)]) -> Result<()> {
        match self {
            Self::Tcp(c) => c.set_switch_time(entries).await,
            Self::Http(h) => h.set_switch_time(entries).await,
        }
    }

    pub async fn set_volume(&mut self, level: u8) -> Result<()> {
        match self {
            Self::Tcp(c) => c.set_volume(level).await,
            Self::Http(h) => h.set_volume(level).await,
        }
    }

    pub async fn set_rotation(&mut self, angle: u16) -> Result<()> {
        match self {
            Self::Tcp(c) => c.set_rotation(angle).await,
            Self::Http(h) => h.set_rotation(angle).await,
        }
    }

    pub async fn sync_time(&mut self) -> Result<()> {
        match self {
            Self::Tcp(c) => c.sync_time().await,
            Self::Http(h) => h.sync_time().await,
        }
    }

    pub async fn set_wifi_credentials(
        &self,
        ssid: &str,
        password: &str,
    ) -> Result<()> {
        match self {
            Self::Tcp(_) => Err(Error::Protocol("WiFi provisioning requires WiFi AP connection".into())),
            Self::Http(h) => h.set_wifi_credentials(ssid, password, "255.255.255.0", "192.168.4.1").await,
        }
    }

    pub async fn get_luminance_ploy(&mut self) -> Result<String> {
        match self { Self::Tcp(c) => c.get_luminance_ploy().await, Self::Http(h) => h.get_luminance_ploy().await }
    }

    pub async fn get_switch_time(&mut self) -> Result<String> {
        match self { Self::Tcp(c) => c.get_switch_time().await, Self::Http(h) => h.get_switch_time().await }
    }

    pub async fn get_eth0_info(&mut self) -> Result<EthConfig> {
        match self { Self::Tcp(c) => c.get_eth0_info().await, Self::Http(h) => h.get_eth0_info().await }
    }

    pub async fn set_eth0_info(&mut self, cfg: &EthConfig) -> Result<()> {
        match self { Self::Tcp(c) => c.set_eth0_info(cfg).await, Self::Http(h) => h.set_eth0_info(cfg).await }
    }

    pub async fn set_timezone(&mut self, offset: i8) -> Result<()> {
        match self { Self::Tcp(c) => c.set_timezone(offset).await, Self::Http(h) => h.set_timezone(offset).await }
    }

    pub async fn set_device_name(&mut self, name: &str) -> Result<()> {
        match self { Self::Tcp(c) => c.set_device_name(name).await, Self::Http(h) => h.set_device_name(name).await }
    }

    pub async fn get_rotation(&mut self) -> Result<u16> {
        match self { Self::Tcp(c) => c.get_rotation().await, Self::Http(h) => h.get_rotation().await }
    }

    pub async fn get_boot_logo(&mut self) -> Result<String> {
        match self { Self::Tcp(c) => c.get_boot_logo().await, Self::Http(h) => h.get_boot_logo().await }
    }

    pub async fn set_boot_logo(&mut self, filename: &str) -> Result<()> {
        match self { Self::Tcp(c) => c.set_boot_logo(filename).await, Self::Http(h) => h.set_boot_logo(filename).await }
    }

    pub async fn clear_boot_logo(&mut self) -> Result<()> {
        match self { Self::Tcp(c) => c.clear_boot_logo().await, Self::Http(h) => h.clear_boot_logo().await }
    }

    pub async fn list_files(&mut self) -> Result<Vec<String>> {
        match self { Self::Tcp(c) => c.list_files().await, Self::Http(h) => h.list_files().await }
    }

    pub async fn get_file_checklist(&mut self) -> Result<Vec<FileEntry>> {
        match self { Self::Tcp(c) => c.get_file_checklist().await, Self::Http(h) => h.get_file_checklist().await }
    }

    pub async fn delete_files(&mut self, filenames: &[&str]) -> Result<()> {
        match self { Self::Tcp(c) => c.delete_files(filenames).await, Self::Http(h) => h.delete_files(filenames).await }
    }

    pub async fn get_network_info(&mut self) -> Result<String> {
        match self { Self::Tcp(c) => c.get_network_info().await, Self::Http(h) => h.get_network_info().await }
    }

    pub async fn get_wifi_info(&mut self) -> Result<String> {
        match self { Self::Tcp(c) => c.get_wifi_info().await, Self::Http(h) => h.get_wifi_info().await }
    }

    pub async fn set_wifi(&mut self, ssid: &str, password: &str) -> Result<()> {
        match self { Self::Tcp(c) => c.set_wifi(ssid, password).await, Self::Http(h) => h.set_wifi(ssid, password).await }
    }

    pub async fn get_current_program_guid(&mut self) -> Result<String> {
        match self { Self::Tcp(c) => c.get_current_program_guid().await, Self::Http(h) => h.get_current_program_guid().await }
    }

    pub async fn get_data_sources(&mut self) -> Result<Vec<(String, String)>> {
        match self { Self::Tcp(c) => c.get_data_sources().await, Self::Http(h) => h.get_data_sources().await }
    }

    pub async fn set_data_sources(&mut self, entries: &[(&str, &str)]) -> Result<()> {
        match self { Self::Tcp(c) => c.set_data_sources(entries).await, Self::Http(h) => h.set_data_sources(entries).await }
    }

    pub async fn unlock_admin_password(&mut self, password: &str) -> Result<bool> {
        match self { Self::Tcp(c) => c.unlock_admin_password(password).await, Self::Http(h) => h.unlock_admin_password(password).await }
    }

    pub async fn set_admin_password(&mut self, password: &str) -> Result<()> {
        match self { Self::Tcp(c) => c.set_admin_password(password).await, Self::Http(h) => h.set_admin_password(password).await }
    }

    pub async fn upload_file(
        &mut self,
        transfer: &FileTransfer,
        progress: Option<&dyn Fn(u64, u64)>,
    ) -> Result<()> {
        match self {
            Self::Tcp(c) => c.upload_file(transfer, progress).await,
            Self::Http(_) => Err(Error::Protocol("File upload via HTTP not yet implemented".into())),
        }
    }
}
