//! TCP binary packet framing — shared across HDPlayer, HDSet, and BoxPlayer.
//!
//! ## Wire format (all ports)
//! ```text
//! [u16 LE length]   — total packet size (4 + payload_len), includes this field
//! [u16 LE command]  — Command code
//! [N bytes payload] — command-specific data (N = length - 4)
//! ```
//!
//! Confirmed via live device captures: the real device sends and expects
//! `length = total bytes` (header + payload), not `length = payload + 2`.
//!
//! ## Port 9527 protocol (Huidu C-series BoxPlayer, confirmed from Huidu.pcapng)
//!
//! Each SDK command exchange follows this 14-step sequence:
//! ```text
//! 1. PC → Device: Sdk9527ServiceAsk (0x0200) payload=[u32 LE 0]
//! 2. Device → PC: Sdk9527ServiceAnswer (0x0201) payload=[u16 LE 0]
//! 3. PC → Device: Sdk9527Cmd (0x0202) payload=[u16 LE dir=0][XML bytes]
//! 4. Device → PC: Sdk9527CmdAck1 (0x0203) payload=[u16 LE 0]
//! 5. PC → Device: Sdk9527CmdAck2 (0x0204) payload=[u16 LE 0]
//! 6. Device → PC: Sdk9527CmdAck3 (0x0205) payload=[u16 LE 0]
//! 7. Device → PC: Sdk9527ServiceAsk (0x0200) payload=[u32 LE 1]  ("response ready")
//! 8. PC → Device: Sdk9527ServiceAnswer (0x0201) payload=[u16 LE 0]
//! 9. Device → PC: Sdk9527Cmd (0x0202) payload=[u16 LE dir=1][XML bytes]
//! 10. PC → Device: Sdk9527CmdAck1 (0x0203) payload=[u16 LE 0]
//! 11. Device → PC: Sdk9527CmdAck2 (0x0204) payload=[u16 LE 1]
//! 12. PC → Device: Sdk9527CmdAck3 (0x0205) payload=[u16 LE 0]
//! ```
//!
//! XML GUID is the literal string `##GUID` (not a real UUID).
//! No GetIFVersion registration is needed.
//!
//! ## Port 10001 SDK CMD chunking (HDSet / legacy devices)
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

// ── Well-known port and timing constants ──────────────────────────────────────

/// Legacy TCP port used by the old SdkService/SdkCmd protocol (port 10001).
/// Real Huidu PC tools do NOT use this for BoxPlayer devices — they use port 9527.
pub const DEFAULT_PORT: u16 = 10001;

/// TCP port used by BoxPlayer devices for the BoxStream SDK protocol.
/// Confirmed from Huidu.pcapng: HDPlayer.exe connects here, not port 10001.
pub const BOX_PLAYER_PORT: u16 = 9527;

/// Heartbeat interval — both HDPlayer and HDSet send a ping every 30 s.
pub const HEARTBEAT_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

// ── Protocol version constants ────────────────────────────────────────────────

/// Version the PC tools (HDPlayer, HDSet) send in `SdkServiceAsk`.
///
/// Confirmed from wire capture (More Huidu.pcapng frame 187):
/// PC → device port 10001: `08 00 01 20 07 00 00 01`
/// Bytes `07 00 00 01` are the 4-byte payload, which as LE u32 = 0x0100_0007.
/// Format is [major_byte][0][0][build_byte] i.e. version 7.0.0.1.
pub const SDK_CLIENT_VERSION: u32 = 0x0100_0007;

