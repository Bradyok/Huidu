/// Services manager — coordinates background services.
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::info;

use crate::core::player::PlayerCommand;
use crate::services::brightness::BrightnessService;
use crate::services::screen_schedule::ScreenScheduleService;
use crate::services::storage::StorageService;
use crate::services::time_sync::TimeSyncService;
use crate::services::usb_disk::UsbDiskService;

/// Shared services state
pub struct ServicesState {
    pub brightness: BrightnessService,
    pub screen_schedule: ScreenScheduleService,
    pub storage: StorageService,
}

impl ServicesState {
    pub fn new(program_dir: PathBuf) -> Self {
        Self {
            brightness: BrightnessService::new(),
            screen_schedule: ScreenScheduleService::new(),
            storage: StorageService::new(program_dir),
        }
    }
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
}
