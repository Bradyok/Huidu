//! Huidu `/dev/ttyS1` FPGA-control frame codec.
//!
//! Reverse-engineered from `libFPGADriver.so` (see
//! `products/BoxPlayer/v7.11.18.0/PLAYER_PIPELINE.md` §2). A frame on the wire is:
//!
//! ```text
//! +--------------------------+-------------------+---------------+
//! | 8-byte PREAMBLE (fixed)  |   PAYLOAD (N)     | CRC32 (4, LE) |
//! | 55 55 55 55 55 55 55 D5  |                   |  over PAYLOAD |
//! +--------------------------+-------------------+---------------+
//! ```
//!
//! - Preamble = 7×`0x55` training + `0xD5` SFD (also the RX frame-sync marker).
//! - CRC-32 = standard IEEE-802.3/zlib (reflected, poly 0xEDB88320, init/xorout
//!   0xFFFFFFFF), computed over the PAYLOAD only (not the preamble, not the CRC).
//! - Only two payload sizes are legal (`SendData` rejects everything else):
//!   `CONTROL_PAYLOAD = 25` and `PARAM_PAYLOAD = 521`.
//!
//! ⚠️ Confirm the CRC-32 polynomial against one live ttyS1 capture before trusting
//! it in the field (PLAYER_PIPELINE.md flags this as the one unverified constant).

/// Fixed 8-byte frame preamble: `55 55 55 55 55 55 55 D5`.
pub const PREAMBLE: [u8; 8] = [0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0x55, 0xD5];

/// Legal control-frame payload length (9-byte sub-header + 16-byte data).
pub const CONTROL_PAYLOAD: usize = 25;
/// Legal param-frame payload length (9-byte sub-header + 512-byte data).
pub const PARAM_PAYLOAD: usize = 521;

/// RX receive buffer size used by the stock parser (`0x429`).
pub const RX_BUF: usize = 0x429;

/// Compute the standard reflected CRC-32 (poly 0xEDB88320) over `data`.
///
/// Matches libFPGADriver's `InitCRCTable`/`CRC32`: init 0xFFFFFFFF, per byte
/// `crc = table[(crc ^ byte) & 0xFF] ^ (crc >> 8)`, final xorout 0xFFFFFFFF.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg(); // 0xFFFFFFFF if bit set else 0
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Error decoding a received frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    /// No preamble found in the buffer.
    NoPreamble,
    /// Preamble found but not enough bytes yet for a full frame.
    Incomplete,
    /// Payload length is neither 25 nor 521.
    BadLength(usize),
    /// CRC over the payload did not match the trailing CRC.
    BadCrc { got: u32, want: u32 },
}

/// Encode `payload` (which must be 25 or 521 bytes) into a full wire frame.
///
/// Returns `preamble ++ payload ++ crc32(payload).to_le_bytes()`, total `N + 12`.
pub fn encode(payload: &[u8]) -> Result<Vec<u8>, FrameError> {
    if payload.len() != CONTROL_PAYLOAD && payload.len() != PARAM_PAYLOAD {
        return Err(FrameError::BadLength(payload.len()));
    }
    let mut out = Vec::with_capacity(8 + payload.len() + 4);
    out.extend_from_slice(&PREAMBLE);
    out.extend_from_slice(payload);
    out.extend_from_slice(&crc32(payload).to_le_bytes());
    Ok(out)
}

/// A decoded frame: the payload bytes (25 or 521), sans preamble and CRC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    pub payload: Vec<u8>,
    /// Number of bytes consumed from the input buffer (preamble start .. end of CRC).
    pub consumed: usize,
}

