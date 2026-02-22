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
/// Cache key: TextCacheKey { guid, width, height, font_size_bits, style_flags, shadow_dx, shadow_dy }.
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
    /// Animated HSV rainbow — hue determined by screen X position and time.
    /// As text scrolls the rainbow stays fixed on screen (HDPlayer-compatible).
    Rainbow,
    /// Same as Rainbow but hue increases right-to-left.
    RainbowReverse,
    /// Linear gradient from colour A to colour B across the screen X axis.
    Gradient(u8, u8, u8, u8, u8, u8),
    /// Sine-wave oscillation between two colours, phase tied to screen X position.
    Wave(u8, u8, u8, u8, u8, u8),
    /// Two colours alternating at ~2 Hz (all characters together).
    Flash(u8, u8, u8, u8, u8, u8),
    /// Each glyph gets a distinct hue via the golden-angle HSV distribution.
    Random,
    /// Bright highlight chases left-to-right across the text (~1 cycle/2 s).
    Chase(u8, u8, u8),
    /// Each character flickers independently at ~5 Hz with a prime phase offset.
    Strobe(u8, u8, u8),
    /// All characters pulse together in brightness (smooth ~2 s breathing cycle).
    Breathe(u8, u8, u8),
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
    if let Some(rest) = s.strip_prefix("breathe:").or_else(|| s.strip_prefix("Breathe:")) {
        let (r, g, b) = parse_color(rest);
        return ColorMode::Breathe(r, g, b);
    }
    let (r, g, b) = parse_color(s);
    ColorMode::Solid(r, g, b)
}

