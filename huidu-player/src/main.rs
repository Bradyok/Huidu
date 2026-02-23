#![allow(dead_code)]

use anyhow::Result;
use clap::Parser;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

mod audio;
mod config;
mod core;
mod fpga;
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

    /// TCP listen port for legacy SDK connections (HDSet, old devices)
    #[arg(long, default_value_t = 10001)]
    port: u16,

    /// TCP listen port for BoxStream connections (HDPlayer.exe — port 9527 protocol)
    /// Set to 0 to disable.
    #[arg(long, default_value_t = 9527)]
    box_stream_port: u16,

    /// Target FPS
    #[arg(long, default_value_t = 30)]
    fps: u32,

    /// Output mode: png, raw, framebuffer
    #[arg(long, default_value = "png")]
    output: String,

    /// Output file path (for png mode)
    #[arg(long, default_value = "output.png")]
    output_path: String,

    /// Device ID for network discovery and cloud reporting
    #[arg(long, default_value = "RUST-001")]
    device_id: String,

    /// Cloud OMS base URL (empty = cloud disabled)
    /// Example: http://cloud.example.com
    #[arg(long, default_value = "")]
    cloud_url: String,

    /// Weather API base URL (empty = weather widget disabled)
    /// Supports {city} placeholder.
    /// Example: https://wttr.in/{city}?format=j1
    #[arg(long, default_value = "")]
    weather_url: String,

    /// Log level
    #[arg(long, default_value = "info")]
    log_level: String,

    /// DRM device path (for drm output mode)
    #[arg(long, default_value = "/dev/dri/card0")]
    drm_device: String,

    /// Sync mode: none, master, or slave
    #[arg(long, default_value = "none")]
    sync_mode: String,

    /// UDP port for multi-device sync
    #[arg(long, default_value_t = 9528)]
    sync_port: u16,

    /// Multicast group for sync
    #[arg(long, default_value = "224.0.0.100")]
    sync_group: String,
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

    let cfg = config::PlayerConfig {
        width: args.width,
        height: args.height,
        fps: args.fps,
        program_dir: args.program_dir.clone().into(),
        port: args.port,
        output_mode: args.output.parse().unwrap_or_default(),
        output_path: args.output_path.clone().into(),
        device_id: args.device_id.clone(),
        cloud_url: args.cloud_url.clone(),
        weather_url: args.weather_url.clone(),
    };

    let mut player = Player::new(cfg);

    // Set up DRM output if requested
    if args.output == "drm" {
        player.setup_drm(&args.drm_device);
    }

    // Propagate cloud + device info and restore persisted state
    let (restored_brightness, restored_rotation, restored_volume) = {
        let svc = player.services();
        let mut state = svc.write().await;
        state.cloud_url = args.cloud_url.clone();
        state.device_id = args.device_id.clone();
        state.load_persisted();

        // Apply any pending firmware upgrade staged before the last restart.
        // Must happen before the render loop so the new binary is in place.
        let program_dir_path: std::path::PathBuf = args.program_dir.clone().into();
        if let Some(upgrade_result) =
            services::upgrade::apply_pending_upgrade(&program_dir_path)
        {
            info!("Startup upgrade result: {:?}", upgrade_result);
            state.upgrade_status = upgrade_result;
            state.save_persisted();
        } else if matches!(state.upgrade_status, services::upgrade::UpgradeStatus::Idle) {
            // Check for a persisted result from a previous boot (so GetUpgradeResult
            // keeps reporting success/failure until the next upgrade is triggered).
            if let Some(last) = services::upgrade::read_last_result(&program_dir_path) {
                state.upgrade_status = last;
            }
        }

        (state.brightness.get_level(), state.rotation, state.volume)
    };

    // Apply restored settings to the render engine and audio before the loop starts.
    {
        use crate::core::player::PlayerCommand;
        let tx = player.program_sender();
        let _ = tx.send(PlayerCommand::SetBrightness(restored_brightness)).await;
        let _ = tx.send(PlayerCommand::SetRotation(restored_rotation)).await;
        let _ = tx.send(PlayerCommand::SetVolume(restored_volume)).await;
    }

    // Load any existing programs from disk
    if let Err(e) = player.load_programs_from_dir(&args.program_dir) {
        warn!("No programs loaded from {}: {}", args.program_dir, e);
    }

    let services = player.services();

    // Shared cancellation token — cancelled on Ctrl-C, SIGTERM, or post-upgrade restart.
    let cancel = CancellationToken::new();

    // Make the token available to the FirmwareUpgrade command handler so it can
    // trigger a graceful restart after staging an upgrade.
    {
        let mut state = services.write().await;
        state.shutdown_token = Some(cancel.clone());
    }

    // Spawn signal handler: cancel all services on Ctrl-C / SIGTERM.
    {
        let tok = cancel.clone();
        tokio::spawn(async move {
            #[cfg(unix)]
            {
                use tokio::signal::unix::{signal, SignalKind};
                let mut sigterm = signal(SignalKind::terminate())
                    .expect("failed to register SIGTERM handler");
                tokio::select! {
                    _ = tokio::signal::ctrl_c() => {}
                    _ = sigterm.recv() => {}
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
            info!("Shutdown signal received — stopping services");
            tok.cancel();
        });
    }

    // Start the legacy TCP protocol server (port 10001 — HDSet / old SDK)
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

    // Start the BoxStream server (port 9527 — HDPlayer.exe protocol)
    let box_stream_handle = if args.box_stream_port > 0 {
        let tx = player.program_sender();
        let port = args.box_stream_port;
        let dir = args.program_dir.clone();
        let svc = services.clone();
        let w = args.width;
        let h = args.height;
        Some(tokio::spawn(async move {
            if let Err(e) = protocol::server::run_box_stream(port, tx, dir, svc, w, h).await {
                tracing::error!("BoxStream server error: {}", e);
            }
        }))
    } else {
        None
    };

    // Start UDP discovery on port 9527 (Huidu discovery port)
    let discovery_handle = {
        let ip = protocol::discovery::get_local_ip();
        let device_info = protocol::discovery::DeviceInfo {
            device_id: args.device_id.clone(),
            ip_address: ip,
            screen_width: args.width as u16,
            screen_height: args.height as u16,
            player_name: "BoxPlayer".to_string(),
        };
        let svc = services.clone();
        tokio::spawn(async move {
            if let Err(e) = protocol::discovery::run(device_info, svc).await {
                tracing::error!("UDP discovery error: {}", e);
            }
        })
    };

    // Start background services (scheduling, NTP, USB disk, cloud)
    let program_dir = args.program_dir.clone().into();
    services::manager::start_services(services, player.program_sender(), program_dir, cancel.clone()).await;

    // Set up multi-device sync
    let sync_mode: crate::services::sync_service::SyncMode = args.sync_mode.parse().unwrap_or(crate::services::sync_service::SyncMode::None);
    if sync_mode != crate::services::sync_service::SyncMode::None {
        let clock = crate::core::sync_clock::SyncClock::new();
        match sync_mode {
            crate::services::sync_service::SyncMode::Master => {
                crate::services::sync_service::SyncService::start_master(
                    clock.clone(), &args.sync_group, args.sync_port, cancel.clone()
                );
            }
            crate::services::sync_service::SyncMode::Slave => {
                let tx = player.program_sender();
                crate::services::sync_service::SyncService::start_slave(
                    clock.clone(), &args.sync_group, args.sync_port, cancel.clone(), tx
                );
            }
            crate::services::sync_service::SyncMode::None => {}
        }
        player.set_sync_clock(clock);
    }

    // Run the render loop — exits when cancel fires (Ctrl-C / SIGTERM).
    player.run(cancel).await?;

    protocol_handle.abort();
    if let Some(h) = box_stream_handle { h.abort(); }
    discovery_handle.abort();
    info!("huidu-player shutdown");
    Ok(())
}
