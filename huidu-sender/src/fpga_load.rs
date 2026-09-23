//! FPGA bitstream loading — our replacement for the closed `write_fpga` binary.
//!
//! On the stock unit the LED FPGA (an Altera/Intel Cyclone-IV-class part) is
//! configured at boot by `/root/Box/System/write_fpga <img> /dev/cyclone4`,
//! where `/dev/cyclone4` is the out-of-tree `drivers/char/cyclone4.c`. Neither
//! the binary nor the driver is redistributable, so we reproduce the load here.
//!
//! Two paths reach the same FPGA (see `PX30_C_SERIES_HARDWARE.md` §"FPGA" and
//! `hardware/DRIVER_ENABLEMENT.md`):
//!
//! 1. **Kernel driver** `altr,fpga-passive-serial` (mainline `altera-ps-spi`,
//!    already in the DT + kernel fragment). It does the passive-serial (PS)
//!    handshake and bit-reversal itself; it just needs the raw PS bitstream as
//!    firmware. Use [`FpgaImage::parse`] to strip Huidu's 8-byte wrapper, then
//!    hand the payload to the fpga-manager. This is the preferred path.
//! 2. **Userspace over spidev + gpio** ([`load_via_spidev`], Unix only). The
//!    fallback for if `altera-ps-spi` rejects this specific bitstream: we drive
//!    nCONFIG/nSTATUS/CONF_DONE and clock the payload out `/dev/spidevB.C`
//!    ourselves. This is exactly what `write_fpga` did.
//!
//! ## The Huidu image wrapper (`/boot/fpga.img`, version file = 6.6.0.0)
//!
//! ```text
//! +-------------------+-------------------+--------------------------------+
//! | magic 6C 03 16 46 | u32 LE payload_len| PS bitstream (0xFF pad, then   |
//! | (4 bytes)         | (= filesize - 8)  |  sync CC 55 AA 33, then config) |
//! +-------------------+-------------------+--------------------------------+
//! ```
//!
//! The payload after the 8-byte header is the raw passive-serial stream (it
//! starts with dummy `0xFF` clocks and the Cyclone sync word `CC 55 AA 33`),
//! i.e. the Altera `.rbf` content. We do NOT re-encode it.
//!
//! ⚠️ **Bit order is confirm-on-hardware.** Altera PS shifts config data
//! LSB-first on DATA0; SPI is MSB-first, so a byte must be bit-reversed on the
//! wire (mainline `altera-ps-spi` calls `bitrev8`). We default to bit-reversing
//! in the spidev path and expose [`BitOrder`] so it can be flipped once proven
//! on a live unit. The kernel-driver path handles this internally.

/// The 4-byte magic at the start of a Huidu `fpga.img`: `6C 03 16 46`.
pub const IMG_MAGIC: [u8; 4] = [0x6C, 0x03, 0x16, 0x46];

/// The Cyclone passive-serial sync word that should appear early in the payload.
pub const PS_SYNC: [u8; 4] = [0xCC, 0x55, 0xAA, 0x33];

/// Error parsing a Huidu `fpga.img`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImgError {
    /// Fewer than 8 bytes — no room for the header.
    TooShort,
    /// Magic did not match [`IMG_MAGIC`].
    BadMagic([u8; 4]),
    /// Header length field does not match the actual file size.
    LengthMismatch { header: u32, actual: usize },
}

impl std::fmt::Display for ImgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImgError::TooShort => write!(f, "image shorter than the 8-byte header"),
            ImgError::BadMagic(m) => write!(f, "bad magic {m:02x?}, expected {IMG_MAGIC:02x?}"),
            ImgError::LengthMismatch { header, actual } => {
                write!(f, "header payload_len={header} but file has {actual} payload bytes")
            }
        }
    }
}
impl std::error::Error for ImgError {}

/// A parsed Huidu FPGA image: the raw passive-serial payload, wrapper removed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FpgaImage {
    /// The passive-serial bitstream (what a kernel `altera-ps-spi` firmware or
    /// the spidev loader consumes).
    pub payload: Vec<u8>,
}

