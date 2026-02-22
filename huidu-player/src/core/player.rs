/// Main player — orchestrates program loading, rendering, and output.
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info, warn};

use crate::audio::BackgroundMusicPlayer;
use crate::config::{OutputMode, PlayerConfig};
use crate::core::sync_clock::SyncClock;
use crate::fpga::driver::FpgaDriver;
use crate::program::model::{Program, Screen};
use crate::program::parser;
use crate::render::drm_output::DrmOutput;
use crate::render::engine::RenderEngine;
use crate::services::manager::{ProgramInfo, ServicesState};

/// Commands sent from the protocol server to the player
#[derive(Debug)]
pub enum PlayerCommand {
    /// Load a new screen (replaces all programs)
    LoadScreen(Screen),
    /// Set brightness (0-100)
    SetBrightness(u8),
    /// Turn screen on/off
    ScreenPower(bool),
    /// Set screen rotation (0 / 90 / 180 / 270 degrees)
    SetRotation(u16),
    /// Switch active program by GUID
    SwitchProgram(String),
    /// Delete a program by GUID
    DeleteProgram(String),
    /// Set audio volume (0-100)
    SetVolume(u8),
    /// Insert programs at high priority — plays once then resumes the normal playlist.
    InsertProgram(Vec<Program>),
    /// Show a SMPTE color-bar test pattern for `seconds` seconds (0 = cancel).
    ScreenTest(u32),
    /// Soft-reload: update program list without resetting effect state for unchanged items.
    SoftLoadScreen(Screen),
    /// Apply FPGA hardware config from BoxHwConfig XML string.
    ApplyFpgaConfig(String),
}

pub struct Player {
    config: PlayerConfig,
    engine: RenderEngine,
    /// FPGA hardware driver (no-op if FPGA unavailable or non-Linux build)
    fpga: FpgaDriver,
    programs: Vec<Program>,
    current_program: usize,
    /// Time (in frames) when current program started
    program_start_frame: u64,
    command_rx: mpsc::Receiver<PlayerCommand>,
    command_tx: mpsc::Sender<PlayerCommand>,
    screen_on: bool,
    /// Shared services state
    services: Arc<RwLock<ServicesState>>,
    /// Background music player
    music: BackgroundMusicPlayer,
    /// Programs temporarily inserted at high priority.
    /// While non-empty these play before the regular `programs` list.
    priority_programs: Vec<Program>,
    /// Which priority program is currently active.
    priority_current: usize,
    /// Frame at which the current priority program started.
    priority_start_frame: u64,
    /// `current_program` index saved when priority started.
    priority_return_idx: usize,
    /// `program_start_frame` saved when priority started.
    priority_return_start_frame: u64,
    /// DRM output (Unix only)
    drm_output: Option<DrmOutput>,
    /// Sync clock for multi-device synchronization
    sync_clock: Option<Arc<SyncClock>>,
}

impl Player {
    pub fn new(config: PlayerConfig) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let engine = RenderEngine::new(
            config.width,
            config.height,
            config.fps,
            config.weather_url.clone(),
        );
        let services = Arc::new(RwLock::new(ServicesState::new(config.program_dir.clone())));

