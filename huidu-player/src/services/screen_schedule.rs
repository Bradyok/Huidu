/// Screen on/off scheduling service.
/// Turns the screen on/off based on configured time ranges.
use chrono::{Datelike, Local, Timelike};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::debug;

use crate::core::player::PlayerCommand;
use crate::services::manager::ServicesState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenScheduleEntry {
    /// Time to turn on (HH:MM or HH:MM:SS)
    pub on_time: String,
    /// Time to turn off (HH:MM or HH:MM:SS)
    pub off_time: String,
    /// Days of week enabled.
    /// Bitmask format: "1111111" — index 0=Mon .. 6=Sun; '1' means enabled.
    /// Legacy format:  "Mon,Tue,Wed" — comma-separated three-letter names.
    /// Empty string means every day.
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

    /// Returns whether `days` enables the given weekday.
    ///
    /// - Bitmask `"1111111"`: index 0=Mon..6=Sun, '1' = enabled.
    /// - Comma-separated names `"Mon,Tue,..."`: three-letter English abbreviation.
    /// - Empty string: all days enabled.
    pub fn day_enabled(days: &str, weekday_idx: usize, day_name: &str) -> bool {
        if days.is_empty() {
            return true;
        }
        if days.len() == 7 && days.chars().all(|c| c == '0' || c == '1') {
            days.chars().nth(weekday_idx) == Some('1')
        } else {
            days.contains(day_name)
        }
    }

    /// Check if screen should be on right now.
    /// Returns `None` if no schedule is set (caller should not override screen state).
    pub fn should_be_on(&self) -> Option<bool> {
        if self.entries.is_empty() {
            return None;
        }

        let now = Local::now();
        let cur_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
        // chrono: Mon=0, Tue=1, ..., Sun=6
        let weekday_idx = now.weekday().num_days_from_monday() as usize;
        let day_name = now.format("%a").to_string(); // "Mon", "Tue", …

        for entry in &self.entries {
            if !Self::day_enabled(&entry.days, weekday_idx, &day_name) {
                continue;
            }
            let on_secs  = parse_hm_secs(&entry.on_time).unwrap_or(0);
            let off_secs = parse_hm_secs(&entry.off_time).unwrap_or(86400);

            let in_range = if on_secs <= off_secs {
                // Normal same-day window: e.g. 08:00–22:00
                cur_secs >= on_secs && cur_secs < off_secs
            } else {
                // Midnight-crossing window: e.g. 22:00–08:00
                // Screen is ON if current time is after on OR before off
                cur_secs >= on_secs || cur_secs < off_secs
            };

            if in_range {
                return Some(true);
            }
        }

        Some(false)
    }

    /// Background task that checks schedule every 30 seconds.
    /// Exits when `cancel` is triggered.
    pub async fn run(
        state: Arc<RwLock<ServicesState>>,
        player_tx: mpsc::Sender<PlayerCommand>,
        cancel: CancellationToken,
    ) {
        let mut interval = time::interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => break,
                _ = interval.tick() => {}
            }

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

            // Also check brightness schedule and forward any change to the player
            let brightness_changed = {
                let mut state = state.write().await;
                let old_level = state.brightness.get_level();
                state.brightness.check_schedule();
                let new_level = state.brightness.get_level();
                (new_level != old_level).then_some(new_level)
            };
            if let Some(level) = brightness_changed {
                debug!("Brightness schedule: adjusted to {}", level);
                let _ = player_tx.send(PlayerCommand::SetBrightness(level)).await;
            }
        }
    }
}

/// Parse "HH:MM" or "HH:MM:SS" into seconds-since-midnight.
fn parse_hm_secs(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    let h: u32 = parts.first()?.parse().ok()?;
    let m: u32 = parts.get(1)?.parse().ok()?;
    let sec: u32 = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(0);
    Some(h * 3600 + m * 60 + sec)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitmask_all_days_enabled() {
        // "1111111" = every day enabled
        for idx in 0..7usize {
            assert!(ScreenScheduleService::day_enabled("1111111", idx, "Mon"));
        }
    }

    #[test]
    fn bitmask_weekdays_only() {
        // "1111100" = Mon–Fri enabled, Sat(5) and Sun(6) off
        let days = "1111100";
        for idx in 0..5usize {
            assert!(ScreenScheduleService::day_enabled(days, idx, "Mon"),
                "idx {} should be enabled", idx);
        }
        assert!(!ScreenScheduleService::day_enabled(days, 5, "Sat"));
        assert!(!ScreenScheduleService::day_enabled(days, 6, "Sun"));
    }

    #[test]
    fn bitmask_empty_means_all() {
        assert!(ScreenScheduleService::day_enabled("", 3, "Thu"));
    }

    #[test]
    fn legacy_comma_separated() {
        let days = "Mon,Wed,Fri";
        assert!(ScreenScheduleService::day_enabled(days, 0, "Mon"));
        assert!(!ScreenScheduleService::day_enabled(days, 1, "Tue"));
        assert!(ScreenScheduleService::day_enabled(days, 2, "Wed"));
        assert!(!ScreenScheduleService::day_enabled(days, 3, "Thu"));
        assert!(ScreenScheduleService::day_enabled(days, 4, "Fri"));
    }

    #[test]
    fn should_be_on_returns_none_when_empty() {
        let svc = ScreenScheduleService::new();
        assert_eq!(svc.should_be_on(), None);
    }

    #[test]
    fn parse_hm_secs_hhmm() {
        assert_eq!(parse_hm_secs("08:30"), Some(8 * 3600 + 30 * 60));
    }

    #[test]
    fn parse_hm_secs_hhmmss() {
        assert_eq!(parse_hm_secs("22:00:00"), Some(22 * 3600));
    }

    #[test]
    fn parse_hm_secs_invalid() {
        assert_eq!(parse_hm_secs(""), None);
        assert_eq!(parse_hm_secs("bad"), None);
    }

    /// `should_be_on` uses numeric comparison so a midnight-crossing window
    /// (on=22:00, off=06:00) correctly matches both 23:00 and 02:00.
    #[test]
    fn midnight_crossing_window_logic() {
        // Simulate: on=22:00 (79200s), off=06:00 (21600s) → on_secs > off_secs
        let on_secs: u32 = 22 * 3600;
        let off_secs: u32 = 6 * 3600;
        assert!(on_secs > off_secs, "sanity: this is a crossing window");

        // Helper that mirrors the logic in should_be_on
        let in_range = |cur: u32| -> bool {
            if on_secs <= off_secs {
                cur >= on_secs && cur < off_secs
            } else {
                cur >= on_secs || cur < off_secs
            }
        };

        assert!(in_range(23 * 3600), "23:00 should be ON");
        assert!(in_range(0),         "00:00 should be ON");
        assert!(in_range(3 * 3600),  "03:00 should be ON");
        assert!(!in_range(6 * 3600), "06:00 should be OFF (boundary)");
        assert!(!in_range(12 * 3600),"12:00 should be OFF");
        assert!(!in_range(21 * 3600),"21:00 should be OFF");
    }
}