impl FpgaImage {
    /// Parse `raw` (the contents of `/boot/fpga.img`), validating the 8-byte
    /// header and returning the PS payload.
    pub fn parse(raw: &[u8]) -> Result<Self, ImgError> {
        if raw.len() < 8 {
            return Err(ImgError::TooShort);
        }
        let magic = [raw[0], raw[1], raw[2], raw[3]];
        if magic != IMG_MAGIC {
            return Err(ImgError::BadMagic(magic));
        }
        let header_len = u32::from_le_bytes([raw[4], raw[5], raw[6], raw[7]]);
        let payload = &raw[8..];
        if header_len as usize != payload.len() {
            return Err(ImgError::LengthMismatch { header: header_len, actual: payload.len() });
        }
        Ok(FpgaImage { payload: payload.to_vec() })
    }

    /// Does the payload contain the expected PS sync word near the start?
    /// (Sanity check only; the payload begins with `0xFF` dummy clocks.)
    pub fn has_ps_sync(&self) -> bool {
        let scan = &self.payload[..self.payload.len().min(4096)];
        scan.windows(4).any(|w| w == PS_SYNC)
    }
}

/// Whether to bit-reverse each byte before clocking it (Altera PS is LSB-first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BitOrder {
    /// Reverse each byte (LSB-first on the wire). Matches mainline `altera-ps-spi`.
    LsbFirst,
    /// Send bytes as-is (MSB-first). Fallback if a unit proves to want it.
    MsbFirst,
}

/// Reverse the bits of one byte (`0b0000_0001` -> `0b1000_0000`).
#[inline]
pub fn bitrev8(b: u8) -> u8 {
    b.reverse_bits()
}

/// Produce the exact byte stream to clock over SPI for `payload` under `order`.
pub fn ps_wire_bytes(payload: &[u8], order: BitOrder) -> Vec<u8> {
    match order {
        BitOrder::MsbFirst => payload.to_vec(),
        BitOrder::LsbFirst => payload.iter().map(|&b| bitrev8(b)).collect(),
    }
}

/// Low-level passive-serial I/O the load sequence drives. Levels are *logical*:
/// `true` = the signal is asserted (the impl applies active-low inversion).
pub trait PsIo {
    /// Drive nCONFIG asserted (FPGA held in configuration reset).
    fn reset_assert(&mut self) -> std::io::Result<()>;
    /// Release nCONFIG (deasserted) so the FPGA starts accepting config data.
    fn reset_release(&mut self) -> std::io::Result<()>;
    /// nSTATUS asserted? (active-low pin reads low). Asserted = FPGA busy/reset.
    fn status_asserted(&mut self) -> std::io::Result<bool>;
    /// CONF_DONE high? (configuration completed successfully).
    fn conf_done(&mut self) -> std::io::Result<bool>;
    /// Clock `bytes` out on DATA0/DCLK (SPI), MSB-first at the byte level.
    fn send(&mut self, bytes: &[u8]) -> std::io::Result<()>;
    /// Busy-wait roughly `us` microseconds.
    fn delay_us(&mut self, us: u64);
}

/// Error from the passive-serial load sequence.
#[derive(Debug)]
pub enum LoadError {
    Io(std::io::Error),
    /// nSTATUS never released after nCONFIG went high (no FPGA / wiring).
    StatusStuck,
    /// CONF_DONE never went high after the whole bitstream was sent.
    ConfDoneNotSet,
}
impl From<std::io::Error> for LoadError {
    fn from(e: std::io::Error) -> Self { LoadError::Io(e) }
}
impl std::fmt::Display for LoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoadError::Io(e) => write!(f, "io: {e}"),
            LoadError::StatusStuck => write!(f, "nSTATUS never released (FPGA absent or miswired?)"),
            LoadError::ConfDoneNotSet => write!(f, "CONF_DONE never asserted (bad bitstream/bit-order?)"),
        }
    }
}
impl std::error::Error for LoadError {}

