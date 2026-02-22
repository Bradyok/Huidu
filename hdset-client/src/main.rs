//! HDSet CLI — hardware configuration tool for Huidu LED display systems.
//!
//! Usage examples:
//!   hdset-client discover
//!   hdset-client --host 192.168.1.100 info
//!   hdset-client --host 192.168.1.100 get-hw-config
//!   hdset-client --host 192.168.1.100 set-hw-config --file config.xml
//!   hdset-client --host 192.168.1.100 screen-test --duration 10
//!   hdset-client --host 192.168.1.100 brightness --level 80
//!   hdset-client --host 192.168.1.100 boot-logo --set logo.png
//!   hdset-client --host 192.168.1.100 firmware --file update.zbin
//!   hdset-client --host 192.168.1.100 smart-setting

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use tracing::info;
use tracing_subscriber::EnvFilter;

use hdset::{Discovery, HdsetClient};
use hdset::hw_config::{BoxHwConfig, CardConfig};

#[derive(Parser)]
#[command(name = "hdset-client", about = "Huidu HDSet hardware configuration tool")]
struct Cli {
    /// Device IP address (not needed for 'discover')
    #[arg(long, short = 'H', global = true)]
    host: Option<String>,

    /// TCP port (default 10001)
    #[arg(long, default_value_t = 10001, global = true)]
    port: u16,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Discover Huidu devices on the local network
    Discover {
        /// Scan timeout in seconds
        #[arg(long, default_value_t = 3)]
        timeout: u64,
    },

    /// Print device information
    Info,

    /// Read the BoxHwConfig (FPGA hardware parameters) from the device
    GetHwConfig {
        /// Write config XML to this file (omit to print to stdout)
        #[arg(long)]
        output: Option<PathBuf>,
    },

    /// Write a BoxHwConfig XML file to the device and save it
    SetHwConfig {
        /// Path to BoxHwConfig XML file
        #[arg(long)]
        file: PathBuf,
    },

    /// Apply a preset hardware configuration for common LED module types
    Preset {
        /// Module description (e.g. "P10-outdoor-1:4-ICN2038" or "P3-indoor-static-MBI5153")
        module: String,
    },

    /// Trigger auto-detection of LED module parameters (SmartSetting)
    SmartSetting,

    /// Run the SmartDrawLine test (auto-detect panel dimensions)
    SmartDrawLine,

    /// Run a screen hardware test pattern
    ScreenTest {
        /// Duration in seconds
        #[arg(long, default_value_t = 10)]
        duration: u32,
        /// Solid color test: R,G,B (e.g. "255,0,0" for red)
        #[arg(long)]
        color: Option<String>,
    },

    /// Set screen brightness (0–100)
    Brightness {
        #[arg(long, short)]
        level: u8,
    },

    /// Turn the LED screen on
    ScreenOn,

    /// Turn the LED screen off
    ScreenOff,

    /// Boot logo management
    BootLogo {
        /// Upload and activate this image as the boot logo
        #[arg(long)]
        set: Option<PathBuf>,
        /// Clear the boot logo
        #[arg(long)]
        clear: bool,
        /// Print the current boot logo filename
        #[arg(long)]
        get: bool,
    },

    /// Firmware upgrade (upload a .zbin file)
    Firmware {
        #[arg(long)]
        file: PathBuf,
    },

    /// Print network (Ethernet) configuration
    GetNetwork,

