/// Text content renderer plugin.
/// Renders static and scrolling text using rusttype for font rasterization.
/// Supports multi-line text (newline-separated), horizontal scrolling for
/// single-line overflow, optional background fill, and animated per-character
/// colour effects (rainbow, gradient, flash).
use std::path::Path;
use tiny_skia::Pixmap;
use tracing::debug;

use crate::program::model::{parse_color, ContentItem, TextContent};
use crate::render::plugins::ContentRenderer;

// ── Colour mode ──────────────────────────────────────────────────────────────

/// Per-character colour mode decoded from the FontSpec `color` string.
#[derive(Clone, Copy)]
enum ColorMode {
    /// Solid single colour.
    Solid(u8, u8, u8),
    /// Animated HSV rainbow left-to-right — hue shifts with glyph index and time.
    Rainbow,
    /// Animated HSV rainbow right-to-left.
    RainbowReverse,
    /// Static linear gradient from one colour to another.
    Gradient(u8, u8, u8, u8, u8, u8),
    /// Sine-wave oscillation between two colours per character.
    Wave(u8, u8, u8, u8, u8, u8),
    /// Two colours alternating at ~2 Hz.
    Flash(u8, u8, u8, u8, u8, u8),
    /// Each glyph gets a distinct hue via the golden-angle HSV distribution.
    Random,
    /// Bright highlight chases left-to-right across the text (~1 cycle/2 s).
    Chase(u8, u8, u8),
    /// Each character flickers independently at ~5 Hz with a prime phase offset.
    Strobe(u8, u8, u8),
}

/// Decode the colour string into a `ColorMode`.
/// Special values:
///   `"rainbow"`                       → animated HSV rainbow (L→R)
///   `"rainbow-r"`                     → animated HSV rainbow (R→L)
///   `"gradient:#RRGGBB:#RRGGBB"`      → linear gradient
///   `"wave:#RRGGBB:#RRGGBB"`          → sine wave between two colours
///   `"flash:#RRGGBB:#RRGGBB"`         → flashing alternation
///   `"random"`                        → golden-angle HSV per character
///   `"chase:#RRGGBB"`                 → sweeping highlight in given colour
///   `"strobe:#RRGGBB"`                → per-char flicker in given colour
///   anything else                     → solid hex colour
fn parse_color_mode(s: &str) -> ColorMode {
    let s = s.trim();
    if s.eq_ignore_ascii_case("rainbow") {
        return ColorMode::Rainbow;
    }
    if s.eq_ignore_ascii_case("rainbow-r") {
        return ColorMode::RainbowReverse;
    }
    if s.eq_ignore_ascii_case("random") {
        return ColorMode::Random;
    }
    if let Some(rest) = s.strip_prefix("gradient:").or_else(|| s.strip_prefix("Gradient:")) {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            let (r1, g1, b1) = parse_color(parts[0]);
            let (r2, g2, b2) = parse_color(parts[1]);
            return ColorMode::Gradient(r1, g1, b1, r2, g2, b2);
        }
    }
    if let Some(rest) = s.strip_prefix("wave:").or_else(|| s.strip_prefix("Wave:")) {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            let (r1, g1, b1) = parse_color(parts[0]);
            let (r2, g2, b2) = parse_color(parts[1]);
            return ColorMode::Wave(r1, g1, b1, r2, g2, b2);
        }
    }
    if let Some(rest) = s.strip_prefix("flash:").or_else(|| s.strip_prefix("Flash:")) {
        let parts: Vec<&str> = rest.splitn(2, ':').collect();
        if parts.len() == 2 {
            let (r1, g1, b1) = parse_color(parts[0]);
            let (r2, g2, b2) = parse_color(parts[1]);
            return ColorMode::Flash(r1, g1, b1, r2, g2, b2);
        }
    }
    if let Some(rest) = s.strip_prefix("chase:").or_else(|| s.strip_prefix("Chase:")) {
        let (r, g, b) = parse_color(rest);
        return ColorMode::Chase(r, g, b);
    }
    if let Some(rest) = s.strip_prefix("strobe:").or_else(|| s.strip_prefix("Strobe:")) {
        let (r, g, b) = parse_color(rest);
        return ColorMode::Strobe(r, g, b);
    }
    let (r, g, b) = parse_color(s);
    ColorMode::Solid(r, g, b)
}

