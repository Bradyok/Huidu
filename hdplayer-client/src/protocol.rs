//! Re-export the shared binary framing layer for use within this crate.
//!
//! All packet types and SDK payload helpers come from `huidu_protocol::packet`.
pub use huidu_protocol::packet::{
    Command, Packet,
    BOX_PLAYER_PORT,
    SDK_CLIENT_VERSION, SDK_TRANSPORT_VERSION,
    sdk_service_ask_payload, sdk_cmd_ask_payload, sdk_cmd_answer_payload,
    parse_sdk_cmd_payload,
    box_stream_data_request, parse_box_stream_data,
};
