//! Re-export the shared binary framing layer.
pub use huidu_protocol::packet::{
    Command, Packet, SDK_CLIENT_VERSION, SDK_TRANSPORT_VERSION,
    sdk_service_ask_payload, sdk_cmd_ask_payload, sdk_cmd_answer_payload,
    parse_sdk_cmd_payload,
};
