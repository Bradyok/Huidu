//! Huidu ttyS1 payload sub-header + function codes + command builders.
//!
//! From `libFPGADriver.so` (`RecvCard.cpp`/`SendCard.cpp`, see PLAYER_PIPELINE.md §2.4–2.7).
//! Every payload is a 9-byte sub-header followed by 16 bytes (control) or 512 bytes (param):
//!
//! | off | type | field |
//! |---|---|---|
//! | 0 | u8 | target: 1=recv-card, 2=send-card (resp: 1=control, 3=param) |
//! | 1 | u8 | card index / address |
//! | 2 | u8 | phy / sub-index |
//! | 3 | u16 LE | function code |
//! | 5 | u16 LE | param / offset |
//! | 7 | u16 LE | data length: 0x10 (control) or 0x200 (param) |
//! | 9.. | data | 16 or 512 bytes |
//!
//! Brightness / gamma / scan are NOT individual opcodes — they are fields inside the
//! 512-byte send-card / recv-card param blobs (§2.6). This module builds the framing;
//! the 512-byte blob content comes from `blob.rs` (captured templates or a relinked
//! libFPGADriver), which is the Huidu "secret sauce".

use crate::frame::{self, CONTROL_PAYLOAD, PARAM_PAYLOAD};

/// Sub-header target byte (payload[0]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Target {
    RecvCard = 1,
    SendCard = 2,
}

/// Function codes (payload[3:5], u16 LE). See PLAYER_PIPELINE.md §2.5.
pub mod func {
    pub const SEARCH_OR_BASIC: u16 = 0x0100; // search recv card (ctl) / basic param (param)
    pub const READBACK_OR_SCAN: u16 = 0x0200; // readback (ctl) / high-refresh scan table (param)
    pub const GAMMA_OR_SAVE: u16 = 0x0300; // gamma table / save-to-card (param)
    pub const BRIGHTNESS_LUT: u16 = 0x0400; // locus/brightness index LUT (param); status resp sub-func
    pub const GEOMETRY_OR_TEMP: u16 = 0x0500; // recv-card range/geometry (param) / read temp (ctl)
    pub const HDMI_CHECK: u16 = 0x0600; // check HDMI signal (ctl)
    pub const PHY_CHANGE: u16 = 0x0700; // single-phy param change (param)
    pub const LOCK_FRAME: u16 = 0x1000; // lock last frame (ctl)
    pub const UNLOCK_FRAME: u16 = 0x1100; // unlock last frame (ctl)
    pub const SEND_CARD_PARAM: u16 = 0x0000; // global send-card param incl. brightness (param, target 2)
    pub const SEND_CARD_STATUS: u16 = 0x00FF; // read-back send-card status (ctl)
    pub const SPI_ERASE: u16 = 0x23CC; // erase SPI flash (ctl)
    pub const SPI_READBACK: u16 = 0x34D2; // read back SPI flash (ctl)
}

const CONTROL_DATA: usize = 0x10; // 16
const PARAM_DATA: usize = 0x200; // 512

/// Build the 9-byte sub-header.
fn sub_header(target: Target, card: u8, phy: u8, func: u16, param: u16, data_len: u16) -> [u8; 9] {
    let mut h = [0u8; 9];
    h[0] = target as u8;
    h[1] = card;
    h[2] = phy;
    h[3..5].copy_from_slice(&func.to_le_bytes());
    h[5..7].copy_from_slice(&param.to_le_bytes());
    h[7..9].copy_from_slice(&data_len.to_le_bytes());
    h
}

/// Build a 25-byte CONTROL payload (9-byte header + 16-byte data).
pub fn control(target: Target, card: u8, phy: u8, func: u16, param: u16, data: &[u8]) -> [u8; CONTROL_PAYLOAD] {
    let mut p = [0u8; CONTROL_PAYLOAD];
    p[..9].copy_from_slice(&sub_header(target, card, phy, func, param, CONTROL_DATA as u16));
    let n = data.len().min(CONTROL_DATA);
    p[9..9 + n].copy_from_slice(&data[..n]);
    p
}

/// Build a 521-byte PARAM payload (9-byte header + 512-byte data blob).
pub fn param(target: Target, card: u8, phy: u8, func: u16, param: u16, blob: &[u8]) -> [u8; PARAM_PAYLOAD] {
    let mut p = [0u8; PARAM_PAYLOAD];
    p[..9].copy_from_slice(&sub_header(target, card, phy, func, param, PARAM_DATA as u16));
    let n = blob.len().min(PARAM_DATA);
    p[9..9 + n].copy_from_slice(&blob[..n]);
    p
}

/// Encode a control command straight to a wire frame.
pub fn control_frame(target: Target, card: u8, phy: u8, func: u16, param_off: u16, data: &[u8]) -> Vec<u8> {
    frame::encode(&control(target, card, phy, func, param_off, data)).expect("control payload is 25 B")
}

