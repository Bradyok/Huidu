//! TCP binary packet framing — shared across HDPlayer, HDSet, and BoxPlayer.
//!
//! ## Wire format
//! ```text
//! [u16 LE length]   — payload_len + 2  (includes the command word itself)
//! [u16 LE command]  — Command code
//! [N bytes payload] — command-specific data (N = length - 2)
//! ```
//!
//! ## SDK CMD chunking
//! `SdkCmdAsk` / `SdkCmdAnswer` payloads have an 8-byte framing header
//! **before** the XML bytes (confirmed from BoxPlayer server.rs):
//! ```text
//! [u32 LE total_xml_len]  — total XML length (may span multiple packets)
//! [u32 LE chunk_index]    — 0-based chunk index (0 for single-chunk commands)
//! [N bytes XML]           — UTF-8 XML fragment
//! ```

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use std::io::Cursor;
use crate::error::{Error, Result};

// ── Protocol version constants ────────────────────────────────────────────────

/// Version the PC tools (HDPlayer, HDSet) send in `SdkServiceAsk`.
/// Value `0x01000000` from binary analysis of HCatNet.dll.
pub const SDK_CLIENT_VERSION: u32 = 0x0100_0000;

/// Version BoxPlayer responds with in `SdkServiceAnswer`.
/// The lower byte (`0x05`) encodes a Huidu-internal firmware revision.
pub const SDK_TRANSPORT_VERSION: u32 = 0x0100_0005;

// ── Command codes ─────────────────────────────────────────────────────────────

/// All known command codes in the Huidu binary protocol.
///
/// Codes confirmed from wire captures and binary analysis are marked with their
/// exact hex value.  HDSet-specific FPGA/screen/boot codes are TBD (pending
/// wire capture of HDSet ↔ BoxPlayer traffic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Command {
    // ── UDP device search ─────────────────────────────────────────────────
    /// Sent by PC tools as a broadcast search trigger (inferred; any packet on
    /// port 9527 elicits a response from real firmware).
    SearchDeviceAsk    = 0x0001,

    // ── TCP keep-alive ────────────────────────────────────────────────────
    TcpHeartbeatAsk    = 0x005F,
    TcpHeartbeatAnswer = 0x0060,

    // ── SDK XML channel ───────────────────────────────────────────────────
    /// Client → device: negotiate protocol version (payload = u32 LE version).
    SdkServiceAsk    = 0x2001,
    /// Device → client: acknowledge version (payload = u32 LE device version).
    SdkServiceAnswer = 0x2002,
    /// Client → device: XML SDK command (payload = [u32 total][u32 index][XML]).
    SdkCmdAsk        = 0x2003,
    /// Device → client: XML SDK response (same framing as SdkCmdAsk).
    SdkCmdAnswer     = 0x2004,

    // ── File transfer ─────────────────────────────────────────────────────
    /// Start transfer: [32 B MD5 hex][u64 size][u16 type][filename\0]
    FileStartAsk      = 0x8001,
    /// Device ack: [u32 result=0][u64 resume_offset=0]
    FileStartAnswer   = 0x8002,
    /// One chunk: [u64 offset][data]
    FileContentAsk    = 0x8003,
    /// Per-chunk ack: [u32 result=0]
    FileContentAnswer = 0x8004,
    FileEndAsk        = 0x8005,
    /// End ack: [u32 result]
    FileEndAnswer     = 0x8006,

    // ── HDSet: FPGA configuration session (TBD — pending wire capture) ────
    FpgaSettingInAsk     = 0x3001,
    FpgaSettingInAnswer  = 0x3002,
    FpgaSettingOutAsk    = 0x3003,
    FpgaSettingOutAnswer = 0x3004,
    FpgaParamSetAsk      = 0x3005,
    FpgaParamSetAnswer   = 0x3006,
    FpgaSetCmdAsk        = 0x3007,
    FpgaSetCmdAnswer     = 0x3008,

    // ── HDSet: screen test (TBD) ──────────────────────────────────────────
    ScreenTestInAsk  = 0x4001,
    ScreenTestCmdAsk = 0x4002,

    // ── HDSet: boot logo (TBD) ────────────────────────────────────────────
    BootScreenInAsk     = 0x5001,
    BootScreenOutAsk    = 0x5002,
    RemoveBootScreenAsk = 0x5003,

    Unknown = 0xFFFF,
}

