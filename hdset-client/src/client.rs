//! TCP client for communicating with Huidu devices via the HDSet protocol.
//!
//! HDSet uses the same TCP binary framing as HDPlayer (port 10001) but sends
//! different SDK XML commands focused on hardware configuration:
//!
//! - `GetBoxHwConfig` / `SetBoxHwConfig` — FPGA configuration XML
//! - `SmartSetting` / `SmartDrawLine` — LED module auto-detection & test
//! - `GetBootLogo` / `SetBootLogoName` — startup image management
//! - `ScreenTest` — visual hardware test patterns
//! - Standard device/network/firmware commands (same as HDPlayer)
//!
//! There is also a FPGA session binary sub-protocol (`kFPGASettingIn/Out`,
//! `kFPGAParamSet`, `kFPGASetCmd`) whose exact command codes are pending wire
//! capture confirmation — see `protocol.rs`.

use std::path::Path;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use std::sync::Arc;
use tracing::info;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::hw_config::BoxHwConfig;
use crate::protocol::{Command, Packet, sdk_service_ask_payload};
use crate::transfer::{FileTransfer, ProgressFn};
use crate::xml;

pub const DEFAULT_PORT: u16 = 10001;
pub const SDK_VERSION: u32 = huidu_protocol::packet::SDK_CLIENT_VERSION;
pub const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(30);
pub const CMD_TIMEOUT: Duration = Duration::from_secs(30);

/// Summary info about the connected device.
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
    pub rotation: u32,
    pub raw_xml: String,
}

/// Ethernet configuration of a device.
#[derive(Debug, Clone, Default)]
pub struct EthInfo {
    pub dhcp: bool,
    pub ip: String,
    pub mask: String,
    pub gateway: String,
    pub dns: String,
}

/// TCP client for HDSet hardware configuration operations.
pub struct HdsetClient {
    stream: Arc<Mutex<TcpStream>>,
    client_guid: String,
    read_buf: Vec<u8>,
}

