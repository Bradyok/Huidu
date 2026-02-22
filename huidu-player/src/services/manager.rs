/// Services manager — coordinates background services.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::core::player::PlayerCommand;
use crate::services::brightness::BrightnessService;
use crate::services::modbus_service::ModbusSourceConfig;
use crate::services::screen_schedule::ScreenScheduleService;
use crate::services::storage::StorageService;
use crate::services::time_sync::TimeSyncService;
use crate::services::upgrade::UpgradeStatus;
use crate::services::usb_disk::UsbDiskService;

/// Persisted device state — survives restarts.
/// Saved to `<program_dir>/device_state.json`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeviceState {
    pub device_name: String,
    pub brightness: u8,
    pub volume: u8,
    pub rotation: u16,
    pub admin_mode: bool,
    pub ntp_server: String,
    /// UTC offset in whole hours (-12 … +14).  0 = UTC (default).
    #[serde(default)]
    pub timezone_offset: i8,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub brightness_schedule: Vec<crate::services::brightness::BrightnessScheduleEntry>,
    #[serde(default)]
    pub screen_schedule: Vec<crate::services::screen_schedule::ScreenScheduleEntry>,
    /// Boot logo image filename (relative to program_dir).  None = no boot logo.
    #[serde(default)]
    pub boot_logo: Option<String>,
    /// Live data source values set via SetDataSourceInfo.
    #[serde(default)]
    pub data_sources: HashMap<String, String>,
    /// Status of the most recent firmware upgrade attempt.
    #[serde(default)]
    pub upgrade_status: UpgradeStatus,

    // Network — WiFi
    #[serde(default)]
    pub wifi_ssid: String,
    #[serde(default)]
    pub wifi_password: String,
    #[serde(default)]
    pub wifi_enable: bool,

    // Network — Ethernet (eth0)
    #[serde(default = "default_eth0_dhcp")]
    pub eth0_dhcp: bool,
    #[serde(default)]
    pub eth0_ip: String,
    #[serde(default = "default_eth0_mask")]
    pub eth0_mask: String,
    #[serde(default)]
    pub eth0_gateway: String,
    #[serde(default = "default_dns")]
    pub eth0_dns: String,

    // Network — PPPoE
    #[serde(default)]
    pub pppoe_enable: bool,
    #[serde(default)]
    pub pppoe_user: String,
    #[serde(default)]
    pub pppoe_password: String,

    /// SHA-256 hex hash of the admin unlock password.
    /// Empty string means no password is required.
    #[serde(default)]
    pub admin_password_hash: String,

    // Hardware sensors / relay
    #[serde(default)]
    pub relay_pins: Vec<u32>,
    // GPS
    #[serde(default)]
    pub gps_device: String,
    // Modem
    #[serde(default)]
    pub modem_device: String,
    #[serde(default)]
    pub modem_apn: String,
    #[serde(default)]
    pub modem_user: String,
    #[serde(default)]
    pub modem_password: String,
    // Modbus sources
    #[serde(default)]
    pub modbus_sources: Vec<ModbusSourceConfig>,
}

fn default_eth0_dhcp() -> bool { true }
fn default_eth0_mask() -> String { "255.255.255.0".to_string() }
fn default_dns() -> String { "8.8.8.8".to_string() }

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            device_name: "huidu-player".to_string(),
            brightness: 100,
            volume: 80,
            rotation: 0,
            admin_mode: false,
            ntp_server: "pool.ntp.org".to_string(),
            timezone_offset: 0,
            license: String::new(),
            brightness_schedule: Vec::new(),
            screen_schedule: Vec::new(),
            boot_logo: None,
            data_sources: HashMap::new(),
            upgrade_status: UpgradeStatus::Idle,
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            wifi_enable: false,
            eth0_dhcp: true,
            eth0_ip: String::new(),
            eth0_mask: "255.255.255.0".to_string(),
            eth0_gateway: String::new(),
            eth0_dns: "8.8.8.8".to_string(),
            pppoe_enable: false,
            pppoe_user: String::new(),
            pppoe_password: String::new(),
            admin_password_hash: String::new(),
            relay_pins: Vec::new(),
            gps_device: String::new(),
            modem_device: String::new(),
            modem_apn: String::new(),
            modem_user: String::new(),
            modem_password: String::new(),
            modbus_sources: Vec::new(),
        }
    }
}

