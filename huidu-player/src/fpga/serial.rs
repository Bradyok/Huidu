/// FPGA serial transport — wraps the character device used to talk to the
/// Cyclone IV FPGA send card.
///
/// Device paths (from binary analysis):
///   PX30 (aarch64):  /dev/ttyS1        — standard UART
///   RK3288 (armv7):  /dev/cyclone4-0   — custom kernel driver
///                    /dev/cyclone4-1   — secondary FPGA (dual output)
///
/// The wire protocol is still being reverse-engineered.  The known framing
/// is a binary packet with a 4-byte header:
///
///   [0xFD] [cmd] [len_lo] [len_hi]  payload…  [checksum]
///
/// where `cmd` selects the operation (pixel data write, config read/write,
/// version query, etc.).  Exact opcodes TBD from logic-analyser capture.
use anyhow::Result;
use tracing::{debug, info};

/// Candidate device paths in priority order
const DEVICE_PATHS: &[&str] = &[
    "/dev/cyclone4-0",
    "/dev/cyclone4-1",
    "/dev/ttyS1",
];

/// Packet framing constants (best-effort from binary analysis)
const FRAME_START: u8 = 0xFD;
const CMD_PIXEL_DATA: u8 = 0x01;
const CMD_GET_VERSION: u8 = 0x10;
const CMD_SET_BRIGHTNESS: u8 = 0x20;
const CMD_SET_CONFIG: u8 = 0x30;

pub struct FpgaSerial {
    /// Path of the device that was successfully opened
    pub device_path: String,
    /// Raw file descriptor (Unix only)
    #[cfg(unix)]
    fd: std::os::unix::io::RawFd,
}

impl FpgaSerial {
    /// Try to open the first available FPGA serial device.
    pub fn open() -> Result<Self> {
        for &path in DEVICE_PATHS {
            match Self::try_open(path) {
                Ok(s) => {
                    info!("FPGA serial opened: {}", path);
                    return Ok(s);
                }
                Err(e) => {
                    debug!("FPGA device {} not available: {}", path, e);
                }
            }
        }
        anyhow::bail!("No FPGA serial device found (tried: {:?})", DEVICE_PATHS)
    }

    #[cfg(unix)]
    fn try_open(path: &str) -> Result<Self> {
        use std::os::unix::io::IntoRawFd;
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .map_err(|e| anyhow::anyhow!("open {}: {}", path, e))?;
        Ok(Self {
            device_path: path.to_string(),
            fd: f.into_raw_fd(),
        })
    }

    #[cfg(not(unix))]
    fn try_open(path: &str) -> Result<Self> {
        anyhow::bail!("FPGA serial not supported on this platform (path: {})", path);
    }

    /// Query the FPGA firmware version.
    /// Returns a version string like "1.0.0" or an error if unavailable.
    pub fn get_version(&mut self) -> Result<String> {
        let req = build_packet(CMD_GET_VERSION, &[]);
        self.write_raw(&req)?;
        let resp = self.read_response(32)?;
        if resp.len() >= 4 {
            Ok(format!("{}.{}.{}", resp[1], resp[2], resp[3]))
        } else {
            Ok("0.0.0".to_string())
        }
    }

    /// Set the global brightness (0–100).
    pub fn set_brightness(&mut self, level: u8) -> Result<()> {
        let payload = [level.min(100)];
        let pkt = build_packet(CMD_SET_BRIGHTNESS, &payload);
        self.write_raw(&pkt)
    }

    /// Send a complete RGBA framebuffer to the FPGA.
    ///
    /// The framebuffer is `width * height * 4` bytes in RGBA order (premultiplied).
    /// The FPGA expects RGB24 in scan-line order; this function converts and
    /// packetises the data.
    ///
    /// NOTE: The exact packet framing for pixel data is still TBD.  This
    /// implementation uses a best-effort format based on binary analysis.
    pub fn send_frame(&mut self, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        // Convert premultiplied RGBA → straight RGB for the FPGA
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for chunk in rgba.chunks_exact(4) {
            let a = chunk[3] as f32 / 255.0;
            if a > 0.0 {
                rgb.push((chunk[0] as f32 / a).min(255.0) as u8);
                rgb.push((chunk[1] as f32 / a).min(255.0) as u8);
                rgb.push((chunk[2] as f32 / a).min(255.0) as u8);
            } else {
                rgb.extend_from_slice(&[0, 0, 0]);
            }
        }

        // Send in 4 KB chunks to stay within UART buffer limits
        const CHUNK_SIZE: usize = 4096;
        for (i, chunk) in rgb.chunks(CHUNK_SIZE).enumerate() {
            let mut payload = Vec::with_capacity(8 + chunk.len());
            // Width / height / chunk index header (best-effort)
            payload.extend_from_slice(&(width as u16).to_le_bytes());
            payload.extend_from_slice(&(height as u16).to_le_bytes());
            payload.extend_from_slice(&(i as u16).to_le_bytes());
            payload.extend_from_slice(&(chunk.len() as u16).to_le_bytes());
            payload.extend_from_slice(chunk);
            let pkt = build_packet(CMD_PIXEL_DATA, &payload);
            self.write_raw(&pkt)?;
        }

        Ok(())
    }

    // ── Low-level I/O ────────────────────────────────────────────────────────

    #[cfg(unix)]
    fn write_raw(&self, data: &[u8]) -> Result<()> {
        use std::io::Write;
        use std::os::unix::io::FromRawFd;
        // SAFETY: fd is valid for the lifetime of this struct
        let mut f = unsafe { std::fs::File::from_raw_fd(self.fd) };
        let r = f.write_all(data);
        // Don't drop the fd ownership — convert back
        std::mem::forget(f);
        r.map_err(|e| anyhow::anyhow!("FPGA serial write: {}", e))
    }

    #[cfg(not(unix))]
    fn write_raw(&self, _data: &[u8]) -> Result<()> {
        anyhow::bail!("FPGA serial not supported on this platform")
    }

    #[cfg(unix)]
    fn read_response(&self, max_bytes: usize) -> Result<Vec<u8>> {
        use std::io::Read;
        use std::os::unix::io::FromRawFd;
        let mut buf = vec![0u8; max_bytes];
        let mut f = unsafe { std::fs::File::from_raw_fd(self.fd) };
        let n = f.read(&mut buf).unwrap_or(0);
        std::mem::forget(f);
        buf.truncate(n);
        Ok(buf)
    }

    #[cfg(not(unix))]
    fn read_response(&self, _max: usize) -> Result<Vec<u8>> {
        Ok(Vec::new())
    }
}

#[cfg(unix)]
impl Drop for FpgaSerial {
    fn drop(&mut self) {
        unsafe { libc::close(self.fd) };
    }
}

/// Build a framed FPGA packet:  [FRAME_START] [cmd] [len_lo] [len_hi]  payload…  [checksum]
fn build_packet(cmd: u8, payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u16;
    let mut pkt = Vec::with_capacity(4 + payload.len() + 1);
    pkt.push(FRAME_START);
    pkt.push(cmd);
    pkt.push((len & 0xFF) as u8);
    pkt.push((len >> 8) as u8);
    pkt.extend_from_slice(payload);
    // Simple XOR checksum over everything after FRAME_START
    let cksum = pkt[1..].iter().fold(0u8, |acc, &b| acc ^ b);
    pkt.push(cksum);
    pkt
}
