/// Weather widget content renderer plugin (libweather_plugin.so equivalent).
///
/// Fetches weather data from a configurable HTTP endpoint and renders it.
/// Supported backends:
///   - OpenWeatherMap:  https://api.openweathermap.org/data/2.5/weather?q={city}&appid={key}
///   - wttr.in (no key): https://wttr.in/{city}?format=j1
///   - Custom JSON URL via WeatherContent.city_code
///
/// When weather fetch fails or no URL is configured the widget shows the
/// city name and "N/A" placeholders.
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use tracing::{debug, warn};

use crate::program::model::{parse_color, ContentItem, WeatherContent};
use crate::render::plugins::ContentRenderer;

/// Cached weather reading for one city
#[derive(Debug, Clone)]
pub struct WeatherData {
    pub temp_c: f32,
    pub humidity: u8,
    pub condition: String,
    pub icon: String,
    pub fetched_at: Instant,
}

impl Default for WeatherData {
    fn default() -> Self {
        Self {
            temp_c: 0.0,
            humidity: 0,
            condition: "N/A".to_string(),
            icon: "?".to_string(),
            fetched_at: Instant::now() - Duration::from_secs(3600),
        }
    }
}

pub struct WeatherRenderer {
    font: rusttype::Font<'static>,
    /// Cached weather per city code
    cache: Arc<Mutex<HashMap<String, WeatherData>>>,
    /// Base URL for weather API (from config), e.g. "https://wttr.in/{city}?format=j1"
    api_base: String,
}

