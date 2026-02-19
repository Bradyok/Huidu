use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

mod config;
mod core;
mod program;
mod protocol;
mod render;

use crate::core::player::Player;

#[derive(Parser, Debug)]
#[command(name = "huidu-player", about = "Huidu LED sign player")]
struct Args {
    /// Program directory path
    #[arg(short, long, default_value = "programs")]
    program_dir: String,

    /// Display width in pixels
    #[arg(long, default_value_t = 128)]
    width: u32,

    /// Display height in pixels
    #[arg(long, default_value_t = 64)]
    height: u32,

    /// TCP listen port for HDPlayer connections
    #[arg(long, default_value_t = 10001)]
    port: u16,

    /// Target FPS
    #[arg(long, default_value_t = 30)]
    fps: u32,

    /// Output mode: window, png, framebuffer
    #[arg(long, default_value = "png")]
    output: String,

    /// Output file path (for png mode)
    #[arg(long, default_value = "output.png")]
    output_path: String,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| args.log_level.parse().unwrap_or_default()),
        )
        .init();

    info!(
        "huidu-player v{} starting ({}x{} @ {}fps)",
        env!("CARGO_PKG_VERSION"),
        args.width,
        args.height,
        args.fps
    );

    let mut player = Player::new(config::PlayerConfig {
        width: args.width,
        height: args.height,
        fps: args.fps,
        program_dir: args.program_dir.clone().into(),
        port: args.port,
        output_mode: args.output.parse().unwrap_or_default(),
        output_path: args.output_path.clone().into(),
    });

    // Load any existing program from disk
    if let Err(e) = player.load_programs_from_dir(&args.program_dir) {
        warn!("No programs loaded from {}: {}", args.program_dir, e);
    }

    // Start the TCP protocol server in background
    let protocol_handle = {
        let program_tx = player.program_sender();
        let port = args.port;
        let program_dir = args.program_dir.clone();
        tokio::spawn(async move {
            if let Err(e) = protocol::server::run(port, program_tx, program_dir).await {
                tracing::error!("Protocol server error: {}", e);
            }
        })
    };

    // Run the render loop
    player.run().await?;

    protocol_handle.abort();
    info!("huidu-player shutdown");
    Ok(())
}
