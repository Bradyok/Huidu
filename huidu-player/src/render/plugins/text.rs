/// Text content renderer plugin.
///
/// Renders static and scrolling text with per-character colour effects.
///
/// ## Glyph pixel cache
/// Font layout (glyph shaping) and rasterization (coverage sampling) are
/// expensive and produce identical results for the same text at the same size.
/// The renderer caches the rasterized pixel positions the first time a text
/// element is drawn and replays them on every subsequent frame, only computing
/// per-pixel colour (which can be animated) during replay.
///
/// Cache key: (guid, area_width, area_height, font_size_bits).
/// The cache is never evicted — text elements on LED signs don't change at
/// runtime, so unbounded growth is not a concern in practice.
use std::collections::HashMap;
use std::path::Path;
use tiny_skia::Pixmap;

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

/// Compute the per-glyph colour.
/// `glyph_idx` — index of this glyph in the line (0-based).
/// `total`     — total number of glyphs in the line.
/// `elapsed_ms`— wall-clock time since program start.
#[inline]
fn glyph_color(mode: ColorMode, glyph_idx: usize, total: usize, elapsed_ms: u64) -> (u8, u8, u8) {
    match mode {
        ColorMode::Solid(r, g, b) => (r, g, b),

        ColorMode::Rainbow => {
            let pos_t = glyph_idx as f32 / total.max(1) as f32;
            let time_t = (elapsed_ms as f32 / 3000.0).fract();
            hsv_to_rgb(((pos_t + time_t).fract()) * 360.0, 1.0, 1.0)
        }

        ColorMode::RainbowReverse => {
            let pos_t = 1.0 - glyph_idx as f32 / total.max(1) as f32;
            let time_t = (elapsed_ms as f32 / 3000.0).fract();
            hsv_to_rgb(((pos_t + time_t).fract()) * 360.0, 1.0, 1.0)
        }

        ColorMode::Gradient(r1, g1, b1, r2, g2, b2) => {
            let t = if total <= 1 { 0.0f32 } else { glyph_idx as f32 / (total - 1) as f32 };
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }

        ColorMode::Wave(r1, g1, b1, r2, g2, b2) => {
            let phase = glyph_idx as f32 * 0.5;
            let t = ((elapsed_ms as f32 / 1000.0 + phase).sin() + 1.0) * 0.5;
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }

        ColorMode::Flash(r1, g1, b1, r2, g2, b2) => {
            if (elapsed_ms / 500) % 2 == 0 { (r1, g1, b1) } else { (r2, g2, b2) }
        }

        ColorMode::Random => {
            let hue = (glyph_idx as f32 * 137.508 + elapsed_ms as f32 / 40.0) % 360.0;
            hsv_to_rgb(hue, 1.0, 1.0)
        }

        ColorMode::Chase(r, g, b) => {
            let cycle = (elapsed_ms as f32 / 2000.0).fract();
            let highlight_pos = cycle * total.max(1) as f32;
            let dist = (glyph_idx as f32 - highlight_pos).abs();
            let factor = (-dist * dist / 4.0).exp() * 0.85 + 0.15;
            (
                (r as f32 * factor) as u8,
                (g as f32 * factor) as u8,
                (b as f32 * factor) as u8,
            )
        }

        ColorMode::Strobe(r, g, b) => {
            const PRIMES: [u64; 16] =
                [2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53];
            let prime = PRIMES[glyph_idx % PRIMES.len()];
            if ((elapsed_ms / 100).wrapping_add(prime)) % 2 == 0 {
                (r, g, b)
            } else {
                (0, 0, 0)
            }
        }
    }
}

/// HSV → RGB (H: 0-360, S/V: 0-1).
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

// ── Glyph pixel cache ─────────────────────────────────────────────────────────

/// One rasterized coverage pixel from a font glyph.
/// Coordinates are relative to the line's layout origin.
/// Stored compactly to keep cache memory low.
#[derive(Clone, Copy)]
struct CachedPixel {
    /// X offset from line start (signed; font metrics can be negative).
    x: i16,
    /// Y offset from line ascent baseline.
    y: i16,
    /// Anti-aliasing coverage (only non-zero values stored).
    alpha: u8,
    /// Which glyph (character) this pixel belongs to, capped at 255.
    glyph_idx: u8,
}

