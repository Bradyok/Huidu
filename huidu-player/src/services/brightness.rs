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

        // Find the most recent schedule entry (highest time ≤ current time)
        let mut best: Option<&BrightnessScheduleEntry> = None;
        let mut best_mins: u16 = 0;
        for entry in &self.schedule {
            let entry_minutes = entry.hour as u16 * 60 + entry.minute as u16;
            if entry_minutes <= current_minutes {
                if best.is_none() || entry_minutes >= best_mins {
                    best = Some(entry);
                    best_mins = entry_minutes;
                }
            }
        }

        // If no entry found before current time, use the last one (wrap around)
        if best.is_none() {
            best = self.schedule.last();
        }

        if let Some(entry) = best
            && entry.level != self.current_level {
                self.current_level = entry.level;
                tracing::debug!("Brightness auto-adjusted to {}", self.current_level);
            }
    }

    /// Apply brightness as a multiplier to pixel data (software brightness).
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(hour: u8, minute: u8, level: u8) -> BrightnessScheduleEntry {
        BrightnessScheduleEntry { hour, minute, level }
    }

    /// Inject a fake "current time" as minutes for testing check_schedule logic.
    /// We test the selection algorithm directly rather than calling check_schedule
    /// (which reads the real clock).
    fn best_entry_for(entries: &[BrightnessScheduleEntry], current_minutes: u16) -> Option<u8> {
        let mut best: Option<&BrightnessScheduleEntry> = None;
        let mut best_mins: u16 = 0;
        for entry in entries {
            let em = entry.hour as u16 * 60 + entry.minute as u16;
            if em <= current_minutes {
                if best.is_none() || em >= best_mins {
                    best = Some(entry);
                    best_mins = em;
                }
            }
        }
        if best.is_none() {
            best = entries.last();
        }
        best.map(|e| e.level)
    }

    #[test]
    fn best_entry_picks_highest_time_le_current() {
        // Entries in unsorted order: 06:00→20%, 08:00→50%, 18:00→80%
        let entries = vec![
            make_entry(18, 0, 80),
            make_entry(6,  0, 20),
            make_entry(8,  0, 50),
        ];
        // At 09:00 (540 min) → best is 08:00 (480 min) → level 50
        assert_eq!(best_entry_for(&entries, 9 * 60), Some(50));
        // At 18:00 (1080 min) → best is 18:00 → level 80
        assert_eq!(best_entry_for(&entries, 18 * 60), Some(80));
        // At 19:00 (1140 min) → best is still 18:00 → level 80
        assert_eq!(best_entry_for(&entries, 19 * 60), Some(80));
    }

    #[test]
    fn best_entry_wraps_around_before_first_entry() {
        // Only one entry at 08:00; querying at 05:00 wraps to last entry
        let entries = vec![make_entry(8, 0, 70)];
        assert_eq!(best_entry_for(&entries, 5 * 60), Some(70));
    }

    #[test]
    fn apply_to_pixels_full_brightness_is_noop() {
        let svc = BrightnessService::new(); // level = 100
        let mut data = vec![200u8, 100, 50, 255];
        svc.apply_to_pixels(&mut data);
        assert_eq!(data, [200, 100, 50, 255]);
    }

    #[test]
    fn apply_to_pixels_half_brightness() {
        let mut svc = BrightnessService::new();
        svc.set_level(50);
        let mut data = vec![200u8, 100, 50, 255];
        svc.apply_to_pixels(&mut data);
        assert_eq!(data[0], 100);
        assert_eq!(data[1], 50);
        assert_eq!(data[2], 25);
        assert_eq!(data[3], 255); // alpha unchanged
    }
}