        Self {
            config,
            engine,
            fpga: FpgaDriver::open(),
            programs: Vec::new(),
            current_program: 0,
            program_start_frame: 0,
            command_rx: rx,
            command_tx: tx,
            screen_on: true,
            services,
            music: BackgroundMusicPlayer::new(),
            priority_programs: Vec::new(),
            priority_current: 0,
            priority_start_frame: 0,
            priority_return_idx: 0,
            priority_return_start_frame: 0,
            drm_output: None,
            sync_clock: None,
        }
    }

    pub fn program_sender(&self) -> mpsc::Sender<PlayerCommand> {
        self.command_tx.clone()
    }

    pub fn services(&self) -> Arc<RwLock<ServicesState>> {
        self.services.clone()
    }

    pub fn setup_drm(&mut self, device: &str) {
        match DrmOutput::open(device, self.config.width, self.config.height) {
            Ok(drm) => {
                self.drm_output = Some(drm);
                info!("DRM output initialized: {}", device);
            }
            Err(e) => {
                tracing::warn!("DRM output unavailable: {}", e);
            }
        }
    }

    pub fn set_sync_clock(&mut self, clock: Arc<SyncClock>) {
        self.sync_clock = Some(clock);
    }

    /// Load programs from a directory
    pub fn load_programs_from_dir(&mut self, dir: &str) -> Result<()> {
        let path = Path::new(dir);
        if !path.exists() {
            anyhow::bail!("Program directory does not exist: {}", dir);
        }

        let mut loaded = 0;
        for entry in std::fs::read_dir(path)? {
            let entry = entry?;
            let file_path = entry.path();
            if file_path.extension().is_some_and(|e| e == "xml") {
                match parser::parse_program_file(&file_path) {
                    Ok(screen) => {
                        info!(
                            "Loaded {} program(s) from {}",
                            screen.programs.len(),
                            file_path.display()
                        );
                        self.programs.extend(screen.programs);
                        loaded += 1;
                    }
                    Err(e) => {
                        warn!("Failed to parse {}: {}", file_path.display(), e);
                    }
                }
            }
        }

        if loaded == 0 {
            anyhow::bail!("No program XML files found in {}", dir);
        }

        // Initialize rendering for first program
        if !self.programs.is_empty() {
            self.engine.reset_for_program(&self.programs[0], 0);
            self.sync_programs_to_services(0);
            self.sync_music(0);
        }

        info!("Loaded {} total programs from {}", self.programs.len(), dir);
        Ok(())
    }

    /// Main render loop.  Exits when `cancel` is triggered (Ctrl-C / SIGTERM).
    pub async fn run(&mut self, cancel: CancellationToken) -> Result<()> {
        let frame_duration = Duration::from_millis(1000 / self.config.fps as u64);
        let mut interval = time::interval(frame_duration);
        let mut frames_rendered: u64 = 0;

        info!(
            "Starting render loop: {}x{} @ {}fps, output: {:?}",
            self.config.width, self.config.height, self.config.fps, self.config.output_mode
        );

        if self.programs.is_empty() {
            info!("No programs loaded, waiting for program from network...");
        }

        loop {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => {
                    info!("Player render loop stopping (cancelled)");
                    return Ok(());
                }
                _ = interval.tick() => {
                    // Process pending commands
                    while let Ok(cmd) = self.command_rx.try_recv() {
                        self.handle_command(cmd, frames_rendered);
                    }

                    // Data sources: sync to text renderer once per second
                    let ds_period = self.config.fps as u64;
                    if frames_rendered.is_multiple_of(ds_period.max(1)) {
                        let sources = self.services.try_read()
                            .map(|s| s.data_sources.clone())
                            .unwrap_or_default();
                        self.engine.set_data_sources(sources);
                    }

                    // Brightness: apply the current level once per second.
                    // The brightness schedule is now checked by ScreenScheduleService;
                    // the render loop just picks up any level changes it made.
                    let fps = self.config.fps as u64;
                    if frames_rendered.is_multiple_of(fps.max(1))
                        && let Ok(state) = self.services.try_read() {
                            let level = state.brightness.get_level();
                            self.engine.set_brightness(level);
                            self.fpga.set_brightness(level);
                        }

                    // Render frame
                    let has_content = !self.programs.is_empty()
                        || !self.priority_programs.is_empty();
                    if self.screen_on && has_content {
                        let program_dir = self.config.program_dir.clone();
                        let in_priority = !self.priority_programs.is_empty();
                        if in_priority {
                            let program = &self.priority_programs[self.priority_current];
                            self.engine.render_frame(program, &program_dir);
                        } else {
                            let program = &self.programs[self.current_program];
                            self.engine.render_frame(program, &program_dir);
                        }

                        let w = self.engine.width();
                        let h = self.engine.height();

                        match self.config.output_mode {
                            OutputMode::Png => {
                                // Save PNG to disk every 5 seconds; update shared
                                // screenshot buffer every second for responsiveness.
                                let fps = self.config.fps as u64;
                                if frames_rendered == 0 || frames_rendered.is_multiple_of(fps * 5) {
                                    let output_path = self.config.output_path.clone();
                                    self.engine
                                        .save_png(&output_path)
                                        .context("Failed to save PNG")?;
                                    debug!("Saved frame {}", frames_rendered);
                                }
                                if frames_rendered.is_multiple_of(fps) {
                                    self.update_screenshot().await;
                                }
                            }
                            OutputMode::Raw => {
                                use std::io::Write;
                                std::io::stdout().write_all(self.engine.pixels()).ok();
                            }
                            OutputMode::Framebuffer => {
                                // Send rendered frame to FPGA LED panels
                                let pixels = self.engine.pixels().to_vec();
                                self.fpga.send_frame(&pixels, w, h);

                                // Update screenshot every second even in framebuffer mode
                                if frames_rendered.is_multiple_of(self.config.fps as u64) {
                                    self.update_screenshot().await;
                                }
                            }
                            OutputMode::Drm => {
                                let pixels = self.engine.pixels().to_vec();
                                if let Some(ref mut drm) = self.drm_output {
                                    drm.present(&pixels);
                                }
                                if frames_rendered.is_multiple_of(self.config.fps as u64) {
                                    self.update_screenshot().await;
                                }
                            }
                        }

                        frames_rendered += 1;

                        // Restart background music playlist if it has finished playing.
                        // Always uses the regular program's music — priority programs
                        // don't interrupt background audio.
                        if !self.music.is_playing() {
                            self.sync_music(self.current_program);
                        }

                        // Program rotation based on play control
                        if in_priority {
                            self.check_priority_rotation(frames_rendered);
                        } else {
                            self.check_program_rotation(frames_rendered);
                        }
                    } else if self.screen_on {
                        // No programs loaded — render boot logo (or black) every tick
                        let boot_logo_path = if let Ok(state) = self.services.try_read() {
                            state.boot_logo.as_ref().map(|n| self.config.program_dir.join(n))
                        } else {
                            None
                        };
                        self.engine.render_idle(boot_logo_path.as_deref());

                        let fps = self.config.fps as u64;
                        match self.config.output_mode {
                            OutputMode::Png => {
                                if frames_rendered.is_multiple_of((fps * 5).max(1)) {
                                    let output_path = self.config.output_path.clone();
                                    let _ = self.engine.save_png(&output_path);
                                }
                                if frames_rendered.is_multiple_of(fps) {
                                    self.update_screenshot().await;
                                }
                            }
                            OutputMode::Raw => {
                                use std::io::Write;
                                std::io::stdout().write_all(self.engine.pixels()).ok();
                            }
                            OutputMode::Framebuffer => {
                                let pixels = self.engine.pixels().to_vec();
                                let w = self.engine.width();
                                let h = self.engine.height();
                                self.fpga.send_frame(&pixels, w, h);
                                if frames_rendered.is_multiple_of(fps) {
                                    self.update_screenshot().await;
                                }
                            }
                            OutputMode::Drm => {
                                let pixels = self.engine.pixels().to_vec();
                                if let Some(ref mut drm) = self.drm_output {
                                    drm.present(&pixels);
                                }
                                if frames_rendered.is_multiple_of(self.config.fps as u64) {
                                    self.update_screenshot().await;
                                }
                            }
                        }
                        frames_rendered += 1;
                    }
                }
            }
        }
    }

    /// Encode the current framebuffer as PNG and store it in the shared screenshot buffer.
    async fn update_screenshot(&self) {
        let png = self.engine.screenshot_png();
        if !png.is_empty() {
            let state = self.services.read().await;
            let mut buf = state.screenshot.lock().await;
            *buf = png;
        }
    }

    fn handle_command(&mut self, cmd: PlayerCommand, current_frame: u64) {
        match cmd {
            PlayerCommand::LoadScreen(screen) => {
                info!("Loading new screen with {} program(s)", screen.programs.len());
                self.programs = screen.programs;
                self.current_program = 0;
                self.program_start_frame = current_frame;
                let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                if !self.programs.is_empty() {
                    self.engine.reset_for_program(&self.programs[0], elapsed_ms);
                }
                self.sync_programs_to_services(0);
                self.sync_music(0);
            }
            PlayerCommand::SetBrightness(level) => {
                info!("Brightness: {}", level);
                self.engine.set_brightness(level);
                self.fpga.set_brightness(level);
            }
            PlayerCommand::ScreenPower(on) => {
                info!("Screen: {}", if on { "ON" } else { "OFF" });
                self.screen_on = on;
                // Propagate to ServicesState so discovery broadcasts reflect real power state
                if let Ok(mut state) = self.services.try_write() {
                    state.screen_on = on;
                }
            }
            PlayerCommand::SetRotation(deg) => {
                info!("Rotation: {}°", deg);
                self.engine.set_rotation(deg);
                if let Ok(mut state) = self.services.try_write() {
                    state.rotation = deg;
                }
            }
            PlayerCommand::SwitchProgram(guid) => {
                if let Some(idx) = self.programs.iter().position(|p| p.guid == guid) {
                    info!("Switching to program {} ({})", idx, guid);
                    self.current_program = idx;
                    self.program_start_frame = current_frame;
                    let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                    self.engine.reset_for_program(&self.programs[idx], elapsed_ms);
                    self.sync_programs_to_services(idx);
                    self.sync_music(idx);
                } else {
                    warn!("SwitchProgram: GUID not found: {}", guid);
                }
            }
            PlayerCommand::SetVolume(level) => {
                info!("Volume: {}", level);
                self.music.set_volume(level);
            }
            PlayerCommand::InsertProgram(programs) => {
                if programs.is_empty() {
                    return;
                }
                if self.programs.is_empty() {
                    // No regular playlist — just load the inserted programs normally.
                    self.programs = programs;
                    self.current_program = 0;
                    self.program_start_frame = current_frame;
                    let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                    self.engine.reset_for_program(&self.programs[0], elapsed_ms);
                    self.sync_programs_to_services(0);
                    self.sync_music(0);
                    return;
                }
                // Save current position, then start priority playback.
                self.priority_return_idx = self.current_program;
                self.priority_return_start_frame = self.program_start_frame;
                self.priority_programs = programs;
                self.priority_current = 0;
                self.priority_start_frame = current_frame;
                let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                self.engine.reset_for_program(&self.priority_programs[0], elapsed_ms);
                info!("Priority insert: '{}'", self.priority_programs[0].name);
            }

            PlayerCommand::ScreenTest(secs) => {
                let frames = secs as u64 * self.config.fps as u64;
                self.engine.set_test_pattern(if secs > 0 { Some(frames) } else { None });
                info!("Screen test pattern: {}s", secs);
            }

            PlayerCommand::SoftLoadScreen(screen) => {
                info!("Soft-reloading screen with {} program(s)", screen.programs.len());
                let new_programs = screen.programs;
                // Try to keep the current program index pointing at the same GUID.
                let current_guid = self.programs.get(self.current_program)
                    .map(|p| p.guid.clone())
                    .unwrap_or_default();
                let new_idx = new_programs.iter().position(|p| p.guid == current_guid)
                    .unwrap_or(0);
                self.programs = new_programs;
                // Only reset engine if the current program GUID changed.
                if new_idx != self.current_program
                    || self.programs.get(new_idx).map(|p| &p.guid) != Some(&current_guid)
                {
                    self.current_program = new_idx;
                    self.program_start_frame = current_frame;
                    let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                    if let Some(p) = self.programs.get(new_idx) {
                        self.engine.reset_for_program(p, elapsed_ms);
                    }
                } else {
                    self.current_program = new_idx;
                }
                self.sync_programs_to_services(new_idx);
            }

            PlayerCommand::ApplyFpgaConfig(cfg_xml) => {
                use crate::fpga::params::FpgaConfig;
                let cfg = FpgaConfig::from_xml(&cfg_xml);
                self.fpga.apply_config(cfg);
            }

            PlayerCommand::DeleteProgram(guid) => {
                if let Some(idx) = self.programs.iter().position(|p| p.guid == guid) {
                    info!("Deleting program {} ({})", idx, guid);
                    self.programs.remove(idx);
                    if self.programs.is_empty() {
                        self.current_program = 0;
                    } else {
                        self.current_program = self.current_program.min(self.programs.len() - 1);
                        let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                        self.engine.reset_for_program(&self.programs[self.current_program], elapsed_ms);
                    }
                    self.program_start_frame = current_frame;
                    self.sync_programs_to_services(self.current_program);
                } else {
                    warn!("DeleteProgram: GUID not found: {}", guid);
                }
            }
        }
    }

    /// Start or continue background music for the given program index.
    fn sync_music(&mut self, program_idx: usize) {
        let Some(program) = self.programs.get(program_idx) else { return };
        match &program.background_music {
            Some(bm) if !bm.files.is_empty() => {
                let files: Vec<String> = bm.files.iter().map(|f| f.name.clone()).collect();
                let dir = self.config.program_dir.clone();
                self.music.play(&files, &dir);
            }
            _ => self.music.stop(),
        }
    }

    /// Update ServicesState with the current program list and active index.
    fn sync_programs_to_services(&self, active_idx: usize) {
        if let Ok(mut state) = self.services.try_write() {
            state.programs = self
                .programs
                .iter()
                .map(|p| ProgramInfo {
                    guid: p.guid.clone(),
                    name: p.name.clone(),
                    program_type: p.program_type.clone(),
                })
                .collect();
            state.current_program_guid = self
                .programs
                .get(active_idx)
                .map(|p| p.guid.clone())
                .unwrap_or_default();
        }
    }

    /// Advance through priority programs and restore the normal playlist when done.
    fn check_priority_rotation(&mut self, current_frame: u64) {
        if self.priority_programs.is_empty() {
            return;
        }

        let (duration_secs, count) = {
            let program = &self.priority_programs[self.priority_current];
            if let Some(ref pc) = program.play_control {
                if pc.disabled {
                    (0u32, 1u32) // skip immediately
                } else {
                    (parse_duration_secs(&pc.duration).unwrap_or(10), pc.count)
                }
            } else {
                (10, 0)
            }
        };

        let plays = count.max(1) as u64;
        let frames_per = duration_secs as u64 * plays * self.config.fps as u64;
        let elapsed = current_frame.saturating_sub(self.priority_start_frame);

        if elapsed < frames_per {
            return;
        }

        if self.priority_current + 1 < self.priority_programs.len() {
            // Advance to next priority program in the inserted set
            self.priority_current += 1;
            self.priority_start_frame = current_frame;
            let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
            let idx = self.priority_current;
            self.engine.reset_for_program(&self.priority_programs[idx], elapsed_ms);
        } else {
            // Priority finished — restore original playlist position
            info!("Priority program done, resuming normal playlist");
            self.priority_programs.clear();
            self.current_program = self.priority_return_idx;
            self.program_start_frame = self.priority_return_start_frame;
            if !self.programs.is_empty() {
                let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                let curr = self.current_program;
                self.engine.reset_for_program(&self.programs[curr], elapsed_ms);
                self.sync_programs_to_services(curr);
                self.sync_music(curr);
            }
        }
    }

    /// Check if it's time to rotate to the next program
    fn check_program_rotation(&mut self, current_frame: u64) {
        if self.programs.len() <= 1 {
            return;
        }

        let program = &self.programs[self.current_program];

        // Use play control duration if specified, otherwise default 10s.
        // count=0 means play once; count>0 multiplies the duration.
        let (duration_secs, count) = if let Some(ref pc) = program.play_control {
            if pc.disabled {
                return;
            }
            (parse_duration_secs(&pc.duration).unwrap_or(10), pc.count)
        } else {
            (10, 0)
        };

        let plays = count.max(1) as u64;
        let frames_per_program = duration_secs as u64 * plays * self.config.fps as u64;
        let elapsed = current_frame - self.program_start_frame;

        if elapsed >= frames_per_program {
            // Find the next schedulable program (skip disabled/out-of-schedule ones)
            let n = self.programs.len();
            let mut next = (self.current_program + 1) % n;
            let mut found = false;
            for _ in 0..n {
                if is_program_schedulable(&self.programs[next]) {
                    found = true;
                    break;
                }
                next = (next + 1) % n;
            }

            if found && next != self.current_program {
                self.current_program = next;
                self.program_start_frame = current_frame;
                let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                self.engine.reset_for_program(&self.programs[next], elapsed_ms);
                self.sync_programs_to_services(next);
                self.sync_music(next);
                info!(
                    "Program {}/{}: '{}'",
                    self.current_program + 1,
                    self.programs.len(),
                    self.programs[self.current_program].name
                );
            }
        }
    }
}

