/// QR code content renderer plugin.
///
/// Encodes the `data` string into a QR code and renders it scaled to fill the
/// area.  The quiet zone (border) is controlled by the `border` attribute.
/// Foreground and background colors are fully configurable.
use std::path::Path;
use tiny_skia::Pixmap;
use tracing::warn;

use crate::program::model::{parse_color, ContentItem, QrCodeContent};
use crate::render::plugins::ContentRenderer;

pub struct QrCodeRenderer;

impl QrCodeRenderer {
    pub fn new() -> Self {
        Self
    }

    fn render_qr(&self, qr: &QrCodeContent, target: &mut Pixmap) {
        if qr.data.is_empty() {
            return;
        }

        let code = match qrcode::QrCode::new(qr.data.as_bytes()) {
            Ok(c) => c,
            Err(e) => {
                warn!("QR encode failed for '{}': {}", qr.data, e);
                return;
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
    }
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