/// Lightweight descriptor for a loaded program (used by protocol layer).
#[derive(Debug, Clone)]
pub struct ProgramInfo {
    pub guid: String,
    pub name: String,
    pub program_type: String,
}

/// Shared services state — accessible from both the protocol server and the player.
pub struct ServicesState {
    pub brightness: BrightnessService,
    pub screen_schedule: ScreenScheduleService,
    pub storage: StorageService,

    // Device identity
    pub device_name: String,

    // Display
    pub rotation: u16,
    /// Whether the screen is currently on (updated by player ScreenPower command).
    pub screen_on: bool,

    // Audio
    pub volume: u8,

    // Admin
    pub admin_mode: bool,

    // License
    pub license: String,

    // Time
    pub ntp_server: String,
    /// UTC offset in whole hours (-12 … +14).  0 = UTC (default).
    pub timezone_offset: i8,

    // Currently loaded programs
    pub programs: Vec<ProgramInfo>,
    pub current_program_guid: String,

    // FPGA hardware config XML (stub until real FPGA driver is implemented)
    pub fpga_config: String,

    /// Boot logo filename (relative to program_dir). None = no boot logo.
    pub boot_logo: Option<String>,

    /// Latest rendered frame as PNG bytes (updated by the player render loop).
    /// Use Arc<Mutex<…>> so it can be written from the async render loop and read
    /// by the async command handler without nesting locks inside ServicesState.
    pub screenshot: Arc<Mutex<Vec<u8>>>,

    // Cloud OMS
    pub cloud_url: String,
    pub device_id: String,

    /// Live data source values, keyed by name.
    /// Updated by SetDataSourceInfo; referenced by text renderer as {DS:name}.
    pub data_sources: HashMap<String, String>,

    /// Status of the most recent firmware upgrade.
    /// Set to InProgress when FirmwareUpgrade is triggered; updated to
    /// Success/Failed after the staged upgrade is applied on restart.
    pub upgrade_status: UpgradeStatus,

    /// Cancellation token for the main runtime.  The FirmwareUpgrade handler
    /// cancels this to trigger a graceful restart after staging the upgrade.
    pub shutdown_token: Option<CancellationToken>,

    // Network — WiFi (persisted config)
    pub wifi_ssid: String,
    pub wifi_password: String,
    pub wifi_enable: bool,

    // Network — Ethernet (persisted config)
    pub eth0_dhcp: bool,
    pub eth0_ip: String,
    pub eth0_mask: String,
    pub eth0_gateway: String,
    pub eth0_dns: String,

    // Network — PPPoE (persisted config)
    pub pppoe_enable: bool,
    pub pppoe_user: String,
    pub pppoe_password: String,

    /// SHA-256 hex hash of the admin unlock password.
    pub admin_password_hash: String,

    // Hardware relay pins
    pub relay_pins: Vec<u32>,
    // GPS
    pub gps_device: String,
    pub gps_reading: crate::services::gps::GpsReading,
    // Modem
    pub modem_device: String,
    pub modem_apn: String,
    pub modem_user: String,
    pub modem_password: String,
    // Modbus polling sources
    pub modbus_sources: Vec<ModbusSourceConfig>,
}

