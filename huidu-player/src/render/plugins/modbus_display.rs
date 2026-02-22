/// Modbus TCP data display renderer.
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ContentItem};
use crate::render::plugins::ContentRenderer;
use crate::render::plugins::net_text::{draw_text_centered, draw_ticker, load_font, TextCache};

pub struct ModbusDisplayRenderer {
    font: rusttype::Font<'static>,
    cache: TextCache,
}

impl ModbusDisplayRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }
}

impl ContentRenderer for ModbusDisplayRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        _width: u32,
        _height: u32,
        elapsed_ms: u64,
        _program_dir: &Path,
    ) -> bool {
        let content = match item {
            ContentItem::Modbus(c) => c,
            _ => return false,
        };

        let font_spec = content.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = font_spec
            .map(|f| parse_color(&f.color))
            .unwrap_or((255, 255, 0));

        let cache_key = format!("{}:{}/s{}/r{}", content.host, content.port, content.slave, content.register);
        let max_age = Duration::from_secs(content.update_interval as u64);

        let host = content.host.clone();
        let port = content.port;
        let slave = content.slave;
        let register = content.register;
        let scale_factor = content.scale;

        let raw_value = crate::render::plugins::net_text::cached_fetch(
            &self.cache,
            &cache_key,
            max_age,
            move || {
                read_modbus_register(&host, port, slave, register)
                    .map(|v| format!("{:.6}", v * scale_factor))
            },
        );

        let display_text = if raw_value.is_empty() {
            format!("Modbus: {}:{}", content.host, content.port)
        } else {
            let val_str = raw_value.parse::<f64>()
                .map(|v| format!("{:.1}", v))
                .unwrap_or(raw_value);
            if content.format.is_empty() {
                val_str
            } else {
                content.format.replace("{value}", &val_str)
            }
        };

        if content.scroll_speed > 0 {
            draw_ticker(target, &display_text, elapsed_ms, content.scroll_speed, &self.font, font_size, r, g, b);
        } else {
            draw_text_centered(target, &display_text, &self.font, font_size, r, g, b);
        }

        true
    }
}

/// Read a single holding register from a Modbus TCP device.
/// Returns the register value as f64, or None on error.
fn read_modbus_register(host: &str, port: u16, slave: u8, register: u16) -> Option<f64> {
    use std::io::{Read, Write};
    use std::net::TcpStream;

    let addr = format!("{}:{}", host, port);
    let mut stream = TcpStream::connect_timeout(
        &addr.parse().ok()?,
        Duration::from_secs(3),
    ).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;

    // MBAP header + PDU: Read Holding Registers (FC=0x03)
    // transaction_id=1, protocol_id=0, length=6, unit_id=slave
    // function_code=0x03, start_address, quantity=1
    let start_addr = register.saturating_sub(1); // Modbus address is 1-based
    let request = [
        0x00, 0x01, // transaction ID
        0x00, 0x00, // protocol ID
        0x00, 0x06, // length
        slave,      // unit ID
        0x03,       // function code: Read Holding Registers
        (start_addr >> 8) as u8,
        (start_addr & 0xff) as u8,
        0x00, 0x01, // quantity = 1
    ];

    stream.write_all(&request).ok()?;

    let mut response = [0u8; 256];
    let n = stream.read(&mut response).ok()?;
    if n < 9 { return None; }

    // Response: MBAP(7) + FC(1) + byte_count(1) + data(2)
    let byte_count = response[8] as usize;
    if n < 9 + byte_count { return None; }
    if byte_count < 2 { return None; }

    let hi = response[9] as u16;
    let lo = response[10] as u16;
    let value = (hi << 8) | lo;
    Some(value as f64)
}
