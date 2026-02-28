//! hdplayer-client — CLI tool for controlling Huidu BoxPlayer devices.
//!
//! This binary reproduces the core control functionality of Huidu HDPlayer.exe
//! as a cross-platform command-line tool.

use clap::{Parser, Subcommand};
use std::time::Duration;
use tracing_subscriber::EnvFilter;

use hdplayer::{Client, Discovery};
use hdplayer::transfer::FileTransfer;

#[derive(Parser)]
#[command(
    name = "hdplayer",
    about = "Huidu BoxPlayer control client (HDPlayer.exe reproduction)",
    version
)]
struct Cli {
    /// Device IP address. If omitted, discover devices automatically.
    #[arg(short = 'H', long)]
    host: Option<String>,

    /// TCP port (default: 9527 — BoxStream protocol used by HDPlayer.exe)
    #[arg(short = 'p', long, default_value_t = 9527)]
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

    /// Set the NTP server used for automatic time synchronisation
    SetNtpServer {
        /// NTP server hostname or IP (e.g. pool.ntp.org, time.google.com)
        server: String,
    },

    /// Set device timezone (UTC offset in whole hours, e.g. 8 for UTC+8, -5 for UTC-5)
    SetTimezone {
        /// UTC offset hours (-12 to +14)
        #[arg(allow_hyphen_values = true)]
        offset: i8,
    },

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
        /// Active days: 7-char 0/1 string for Mon–Sun (e.g. "1111100" = weekdays only)
        #[arg(long, default_value = "1111111")]
        days: String,
    },

    /// Take a screenshot (saves the current rendered frame as PNG)
    Screenshot {
        /// Output file path
        #[arg(short, long, default_value = "screenshot.png")]
        output: String,
    },

    /// Get current brightness level
    GetBrightness,

    /// Get brightness schedule
    GetLuminancePloy,

    /// Set brightness schedule (one or more HH:MM:level entries)
    SetBrightnessSchedule {
        /// Schedule entries in "HH:MM:LEVEL" format, e.g. "08:00:100 22:00:50"
        #[arg(required = true, num_args = 1..)]
        entries: Vec<String>,
    },

    /// Get device time
    GetTime,

    /// List files stored on the device
    ListFiles,

    /// Show currently playing program GUID
    GetCurrentProgram,

    /// Reboot the device
    Reboot,

    /// Delete orphaned program files from device storage
    Cleanup,

    /// Get device license
    GetLicense,

    /// Set device license
    SetLicense {
        /// License string
        value: String,
    },

    /// List all data source key/value pairs
    GetDataSources,

    /// Set a data source value
    SetDataSource {
        /// Data source name
        name: String,
        /// Data source value
        value: String,
    },

    /// Get current screen rotation
    GetRotation,

    /// Get boot logo filename
    GetBootLogo,

    /// Set boot logo (file must already be on device)
    SetBootLogo {
        /// Filename on device
        filename: String,
    },

    /// Clear boot logo
    ClearBootLogo,

    /// Delete files from device storage
    DeleteFiles {
        /// Filenames to delete
        #[arg(required = true, num_args = 1..)]
        files: Vec<String>,
    },

    /// Get network connectivity status
    GetNetworkInfo,

    /// Get WiFi status
    GetWifiInfo,

    /// Connect to a WiFi network
    SetWifi {
        /// Network SSID
        ssid: String,
        /// Password (use empty string for open networks)
        #[arg(default_value = "")]
        password: String,
    },

    /// Get firmware upgrade result
    GetUpgradeResult,

    /// Trigger firmware upgrade.
    ///
    /// If the argument is a local file path that exists on this machine the file
    /// is uploaded to the device first, then the upgrade is triggered
    /// automatically.  If it is already on the device pass just the filename.
    ///
    /// After triggering, the command polls GetUpgradeResult until the device
    /// reports success/failure or the connection drops (indicating a reboot).
    FirmwareUpgrade {
        /// Local path to the .zbin file OR the filename already on the device.
        filename: String,
        /// Do not upload — assume the file is already on the device.
        #[arg(long)]
        no_upload: bool,
        /// Seconds between GetUpgradeResult polls (default 5).
        #[arg(long, default_value_t = 5)]
        poll_interval: u64,
        /// Maximum seconds to wait for completion before giving up (default 120).
        #[arg(long, default_value_t = 120)]
        timeout: u64,
    },

    /// Firmware upgrade using the native port-9528 binary protocol.
    ///
    /// Uploads the firmware archive to the device via the same protocol as
    /// HDPlayer.exe: connect on port 9528, stream the file in 9212-byte chunks,
    /// then trigger extraction and execution.  This is independent of the
    /// SDK XML protocol used by `firmware-upgrade`.
    ///
    /// The firmware archive is typically named `Box.tar.gz`.
    UpgradeNative {
        /// Local path to the firmware archive (e.g. Box.tar.gz).
        file: String,
        /// Seconds between completion polls after the upgrade executes.
        #[arg(long, default_value_t = 5)]
        poll_interval: u64,
        /// Maximum seconds to wait for the device to report completion.
        #[arg(long, default_value_t = 600)]
        timeout: u64,
        /// TCP port for the binary upgrade protocol (default 9528).
        /// Use 10001 for legacy devices that only have port 10001 open.
        #[arg(long, default_value_t = 9528)]
        upgrade_port: u16,
    },

    /// Get file list with MD5 hashes and sizes (for verifying transfer integrity)
    Checklist,

    /// Set Ethernet configuration
    SetNetwork {
        /// Enable DHCP (mutually exclusive with --ip)
        #[arg(long, conflicts_with = "ip")]
        dhcp: bool,
        /// Static IP address (required if --dhcp is not set)
        #[arg(long, default_value = "")]
        ip: String,
        /// Subnet mask
        #[arg(long, default_value = "255.255.255.0")]
        mask: String,
        /// Default gateway
        #[arg(long, default_value = "")]
        gateway: String,
        /// DNS server
        #[arg(long, default_value = "8.8.8.8")]
        dns: String,
    },

    /// Get PPPoE connection information
    GetPppoe,

    /// Configure PPPoE connection
    SetPppoe {
        /// Enable PPPoE
        #[arg(long)]
        enable: bool,
        /// PPPoE username
        #[arg(long, default_value = "")]
        user: String,
        /// PPPoE password
        #[arg(long, default_value = "")]
        password: String,
    },

    /// Unlock admin mode using the device password
    UnlockAdmin {
        /// Admin password
        password: String,
    },

    /// Set the admin unlock password (empty string removes the password)
    SetAdminPassword {
        /// New password (omit or leave empty to clear)
        #[arg(default_value = "")]
        password: String,
    },

    /// Show a color-bar test pattern on the display for 10 seconds
    ScreenTest,

    /// Get SDK protocol version from device
    SdkVersion,

    /// Update (replace) an existing program on the device
    UpdateProgram {
        /// Path to program XML (.boo) file
        file: String,
    },

    /// Get device play status (stopped / playing)
    GetPlayStatus,

    /// Get current audio volume
    GetVolume,

    /// Get ambient sensor info (temperature, humidity, GPS, etc.)
    GetSensorInfo,

    /// Get GPS coordinates
    GetGpsInfo,

    /// Get current temperature from sensor
    GetTemperature,

    /// Get current humidity from sensor
    GetHumidity,

    /// Get relay status list
    GetRelay,

    /// Get key definition value
    GetKeyDefine,

    /// Get sensor measurement range (kUnsupportMethod on most devices)
    GetSensorRange,

    /// Get device locker enable status
    GetLockerEnable,

    /// Get admin mode status (enabled / locker state)
    GetAdminInfo,

    /// Toggle a relay output on or off
    SetRelay {
        /// 0-based relay index
        index: u8,
        /// on or off
        #[arg(value_parser = parse_relay_state)]
        state: bool,
    },

    /// Remove the device license key
    ClearLicense,

    /// Request automatic FPGA/hardware configuration for the attached LED module
    SmartSetting,

    /// Persist the current FPGA/BoxHwConfig to flash storage
    SaveHwConfig,

    /// Get live sensor readings (temperature, humidity, luminance, etc.)
    GetCurrentSensor,
}