/// Cached rasterized layout for one text line.
struct RasterLine {
    pixels: Vec<CachedPixel>,
    /// Width of this line in screen pixels (for horizontal alignment).
    line_width: i32,
    /// Number of glyphs in this line (used for colour range calculations).
    total_glyphs: usize,
}

/// Full cached layout for one text item at one specific area size.
struct TextLayoutCache {
    lines: Vec<RasterLine>,
    line_height: i32,
    /// Y coordinate of the first line's origin (accounts for valign).
    block_y: i32,
}

/// Italic shear factor: tan(14°) ≈ 0.249 — shifts a pixel upward by this
/// many horizontal pixels per pixel of distance above the baseline.
const ITALIC_SHEAR: f32 = 0.249;

/// Build and return a `TextLayoutCache` for `text` at (`width` × `height`).
/// Applies fake-bold (duplicate pixels shifted +1 x), fake-italic (x shear),
/// and underline (pixel row at descent + 1).
/// This is the expensive path — called once per unique key.
fn build_layout_cache(
    font: &rusttype::Font<'static>,
    text: &TextContent,
    _width: u32,
    height: u32,
) -> TextLayoutCache {
    let content = text.string.as_deref().unwrap_or("");
    let font_spec = text.font.as_ref();
    let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
    let bold      = font_spec.map(|f| f.bold).unwrap_or(false);
    let italic    = font_spec.map(|f| f.italic).unwrap_or(false);
    let underline = font_spec.map(|f| f.underline).unwrap_or(false);

    let scale = rusttype::Scale::uniform(font_size);
    let v_metrics = font.v_metrics(scale);
    let ascent = v_metrics.ascent;
    let line_height =
        (ascent - v_metrics.descent + v_metrics.line_gap).ceil() as i32;

    let style = text.style.as_ref();
    let valign = style.map(|s| s.valign.as_str()).unwrap_or("middle");

    let lines_text: Vec<&str> = if text.single_line {
        vec![content]
    } else {
        content.split('\n').collect()
    };

    let num_lines = lines_text.len() as i32;
    let total_block_h = line_height * num_lines;
    let block_y = match valign {
        "top"    => 0,
        "bottom" => (height as i32 - total_block_h).max(0),
        _        => ((height as i32 - total_block_h) / 2).max(0),
    };

    let mut raster_lines = Vec::with_capacity(lines_text.len());

    for line_text in &lines_text {
        let glyphs: Vec<_> = font
            .layout(line_text, scale, rusttype::point(0.0, ascent))
            .collect();

        let base_line_width = glyphs
            .last()
            .map(|g| {
                if let Some(bb) = g.pixel_bounding_box() {
                    bb.max.x
                } else {
                    g.position().x as i32 + (font_size * 0.6) as i32
                }
            })
            .unwrap_or(0);

        // Extra width added by bold (+1) and italic shear
        let italic_extra = if italic { (ascent * ITALIC_SHEAR) as i32 } else { 0 };
        let bold_extra    = if bold   { 1 } else { 0 };
        let line_width    = base_line_width + italic_extra + bold_extra;

        let total_glyphs = glyphs.len();
        let mut pixels: Vec<CachedPixel> = Vec::new();

        for (glyph_idx, glyph) in glyphs.iter().enumerate() {
            if let Some(bb) = glyph.pixel_bounding_box() {
                let gi = (glyph_idx as u8).min(u8::MAX);
                glyph.draw(|gx, gy, v| {
                    let alpha = (v * 255.0) as u8;
                    if alpha == 0 {
                        return;
                    }
                    let mut rx = bb.min.x + gx as i32;
                    let ry     = bb.min.y + gy as i32;

                    // Italic: shear x toward top of glyph
                    if italic {
                        let y_above_baseline = ascent - ry as f32;
                        rx += (y_above_baseline * ITALIC_SHEAR).round() as i32;
                    }

                    let clamp = |v: i32| v.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                    pixels.push(CachedPixel { x: clamp(rx), y: clamp(ry), alpha, glyph_idx: gi });

                    // Bold: duplicate pixel one step to the right
                    if bold {
                        pixels.push(CachedPixel { x: clamp(rx + 1), y: clamp(ry), alpha, glyph_idx: gi });
                    }
                });
            }
        }

        // Underline: solid pixel row at descent + 1
        if underline && base_line_width > 0 {
            let uy = (v_metrics.descent.ceil() as i32 + 1)
                .clamp(i16::MIN as i32, i16::MAX as i32) as i16;
            for ux in 0..line_width {
                let ux16 = ux.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                pixels.push(CachedPixel { x: ux16, y: uy, alpha: 255, glyph_idx: 0 });
            }
        }

        raster_lines.push(RasterLine { pixels, line_width, total_glyphs });
    }

    TextLayoutCache { lines: raster_lines, line_height, block_y }
}

