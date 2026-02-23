//! TCP client for communicating with Huidu BoxPlayer devices.
//!
//! Implements the full SDK XML command protocol plus file transfer.

use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::Arc;
use tracing::{debug, info, warn};


use crate::command;
use crate::error::{Error, Result};
use crate::protocol::{Command, Packet};
use crate::transfer::FileTransfer;
use crate::xml;

/// Default TCP port for Huidu BoxPlayer — the BoxStream protocol on port 9527.
/// Confirmed from Huidu.pcapng: HDPlayer.exe connects to port 9527, not 10001.
pub const DEFAULT_PORT: u16 = huidu_protocol::packet::BOX_PLAYER_PORT;

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
            // BoxStream protocol uses the literal "##GUID" placeholder (not a real UUID).
            // Confirmed from Huidu.pcapng: all SDK requests and responses use guid="##GUID".
            client_guid: "##GUID".to_string(),
            read_buf: Vec::with_capacity(65536),
        };

        // The device sends TcpHeartbeatAnswer (0x0060) as its FIRST packet on TCP connect.
        // We must respond with TcpHeartbeatAsk (0x005F) before the device will accept
        // BoxStreamInit commands.
        //
        // Observed timing: the device sends TcpHeartbeatAnswer ~5 seconds after TCP connect
        // (both on fresh connections and after UDP discovery). We wait up to 12 seconds.
        //
        // After responding to the heartbeat we loop — the device may send additional
        // TcpHeartbeatAnswer packets before it transitions to the BoxStream-ready state.
        // We stop looping once we've responded to the first heartbeat and the 300 ms inter-
        // packet gap fires, signaling the device is not sending more heartbeats immediately.
        info!("Waiting for device TcpHeartbeatAnswer (up to 12s)...");
        let mut got_heartbeat = false;
        loop {
            match client.recv_with_timeout(Duration::from_secs(12)).await {
                Ok(pkt) if pkt.command == Command::TcpHeartbeatAnswer => {
                    info!("Received TcpHeartbeatAnswer from device");
                    client.send_packet(&Packet::new(Command::TcpHeartbeatAsk, vec![])).await?;
                    info!("Responded with TcpHeartbeatAsk");
                    got_heartbeat = true;
                    // After responding, wait briefly (300 ms) to see if device sends another
                    // heartbeat immediately. If not (timeout), break and proceed.
                    match client.recv_with_timeout(Duration::from_millis(300)).await {
                        Ok(next) if next.command == Command::TcpHeartbeatAnswer => {
                            // Device sent another heartbeat right away — push back via temp store.
                            // We can't push back onto TcpStream, so serialize the packet bytes
                            // and prepend them to read_buf.
                            let bytes = next.to_bytes();
                            client.read_buf.splice(0..0, bytes);
                            // Loop again to handle it.
                        }
                        Ok(other) => {
                            // Something else arrived — prepend to read_buf and break.
                            let bytes = other.to_bytes();
                            client.read_buf.splice(0..0, bytes);
                            break;
                        }
                        Err(Error::Timeout) => break, // quiet gap → device is done heartbeating
                        Err(e) => return Err(e),
                    }
                }
                Ok(pkt) => {
                    warn!("Expected TcpHeartbeatAnswer, got {:?} — buffering for later", pkt.command);
                    let bytes = pkt.to_bytes();
                    client.read_buf.splice(0..0, bytes);
                    break;
                }
                Err(Error::Timeout) => {
                    if got_heartbeat {
                        break; // already got at least one heartbeat, proceed
                    }
                    warn!("No TcpHeartbeatAnswer received within 12s — proceeding anyway");
                    break;
                }
                Err(e) => return Err(e),
            }
        }

        // Note: box_stream_init() is NOT called here.
        // send_xml_request() sends BoxStreamInit at the start of every XML exchange.
        Ok(client)
    }

    /// Connect using the default port (9527).
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

    /// Initiate a BoxStream session (port 9527 protocol).
    ///
    /// Sends `BoxStreamInit` with payload `[0,0,0,0]` and waits for `BoxStreamInitAck`.
    /// Confirmed from Huidu.pcapng frames 8–10.
    async fn box_stream_init(&mut self) -> Result<()> {
        let pkt = Packet::new(Command::BoxStreamInit, vec![0x00, 0x00, 0x00, 0x00]);
        self.send_packet(&pkt).await?;
        let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
        match resp.command {
            Command::BoxStreamInitAck => {
                info!("BoxStream session established");
                Ok(())
            }
            other => Err(Error::Protocol(format!(
                "Expected BoxStreamInitAck, got {other:?}"
            ))),
        }
    }

    /// Wait for a specific command, discarding heartbeat pings in between.
    async fn expect_cmd(&mut self, expected: Command, timeout: Duration) -> Result<Packet> {
        loop {
            let pkt = self.recv_with_timeout(timeout).await?;
            if pkt.command == expected {
                return Ok(pkt);
            }
            match pkt.command {
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                other => {
                    warn!("Expected {:?}, got {:?} — ignoring", expected, other);
                }
            }
        }
    }

    /// Send XML over BoxStream and receive the response XML.
    ///
    /// Full 10-step handshake confirmed from Huidu.pcapng:
    ///
    /// **Request delivery:**
    /// 1. PC→Dev: BoxStreamData `[0x00,0x00] + xml`
    /// 2. Dev→PC: BoxStreamRxAck `[0x00,0x00]`
    /// 3. PC→Dev: BoxStreamTxAck `[0x00,0x00]`
    /// 4. Dev→PC: BoxStreamFinalAck `[0x00,0x00]`
    ///
    /// **Response delivery:**
    /// 5. Dev→PC: BoxStreamInit `[0x01,0x00,0x00,0x00]`  ← "response ready"
    /// 6. PC→Dev: BoxStreamInitAck `[0x00,0x00]`
    /// 7. Dev→PC: BoxStreamData `[0x01,0x00] + xml`      ← actual response
    /// 8. PC→Dev: BoxStreamRxAck `[0x00,0x00]`
    /// 9. Dev→PC: BoxStreamTxAck `[0x01,0x00]`
    /// 10. PC→Dev: BoxStreamFinalAck `[0x00,0x00]`
    async fn send_xml_request(&mut self, xml_str: &str) -> Result<String> {
        use crate::protocol::{box_stream_data_request, parse_box_stream_data};

        // Step 0 (pre-request): BoxStreamInit → BoxStreamInitAck handshake.
        // Required before EVERY XML exchange (confirmed from More Huidu.pcapng:
        // every subsequent call starts with BoxStreamInit [0,0,0,0] → BoxStreamInitAck).
        self.box_stream_init().await?;

        // Step 1: send BoxStreamData with [0x00,0x00] + xml
        let payload = box_stream_data_request(xml_str);
        self.send_packet(&Packet::new(Command::BoxStreamData, payload)).await?;

        // Step 2: wait for BoxStreamRxAck
        self.expect_cmd(Command::BoxStreamRxAck, CMD_TIMEOUT).await?;

        // Step 3: send BoxStreamTxAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamTxAck, vec![0x00, 0x00])).await?;

        // Step 4: wait for BoxStreamFinalAck
        self.expect_cmd(Command::BoxStreamFinalAck, CMD_TIMEOUT).await?;

        // Step 5: wait for BoxStreamInit with payload [0x01,0x00,0x00,0x00] ("response ready")
        loop {
            let pkt = self.recv_with_timeout(CMD_TIMEOUT).await?;
            match pkt.command {
                Command::BoxStreamInit => {
                    // payload[0] == 0x01 means device has a response queued
                    if pkt.payload.first() == Some(&0x01) {
                        break;
                    }
                    warn!("BoxStreamInit with unexpected payload {:?}", pkt.payload);
                }
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                other => {
                    warn!("Unexpected {:?} while waiting for BoxStreamInit(ready)", other);
                }
            }
        }

        // Step 6: send BoxStreamInitAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamInitAck, vec![0x00, 0x00])).await?;

        // Step 7: wait for BoxStreamData response
        let resp = self.expect_cmd(Command::BoxStreamData, CMD_TIMEOUT).await?;
        let xml_bytes = parse_box_stream_data(&resp.payload)
            .ok_or_else(|| Error::Protocol(
                format!("BoxStreamData response too short: {} bytes", resp.payload.len())
            ))?;
        let response_xml = String::from_utf8_lossy(xml_bytes).into_owned();
        debug!("BoxStream response: {} bytes", response_xml.len());

        // Step 8: send BoxStreamRxAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamRxAck, vec![0x00, 0x00])).await?;

        // Step 9: wait for BoxStreamTxAck from device
        self.expect_cmd(Command::BoxStreamTxAck, CMD_TIMEOUT).await?;

        // Step 10: send BoxStreamFinalAck [0x00,0x00]
        self.send_packet(&Packet::new(Command::BoxStreamFinalAck, vec![0x00, 0x00])).await?;

        Ok(response_xml)
    }

    /// Send a single SDK XML method call and check the result.
    async fn sdk_cmd(&mut self, method: &str, body: &str) -> Result<String> {
        let xml_str = xml::sdk_request(&self.client_guid, method, body);
        let response = self.send_xml_request(&xml_str).await?;
        debug!("SDK response for {method}: {} bytes", response.len());
        xml::parse_result(&response)?;
        Ok(response)
    }

    /// Send a heartbeat packet.
    pub async fn heartbeat(&mut self) -> Result<()> {
        let pkt = Packet::heartbeat();
        self.send_packet(&pkt).await
    }

    // ── Device Info ──────────────────────────────────────────────────────

    /// Get device information via a batch request.
    ///
    /// The Huidu C15 BoxPlayer does not support `GetDeviceInfo` — instead we send a
    /// batch of individual methods matching what HDPlayer.exe requests in practice
    /// (confirmed from Huidu.pcapng).
    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        use huidu_protocol::xml::sdk_batch_request;
        let request_xml = sdk_batch_request(&[
            ("GetDeviceName",         ""),
            ("GetFirewareVersion",    ""),
            ("GetScreenInfo",         ""),
            ("GetEth0Info",           ""),
            ("GetTimeInfo",           ""),
            ("GetCurrentLuminance",   ""),
        ]);
        let response = self.send_xml_request(&request_xml).await?;
        let mut info = DeviceDetails { raw_xml: response.clone(), ..Default::default() };
        info.device_type = "BoxPlayer".to_string();

        // GetDeviceName → <name value="BoxPlayer"/>
        if let Some(body) = extract_out_body(&response, "GetDeviceName") {
            info.device_name = xml::get_attr(&body, "value")
                .map(xml::xml_unescape).unwrap_or_default();
        }

        // GetFirewareVersion → <app version="7.4.61.0"/><fpga version="6.3.70.0"/>
        if let Some(body) = extract_out_body(&response, "GetFirewareVersion") {
            if let Some(app_pos) = body.find("<app ") {
                info.firmware_version = xml::get_attr(&body[app_pos..], "version")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
        }

        // GetScreenInfo → <width value="128"/><height value="128"/>
        //                  <space value="..."/><total value="..."/>
        if let Some(body) = extract_out_body(&response, "GetScreenInfo") {
            if let Some(p) = body.find("<width ") {
                info.screen_width = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<height ") {
                info.screen_height = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<space ") {
                info.storage_free = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<total ") {
                info.storage_total = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
            if let Some(p) = body.find("<rotation ") {
                info.rotation = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }

        // GetEth0Info → <ip addr="..."/><netmask addr="..."/><mac addr="..."/>
        if let Some(body) = extract_out_body(&response, "GetEth0Info") {
            if let Some(p) = body.find("<ip ") {
                info.ip_address = xml::get_attr(&body[p..], "addr")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(p) = body.find("<mac ") {
                info.mac_address = xml::get_attr(&body[p..], "addr")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(p) = body.find("<dhcp ") {
                let auto_val = xml::get_attr(&body[p..], "auto").unwrap_or("0");
                info.device_id = format!("dhcp={auto_val}"); // placeholder until a real device_id method exists
            }
        }

        // GetCurrentLuminance → <percent value="100"/>
        if let Some(body) = extract_out_body(&response, "GetCurrentLuminance") {
            if let Some(p) = body.find("<percent ") {
                info.brightness = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(100);
            }
        }

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
        // Real device response (confirmed from Huidu.pcapng):
        //   <enable value="true"/>
        //   <dhcp auto="1"/>
        //   <ip addr="192.168.1.104"/>
        //   <netmask addr="255.255.255.0"/>
        //   <gateway addr="192.168.1.1"/>
        //   <dns addr="192.168.1.1"/>
        //   <mac addr="28:32:fd:be:36:40"/>
        let addr_of = |tag: &str| -> String {
            xml.find(&format!("<{tag} "))
                .and_then(|p| crate::xml::get_attr(&xml[p..], "addr"))
                .map(str::to_string)
                .unwrap_or_default()
        };
        let dhcp = xml.find("<dhcp ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "auto"))
            .map(|v| v == "1" || v == "true")
            .unwrap_or(false);
        Ok(EthConfig {
            dhcp,
            ip:      addr_of("ip"),
            mask:    addr_of("netmask"),
            gateway: addr_of("gateway"),
            dns:     addr_of("dns"),
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
    ///
    /// Uses `GetCurrentLuminance` which returns `<percent value="N"/>`.
    /// Confirmed from More Huidu.pcapng: response is `<percent value="100"/>`.
    pub async fn get_brightness(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetCurrentLuminance", "").await?;
        // Response: <out result="kSuccess" method="GetCurrentLuminance"><percent value="100"/></out>
        let level = xml.find("<percent ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
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

// ── Free helpers ──────────────────────────────────────────────────────────────

/// Extract the inner body of `<out method="name">...</out>` from a batch response XML.
///
/// Returns `Some(body_xml)` — the content between the opening and closing `</out>` tags.
/// Returns `Some("")` for self-closing tags (e.g. `<out result="kUnsupportMethod" .../>`).
/// Returns `None` if the method's `<out>` block is not present in the XML.
fn extract_out_body(xml: &str, method: &str) -> Option<String> {
    let needle = format!("method=\"{method}\"");
    let method_pos = xml.find(&needle)?;
    // Walk back to the start of the enclosing <out element
    let out_start = xml[..method_pos].rfind("<out")?;
    let rest = &xml[out_start..];
    // Find the end of the opening tag
    let tag_end = rest.find('>')?;
    // Self-closing? e.g. <out result="kUnsupportMethod" method="X"/>
    if rest.as_bytes().get(tag_end.saturating_sub(1)) == Some(&b'/') {
        return Some(String::new());
    }
    let body_start = tag_end + 1;
    let close_off = rest[body_start..].find("</out>")?;
    Some(rest[body_start..body_start + close_off].to_string())
}
