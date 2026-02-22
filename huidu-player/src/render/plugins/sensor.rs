/// Hardware sensor content renderer (ds18b20, cpu_temp, dht22, generic_file).
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ContentItem};
use crate::render::plugins::ContentRenderer;
use crate::render::plugins::net_text::{draw_text_centered, draw_ticker, load_font, TextCache};

pub struct SensorRenderer {
    font: rusttype::Font<'static>,
    cache: TextCache,
}

impl SensorRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }
}

impl ContentRenderer for SensorRenderer {
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
            ContentItem::Sensor(c) => c,
            _ => return false,
        };

        let font_spec = content.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = font_spec
            .map(|f| parse_color(&f.color))
            .unwrap_or((0, 255, 128));

        let cache_key = format!("sensor:{}:{}", content.sensor_type, content.device);
        let max_age = Duration::from_secs(content.update_interval as u64);
        let sensor_type = content.sensor_type.clone();
        let device = content.device.clone();

        let raw = crate::render::plugins::net_text::cached_fetch(
            &self.cache,
            &cache_key,
            max_age,
            move || read_sensor(&sensor_type, &device),
        );

        let display = if raw.is_empty() {
            format!("Sensor: {}", content.sensor_type)
        } else if content.format.is_empty() || content.format == "{value}" {
            raw.clone()
        } else {
            content.format.replace("{value}", &raw)
        };

        if content.scroll_speed > 0 {
            draw_ticker(target, &display, elapsed_ms, content.scroll_speed, &self.font, font_size, r, g, b);
        } else {
            draw_text_centered(target, &display, &self.font, font_size, r, g, b);
        }

        true
    }
}

fn read_sensor(sensor_type: &str, device: &str) -> Option<String> {
    match sensor_type {
        "ds18b20" => read_ds18b20(device),
        "cpu_temp" => read_cpu_temp(),
        "dht22" => read_dht22(device),
        _ => read_generic_file(device),
    }
}

fn read_ds18b20(device: &str) -> Option<String> {
    // Try glob pattern for DS18B20 devices
    let paths: Vec<_> = if device.is_empty() {
        // Auto-discover
        std::fs::read_dir("/sys/bus/w1/devices")
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path().join("w1_slave"))
            .filter(|p| p.exists())
            .collect()
    } else {
        let p = std::path::PathBuf::from(device).join("w1_slave");
        if p.exists() { vec![p] } else { return None; }
    };

    for path in paths {
        if let Ok(content) = std::fs::read_to_string(&path) {
            // Parse "t=21500" → 21.5
            if let Some(t_pos) = content.find("t=") {
                let t_str = content[t_pos + 2..].split_whitespace().next()?;
                if let Ok(raw) = t_str.parse::<i32>() {
                    return Some(format!("{:.1}", raw as f32 / 1000.0));
                }
            }
        }
    }
    None
}

fn read_cpu_temp() -> Option<String> {
    let content = std::fs::read_to_string("/sys/class/thermal/thermal_zone0/temp").ok()?;
    let raw: i32 = content.trim().parse().ok()?;
    Some(format!("{:.1}", raw as f32 / 1000.0))
}

fn read_dht22(device: &str) -> Option<String> {
    use std::process::Command;
    let script = format!(
        "import adafruit_dht, board; d=adafruit_dht.DHT22(board.{}); print(f'{{d.temperature:.1f}}')",
        if device.is_empty() { "D4" } else { device }
    );
    let output = Command::new("python3")
        .args(["-c", &script])
        .output()
        .ok()?;
    if output.status.success() {
        let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !s.is_empty() { Some(s) } else { None }
    } else {
        None
    }
}

fn read_generic_file(path: &str) -> Option<String> {
    if path.is_empty() { return None; }
    let content = std::fs::read_to_string(path).ok()?;
    Some(content.lines().next()?.trim().to_string())
}