    /// Reboot the device
    Reboot,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("hdset=info".parse()?))
        .init();

    let cli = Cli::parse();

    if matches!(cli.cmd, Cmd::Discover { .. }) {
        return run_discover(&cli).await;
    }

    let host = cli.host.as_deref()
        .context("--host <IP> is required for this command")?;

    let mut client = HdsetClient::connect(host, cli.port).await
        .with_context(|| format!("Failed to connect to {host}:{}", cli.port))?;
    info!("Connected to {host}:{}", cli.port);

    match cli.cmd {
        Cmd::Discover { .. } => unreachable!(),

        Cmd::Info => {
            let info = client.get_device_info().await?;
            println!("Device ID:    {}", info.device_id);
            println!("Device name:  {}", info.device_name);
            println!("Device type:  {}", info.device_type);
            println!("Firmware:     {}", info.firmware_version);
            println!("Screen:       {}×{}", info.screen_width, info.screen_height);
            println!("IP:           {}", info.ip_address);
            println!("MAC:          {}", info.mac_address);
            println!("Brightness:   {}%", info.brightness);
            println!("Rotation:     {}°", info.rotation * 90);
            println!("Storage:      {} MB free / {} MB total",
                info.storage_free / 1_048_576, info.storage_total / 1_048_576);
        }

        Cmd::GetHwConfig { output } => {
            let cfg = client.get_box_hw_config().await?;
            let xml = hdset::hw_config::to_xml(&cfg);
            match output {
                Some(path) => {
                    std::fs::write(&path, &xml)?;
                    println!("BoxHwConfig saved to {}", path.display());
                }
                None => println!("{}", xml),
            }
        }

        Cmd::SetHwConfig { file } => {
            let xml_str = std::fs::read_to_string(&file)
                .with_context(|| format!("Cannot read {}", file.display()))?;
            let cfg = hdset::hw_config::from_xml(&xml_str)
                .context("Failed to parse BoxHwConfig XML")?;
            client.set_box_hw_config(&cfg).await?;
            client.save_box_hw_config().await?;
            println!("BoxHwConfig applied and saved.");
        }

        Cmd::Preset { module } => {
            let cfg = build_preset_config(&module)
                .with_context(|| format!("Unknown preset '{}'", module))?;
            client.set_box_hw_config(&cfg).await?;
            client.save_box_hw_config().await?;
            println!("Preset '{}' applied.", module);
        }

        Cmd::SmartSetting => {
            client.smart_setting().await?;
            println!("SmartSetting triggered — device is probing hardware.");
        }

        Cmd::SmartDrawLine => {
            client.smart_draw_line().await?;
            println!("SmartDrawLine triggered.");
        }

        Cmd::ScreenTest { duration, color } => {
            match color {
                Some(rgb) => {
                    let parts: Vec<u8> = rgb.split(',')
                        .filter_map(|s| s.trim().parse().ok())
                        .collect();
                    if parts.len() != 3 {
                        bail!("--color must be 'R,G,B' (e.g. 255,0,0)");
                    }
                    client.screen_test_color(parts[0], parts[1], parts[2]).await?;
                    println!("Solid color screen test started.");
                }
                None => {
                    client.screen_test(duration).await?;
                    println!("Screen test started ({} seconds).", duration);
                }
            }
        }

        Cmd::Brightness { level } => {
            client.set_brightness(level).await?;
            println!("Brightness set to {}%", level);
        }

        Cmd::ScreenOn => {
            client.open_screen().await?;
            println!("Screen turned on.");
        }

        Cmd::ScreenOff => {
            client.close_screen().await?;
            println!("Screen turned off.");
        }

        Cmd::BootLogo { set, clear, get } => {
            if get || (!set.is_some() && !clear) {
                let name = client.get_boot_logo().await?;
                if name.is_empty() {
                    println!("No boot logo configured.");
                } else {
                    println!("Boot logo: {}", name);
                }
            }
            if let Some(path) = set {
                client.upload_boot_logo(&path).await?;
                println!("Boot logo set to {}", path.display());
            }
            if clear {
                client.clear_boot_logo().await?;
                println!("Boot logo cleared.");
            }
        }

        Cmd::Firmware { file } => {
            println!("Uploading firmware {}...", file.display());
            client.firmware_upgrade(&file).await?;
            println!("Firmware upload complete. Device will upgrade and reboot.");
        }

        Cmd::GetNetwork => {
            let eth = client.get_eth_info().await?;
            println!("DHCP:    {}", eth.dhcp);
            println!("IP:      {}", eth.ip);
            println!("Mask:    {}", eth.mask);
            println!("Gateway: {}", eth.gateway);
            println!("DNS:     {}", eth.dns);
        }

        Cmd::Reboot => {
            client.reboot().await?;
            println!("Reboot command sent.");
        }
    }

    Ok(())
}

