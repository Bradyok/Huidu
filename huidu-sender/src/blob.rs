//! The 512-byte param blobs (scan table / gamma / send-card config) — the Huidu
//! "secret sauce" (PLAYER_PIPELINE.md §2.6).
//!
//! These blobs are built by large per-driver-IC const tables baked into
//! `libFPGADriver.so` (`s_dualScanTab`, `s_lightPriority`/`s_refreshPriority`/
//! `s_grayPriority`, gamma, color-correction, locus). Reproducing those tables is the
//! hard part, so this daemon does **not** synthesize them. Instead it ships the blobs
//! one of two ways (both keep the panel lit without re-deriving Huidu's tables):
//!
//! 1. **Captured templates** — dump the exact param frames the stock BoxPlayer sends on
//!    a correctly-configured C15/C35/C36 (logic-analyzer or an on-device serial tap),
//!    strip preamble+CRC, and drop the 512-byte payloads here as files. Replaying them
//!    verbatim reproduces the stock configuration bit-for-bit.
//! 2. **Relinked libFPGADriver** — call the stock `.so`'s param builders
//!    (`GenSendCardParamAsk` etc.) via FFI to produce the blob, then frame it here.
//!    (Left as a follow-up; requires resolving the C++ ABI / Qt deps of the lib.)
//!
//! Global brightness is a *field inside* the 512-byte send-card blob, not its own
//! opcode. `patch_brightness` rewrites just that field so brightness changes don't need
//! a fresh table build. **The exact offset must be confirmed on hardware** (see below).

use std::io;
use std::path::Path;

/// A 512-byte param data blob (goes after the 9-byte sub-header in a param frame).
pub type Blob = [u8; 512];

/// Load a 512-byte blob from a file (e.g. a captured send-card/recv-card param payload).
pub fn load(path: &Path) -> io::Result<Blob> {
    let data = std::fs::read(path)?;
    if data.len() < 512 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{}: expected >=512 bytes, got {}", path.display(), data.len()),
        ));
    }
    let mut b = [0u8; 512];
    b.copy_from_slice(&data[..512]);
    Ok(b)
}

/// Where the global brightness byte(s) live inside the 512-byte send-card blob.
///
/// ⚠️ **UNCONFIRMED — verify on hardware.** From the RE, brightness/on-off/gamma are
/// packed by `HFPGAParam::UpdateFPGASendCardParam` into the send-card blob shipped by
/// `GenSendCardParamAsk` (func 0x0000). The precise offset/encoding (single 0..255
/// byte, or per-channel gains) was not pinned in the static analysis. The safest
/// bring-up path is to capture two stock send-card frames at different brightnesses and
/// diff them to find the field; wire that offset here.
pub const BRIGHTNESS_OFFSET: usize = usize::MAX; // sentinel = "unknown, do not patch"

/// Rewrite the brightness field of a send-card blob to `level` (0..=255).
///
/// Returns `false` (and leaves the blob unchanged) if `BRIGHTNESS_OFFSET` is still the
/// unknown sentinel — so a mis-set offset can never silently corrupt the blob.
pub fn patch_brightness(blob: &mut Blob, level: u8) -> bool {
    if BRIGHTNESS_OFFSET == usize::MAX || BRIGHTNESS_OFFSET >= blob.len() {
        return false;
    }
    blob[BRIGHTNESS_OFFSET] = level;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_is_refused_until_offset_known() {
        let mut b = [0u8; 512];
        assert!(!patch_brightness(&mut b, 128));
        assert!(b.iter().all(|&x| x == 0)); // untouched
    }
}
