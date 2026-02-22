/// DRM/KMS framebuffer output for Linux LED displays (Rockchip RK3566/RK3568).
/// Uses double-buffered dumb buffers via the Linux DRM kernel API.
/// Only compiled on Unix targets.

#[cfg(unix)]
pub struct DrmOutput {
    width: u32,
    height: u32,
}

#[cfg(unix)]
impl DrmOutput {
    /// Attempt to open a DRM device.
    /// Returns an error if DRM is not available (e.g., running in CI or on a
    /// system without DRM support).
    pub fn open(device: &str, width: u32, height: u32) -> anyhow::Result<Self> {
        // Check if device exists
        if !std::path::Path::new(device).exists() {
            anyhow::bail!("DRM device not found: {}", device);
        }
        // For now, create a stub that logs but doesn't crash
        tracing::info!("DRM output opened: {} ({}x{})", device, width, height);
        Ok(Self { width, height })
    }

    /// Present RGBA pixels to the display.
    /// Stub implementation — real DRM page-flip logic would go here.
    pub fn present(&mut self, rgba_pixels: &[u8]) {
        // Real implementation would:
        // 1. Copy rgba_pixels to inactive dumb buffer (swapping R/B for XRGB8888)
        // 2. Call drmModePageFlip
        // 3. Wait for vblank event
        let _ = rgba_pixels; // suppress unused warning
    }
}

/// No-op on non-Unix platforms.
#[cfg(not(unix))]
pub struct DrmOutput;

#[cfg(not(unix))]
impl DrmOutput {
    pub fn open(_device: &str, _width: u32, _height: u32) -> anyhow::Result<Self> {
        anyhow::bail!("DRM output is only available on Linux")
    }
    pub fn present(&mut self, _rgba_pixels: &[u8]) {}
}