impl ServicesState {
    pub fn new(program_dir: PathBuf) -> Self {
        Self {
            brightness: BrightnessService::new(),
            screen_schedule: ScreenScheduleService::new(),
            storage: StorageService::new(program_dir),
            device_name: "huidu-player".to_string(),
            rotation: 0,
            screen_on: true,
            volume: 80,
            admin_mode: false,
            license: String::new(),
            ntp_server: "pool.ntp.org".to_string(),
            timezone_offset: 0,
            programs: Vec::new(),
            current_program_guid: String::new(),
            fpga_config: default_fpga_config(),
            boot_logo: None,
            screenshot: Arc::new(Mutex::new(Vec::new())),
            cloud_url: String::new(),
            device_id: String::new(),
            data_sources: HashMap::new(),
            upgrade_status: UpgradeStatus::Idle,
            shutdown_token: None,
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            wifi_enable: false,
            eth0_dhcp: true,
            eth0_ip: String::new(),
            eth0_mask: "255.255.255.0".to_string(),
            eth0_gateway: String::new(),
            eth0_dns: "8.8.8.8".to_string(),
            pppoe_enable: false,
            pppoe_user: String::new(),
            pppoe_password: String::new(),
            admin_password_hash: String::new(),
            relay_pins: Vec::new(),
            gps_device: String::new(),
            gps_reading: crate::services::gps::GpsReading::default(),
            modem_device: String::new(),
            modem_apn: String::new(),
            modem_user: String::new(),
            modem_password: String::new(),
            modbus_sources: Vec::new(),
        }
    }

    /// Load persisted device state from disk and apply it.
    pub fn load_persisted(&mut self) {
        let path = self.storage.program_dir().join("device_state.json");
        if let Ok(data) = std::fs::read_to_string(&path)
            && let Ok(s) = serde_json::from_str::<DeviceState>(&data) {
                self.device_name = s.device_name;
                self.brightness.set_level(s.brightness);
                self.volume = s.volume;
                self.rotation = s.rotation;
                self.admin_mode = s.admin_mode;
                self.ntp_server = s.ntp_server;
                self.timezone_offset = s.timezone_offset;
                if !s.license.is_empty() {
                    self.license = s.license;
                }
                if !s.brightness_schedule.is_empty() {
                    self.brightness.set_schedule(s.brightness_schedule);
                }
                if !s.screen_schedule.is_empty() {
                    self.screen_schedule.set_schedule(s.screen_schedule);
                }
                self.boot_logo = s.boot_logo;
                if !s.data_sources.is_empty() {
                    self.data_sources = s.data_sources;
                }
                self.upgrade_status = s.upgrade_status;
                self.wifi_ssid = s.wifi_ssid;
                self.wifi_password = s.wifi_password;
                self.wifi_enable = s.wifi_enable;
                self.eth0_dhcp = s.eth0_dhcp;
                self.eth0_ip = s.eth0_ip;
                self.eth0_mask = s.eth0_mask;
                self.eth0_gateway = s.eth0_gateway;
                self.eth0_dns = s.eth0_dns;
                self.pppoe_enable = s.pppoe_enable;
                self.pppoe_user = s.pppoe_user;
                self.pppoe_password = s.pppoe_password;
                self.admin_password_hash = s.admin_password_hash;
                self.relay_pins = s.relay_pins;
                self.gps_device = s.gps_device;
                self.modem_device = s.modem_device;
                self.modem_apn = s.modem_apn;
                self.modem_user = s.modem_user;
                self.modem_password = s.modem_password;
                self.modbus_sources = s.modbus_sources;
                info!("Restored device state from {}", path.display());
            }
    }

