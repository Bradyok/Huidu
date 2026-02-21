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
use tracing::{debug, warn};

use crate::services::manager::ServicesState;

pub struct CloudApiService;

impl CloudApiService {
    /// Spawn the cloud heartbeat loop.  Returns immediately; all network I/O
    /// runs in the background Tokio task spawned inside this function.
    pub fn start(state: Arc<RwLock<ServicesState>>, cloud_url: String, device_id: String) {
        tokio::spawn(async move {
            Self::run(state, cloud_url, device_id).await;
        });
    }

    async fn run(state: Arc<RwLock<ServicesState>>, cloud_url: String, device_id: String) {
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

        // Initial registration
        register(&client, &cloud_url, &device_id, &state).await;

        let mut heartbeat_tick = time::interval(Duration::from_secs(60));
        let mut report_tick = time::interval(Duration::from_secs(300));

        // Burn the first (immediate) tick so the loops don't double-fire at t=0
        heartbeat_tick.tick().await;
        report_tick.tick().await;

        loop {
            tokio::select! {
                _ = heartbeat_tick.tick() => {
                    send_heartbeat(&client, &cloud_url, &device_id).await;
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

async fn post_json(client: &reqwest::Client, url: &str, body: serde_json::Value) {
    debug!("Cloud → POST {}", url);
    match client.post(url).json(&body).send().await {
        Ok(resp) => debug!("Cloud ← {} {}", url, resp.status()),
        Err(e) => warn!("Cloud POST {} failed: {}", url, e),
    }
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
    post_json(client, &format!("{}/api/DeviceApi/Register", base), body).await;
}

async fn send_heartbeat(client: &reqwest::Client, base: &str, device_id: &str) {
    let body = serde_json::json!({
        "DeviceId":  device_id,
        "Timestamp": chrono::Utc::now().to_rfc3339(),
    });
    post_json(client, &format!("{}/api/DeviceApi/Heartbeat", base), body).await;
}

async fn report_all_info(
    client: &reqwest::Client,
    base: &str,
    device_id: &str,
    state: &Arc<RwLock<ServicesState>>,
) {
    let (programs, current_guid, brightness, rotation) = {
        let s = state.read().await;
        (
            s.programs.clone(),
            s.current_program_guid.clone(),
            s.brightness.get_level(),
            s.rotation,
        )
    };
    let programs_json: Vec<_> = programs
        .iter()
        .map(|p| {
            serde_json::json!({
                "Guid": p.guid,
                "Name": p.name,
            })
        })
        .collect();
    let body = serde_json::json!({
        "DeviceId":           device_id,
        "CurrentProgramGuid": current_guid,
        "Brightness":         brightness,
        "Rotation":           rotation,
        "Programs":           programs_json,
        "Timestamp":          chrono::Utc::now().to_rfc3339(),
    });
    post_json(client, &format!("{}/api/DeviceApi/ReportAllInfo", base), body).await;
}
