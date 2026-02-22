/// RSS / Atom feed content renderer plugin (libNetworkData_plugin.so equivalent).
///
/// Fetches an RSS or Atom feed URL periodically and renders item titles as a
/// horizontally-scrolling ticker.
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tiny_skia::Pixmap;
use tracing::debug;

use crate::program::model::{parse_color, ContentItem, RssContent};
use crate::render::plugins::net_text::{
    cached_fetch, draw_text_centered, draw_ticker, fetch_url_sync, load_font, parse_rss_items,
    TextCache,
};
use crate::render::plugins::ContentRenderer;

pub struct RssRenderer {
    font: rusttype::Font<'static>,
    cache: TextCache,
}

impl RssRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn get_text(&self, rss: &RssContent) -> String {
        let url = rss.url.clone();
        let max_age = Duration::from_secs(rss.update_interval.max(1) as u64 * 60);
        let max_items = rss.item_count;
        let separator = rss.separator.clone();
        cached_fetch(&self.cache, &url.clone(), max_age, move || {
            debug!("Fetching RSS feed: {}", url);
            let body = fetch_url_sync(&url)?;
            let text = parse_rss_items(&body, max_items, &separator);
            if text.is_empty() { None } else { Some(text) }
        })
    }

    fn render_rss(&self, rss: &RssContent, target: &mut Pixmap, elapsed_ms: u64) {
        let text = self.get_text(rss);
        let display = if text.is_empty() {
            "Loading RSS feed…".to_string()
        } else {
            text
        };

        let font_size = rss.font.as_ref().map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = rss
            .font
            .as_ref()
            .map(|f| parse_color(&f.color))
            .unwrap_or((255, 255, 255));

        if rss.scroll_speed == 0 {
            draw_text_centered(target, &display, &self.font, font_size, r, g, b);
        } else {
            draw_ticker(
                target,
                &display,
                elapsed_ms,
                rss.scroll_speed,
                &self.font,
                font_size,
                r,
                g,
                b,
            );
        }
    }
}

impl ContentRenderer for RssRenderer {
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
        let rss = match item {
            ContentItem::Rss(r) => r,
            _ => return false,
        };
        self.render_rss(rss, target, elapsed_ms);
        true
    }
}
