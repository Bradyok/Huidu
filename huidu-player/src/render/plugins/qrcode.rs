/// QR code content renderer plugin.
///
/// Encodes the `data` string into a QR code and renders it scaled to fill the
/// area.  The quiet zone (border) is controlled by the `border` attribute.
/// Foreground and background colors are fully configurable.
///
/// QR encoding is expensive (Reed-Solomon ECC), so rendered frames are cached
/// by content + dimensions.  The content of a QR element almost never changes
/// at runtime, so the cache hit rate is effectively 100% after the first frame.
///
/// Variable expansion: `{DS:name}` and the same time tokens as the text renderer
/// (`{TIME}`, `{DATE}`, etc.) are expanded in the `data` field before encoding.
/// Dynamic QR codes (containing `{`) bypass the cache and re-encode every frame.
use std::collections::{HashMap, VecDeque};
use std::path::Path;
use tiny_skia::Pixmap;
use tracing::warn;

use crate::program::model::{parse_color, ContentItem, QrCodeContent};
use crate::render::plugins::text::{expand_ds_tokens, expand_variables, has_variables};
use crate::render::plugins::ContentRenderer;

/// Maximum number of rendered QR codes kept in memory.
const MAX_CACHE: usize = 32;

/// Cache key: all parameters that affect the rendered output.
#[derive(Hash, PartialEq, Eq, Clone)]
struct QrCodeKey {
    data: String,
    width: u32,
    height: u32,
    border: u32,
    fg_color: String,
    bg_color: String,
}

pub struct QrCodeRenderer {
    cache: HashMap<QrCodeKey, Pixmap>,
    keys: VecDeque<QrCodeKey>,
    /// Current data source values, updated each second by the player.
    data_sources: HashMap<String, String>,
}

impl QrCodeRenderer {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            keys: VecDeque::new(),
            data_sources: HashMap::new(),
        }
    }

    /// Replace the stored data sources with a fresh snapshot.
    pub fn set_data_sources(&mut self, sources: HashMap<String, String>) {
        self.data_sources = sources;
    }

    fn render_qr(&mut self, qr: &QrCodeContent, target: &mut Pixmap) {
        if qr.data.is_empty() {
            return;
        }

        // Expand variable tokens ({DS:name}, {TIME}, …) if present.
        // Dynamic data bypasses the cache since the value changes every frame.
        let is_dynamic = has_variables(&qr.data);
        let expanded_data;
        let data_str: &str = if is_dynamic {
            let time_exp = expand_variables(&qr.data);
            expanded_data = expand_ds_tokens(&time_exp, &self.data_sources);
            &expanded_data
        } else {
            &qr.data
        };

        let key = QrCodeKey {
            data: data_str.to_string(),
            width: target.width(),
            height: target.height(),
            border: qr.border,
            fg_color: qr.fg_color.clone(),
            bg_color: qr.bg_color.clone(),
        };

        // Cache hit: copy pre-rendered pixels into target
        if !is_dynamic
            && let Some(cached) = self.cache.get(&key) {
                target.data_mut().copy_from_slice(cached.data());
                return;
            }

        // Cache miss: encode and render into a temporary Pixmap
        let mut tmp = match Pixmap::new(target.width(), target.height()) {
            Some(p) => p,
            None => return,
        };

        // Build a temporary QrCodeContent with the expanded data string
        let mut qr_expanded = qr.clone();
        qr_expanded.data = data_str.to_string();

        if render_into_pixmap(&qr_expanded, &mut tmp) {
            target.data_mut().copy_from_slice(tmp.data());
            // Only cache static content
            if !is_dynamic {
                if self.cache.len() >= MAX_CACHE
                    && let Some(oldest) = self.keys.pop_front() {
                        self.cache.remove(&oldest);
                    }
                self.cache.insert(key.clone(), tmp);
                self.keys.push_back(key);
            }
        }
    }
}

/// Render a QR code into `target`, returning whether any pixels were written.
fn render_into_pixmap(qr: &QrCodeContent, target: &mut Pixmap) -> bool {
    let code = match qrcode::QrCode::new(qr.data.as_bytes()) {
        Ok(c) => c,
        Err(e) => {
            warn!("QR encode failed for '{}': {}", qr.data, e);
            return false;
        }
    };

    let (fg_r, fg_g, fg_b) = parse_color(&qr.fg_color);
    let (bg_r, bg_g, bg_b) = parse_color(&qr.bg_color);

    // Module count including quiet zone
    let modules = code.width();
    let border = qr.border as usize;
    let total = modules + border * 2;

    let tw = target.width() as usize;
    let th = target.height() as usize;

    // Module size in pixels (integer, uniform scaling)
    let module_px = (tw.min(th) / total).max(1);

    // Offset to centre the code in the area
    let off_x = (tw.saturating_sub(total * module_px)) / 2;
    let off_y = (th.saturating_sub(total * module_px)) / 2;

    let data = target.data_mut();

    // Fill background
    for chunk in data.chunks_exact_mut(4) {
        chunk[0] = bg_r;
        chunk[1] = bg_g;
        chunk[2] = bg_b;
        chunk[3] = 255;
    }

    // Paint modules
    let colors = code.to_colors();
    for row in 0..modules {
        for col in 0..modules {
            let is_dark = matches!(colors[row * modules + col], qrcode::Color::Dark);
            let (pr, pg, pb) = if is_dark {
                (fg_r, fg_g, fg_b)
            } else {
                (bg_r, bg_g, bg_b)
            };

            let px0 = off_x + (border + col) * module_px;
            let py0 = off_y + (border + row) * module_px;

            for dy in 0..module_px {
                for dx in 0..module_px {
                    let px = px0 + dx;
                    let py = py0 + dy;
                    if px < tw && py < th {
                        let i = (py * tw + px) * 4;
                        data[i] = pr;
                        data[i + 1] = pg;
                        data[i + 2] = pb;
                        data[i + 3] = 255;
                    }
                }
            }
        }
    }

    true
}

impl ContentRenderer for QrCodeRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        _width: u32,
        _height: u32,
        _elapsed_ms: u64,
        _program_dir: &Path,
    ) -> bool {
        let qr = match item {
            ContentItem::QrCode(q) => q,
            _ => return false,
        };
        self.render_qr(qr, target);
        true
    }
}
