/// Clock content renderer plugin.
/// Renders digital clock with date/time/week fields.
use chrono::Local;
use std::path::Path;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ClockContent, ContentItem};
use crate::render::plugins::ContentRenderer;

pub struct ClockRenderer {
    font: rusttype::Font<'static>,
}

impl ClockRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self { font }
    }

    fn render_clock(
        &self,
        clock: &ClockContent,
        target: &mut Pixmap,
        width: u32,
        height: u32,
    ) {
        let now = Local::now();

        // Collect lines to render with their colors
        let mut lines: Vec<(String, (u8, u8, u8))> = Vec::new();

        // Date line
        if let Some(ref date_field) = clock.date {
            if date_field.display {
                let date_str = match date_field.format.as_str() {
                    "2" => now.format("%m/%d/%Y").to_string(),
                    "3" => now.format("%d/%m/%Y").to_string(),
                    "4" => now.format("%b %d, %Y").to_string(),
                    "5" => now.format("%d %b, %Y").to_string(),
                    _ => now.format("%Y/%m/%d").to_string(),
                };
                lines.push((date_str, parse_color(&date_field.color)));
            }
        }

        // Week line
        if let Some(ref week_field) = clock.week {
            if week_field.display {
                let week_str = match week_field.format.as_str() {
                    "2" => now.format("%A").to_string(),
                    "3" => now.format("%a").to_string(),
                    _ => now.format("%A").to_string(),
                };
                lines.push((week_str, parse_color(&week_field.color)));
            }
        }

        // Time line
        if let Some(ref time_field) = clock.time {
            if time_field.display {
                let time_str = match time_field.format.as_str() {
                    "2" => now.format("%H:%M").to_string(),
                    "3" => now.format("%I:%M:%S %p").to_string(),
                    "4" => now.format("%I:%M %p").to_string(),
                    _ => now.format("%H:%M:%S").to_string(),
                };
                lines.push((time_str, parse_color(&time_field.color)));
            }
        }

        if lines.is_empty() {
            lines.push((
                now.format("%H:%M:%S").to_string(),
                (255, 255, 255),
            ));
        }

        // Calculate layout
        let font_size = (height as f32 / lines.len() as f32).min(height as f32 * 0.8);
        let scale = rusttype::Scale::uniform(font_size);
        let v_metrics = self.font.v_metrics(scale);
        let line_height = (v_metrics.ascent - v_metrics.descent + v_metrics.line_gap).ceil();

        let total_height = line_height * lines.len() as f32;
        let start_y = ((height as f32 - total_height) / 2.0).max(0.0);

        // Get dimensions before mutable borrow
        let tw = target.width() as i32;
        let th = target.height() as i32;
        let data = target.data_mut();

        for (i, (text, (r, g, b))) in lines.iter().enumerate() {
            let y_offset = start_y + i as f32 * line_height;

            let glyphs: Vec<_> = self
                .font
                .layout(
                    text,
                    scale,
                    rusttype::point(0.0, y_offset + v_metrics.ascent),
                )
                .collect();

            // Center horizontally
            let text_width = glyphs
                .last()
                .and_then(|g| g.pixel_bounding_box().map(|bb| bb.max.x))
                .unwrap_or(0);
            let x_offset = ((width as i32 - text_width) / 2).max(0);

            for glyph in &glyphs {
                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph.draw(|gx, gy, v| {
                        let px = x_offset + bb.min.x + gx as i32;
                        let py = bb.min.y + gy as i32;

                        if px >= 0 && px < tw && py >= 0 && py < th {
                            let alpha = (v * 255.0) as u8;
                            if alpha > 0 {
                                let idx = ((py * tw + px) * 4) as usize;
                                let a = alpha as f32 / 255.0;
                                let dst_a = data[idx + 3] as f32 / 255.0;
                                let out_a = a + dst_a * (1.0 - a);
                                if out_a > 0.0 {
                                    data[idx] = ((*r as f32 * a
                                        + data[idx] as f32 * dst_a * (1.0 - a))
                                        / out_a)
                                        as u8;
                                    data[idx + 1] = ((*g as f32 * a
                                        + data[idx + 1] as f32 * dst_a * (1.0 - a))
                                        / out_a)
                                        as u8;
                                    data[idx + 2] = ((*b as f32 * a
                                        + data[idx + 2] as f32 * dst_a * (1.0 - a))
                                        / out_a)
                                        as u8;
                                    data[idx + 3] = (out_a * 255.0) as u8;
                                }
                            }
                        }
                    });
                }
            }
        }
    }
}

impl ContentRenderer for ClockRenderer {
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
        let clock = match item {
            ContentItem::Clock(c) => c,
            _ => return false,
        };

        self.render_clock(clock, target, width, height);
        true
    }
}