async fn run_discover(cli: &Cli) -> Result<()> {
    let timeout = match &cli.cmd {
        Cmd::Discover { timeout } => Duration::from_secs(*timeout),
        _ => Duration::from_secs(3),
    };
    println!("Scanning for Huidu devices ({:.0}s)...", timeout.as_secs_f32());
    let devices = Discovery::scan(timeout).await?;
    if devices.is_empty() {
        println!("No devices found.");
        return Ok(());
    }
    println!("Found {} device(s):", devices.len());
    for dev in &devices {
        println!(
            "  {:15}  {:15}  type={:8}  fw={}  screen={}×{}",
            dev.device_id,
            dev.addr,
            dev.device_type.as_deref().unwrap_or("?"),
            dev.firmware_version.as_deref().unwrap_or("?"),
            dev.screen_width.unwrap_or(0),
            dev.screen_height.unwrap_or(0),
        );
    }
    Ok(())
}

/// Build a BoxHwConfig from a human-readable module description string.
///
/// Preset format: `{pixel_pitch}-{env}-{scan}-{chip}` e.g.:
///   "P10-outdoor-1:4-ICN2038"
///   "P5-indoor-1:8-SM16207"
///   "P3-indoor-static-MBI5153"
fn build_preset_config(desc: &str) -> Option<BoxHwConfig> {
    let desc_lower = desc.to_lowercase();
    let mut card = CardConfig::default();

    // Pixel pitch → cell dimensions (common sizes)
    if desc_lower.contains("p10") {
        card.cell_width = 32; card.cell_height = 16;
    } else if desc_lower.contains("p8") {
        card.cell_width = 32; card.cell_height = 16;
    } else if desc_lower.contains("p6") || desc_lower.contains("p5") {
        card.cell_width = 64; card.cell_height = 32;
    } else if desc_lower.contains("p4") || desc_lower.contains("p3") {
        card.cell_width = 64; card.cell_height = 64;
    } else if desc_lower.contains("p2") {
        card.cell_width = 128; card.cell_height = 64;
    }

    // Scan ratio
    if desc_lower.contains("static") || desc_lower.contains("1:1") {
        card.scan_mode = 0;
    } else if desc_lower.contains("1:2") {
        card.scan_mode = 1;
    } else if desc_lower.contains("1:4") {
        card.scan_mode = 2;
    } else if desc_lower.contains("1:8") {
        card.scan_mode = 3;
    } else if desc_lower.contains("1:16") {
        card.scan_mode = 4;
    } else if desc_lower.contains("1:32") {
        card.scan_mode = 5;
    }

    // Environment (indoor vs outdoor affects refresh rate)
    if desc_lower.contains("outdoor") {
        card.refresh_rate = 960;
    } else if desc_lower.contains("indoor") {
        card.refresh_rate = 3840;
    }

    // Driver chip
    if desc_lower.contains("icn") {
        card.drive_chip_type = 1; // ICN2038 approx
    } else if desc_lower.contains("sm") {
        card.drive_chip_type = 10; // SM16207
    } else if desc_lower.contains("mbi") {
        card.drive_chip_type = 20; // MBI5153
    } else if desc_lower.contains("fm") {
        card.drive_chip_type = 30; // FM6124
    }

    Some(BoxHwConfig {
        card_info: vec![card],
        rgb_ctrl_width: 128,
        rgb_ctrl_height: 64,
        ..Default::default()
    })
}
