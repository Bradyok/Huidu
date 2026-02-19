#![allow(dead_code)]

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

mod config;
mod core;
mod program;
mod protocol;
mod render;
mod services;

use crate::core::player::Player;

#[derive(Parser, Debug)]
#[command(name = "huidu-player", about = "Huidu LED sign player - Rust reproduction of BoxPlayer")]
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

    /// Output mode: png, raw, framebuffer
    #[arg(long, default_value = "png")]
    output: String,

    /// Output file path (for png mode)
    #[arg(long, default_value = "output.png")]
    output_path: String,

    /// Device ID for network discovery
    #[arg(long, default_value = "RUST-001")]
    device_id: String,

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
        "huidu-player v{} starting ({}x{} @ {}fps, device={})",
        env!("CARGO_PKG_VERSION"),
        args.width,
        args.height,
        args.fps,
        args.device_id,
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

    // Load any existing programs from disk
    if let Err(e) = player.load_programs_from_dir(&args.program_dir) {
        warn!("No programs loaded from {}: {}", args.program_dir, e);
    }

    let services = player.services();

    // Start the TCP protocol server
    let protocol_handle = {
        let tx = player.program_sender();
        let port = args.port;
        let dir = args.program_dir.clone();
        let svc = services.clone();
        let w = args.width;
        let h = args.height;
        tokio::spawn(async move {
            if let Err(e) = protocol::server::run(port, tx, dir, svc, w, h).await {
                tracing::error!("Protocol server error: {}", e);
            }
        })
    };

    // Start UDP discovery responder
    let discovery_handle = {
        let port = args.port;
        let ip = protocol::discovery::get_local_ip();
        let device_info = protocol::discovery::DeviceInfo {
            device_id: args.device_id.clone(),
            ip_address: ip,
            screen_width: args.width as u16,
            screen_height: args.height as u16,
        };
        tokio::spawn(async move {
            if let Err(e) = protocol::discovery::run(port, device_info).await {
                tracing::error!("UDP discovery error: {}", e);
            }
        })
    };

    // Start background services (scheduling, NTP, USB disk)
    let program_dir = args.program_dir.clone().into();
    services::manager::start_services(services, player.program_sender(), program_dir).await;

    // Run the render loop (blocks)
    player.run().await?;

    protocol_handle.abort();
    discovery_handle.abort();
    info!("huidu-player shutdown");
    Ok(())
}
