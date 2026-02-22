/// Services manager — coordinates background services.
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, RwLock};
use tracing::info;

use crate::core::player::PlayerCommand;
use crate::services::brightness::BrightnessService;
use crate::services::screen_schedule::ScreenScheduleService;
use crate::services::storage::StorageService;
use crate::services::time_sync::TimeSyncService;
use crate::services::usb_disk::UsbDiskService;

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

    // Audio
    pub volume: u8,

    // Admin
    pub admin_mode: bool,

    // License
    pub license: String,

    // Time
    pub ntp_server: String,

    // Currently loaded programs
    pub programs: Vec<ProgramInfo>,
    pub current_program_guid: String,

    // FPGA hardware config XML (stub until real FPGA driver is implemented)
    pub fpga_config: String,

    /// Latest rendered frame as PNG bytes (updated by the player render loop).
    /// Use Arc<Mutex<…>> so it can be written from the async render loop and read
    /// by the async command handler without nesting locks inside ServicesState.
    pub screenshot: Arc<Mutex<Vec<u8>>>,

    // Cloud OMS
    pub cloud_url: String,
    pub device_id: String,
}

impl ServicesState {
    pub fn new(program_dir: PathBuf) -> Self {
        Self {
            brightness: BrightnessService::new(),
            screen_schedule: ScreenScheduleService::new(),
            storage: StorageService::new(program_dir),
            device_name: "huidu-player".to_string(),
            rotation: 0,
            volume: 80,
            admin_mode: false,
            license: String::new(),
            ntp_server: "pool.ntp.org".to_string(),
            programs: Vec::new(),
            current_program_guid: String::new(),
            fpga_config: default_fpga_config(),
            screenshot: Arc::new(Mutex::new(Vec::new())),
            cloud_url: String::new(),
            device_id: String::new(),
        }
    }
}

/// Minimal stub BoxHwConfig XML returned by GetBoxHwConfig / GetSDKFPGAConfig.
fn default_fpga_config() -> String {
    r#"<BoxHwConfig><CardInfo><Card width="128" height="64"><ModuleType>0</ModuleType><CellWidth>32</CellWidth><CellHight>16</CellHight><CellScanRow>8</CellScanRow><GrayLevel>16</GrayLevel><RefreshRate>60</RefreshRate><Brightness>100</Brightness></Card></CardInfo></BoxHwConfig>"#.to_string()
}

/// Start all background services
pub async fn start_services(
    state: Arc<RwLock<ServicesState>>,
    player_tx: mpsc::Sender<PlayerCommand>,
    program_dir: PathBuf,
) {
    info!("Starting background services");

    // Screen schedule checker (runs every minute)
    let tx = player_tx.clone();
    let state_clone = state.clone();
    tokio::spawn(async move {
        ScreenScheduleService::run(state_clone, tx).await;
    });

    // NTP time sync (runs every 6 hours)
    tokio::spawn(async move {
        TimeSyncService::run().await;
    });

    // USB disk watcher
    let tx = player_tx.clone();
    let dir = program_dir.clone();
    tokio::spawn(async move {
        UsbDiskService::run(tx, dir).await;
    });

    // Cloud OMS heartbeat
    let (cloud_url, device_id) = {
        let s = state.read().await;
        (s.cloud_url.clone(), s.device_id.clone())
    };
    if !cloud_url.is_empty() {
        crate::services::cloud_api::CloudApiService::start(state, cloud_url, device_id);
    }
}
