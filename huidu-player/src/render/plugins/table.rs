/// Table/grid content renderer plugin (libtable_plugin.so equivalent).
/// Renders a configurable grid with text cells — used for scoreboards,
/// bus schedules, data displays, etc.
use std::path::Path;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ContentItem, TableContent};
use crate::render::plugins::ContentRenderer;

pub struct TableRenderer {
    font: rusttype::Font<'static>,
}

impl TableRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self { font }
    }

    fn render_table(&self, table: &TableContent, target: &mut Pixmap, width: u32, height: u32) {
        if table.cols == 0 || table.rows == 0 {
            return;
        }

        let style = table.style.as_ref();
        let font_size = style.map(|s| s.font_size).unwrap_or(10.0);
        let (tr, tg, tb) = style
            .map(|s| parse_color(&s.text_color))
            .unwrap_or((255, 255, 255));
        let border_color = style.map(|s| parse_color(&s.border_color)).unwrap_or((80, 80, 80));
        let header_bg = style
            .and_then(|s| if s.header_bg_color.is_empty() { None } else { Some(parse_color(&s.header_bg_color)) })
            .unwrap_or((40, 40, 120));

        let num_rows = table.rows as usize;
        let num_cols = table.cols as usize;
        let cell_w = width / table.cols;
        let cell_h = height / table.rows;

        let scale = rusttype::Scale::uniform(font_size);
        let v_metrics = self.font.v_metrics(scale);
        let ascent = v_metrics.ascent;
        let tw = target.width() as i32;
        let _th = target.height() as i32;

        // Draw background (cells) and borders
        let data = target.data_mut();

        // Fill background with dark color
        for chunk in data.chunks_exact_mut(4) {
            chunk[0] = 10;
            chunk[1] = 10;
            chunk[2] = 20;
            chunk[3] = 255;
        }

        // Draw header row background (row 0)
        if style.map(|s| s.header_row).unwrap_or(false) {
            for y in 0..cell_h as i32 {
                for x in 0..width as i32 {
                    let idx = ((y * tw + x) * 4) as usize;
                    if idx + 3 < data.len() {
                        data[idx] = header_bg.0;
                        data[idx + 1] = header_bg.1;
                        data[idx + 2] = header_bg.2;
                        data[idx + 3] = 255;
                    }
                }
            }
        }

        // Draw grid lines (1-pixel border in border_color)
        for row in 0..=num_rows {
            let y = (row as u32 * cell_h).min(height - 1) as i32;
            for x in 0..width as i32 {
                let idx = ((y * tw + x) * 4) as usize;
                if idx + 3 < data.len() {
                    data[idx] = border_color.0;
                    data[idx + 1] = border_color.1;
                    data[idx + 2] = border_color.2;
                    data[idx + 3] = 255;
                }
            }
        }
        for col in 0..=num_cols {
            let x = (col as u32 * cell_w).min(width - 1) as i32;
            for y in 0..height as i32 {
                let idx = ((y * tw + x) * 4) as usize;
                if idx + 3 < data.len() {
                    data[idx] = border_color.0;
                    data[idx + 1] = border_color.1;
                    data[idx + 2] = border_color.2;
                    data[idx + 3] = 255;
                }
            }
        }

        let _ = data; // release borrow before rendering text

        // Render cell text
        for (row_idx, row) in table.data_rows.iter().enumerate().take(num_rows) {
            let cell_top = row_idx as i32 * cell_h as i32 + 1;

            for (col_idx, cell) in row.cells.iter().enumerate().take(num_cols) {
                let cell_left = col_idx as i32 * cell_w as i32 + 2;
                let available_w = cell_w as i32 - 4;

                if cell.text.is_empty() {
                    continue;
                }

                let (r, g, b) = if !cell.color.is_empty() {
                    parse_color(&cell.color)
                } else {
                    (tr, tg, tb)
                };

                let glyphs: Vec<_> = self
                    .font
                    .layout(&cell.text, scale, rusttype::point(0.0, ascent))
                    .collect();

                let text_w = glyphs
                    .last()
                    .and_then(|g| g.pixel_bounding_box().map(|bb| bb.max.x))
                    .unwrap_or(0);

                let x_off = cell_left + ((available_w - text_w) / 2).max(0);
                let y_off = cell_top + ((cell_h as i32 - font_size as i32) / 2).max(0);

                let tw2 = target.width() as i32;
                let th2 = target.height() as i32;
                let data2 = target.data_mut();

                for glyph in &glyphs {
                    if let Some(bb) = glyph.pixel_bounding_box() {
                        glyph.draw(|gx, gy, v| {
                            let px = x_off + bb.min.x + gx as i32;
                            let py = y_off + bb.min.y + gy as i32;
                            if px < 0 || px >= tw2 || py < 0 || py >= th2 { return; }
                            let alpha = (v * 255.0) as u8;
                            if alpha == 0 { return; }
                            let idx = ((py * tw2 + px) * 4) as usize;
                            let a = alpha as f32 / 255.0;
                            let dst_a = data2[idx + 3] as f32 / 255.0;
                            let out_a = a + dst_a * (1.0 - a);
                            if out_a > 0.0 {
                                data2[idx]     = ((r as f32 * a + data2[idx]     as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                                data2[idx + 1] = ((g as f32 * a + data2[idx + 1] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                                data2[idx + 2] = ((b as f32 * a + data2[idx + 2] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                                data2[idx + 3] = (out_a * 255.0) as u8;
                            }
                        });
                    }
                }
            }
        }
    }
}

impl ContentRenderer for TableRenderer {
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
        let table = match item {
            ContentItem::Table(t) => t,
            _ => return false,
        };
        self.render_table(table, target, width, height);
        true
    }
}
