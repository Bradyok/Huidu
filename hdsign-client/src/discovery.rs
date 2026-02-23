//! UDP device discovery — thin wrapper around `huidu_protocol::discovery`.
pub use huidu_protocol::discovery::{
    DISCOVERY_PORT, BOXPLAYER_VERSION,
    DeviceInfo, Discovery,
    build_device_info_packet, build_ext1_packet, get_local_ip,
};

/// Default device IP used when connecting to a card in WiFi AP mode.
pub const WIFI_AP_DEFAULT_IP: &str = "192.168.4.1";

/// Default HTTP port for WiFi AP cards.
pub const WIFI_AP_HTTP_PORT: u16 = 6104;
