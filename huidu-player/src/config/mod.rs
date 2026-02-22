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
    /// Device ID broadcast in UDP discovery and sent to cloud
    pub device_id: String,
    /// Cloud OMS base URL (empty = cloud disabled)
    pub cloud_url: String,
    /// Weather API URL (empty = weather widget disabled)
    pub weather_url: String,
}

#[derive(Debug, Clone, Default)]
pub enum OutputMode {
    /// Save each frame as PNG (for testing)
    #[default]
    Png,
    /// Send rendered frames to FPGA via serial (production)
    Framebuffer,
    /// Output raw RGBA pixels to stdout (for piping to another tool)
    Raw,
    /// DRM/KMS direct rendering (Linux LED display)
    Drm,
}

impl std::str::FromStr for OutputMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "png"                => Ok(OutputMode::Png),
            "framebuffer" | "fb" => Ok(OutputMode::Framebuffer),
            "drm" | "dri"        => Ok(OutputMode::Drm),
            "raw" | "stdout"     => Ok(OutputMode::Raw),
            _ => Err(format!("Unknown output mode: {s}")),
        }
    }
}
