/// Web-page content renderer plugin (web_plugin.dll / libweb_plugin.so equivalent).
///
/// Fetches the page HTML periodically and renders its textual content as a
/// horizontally-scrolling ticker.  Full browser rendering is not attempted —
/// this targets informational URL endpoints that return readable text or simple HTML.
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::Pixmap;
use tracing::debug;

use crate::program::model::{parse_color, ContentItem, WebContent};
use crate::render::plugins::net_text::{
    cached_fetch, draw_text_centered, draw_ticker, extract_xml_text, fetch_url_sync, load_font,
    strip_html, TextCache,
};
use crate::render::plugins::ContentRenderer;

pub struct WebRenderer {
    font: rusttype::Font<'static>,
    cache: TextCache,
}

impl WebRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_text(&self, web: &WebContent) -> String {
        let url = web.url.clone();
        let max_age = Duration::from_secs(web.update_interval.max(1) as u64 * 60);
        cached_fetch(&self.cache, &url.clone(), max_age, move || {
            debug!("Fetching web page: {}", url);
            let body = fetch_url_sync(&url)?;
            // Extract <title> + strip body
            let title = extract_xml_text(&body, "title")
                .map(|t| strip_html(&t))
                .unwrap_or_default();
            let body_text = strip_html(&body);
            let combined = if title.is_empty() {
                body_text
            } else {
                format!("{}: {}", title, body_text)
            };
            // Trim to a readable length
            let trimmed: String = if combined.chars().count() > 750 {
                combined.chars().take(750).collect::<String>() + "..."
            } else {
                combined
            };
            if trimmed.is_empty() { None } else { Some(trimmed) }
        })
    }

    fn render_web(&self, web: &WebContent, target: &mut Pixmap, elapsed_ms: u64) {
        let text = self.get_text(web);
        let host = web
            .url
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or(&web.url);
        let display = if text.is_empty() {
            format!("Loading {}…", host)
        } else {
            text
        };

        let font_size = web.font.as_ref().map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = web
            .font
            .as_ref()
            .map(|f| parse_color(&f.color))
            .unwrap_or((255, 255, 255));

        if web.scroll_speed == 0 {
            draw_text_centered(target, &display, &self.font, font_size, r, g, b);
        } else {
            draw_ticker(
                target,
                &display,
                elapsed_ms,
                web.scroll_speed,
                &self.font,
                font_size,
                r,
                g,
                b,
            );
        }
    }
}

impl ContentRenderer for WebRenderer {
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
        let web = match item {
            ContentItem::Web(w) => w,
            _ => return false,
        };
        self.render_web(web, target, elapsed_ms);
        true
    }
}
