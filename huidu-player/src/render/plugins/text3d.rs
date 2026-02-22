/// 3D animated text renderer using layered shadow offset technique.
use std::path::Path;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ContentItem, Text3DContent};
use crate::render::plugins::ContentRenderer;
use crate::render::plugins::net_text::load_font;

pub struct Text3DRenderer {
    font: rusttype::Font<'static>,
}

impl Text3DRenderer {
    pub fn new() -> Self {
        Self { font: load_font() }
    }
}

impl ContentRenderer for Text3DRenderer {
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
            ContentItem::Text3D(c) => c,
            _ => return false,
        };

        render_text3d(&self.font, content, target, elapsed_ms);
        true
    }
}

fn render_text3d(font: &rusttype::Font<'_>, content: &Text3DContent, target: &mut Pixmap, elapsed_ms: u64) {
    let tw = target.width() as i32;
    let th = target.height() as i32;
    if tw <= 0 || th <= 0 || content.text.is_empty() { return; }

    let scale = rusttype::Scale::uniform(content.font_size);
    let vm = font.v_metrics(scale);
    let text_h = (vm.ascent - vm.descent).ceil() as i32;
    let text_w = measure_text(font, &content.text, scale);
    let cx = tw / 2;
    let cy = th / 2;

    // Animate angle based on elapsed time and rotate speed
    let t = (elapsed_ms as f32 / 1000.0) * content.rotate_speed * std::f32::consts::TAU;
    let cos_t = t.cos();

    let (fr, fg, fb) = parse_color(&content.color);
    let (dr, dg, db) = parse_color(&content.depth_color);

    let effect = content.effect_3d.as_str();

    match effect {
        "rotate_y" => {
            // Perspective squeeze: x-coordinates are scaled by cos(t)
            // Draw 6 depth layers first
            let depth_layers = 6i32;
            for i in (1..=depth_layers).rev() {
                let angle = t + (i as f32) * 0.05;
                let cos_a = angle.cos();
                let layer_alpha = ((depth_layers - i + 1) as f32 / depth_layers as f32 * 200.0) as u8;
                let x_squeeze = cos_a;
                if x_squeeze.abs() < 0.05 { continue; }
                let x_off = cx - ((text_w as f32 * x_squeeze.abs()) / 2.0) as i32;
                let y_off = cy - text_h / 2 + i;
                blit_text_squeezed(font, &content.text, scale, x_off, y_off, tw, th,
                    dr, dg, db, layer_alpha, x_squeeze, cx, target.data_mut());
            }
            // Draw main text
            if cos_t.abs() > 0.05 {
                let x_off = cx - ((text_w as f32 * cos_t.abs()) / 2.0) as i32;
                let y_off = cy - text_h / 2;
                blit_text_squeezed(font, &content.text, scale, x_off, y_off, tw, th,
                    fr, fg, fb, 255, cos_t, cx, target.data_mut());
            }
        }
        "pulse" => {
            let pulse_scale = 1.0 + 0.2 * t.sin();
            let ps = rusttype::Scale::uniform(content.font_size * pulse_scale);
            let pw = measure_text(font, &content.text, ps);
            let x_off = cx - pw / 2;
            let y_off = cy - (content.font_size * pulse_scale / 2.0) as i32;
            // Depth layers
            for i in (1..=4i32).rev() {
                let layer_alpha = 100u8;
                blit_text_at(font, &content.text, ps, x_off + i, y_off + i, tw, th,
                    dr, dg, db, layer_alpha, target.data_mut());
            }
            blit_text_at(font, &content.text, ps, x_off, y_off, tw, th, fr, fg, fb, 255, target.data_mut());
        }
        "wave" => {
            // Each character is offset vertically in a wave
            let chars: Vec<char> = content.text.chars().collect();
            let char_w = if chars.is_empty() { 8 } else { (text_w / chars.len().max(1) as i32).max(1) };
            let start_x = cx - text_w / 2;
            for (i, ch) in chars.iter().enumerate() {
                let wave_y = (4.0 * (t + i as f32 * 0.5).sin()) as i32;
                let x_off = start_x + i as i32 * char_w;
                let y_off = cy - text_h / 2 + wave_y;
                let cs = ch.to_string();
                blit_text_at(font, &cs, scale, x_off + 1, y_off + 2, tw, th, dr, dg, db, 180, target.data_mut());
                blit_text_at(font, &cs, scale, x_off, y_off, tw, th, fr, fg, fb, 255, target.data_mut());
            }
        }
        _ => {
            // rotate_x: vertical squeeze
            let y_squeeze = cos_t;
            let new_h = (text_h as f32 * y_squeeze.abs()).max(1.0) as i32;
            let y_off = cy - new_h / 2;
            let x_off = cx - text_w / 2;
            // Depth layers
            for i in (1..=4i32).rev() {
                blit_text_at(font, &content.text, scale, x_off + i, y_off + i, tw, th,
                    dr, dg, db, 120, target.data_mut());
            }
            blit_text_at(font, &content.text, scale, x_off, y_off, tw, th, fr, fg, fb, 255, target.data_mut());
        }
    }
}

