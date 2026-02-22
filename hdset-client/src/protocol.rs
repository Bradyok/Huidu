//! Re-export the shared binary framing layer for use within this crate.
//!
//! All packet types, command codes (including HDSet FPGA/screen/boot variants),
//! and SDK payload helpers come from `huidu_protocol::packet`.
pub use huidu_protocol::packet::{
    Command, Packet, SDK_CLIENT_VERSION, SDK_TRANSPORT_VERSION,
    sdk_service_ask_payload, sdk_cmd_ask_payload, sdk_cmd_answer_payload,
    parse_sdk_cmd_payload,
};
