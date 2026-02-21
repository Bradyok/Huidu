/// Program persistence service.
/// Saves and loads program state to/from disk, indexed by program GUID.
use std::path::PathBuf;
use tracing::{info, warn};

use crate::program::model::Screen;

pub struct StorageService {
    program_dir: PathBuf,
}

impl StorageService {
    pub fn new(program_dir: PathBuf) -> Self {
        Self { program_dir }
    }

    /// Save screen XML to the program directory.
    /// The file is named after the first program's GUID so that GetProgram
    /// can retrieve it by GUID later.
    pub fn save_program(&self, screen: &Screen, xml: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.program_dir)?;

        let filename = if let Some(first) = screen.programs.first() {
            guid_to_filename(&first.guid)
        } else if !screen.timestamps.is_empty() {
            format!("{}.xml", sanitize_guid(&screen.timestamps))
        } else {
            "current_program.xml".to_string()
        };

        let path = self.program_dir.join(&filename);
        std::fs::write(&path, xml)?;
        info!(
            "Saved {} program(s) to {}",
            screen.programs.len(),
            path.display()
        );
        Ok(())
    }

    /// Load raw XML for a program by GUID.
    /// First tries the GUID-derived filename; falls back to scanning all XML
    /// files in the directory for one that contains the requested GUID.
    pub fn load_program_xml_by_guid(&self, guid: &str) -> Option<String> {
        // Direct GUID-based filename
        let direct = self.program_dir.join(guid_to_filename(guid));
        if direct.exists() {
            if let Ok(content) = std::fs::read_to_string(&direct) {
                return Some(content);
            }
        }

        // Legacy fallback: scan all XML files for one referencing this GUID
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "xml") {
                    if let Ok(content) = std::fs::read_to_string(&path) {
                        if content.contains(&format!("guid=\"{}\"", guid)) {
                            return Some(content);
                        }
                    }
                }
            }
        }

        None
    }

    /// Load the most recent program from disk (scans the directory for any XML).
    pub fn load_current_program(&self) -> Option<Screen> {
        // Try legacy filename first for backward compatibility
        let legacy = self.program_dir.join("current_program.xml");
        if legacy.exists() {
            if let Ok(s) = crate::program::parser::parse_program_file(&legacy) {
                info!("Restored program from {}", legacy.display());
                return Some(s);
            }
        }

        // Otherwise take the first XML we find
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "xml") {
                    match crate::program::parser::parse_program_file(&path) {
                        Ok(screen) => {
                            info!("Restored program from {}", path.display());
                            return Some(screen);
                        }
                        Err(e) => {
                            warn!("Failed to parse {}: {}", path.display(), e);
                        }
                    }
                }
            }
        }
        None
    }

    /// List all filenames in the program directory.
    pub fn list_files(&self) -> Vec<String> {
        let mut files = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        files.push(name.to_string());
                    }
                }
            }
        }
        files
    }

    /// Delete a file from the program directory.
    pub fn delete_file(&self, filename: &str) -> anyhow::Result<()> {
        let path = self.program_dir.join(filename);
        if path.exists() {
            std::fs::remove_file(&path)?;
            info!("Deleted file: {}", path.display());
        }
        Ok(())
    }

    /// Delete all XML files in the program directory.
    pub fn clear(&self) -> anyhow::Result<()> {
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    std::fs::remove_file(&path)?;
                }
            }
        }
        info!("Cleared program directory");
        Ok(())
    }

    /// Get the program directory path.
    pub fn program_dir(&self) -> &PathBuf {
        &self.program_dir
    }

    /// Return the GUID-derived filename for a given program GUID.
    pub fn filename_for_guid(guid: &str) -> String {
        guid_to_filename(guid)
    }
}

/// Sanitise a GUID/timestamp string to make it safe for use as a filename.
fn sanitize_guid(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect()
}

fn guid_to_filename(guid: &str) -> String {
    format!("{}.xml", sanitize_guid(guid))
}