/// Encode a param command straight to a wire frame.
pub fn param_frame(target: Target, card: u8, phy: u8, func: u16, param_off: u16, blob: &[u8]) -> Vec<u8> {
    frame::encode(&param(target, card, phy, func, param_off, blob)).expect("param payload is 521 B")
}

// ── Common commands (control frames; param frames need a 512-B blob from blob.rs) ──

/// Broadcast search for receiving cards (func 0x0100, target recv-card, card 0).
pub fn search_recv_cards() -> Vec<u8> {
    control_frame(Target::RecvCard, 0, 0, func::SEARCH_OR_BASIC, 0, &[])
}

/// Read back a send-card status frame (func 0x00FF, target send-card).
pub fn read_send_card_status() -> Vec<u8> {
    control_frame(Target::SendCard, 0, 0, func::SEND_CARD_STATUS, 0, &[])
}

/// Lock the last displayed frame on a receiving card (during config updates).
pub fn lock_last_frame(card: u8) -> Vec<u8> {
    control_frame(Target::RecvCard, card, 0, func::LOCK_FRAME, 0, &[])
}

/// Unlock the last displayed frame.
pub fn unlock_last_frame(card: u8) -> Vec<u8> {
    control_frame(Target::RecvCard, card, 0, func::UNLOCK_FRAME, 0, &[])
}

/// Read the send-card temperature (func 0x0500, control).
pub fn read_temp() -> Vec<u8> {
    control_frame(Target::SendCard, 0, 0, func::GEOMETRY_OR_TEMP, 0, &[])
}

/// Check for an HDMI input signal (func 0x0600, control).
pub fn check_hdmi() -> Vec<u8> {
    control_frame(Target::SendCard, 0, 0, func::HDMI_CHECK, 0, &[])
}

/// Send the 512-byte send-card param blob (brightness/global config; func 0x0000, target 2).
pub fn send_card_param(blob: &[u8]) -> Vec<u8> {
    param_frame(Target::SendCard, 0, 0, func::SEND_CARD_PARAM, 0, blob)
}

/// Persist the pushed parameters to the receiving card's flash (func 0x0300 save).
pub fn save_to_recv_card(card: u8) -> Vec<u8> {
    // The save uses the param frame form in the stock code (GenSaveParamToRecvCardAsk);
    // callers pass the same blob they configured.
    control_frame(Target::RecvCard, card, 0, func::GAMMA_OR_SAVE, 1, &[])
}

/// Classify a decoded response payload: returns (target, func, status_ok, hdmi_present).
/// For a control status response (`ParseControllFrame`): target byte is 1, sub-func 0x0400 at [3],
/// status byte [9]==0xAA is OK, [10]==0x99 means HDMI present.
pub fn parse_status(payload: &[u8]) -> Option<(u8, u16, bool, bool)> {
    if payload.len() < 11 {
        return None;
    }
    let target = payload[0];
    let func = u16::from_le_bytes([payload[3], payload[4]]);
    let ok = payload[9] == 0xAA;
    let hdmi = payload[10] == 0x99;
    Some((target, func, ok, hdmi))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame;

    #[test]
    fn control_payload_is_25_and_header_correct() {
        let p = control(Target::RecvCard, 0, 0, func::SEARCH_OR_BASIC, 0, &[]);
        assert_eq!(p.len(), CONTROL_PAYLOAD);
        assert_eq!(p[0], 1); // recv-card
        assert_eq!(u16::from_le_bytes([p[3], p[4]]), 0x0100);
        assert_eq!(u16::from_le_bytes([p[7], p[8]]), 0x10); // control data length
    }

    #[test]
    fn param_payload_is_521_and_datalen_512() {
        let blob = [0x5Au8; 512];
        let p = param(Target::SendCard, 0, 0, func::SEND_CARD_PARAM, 0, &blob);
        assert_eq!(p.len(), PARAM_PAYLOAD);
        assert_eq!(p[0], 2); // send-card
        assert_eq!(u16::from_le_bytes([p[7], p[8]]), 0x200);
        assert_eq!(&p[9..9 + 512], &blob);
    }

    #[test]
    fn search_frame_round_trips_through_codec() {
        let wire = search_recv_cards();
        // control response type is 1; our request also uses target 1 → decodable as control
        let dec = frame::decode(&wire).unwrap();
        assert_eq!(dec.payload.len(), CONTROL_PAYLOAD);
        assert_eq!(u16::from_le_bytes([dec.payload[3], dec.payload[4]]), 0x0100);
    }

    #[test]
    fn parse_status_reads_flags() {
        let mut p = [0u8; CONTROL_PAYLOAD];
        p[0] = 1;
        p[3] = 0x00; p[4] = 0x04; // 0x0400
        p[9] = 0xAA; p[10] = 0x99;
        let (t, f, ok, hdmi) = parse_status(&p).unwrap();
        assert_eq!((t, f, ok, hdmi), (1, 0x0400, true, true));
    }
}
