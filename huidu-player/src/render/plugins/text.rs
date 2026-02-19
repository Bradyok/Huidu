/// Text content renderer plugin.
/// Renders static and scrolling text using rusttype for font rasterization.
use std::path::Path;
use tiny_skia::Pixmap;
use tracing::debug;

use crate::program::model::{parse_color, ContentItem, TextContent};
use crate::render::plugins::ContentRenderer;

pub struct TextRenderer {
    font: rusttype::Font<'static>,
}

impl TextRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self { font }
    }

    fn render_text_content(
        &self,
        text: &TextContent,
        target: &mut Pixmap,
        width: u32,
        height: u32,
        elapsed_ms: u64,
    ) {
        let content = match &text.string {
            Some(s) => s.as_str(),
            None => return,
        };

        if content.is_empty() {
            return;
        }

        // Get font properties
        let font_spec = text.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let (r, g, b) = font_spec
            .map(|f| parse_color(&f.color))
            .unwrap_or((255, 0, 0));

        let scale = rusttype::Scale::uniform(font_size);
        let v_metrics = self.font.v_metrics(scale);
        let line_height = (v_metrics.ascent - v_metrics.descent + v_metrics.line_gap).ceil() as i32;

        // Layout glyphs
        let glyphs: Vec<_> = self
            .font
            .layout(content, scale, rusttype::point(0.0, v_metrics.ascent))
            .collect();

        if glyphs.is_empty() {
            return;
        }

        // Calculate text bounding box
        let text_width = glyphs
            .last()
            .map(|g| {
                if let Some(bb) = g.pixel_bounding_box() {
                    bb.max.x
                } else {
                    g.position().x as i32 + (font_size * 0.6) as i32
                }
            })
            .unwrap_or(0);

        // Calculate alignment offset
        let style = text.style.as_ref();
        let align = style.map(|s| s.align.as_str()).unwrap_or("center");
        let valign = style.map(|s| s.valign.as_str()).unwrap_or("middle");

        let offset_x = match align {
            "left" => 0,
            "right" => (width as i32 - text_width).max(0),
            _ => ((width as i32 - text_width) / 2).max(0),
        };

        let offset_y = match valign {
            "top" => 0,
            "bottom" => (height as i32 - line_height).max(0),
            _ => ((height as i32 - line_height) / 2).max(0),
        };

        // Handle single-line scrolling
        let scroll_offset = if text.single_line && text_width > width as i32 {
            let total_scroll = text_width + width as i32;
            let scroll_period_ms = (total_scroll as u64 * 1000) / 50;
            let progress = (elapsed_ms % scroll_period_ms) as i32;
            let scroll_px = (progress as f64 * 50.0 / 1000.0) as i32;
            width as i32 - scroll_px
        } else {
            offset_x
        };

        // Get dimensions before mutable borrow
        let tw = target.width() as i32;
        let th = target.height() as i32;
        let data = target.data_mut();

        // Rasterize each glyph
        for glyph in &glyphs {
            if let Some(bb) = glyph.pixel_bounding_box() {
                glyph.draw(|gx, gy, v| {
                    let px = scroll_offset + bb.min.x + gx as i32;
                    let py = offset_y + bb.min.y + gy as i32;

                    if px >= 0 && px < tw && py >= 0 && py < th {
                        let alpha = (v * 255.0) as u8;
                        if alpha > 0 {
                            let idx = ((py * tw + px) * 4) as usize;
                            let a = alpha as f32 / 255.0;
                            let dst_a = data[idx + 3] as f32 / 255.0;
                            let out_a = a + dst_a * (1.0 - a);
                            if out_a > 0.0 {
                                data[idx] = ((r as f32 * a
                                    + data[idx] as f32 * dst_a * (1.0 - a))
                                    / out_a) as u8;
                                data[idx + 1] = ((g as f32 * a
                                    + data[idx + 1] as f32 * dst_a * (1.0 - a))
                                    / out_a) as u8;
                                data[idx + 2] = ((b as f32 * a
                                    + data[idx + 2] as f32 * dst_a * (1.0 - a))
                                    / out_a) as u8;
                                data[idx + 3] = (out_a * 255.0) as u8;
                            }
                        }
                    }
                });
            }
        }

        debug!(
            "Rendered text '{}' ({}x{}) at offset ({}, {})",
            content, text_width, line_height, scroll_offset, offset_y
        );
    }
}

impl ContentRenderer for TextRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        width: u32,
        height: u32,
        elapsed_ms: u64,
        _program_dir: &Path,
    ) -> bool {
        let text_content = match item {
            ContentItem::Text(t) => t,
            _ => return false,
        };

        self.render_text_content(text_content, target, width, height, elapsed_ms);
        true
    }
}
