//! TCP client for communicating with Huidu BoxPlayer devices.
//!
//! Implements the full SDK XML command protocol plus file transfer.

use std::net::Ipv4Addr;
use std::str::FromStr;
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

/// Information returned by the full device status batch request.
#[derive(Debug, Clone, Default)]
pub struct DeviceDetails {
    pub device_id: String,
    pub device_name: String,
    pub device_type: String,
    pub firmware_version: String,
    pub fpga_version: String,
    pub screen_width: u32,
    pub screen_height: u32,
    pub storage_total: u64,
    pub storage_free: u64,
    pub ip_address: String,
    pub mac_address: String,
    pub dhcp: bool,
    pub volume: u8,
    pub brightness: u8,
    pub rotation: u32,
    pub admin_mode: bool,
    pub current_program_guid: Option<String>,
    /// Play status: 0 = stopped, 1 = playing (from GetPlayStatus).
    pub play_status: u8,
    /// Device time string from GetTimeInfo (e.g. "2026-02-23 05:01:32").
    pub device_time: String,
    /// NTP server list from GetTimeInfo.
    pub ntp_server: String,
    /// Screen-on scheduling enabled (from GetSwitchTime).
    pub switch_time_on: bool,
    /// Device locker feature enabled (from GetDeviceLockerEnable).
    pub locker_enabled: bool,
    /// Boot logo present on device (from GetBootLogo).
    pub boot_logo_exists: bool,
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
        // UDP registration MUST happen before TCP BoxStream connection.
        //
        // Confirmed from hdplayer_real.pcapng: real HDPlayer performs a UDP handshake on
        // port 9526 before connecting TCP.  Without this, the device accepts the TCP
        // connection, sends TcpHeartbeatAnswer, but then closes the connection (FIN) after
        // receiving BoxStreamInit — regardless of timing.
        //
        // The device sends unicast UDP from port 9527 TO port 9526 on the controller.
        // We must respond with cmd=0x0003 (and optionally cmd=0x0006/0x0341) to be
        // "registered" as an authorised controller.
        if let Ok(ip) = Ipv4Addr::from_str(host) {
            match huidu_protocol::udp_register(ip, Duration::from_secs(12)).await {
                Ok(()) => info!("UDP registration complete"),
                Err(e) => warn!("UDP registration failed ({e}) — attempting TCP anyway"),
            }
        } else {
            warn!("Host '{host}' is not an IPv4 address — skipping UDP registration");
        }

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

