use std::path::PathBuf;

/// Top-level player configuration
#[derive(Debug, Clone)]
pub struct PlayerConfig {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub program_dir: PathBuf,
    pub port: u16,
    pub output_mode: OutputMode,
    pub output_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub enum OutputMode {
    /// Save each frame as PNG (for testing)
    #[default]
    Png,
    /// Render to DRM/KMS framebuffer (production)
    Framebuffer,
    /// Output raw pixels to stdout (for piping)
    Raw,
}

impl std::str::FromStr for OutputMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "png" => Ok(OutputMode::Png),
            "framebuffer" | "fb" | "drm" => Ok(OutputMode::Framebuffer),
            "raw" | "stdout" => Ok(OutputMode::Raw),
            _ => Err(format!("Unknown output mode: {s}")),
        }
    }
}