/// Compute the per-pixel colour.
///
/// `glyph_idx`  — character index in the line (used for character-based modes).
/// `total`      — total number of glyphs in the line.
/// `elapsed_ms` — wall-clock milliseconds since program start.
/// `screen_pos` — pixel's screen X position normalised to \[0, 1\].
///               Used by position-based modes (rainbow, gradient, wave) so
///               that the colour pattern stays **fixed on-screen** as scrolling
///               text moves through it — matching HDPlayer behaviour.
#[inline]
fn glyph_color(
    mode: ColorMode,
    glyph_idx: usize,
    total: usize,
    elapsed_ms: u64,
    screen_pos: f32,
) -> (u8, u8, u8) {
    match mode {
        ColorMode::Solid(r, g, b) => (r, g, b),

        ColorMode::Rainbow => {
            let time_t = (elapsed_ms as f32 / 3000.0).fract();
            hsv_to_rgb(((screen_pos + time_t).fract()) * 360.0, 1.0, 1.0)
        }

        ColorMode::RainbowReverse => {
            let time_t = (elapsed_ms as f32 / 3000.0).fract();
            let pos = 1.0 - screen_pos;
            hsv_to_rgb(((pos + time_t).fract()) * 360.0, 1.0, 1.0)
        }

        ColorMode::Gradient(r1, g1, b1, r2, g2, b2) => {
            let t = screen_pos.clamp(0.0, 1.0);
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }

        ColorMode::Wave(r1, g1, b1, r2, g2, b2) => {
            // Phase tied to screen X so the wave crests sweep left→right.
            let phase = screen_pos * std::f32::consts::TAU;
            let t = ((elapsed_ms as f32 / 1000.0 + phase).sin() + 1.0) * 0.5;
            let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
            (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
        }

        ColorMode::Flash(r1, g1, b1, r2, g2, b2) => {
            if (elapsed_ms / 500).is_multiple_of(2) { (r1, g1, b1) } else { (r2, g2, b2) }
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
            if ((elapsed_ms / 100).wrapping_add(prime)).is_multiple_of(2) {
                (r, g, b)
            } else {
                (0, 0, 0)
            }
        }

        ColorMode::Breathe(r, g, b) => {
            // All characters pulse together: min 10%, max 100%, ~2 s period.
            let t = (elapsed_ms as f32 * std::f32::consts::PI / 1000.0).sin();
            let v = 0.10 + 0.90 * (t + 1.0) * 0.5;
            ((r as f32 * v) as u8, (g as f32 * v) as u8, (b as f32 * v) as u8)
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

// ── Live text variable expansion ──────────────────────────────────────────────

/// Returns true if the string contains any `{TOKEN}` variable reference.
#[inline]
pub(crate) fn has_variables(s: &str) -> bool {
    s.contains('{')
}

/// Replace all `{TOKEN}` placeholders with the current local date/time values.
///
/// Supported tokens:
/// `{TIME}` `{DATE}` `{WEEK}` `{YEAR}` `{MONTH}` `{MONTH_S}` `{MONTH_NUM}`
/// `{DAY}` `{HOUR}` `{MIN}` `{SEC}` `{DATE_S}` `{WEEK_S}` `{TIME_S}`
pub(crate) fn expand_variables(s: &str) -> String {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    const WEEKDAY_NAMES: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
    const MONTH_NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun",
        "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let wd = now.weekday().num_days_from_monday() as usize;
    let mo = (now.month() as usize).saturating_sub(1);

    s.replace("{TIME}", &format!("{:02}:{:02}:{:02}", now.hour(), now.minute(), now.second()))
     .replace("{DATE}", &format!("{}-{:02}-{:02}", now.year(), now.month(), now.day()))
     .replace("{WEEK}", WEEKDAY_NAMES[wd.min(6)])
     .replace("{YEAR}", &now.year().to_string())
     .replace("{MONTH}", MONTH_NAMES[mo.min(11)])
     .replace("{MONTH_NUM}", &format!("{:02}", now.month()))
     .replace("{DAY}", &format!("{:02}", now.day()))
     .replace("{HOUR}", &format!("{:02}", now.hour()))
     .replace("{MIN}", &format!("{:02}", now.minute()))
     .replace("{SEC}", &format!("{:02}", now.second()))
     .replace("{DATE_S}", &format!("{}/{}", now.month(), now.day()))
     .replace("{WEEK_S}", WEEKDAY_NAMES[wd.min(6)])
     .replace("{TIME_S}", &format!("{:02}:{:02}", now.hour(), now.minute()))
     .replace("{MONTH_S}", MONTH_NAMES[mo.min(11)])
}

/// Replace `{DS:name}` placeholders with values from the given data sources map.
///
/// Unknown names are silently omitted (the placeholder is removed).
pub(crate) fn expand_ds_tokens(s: &str, sources: &HashMap<String, String>) -> String {
    if sources.is_empty() || !s.contains("{DS:") {
        return s.to_string();
    }
    let mut result = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(start) = rest.find("{DS:") {
        result.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        if let Some(end) = after.find('}') {
            let name = &after[..end];
            if let Some(val) = sources.get(name) {
                result.push_str(val);
            }
            rest = &after[end + 1..];
        } else {
            // No closing brace — emit the rest as-is and stop
            result.push_str(&rest[start..]);
            return result;
        }
    }
    result.push_str(rest);
    result
}

// ── Word-wrap helpers ─────────────────────────────────────────────────────────

/// Measure the pixel width of `text` rendered at `scale`.
fn measure_text_width(font: &rusttype::Font, text: &str, scale: rusttype::Scale) -> i32 {
    font.layout(text, scale, rusttype::point(0.0, 0.0))
        .last()
        .and_then(|g| g.pixel_bounding_box().map(|bb| bb.max.x))
        .unwrap_or(0)
}

/// Greedy word-wrap: split `para` into lines that fit within `max_width` pixels.
/// Preserves empty paragraphs (returns one empty string) to keep blank lines.
fn word_wrap_text(
    font: &rusttype::Font,
    para: &str,
    scale: rusttype::Scale,
    max_width: i32,
) -> Vec<String> {
    if para.is_empty() {
        return vec![String::new()];
    }
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in para.split_whitespace() {
        let candidate = if current.is_empty() {
            word.to_string()
        } else {
            format!("{} {}", current, word)
        };
        if measure_text_width(font, &candidate, scale) <= max_width || current.is_empty() {
            current = candidate;
        } else {
            lines.push(current.clone());
            current = word.to_string();
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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
    /// Main text pixels.
    pixels: Vec<CachedPixel>,
    /// Shadow layer: offset copies of main pixels at (shadow_dx, shadow_dy).
    shadow_pixels: Vec<CachedPixel>,
    /// Outline layer: 8-neighbor expansion of sufficiently-covered pixels.
    outline_pixels: Vec<CachedPixel>,
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

/// Cache key for the glyph pixel cache.
#[derive(Hash, PartialEq, Eq, Clone)]
struct TextCacheKey {
    guid: String,
    width: u32,
    height: u32,
    font_size_bits: u32,
    /// Packed: bold=bit0, italic=bit1, underline=bit2, shadow=bit3, outline=bit4
    style_flags: u8,
    /// Shadow X offset clamped to i8 range.
    shadow_dx: i8,
    /// Shadow Y offset clamped to i8 range.
    shadow_dy: i8,
}

/// Italic shear factor: tan(14°) ≈ 0.249 — shifts a pixel upward by this
/// many horizontal pixels per pixel of distance above the baseline.
const ITALIC_SHEAR: f32 = 0.249;

/// Build and return a `TextLayoutCache` for `text` at (`width` × `height`).
/// Applies fake-bold (duplicate pixels shifted +1 x), fake-italic (x shear),
/// underline (pixel row at descent + 1), shadow (offset copies), and
/// outline (8-neighbor expansion).
/// This is the expensive path — called once per unique key.
fn build_layout_cache(
    font: &rusttype::Font<'static>,
    content: &str,
    text: &TextContent,
    width: u32,
    height: u32,
    word_wrap: bool,
) -> TextLayoutCache {
    let font_spec = text.font.as_ref();
    let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
    let bold      = font_spec.map(|f| f.bold).unwrap_or(false);
    let italic    = font_spec.map(|f| f.italic).unwrap_or(false);
    let underline = font_spec.map(|f| f.underline).unwrap_or(false);
    let shadow    = font_spec.map(|f| f.shadow).unwrap_or(false);
    let shadow_dx = font_spec.map(|f| f.shadow_dx).unwrap_or(1);
    let shadow_dy = font_spec.map(|f| f.shadow_dy).unwrap_or(1);
    let outline   = font_spec.map(|f| f.outline).unwrap_or(false);

    let scale = rusttype::Scale::uniform(font_size);
    let v_metrics = font.v_metrics(scale);
    let ascent = v_metrics.ascent;
    let line_height =
        (ascent - v_metrics.descent + v_metrics.line_gap).ceil() as i32;

    let style = text.style.as_ref();
    let valign = style.map(|s| s.valign.as_str()).unwrap_or("middle");

    let lines_text: Vec<String> = if text.single_line {
        vec![content.to_string()]
    } else if word_wrap {
        content.split('\n')
            .flat_map(|para| word_wrap_text(font, para, scale, width as i32))
            .collect()
    } else {
        content.split('\n').map(|s| s.to_string()).collect()
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
                let gi = glyph_idx as u8;
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

        // Shadow: offset copies of all main pixels
        let mut shadow_pixels = Vec::new();
        if shadow {
            for &px in &pixels {
                let sx = (px.x as i32 + shadow_dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                let sy = (px.y as i32 + shadow_dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                shadow_pixels.push(CachedPixel { x: sx, y: sy, alpha: px.alpha, glyph_idx: px.glyph_idx });
            }
        }

        // Outline: 8-neighbor expansion of sufficiently-covered pixels
        let mut outline_pixels = Vec::new();
        if outline {
            for &px in &pixels {
                if px.alpha > 64 {
                    for (dx, dy) in [(-1i32,-1i32),(0,-1),(1,-1),(-1,0),(1,0),(-1,1),(0,1),(1,1)] {
                        let ox = (px.x as i32 + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                        let oy = (px.y as i32 + dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                        outline_pixels.push(CachedPixel { x: ox, y: oy, alpha: 255, glyph_idx: px.glyph_idx });
                    }
                }
            }
        }

        raster_lines.push(RasterLine { pixels, shadow_pixels, outline_pixels, line_width, total_glyphs });
    }

    TextLayoutCache { lines: raster_lines, line_height, block_y }
}

// ── Pixel blit helpers ────────────────────────────────────────────────────────

/// Blit a slice of cached pixels with a single fixed (r,g,b) colour.
#[allow(clippy::too_many_arguments)]
#[inline]
fn blit_fixed(
    pixels: &[CachedPixel],
    x_off: i32, y_off: i32,
    tw: i32, th: i32,
    data: &mut [u8],
    r: u8, g: u8, b: u8,
) {
    for &px in pixels {
        let sx = x_off + px.x as i32;
        let sy = y_off + px.y as i32;
        if sx < 0 || sx >= tw || sy < 0 || sy >= th {
            continue;
        }
        let idx = ((sy * tw + sx) * 4) as usize;
        let a = px.alpha as f32 / 255.0;
        let da = data[idx + 3] as f32 / 255.0;
        let oa = a + da * (1.0 - a);
        if oa > 0.0 {
            data[idx]     = ((r as f32 * a + data[idx]     as f32 * da * (1.0 - a)) / oa) as u8;
            data[idx + 1] = ((g as f32 * a + data[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
            data[idx + 2] = ((b as f32 * a + data[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
            data[idx + 3] = (oa * 255.0) as u8;
        }
    }
}

/// Blit a slice of cached pixels with animated per-pixel colour.
///
/// `screen_width` is the target surface width in pixels; used to normalise
/// screen X to \[0, 1\] for position-based colour modes (rainbow, gradient, wave).
#[allow(clippy::too_many_arguments)]
#[inline]
fn blit_animated(
    pixels: &[CachedPixel],
    x_off: i32, y_off: i32,
    tw: i32, th: i32,
    data: &mut [u8],
    color_mode: ColorMode,
    total_glyphs: usize,
    elapsed_ms: u64,
) {
    let tw_f = tw.max(1) as f32;
    for &px in pixels {
        let sx = x_off + px.x as i32;
        let sy = y_off + px.y as i32;
        if sx < 0 || sx >= tw || sy < 0 || sy >= th {
            continue;
        }
        // Normalised screen X in [0, 1] — drives position-based colour modes.
        let screen_pos = sx as f32 / tw_f;
        let (r, g, b) = glyph_color(color_mode, px.glyph_idx as usize, total_glyphs, elapsed_ms, screen_pos);
        let idx = ((sy * tw + sx) * 4) as usize;
        let a = px.alpha as f32 / 255.0;
        let da = data[idx + 3] as f32 / 255.0;
        let oa = a + da * (1.0 - a);
        if oa > 0.0 {
            data[idx]     = ((r as f32 * a + data[idx]     as f32 * da * (1.0 - a)) / oa) as u8;
            data[idx + 1] = ((g as f32 * a + data[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
            data[idx + 2] = ((b as f32 * a + data[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
            data[idx + 3] = (oa * 255.0) as u8;
        }
    }
}

// ── Renderer ─────────────────────────────────────────────────────────────────

pub struct TextRenderer {
    font: rusttype::Font<'static>,
    cache: HashMap<TextCacheKey, TextLayoutCache>,
    /// Current data source values, updated each second by the player.
    data_sources: HashMap<String, String>,
}

impl TextRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self { font, cache: HashMap::new(), data_sources: HashMap::new() }
    }

    /// Replace the stored data sources with a fresh snapshot.
    pub fn set_data_sources(&mut self, sources: HashMap<String, String>) {
        self.data_sources = sources;
    }

    fn render_text_content(
        &mut self,
        text: &TextContent,
        target: &mut Pixmap,
        width: u32,
        height: u32,
        elapsed_ms: u64,
    ) {
        let content_str = match &text.string {
            Some(s) => s.as_str(),
            None => return,
        };
        if content_str.is_empty() {
            return;
        }

        let word_wrap = text.word_wrap;

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

        // ── Font spec ────────────────────────────────────────────────────────
        let font_spec = text.font.as_ref();
        let font_size = font_spec.map(|f| f.size).unwrap_or(12.0);
        let color_mode =
            parse_color_mode(font_spec.map(|f| f.color.as_str()).unwrap_or("#ff0000"));
        let has_shadow  = font_spec.map(|f| f.shadow).unwrap_or(false);
        let has_outline = font_spec.map(|f| f.outline).unwrap_or(false);
        let (sr, sg, sb) = parse_color(
            font_spec.filter(|_| has_shadow)
                     .map(|f| f.shadow_color.as_str())
                     .unwrap_or("#000000"),
        );
        let (or_, og, ob) = parse_color(
            font_spec.filter(|_| has_outline)
                     .map(|f| f.outline_color.as_str())
                     .unwrap_or("#000000"),
        );

        // ── Layout cache key ─────────────────────────────────────────────────
        let style_flags = {
            let b = font_spec.map(|f| f.bold).unwrap_or(false) as u8;
            let i = font_spec.map(|f| f.italic).unwrap_or(false) as u8;
            let u = font_spec.map(|f| f.underline).unwrap_or(false) as u8;
            let s = has_shadow as u8;
            let o = has_outline as u8;
            let w = word_wrap as u8;
            b | (i << 1) | (u << 2) | (s << 3) | (o << 4) | (w << 5)
        };
        let shadow_dx = font_spec
            .filter(|_| has_shadow)
            .map(|f| f.shadow_dx.clamp(-128, 127) as i8)
            .unwrap_or(1);
        let shadow_dy = font_spec
            .filter(|_| has_shadow)
            .map(|f| f.shadow_dy.clamp(-128, 127) as i8)
            .unwrap_or(1);
        let cache_key = TextCacheKey {
            guid: text.guid.clone(),
            width,
            height,
            font_size_bits: font_size.to_bits(),
            style_flags,
            shadow_dx,
            shadow_dy,
        };
        // Dynamic text (contains live variables) bypasses the cache and
        // rebuilds the layout every frame with the current expanded content.
        let is_dynamic = has_variables(content_str);
        let layout_owned: TextLayoutCache;
        let layout: &TextLayoutCache = if is_dynamic {
            let time_expanded = expand_variables(content_str);
            let expanded = expand_ds_tokens(&time_expanded, &self.data_sources);
            layout_owned = build_layout_cache(&self.font, &expanded, text, width, height, word_wrap);
            &layout_owned
        } else {
            if !self.cache.contains_key(&cache_key) {
                let entry = build_layout_cache(&self.font, content_str, text, width, height, word_wrap);
                self.cache.insert(cache_key.clone(), entry);
            }
            &self.cache[&cache_key]
        };

        // ── Blit pixels ──────────────────────────────────────────────────────
        let style = text.style.as_ref();
        let align = style.map(|s| s.align.as_str()).unwrap_or("center");

        let tw = target.width() as i32;
        let th = target.height() as i32;

        let scroll_speed  = text.scroll_speed.max(1) as u64;
        let scroll_dir    = text.scroll_dir.as_str();
        let total_lines   = layout.lines.len() as i32;
        let total_block_h = total_lines * layout.line_height;

        // Pre-compute vertical scroll position once for all lines
        let vert_total  = (total_block_h + height as i32) as u64;
        let vert_period = (vert_total * 1000) / scroll_speed;
        let vert_px     = ((elapsed_ms % vert_period.max(1)) as f64
                           * scroll_speed as f64 / 1000.0) as i32;

        let data = target.data_mut();

        for (line_idx, rline) in layout.lines.iter().enumerate() {
            let base_x = match align {
                "left"  => 0,
                "right" => (width as i32 - rline.line_width).max(0),
                _       => ((width as i32 - rline.line_width) / 2).max(0),
            };
            let y_fixed = layout.block_y + line_idx as i32 * layout.line_height;

            // Compute one or two (x, y) draw positions.
            // Two positions are used for seamless (looping) tickers.
            let offsets: Vec<(i32, i32)> = match scroll_dir {
                // Reverse scroll: text enters from the left edge
                "right" if text.single_line && rline.line_width > width as i32 => {
                    let total  = (rline.line_width + width as i32) as u64;
                    let period = (total * 1000) / scroll_speed;
                    let px     = ((elapsed_ms % period.max(1)) as f64
                                  * scroll_speed as f64 / 1000.0) as i32;
                    vec![(-(rline.line_width) + px, y_fixed)]
                }
                // Vertical scroll upward (entrance from bottom)
                "up" => vec![(
                    base_x,
                    height as i32 - vert_px + line_idx as i32 * layout.line_height,
                )],
                // Vertical scroll downward (entrance from top)
                "down" => vec![(
                    base_x,
                    -(total_block_h) + vert_px + line_idx as i32 * layout.line_height,
                )],
                // Default: "left" — scroll left, with optional seamless looping
                _ => {
                    if text.single_line && rline.line_width > width as i32 {
                        if text.seamless {
                            // Seamless: period = (line_width + gap) / speed
                            // Two copies always (line_width + gap) apart.
                            let gap    = text.ticker_gap as i32;
                            let step   = rline.line_width + gap;
                            let period = (step as u64 * 1000) / scroll_speed;
                            let px     = ((elapsed_ms % period.max(1)) as f64
                                          * scroll_speed as f64 / 1000.0) as i32;
                            let x1 = width as i32 - px;
                            vec![(x1, y_fixed), (x1 + step, y_fixed)]
                        } else {
                            let total  = (rline.line_width + width as i32) as u64;
                            let period = (total * 1000) / scroll_speed;
                            let px     = ((elapsed_ms % period.max(1)) as f64
                                          * scroll_speed as f64 / 1000.0) as i32;
                            vec![(width as i32 - px, y_fixed)]
                        }
                    } else {
                        vec![(base_x, y_fixed)]
                    }
                }
            };

            for (x_off, y_off) in offsets {
                // Shadow layer (drawn first — furthest back)
                if has_shadow {
                    blit_fixed(&rline.shadow_pixels, x_off, y_off, tw, th, data, sr, sg, sb);
                }
                // Outline layer
                if has_outline {
                    blit_fixed(&rline.outline_pixels, x_off, y_off, tw, th, data, or_, og, ob);
                }
                // Main text layer (animated colour)
                blit_animated(&rline.pixels, x_off, y_off, tw, th, data, color_mode, rline.total_glyphs, elapsed_ms);
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── has_variables ────────────────────────────────────────────────────────

    #[test]
    fn test_has_variables_true() {
        assert!(has_variables("{TIME}"));
        assert!(has_variables("Hello {DATE}!"));
        assert!(has_variables("Min:{MIN} Sec:{SEC}"));
    }

    #[test]
    fn test_has_variables_false() {
        assert!(!has_variables("Hello World"));
        assert!(!has_variables(""));
        assert!(!has_variables("No braces here"));
    }

    // ── expand_variables ─────────────────────────────────────────────────────

    #[test]
    fn test_expand_variables_passthrough() {
        // Strings without tokens are returned unchanged.
        assert_eq!(expand_variables("Hello World"), "Hello World");
        assert_eq!(expand_variables(""), "");
    }

    #[test]
    fn test_expand_variables_removes_tokens() {
        // After expansion no token placeholder should remain.
        let tokens = [
            "{TIME}", "{DATE}", "{WEEK}", "{YEAR}", "{MONTH}", "{MONTH_S}",
            "{MONTH_NUM}", "{DAY}", "{HOUR}", "{MIN}", "{SEC}",
            "{DATE_S}", "{WEEK_S}", "{TIME_S}",
        ];
        for token in &tokens {
            let result = expand_variables(token);
            assert!(
                !result.contains(token),
                "Token {token} was not replaced, got: {result}"
            );
        }
    }

    #[test]
    fn test_expand_variables_preserves_surrounding_text() {
        let result = expand_variables("Time: {TIME} | Date: {DATE}");
        assert!(result.starts_with("Time: "));
        assert!(result.contains(" | Date: "));
        assert!(!result.contains("{TIME}"));
        assert!(!result.contains("{DATE}"));
    }

    // ── word_wrap_text ───────────────────────────────────────────────────────

    fn test_font() -> rusttype::Font<'static> {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        rusttype::Font::try_from_bytes(font_data as &[u8]).expect("font load")
    }

    #[test]
    fn test_word_wrap_empty_para() {
        let font = test_font();
        let scale = rusttype::Scale::uniform(12.0);
        let result = word_wrap_text(&font, "", scale, 100);
        // Empty paragraph → one empty line preserved
        assert_eq!(result, vec!["".to_string()]);
    }

    #[test]
    fn test_word_wrap_short_fits_on_one_line() {
        let font = test_font();
        let scale = rusttype::Scale::uniform(12.0);
        // "Hi" is much shorter than 1000 px — must stay on one line.
        let result = word_wrap_text(&font, "Hi", scale, 1000);
        assert_eq!(result, vec!["Hi".to_string()]);
    }

    #[test]
    fn test_word_wrap_splits_at_boundary() {
        let font = test_font();
        let scale = rusttype::Scale::uniform(12.0);
        // Width of 1px forces each word onto its own line (single words always fit).
        let result = word_wrap_text(&font, "Hello World", scale, 1);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "World");
    }

    #[test]
    fn test_word_wrap_multiple_words_pack_when_space_allows() {
        let font = test_font();
        let scale = rusttype::Scale::uniform(12.0);
        // With generous width all words fit on one line.
        let result = word_wrap_text(&font, "One Two Three", scale, 2000);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "One Two Three");
    }

    // ── measure_text_width ───────────────────────────────────────────────────

    #[test]
    fn test_measure_text_width_empty() {
        let font = test_font();
        let scale = rusttype::Scale::uniform(12.0);
        assert_eq!(measure_text_width(&font, "", scale), 0);
    }

    #[test]
    fn test_measure_text_width_increases_with_length() {
        let font = test_font();
        let scale = rusttype::Scale::uniform(12.0);
        let w1 = measure_text_width(&font, "A", scale);
        let w2 = measure_text_width(&font, "AA", scale);
        assert!(w2 >= w1, "Longer string should be at least as wide");
    }

    // ── expand_ds_tokens ─────────────────────────────────────────────────────

    #[test]
    fn test_expand_ds_tokens_empty_sources() {
        let sources = HashMap::new();
        // No sources → string unchanged
        assert_eq!(expand_ds_tokens("{DS:ticker}", &sources), "{DS:ticker}");
        assert_eq!(expand_ds_tokens("plain text", &sources), "plain text");
    }

    #[test]
    fn test_expand_ds_tokens_known_key() {
        let mut sources = HashMap::new();
        sources.insert("ticker".to_string(), "AAPL 150.25".to_string());
        assert_eq!(expand_ds_tokens("{DS:ticker}", &sources), "AAPL 150.25");
    }

    #[test]
    fn test_expand_ds_tokens_unknown_key_omitted() {
        let mut sources = HashMap::new();
        sources.insert("a".to_string(), "A".to_string());
        // Unknown key placeholder is silently removed
        assert_eq!(expand_ds_tokens("{DS:missing}", &sources), "");
    }

    #[test]
    fn test_expand_ds_tokens_multiple_keys() {
        let mut sources = HashMap::new();
        sources.insert("score".to_string(), "3-1".to_string());
        sources.insert("team".to_string(), "Home".to_string());
        let result = expand_ds_tokens("{DS:team}: {DS:score}", &sources);
        assert_eq!(result, "Home: 3-1");
    }

    #[test]
    fn test_expand_ds_tokens_no_placeholder_passthrough() {
        let mut sources = HashMap::new();
        sources.insert("x".to_string(), "y".to_string());
        // No {DS:…} in input → unchanged
        assert_eq!(expand_ds_tokens("Hello World", &sources), "Hello World");
    }

    #[test]
    fn test_expand_ds_tokens_unclosed_brace_emitted() {
        let mut sources = HashMap::new();
        sources.insert("k".to_string(), "v".to_string());
        // Malformed {DS: without closing brace — emitted verbatim
        let result = expand_ds_tokens("prefix {DS:k", &sources);
        assert_eq!(result, "prefix {DS:k");
    }

    // ── color mode parsing + glyph_color ─────────────────────────────────────

    #[test]
    fn test_breathe_parses() {
        match parse_color_mode("breathe:#00ff88") {
            ColorMode::Breathe(r, g, b) => {
                assert_eq!(r, 0x00);
                assert_eq!(g, 0xff);
                assert_eq!(b, 0x88);
            }
            other => panic!("Expected Breathe, got {:?}", std::mem::discriminant(&other)),
        }
    }

    #[test]
    fn test_breathe_dims_at_zero() {
        // At elapsed_ms = 0: sin(0) = 0, so v = 0.10 + 0.90 * 0.5 = 0.55
        // The value is not at minimum; minimum occurs at sin = -1 → 750ms
        let mode = ColorMode::Breathe(255, 255, 255);
        let (r, _g, _b) = glyph_color(mode, 0, 1, 0, 0.5);
        assert!(r > 0, "Breathe should never fully extinguish at t=0");
    }

    #[test]
    fn test_breathe_all_chars_same_color() {
        // Breathe doesn't vary by glyph_idx — all chars get the same color.
        let mode = ColorMode::Breathe(200, 100, 50);
        let c0 = glyph_color(mode, 0, 10, 500, 0.3);
        let c1 = glyph_color(mode, 5, 10, 500, 0.3);
        let c9 = glyph_color(mode, 9, 10, 500, 0.3);
        assert_eq!(c0, c1);
        assert_eq!(c1, c9);
    }

    #[test]
    fn test_rainbow_varies_by_screen_pos() {
        // Two pixels at different screen positions should get different rainbow hues.
        let mode = ColorMode::Rainbow;
        let left  = glyph_color(mode, 0, 10, 0, 0.0);
        let right = glyph_color(mode, 9, 10, 0, 0.9);
        assert_ne!(left, right, "Rainbow should produce different colors at different screen positions");
    }

    #[test]
    fn test_gradient_left_vs_right() {
        // Left edge of screen gets color A, right edge gets color B.
        let mode = ColorMode::Gradient(255, 0, 0, 0, 0, 255);
        let left  = glyph_color(mode, 0, 10, 0, 0.0);
        let right = glyph_color(mode, 9, 10, 0, 1.0);
        assert_eq!(left,  (255, 0, 0),   "Left edge should be pure red");
        assert_eq!(right, (0, 0, 255),   "Right edge should be pure blue");
    }
}
