//! hdplayer-client — CLI tool for controlling Huidu BoxPlayer devices.
//!
//! This binary reproduces the core control functionality of Huidu HDPlayer.exe
//! as a cross-platform command-line tool.

use clap::{Parser, Subcommand};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

use hdplayer_client::{Client, Discovery};
use hdplayer_client::transfer::FileTransfer;

#[derive(Parser)]
#[command(
    name = "hdplayer-client",
    about = "Huidu BoxPlayer control client (HDPlayer.exe reproduction)",
    version
)]
struct Cli {
    /// Device IP address. If omitted, discover devices automatically.
    #[arg(short, long)]
    host: Option<String>,

    /// TCP port (default: 10001)
    #[arg(short, long, default_value_t = 10001)]
    port: u16,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover Huidu devices on the local network
    Discover {
        /// Discovery timeout in seconds
        #[arg(short, long, default_value_t = 3)]
        timeout: u64,
    },

    /// Get device information
    Info,

    /// Get hardware info
    HwInfo,

    /// List all programs on the device
    ListPrograms,

    /// Upload a program XML file to the device
    Upload {
        /// Path to program XML (.boo) file
        file: String,
    },

    /// Upload a media file (image, video, etc.)
    UploadFile {
        /// Path to file
        file: String,
    },

    /// Switch to a program by GUID
    Switch {
        /// Program GUID
        guid: String,
    },

    /// Switch to a program by index
    SwitchIndex {
        /// Program index (0-based)
        index: u32,
    },

    /// Delete a program by GUID
    DeleteProgram {
        /// Program GUID
        guid: String,
    },

    /// Set display brightness
    Brightness {
        /// Brightness level 0-100
        level: u8,
    },

    /// Turn screen on
    ScreenOn,

    /// Turn screen off
    ScreenOff,

    /// Set screen rotation
    Rotate {
        /// Rotation angle (0, 90, 180, 270)
        #[arg(value_parser = clap::value_parser!(u16))]
        angle: u16,
    },

    /// Set audio volume
    Volume {
        /// Volume level 0-100
        level: u8,
    },

    /// Set device name
    SetName {
        /// New device name
        name: String,
    },

    /// Sync device time to current local time
    SyncTime,

    /// Get Ethernet configuration
    GetNetwork,

    /// Get FPGA hardware configuration
    GetFpgaConfig,

    /// Get screen on/off schedule
    GetSchedule,

    /// Set screen on/off schedule
    SetSchedule {
        /// Enable schedule
        #[arg(long)]
        enable: bool,
        /// Screen on time (HH:MM)
        #[arg(long)]
        on: String,
        /// Screen off time (HH:MM)
        #[arg(long)]
        off: String,
    },

    /// Take a screenshot
    Screenshot {
        /// Output file path
        #[arg(short, long, default_value = "screenshot.jpg")]
        output: String,
        /// Width
        #[arg(long, default_value_t = 1920)]
        width: u32,
        /// Height
        #[arg(long, default_value_t = 1080)]
        height: u32,
    },