/// Compute the per-glyph colour from the colour mode.
/// `glyph_idx` — index of this glyph in the line (0-based).
/// `total`     — total number of glyphs in the line.
/// `elapsed_ms`— wall-clock time since program start (for animation).
#[inline]
fn glyph_color(mode: ColorMode, glyph_idx: usize, total: usize, elapsed_ms: u64) -> (u8, u8, u8) {
    match mode {
        ColorMode::Solid(r, g, b) => (r, g, b),

        ColorMode::Rainbow => {
            // Hue advances with glyph position and cycles with time.
            // Full rainbow spans the whole text; completes one hue cycle per 3 s.
            let pos_t = glyph_idx as f32 / total.max(1) as f32;
            let time_t = (elapsed_ms as f32 / 3000.0).fract();
            let hue = ((pos_t + time_t).fract()) * 360.0;
            hsv_to_rgb(hue, 1.0, 1.0)
        }

        ColorMode::RainbowReverse => {
            // Same as Rainbow but position runs right-to-left.
            let pos_t = 1.0 - glyph_idx as f32 / total.max(1) as f32;
            let time_t = (elapsed_ms as f32 / 3000.0).fract();
            let hue = ((pos_t + time_t).fract()) * 360.0;
            hsv_to_rgb(hue, 1.0, 1.0)
        }

        ColorMode::Gradient(r1, g1, b1, r2, g2, b2) => {
            let t = if total <= 1 {
                0.0f32
            } else {
                glyph_idx as f32 / (total - 1) as f32
            };
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }

        ColorMode::Wave(r1, g1, b1, r2, g2, b2) => {
            // Each glyph's colour oscillates on a sine wave between the two
            // colours. Phase is offset per character so the wave travels.
            let phase = glyph_idx as f32 * 0.5;
            let t_raw = (elapsed_ms as f32 / 1000.0 + phase).sin();
            let t = (t_raw + 1.0) * 0.5; // remap [-1,1] → [0,1]
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }

        ColorMode::Flash(r1, g1, b1, r2, g2, b2) => {
            // Toggle every 500 ms (2 Hz).
            if (elapsed_ms / 500) % 2 == 0 {
                (r1, g1, b1)
            } else {
                (r2, g2, b2)
            }
        }

        ColorMode::Random => {
            // Assign each glyph a distinct, stable hue using the golden angle
            // (137.508°). The whole palette rotates slowly with time.
            let hue = (glyph_idx as f32 * 137.508 + elapsed_ms as f32 / 40.0) % 360.0;
            hsv_to_rgb(hue, 1.0, 1.0)
        }

        ColorMode::Chase(r, g, b) => {
            // A bright highlight travels from left to right across the text,
            // completing one pass every 2 s. Characters away from the highlight
            // show at ~15% brightness; the peak glyph is full brightness.
            let cycle = (elapsed_ms as f32 / 2000.0).fract();
            let highlight_pos = cycle * total.max(1) as f32;
            let dist = (glyph_idx as f32 - highlight_pos).abs();
            // Gaussian-like falloff over ±3 characters.
            let factor = (-dist * dist / 4.0).exp() * 0.85 + 0.15;
            (
                (r as f32 * factor) as u8,
                (g as f32 * factor) as u8,
                (b as f32 * factor) as u8,
            )
        }

        ColorMode::Strobe(r, g, b) => {
            // Each character flickers at ~5 Hz using a prime-number phase offset
            // so no two adjacent characters are in sync.
            const PRIMES: [u64; 16] = [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];
            let prime = PRIMES[glyph_idx % PRIMES.len()];
            let on = ((elapsed_ms / 100).wrapping_add(prime)) % 2 == 0;
            if on {
                (r, g, b)
            } else {
                (0, 0, 0)
            }
        }
    }
}

