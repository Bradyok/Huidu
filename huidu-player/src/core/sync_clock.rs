/// Multi-device synchronization clock.
/// Master broadcasts frame numbers via UDP multicast; slaves discipline
/// their local frame counter to match.

use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

/// Shared sync clock state accessible from the render loop.
pub struct SyncClock {
    /// Local frame counter
    local_frame: AtomicU64,
    /// Adjustment offset: `adjusted_frame = local_frame + offset`
    offset: AtomicI64,
    /// Current program GUID as 36-byte ASCII (for mismatch detection)
    program_guid: std::sync::Mutex<String>,
}

impl SyncClock {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            local_frame: AtomicU64::new(0),
            offset: AtomicI64::new(0),
            program_guid: std::sync::Mutex::new(String::new()),
        })
    }

    /// Return the current local frame number.
    pub fn current_frame(&self) -> u64 {
        self.local_frame.load(Ordering::Relaxed)
    }

    /// Increment local frame, return adjusted frame number.
    pub fn tick(&self) -> u64 {
        let frame = self.local_frame.fetch_add(1, Ordering::Relaxed);
        let offset = self.offset.load(Ordering::Relaxed);
        (frame as i64 + offset).max(0) as u64
    }

    /// Apply a received master sync packet.
    /// `master_frame` is the master's current frame number.
    pub fn apply_sync(&self, master_frame: u64) {
        let local = self.local_frame.load(Ordering::Relaxed);
        let diff = master_frame as i64 - local as i64;
        if diff.abs() > 2 {
            // Snap immediately
            self.offset.store(diff, Ordering::Relaxed);
        } else if diff != 0 {
            // Nudge by 1 toward master
            let current = self.offset.load(Ordering::Relaxed);
            self.offset.store(current + diff.signum(), Ordering::Relaxed);
        }
    }

    pub fn set_program_guid(&self, guid: &str) {
        let mut g = self.program_guid.lock().unwrap_or_else(|e| e.into_inner());
        *g = guid.to_string();
    }

    pub fn get_program_guid(&self) -> String {
        self.program_guid.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
