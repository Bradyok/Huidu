//! UDP device discovery — thin wrapper around `huidu_protocol::discovery`.
pub use huidu_protocol::discovery::{
    DISCOVERY_PORT, BOXPLAYER_VERSION,
    DeviceInfo, Discovery,
    build_device_info_packet, build_ext1_packet, get_local_ip,
};