fn measure_text(font: &rusttype::Font<'_>, text: &str, scale: rusttype::Scale) -> i32 {
    font.layout(text, scale, rusttype::point(0.0, 0.0))
        .filter_map(|g| g.pixel_bounding_box())
        .map(|bb| bb.max.x)
        .last()
        .unwrap_or(0)
}

fn blit_text_at(
    font: &rusttype::Font<'_>,
    text: &str,
    scale: rusttype::Scale,
    x_off: i32,
    y_off: i32,
    tw: i32,
    th: i32,
    r: u8,
    g: u8,
    b: u8,
    alpha_mul: u8,
    data: &mut [u8],
) {
    let v = font.v_metrics(scale);
    let glyphs: Vec<_> = font.layout(text, scale, rusttype::point(0.0, v.ascent)).collect();
    for glyph in &glyphs {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, cv| {
                let alpha = ((cv * 255.0) as u8 as u32 * alpha_mul as u32 / 255) as u8;
                if alpha == 0 { return; }
                let px = x_off + bb.min.x + gx as i32;
                let py = y_off + bb.min.y + gy as i32;
                if px < 0 || px >= tw || py < 0 || py >= th { return; }
                let idx = ((py * tw + px) * 4) as usize;
                let a = alpha as f32 / 255.0;
                let da = data[idx + 3] as f32 / 255.0;
                let oa = a + da * (1.0 - a);
                if oa > 0.0 {
                    data[idx]     = ((r as f32 * a + data[idx]     as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 1] = ((g as f32 * a + data[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 2] = ((b as f32 * a + data[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 3] = (oa * 255.0) as u8;
                }
            });
        }
    }
}

fn blit_text_squeezed(
    font: &rusttype::Font<'_>,
    text: &str,
    scale: rusttype::Scale,
    _x_off: i32,
    y_off: i32,
    tw: i32,
    th: i32,
    r: u8,
    g: u8,
    b: u8,
    alpha_mul: u8,
    x_squeeze: f32,
    cx: i32,
    data: &mut [u8],
) {
    let v = font.v_metrics(scale);
    let glyphs: Vec<_> = font.layout(text, scale, rusttype::point(0.0, v.ascent)).collect();
    // Find text width to center
    let text_w = glyphs.last().and_then(|g| g.pixel_bounding_box()).map(|bb| bb.max.x).unwrap_or(0);
    let raw_cx = text_w / 2;

    for glyph in &glyphs {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, cv| {
                let alpha = ((cv * 255.0) as u8 as u32 * alpha_mul as u32 / 255) as u8;
                if alpha == 0 { return; }
                let raw_x = bb.min.x + gx as i32;
                // Apply x squeeze around center of text
                let squeezed_x = ((raw_x - raw_cx) as f32 * x_squeeze.abs()) as i32 + cx;
                let py = y_off + bb.min.y + gy as i32;
                if squeezed_x < 0 || squeezed_x >= tw || py < 0 || py >= th { return; }
                let idx = ((py * tw + squeezed_x) * 4) as usize;
                let a = alpha as f32 / 255.0;
                let da = data[idx + 3] as f32 / 255.0;
                let oa = a + da * (1.0 - a);
                if oa > 0.0 {
                    data[idx]     = ((r as f32 * a + data[idx]     as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 1] = ((g as f32 * a + data[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 2] = ((b as f32 * a + data[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 3] = (oa * 255.0) as u8;
                }
            });
        }
    }
}
