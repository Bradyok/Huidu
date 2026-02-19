/// Program persistence service.
/// Saves and loads program state to/from disk.
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

    /// Save screen XML to the program directory
    pub fn save_program(&self, _screen: &Screen, xml: &str) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.program_dir)?;

        // Save as current_program.xml
        let path = self.program_dir.join("current_program.xml");
        std::fs::write(&path, xml)?;
        info!("Saved program to {}", path.display());
        Ok(())
    }

    /// Load the most recent program from disk
    pub fn load_current_program(&self) -> Option<Screen> {
        let path = self.program_dir.join("current_program.xml");
        if !path.exists() {
            return None;
        }

        match crate::program::parser::parse_program_file(&path) {
            Ok(screen) => {
                info!("Restored program from {}", path.display());
                Some(screen)
            }
            Err(e) => {
                warn!("Failed to restore program: {}", e);
                None
            }
        }
    }

    /// Get the program directory path
    pub fn program_dir(&self) -> &PathBuf {
        &self.program_dir
    }

    /// List all files in the program directory
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

    /// Delete a file from the program directory
    pub fn delete_file(&self, filename: &str) -> anyhow::Result<()> {
        let path = self.program_dir.join(filename);
        if path.exists() {
            std::fs::remove_file(&path)?;
            info!("Deleted file: {}", path.display());
        }
        Ok(())
    }

    /// Delete all files in the program directory
    pub fn clear(&self) -> anyhow::Result<()> {
        if let Ok(entries) = std::fs::read_dir(&self.program_dir) {
            for entry in entries.flatten() {
                if entry.path().is_file() {
                    std::fs::remove_file(entry.path())?;
                }
            }
        }
        info!("Cleared program directory");
        Ok(())
    }
}