impl Command {
    pub fn from_u16(v: u16) -> Self {
        match v {
            0x0001 => Self::SearchDeviceAsk,
            0x005F => Self::TcpHeartbeatAsk,
            0x0060 => Self::TcpHeartbeatAnswer,
            0x2001 => Self::SdkServiceAsk,
            0x2002 => Self::SdkServiceAnswer,
            0x2003 => Self::SdkCmdAsk,
            0x2004 => Self::SdkCmdAnswer,
            0x8001 => Self::FileStartAsk,
            0x8002 => Self::FileStartAnswer,
            0x8003 => Self::FileContentAsk,
            0x8004 => Self::FileContentAnswer,
            0x8005 => Self::FileEndAsk,
            0x8006 => Self::FileEndAnswer,
            0x3001 => Self::FpgaSettingInAsk,
            0x3002 => Self::FpgaSettingInAnswer,
            0x3003 => Self::FpgaSettingOutAsk,
            0x3004 => Self::FpgaSettingOutAnswer,
            0x3005 => Self::FpgaParamSetAsk,
            0x3006 => Self::FpgaParamSetAnswer,
            0x3007 => Self::FpgaSetCmdAsk,
            0x3008 => Self::FpgaSetCmdAnswer,
            0x4001 => Self::ScreenTestInAsk,
            0x4002 => Self::ScreenTestCmdAsk,
            0x5001 => Self::BootScreenInAsk,
            0x5002 => Self::BootScreenOutAsk,
            0x5003 => Self::RemoveBootScreenAsk,
            _ => Self::Unknown,
        }
    }

    pub fn as_u16(self) -> u16 {
        self as u16
    }
}

// ── Packet struct ─────────────────────────────────────────────────────────────

/// A decoded protocol packet.
#[derive(Debug, Clone)]
pub struct Packet {
    pub command: Command,
    pub payload: Vec<u8>,
}

impl Packet {
    pub fn new(command: Command, payload: Vec<u8>) -> Self {
        Self { command, payload }
    }

    pub fn heartbeat() -> Self {
        Self::new(Command::TcpHeartbeatAsk, vec![])
    }

    /// Serialize to wire bytes: `[u16 LE length][u16 LE command][payload]`
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.payload.len());
        let length = (self.payload.len() + 2) as u16;
        buf.write_u16::<LittleEndian>(length).unwrap();
        buf.write_u16::<LittleEndian>(self.command.as_u16()).unwrap();
        buf.extend_from_slice(&self.payload);
        buf
    }

    /// Try to parse one packet from a byte buffer.
    ///
    /// Returns `Ok(Some((packet, bytes_consumed)))` if a complete packet is
    /// present, `Ok(None)` if more data is needed, or `Err` on a framing error.
    pub fn from_bytes(data: &[u8]) -> Result<Option<(Packet, usize)>> {
        if data.len() < 4 {
            return Ok(None);
        }
        let mut cur = Cursor::new(data);
        let length = cur.read_u16::<LittleEndian>()
            .map_err(|e| Error::Protocol(e.to_string()))? as usize;
        if length < 2 {
            return Err(Error::Protocol("packet length field < 2".into()));
        }
        let payload_len = length - 2;
        if data.len() < 4 + payload_len {
            return Ok(None);
        }
        let command_raw = cur.read_u16::<LittleEndian>()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let command = Command::from_u16(command_raw);
        let payload = data[4..4 + payload_len].to_vec();
        Ok(Some((Packet { command, payload }, 4 + payload_len)))
    }
}

// ── SDK payload helpers ───────────────────────────────────────────────────────

/// Build the `SdkServiceAsk` payload: `[u32 LE version]`.
pub fn sdk_service_ask_payload(version: u32) -> Vec<u8> {
    let mut v = Vec::with_capacity(4);
    v.write_u32::<LittleEndian>(version).unwrap();
    v
}

/// Build a `SdkCmdAsk` payload with the required 8-byte framing header.
///
/// Format: `[u32 LE total_xml_len][u32 LE chunk_index=0][xml_bytes]`
///
/// The device accumulates chunks until `xml_buffer.len() >= total_xml_len`.
/// Single-command calls always use `chunk_index = 0`.
pub fn sdk_cmd_ask_payload(xml: &str) -> Vec<u8> {
    let xml_bytes = xml.as_bytes();
    let mut v = Vec::with_capacity(8 + xml_bytes.len());
    v.write_u32::<LittleEndian>(xml_bytes.len() as u32).unwrap();
    v.write_u32::<LittleEndian>(0).unwrap(); // chunk index 0
    v.extend_from_slice(xml_bytes);
    v
}

/// Build a `SdkCmdAnswer` payload (server-side mirror of sdk_cmd_ask_payload).
pub fn sdk_cmd_answer_payload(xml: &str) -> Vec<u8> {
    sdk_cmd_ask_payload(xml) // same framing
}

/// Extract the XML string from a `SdkCmdAsk` or `SdkCmdAnswer` payload.
///
/// Strips the 8-byte `[u32 total][u32 index]` header.
/// Returns `None` if the payload is too short.
pub fn parse_sdk_cmd_payload(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 8 {
        return None;
    }
    Some(&payload[8..])
}