/// Parse "HH:MM:SS" duration to seconds
fn parse_duration_secs(s: &str) -> Option<u32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 3 {
        let h: u32 = parts[0].parse().ok()?;
        let m: u32 = parts[1].parse().ok()?;
        let s: u32 = parts[2].parse().ok()?;
        Some(h * 3600 + m * 60 + s)
    } else {
        None
    }
}

/// Check whether a program's PlayControl allows it to play right now.
/// Returns `true` if the program is schedulable (or has no restrictions).
pub fn is_program_schedulable(program: &Program) -> bool {
    use chrono::{Datelike, Local, NaiveDate, Timelike};

    let pc = match &program.play_control {
        Some(pc) => pc,
        None => return true, // no restrictions → always play
    };

    if pc.disabled {
        return false;
    }

    let now = Local::now();

    // ── Date range ──────────────────────────────────────────────────────────
    if let Some(ref date) = pc.date {
        if !date.start.is_empty()
            && let Ok(start) = NaiveDate::parse_from_str(&date.start, "%Y-%m-%d")
                && now.date_naive() < start {
                    return false;
                }
        if !date.end.is_empty()
            && let Ok(end) = NaiveDate::parse_from_str(&date.end, "%Y-%m-%d")
                && now.date_naive() > end {
                    return false;
                }
    }

    // ── Time range (supports midnight-crossing windows) ─────────────────────
    if let Some(ref time) = pc.time {
        let cur_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
        // Empty start/end means no lower/upper bound; default to fully-open window.
        let start_s = parse_hm_secs(&time.start).unwrap_or(0);
        let end_s   = parse_hm_secs(&time.end).unwrap_or(86400);
        let in_range = if start_s <= end_s {
            // Normal same-day window: e.g. 08:00–22:00
            cur_secs >= start_s && cur_secs < end_s
        } else {
            // Midnight-crossing window: e.g. 22:00–06:00
            cur_secs >= start_s || cur_secs < end_s
        };
        if !in_range {
            return false;
        }
    }

    // ── Week filter ─────────────────────────────────────────────────────────
    // WeekFilter.enable is a 7-char string like "1111111" (Mon-Sun) or "0010100"
    if let Some(ref week) = pc.week
        && week.enable.len() >= 7 {
            // weekday(): Mon=0 … Sun=6
            let wd = now.weekday().num_days_from_monday() as usize;
            let ch = week.enable.chars().nth(wd).unwrap_or('1');
            if ch == '0' {
                return false;
            }
        }

    true
}