/// Scan `buf` for the next complete frame, matching the stock RX parser (`Read`):
/// find the preamble, decide the payload length from payload byte `[0]`
/// (`1` → control/37B total, `3` → param/533B total), verify the CRC.
///
/// Returns the frame and how many bytes to drain (from the buffer start through the
/// end of that frame). On `Incomplete`/`NoPreamble` the caller keeps buffering.
pub fn decode(buf: &[u8]) -> Result<Frame, FrameError> {
    // Locate the preamble.
    let start = buf
        .windows(PREAMBLE.len())
        .position(|w| w == PREAMBLE)
        .ok_or(FrameError::NoPreamble)?;
    let after = start + PREAMBLE.len();
    if after >= buf.len() {
        return Err(FrameError::Incomplete);
    }
    // Payload length is implied by the response type in payload byte [0].
    let payload_len = match buf[after] {
        1 => CONTROL_PAYLOAD, // control response
        3 => PARAM_PAYLOAD,   // param response
        other => {
            // Unknown type: skip past this preamble and let the caller retry.
            let _ = other;
            return Err(FrameError::BadLength(0));
        }
    };
    let end = after + payload_len + 4;
    if end > buf.len() {
        return Err(FrameError::Incomplete);
    }
    let payload = &buf[after..after + payload_len];
    let want = crc32(payload);
    let got = u32::from_le_bytes([
        buf[after + payload_len],
        buf[after + payload_len + 1],
        buf[after + payload_len + 2],
        buf[after + payload_len + 3],
    ]);
    if got != want {
        return Err(FrameError::BadCrc { got, want });
    }
    Ok(Frame { payload: payload.to_vec(), consumed: end })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vectors() {
        // Standard IEEE CRC-32 check values.
        assert_eq!(crc32(b""), 0x0000_0000);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b"The quick brown fox jumps over the lazy dog"), 0x414F_A339);
    }

    #[test]
    fn encode_rejects_bad_length() {
        assert_eq!(encode(&[0u8; 10]), Err(FrameError::BadLength(10)));
        assert!(encode(&[0u8; CONTROL_PAYLOAD]).is_ok());
        assert!(encode(&[0u8; PARAM_PAYLOAD]).is_ok());
    }

    #[test]
    fn encode_layout() {
        let payload = [0u8; CONTROL_PAYLOAD];
        let f = encode(&payload).unwrap();
        assert_eq!(f.len(), 8 + CONTROL_PAYLOAD + 4);
        assert_eq!(&f[..8], &PREAMBLE);
        let crc = u32::from_le_bytes([f[33], f[34], f[35], f[36]]);
        assert_eq!(crc, crc32(&payload));
    }

    #[test]
    fn round_trip_control() {
        let mut payload = [0u8; CONTROL_PAYLOAD];
        payload[0] = 1; // control response type
        payload[3] = 0x00; payload[4] = 0x01; // func 0x0100
        let wire = encode(&payload).unwrap();
        let dec = decode(&wire).unwrap();
        assert_eq!(dec.payload, payload);
        assert_eq!(dec.consumed, wire.len());
    }

    #[test]
    fn round_trip_param_with_leading_garbage() {
        let mut payload = [0u8; PARAM_PAYLOAD];
        payload[0] = 3; // param response type
        let wire = encode(&payload).unwrap();
        // prepend noise before the preamble
        let mut buf = vec![0xAA, 0xBB, 0xCC];
        buf.extend_from_slice(&wire);
        let dec = decode(&buf).unwrap();
        assert_eq!(dec.payload, payload);
    }

    #[test]
    fn detects_bad_crc() {
        let mut payload = [0u8; CONTROL_PAYLOAD];
        payload[0] = 1;
        let mut wire = encode(&payload).unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 0xFF;
        assert!(matches!(decode(&wire), Err(FrameError::BadCrc { .. })));
    }

    #[test]
    fn incomplete_is_reported() {
        let payload = { let mut p = [0u8; CONTROL_PAYLOAD]; p[0] = 1; p };
        let wire = encode(&payload).unwrap();
        assert_eq!(decode(&wire[..wire.len() - 2]), Err(FrameError::Incomplete));
    }
}