impl WeatherRenderer {
    pub fn new(api_base: String) -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self {
            font,
            cache: Arc::new(Mutex::new(HashMap::new())),
            api_base,
        }
    }

    fn get_weather(&self, city: &str, city_code: &str, update_interval_mins: u32) -> WeatherData {
        let key = if city_code.is_empty() { city } else { city_code };

        // Check cache freshness
        {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(data) = cache.get(key) {
                let age = data.fetched_at.elapsed();
                if age < Duration::from_secs(update_interval_mins as u64 * 60) {
                    return data.clone();
                }
            }
        }

        // Fetch weather asynchronously (fire-and-forget via std::thread)
        // For now return cached/default data and trigger background refresh
        let key_owned = key.to_string();
        let city_owned = city.to_string();
        let cache_clone = self.cache.clone();
        let api = self.api_base.clone();

        std::thread::spawn(move || {
            if let Some(data) = fetch_weather_sync(&api, &city_owned, &key_owned) {
                let mut cache = cache_clone.lock().unwrap_or_else(|e| e.into_inner());
                cache.insert(key_owned, data);
            }
        });

        // Return current cached value (may be default/stale)
        let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.get(key).cloned().unwrap_or_default()
    }

    fn render_weather(&self, weather: &WeatherContent, target: &mut Pixmap, width: u32, height: u32) {
        let data = self.get_weather(&weather.city, &weather.city_code, weather.update_interval);

        let font_spec = weather.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = font_spec
            .map(|f| parse_color(&f.color))
            .unwrap_or((255, 255, 255));

        // Lines to render: city, temperature (+icon), condition
        let city_display = if weather.city.is_empty() { "Weather".to_string() } else { weather.city.clone() };
        let base_temp = if weather.temp_unit.eq_ignore_ascii_case("F") {
            format!("{:.0}°F  {}%", data.temp_c * 9.0 / 5.0 + 32.0, data.humidity)
        } else {
            format!("{:.0}°C  {}%", data.temp_c, data.humidity)
        };
        let temp_str = if weather.show_icon {
            let icon = condition_to_icon(&data.condition);
            format!("{} {}", icon, base_temp)
        } else {
            base_temp
        };
        let lines = [
            city_display,
            temp_str,
            data.condition.clone(),
        ];

        let scale = rusttype::Scale::uniform(font_size);
        let v_metrics = self.font.v_metrics(scale);
        let line_h = (v_metrics.ascent - v_metrics.descent + v_metrics.line_gap).ceil() as i32;

        let total_h = line_h * lines.len() as i32;
        let start_y = ((height as i32 - total_h) / 2).max(0);

        let tw = target.width() as i32;
        let th = target.height() as i32;

        for (i, line) in lines.iter().enumerate() {
            let glyphs: Vec<_> = self
                .font
                .layout(line, scale, rusttype::point(0.0, v_metrics.ascent))
                .collect();

            let text_w = glyphs
                .last()
                .and_then(|g| g.pixel_bounding_box().map(|bb| bb.max.x))
                .unwrap_or(0);
            let x_off = ((width as i32 - text_w) / 2).max(0);
            let y_off = start_y + i as i32 * line_h;

            let data_pixels = target.data_mut();
            for glyph in &glyphs {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph.draw(|gx, gy, v| {
                        let px = x_off + bb.min.x + gx as i32;
                        let py = y_off + bb.min.y + gy as i32;
                        if px < 0 || px >= tw || py < 0 || py >= th { return; }
                        let alpha = (v * 255.0) as u8;
                        if alpha == 0 { return; }
                        let idx = ((py * tw + px) * 4) as usize;
                        let a = alpha as f32 / 255.0;
                        let dst_a = data_pixels[idx + 3] as f32 / 255.0;
                        let out_a = a + dst_a * (1.0 - a);
                        if out_a > 0.0 {
                            data_pixels[idx]     = ((r as f32 * a + data_pixels[idx]     as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                            data_pixels[idx + 1] = ((g as f32 * a + data_pixels[idx + 1] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                            data_pixels[idx + 2] = ((b as f32 * a + data_pixels[idx + 2] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                            data_pixels[idx + 3] = (out_a * 255.0) as u8;
                        }
                    });
                }
            }
        }

        debug!("Weather rendered: {} {:.0}°C", weather.city, data.temp_c);
    }
}

impl ContentRenderer for WeatherRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        width: u32,
        height: u32,
        _elapsed_ms: u64,
        _program_dir: &Path,
    ) -> bool {
        let weather = match item {
            ContentItem::Weather(w) => w,
            _ => return false,
        };
        self.render_weather(weather, target, width, height);
        true
    }
}

/// Synchronous weather fetch using a minimal HTTP GET (no async runtime needed
/// since this runs in a std::thread).
fn fetch_weather_sync(api_base: &str, city: &str, _city_code: &str) -> Option<WeatherData> {
    if api_base.is_empty() {
        return None;
    }

    // Build URL — support {city} placeholder or append city as path component
    let url = if api_base.contains("{city}") {
        api_base.replace("{city}", city)
    } else {
        format!("{}/{}", api_base.trim_end_matches('/'), city)
    };

    debug!("Fetching weather from: {}", url);

    // Use reqwest blocking client
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .ok()?;

    let resp = client.get(&url).send().ok()?;
    let text = resp.text().ok()?;

    // Try to parse wttr.in JSON format (j1)
    if let Some(data) = parse_wttr_json(&text) {
        return Some(data);
    }
    // Try OpenWeatherMap format
    if let Some(data) = parse_owm_json(&text) {
        return Some(data);
    }

    warn!("Could not parse weather response from {}", url);
    None
}

/// Parse wttr.in `?format=j1` JSON using serde_json.
/// Format: `{"current_condition":[{"temp_C":"20","humidity":"60","weatherDesc":[{"value":"Sunny"}]}]}`
fn parse_wttr_json(json: &str) -> Option<WeatherData> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let cc = v.get("current_condition")?.get(0)?;
    let temp: f32 = cc.get("temp_C")?.as_str()?.parse().ok()?;
    let humidity: u8 = cc.get("humidity")?.as_str()?.parse().ok()?;
    let condition = cc
        .get("weatherDesc")
        .and_then(|a| a.get(0))
        .and_then(|o| o.get("value"))
        .and_then(|v| v.as_str())
        .unwrap_or("N/A")
        .to_string();
    Some(WeatherData {
        temp_c: temp,
        humidity,
        icon: condition_to_icon(&condition).to_string(),
        condition,
        fetched_at: Instant::now(),
    })
}

/// Parse OpenWeatherMap current weather JSON using serde_json.
/// Format: `{"main":{"temp":293.15,"humidity":60},"weather":[{"description":"clear sky"}]}`
fn parse_owm_json(json: &str) -> Option<WeatherData> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let temp_k: f32 = v.get("main")?.get("temp")?.as_f64()? as f32;
    let humidity: u8 = v.get("main")?.get("humidity")?.as_u64()? as u8;
    let condition = v
        .get("weather")
        .and_then(|a| a.get(0))
        .and_then(|o| o.get("description"))
        .and_then(|s| s.as_str())
        .unwrap_or("N/A")
        .to_string();
    let temp_c = temp_k - 273.15;
    Some(WeatherData {
        temp_c,
        humidity,
        icon: condition_to_icon(&condition).to_string(),
        condition,
        fetched_at: Instant::now(),
    })
}