    /// Reboot the device
    Reboot,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("hdplayer_client=info".parse()?))
        .init();

    // Handle discover command without a host
    if let Commands::Discover { timeout } = &cli.command {
        println!("Scanning for Huidu devices ({timeout}s timeout)...");
        let devices = Discovery::scan(Duration::from_secs(*timeout)).await?;
        if devices.is_empty() {
            println!("No devices found.");
        } else {
            println!("Found {} device(s):", devices.len());
            for (i, dev) in devices.iter().enumerate() {
                println!(
                    "  [{i}] {} (ID: {}) @ {}",
                    dev.name, dev.device_id, dev.addr
                );
                if let Some(t) = &dev.device_type {
                    println!("       Type: {t}");
                }
                if let Some(v) = &dev.firmware_version {
                    println!("       Firmware: {v}");
                }
                if let (Some(w), Some(h)) = (dev.screen_width, dev.screen_height) {
                    println!("       Screen: {w}x{h}");
                }
            }
        }
        return Ok(());
    }

    // All other commands need a host
    let host = match &cli.host {
        Some(h) => h.clone(),
        None => {
            // Auto-discover and use first device
            println!("No host specified, discovering...");
            let devices = Discovery::scan(Duration::from_secs(3)).await?;
            match devices.first() {
                Some(dev) => {
                    println!("Using device: {} @ {}", dev.name, dev.addr);
                    dev.addr.to_string()
                }
                None => {
                    eprintln!("No devices found. Specify --host.");
                    std::process::exit(1);
                }
            }
        }
    };

    let mut client = Client::connect(&host, cli.port).await?;
    println!("Connected to {host}:{}", cli.port);

    match &cli.command {
        Commands::Discover { .. } => unreachable!(),

        Commands::Info => {
            let info = client.get_device_info().await?;
            println!("Device ID:    {}", info.device_id);
            println!("Name:         {}", info.device_name);
            println!("Type:         {}", info.device_type);
            println!("Firmware:     {}", info.firmware_version);
            println!("Screen:       {}x{}", info.screen_width, info.screen_height);
            println!("IP:           {}", info.ip_address);
            println!("MAC:          {}", info.mac_address);
            println!("Brightness:   {}%", info.brightness);
            println!("Volume:       {}", info.volume);
        }

        Commands::HwInfo => {
            let xml = client.get_hardware_info().await?;
            println!("{xml}");
        }

        Commands::ListPrograms => {
            let programs = client.get_all_programs().await?;
            if programs.is_empty() {
                println!("No programs on device.");
            } else {
                println!("Programs ({}):", programs.len());
                for p in &programs {
                    let current = if p.is_current { " [ACTIVE]" } else { "" };
                    println!("  {} -- {} ({}){}", p.guid, p.name, p.program_type, current);
                }
            }
        }

        Commands::Upload { file } => {
            let content = tokio::fs::read_to_string(file).await?;
            // Strip SDK envelope if present, keep just the screen XML
            let screen_xml = if content.contains("<screen") {
                let start = content.find("<screen").unwrap_or(0);
                let end = content.rfind("</screen>")
                    .map(|i| i + "</screen>".len())
                    .unwrap_or(content.len());
                content[start..end].to_string()
            } else {
                content
            };
            client.add_program(&screen_xml).await?;
            println!("Program uploaded successfully.");
        }

        Commands::UploadFile { file } => {
            let transfer = FileTransfer::from_file(file).await?;
            let total = transfer.total_size();
            client.upload_file(&transfer, Some(&|sent, total| {
                print!("\rUploading: {}/{} bytes ({:.1}%)",
                    sent, total, sent as f64 / total as f64 * 100.0);
                let _ = std::io::Write::flush(&mut std::io::stdout());
            })).await?;
            println!("\nUploaded {} bytes.", total);
        }

        Commands::Switch { guid } => {
            client.switch_program(guid).await?;
            println!("Switched to program {guid}");
        }

        Commands::SwitchIndex { index } => {
            client.switch_program_index(*index).await?;
            println!("Switched to program index {index}");
        }

        Commands::DeleteProgram { guid } => {
            client.delete_program(guid).await?;
            println!("Deleted program {guid}");
        }

        Commands::Brightness { level } => {
            client.set_brightness(*level).await?;
            println!("Brightness set to {level}%");
        }

        Commands::ScreenOn => {
            client.screen_on().await?;
            println!("Screen on.");
        }

        Commands::ScreenOff => {
            client.screen_off().await?;
            println!("Screen off.");
        }

        Commands::Rotate { angle } => {
            client.set_rotation(*angle).await?;
            println!("Rotation set to {angle} degrees");
        }

        Commands::Volume { level } => {
            client.set_volume(*level).await?;
            println!("Volume set to {level}");
        }

        Commands::SetName { name } => {
            client.set_device_name(name).await?;
            println!("Device name set to '{name}'");
        }

        Commands::SyncTime => {
            client.sync_time().await?;
            println!("Time synced.");
        }

        Commands::GetNetwork => {
            let xml = client.get_eth0_info().await?;
            println!("{xml}");
        }

        Commands::GetFpgaConfig => {
            let xml = client.get_fpga_config().await?;
            println!("{xml}");
        }

        Commands::GetSchedule => {
            let xml = client.get_switch_time().await?;
            println!("{xml}");
        }

        Commands::SetSchedule { enable, on, off } => {
            client.set_switch_time(*enable, on, off).await?;
            println!("Schedule set: on={on} off={off} enabled={enable}");
        }

        Commands::Screenshot { output, width, height } => {
            let data = client.screenshot(*width, *height).await?;
            tokio::fs::write(output, &data).await?;
            println!("Screenshot saved to {output} ({} bytes)", data.len());
        }

        Commands::Reboot => {
            client.reboot().await?;
            println!("Reboot initiated.");
        }
    }

    Ok(())
}