    /// Save current device state to disk.
    pub fn save_persisted(&self) {
        let path = self.storage.program_dir().join("device_state.json");
        let s = DeviceState {
            device_name: self.device_name.clone(),
            brightness: self.brightness.get_level(),
            volume: self.volume,
            rotation: self.rotation,
            admin_mode: self.admin_mode,
            ntp_server: self.ntp_server.clone(),
            timezone_offset: self.timezone_offset,
            license: self.license.clone(),
            brightness_schedule: self.brightness.get_schedule().to_vec(),
            screen_schedule: self.screen_schedule.get_schedule().to_vec(),
            boot_logo: self.boot_logo.clone(),
            data_sources: self.data_sources.clone(),
            upgrade_status: self.upgrade_status.clone(),
            wifi_ssid: self.wifi_ssid.clone(),
            wifi_password: self.wifi_password.clone(),
            wifi_enable: self.wifi_enable,
            eth0_dhcp: self.eth0_dhcp,
            eth0_ip: self.eth0_ip.clone(),
            eth0_mask: self.eth0_mask.clone(),
            eth0_gateway: self.eth0_gateway.clone(),
            eth0_dns: self.eth0_dns.clone(),
            pppoe_enable: self.pppoe_enable,
            pppoe_user: self.pppoe_user.clone(),
            pppoe_password: self.pppoe_password.clone(),
            admin_password_hash: self.admin_password_hash.clone(),
            relay_pins: self.relay_pins.clone(),
            gps_device: self.gps_device.clone(),
            modem_device: self.modem_device.clone(),
            modem_apn: self.modem_apn.clone(),
            modem_user: self.modem_user.clone(),
            modem_password: self.modem_password.clone(),
            modbus_sources: self.modbus_sources.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&s)
            && let Err(e) = std::fs::write(&path, json) {
                tracing::warn!("Failed to save device state: {}", e);
            }
    }
}

/// Minimal stub BoxHwConfig XML returned by GetBoxHwConfig / GetSDKFPGAConfig.
fn default_fpga_config() -> String {
    r#"<BoxHwConfig><CardInfo><Card width="128" height="64"><ModuleType>0</ModuleType><CellWidth>32</CellWidth><CellHight>16</CellHight><CellScanRow>8</CellScanRow><GrayLevel>16</GrayLevel><RefreshRate>60</RefreshRate><Brightness>100</Brightness></Card></CardInfo></BoxHwConfig>"#.to_string()
}

/// Start all background services.
///
/// All spawned tasks hold a child token cloned from `cancel`.  Calling
/// `cancel.cancel()` (e.g. on Ctrl-C / SIGTERM) stops every service cleanly.
pub async fn start_services(
    state: Arc<RwLock<ServicesState>>,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: PathBuf,
    cancel: CancellationToken,
) {
    info!("Starting background services");

    // Screen schedule checker (runs every 30 seconds)
    let tx = player_tx.clone();
    let state_clone = state.clone();
    let tok = cancel.clone();
    tokio::spawn(async move {
        ScreenScheduleService::run(state_clone, tx, tok).await;
    });

    // NTP time sync (runs every 6 hours) — use configured NTP server
    let ntp_server = {
        let s = state.read().await;
        s.ntp_server.clone()
    };
    let tok = cancel.clone();
    tokio::spawn(async move {
        TimeSyncService::run(ntp_server, tok).await;
    });

    // USB disk watcher
    let tx = player_tx.clone();
    let dir = program_dir.clone();
    let tok = cancel.clone();
    tokio::spawn(async move {
        UsbDiskService::run(tx, dir, tok).await;
    });

    // Cloud OMS heartbeat
    let (cloud_url, device_id) = {
        let s = state.read().await;
        (s.cloud_url.clone(), s.device_id.clone())
    };
    if !cloud_url.is_empty() {
        let tok = cancel.clone();
        crate::services::cloud_api::CloudApiService::start(state.clone(), cloud_url, device_id, tok);
    }

    // Modbus polling service
    let tok = cancel.clone();
    let state2 = state.clone();
    tokio::spawn(async move {
        crate::services::modbus_service::ModbusService::run(state2, tok).await;
    });

    // GPS service (no-op on non-Unix)
    let gps_device = {
        let s = state.read().await;
        s.gps_device.clone()
    };
    let tok = cancel.clone();
    let state3 = state.clone();
    tokio::spawn(async move {
        crate::services::gps::run(gps_device, state3, tok).await;
    });
}