fn parse_relay_state(s: &str) -> Result<bool, String> {
    match s.to_ascii_lowercase().as_str() {
        "on" | "1" | "true" => Ok(true),
        "off" | "0" | "false" => Ok(false),
        _ => Err(format!("expected 'on'/'off' or '1'/'0', got '{s}'")),
    }
}

/// Query a device's running firmware version string.
///
/// Strategy (in order):
/// 1. UDP discovery — check `firmware_version` field (parsed from `SoftwareVersion` attr).
///    If absent, also search the raw `info_xml` for alternative attribute names used by
///    older BoxPlayer firmware (`AppVersion`, `BoxPlayerVersion`, `Version`).
/// 2. BoxStream SDK (port 9527) — `get_device_info().firmware_version`.
///
/// Returns an empty string if the version cannot be determined.
async fn query_device_firmware_version(host: &str) -> String {
    // 1. Direct port 9528 management login — fastest path, works even when port 9527
    //    rejects BoxStreamInit (e.g. firmware 7.4.59.0).
    if let Ok(v) = hdplayer::client::firmware_version_via_9528(host).await {
        if !v.is_empty() {
            return v;
        }
    }
    // 2. UDP discovery fallback
    if let Ok(devs) = Discovery::scan(Duration::from_secs(4)).await {
        if let Some(d) = devs.into_iter().find(|d| d.addr.to_string() == host) {
            if let Some(v) = d.firmware_version {
                return v;
            }
            for attr in &["AppVersion", "BoxPlayerVersion", "Version", "SWVersion", "FirmwareVer"] {
                if let Some(v) = hdplayer::xml::get_attr(&d.info_xml, attr) {
                    return v.to_string();
                }
            }
        }
    }
    // 3. BoxStream SDK last resort
    if let Ok(mut c) = Client::connect(host, 9527).await {
        if let Ok(info) = c.get_device_info().await {
            if !info.firmware_version.is_empty() {
                return info.firmware_version;
            }
        }
    }
    String::new()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // When RUST_LOG is set, respect it fully. Otherwise default to INFO.
    let filter = if std::env::var("RUST_LOG").is_ok() {
        EnvFilter::from_default_env()
    } else {
        EnvFilter::new("hdplayer=info,huidu_protocol=info")
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
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
                if let Some(r) = dev.rotation {
                    if r != 0 { println!("       Rotation: {r}°"); }
                }
                if let Some(b) = dev.brightness {
                    println!("       Brightness: {b}%");
                }
                if let Some(on) = dev.screen_on {
                    println!("       Screen: {}", if on { "ON" } else { "OFF" });
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

    // UpgradeNative connects directly to the binary upgrade port — no SDK client needed.
    if let Commands::UpgradeNative { file, poll_interval, timeout, upgrade_port } = &cli.command {
        let path = std::path::Path::new(file.as_str());
        if !path.exists() {
            eprintln!("File not found: {file}");
            std::process::exit(1);
        }

        // ── Version: firmware file ────────────────────────────────────────────
        let raw = tokio::fs::read(path).await?;
        let fw_version = hdplayer::upgrade::firmware_file_version(&raw)
            .unwrap_or_else(|| "unknown".to_string());
        drop(raw); // release 330 MB before the upgrade re-reads the file
        println!("Firmware file version:  {fw_version}");

        // ── Version: device before upgrade ───────────────────────────────────
        let before_version = query_device_firmware_version(&host).await;
        if before_version.is_empty() {
            println!("Device version before:  (unavailable)");
        } else {
            println!("Device version before:  {before_version}");
        }
        if !before_version.is_empty() && before_version == fw_version {
            println!("Note: device already reports {fw_version} — upgrade may be a no-op.");
        }

        println!("Starting native firmware upgrade: {}", path.display());
        println!("  Device: {host}:{upgrade_port}");
        let opts = hdplayer::upgrade::UpgradeOptions {
            poll_interval: Duration::from_secs(*poll_interval),
            poll_timeout: Duration::from_secs(*timeout),
            progress: Some(Box::new(|sent, total| {
                print!(
                    "\r  {sent}/{total} bytes ({:.1}%)  ",
                    sent as f64 / total as f64 * 100.0
                );
                let _ = std::io::Write::flush(&mut std::io::stdout());
            })),
            phase: Some(Box::new(|msg: &str| {
                println!("\n  [{msg}]");
            })),
        };
        let host_owned = host.clone();
        let path_owned = path.to_path_buf();
        let upgrade_port = *upgrade_port;
        std::thread::Builder::new()
            .stack_size(64 * 1024 * 1024)
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("tokio runtime")
                    .block_on(hdplayer::upgrade::run_upgrade(&host_owned, upgrade_port, &path_owned, opts))
            })?
            .join()
            .map_err(|_| anyhow::anyhow!("upgrade thread panicked"))??;

        // ── Version: device after upgrade ────────────────────────────────────
        println!("\nWaiting for device to come back online…");
        // The upgrade script (upgrade.sh) on some devices uses SIGHUP to signal
        // BoxDaemon (which on this device causes a config-reload, not a restart).
        // BoxDaemon then closes the upgrade TCP connection and reports the OLD
        // firmware version while upgrade.sh continues copying files in the
        // background.  A second OS-level reboot follows ~30-90 s later when
        // upgrade.sh calls `reboot`.  We must keep polling until the firmware
        // version CHANGES (or until we're confident the upgrade failed).
        // Strategy:
        //   - If the device responds with the SAME version as before, keep waiting.
        //   - Only declare success when it reports the NEW version.
        //   - After seeing the old version for 20+ minutes with no change, declare failure.
        //     write_fpga on this device can take ~13 minutes while BoxDaemon stays alive.
        let reconnect_deadline = std::time::Instant::now() + Duration::from_secs(2400);
        let mut old_version_since: Option<std::time::Instant> = None;
        let after_version = loop {
            if std::time::Instant::now() > reconnect_deadline {
                println!("  Timed out — device did not respond within 25 min.");
                break String::new();
            }
            tokio::time::sleep(Duration::from_secs(15)).await;
            let v = query_device_firmware_version(&host).await;
            if v.is_empty() {
                // Device offline — rebooting or flashing; reset the "stuck at old" timer
                old_version_since = None;
                println!("  Device offline — waiting for reboot to complete…");
                continue;
            }
            if v == fw_version {
                println!("  Device is back online (firmware: {v})");
                break v;
            }
            if !before_version.is_empty() && v == before_version {
                // Device is online but still reporting old firmware.
                // This is expected during the two-stage upgrade: BoxDaemon
                // reloads via SIGHUP while upgrade.sh copies files in the
                // background, then calls `reboot` for the real OS reboot.
                // Keep polling; only give up after 20 min of unchanged version.
                // write_fpga on this device can take ~13 min while BoxDaemon stays alive.
                let since = old_version_since.get_or_insert_with(std::time::Instant::now);
                let elapsed = since.elapsed().as_secs();
                println!("  Device reports old firmware ({v}) — upgrade in progress, {elapsed}s so far…");
                if elapsed >= 1200 {
                    println!("  Old firmware for 20+ min — upgrade did not apply.");
                    break v;
                }
                continue;
            }
            // Some other (unexpected) version
            println!("  Device is back online (firmware: {v})");
            break v;
        };

        println!("─────────────────────────────────────────");
        println!("Firmware file version:  {fw_version}");
        if before_version.is_empty() {
            println!("Device version before:  unknown");
        } else {
            println!("Device version before:  {before_version}");
        }
        if after_version.is_empty() {
            println!("Device version after:   unknown");
        } else {
            println!("Device version after:   {after_version}");
        }
        if !after_version.is_empty() && !before_version.is_empty() {
            if after_version == before_version {
                println!("RESULT: UNCHANGED — upgrade did not apply.");
            } else if after_version == fw_version {
                println!("RESULT: SUCCESS — device updated to {after_version}.");
            } else {
                println!("RESULT: version changed to {after_version} (expected {fw_version}).");
            }
        }
        return Ok(());
    }

    // Auto-select protocol: port 9527 → BoxStream; anything else → legacy SdkService (port 10001)
    let mut client = if cli.port == 9527 {
        Client::connect(&host, cli.port).await?
    } else {
        Client::connect_legacy(&host, cli.port).await?
    };
    println!("Connected to {host}:{}", cli.port);

    match &cli.command {
        Commands::Discover { .. } => unreachable!(),

        Commands::Info => {
            let info = client.get_device_info().await?;
            println!("Device ID:    {}", info.device_id);
            println!("Name:         {}", info.device_name);
            println!("Type:         {}", info.device_type);
            println!("Firmware:     {}", info.firmware_version);
            if !info.fpga_version.is_empty() {
                println!("FPGA:         {}", info.fpga_version);
            }
            println!("Screen:       {}x{}", info.screen_width, info.screen_height);
            println!("Rotation:     {}°", info.rotation);
            println!("IP:           {}", info.ip_address);
            println!("MAC:          {}", info.mac_address);
            println!("DHCP:         {}", if info.dhcp { "enabled" } else { "disabled" });
            println!("Brightness:   {}%", info.brightness);
            println!("Volume:       {}%", info.volume);
            println!("Play status:  {}", match info.play_status { 1 => "playing", _ => "stopped" });
            if !info.device_time.is_empty() {
                println!("Device time:  {}", info.device_time);
            }
            if !info.ntp_server.is_empty() {
                println!("NTP server:   {}", info.ntp_server);
            }
            println!("Screen sched: {}", if info.switch_time_on { "on" } else { "off" });
            if info.boot_logo_exists {
                println!("Boot logo:    set");
            }
            if info.storage_total > 0 {
                let used = info.storage_total.saturating_sub(info.storage_free);
                println!("Storage:      {:.1} GB used / {:.1} GB total",
                    used as f64 / 1e9, info.storage_total as f64 / 1e9);
            }
            if info.admin_mode {
                println!("Admin mode:   enabled");
            }
        }

        Commands::HwInfo => {
            let xml = client.get_hardware_info().await?;
            // Pretty-print the <hardware .../> element attributes
            if let Some(start) = xml.find("<hardware ") {
                let end = xml[start..].find("/>").map(|e| start + e + 2).unwrap_or(xml.len());
                let tag = &xml[start..end];
                let attrs = ["cpu", "os", "ram", "storage", "cpuUsage", "memUsage", "temperature"];
                let labels = ["CPU arch", "OS", "RAM (MB)", "Storage (MB)", "CPU usage %", "Mem usage %", "Temperature °C"];
                for (attr, label) in attrs.iter().zip(labels.iter()) {
                    if let Some(val) = hdplayer::xml::get_attr(tag, attr) {
                        println!("{:<16} {}", format!("{}:", label), val);
                    }
                }
            } else {
                println!("{xml}");
            }
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

        Commands::SetNtpServer { server } => {
            client.set_ntp_server(server).await?;
            println!("NTP server set to '{server}'.");
        }

        Commands::SetTimezone { offset } => {
            client.set_timezone(*offset).await?;
            println!("Timezone set to UTC{:+}.", offset);
        }

        Commands::GetNetwork => {
            let cfg = client.get_eth0_info().await?;
            println!("DHCP:    {}", if cfg.dhcp { "enabled" } else { "disabled" });
            println!("IP:      {}", cfg.ip);
            println!("Mask:    {}", cfg.mask);
            println!("Gateway: {}", cfg.gateway);
            println!("DNS:     {}", cfg.dns);
        }

        Commands::GetFpgaConfig => {
            let xml = client.get_fpga_config().await?;
            // Extract known BoxHwConfig / SDKFPGAConfig attributes; fall back to raw XML
            // if the device hasn't been configured yet (empty or unrecognised format).
            let elem_attr = |elem: &str, attr: &str| -> Option<String> {
                let tag = format!("<{elem}");
                xml.find(&tag)
                    .and_then(|pos| hdplayer::xml::get_attr(&xml[pos..], attr))
                    .map(str::to_string)
            };
            let mut printed = false;
            if let (Some(w), Some(h)) = (elem_attr("Card", "width"), elem_attr("Card", "height")) {
                println!("Screen:      {w}×{h}");
                printed = true;
            }
            if let Some(b) = elem_attr("Brightness", "value") {
                println!("Brightness:  {b}%");
                printed = true;
            }
            if let Some(rr) = elem_attr("RefreshRate", "value") {
                println!("Refresh:     {rr} Hz");
                printed = true;
            }
            if let Some(scan) = elem_attr("ScanType", "value").or_else(|| elem_attr("Scan", "type")) {
                println!("Scan type:   {scan}");
                printed = true;
            }
            if !printed {
                // No known structure recognised — show raw XML so the user can see what's stored
                println!("{xml}");
            }
        }

        Commands::GetSchedule => {
            let xml = client.get_switch_time().await?;
            // Response contains <item onTime="HH:MM" offTime="HH:MM" days="1111111"/> elements
            let mut found = false;
            let mut search = xml.as_str();
            while let Some(start) = search.find("<item ") {
                let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
                let tag = &search[start..end];
                let on   = hdplayer::xml::get_attr(tag, "onTime").unwrap_or("?");
                let off  = hdplayer::xml::get_attr(tag, "offTime").unwrap_or("?");
                let days = hdplayer::xml::get_attr(tag, "days").unwrap_or("?");
                println!("ON: {on}  OFF: {off}  Days: {days}");
                found = true;
                search = &search[end.min(search.len())..];
            }
            if !found {
                println!("No schedule set (screen always on).");
            }
        }

        Commands::SetSchedule { enable, on, off, days } => {
            if *enable {
                client.set_switch_time(&[(on.as_str(), off.as_str(), days.as_str())]).await?;
                println!("Schedule set: on={on} off={off} days={days}");
            } else {
                client.set_switch_time(&[]).await?;
                println!("Schedule cleared.");
            }
        }

        Commands::Screenshot { output } => {
            let data = client.screenshot().await?;
            tokio::fs::write(output, &data).await?;
            println!("Screenshot saved to {output} ({} bytes)", data.len());
        }

        Commands::GetBrightness => {
            let level = client.get_brightness().await?;
            println!("Brightness: {level}%");
        }

        Commands::GetLuminancePloy => {
            let xml = client.get_luminance_ploy().await?;
            // Try to pretty-print the schedule
            if xml.contains("mode=\"auto\"") {
                println!("Brightness schedule (auto mode):");
                let mut search = xml.as_str();
                let mut found = false;
                while let Some(start) = search.find("<item ") {
                    let end = search[start..].find("/>").map(|e| start + e + 2).unwrap_or(search.len());
                    let tag = &search[start..end];
                    let time  = hdplayer::xml::get_attr(tag, "time").unwrap_or("?");
                    let level = hdplayer::xml::get_attr(tag, "level").unwrap_or("?");
                    println!("  {time} → {level}%");
                    search = &search[end.min(search.len())..];
                    found = true;
                }
                if !found { println!("  (empty schedule)"); }
            } else {
                let level = hdplayer::xml::get_attr(&xml, "value").unwrap_or("?");
                println!("Brightness: {level}% (manual mode)");
            }
        }

        Commands::SetBrightnessSchedule { entries } => {
            // Parse "HH:MM:LEVEL" strings
            let mut parsed: Vec<(u8, u8, u8)> = Vec::new();
            for e in entries {
                let parts: Vec<&str> = e.split(':').collect();
                if parts.len() == 3 {
                    if let (Ok(h), Ok(m), Ok(l)) = (
                        parts[0].parse::<u8>(),
                        parts[1].parse::<u8>(),
                        parts[2].parse::<u8>(),
                    ) {
                        parsed.push((h, m, l));
                        continue;
                    }
                }
                eprintln!("Invalid entry '{}' — expected HH:MM:LEVEL", e);
                std::process::exit(1);
            }
            client.set_luminance_ploy(&parsed).await?;
            println!("Brightness schedule set ({} entries).", parsed.len());
        }

        Commands::GetTime => {
            let xml = client.get_time_info().await?;
            let time = hdplayer::xml::get_attr(&xml, "value").unwrap_or("?");
            let ntp  = hdplayer::xml::get_attr(&xml, "server").unwrap_or("");
            let tz   = hdplayer::xml::get_attr(&xml, "timezone").unwrap_or("0");
            println!("Device time: {time}");
            println!("UTC offset:  UTC{:+}", tz.parse::<i8>().unwrap_or(0));
            if !ntp.is_empty() { println!("NTP server:  {ntp}"); }
        }

        Commands::ListFiles => {
            let files = client.list_files().await?;
            if files.is_empty() {
                println!("No files on device.");
            } else {
                println!("Files ({}):", files.len());
                for f in &files {
                    println!("  {f}");
                }
            }
        }

        Commands::GetCurrentProgram => {
            let guid = client.get_current_program_guid().await?;
            if guid.is_empty() {
                println!("No program currently playing.");
            } else {
                println!("Current program GUID: {guid}");
            }
        }

        Commands::Reboot => {
            client.reboot().await?;
            println!("Reboot initiated.");
        }

        Commands::Cleanup => {
            client.cleanup().await?;
            println!("Storage cleanup complete.");
        }

        Commands::GetLicense => {
            let (lic, valid) = client.get_license().await?;
            if lic.is_empty() {
                println!("No license set.");
            } else {
                println!("License: {lic}");
                println!("Valid:   {valid}");
            }
        }

        Commands::SetLicense { value } => {
            client.set_license(value).await?;
            println!("License set.");
        }

        Commands::GetDataSources => {
            let entries = client.get_data_sources().await?;
            if entries.is_empty() {
                println!("No data sources configured.");
            } else {
                println!("Data sources ({}):", entries.len());
                for (name, value) in &entries {
                    println!("  {name} = {value}");
                }
            }
        }

        Commands::SetDataSource { name, value } => {
            client.set_data_source(name, value).await?;
            println!("Data source '{name}' set to '{value}'.");
        }

        Commands::GetRotation => {
            let degrees = client.get_rotation().await?;
            println!("Rotation: {degrees}°");
        }

        Commands::GetBootLogo => {
            let name = client.get_boot_logo().await?;
            if name.is_empty() {
                println!("No boot logo set.");
            } else {
                println!("Boot logo: {name}");
            }
        }

        Commands::SetBootLogo { filename } => {
            client.set_boot_logo(filename).await?;
            println!("Boot logo set to '{filename}'.");
        }

        Commands::ClearBootLogo => {
            client.clear_boot_logo().await?;
            println!("Boot logo cleared.");
        }

        Commands::DeleteFiles { files } => {
            let refs: Vec<&str> = files.iter().map(|s| s.as_str()).collect();
            client.delete_files(&refs).await?;
            println!("Deleted {} file(s).", refs.len());
        }

        Commands::GetNetworkInfo => {
            let xml = client.get_network_info().await?;
            // Response: <network eth0Connected="true" wifiConnected="false" internet="true" ip="..."/>
            let get  = |a: &str| hdplayer::xml::get_attr(&xml, a).unwrap_or("?");
            let yesno = |v: &str| if v == "true" { "yes" } else { "no" };
            println!("Ethernet:  {}", yesno(get("eth0Connected")));
            println!("WiFi:      {}", yesno(get("wifiConnected")));
            println!("Internet:  {}", yesno(get("internet")));
            let ip = get("ip");
            if ip != "?" && !ip.is_empty() {
                println!("IP:        {ip}");
            }
        }

        Commands::GetWifiInfo => {
            let xml = client.get_wifi_info().await?;
            let ssid  = hdplayer::xml::get_attr(&xml, "ssid").unwrap_or("(none)");
            let state = hdplayer::xml::get_attr(&xml, "state").unwrap_or("unknown");
            println!("WiFi SSID:  {ssid}");
            println!("WiFi state: {state}");
        }

        Commands::SetWifi { ssid, password } => {
            client.set_wifi(ssid, password).await?;
            println!("Connecting to WiFi '{ssid}'…");
        }

        Commands::GetUpgradeResult => {
            let result = client.get_upgrade_result().await?;
            println!("Upgrade result: {}", if result.is_empty() { "(none)" } else { &result });
        }

        Commands::FirmwareUpgrade { filename, no_upload, poll_interval, timeout } => {
            // Device stores files by their basename only.
            let device_filename = std::path::Path::new(filename)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(filename.as_str())
                .to_string();

            // Upload if the file exists locally and --no-upload was not set.
            if !no_upload && std::path::Path::new(filename).exists() {
                let file_size = std::fs::metadata(filename).map(|m| m.len()).unwrap_or(0);
                println!("Uploading '{device_filename}' ({file_size} bytes) to device...");
                let transfer = FileTransfer::from_file(filename).await?;
                let total = transfer.total_size();
                client.upload_file(&transfer, Some(&|sent: u64, sz: u64| {
                    print!("\r  {sent}/{sz} bytes ({:.1}%)", sent as f64 / sz as f64 * 100.0);
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                })).await?;
                println!("\n  Upload complete ({total} bytes).");
            } else if !no_upload {
                println!("'{filename}' not found locally — assuming file is already on device.");
            }

            // Trigger the upgrade.
            println!("Triggering upgrade from '{device_filename}'...");
            client.firmware_upgrade(&device_filename).await?;
            println!("Upgrade initiated. Polling every {poll_interval}s (timeout {timeout}s)...");

            let start = std::time::Instant::now();
            let deadline = start + Duration::from_secs(*timeout);
            let mut rebooted = false;

            loop {
                if std::time::Instant::now() > deadline {
                    eprintln!("Timed out after {timeout}s waiting for upgrade to complete.");
                    std::process::exit(1);
                }

                tokio::time::sleep(Duration::from_secs(*poll_interval)).await;
                let elapsed = start.elapsed().as_secs();

                match client.get_upgrade_result().await {
                    Ok(msg) => {
                        println!("  [{elapsed}s] {msg}");
                        if msg.starts_with("success") {
                            println!("Firmware upgrade completed successfully.");
                            break;
                        } else if msg.starts_with("failed") {
                            eprintln!("Firmware upgrade failed: {msg}");
                            std::process::exit(1);
                        }
                        // "upgrading: ..." — still in progress, keep polling.
                    }
                    Err(e) => {
                        if !rebooted {
                            println!("  [{elapsed}s] Connection lost ({e}) — device is rebooting...");
                            rebooted = true;
                        }
                        // Try to reconnect; if it fails we keep trying next iteration.
                        let reconnect = if cli.port == 9527 {
                            Client::connect(&host, cli.port).await
                        } else {
                            Client::connect_legacy(&host, cli.port).await
                        };
                        if let Ok(new_client) = reconnect {
                            client = new_client;
                            println!("  [{elapsed}s] Reconnected to device.");
                        }
                    }
                }
            }
        }

        Commands::UpgradeNative { .. } => unreachable!(),

        Commands::Checklist => {
            let files = client.get_file_checklist().await?;
            if files.is_empty() {
                println!("No files on device.");
            } else {
                println!("Files ({}):", files.len());
                println!("{:<40} {:>10}  MD5", "Name", "Size");
                println!("{}", "-".repeat(80));
                for f in &files {
                    println!("{:<40} {:>10}  {}", f.name, f.size, f.md5);
                }
            }
        }

        Commands::SetNetwork { dhcp, ip, mask, gateway, dns } => {
            if !dhcp && ip.is_empty() {
                eprintln!("Error: --ip is required for static configuration (or use --dhcp)");
                std::process::exit(1);
            }
            let cfg = hdplayer::EthConfig {
                dhcp: *dhcp,
                ip: ip.clone(),
                mask: mask.clone(),
                gateway: gateway.clone(),
                dns: dns.clone(),
            };
            client.set_eth0_info(&cfg).await?;
            if *dhcp {
                println!("Network set to DHCP.");
            } else {
                println!("Network set: IP={ip}/{mask}  gateway={gateway}  DNS={dns}");
            }
        }

        Commands::GetPppoe => {
            let xml = client.get_pppoe_info().await?;
            let get = |a: &str| hdplayer::xml::get_attr(&xml, a).unwrap_or("?");
            println!("PPPoE enabled: {}", get("enable"));
            println!("User:          {}", get("user"));
            println!("Status:        {}", get("status"));
        }

        Commands::SetPppoe { enable, user, password } => {
            client.set_pppoe_info(*enable, user, password).await?;
            if *enable {
                println!("PPPoE enabled for user '{user}'.");
            } else {
                println!("PPPoE disabled.");
            }
        }

        Commands::UnlockAdmin { password } => {
            let ok = client.unlock_admin_password(password).await?;
            if ok {
                println!("Admin mode unlocked.");
            } else {
                eprintln!("Wrong password — admin mode not unlocked.");
                std::process::exit(1);
            }
        }

        Commands::SetAdminPassword { password } => {
            client.set_admin_password(password).await?;
            if password.is_empty() {
                println!("Admin password cleared (no password required).");
            } else {
                println!("Admin password updated.");
            }
        }

        Commands::ScreenTest => {
            client.screen_test().await?;
            println!("Color-bar test pattern started (10 seconds).");
        }

        Commands::SdkVersion => {
            let ver = client.get_sdk_version().await?;
            println!("SDK version: {ver}");
        }

        Commands::UpdateProgram { file } => {
            let content = tokio::fs::read_to_string(file).await?;
            let screen_xml = if content.contains("<screen") {
                let start = content.find("<screen").unwrap_or(0);
                let end = content.rfind("</screen>")
                    .map(|i| i + "</screen>".len())
                    .unwrap_or(content.len());
                content[start..end].to_string()
            } else {
                content
            };
            client.update_program(&screen_xml).await?;
            println!("Program updated successfully.");
        }

        Commands::GetPlayStatus => {
            let status = client.get_play_status().await?;
            println!("Play status: {}", match status { 1 => "playing", _ => "stopped" });
        }

        Commands::GetVolume => {
            let vol = client.get_volume().await?;
            println!("Volume: {vol}%");
        }

        Commands::GetSensorInfo => {
            let xml = client.get_sensor_info().await?;
            // Parse each sensor's connect enable status
            for sensor in &["luminance", "temperature", "fpgaTemp", "humity", "gps",
                             "windSpeed", "windDirection", "noise", "pressure"] {
                if let Some(pos) = xml.find(&format!("<{sensor}")) {
                    let slice = &xml[pos..];
                    let connected = slice.find("<connect ")
                        .and_then(|p| hdplayer::xml::get_attr(&slice[p..], "enable"))
                        .map(|v| v == "1")
                        .unwrap_or(false);
                    println!("{:<16} {}", format!("{}:", sensor),
                        if connected { "connected" } else { "not connected" });
                }
            }
        }

        Commands::GetGpsInfo => {
            let xml = client.get_gps_info().await?;
            let lat  = hdplayer::xml::get_attr(&xml, "value").unwrap_or("-1");
            let lon  = hdplayer::xml::get_attr(&xml, "value").unwrap_or("-1");
            let cnts = {
                let p = xml.find("<counts ").unwrap_or(0);
                hdplayer::xml::get_attr(&xml[p..], "value").unwrap_or("0")
            };
            if lat == "-1" && lon == "-1" {
                println!("GPS: no fix (satellite count: {cnts})");
            } else {
                println!("Latitude:  {lat}");
                println!("Longitude: {lon}");
                println!("Satellites:{cnts}");
            }
        }

        Commands::GetTemperature => {
            let t = client.get_current_temperature().await?;
            if t == -1 {
                println!("Temperature: no sensor connected");
            } else {
                println!("Temperature: {t}°C");
            }
        }

        Commands::GetHumidity => {
            let h = client.get_current_humidity().await?;
            if h == -1 {
                println!("Humidity: no sensor connected");
            } else {
                println!("Humidity: {h}%");
            }
        }

        Commands::GetRelay => {
            let relays = client.get_relay().await?;
            if relays.is_empty() {
                println!("No relay items.");
            } else {
                println!("Relays ({}):", relays.len());
                for (i, (use_sw, name, status)) in relays.iter().enumerate() {
                    let name_str = if name.is_empty() { "(unnamed)" } else { name.as_str() };
                    println!("  [{}] {} — enabled:{} status:{}", i, name_str, use_sw, status);
                }
            }
        }

        Commands::GetKeyDefine => {
            let key = client.get_key_define().await?;
            println!("Key define: {key}");
        }

        Commands::GetSensorRange => {
            match client.get_sensor_range().await? {
                Some(xml) => println!("{xml}"),
                None => println!("GetSensorRange: not supported by this device."),
            }
        }

        Commands::GetLockerEnable => {
            let enabled = client.get_device_locker_enable().await?;
            println!("Device locker: {}", if enabled { "enabled" } else { "disabled" });
        }

        Commands::GetAdminInfo => {
            let (enabled, locker) = client.get_admin_mode_info().await?;
            println!("Admin mode: {}", if enabled { "enabled" } else { "disabled" });
            println!("Locker:     {}", if locker { "on" } else { "off" });
        }

        Commands::SetRelay { index, state } => {
            client.set_relay_status(*index, *state).await?;
            println!("Relay {} set to {}.", index, if *state { "ON" } else { "OFF" });
        }

        Commands::ClearLicense => {
            client.clear_license().await?;
            println!("License cleared.");
        }

        Commands::SmartSetting => {
            client.smart_setting().await?;
            println!("Smart setting applied.");
        }

        Commands::SaveHwConfig => {
            client.save_box_hw_config().await?;
            println!("Hardware config saved to flash.");
        }

        Commands::GetCurrentSensor => {
            let xml = client.get_current_sensor_value().await?;
            if xml.is_empty() || xml.contains("kUnsupportMethod") {
                println!("GetCurrentSensorValue: not supported by this device.");
            } else {
                // Try to print known sensor values
                let get = |tag: &str, attr: &str| -> Option<String> {
                    xml.find(&format!("<{tag}"))
                        .and_then(|p| hdplayer::xml::get_attr(&xml[p..], attr))
                        .map(str::to_string)
                };
                if let Some(v) = get("temperature", "value") { println!("Temperature: {v}°C"); }
                if let Some(v) = get("humidity", "value")    { println!("Humidity:    {v}%"); }
                if let Some(v) = get("luminance", "value")   { println!("Luminance:   {v}"); }
                if let Some(v) = get("noise", "value")       { println!("Noise:       {v} dB"); }
                if let Some(v) = get("pressure", "value")    { println!("Pressure:    {v} hPa"); }
                if let Some(v) = get("windSpeed", "value")   { println!("Wind speed:  {v} m/s"); }
                // Fallback: print raw XML if none of the above matched
                if !xml.contains("value=") {
                    println!("{xml}");
                }
            }
        }
    }

    Ok(())
}