fn weather_icon(condition: &str) -> String {
    condition_to_icon(condition).to_string()
}

fn condition_to_icon(condition: &str) -> &'static str {
    let c = condition.to_ascii_lowercase();
    if c.contains("clear") || c.contains("sunny") { "☀" }
    else if c.contains("partly") || c.contains("partly cloudy") { "⛅" }
    else if c.contains("cloud") || c.contains("overcast") { "☁" }
    else if c.contains("rain") || c.contains("drizzle") { "🌧" }
    else if c.contains("snow") || c.contains("sleet") { "❄" }
    else if c.contains("storm") || c.contains("thunder") { "⚡" }
    else if c.contains("fog") || c.contains("mist") || c.contains("haze") { "🌫" }
    else if c.contains("wind") || c.contains("breezy") { "💨" }
    else { "🌡" }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── parse_wttr_json ───────────────────────────────────────────────────────

    #[test]
    fn test_parse_wttr_json_valid() {
        let json = r#"{
            "current_condition": [{
                "temp_C": "22",
                "humidity": "65",
                "weatherDesc": [{"value": "Partly cloudy"}]
            }]
        }"#;
        let data = parse_wttr_json(json).unwrap();
        assert!((data.temp_c - 22.0).abs() < 0.01);
        assert_eq!(data.humidity, 65);
        assert_eq!(data.condition, "Partly cloudy");
    }

    #[test]
    fn test_parse_wttr_json_invalid_returns_none() {
        assert!(parse_wttr_json("not json").is_none());
        assert!(parse_wttr_json("{}").is_none());
    }

    #[test]
    fn test_parse_wttr_json_missing_desc_uses_na() {
        let json = r#"{
            "current_condition": [{
                "temp_C": "10",
                "humidity": "80"
            }]
        }"#;
        // Should succeed despite missing weatherDesc, falling back to "N/A"
        let data = parse_wttr_json(json).unwrap();
        assert_eq!(data.condition, "N/A");
    }

    // ── parse_owm_json ────────────────────────────────────────────────────────

    #[test]
    fn test_parse_owm_json_valid() {
        // 300.15 K − 273.15 = 27.0 °C
        let json = r#"{
            "main": {"temp": 300.15, "humidity": 70},
            "weather": [{"description": "clear sky"}]
        }"#;
        let data = parse_owm_json(json).unwrap();
        assert!((data.temp_c - 27.0).abs() < 0.1);
        assert_eq!(data.humidity, 70);
        assert_eq!(data.condition, "clear sky");
    }

    #[test]
    fn test_parse_owm_json_invalid_returns_none() {
        assert!(parse_owm_json("not json").is_none());
        assert!(parse_owm_json("{}").is_none());
    }

    // ── condition_to_icon ─────────────────────────────────────────────────────

    #[test]
    fn test_condition_to_icon() {
        assert_eq!(condition_to_icon("clear sky"), "☀");
        assert_eq!(condition_to_icon("Sunny"), "☀");
        assert_eq!(condition_to_icon("Partly cloudy"), "⛅");
        assert_eq!(condition_to_icon("Overcast"), "☁");
        assert_eq!(condition_to_icon("Light rain"), "🌧");
        assert_eq!(condition_to_icon("Snow"), "❄");
        assert_eq!(condition_to_icon("Thunderstorm"), "⚡");
        assert_eq!(condition_to_icon("Fog"), "🌫");
        assert_eq!(condition_to_icon("Windy"), "💨");
        assert_eq!(condition_to_icon("N/A"), "🌡");
    }
}
