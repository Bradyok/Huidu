/// Modbus TCP polling service — polls configured registers and stores results
/// as data sources in ServicesState.
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::info;

use super::manager::ServicesState;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ModbusSourceConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_slave")]
    pub slave: u8,
    pub register: u16,
    pub name: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default = "default_interval")]
    pub interval_secs: u64,
}

fn default_port() -> u16 { 502 }
fn default_slave() -> u8 { 1 }
fn default_scale() -> f64 { 1.0 }
fn default_interval() -> u64 { 5 }

pub struct ModbusService;

impl ModbusService {
    pub async fn run(
        state: Arc<RwLock<ServicesState>>,
        cancel: CancellationToken,
    ) {
        info!("Modbus polling service starting");

        loop {
            // Get current sources configuration
            let sources: Vec<ModbusSourceConfig> = {
                let s = state.read().await;
                s.modbus_sources.clone()
            };

            if !sources.is_empty() {
                // Poll each source in a blocking thread
                let results = tokio::task::spawn_blocking(move || {
                    sources.iter().map(|src| {
                        let value = read_modbus_register(&src.host, src.port, src.slave, src.register);
                        let text = value.map(|v| format!("{:.2}", v * src.scale))
                            .unwrap_or_else(|| "ERR".to_string());
                        (src.name.clone(), text)
                    }).collect::<Vec<_>>()
                }).await.unwrap_or_default();

                // Store results as data sources
                let mut s = state.write().await;
                for (name, value) in results {
                    s.data_sources.insert(format!("DS:{}", name), value);
                }
            }

            tokio::select! {
                _ = cancel.cancelled() => {
                    info!("Modbus polling service stopping");
                    return;
                }
                _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
            }
        }
    }
}

fn read_modbus_register(host: &str, port: u16, slave: u8, register: u16) -> Option<f64> {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let addr = format!("{}:{}", host, port);
    let sock_addr: std::net::SocketAddr = addr.parse().ok()?;
    let mut stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(3)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;

    let start_addr = register.saturating_sub(1);
    let request = [
        0x00, 0x01, 0x00, 0x00, 0x00, 0x06,
        slave, 0x03,
        (start_addr >> 8) as u8, (start_addr & 0xff) as u8,
        0x00, 0x01,
    ];

    stream.write_all(&request).ok()?;
    let mut response = [0u8; 256];
    let n = stream.read(&mut response).ok()?;
    if n < 11 { return None; }
    let hi = response[9] as u16;
    let lo = response[10] as u16;
    Some(((hi << 8) | lo) as f64)
}