/// HSV → RGB conversion (H: 0-360, S/V: 0-1).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h % 360.0;
    let i = (h / 60.0) as u32;
    let f = h / 60.0 - i as f32;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * f);
    let t = v * (1.0 - s * (1.0 - f));
    let (r, g, b) = match i {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    ((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

// ── Renderer ─────────────────────────────────────────────────────────────────

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

        // ── Background fill ──────────────────────────────────────────────────
        if !text.background.is_empty() && text.background != "transparent" {
            let (br, bg_c, bb) = parse_color(&text.background);
            let data = target.data_mut();
            for chunk in data.chunks_exact_mut(4) {
                chunk[0] = br;
                chunk[1] = bg_c;
                chunk[2] = bb;
                chunk[3] = 255;
            }
        }

        // ── Font metrics ─────────────────────────────────────────────────────
        let font_spec = text.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let color_mode = parse_color_mode(
            font_spec.map(|f| f.color.as_str()).unwrap_or("#ff0000"),
        );

        let scale = rusttype::Scale::uniform(font_size);
        let v_metrics = self.font.v_metrics(scale);
        let line_height =
            (v_metrics.ascent - v_metrics.descent + v_metrics.line_gap).ceil() as i32;

        // ── Layout ───────────────────────────────────────────────────────────
        let style = text.style.as_ref();
        let align  = style.map(|s| s.align.as_str()).unwrap_or("center");
        let valign = style.map(|s| s.valign.as_str()).unwrap_or("middle");

        let lines: Vec<&str> = if text.single_line {
            vec![content]
        } else {
            content.split('\n').collect()
        };

        let num_lines = lines.len() as i32;
        let total_block_h = line_height * num_lines;

        let block_y = match valign {
            "top"    => 0,
            "bottom" => (height as i32 - total_block_h).max(0),
            _        => ((height as i32 - total_block_h) / 2).max(0),
        };

        let tw = target.width() as i32;
        let th = target.height() as i32;

        for (line_idx, line) in lines.iter().enumerate() {
            let glyphs: Vec<_> = self
                .font
                .layout(line, scale, rusttype::point(0.0, v_metrics.ascent))
                .collect();

            if glyphs.is_empty() {
                continue;
            }

            let line_w = glyphs
                .last()
                .map(|g| {
                    if let Some(bb) = g.pixel_bounding_box() {
                        bb.max.x
                    } else {
                        g.position().x as i32 + (font_size * 0.6) as i32
                    }
                })
                .unwrap_or(0);

            let base_x = match align {
                "left"  => 0,
                "right" => (width as i32 - line_w).max(0),
                _       => ((width as i32 - line_w) / 2).max(0),
            };

            // Scrolling for single-line overflow (50 px/s)
            let x_offset = if text.single_line && line_w > width as i32 {
                let total_scroll = line_w + width as i32;
                let period_ms = (total_scroll as u64 * 1000) / 50;
                let scroll_px = ((elapsed_ms % period_ms) as f64 * 50.0 / 1000.0) as i32;
                width as i32 - scroll_px
            } else {
                base_x
            };

            let y_offset = block_y + line_idx as i32 * line_height;
            let total_glyphs = glyphs.len();

            // ── Rasterise ────────────────────────────────────────────────────
            let data = target.data_mut();
            for (glyph_idx, glyph) in glyphs.iter().enumerate() {
                let (r, g, b) = glyph_color(color_mode, glyph_idx, total_glyphs, elapsed_ms);

                if let Some(bb) = glyph.pixel_bounding_box() {
                    glyph.draw(|gx, gy, v| {
                        let px = x_offset + bb.min.x + gx as i32;
                        let py = y_offset + bb.min.y + gy as i32;
                        if px < 0 || px >= tw || py < 0 || py >= th {
                            return;
                        }
                        let alpha = (v * 255.0) as u8;
                        if alpha == 0 {
                            return;
                        }
                        let idx = ((py * tw + px) * 4) as usize;
                        let a = alpha as f32 / 255.0;
                        let dst_a = data[idx + 3] as f32 / 255.0;
                        let out_a = a + dst_a * (1.0 - a);
                        if out_a > 0.0 {
                            data[idx]     = ((r as f32 * a + data[idx]     as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                            data[idx + 1] = ((g as f32 * a + data[idx + 1] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                            data[idx + 2] = ((b as f32 * a + data[idx + 2] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                            data[idx + 3] = (out_a * 255.0) as u8;
                        }
                    });
                }
            }

            debug!(
                "Text line {}: '{}' w={} at ({}, {})",
                line_idx, line, line_w, x_offset, y_offset
            );
        }
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
