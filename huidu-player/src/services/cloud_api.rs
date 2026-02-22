/// Cloud OMS (Online Management System) heartbeat service.
///
/// Mirrors the cloud communication found in libclound_service.so (PX30 firmware):
///   1. Register the device on startup
///   2. POST a heartbeat every 60 seconds  → /api/DeviceApi/Heartbeat
///   3. POST a full report every 5 minutes → /api/DeviceApi/ReportAllInfo
///
/// If `cloud_url` is empty the service exits immediately (disabled).
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::RwLock;
use tokio::time;
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::services::manager::ServicesState;

/// Maximum number of POST retries on transient network failures.
const MAX_RETRIES: u32 = 3;
/// Delay between retries.
const RETRY_DELAY: Duration = Duration::from_secs(5);

pub struct CloudApiService;

impl CloudApiService {
    /// Spawn the cloud heartbeat loop.  Returns immediately; all network I/O
    /// runs in the background Tokio task spawned inside this function.
    /// The task exits when `cancel` is triggered.
    pub fn start(
        state: Arc<RwLock<ServicesState>>,
        cloud_url: String,
        device_id: String,
        cancel: CancellationToken,
    ) {
        tokio::spawn(async move {
            Self::run(state, cloud_url, device_id, cancel).await;
        });
    }

    async fn run(
        state: Arc<RwLock<ServicesState>>,
        cloud_url: String,
        device_id: String,
        cancel: CancellationToken,
    ) {
        if cloud_url.is_empty() {
            debug!("Cloud API disabled (no URL configured)");
            return;
        }

        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
        {
            Ok(c) => c,
            Err(e) => {
                warn!("Cloud API: failed to build HTTP client: {}", e);
                return;
            }
        };

        info!("Cloud API connected to {cloud_url}");

        // Initial registration
        register(&client, &cloud_url, &device_id, &state).await;

        let mut heartbeat_tick = time::interval(Duration::from_secs(60));
        let mut report_tick    = time::interval(Duration::from_secs(300));

        // Burn the first (immediate) tick so the loops don't double-fire at t=0
        heartbeat_tick.tick().await;
        report_tick.tick().await;

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                _ = heartbeat_tick.tick() => {
                    send_heartbeat(&client, &cloud_url, &device_id, &state).await;
                }
                _ = report_tick.tick() => {
                    report_all_info(&client, &cloud_url, &device_id, &state).await;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// HTTP helpers
// ---------------------------------------------------------------------------

/// POST JSON with up to MAX_RETRIES attempts on transient failure.
async fn post_json(client: &reqwest::Client, url: &str, body: serde_json::Value) {
    let mut last_err = String::new();
    for attempt in 1..=MAX_RETRIES {
        debug!("Cloud → POST {} (attempt {attempt}/{MAX_RETRIES})", url);
        match client.post(url).json(&body).send().await {
            Ok(resp) => {
                debug!("Cloud ← {} {}", url, resp.status());
                return; // success
            }
            Err(e) => {
                last_err = e.to_string();
                if attempt < MAX_RETRIES {
                    tokio::time::sleep(RETRY_DELAY).await;
                }
            }
        }
    }
    warn!("Cloud POST {} failed after {MAX_RETRIES} attempts: {}", url, last_err);
}

// ---------------------------------------------------------------------------
// Payloads
// ---------------------------------------------------------------------------

async fn register(
    client: &reqwest::Client,
    base: &str,
    device_id: &str,
    state: &Arc<RwLock<ServicesState>>,
) {
    let (name, license) = {
        let s = state.read().await;
        (s.device_name.clone(), s.license.clone())
    };
    let body = serde_json::json!({
        "DeviceId":   device_id,
        "DeviceName": name,
        "License":    license,
        "Version":    env!("CARGO_PKG_VERSION"),
        "Os":         std::env::consts::OS,
    });
    post_json(client, &format!("{base}/api/DeviceApi/Register"), body).await;
}

async fn send_heartbeat(
    client: &reqwest::Client,
    base: &str,
    device_id: &str,
    state: &Arc<RwLock<ServicesState>>,
) {
    let (screen_on, brightness) = {
        let s = state.read().await;
        (s.screen_on, s.brightness.get_level())
    };
    let body = serde_json::json!({
        "DeviceId":  device_id,
        "ScreenOn":  screen_on,
        "Brightness": brightness,
        "Timestamp": chrono::Utc::now().to_rfc3339(),
    });
    post_json(client, &format!("{base}/api/DeviceApi/Heartbeat"), body).await;
}

async fn report_all_info(
    client: &reqwest::Client,
    base: &str,
    device_id: &str,
    state: &Arc<RwLock<ServicesState>>,
) {
    let snap = {
        let s = state.read().await;
        (
            s.device_name.clone(),
            s.programs.clone(),
            s.current_program_guid.clone(),
            s.brightness.get_level(),
            s.rotation,
            s.volume,
            s.screen_on,
            s.data_sources.clone(),
        )
    };
    let (device_name, programs, current_guid, brightness, rotation, volume, screen_on, data_sources) = snap;

    let programs_json: Vec<_> = programs
        .iter()
        .map(|p| serde_json::json!({ "Guid": p.guid, "Name": p.name }))
        .collect();

    // Convert data_sources HashMap to JSON object
    let ds_json: serde_json::Map<String, serde_json::Value> = data_sources
        .into_iter()
        .map(|(k, v)| (k, serde_json::Value::String(v)))
        .collect();

    let body = serde_json::json!({
        "DeviceId":           device_id,
        "DeviceName":         device_name,
        "CurrentProgramGuid": current_guid,
        "Brightness":         brightness,
        "Rotation":           rotation,
        "Volume":             volume,
        "ScreenOn":           screen_on,
        "Programs":           programs_json,
        "DataSources":        serde_json::Value::Object(ds_json),
        "Timestamp":          chrono::Utc::now().to_rfc3339(),
    });
    post_json(client, &format!("{base}/api/DeviceApi/ReportAllInfo"), body).await;
}