        // Handshake sequence confirmed from Huidu.pcapng (working capture):
        //
        //  1. TCP connect
        //  2. Device → PC: TcpHeartbeatAnswer (0x0060)  ← device signals "ready"
        //  3. PC → Device: TcpHeartbeatAsk   (0x005F)   ← PC acknowledges device greeting
        //  4. ~107ms gap  (device processes TcpHeartbeatAsk before next packet)
        //  5. PC → Device: BoxStreamInit      (0x0200)   ← done by send_xml_request()
        //
        // From live testing failures (confirmed by packet captures):
        //  - BoxStreamInit before TcpHeartbeatAnswer  → RST  (app not ready)
        //  - BoxStreamInit immediately after TcpHeartbeatAnswer without TcpHeartbeatAsk → FIN
        //  - TcpHeartbeatAsk + BoxStreamInit with 0ms gap → FIN
        //  - TcpHeartbeatAsk + BoxStreamInit with 2s gap → FIN (too slow overall)
        //
        // The 150ms sleep after TcpHeartbeatAsk matches the observed ~107ms gap in the
        // working capture and gives the device time to process TcpHeartbeatAsk before
        // BoxStreamInit arrives.
        // Wait for TWO TcpHeartbeatAnswer cycles (~12s) before attempting BoxStreamInit.
        // Testing hypothesis: device only accepts BoxStreamInit after connection has been
        // alive long enough to complete multiple heartbeat cycles.
        info!("Waiting for TcpHeartbeatAnswer cycles...");
        let mut heartbeat_count = 0u32;
        loop {
            match client.recv_with_timeout(Duration::from_secs(20)).await {
                Ok(pkt) if pkt.command == Command::TcpHeartbeatAnswer => {
                    heartbeat_count += 1;
                    info!("Received TcpHeartbeatAnswer #{heartbeat_count} — sending TcpHeartbeatAsk");
                    client.send_packet(&Packet::new(Command::TcpHeartbeatAsk, vec![])).await?;
                    if heartbeat_count >= 2 {
                        tokio::time::sleep(Duration::from_millis(150)).await;
                        info!("Got {heartbeat_count} heartbeats — ready for BoxStreamInit");
                        break;
                    }
                }
                Ok(pkt) => {
                    warn!("Expected TcpHeartbeatAnswer, got {:?} — ignoring", pkt.command);
                }
                Err(Error::Timeout) => {
                    warn!("No TcpHeartbeatAnswer within 20s — proceeding anyway");
                    break;
                }
                Err(e) => return Err(e),
            }
        }

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
        loop {
            let resp = self.recv_with_timeout(CMD_TIMEOUT).await?;
            match resp.command {
                Command::BoxStreamInitAck => {
                    info!("BoxStream session established");
                    return Ok(());
                }
                // Device may send periodic keepalives while we wait.
                Command::TcpHeartbeatAnswer => {
                    debug!("Received TcpHeartbeatAnswer while waiting for BoxStreamInitAck — ignoring");
                }
                Command::TcpHeartbeatAsk => {
                    self.send_packet(&Packet::heartbeat()).await?;
                }
                other => {
                    return Err(Error::Protocol(format!(
                        "Expected BoxStreamInitAck, got {other:?}"
                    )));
                }
            }
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
                // Device sends TcpHeartbeatAnswer periodically; ignore mid-exchange.
                Command::TcpHeartbeatAnswer => {
                    debug!("Received TcpHeartbeatAnswer mid-exchange — ignoring");
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

    /// Get full device status via a 23-method batch request.
    ///
    /// Sends the same methods HDPlayer.exe requests on initial connection
    /// (confirmed from Huidu.pcapng and More Huidu.pcapng):
    /// GetDeviceName, GetFirewareVersion, GetKeyDefine, GetPlayStatus,
    /// GetSystemVolume, GetBootLogo, GetSensorInfo, GetGPSInfo,
    /// GetCurrentLuminance, GetCurrentTemperature, GetCurrentHumity,
    /// GetSensorType, GetSwitchTime, GetTimeInfo, GetLuminancePloy,
    /// GetScreenInfo, GetLicense, GetEth0Info, GetWifiInfo, GetPppoeInfo,
    /// GetDeviceInfo, GetDataSourceInfo, GetRelay.
    ///
    /// Methods not supported by the device (e.g. GetSensorType, GetDeviceInfo,
    /// GetDataSourceInfo) return kUnsupportMethod and are silently skipped.
    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        use huidu_protocol::xml::sdk_batch_request;
        // Full 23-method batch — mirrors HDPlayer.exe initial sync exactly.
        let request_xml = sdk_batch_request(&[
            ("GetDeviceName",           ""),
            ("GetFirewareVersion",      ""),
            ("GetKeyDefine",            ""),
            ("GetPlayStatus",           ""),
            ("GetSystemVolume",         ""),
            ("GetBootLogo",             ""),
            ("GetSensorInfo",           ""),
            ("GetGPSInfo",              ""),
            ("GetCurrentLuminance",     ""),
            ("GetCurrentTemperature",   ""),
            ("GetCurrentHumity",        ""),  // note: protocol typo "Humity"
            ("GetSensorType",           ""),
            ("GetSwitchTime",           ""),
            ("GetTimeInfo",             ""),
            ("GetLuminancePloy",        ""),
            ("GetScreenInfo",           ""),
            ("GetLicense",              ""),
            ("GetEth0Info",             ""),
            ("GetWifiInfo",             ""),
            ("GetPppoeInfo",            ""),
            ("GetDeviceInfo",           ""),  // returns kUnsupportMethod on C-series
            ("GetDataSourceInfo",       ""),  // returns kUnsupportMethod on C-series
            ("GetRelay",                ""),
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
            if let Some(fpga_pos) = body.find("<fpga ") {
                info.fpga_version = xml::get_attr(&body[fpga_pos..], "version")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
        }

        // GetPlayStatus → <status value="1"/> (1=playing, 0=stopped)
        if let Some(body) = extract_out_body(&response, "GetPlayStatus") {
            if let Some(p) = body.find("<status ") {
                info.play_status = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(0);
            }
        }

        // GetSystemVolume → <volume precent="100"/> (note: protocol typo "precent")
        if let Some(body) = extract_out_body(&response, "GetSystemVolume") {
            if let Some(p) = body.find("<volume ") {
                info.volume = xml::get_attr(&body[p..], "precent")
                    .and_then(|v| v.parse().ok()).unwrap_or(100);
            }
        }

        // GetBootLogo → <logo exist="false"/>
        if let Some(body) = extract_out_body(&response, "GetBootLogo") {
            info.boot_logo_exists = xml::get_attr(&body, "exist")
                .map(|v| v == "true")
                .unwrap_or(false);
        }

        // GetCurrentLuminance → <percent value="100"/>
        if let Some(body) = extract_out_body(&response, "GetCurrentLuminance") {
            if let Some(p) = body.find("<percent ") {
                info.brightness = xml::get_attr(&body[p..], "value")
                    .and_then(|v| v.parse().ok()).unwrap_or(100);
            }
        }

        // GetSwitchTime → <open enable="true"/>
        if let Some(body) = extract_out_body(&response, "GetSwitchTime") {
            info.switch_time_on = body.find("<open ")
                .and_then(|p| xml::get_attr(&body[p..], "enable"))
                .map(|v| v == "true")
                .unwrap_or(false);
        }

        // GetTimeInfo → <time value="..."/><server list="..."/>
        if let Some(body) = extract_out_body(&response, "GetTimeInfo") {
            if let Some(p) = body.find("<time ") {
                info.device_time = xml::get_attr(&body[p..], "value")
                    .map(xml::xml_unescape).unwrap_or_default();
            }
            if let Some(p) = body.find("<server ") {
                info.ntp_server = xml::get_attr(&body[p..], "list")
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

        // GetEth0Info → <enable value="true"/><dhcp auto="1"/>
        //               <ip addr="..."/><netmask addr="..."/>
        //               <gateway addr="..."/><dns addr="..."/>
        //               <mac addr="28:32:fd:be:36:40"/>
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
                info.dhcp = auto_val == "1" || auto_val == "true";
            }
            // Use MAC as device_id — the most stable unique identifier.
            if !info.mac_address.is_empty() {
                info.device_id = info.mac_address.replace(':', "").to_uppercase();
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

    // ── Sensor / Status queries ───────────────────────────────────────────

    /// Get current play status (0 = stopped, 1 = playing).
    ///
    /// Confirmed from More Huidu.pcapng: response is `<status value="1"/>`.
    pub async fn get_play_status(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetPlayStatus", "").await?;
        Ok(xml.find("<status ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// Get current audio volume (0–100).
    ///
    /// Confirmed from More Huidu.pcapng: response is `<volume precent="100"/>`.
    /// Note: "precent" is the protocol's typo for "percent".
    pub async fn get_volume(&mut self) -> Result<u8> {
        let xml = self.sdk_cmd("GetSystemVolume", "").await?;
        Ok(xml.find("<volume ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "precent"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(100))
    }

    /// Get sensor connectivity info (luminance, temperature, GPS, humidity, etc.)
    ///
    /// Confirmed from More Huidu.pcapng: each sensor has a `<connect enable="0"/>` child.
    pub async fn get_sensor_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetSensorInfo", "").await
    }

    /// Get GPS coordinates.
    ///
    /// Returns `<latitude value="-1"/><longitude value="-1"/>` when GPS is not connected.
    pub async fn get_gps_info(&mut self) -> Result<String> {
        self.sdk_cmd("GetGPSInfo", "").await
    }

    /// Get current ambient temperature from sensor (-1 = no sensor connected).
    pub async fn get_current_temperature(&mut self) -> Result<i32> {
        let xml = self.sdk_cmd("GetCurrentTemperature", "").await?;
        Ok(xml.find("<temperature ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1))
    }

    /// Get current ambient humidity from sensor (-1 = no sensor connected).
    ///
    /// Note: the protocol spells this "Humity" (missing an 'i') — that is intentional
    /// and matches the real device firmware.
    pub async fn get_current_humidity(&mut self) -> Result<i32> {
        let xml = self.sdk_cmd("GetCurrentHumity", "").await?;
        Ok(xml.find("<humity ")
            .and_then(|p| crate::xml::get_attr(&xml[p..], "value"))
            .and_then(|v| v.parse().ok())
            .unwrap_or(-1))
    }

    /// Get key definition (returned as `<key value="0"/>` on most devices).
    pub async fn get_key_define(&mut self) -> Result<u32> {
        let xml = self.sdk_cmd("GetKeyDefine", "").await?;
        Ok(crate::xml::get_attr(&xml, "value")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0))
    }

    /// Get relay status list.
    ///
    /// Returns a Vec of `(use_switch, name, relay_status)` tuples — one per relay.
    /// Confirmed from More Huidu.pcapng: 6 relay items, all disabled.
    pub async fn get_relay(&mut self) -> Result<Vec<(bool, String, u8)>> {
        let xml = self.sdk_cmd("GetRelay", "").await?;
        let mut relays = Vec::new();
        let mut search = xml.as_str();
        while let Some(start) = search.find("<item ") {
            let end = search[start..].find("/>")
                .map(|e| start + e + 2)
                .unwrap_or(search.len());
            let tag = &search[start..end];
            let use_switch = crate::xml::get_attr(tag, "useSwitch")
                .map(|v| v == "1")
                .unwrap_or(false);
            let name = crate::xml::get_attr(tag, "name")
                .map(crate::xml::xml_unescape)
                .unwrap_or_default();
            let status = crate::xml::get_attr(tag, "relayStatus")
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            relays.push((use_switch, name, status));
            search = &search[end.min(search.len())..];
        }
        Ok(relays)
    }

    /// Get sensor measurement range.
    ///
    /// Returns `Ok(Some(xml))` when supported, `Ok(None)` when the device responds
    /// with kUnsupportMethod (confirmed: C-series BoxPlayer does not support this).
    pub async fn get_sensor_range(&mut self) -> Result<Option<String>> {
        let request_xml = xml::sdk_request(&self.client_guid, "GetSensorRange", "");
        let response = self.send_xml_request(&request_xml).await?;
        if response.contains("kUnsupportMethod") {
            return Ok(None);
        }
        Ok(Some(response))
    }

    /// Get device locker enabled status.
    ///
    /// Confirmed from More Huidu.pcapng: response is `<enable value="false"/>`.
    pub async fn get_device_locker_enable(&mut self) -> Result<bool> {
        let request_xml = xml::sdk_request(&self.client_guid, "GetDeviceLockerEnable", "");
        let response = self.send_xml_request(&request_xml).await?;
        if response.contains("kUnsupportMethod") {
            return Ok(false);
        }
        Ok(response.find("<enable ")
            .and_then(|p| crate::xml::get_attr(&response[p..], "value"))
            .map(|v| v == "true")
            .unwrap_or(false))
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