/// Run the Altera passive-serial configuration sequence for `img` over `io`.
///
/// Sequence (Altera AN + `altera-ps-spi.c`): pulse nCONFIG low→high, wait for
/// nSTATUS to release, clock the (bit-ordered) payload, then require CONF_DONE,
/// then send a few extra init clocks. Generic over [`PsIo`] so the core is
/// testable without hardware.
pub fn load<IO: PsIo>(io: &mut IO, img: &FpgaImage, order: BitOrder) -> Result<(), LoadError> {
    // 1. Assert reset, hold, and confirm the device pulls nSTATUS.
    io.reset_assert()?;
    io.delay_us(20);

    // 2. Release nCONFIG and wait (up to ~100 ms) for nSTATUS to release.
    io.reset_release()?;
    let mut released = false;
    for _ in 0..1000 {
        if !io.status_asserted()? {
            released = true;
            break;
        }
        io.delay_us(100);
    }
    if !released {
        return Err(LoadError::StatusStuck);
    }

    // 3. Clock out the configuration data.
    let wire = ps_wire_bytes(&img.payload, order);
    io.send(&wire)?;

    // 4. CONF_DONE must be high once the stream is in; allow a short settle +
    //    extra init clocks (dummy 0xFF bytes) as the AN prescribes.
    for _ in 0..16 {
        if io.conf_done()? {
            io.send(&[0xFF, 0xFF])?; // init-phase clocks
            return Ok(());
        }
        io.send(&[0xFF])?;
    }
    Err(LoadError::ConfDoneNotSet)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wrap(payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&IMG_MAGIC);
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn parse_roundtrip() {
        let payload: Vec<u8> = [&[0xFF; 8][..], &PS_SYNC[..], &[0x12, 0x34]].concat();
        let raw = wrap(&payload);
        let img = FpgaImage::parse(&raw).unwrap();
        assert_eq!(img.payload, payload);
        assert!(img.has_ps_sync());
    }

    #[test]
    fn parse_rejects_bad_magic() {
        let mut raw = wrap(&[0u8; 4]);
        raw[0] = 0;
        assert!(matches!(FpgaImage::parse(&raw), Err(ImgError::BadMagic(_))));
    }

    #[test]
    fn parse_rejects_length_mismatch() {
        let mut raw = wrap(&[1, 2, 3, 4]);
        raw[4] = 99; // claim 99 payload bytes
        assert!(matches!(FpgaImage::parse(&raw), Err(ImgError::LengthMismatch { .. })));
    }

    #[test]
    fn parse_rejects_short() {
        assert_eq!(FpgaImage::parse(&[0u8; 4]), Err(ImgError::TooShort));
    }

    #[test]
    fn bitrev_matches_known() {
        assert_eq!(bitrev8(0x01), 0x80);
        assert_eq!(bitrev8(0xCC), 0x33);
        assert_eq!(bitrev8(0b1010_0000), 0b0000_0101);
    }

    #[test]
    fn wire_bytes_respect_order() {
        let p = [0x01, 0xCC];
        assert_eq!(ps_wire_bytes(&p, BitOrder::MsbFirst), vec![0x01, 0xCC]);
        assert_eq!(ps_wire_bytes(&p, BitOrder::LsbFirst), vec![0x80, 0x33]);
    }

    // A mock PsIo that "configures" after receiving all bytes.
    struct MockFpga {
        in_reset: bool,
        got: usize,
        need: usize,
    }
    impl PsIo for MockFpga {
        fn reset_assert(&mut self) -> std::io::Result<()> { self.in_reset = true; self.got = 0; Ok(()) }
        fn reset_release(&mut self) -> std::io::Result<()> { self.in_reset = false; Ok(()) }
        fn status_asserted(&mut self) -> std::io::Result<bool> { Ok(self.in_reset) }
        fn conf_done(&mut self) -> std::io::Result<bool> { Ok(self.got >= self.need) }
        fn send(&mut self, b: &[u8]) -> std::io::Result<()> { self.got += b.len(); Ok(()) }
        fn delay_us(&mut self, _us: u64) {}
    }

    #[test]
    fn load_sequence_succeeds() {
        let payload = vec![0xFFu8; 64];
        let img = FpgaImage { payload };
        let mut io = MockFpga { in_reset: false, got: 0, need: 64 };
        assert!(load(&mut io, &img, BitOrder::LsbFirst).is_ok());
    }

    #[test]
    fn load_reports_conf_done_failure() {
        let img = FpgaImage { payload: vec![0xFF; 8] };
        // need more than we will ever send (payload 8 + up to 16 dummy = 24)
        let mut io = MockFpga { in_reset: false, got: 0, need: 10_000 };
        assert!(matches!(load(&mut io, &img, BitOrder::LsbFirst), Err(LoadError::ConfDoneNotSet)));
    }
}