/// Parse "HH:MM" or "HH:MM:SS" into seconds-since-midnight
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
    use crate::program::model::{DateRange, PlayControl, Program, TimeRange, WeekFilter};

    // ── parse_duration_secs ──────────────────────────────────────────────────

    #[test]
    fn test_parse_duration_ten_secs() {
        assert_eq!(parse_duration_secs("00:00:10"), Some(10));
    }

    #[test]
    fn test_parse_duration_one_minute() {
        assert_eq!(parse_duration_secs("00:01:00"), Some(60));
    }

    #[test]
    fn test_parse_duration_one_hour() {
        assert_eq!(parse_duration_secs("01:00:00"), Some(3600));
    }

    #[test]
    fn test_parse_duration_mixed() {
        assert_eq!(parse_duration_secs("01:30:45"), Some(5445));
    }

    #[test]
    fn test_parse_duration_invalid_returns_none() {
        assert_eq!(parse_duration_secs("10"), None);
        assert_eq!(parse_duration_secs(""), None);
        assert_eq!(parse_duration_secs("abc:def:ghi"), None);
    }

    // ── parse_hm_secs ────────────────────────────────────────────────────────

    #[test]
    fn test_parse_hm_secs_hhmm() {
        assert_eq!(parse_hm_secs("08:30"), Some(8 * 3600 + 30 * 60));
    }

    #[test]
    fn test_parse_hm_secs_hhmmss() {
        assert_eq!(parse_hm_secs("08:30:15"), Some(8 * 3600 + 30 * 60 + 15));
    }

    #[test]
    fn test_parse_hm_secs_midnight() {
        assert_eq!(parse_hm_secs("00:00"), Some(0));
    }

    #[test]
    fn test_parse_hm_secs_invalid_returns_none() {
        assert_eq!(parse_hm_secs(""), None);
        assert_eq!(parse_hm_secs("bad"), None);
    }

    // ── is_program_schedulable ───────────────────────────────────────────────

    fn make_program(play_control: Option<PlayControl>) -> Program {
        Program {
            guid: "test".to_string(),
            name: "Test".to_string(),
            program_type: "normal".to_string(),
            flag: String::new(),
            border: None,
            background_music: None,
            play_control,
            areas: Vec::new(),
        }
    }

    fn simple_play_control() -> PlayControl {
        PlayControl {
            duration: "00:00:10".to_string(),
            count: 0,
            disabled: false,
            date: None,
            time: None,
            week: None,
        }
    }

    #[test]
    fn test_schedulable_no_play_control() {
        // No PlayControl → always schedulable
        let prog = make_program(None);
        assert!(is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_disabled_returns_false() {
        let mut pc = simple_play_control();
        pc.disabled = true;
        let prog = make_program(Some(pc));
        assert!(!is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_past_end_date_returns_false() {
        let mut pc = simple_play_control();
        pc.date = Some(DateRange {
            start: "2000-01-01".to_string(),
            end: "2000-12-31".to_string(), // well in the past
        });
        let prog = make_program(Some(pc));
        assert!(!is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_future_start_date_returns_false() {
        let mut pc = simple_play_control();
        pc.date = Some(DateRange {
            start: "2099-01-01".to_string(), // far future
            end: "2099-12-31".to_string(),
        });
        let prog = make_program(Some(pc));
        assert!(!is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_no_date_restriction() {
        // Empty date strings mean no restriction
        let mut pc = simple_play_control();
        pc.date = Some(DateRange {
            start: String::new(),
            end: String::new(),
        });
        let prog = make_program(Some(pc));
        assert!(is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_week_all_enabled() {
        let mut pc = simple_play_control();
        pc.week = Some(WeekFilter { enable: "1111111".to_string() });
        let prog = make_program(Some(pc));
        assert!(is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_week_all_disabled_returns_false() {
        let mut pc = simple_play_control();
        pc.week = Some(WeekFilter { enable: "0000000".to_string() });
        let prog = make_program(Some(pc));
        // At least one day is always today, so this should return false always
        assert!(!is_program_schedulable(&prog));
    }

    #[test]
    fn test_schedulable_outside_time_range_returns_false() {
        // Time window 25:00-26:00 is impossible — will never be in range
        let mut pc = simple_play_control();
        pc.time = Some(TimeRange {
            start: "25:00".to_string(),
            end: "26:00".to_string(),
        });
        let prog = make_program(Some(pc));
        assert!(!is_program_schedulable(&prog));
    }

    /// Verify the midnight-crossing logic directly (without relying on wall clock).
    #[test]
    fn test_midnight_crossing_time_range_logic() {
        // on=22:00 (79200), off=06:00 (21600) → crossing window
        let start_s: u32 = 22 * 3600;
        let end_s:   u32 = 6 * 3600;
        assert!(start_s > end_s, "sanity: crossing window");

        let in_range = |cur: u32| -> bool {
            if start_s <= end_s {
                cur >= start_s && cur < end_s
            } else {
                cur >= start_s || cur < end_s
            }
        };

        assert!(in_range(23 * 3600), "23:00 should be in range");
        assert!(in_range(0),          "00:00 should be in range");
        assert!(in_range(3 * 3600),   "03:00 should be in range");
        assert!(!in_range(6 * 3600),  "06:00 should be out of range (exclusive)");
        assert!(!in_range(12 * 3600), "12:00 should be out of range");
        assert!(!in_range(21 * 3600), "21:00 should be out of range");
    }

    #[test]
    fn test_schedulable_wide_open_time_range() {
        // 00:00–23:59 covers all normal times
        let mut pc = simple_play_control();
        pc.time = Some(TimeRange {
            start: "00:00".to_string(),
            end: "23:59".to_string(),
        });
        let prog = make_program(Some(pc));
        assert!(is_program_schedulable(&prog));
    }
}
