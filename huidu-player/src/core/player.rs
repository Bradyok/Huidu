/// Main player — orchestrates program loading, rendering, and output.
use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

use crate::config::{OutputMode, PlayerConfig};
use crate::program::model::{Program, Screen};
use crate::program::parser;
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
}

pub struct Player {
    config: PlayerConfig,
    engine: RenderEngine,
    programs: Vec<Program>,
    current_program: usize,
    /// Time (in frames) when current program started
    program_start_frame: u64,
    command_rx: mpsc::Receiver<PlayerCommand>,
    command_tx: mpsc::Sender<PlayerCommand>,
    screen_on: bool,
    /// Shared services state
    services: Arc<RwLock<ServicesState>>,
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
            programs: Vec::new(),
            current_program: 0,
            program_start_frame: 0,
            command_rx: rx,
            command_tx: tx,
            screen_on: true,
            services,
        }
    }

    pub fn program_sender(&self) -> mpsc::Sender<PlayerCommand> {
        self.command_tx.clone()
    }

    pub fn services(&self) -> Arc<RwLock<ServicesState>> {
        self.services.clone()
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
        }

        info!("Loaded {} total programs from {}", self.programs.len(), dir);
        Ok(())
    }

    /// Main render loop
    pub async fn run(&mut self) -> Result<()> {
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
                _ = interval.tick() => {
                    // Process pending commands
                    while let Ok(cmd) = self.command_rx.try_recv() {
                        self.handle_command(cmd, frames_rendered);
                    }

                    // Render frame
                    if self.screen_on && !self.programs.is_empty() {
                        let program_dir = self.config.program_dir.clone();
                        let program = &self.programs[self.current_program];
                        self.engine.render_frame(program, &program_dir);

                        match self.config.output_mode {
                            OutputMode::Png => {
                                // Save every 5 seconds
                                if frames_rendered == 0
                                    || frames_rendered % (self.config.fps as u64 * 5) == 0
                                {
                                    let output_path = self.config.output_path.clone();
                                    self.engine
                                        .save_png(&output_path)
                                        .context("Failed to save PNG")?;
                                    debug!("Saved frame {}", frames_rendered);

                                    // Also update the shared screenshot buffer
                                    self.update_screenshot().await;
                                }
                            }
                            OutputMode::Raw => {
                                use std::io::Write;
                                std::io::stdout().write_all(self.engine.pixels()).ok();
                            }
                            OutputMode::Framebuffer => {
                                // TODO: DRM/KMS output
                                // Update screenshot every 5 seconds even in framebuffer mode
                                if frames_rendered % (self.config.fps as u64 * 5) == 0 {
                                    self.update_screenshot().await;
                                }
                            }
                        }

                        frames_rendered += 1;

                        // Program rotation based on play control
                        self.check_program_rotation(frames_rendered);
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
            }
            PlayerCommand::SetBrightness(level) => {
                info!("Brightness: {}", level);
                self.engine.set_brightness(level);
            }
            PlayerCommand::ScreenPower(on) => {
                info!("Screen: {}", if on { "ON" } else { "OFF" });
                self.screen_on = on;
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
                } else {
                    warn!("SwitchProgram: GUID not found: {}", guid);
                }
            }
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
                })
                .collect();
            state.current_program_guid = self
                .programs
                .get(active_idx)
                .map(|p| p.guid.clone())
                .unwrap_or_default();
        }
    }

    /// Check if it's time to rotate to the next program
    fn check_program_rotation(&mut self, current_frame: u64) {
        if self.programs.len() <= 1 {
            return;
        }

        let program = &self.programs[self.current_program];

        // Use play control duration if specified, otherwise default 10s
        let duration_secs = if let Some(ref pc) = program.play_control {
            if pc.disabled {
                return;
            }
            parse_duration_secs(&pc.duration).unwrap_or(10)
        } else {
            10
        };

        let frames_per_program = duration_secs as u64 * self.config.fps as u64;
        let elapsed = current_frame - self.program_start_frame;

        if elapsed >= frames_per_program {
            // Find the next schedulable program (skip disabled/out-of-schedule ones)
            let n = self.programs.len();
            let mut next = (self.current_program + 1) % n;
            for _ in 0..n {
                if is_program_schedulable(&self.programs[next]) {
                    break;
                }
                next = (next + 1) % n;
            }

            if next != self.current_program {
                self.current_program = next;
                self.program_start_frame = current_frame;
                let elapsed_ms = current_frame * (1000 / self.config.fps as u64);
                self.engine.reset_for_program(&self.programs[next], elapsed_ms);
                self.sync_programs_to_services(next);
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
        if !date.start.is_empty() {
            if let Ok(start) = NaiveDate::parse_from_str(&date.start, "%Y-%m-%d") {
                if now.date_naive() < start {
                    return false;
                }
            }
        }
        if !date.end.is_empty() {
            if let Ok(end) = NaiveDate::parse_from_str(&date.end, "%Y-%m-%d") {
                if now.date_naive() > end {
                    return false;
                }
            }
        }
    }

    // ── Time range ──────────────────────────────────────────────────────────
    if let Some(ref time) = pc.time {
        let cur_secs = now.hour() * 3600 + now.minute() * 60 + now.second();
        if !time.start.is_empty() {
            if let Some(start_s) = parse_hm_secs(&time.start) {
                if cur_secs < start_s {
                    return false;
                }
            }
        }
        if !time.end.is_empty() {
            if let Some(end_s) = parse_hm_secs(&time.end) {
                if cur_secs > end_s {
                    return false;
                }
            }
        }
    }

    // ── Week filter ─────────────────────────────────────────────────────────
    // WeekFilter.enable is a 7-char string like "1111111" (Mon-Sun) or "0010100"
    if let Some(ref week) = pc.week {
        if week.enable.len() >= 7 {
            // weekday(): Mon=0 … Sun=6
            let wd = now.weekday().num_days_from_monday() as usize;
            let ch = week.enable.chars().nth(wd).unwrap_or('1');
            if ch == '0' {
                return false;
            }
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