impl HdsetClient {
    /// Connect to a Huidu device at `host:port` and perform the SDK handshake.
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
        // SDK handshake: send SdkServiceAsk with version
        let handshake = Packet::new(Command::SdkServiceAsk, sdk_service_ask_payload(SDK_VERSION));
        client.send_packet(&handshake).await?;
        let resp = client.recv_packet().await?;
        if resp.command != Command::SdkServiceAnswer {
            return Err(Error::Protocol("Expected SdkServiceAnswer".into()));
        }
        info!("SDK handshake complete for {addr}");
        Ok(client)
    }

    // ── Internal helpers ────────────────────────────────────────────────────────

    async fn send_packet(&self, pkt: &Packet) -> Result<()> {
        let bytes = pkt.to_bytes();
        let mut stream = self.stream.lock().await;
        stream.write_all(&bytes).await?;
        Ok(())
    }

    async fn recv_packet(&mut self) -> Result<Packet> {
        let deadline = tokio::time::Instant::now() + CMD_TIMEOUT;
        loop {
            if let Some((pkt, consumed)) = Packet::from_bytes(&self.read_buf)? {
                self.read_buf.drain(..consumed);
                if pkt.command == Command::TcpHeartbeatAsk {
                    // Auto-respond to heartbeat pings
                    let resp = Packet::new(Command::TcpHeartbeatAnswer, vec![]);
                    self.send_packet(&resp).await?;
                    continue;
                }
                return Ok(pkt);
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(Error::Timeout);
            }
            let mut tmp = [0u8; 4096];
            let n = tokio::time::timeout(remaining, async {
                let mut s = self.stream.lock().await;
                s.read(&mut tmp).await
            }).await
                .map_err(|_| Error::Timeout)?
                .map_err(|e| Error::Connection(format!("read: {e}")))?;
            if n == 0 {
                return Err(Error::Connection("connection closed".into()));
            }
            self.read_buf.extend_from_slice(&tmp[..n]);
        }
    }

    /// Send an SDK XML command and return the response XML.
    async fn send_sdk_cmd(&mut self, method: &str, body: &str) -> Result<String> {
        let req_xml = xml::sdk_request(&self.client_guid, method, body);
        let payload = req_xml.into_bytes();
        let pkt = Packet::new(Command::SdkCmdAsk, payload);
        self.send_packet(&pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::SdkCmdAnswer {
            return Err(Error::Protocol(format!("Expected SdkCmdAnswer, got {:?}", resp.command)));
        }
        let xml_str = String::from_utf8_lossy(&resp.payload).into_owned();
        xml::parse_result(&xml_str)?;
        Ok(xml_str)
    }

    // ── Device information ──────────────────────────────────────────────────────

    pub async fn get_device_info(&mut self) -> Result<DeviceDetails> {
        let xml = self.send_sdk_cmd("GetDeviceInfo", "").await?;
        let body = xml::parse_out_body(&xml).unwrap_or_default();
        Ok(DeviceDetails {
            device_id:       xml::get_attr(&body, "DeviceID").unwrap_or("").to_string(),
            device_name:     xml::get_attr(&body, "DeviceName").unwrap_or("").to_string(),
            device_type:     xml::get_attr(&body, "DeviceType").unwrap_or("").to_string(),
            firmware_version: xml::get_attr(&body, "SoftwareVersion").unwrap_or("").to_string(),
            screen_width:    xml::get_attr(&body, "ScreenWidth").and_then(|s| s.parse().ok()).unwrap_or(0),
            screen_height:   xml::get_attr(&body, "ScreenHeight").and_then(|s| s.parse().ok()).unwrap_or(0),
            storage_total:   xml::get_attr(&body, "StorageSize").and_then(|s| s.parse().ok()).unwrap_or(0),
            storage_free:    xml::get_attr(&body, "StorageFreeSize").and_then(|s| s.parse().ok()).unwrap_or(0),
            ip_address:      xml::get_attr(&body, "IPAddress").unwrap_or("").to_string(),
            mac_address:     xml::get_attr(&body, "MacAddress").unwrap_or("").to_string(),
            brightness:      xml::get_attr(&body, "Brightness").and_then(|s| s.parse().ok()).unwrap_or(0),
            rotation:        xml::get_attr(&body, "Rotation").and_then(|s| s.parse().ok()).unwrap_or(0),
            raw_xml:         xml,
        })
    }

    pub async fn reboot(&mut self) -> Result<()> {
        self.send_sdk_cmd("Reboot", "").await?;
        Ok(())
    }

    // ── FPGA / hardware configuration ───────────────────────────────────────────

    /// Read the complete BoxHwConfig from the device.
    pub async fn get_box_hw_config(&mut self) -> Result<BoxHwConfig> {
        let resp_xml = self.send_sdk_cmd("GetBoxHwConfig", "").await?;
        let body = xml::parse_out_body(&resp_xml).unwrap_or_default();
        crate::hw_config::from_xml(&body)
            .ok_or_else(|| Error::Xml("Failed to parse BoxHwConfig".into()))
    }

    /// Write a BoxHwConfig to the device (applies immediately).
    pub async fn set_box_hw_config(&mut self, config: &BoxHwConfig) -> Result<()> {
        let hw_xml = crate::hw_config::to_xml(config);
        self.send_sdk_cmd("SetBoxHwConfig", &hw_xml).await?;
        Ok(())
    }

    /// Persist the current BoxHwConfig to flash storage.
    pub async fn save_box_hw_config(&mut self) -> Result<()> {
        self.send_sdk_cmd("SaveBoxHwConfig", "").await?;
        Ok(())
    }

    /// Replace the entire BoxHwConfig (full replacement, not merge).
    pub async fn replace_box_hw_config(&mut self, config: &BoxHwConfig) -> Result<()> {
        let hw_xml = crate::hw_config::to_xml(config);
        self.send_sdk_cmd("ReplaceBoxHwConfig", &hw_xml).await?;
        Ok(())
    }

    /// Read raw FPGA config XML (alternate method used by older SDK).
    pub async fn get_sdk_fpga_config(&mut self) -> Result<String> {
        let resp = self.send_sdk_cmd("GetSDKFPGAConfig", "").await?;
        Ok(xml::parse_out_body(&resp).unwrap_or_default())
    }

    /// Write raw FPGA config XML.
    pub async fn set_sdk_fpga_config(&mut self, fpga_xml: &str) -> Result<()> {
        self.send_sdk_cmd("SetSDKFPGAConfig", fpga_xml).await?;
        Ok(())
    }

    /// Request the device to auto-detect LED module parameters (SmartSetting).
    /// This triggers the device to probe the connected hardware and fill in
    /// the BoxHwConfig automatically.
    pub async fn smart_setting(&mut self) -> Result<()> {
        self.send_sdk_cmd("SmartSetting", "").await?;
        Ok(())
    }

    /// Request SmartDrawLine — automatic LED line detection test.
    /// The device draws diagonal lines at various orientations to detect
    /// physical LED panel dimensions and scan parameters.
    pub async fn smart_draw_line(&mut self) -> Result<()> {
        self.send_sdk_cmd("SmartDrawLine", "").await?;
        Ok(())
    }

    // ── Screen test ─────────────────────────────────────────────────────────────

    /// Run the built-in screen test pattern for `duration_secs` seconds.
    /// Displays SMPTE color bars, white/red/green/blue/black fill patterns.
    pub async fn screen_test(&mut self, duration_secs: u32) -> Result<()> {
        let body = format!("<screenTest duration=\"{}\"/>", duration_secs);
        self.send_sdk_cmd("ScreenTest", &body).await?;
        Ok(())
    }

    /// Display a solid color on the entire screen (useful for hardware check).
    /// r/g/b are 0–255.
    pub async fn screen_test_color(&mut self, r: u8, g: u8, b: u8) -> Result<()> {
        let body = format!(
            "<screenTest type=\"solidColor\" r=\"{}\" g=\"{}\" b=\"{}\"/>",
            r, g, b
        );
        self.send_sdk_cmd("ScreenTest", &body).await?;
        Ok(())
    }

    // ── Boot logo ───────────────────────────────────────────────────────────────

    pub async fn get_boot_logo(&mut self) -> Result<String> {
        let resp = self.send_sdk_cmd("GetBootLogo", "").await?;
        let body = xml::parse_out_body(&resp).unwrap_or_default();
        Ok(xml::get_attr(&body, "name").unwrap_or("").to_string())
    }

    pub async fn set_boot_logo(&mut self, filename: &str) -> Result<()> {
        let body = format!("<bootLogo name=\"{}\"/>", xml::xml_escape(filename));
        self.send_sdk_cmd("SetBootLogoName", &body).await?;
        Ok(())
    }

    pub async fn clear_boot_logo(&mut self) -> Result<()> {
        self.send_sdk_cmd("ClearBootLogo", "").await?;
        Ok(())
    }

    /// Upload a boot logo image file to the device, then activate it.
    pub async fn upload_boot_logo(&mut self, path: &Path) -> Result<()> {
        let transfer = FileTransfer::from_file(path).await?;
        let filename = transfer.filename.clone();
        self.upload_file(&transfer, None).await?;
        self.set_boot_logo(&filename).await?;
        Ok(())
    }

    // ── Brightness ──────────────────────────────────────────────────────────────

    pub async fn set_brightness(&mut self, level: u8) -> Result<()> {
        let body = format!("<luminancePloy type=\"0\" brightness=\"{}\"/>", level);
        self.send_sdk_cmd("SetLuminancePloy", &body).await?;
        Ok(())
    }

    pub async fn get_brightness_policy(&mut self) -> Result<String> {
        let resp = self.send_sdk_cmd("GetLuminancePloy", "").await?;
        Ok(xml::parse_out_body(&resp).unwrap_or_default())
    }

    // ── Screen on/off ───────────────────────────────────────────────────────────

    pub async fn open_screen(&mut self) -> Result<()> {
        self.send_sdk_cmd("OpenScreen", "").await?;
        Ok(())
    }

    pub async fn close_screen(&mut self) -> Result<()> {
        self.send_sdk_cmd("CloseScreen", "").await?;
        Ok(())
    }

    // ── Network configuration ───────────────────────────────────────────────────

    pub async fn get_eth_info(&mut self) -> Result<EthInfo> {
        let resp = self.send_sdk_cmd("GetEth0Info", "").await?;
        let body = xml::parse_out_body(&resp).unwrap_or_default();
        Ok(EthInfo {
            dhcp:    xml::get_attr(&body, "dhcp").map(|s| s == "1" || s.eq_ignore_ascii_case("true")).unwrap_or(true),
            ip:      xml::get_attr(&body, "ip").unwrap_or("").to_string(),
            mask:    xml::get_attr(&body, "mask").unwrap_or("").to_string(),
            gateway: xml::get_attr(&body, "gateway").unwrap_or("").to_string(),
            dns:     xml::get_attr(&body, "dns").unwrap_or("").to_string(),
        })
    }

    pub async fn set_eth_info(&mut self, info: &EthInfo) -> Result<()> {
        let body = format!(
            "<ethernet dhcp=\"{}\" ip=\"{}\" mask=\"{}\" gateway=\"{}\" dns=\"{}\"/>",
            if info.dhcp { "1" } else { "0" },
            xml::xml_escape(&info.ip),
            xml::xml_escape(&info.mask),
            xml::xml_escape(&info.gateway),
            xml::xml_escape(&info.dns),
        );
        self.send_sdk_cmd("SetEth0Info", &body).await?;
        Ok(())
    }

    // ── Firmware upgrade ────────────────────────────────────────────────────────

    /// Upload a .zbin firmware file and trigger the upgrade.
    pub async fn firmware_upgrade(&mut self, path: &Path) -> Result<()> {
        let transfer = FileTransfer::from_file(path).await?;
        let filename = transfer.filename.clone();
        info!("Uploading firmware: {} ({} bytes)", filename, transfer.total_size());
        self.upload_file(&transfer, None).await?;
        let body = format!("<firmwareUpgrade filename=\"{}\"/>", xml::xml_escape(&filename));
        self.send_sdk_cmd("FirmwareUpgrade", &body).await?;
        Ok(())
    }

    pub async fn get_upgrade_result(&mut self) -> Result<String> {
        let resp = self.send_sdk_cmd("GetUpgradeResult", "").await?;
        Ok(xml::parse_out_body(&resp).unwrap_or_default())
    }

    // ── FPGA session binary protocol ────────────────────────────────────────────
    //
    // These commands enter a low-level FPGA configuration session where raw
    // parameter data is sent as binary packets rather than XML.  The exact
    // command code values are placeholders pending wire-capture confirmation.

    /// Enter the FPGA hardware configuration session.
    pub async fn fpga_setting_in(&mut self) -> Result<()> {
        let pkt = Packet::new(Command::FpgaSettingInAsk, vec![]);
        self.send_packet(&pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::FpgaSettingInAnswer {
            return Err(Error::Fpga("FpgaSettingIn: unexpected response".into()));
        }
        Ok(())
    }

    /// Exit the FPGA hardware configuration session.
    pub async fn fpga_setting_out(&mut self) -> Result<()> {
        let pkt = Packet::new(Command::FpgaSettingOutAsk, vec![]);
        self.send_packet(&pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::FpgaSettingOutAnswer {
            return Err(Error::Fpga("FpgaSettingOut: unexpected response".into()));
        }
        Ok(())
    }

    /// Send a raw FPGA parameter block inside an active FPGA session.
    /// `param_data` is device/chip-specific binary data.
    pub async fn fpga_param_set(&mut self, param_data: &[u8]) -> Result<()> {
        let pkt = Packet::new(Command::FpgaParamSetAsk, param_data.to_vec());
        self.send_packet(&pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::FpgaParamSetAnswer {
            return Err(Error::Fpga("FpgaParamSet: unexpected response".into()));
        }
        Ok(())
    }

    /// Send a raw FPGA command inside an active FPGA session.
    pub async fn fpga_set_cmd(&mut self, cmd_data: &[u8]) -> Result<()> {
        let pkt = Packet::new(Command::FpgaSetCmdAsk, cmd_data.to_vec());
        self.send_packet(&pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::FpgaSetCmdAnswer {
            return Err(Error::Fpga("FpgaSetCmd: unexpected response".into()));
        }
        Ok(())
    }

    // ── File transfer ───────────────────────────────────────────────────────────

    pub async fn upload_file(
        &mut self,
        transfer: &FileTransfer,
        progress: Option<ProgressFn>,
    ) -> Result<()> {
        // File start
        let start_pkt = transfer.start_packet();
        self.send_packet(&start_pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::FileStartAnswer {
            return Err(Error::Transfer(format!("Expected FileStartAnswer, got {:?}", resp.command)));
        }
        // Content chunks
        let total = transfer.total_size();
        for (offset, chunk) in transfer.chunks() {
            let content_pkt = crate::transfer::build_content_packet(offset, chunk);
            self.send_packet(&content_pkt).await?;
            let resp = self.recv_packet().await?;
            if resp.command != Command::FileContentAnswer {
                return Err(Error::Transfer(format!("Expected FileContentAnswer, got {:?}", resp.command)));
            }
            if let Some(ref cb) = progress {
                cb(offset + chunk.len() as u64, total);
            }
        }
        // File end
        let end_pkt = transfer.end_packet();
        self.send_packet(&end_pkt).await?;
        let resp = self.recv_packet().await?;
        if resp.command != Command::FileEndAnswer {
            return Err(Error::Transfer(format!("Expected FileEndAnswer, got {:?}", resp.command)));
        }
        info!("File transfer complete: {}", transfer.filename);
        Ok(())
    }

    // ── Heartbeat ───────────────────────────────────────────────────────────────

    pub async fn heartbeat(&mut self) -> Result<()> {
        self.send_packet(&Packet::heartbeat()).await
    }
}