/// Version BoxPlayer responds with in `SdkServiceAnswer`.
///
/// Confirmed from wire capture (More Huidu.pcapng frame 189):
/// device → PC port 10001: `08 00 02 20 06 00 00 01`
/// Bytes `06 00 00 01` = LE u32 = 0x0100_0006 — version 6.0.0.1.
pub const SDK_TRANSPORT_VERSION: u32 = 0x0100_0006;

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
    /// Device → client: generic error response (payload = u16 LE error_code).
    /// Sent when a command fails at the protocol layer (version mismatch, invalid
    /// method, XML parse failure, etc.).  Error codes: 3=kVersionTooLow, 22=kVersionNotSupport,
    /// 23=kInvalidMethod.
    SdkErrorAnswer   = 0x2000,
    /// Client → device: negotiate protocol version (payload = u32 LE version).
    SdkServiceAsk    = 0x2001,
    /// Device → client: acknowledge version (payload = u32 LE device version).
    SdkServiceAnswer = 0x2002,
    /// Client → device: XML SDK command (payload = [u32 total][u32 index][XML]).
    SdkCmdAsk        = 0x2003,
    /// Device → client: XML SDK response (same framing as SdkCmdAsk).
    SdkCmdAnswer     = 0x2004,

    // ── BoxPlayer BoxStream channel (port 9527) ───────────────────────────
    /// PC → device: initiate a BoxStream session (payload = `[u32 LE 0x00000000]`).
    /// Device signals response ready by sending BoxStreamInit with payload `[u32 LE 0x01000000]`.
    BoxStreamInit     = 0x0200,
    /// Device → PC or PC → device: session init acknowledge (payload = `[u16 LE 0x0000]`).
    BoxStreamInitAck  = 0x0201,
    /// Bidirectional: XML data (payload = `[u16 LE direction][xml_bytes]`).
    /// direction: `0x0000` = PC→device (request), `0x0100` = device→PC (response).
    BoxStreamData     = 0x0202,
    /// Device → PC: confirms receipt of BoxStreamData (payload = `[u16 LE 0x0000]`).
    BoxStreamRxAck    = 0x0203,
    /// Bidirectional: sender acknowledges the RxAck (payload = `[u16 LE direction]`).
    BoxStreamTxAck    = 0x0204,
    /// Bidirectional: final handshake step (payload = `[u16 LE 0x0000]`).
    BoxStreamFinalAck = 0x0205,

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
            0x2000 => Self::SdkErrorAnswer,
            0x2001 => Self::SdkServiceAsk,
            0x2002 => Self::SdkServiceAnswer,
            0x2003 => Self::SdkCmdAsk,
            0x2004 => Self::SdkCmdAnswer,
            0x0200 => Self::BoxStreamInit,
            0x0201 => Self::BoxStreamInitAck,
            0x0202 => Self::BoxStreamData,
            0x0203 => Self::BoxStreamRxAck,
            0x0204 => Self::BoxStreamTxAck,
            0x0205 => Self::BoxStreamFinalAck,
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
    ///
    /// `length` = total packet size (4 + payload.len()), confirmed by live device testing.
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(4 + self.payload.len());
        let length = (self.payload.len() + 4) as u16;
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
        if length < 4 {
            return Err(Error::Protocol("packet length field < 4".into()));
        }
        let payload_len = length - 4;
        if data.len() < length {
            return Ok(None);
        }
        let command_raw = cur.read_u16::<LittleEndian>()
            .map_err(|e| Error::Protocol(e.to_string()))?;
        let command = Command::from_u16(command_raw);
        let payload = data[4..4 + payload_len].to_vec();
        Ok(Some((Packet { command, payload }, length)))
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

// ── BoxStream payload helpers ─────────────────────────────────────────────────

/// Build a `BoxStreamData` request payload (PC → device direction).
///
/// Format: `[u16 LE 0x0000][xml_bytes]`
/// The `0x0000` direction prefix marks this as a PC→device request.
/// Confirmed from Huidu.pcapng frame 11 (PC→device XML batch request).
pub fn box_stream_data_request(xml: &str) -> Vec<u8> {
    let xml_bytes = xml.as_bytes();
    let mut v = Vec::with_capacity(2 + xml_bytes.len());
    v.push(0x00);
    v.push(0x00);
    v.extend_from_slice(xml_bytes);
    v
}

