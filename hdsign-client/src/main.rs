//! hdsign — CLI tool for controlling Huidu HDSign LED sign cards.

use clap::{Parser, Subcommand};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

use hdsign::{HdsignClient, Discovery};
use hdsign::client::DEFAULT_PORT;

#[derive(Parser)]
#[command(
    name = "hdsign",
    about = "Huidu HDSign LED sign control client",
    version
)]
struct Cli {
    /// Device IP address (Ethernet mode). Auto-discovers if omitted.
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// TCP port (default: 10001, Ethernet mode)
    #[arg(short = 'p', long, default_value_t = DEFAULT_PORT)]
    port: u16,

    /// WiFi AP device IP (default: 192.168.4.1)
    #[arg(long, default_value = hdsign::WIFI_AP_DEFAULT_IP)]
    wifi_ip: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover Huidu devices on the local network (Ethernet mode)
    Discover {
        #[arg(short, long, default_value_t = 3)]
        timeout: u64,
    },

    /// Get device information (Ethernet mode)
    Info,

    /// Set display brightness (0–100)
    Brightness { level: u8 },

    /// Turn screen on
    ScreenOn,

    /// Turn screen off
    ScreenOff,

    /// Reboot device
    Reboot,

    /// Run color-bar screen test
    ScreenTest,

    /// Configure WiFi credentials (WiFi AP mode)
    SetWifi {
        ssid: String,
        #[arg(default_value = "")]
        password: String,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env()
            .add_directive("hdsign=info".parse()?))
        .init();

    // WiFi commands use HTTP client
    if let Commands::SetWifi { ssid, password } = &cli.command {
        let client = HdsignClient::connect_wifi_ap(&cli.wifi_ip);
        client.set_wifi_credentials(ssid, password).await?;
        println!("WiFi credentials sent to device at {}", cli.wifi_ip);
        return Ok(());
    }

    // Discover command
    if let Commands::Discover { timeout } = &cli.command {
        let devs = Discovery::scan(Duration::from_secs(*timeout)).await?;
        if devs.is_empty() { println!("No devices found."); }
        for (i, d) in devs.iter().enumerate() {
            println!("[{i}] {} (ID: {}) @ {}", d.name, d.device_id, d.addr);
        }
        return Ok(());
    }

    // All other commands: Ethernet TCP
    let host = match &cli.host {
        Some(h) => h.clone(),
        None => {
            let devs = Discovery::scan(Duration::from_secs(3)).await?;
            match devs.first() {
                Some(d) => { println!("Using device: {} @ {}", d.name, d.addr); d.addr.to_string() }
                None => { eprintln!("No devices found. Use --host."); std::process::exit(1); }
            }
        }
    };

    let mut client = HdsignClient::connect_ethernet(&host, cli.port).await?;

    match &cli.command {
        Commands::Info => {
            let info = client.get_device_info().await?;
            println!("Name:     {}", info.device_name);
            println!("Firmware: {}", info.firmware_version);
            println!("Screen:   {}x{}", info.screen_width, info.screen_height);
            println!("IP:       {}", info.ip_address);
        }
        Commands::Brightness { level } => {
            client.set_brightness(*level).await?;
            println!("Brightness set to {level}%");
        }
        Commands::ScreenOn  => { client.screen_on().await?;  println!("Screen on."); }
        Commands::ScreenOff => { client.screen_off().await?; println!("Screen off."); }
        Commands::Reboot    => { client.reboot().await?;     println!("Reboot initiated."); }
        Commands::ScreenTest => { client.screen_test().await?; println!("Screen test started."); }
        Commands::Discover { .. } | Commands::SetWifi { .. } => unreachable!(),
    }

    Ok(())
}
