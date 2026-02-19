/// Brightness control service.
/// Manages brightness level and scheduled brightness changes.
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrightnessScheduleEntry {
    pub hour: u8,
    pub minute: u8,
    pub level: u8, // 0-100
}

pub struct BrightnessService {
    /// Current brightness level (0-100)
    current_level: u8,
    /// Brightness schedule (time-of-day based)
    schedule: Vec<BrightnessScheduleEntry>,
}

impl BrightnessService {
    pub fn new() -> Self {
        Self {
            current_level: 100,
            schedule: Vec::new(),
        }
    }

    pub fn get_level(&self) -> u8 {
        self.current_level
    }

    pub fn set_level(&mut self, level: u8) {
        self.current_level = level.min(100);
        tracing::info!("Brightness set to {}", self.current_level);
    }

    pub fn set_schedule(&mut self, schedule: Vec<BrightnessScheduleEntry>) {
        self.schedule = schedule;
        tracing::info!("Brightness schedule updated: {} entries", self.schedule.len());
    }

    pub fn get_schedule(&self) -> &[BrightnessScheduleEntry] {
        &self.schedule
    }

    /// Check schedule and update brightness if needed
    pub fn check_schedule(&mut self) {
        if self.schedule.is_empty() {
            return;
        }

        let now = chrono::Local::now();
        let current_minutes = now.format("%H").to_string().parse::<u16>().unwrap_or(0) * 60
            + now.format("%M").to_string().parse::<u16>().unwrap_or(0);

        // Find the most recent schedule entry
        let mut best: Option<&BrightnessScheduleEntry> = None;
        for entry in &self.schedule {
            let entry_minutes = entry.hour as u16 * 60 + entry.minute as u16;
            if entry_minutes <= current_minutes {
                best = Some(entry);
            }
        }

        // If no entry found before current time, use the last one (wrap around)
        if best.is_none() {
            best = self.schedule.last();
        }

        if let Some(entry) = best {
            if entry.level != self.current_level {
                self.current_level = entry.level;
                tracing::debug!("Brightness auto-adjusted to {}", self.current_level);
            }
        }
    }

    /// Apply brightness as a multiplier to pixel data (software brightness)
    pub fn apply_to_pixels(&self, data: &mut [u8]) {
        if self.current_level >= 100 {
            return;
        }
        let factor = self.current_level as f32 / 100.0;
        for chunk in data.chunks_exact_mut(4) {
            chunk[0] = (chunk[0] as f32 * factor) as u8;
            chunk[1] = (chunk[1] as f32 * factor) as u8;
            chunk[2] = (chunk[2] as f32 * factor) as u8;
            // Alpha stays the same
        }
    }
}
