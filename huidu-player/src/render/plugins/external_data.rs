/// External-data content renderer plugin (DynamicData_plugin.dll / libDynamicData_plugin.so equivalent).
///
/// Fetches a JSON, XML, or plain-text URL periodically, extracts a value using
/// a dot-notation path or XML element name, applies an optional format string,
/// and renders the result as static or scrolling text.
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::Pixmap;
use tracing::debug;

use crate::program::model::{parse_color, ContentItem, ExternalDataContent};
use crate::render::plugins::net_text::{
    cached_fetch, draw_text_centered, draw_ticker, extract_path_value, fetch_url_sync, load_font,
    TextCache,
};
use crate::render::plugins::ContentRenderer;

pub struct ExternalDataRenderer {
    font: rusttype::Font<'static>,
    cache: TextCache,
}

impl ExternalDataRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_value(&self, ext: &ExternalDataContent) -> String {
        let url = ext.url.clone();
        let path = ext.path.clone();
        let max_age = Duration::from_secs(ext.update_interval.max(1) as u64 * 60);
        // Cache key includes path so the same URL can serve different fields
        let cache_key = format!("{}|{}", url, path);
        cached_fetch(&self.cache, &cache_key, max_age, move || {
            debug!("Fetching external data: {} (path={})", url, path);
            let body = fetch_url_sync(&url)?;
            let value = extract_path_value(&body, &path);
            if value.is_empty() { None } else { Some(value) }
        })
    }

    fn render_external(
        &self,
        ext: &ExternalDataContent,
        target: &mut Pixmap,
        elapsed_ms: u64,
    ) {
        let raw = self.get_value(ext);
        let display = if raw.is_empty() {
            "Loading…".to_string()
        } else if ext.format.contains("{value}") {
            ext.format.replace("{value}", &raw)
        } else if !ext.format.is_empty() {
            format!("{}{}", ext.format, raw)
        } else {
            raw
        };

        let font_size = ext.font.as_ref().map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = ext
            .font
            .as_ref()
            .map(|f| parse_color(&f.color))
            .unwrap_or((255, 255, 255));

        if ext.scroll_speed == 0 {
            draw_text_centered(target, &display, &self.font, font_size, r, g, b);
        } else {
            draw_ticker(
                target,
                &display,
                elapsed_ms,
                ext.scroll_speed,
                &self.font,
                font_size,
                r,
                g,
                b,
            );
        }
    }
}

impl ContentRenderer for ExternalDataRenderer {
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
        let ext = match item {
            ContentItem::ExternalData(e) => e,
            _ => return false,
        };
        self.render_external(ext, target, elapsed_ms);
        true
    }
}