/// Extract the XML bytes from a `BoxStreamData` payload (strips the 2-byte direction prefix).
///
/// Returns `None` if the payload is less than 2 bytes.
pub fn parse_box_stream_data(payload: &[u8]) -> Option<&[u8]> {
    if payload.len() < 2 {
        return None;
    }
    Some(&payload[2..])
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

// ── Display impl for logging ──────────────────────────────────────────────────

impl std::fmt::Display for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::SearchDeviceAsk    => "SearchDeviceAsk",
            Self::TcpHeartbeatAsk    => "TcpHeartbeatAsk",
            Self::TcpHeartbeatAnswer => "TcpHeartbeatAnswer",
            Self::SdkErrorAnswer     => "SdkErrorAnswer",
            Self::SdkServiceAsk      => "SdkServiceAsk",
            Self::SdkServiceAnswer   => "SdkServiceAnswer",
            Self::SdkCmdAsk          => "SdkCmdAsk",
            Self::SdkCmdAnswer       => "SdkCmdAnswer",
            Self::BoxStreamInit      => "BoxStreamInit",
            Self::BoxStreamInitAck   => "BoxStreamInitAck",
            Self::BoxStreamData      => "BoxStreamData",
            Self::BoxStreamRxAck     => "BoxStreamRxAck",
            Self::BoxStreamTxAck     => "BoxStreamTxAck",
            Self::BoxStreamFinalAck  => "BoxStreamFinalAck",
            Self::FileStartAsk       => "FileStartAsk",
            Self::FileStartAnswer    => "FileStartAnswer",
            Self::FileContentAsk     => "FileContentAsk",
            Self::FileContentAnswer  => "FileContentAnswer",
            Self::FileEndAsk         => "FileEndAsk",
            Self::FileEndAnswer      => "FileEndAnswer",
            Self::FpgaSettingInAsk   => "FpgaSettingInAsk",
            Self::FpgaSettingInAnswer=> "FpgaSettingInAnswer",
            Self::FpgaSettingOutAsk  => "FpgaSettingOutAsk",
            Self::FpgaSettingOutAnswer=>"FpgaSettingOutAnswer",
            Self::FpgaParamSetAsk    => "FpgaParamSetAsk",
            Self::FpgaParamSetAnswer => "FpgaParamSetAnswer",
            Self::FpgaSetCmdAsk      => "FpgaSetCmdAsk",
            Self::FpgaSetCmdAnswer   => "FpgaSetCmdAnswer",
            Self::ScreenTestInAsk    => "ScreenTestInAsk",
            Self::ScreenTestCmdAsk   => "ScreenTestCmdAsk",
            Self::BootScreenInAsk    => "BootScreenInAsk",
            Self::BootScreenOutAsk   => "BootScreenOutAsk",
            Self::RemoveBootScreenAsk=> "RemoveBootScreenAsk",
            Self::Unknown            => "Unknown",
        };
        write!(f, "{name}(0x{:04X})", self.as_u16())
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_to_bytes_format() {
        let pkt = Packet::new(Command::TcpHeartbeatAsk, vec![]);
        let bytes = pkt.to_bytes();
        // [u16 LE length=4 (total)][u16 LE command=0x005F]
        assert_eq!(bytes, [0x04, 0x00, 0x5F, 0x00]);
    }

    #[test]
    fn packet_with_payload_roundtrip() {
        let payload = b"hello".to_vec();
        let pkt = Packet::new(Command::SdkCmdAsk, payload.clone());
        let bytes = pkt.to_bytes();
        // Length field = payload.len() + 4 = 9 (total size)
        assert_eq!(bytes[0], 9);
        assert_eq!(bytes[1], 0);
        let (parsed, consumed) = Packet::from_bytes(&bytes).unwrap().unwrap();
        assert_eq!(consumed, bytes.len());
        assert_eq!(parsed.command, Command::SdkCmdAsk);
        assert_eq!(parsed.payload, payload);
    }

    #[test]
    fn packet_from_bytes_needs_more_data() {
        // Only 3 bytes — not enough for the 4-byte header
        assert!(Packet::from_bytes(&[0x04, 0x00, 0x5F]).unwrap().is_none());
    }

    #[test]
    fn packet_from_bytes_partial_payload() {
        // total=12 means 8 bytes payload, but only 6 bytes available
        let bytes = [0x0C, 0x00, 0x5F, 0x00, 0x01, 0x02];
        assert!(Packet::from_bytes(&bytes).unwrap().is_none());
    }

    #[test]
    fn packet_from_bytes_invalid_length() {
        // Length field = 1 (< 4 = minimum valid total)
        let bytes = [0x01, 0x00, 0x5F, 0x00];
        assert!(Packet::from_bytes(&bytes).is_err());
    }

    #[test]
    fn command_from_u16_known_values() {
        assert_eq!(Command::from_u16(0x005F), Command::TcpHeartbeatAsk);
        assert_eq!(Command::from_u16(0x0060), Command::TcpHeartbeatAnswer);
        assert_eq!(Command::from_u16(0x2001), Command::SdkServiceAsk);
        assert_eq!(Command::from_u16(0x8001), Command::FileStartAsk);
        assert_eq!(Command::from_u16(0x8006), Command::FileEndAnswer);
    }

    #[test]
    fn command_from_u16_unknown() {
        assert_eq!(Command::from_u16(0xDEAD), Command::Unknown);
    }

    #[test]
    fn command_as_u16_roundtrip() {
        for &cmd in &[
            Command::TcpHeartbeatAsk, Command::SdkServiceAsk,
            Command::SdkCmdAsk, Command::FileStartAsk, Command::FileEndAnswer,
        ] {
            assert_eq!(Command::from_u16(cmd.as_u16()), cmd);
        }
    }

    #[test]
    fn command_display_includes_hex() {
        let s = Command::TcpHeartbeatAsk.to_string();
        assert!(s.contains("TcpHeartbeatAsk"));
        assert!(s.contains("005F"));
    }

    #[test]
    fn sdk_cmd_payload_roundtrip() {
        let xml = "<foo/>";
        let payload = sdk_cmd_ask_payload(xml);
        // 8-byte header + xml bytes
        assert_eq!(payload.len(), 8 + xml.len());
        // First 4 bytes = xml length as u32 LE
        let len = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
        assert_eq!(len, xml.len());
        // Chunk index = 0
        let idx = u32::from_le_bytes(payload[4..8].try_into().unwrap());
        assert_eq!(idx, 0);
        // XML content
        assert_eq!(&payload[8..], xml.as_bytes());
        // parse_sdk_cmd_payload extracts xml correctly
        assert_eq!(parse_sdk_cmd_payload(&payload), Some(xml.as_bytes()));
    }

    #[test]
    fn parse_sdk_cmd_payload_too_short() {
        assert_eq!(parse_sdk_cmd_payload(&[1, 2, 3, 4, 5, 6, 7]), None);
        assert_eq!(parse_sdk_cmd_payload(&[]), None);
    }

    #[test]
    fn heartbeat_packet_command() {
        let pkt = Packet::heartbeat();
        assert_eq!(pkt.command, Command::TcpHeartbeatAsk);
        assert!(pkt.payload.is_empty());
    }
}
