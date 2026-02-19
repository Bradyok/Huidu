/// Main player — orchestrates program loading, rendering, and output.
use anyhow::{Context, Result};
use std::path::Path;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

use crate::config::{OutputMode, PlayerConfig};
use crate::program::model::{Program, Screen};
use crate::program::parser;
use crate::render::engine::RenderEngine;

/// Commands sent from the protocol server to the player
#[derive(Debug)]
pub enum PlayerCommand {
    /// Load a new screen (replaces all programs)
    LoadScreen(Screen),
    /// Set brightness (0-100)
    SetBrightness(u8),
    /// Turn screen on/off
    ScreenPower(bool),
}

pub struct Player {
    config: PlayerConfig,
    engine: RenderEngine,
    /// Currently loaded programs
    programs: Vec<Program>,
    /// Index of currently playing program
    current_program: usize,
    /// Channel for receiving commands from protocol server
    command_rx: mpsc::Receiver<PlayerCommand>,
    /// Sender clone for giving to the protocol server
    command_tx: mpsc::Sender<PlayerCommand>,
    /// Screen on/off state
    screen_on: bool,
}

impl Player {
    pub fn new(config: PlayerConfig) -> Self {
        let (tx, rx) = mpsc::channel(64);
        let engine = RenderEngine::new(config.width, config.height, config.fps);

        Self {
            config,
            engine,
            programs: Vec::new(),
            current_program: 0,
            command_rx: rx,
            command_tx: tx,
            screen_on: true,
        }
    }

    /// Get a clone of the command sender for the protocol server
    pub fn program_sender(&self) -> mpsc::Sender<PlayerCommand> {
        self.command_tx.clone()
    }

    /// Load programs from a directory (looks for *.xml files)
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
                    // Process any pending commands
                    while let Ok(cmd) = self.command_rx.try_recv() {
                        self.handle_command(cmd);
                    }

                    // Render frame
                    if self.screen_on && !self.programs.is_empty() {
                        let program_dir = self.config.program_dir.clone();
                        let program = &self.programs[self.current_program];
                        self.engine.render_frame(program, &program_dir);

                        // Output the frame
                        match self.config.output_mode {
                            OutputMode::Png => {
                                if frames_rendered == 0 || frames_rendered % (self.config.fps as u64 * 5) == 0 {
                                    let output_path = self.config.output_path.clone();
                                    self.engine.save_png(&output_path)
                                        .context("Failed to save PNG output")?;
                                    debug!("Saved frame {} to {}", frames_rendered, output_path.display());
                                }
                            }
                            OutputMode::Raw => {
                                let data = self.engine.pixels();
                                use std::io::Write;
                                std::io::stdout().write_all(data).ok();
                            }
                            OutputMode::Framebuffer => {
                                // TODO: DRM/KMS output for production
                                debug!("Framebuffer output not yet implemented");
                            }
                        }

                        frames_rendered += 1;

                        // Check if we should advance to next program
                        // (simplified: advance every 10 seconds if multiple programs)
                        if self.programs.len() > 1 {
                            let secs = frames_rendered / self.config.fps as u64;
                            let prog_idx = (secs / 10) as usize % self.programs.len();
                            if prog_idx != self.current_program {
                                self.current_program = prog_idx;
                                info!("Switching to program {}/{}: '{}'",
                                    self.current_program + 1,
                                    self.programs.len(),
                                    self.programs[self.current_program].name
                                );
                            }
                        }
                    }

                    // In PNG mode, exit after rendering first frame if no server
                    if matches!(self.config.output_mode, OutputMode::Png)
                        && frames_rendered > 0
                        && self.programs.is_empty()
                    {
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    fn handle_command(&mut self, cmd: PlayerCommand) {
        match cmd {
            PlayerCommand::LoadScreen(screen) => {
                info!(
                    "Loading new screen with {} program(s)",
                    screen.programs.len()
                );
                self.programs = screen.programs;
                self.current_program = 0;
            }
            PlayerCommand::SetBrightness(level) => {
                info!("Setting brightness to {}", level);
                // TODO: FPGA brightness control
            }
            PlayerCommand::ScreenPower(on) => {
                info!("Screen power: {}", if on { "ON" } else { "OFF" });
                self.screen_on = on;
            }
        }
    }
}
