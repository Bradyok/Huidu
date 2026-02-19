/// USB disk program loading service.
/// Watches for USB drives containing program files and loads them.
use std::path::{Path, PathBuf};
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::{debug, info, warn};

use crate::core::player::PlayerCommand;
use crate::program::parser;

pub struct UsbDiskService;

impl UsbDiskService {
    /// Watch for USB drives with program files
    pub async fn run(player_tx: mpsc::Sender<PlayerCommand>, program_dir: PathBuf) {
        let mut interval = time::interval(Duration::from_secs(5));
        let mut last_seen: Option<PathBuf> = None;

        loop {
            interval.tick().await;

            let usb_paths = Self::find_usb_program_paths();
            for usb_path in &usb_paths {
                if last_seen.as_ref() == Some(usb_path) {
                    continue; // Already processed this drive
                }

                info!("Found USB program at: {}", usb_path.display());
                if let Err(e) = Self::load_from_usb(usb_path, &program_dir, &player_tx).await {
                    warn!("Failed to load USB program: {}", e);
                } else {
                    last_seen = Some(usb_path.clone());
                }
            }

            if usb_paths.is_empty() && last_seen.is_some() {
                debug!("USB drive removed");
                last_seen = None;
            }
        }
    }

    /// Find USB mount points containing program XML files
    fn find_usb_program_paths() -> Vec<PathBuf> {
        let mut results = Vec::new();

        #[cfg(unix)]
        {
            // Check common USB mount points on embedded Linux
            let mount_points = [
                "/mnt/usb",
                "/media/usb",
                "/media/usb0",
                "/run/media",
                "/mnt/sdcard",
            ];

            for mount in &mount_points {
                let path = Path::new(mount);
                if path.exists() {
                    if let Ok(entries) = std::fs::read_dir(path) {
                        for entry in entries.flatten() {
                            let p = entry.path();
                            // Look for program.xml or *.xml in the root
                            if p.is_dir() {
                                let prog_xml = p.join("program.xml");
                                if prog_xml.exists() {
                                    results.push(p);
                                    continue;
                                }
                                // Check for any .xml files
                                if let Ok(files) = std::fs::read_dir(&p) {
                                    for f in files.flatten() {
                                        if f.path().extension().is_some_and(|e| e == "xml") {
                                            results.push(p);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        #[cfg(windows)]
        {
            // Check removable drives D: through Z:
            for letter in b'D'..=b'Z' {
                let drive = format!("{}:\\", letter as char);
                let path = Path::new(&drive);
                if path.exists() {
                    let prog_xml = path.join("program.xml");
                    if prog_xml.exists() {
                        results.push(path.to_path_buf());
                    }
                }
            }
        }

        results
    }

    /// Load program from USB and copy files to program directory
    async fn load_from_usb(
        usb_path: &Path,
        program_dir: &Path,
        player_tx: &mpsc::Sender<PlayerCommand>,
    ) -> anyhow::Result<()> {
        // Find XML files on USB
        let mut xml_files = Vec::new();
        for entry in std::fs::read_dir(usb_path)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "xml") {
                xml_files.push(path);
            }
        }

        if xml_files.is_empty() {
            anyhow::bail!("No XML program files found on USB");
        }

        // Create program directory if it doesn't exist
        std::fs::create_dir_all(program_dir)?;

        // Copy all files from USB to program directory
        for entry in std::fs::read_dir(usb_path)? {
            let entry = entry?;
            let src = entry.path();
            if src.is_file() {
                let filename = src.file_name().unwrap();
                let dst = program_dir.join(filename);
                info!("Copying {} -> {}", src.display(), dst.display());
                std::fs::copy(&src, &dst)?;
            }
        }

        // Parse and load the first XML program
        for xml_file in &xml_files {
            let dst = program_dir.join(xml_file.file_name().unwrap());
            match parser::parse_program_file(&dst) {
                Ok(screen) => {
                    info!("Loaded {} programs from USB", screen.programs.len());
                    player_tx
                        .send(PlayerCommand::LoadScreen(screen))
                        .await
                        .ok();
                    return Ok(());
                }
                Err(e) => {
                    warn!("Failed to parse {}: {}", dst.display(), e);
                }
            }
        }

        anyhow::bail!("No valid program XML found on USB")
    }
}