// ── Renderer ─────────────────────────────────────────────────────────────────

pub struct TextRenderer {
    font: rusttype::Font<'static>,
    /// Key: (guid, area_width, area_height, font_size_bits, style_flags)
    /// style_flags: bit 0 = bold, bit 1 = italic, bit 2 = underline
    cache: HashMap<(String, u32, u32, u32, u8), TextLayoutCache>,
}

impl TextRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self { font, cache: HashMap::new() }
    }

    fn render_text_content(
        &mut self,
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

        // ── Colour mode ──────────────────────────────────────────────────────
        let font_spec = text.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let color_mode =
            parse_color_mode(font_spec.map(|f| f.color.as_str()).unwrap_or("#ff0000"));

        // ── Layout cache (build once, replay every frame) ────────────────────
        let style_flags = {
            let b = font_spec.map(|f| f.bold).unwrap_or(false) as u8;
            let i = font_spec.map(|f| f.italic).unwrap_or(false) as u8;
            let u = font_spec.map(|f| f.underline).unwrap_or(false) as u8;
            b | (i << 1) | (u << 2)
        };
        let cache_key = (text.guid.clone(), width, height, font_size.to_bits(), style_flags);
        if !self.cache.contains_key(&cache_key) {
            let entry = build_layout_cache(&self.font, text, width, height);
            self.cache.insert(cache_key.clone(), entry);
        }
        let layout = &self.cache[&cache_key];

        // ── Blit cached pixels with per-frame colour ─────────────────────────
        let style = text.style.as_ref();
        let align = style.map(|s| s.align.as_str()).unwrap_or("center");

        let tw = target.width() as i32;
        let th = target.height() as i32;
        let data = target.data_mut();

        for (line_idx, rline) in layout.lines.iter().enumerate() {
            let base_x = match align {
                "left"  => 0,
                "right" => (width as i32 - rline.line_width).max(0),
                _       => ((width as i32 - rline.line_width) / 2).max(0),
            };

            // Scrolling for single-line overflow (50 px/s).
            let x_offset = if text.single_line && rline.line_width > width as i32 {
                let total_scroll = rline.line_width + width as i32;
                let period_ms = (total_scroll as u64 * 1000) / 50;
                let scroll_px =
                    ((elapsed_ms % period_ms) as f64 * 50.0 / 1000.0) as i32;
                width as i32 - scroll_px
            } else {
                base_x
            };

            let y_offset = layout.block_y + line_idx as i32 * layout.line_height;

            // Replay cached pixels — only the colour computation runs per frame.
            for &px in &rline.pixels {
                let (r, g, b) = glyph_color(
                    color_mode,
                    px.glyph_idx as usize,
                    rline.total_glyphs,
                    elapsed_ms,
                );

                let screen_x = x_offset + px.x as i32;
                let screen_y = y_offset + px.y as i32;
                if screen_x < 0 || screen_x >= tw || screen_y < 0 || screen_y >= th {
                    continue;
                }

                let idx = ((screen_y * tw + screen_x) * 4) as usize;
                let a = px.alpha as f32 / 255.0;
                let dst_a = data[idx + 3] as f32 / 255.0;
                let out_a = a + dst_a * (1.0 - a);
                if out_a > 0.0 {
                    data[idx]     = ((r as f32 * a + data[idx]     as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                    data[idx + 1] = ((g as f32 * a + data[idx + 1] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                    data[idx + 2] = ((b as f32 * a + data[idx + 2] as f32 * dst_a * (1.0 - a)) / out_a) as u8;
                    data[idx + 3] = (out_a * 255.0) as u8;
                }
            }
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
