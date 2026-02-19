/// Screen on/off scheduling service.
/// Turns the screen on/off based on configured time ranges.
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use tracing::debug;

use crate::core::player::PlayerCommand;
use crate::services::manager::ServicesState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenScheduleEntry {
    /// Time to turn on (HH:MM:SS)
    pub on_time: String,
    /// Time to turn off (HH:MM:SS)
    pub off_time: String,
    /// Days of week enabled (comma-separated: "Mon,Tue,Wed,Thu,Fri,Sat,Sun")
    pub days: String,
}

pub struct ScreenScheduleService {
    entries: Vec<ScreenScheduleEntry>,
    last_state: Option<bool>,
}

impl ScreenScheduleService {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            last_state: None,
        }
    }

    pub fn set_schedule(&mut self, entries: Vec<ScreenScheduleEntry>) {
        self.entries = entries;
        self.last_state = None;
        tracing::info!("Screen schedule updated: {} entries", self.entries.len());
    }

    pub fn get_schedule(&self) -> &[ScreenScheduleEntry] {
        &self.entries
    }

    /// Check if screen should be on right now
    pub fn should_be_on(&self) -> Option<bool> {
        if self.entries.is_empty() {
            return None; // No schedule, don't override
        }

        let now = Local::now();
        let current_time = now.format("%H:%M:%S").to_string();
        let day_name = now.format("%a").to_string(); // Mon, Tue, etc.

        for entry in &self.entries {
            // Check if today is in the enabled days
            if !entry.days.is_empty() && !entry.days.contains(&day_name) {
                continue;
            }

            // Check if current time is within on/off range
            if current_time >= entry.on_time && current_time < entry.off_time {
                return Some(true);
            }
        }

        Some(false)
    }

    /// Background task that checks schedule every 30 seconds
    pub async fn run(
        state: Arc<RwLock<ServicesState>>,
        player_tx: mpsc::Sender<PlayerCommand>,
    ) {
        let mut interval = time::interval(Duration::from_secs(30));

        loop {
            interval.tick().await;

            let should_on = {
                let state = state.read().await;
                state.screen_schedule.should_be_on()
            };

            if let Some(on) = should_on {
                let mut state = state.write().await;
                if state.screen_schedule.last_state != Some(on) {
                    state.screen_schedule.last_state = Some(on);
                    debug!("Screen schedule: turning {}", if on { "ON" } else { "OFF" });
                    let _ = player_tx.send(PlayerCommand::ScreenPower(on)).await;
                }
            }

            // Also check brightness schedule
            {
                let mut state = state.write().await;
                state.brightness.check_schedule();
            }
        }
    }
}
