/// Animated border / neon-frame renderer.
///
/// Reads the `Border` struct from the program model and draws an animated
/// rectangular border around the display (program-level) or an area
/// (area-level).
///
/// Border styles selected by `border.index`:
///   0  – none (invisible)
///   1  – white solid
///   2  – red solid
///   3  – green solid
///   4  – blue solid
///   5  – yellow solid
///   6  – cyan solid
///   7  – magenta solid
///   8  – rainbow (hue walks around the perimeter over time)
///   9  – neon chase (cyan bead races around the perimeter)
///   10 – breathing (all pixels pulse in and out)
///   11 – blink (all pixels toggle on/off)
///   12 – alternating segments (two-colour strobe pattern)
///   13 – sparkle (random bright pixels on a dim base)
///   _  – white solid (fallback)
///
/// The `border.effect` string **overrides** the index-based style:
///   "rainbow"           → rainbow
///   "neon" / "chase"    → cyan chase
///   "breath" / "breathing" → white breathing
///   "blink"             → red blink
///   "solid"             → white solid
///   "sparkle"           → sparkle
///
/// `border.speed` is parsed as an integer 1–10 (default 5).
use std::f32::consts::PI;
use tiny_skia::Pixmap;

use crate::program::model::Border;

/// Rendered thickness of the border stripe in pixels.
const THICKNESS: i32 = 3;

#[derive(Clone, Copy)]
enum Style {
    None,
    Solid(u8, u8, u8),
    Rainbow,
    Chase(u8, u8, u8),
    Breathing(u8, u8, u8),
    Blink(u8, u8, u8),
    Alternating,
    Sparkle(u8, u8, u8),
}

fn decode_style(b: &Border) -> Style {
    match b.effect.to_lowercase().as_str() {
        "rainbow" => return Style::Rainbow,
        "neon" | "chase" => return Style::Chase(0, 200, 255),
        "breath" | "breathing" => return Style::Breathing(255, 255, 255),
        "blink" => return Style::Blink(255, 50, 50),
        "solid" => return Style::Solid(255, 255, 255),
        "sparkle" => return Style::Sparkle(255, 255, 255),
        _ => {}
    }
    match b.index {
        0 => Style::None,
        1 => Style::Solid(255, 255, 255),
        2 => Style::Solid(255, 40, 40),
        3 => Style::Solid(40, 255, 40),
        4 => Style::Solid(40, 80, 255),
        5 => Style::Solid(255, 255, 0),
        6 => Style::Solid(0, 255, 255),
        7 => Style::Solid(255, 0, 255),
        8 => Style::Rainbow,
        9 => Style::Chase(0, 200, 255),
        10 => Style::Breathing(255, 255, 255),
        11 => Style::Blink(255, 220, 0),
        12 => Style::Alternating,
        13 => Style::Sparkle(100, 200, 255),
        _ => Style::Solid(255, 255, 255),
    }
}

// ---------------------------------------------------------------------------
// Colour helpers
// ---------------------------------------------------------------------------

/// Convert HSV (h: 0..360, s/v: 0..1) to (r, g, b).
fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (u8, u8, u8) {
    let h = h.rem_euclid(360.0);
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0).rem_euclid(2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 {
        0 => (c, x, 0.0_f32),
        1 => (x, c, 0.0_f32),
        2 => (0.0_f32, c, x),
        3 => (0.0_f32, x, c),
        4 => (x, 0.0_f32, c),
        _ => (c, 0.0_f32, x),
    };
    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

// ---------------------------------------------------------------------------
// Perimeter position
// ---------------------------------------------------------------------------

/// Clockwise perimeter position (0 .. 2*(w+h)) for a border pixel.
///
/// Projects the pixel onto the nearest outer-ring edge so that inner
/// layers (t > 0) animate in sync with the outermost ring.
fn perimeter_pos(px: i32, py: i32, w: i32, h: i32) -> u64 {
    let px = px.clamp(0, w - 1);
    let py = py.clamp(0, h - 1);
    let dt = py;          // distance from top
    let dr = w - 1 - px; // distance from right
    let db = h - 1 - py; // distance from bottom
    let dl = px;          // distance from left
    let m = dt.min(dr).min(db).min(dl);
    if dt == m {
        px as u64                                           // top  →
    } else if dr == m {
        w as u64 + py as u64                               // right ↓
    } else if db == m {
        w as u64 + h as u64 + (w - 1 - px) as u64         // bottom ←
    } else {
        2 * w as u64 + h as u64 + (h - 1 - py) as u64     // left  ↑
    }
}

// ---------------------------------------------------------------------------
// Per-pixel colour computation
// ---------------------------------------------------------------------------

fn compute_color(
    style: Style,
    px: i32,
    py: i32,
    w: i32,
    h: i32,
    elapsed_ms: u64,
    speed: u64,
) -> (u8, u8, u8) {
    match style {
        Style::None => (0, 0, 0),

        Style::Solid(r, g, b) => (r, g, b),

        Style::Rainbow => {
            let perim = 2 * (w + h) as u64;
            let pos = perimeter_pos(px, py, w, h);
            let hue = ((pos * 360 / perim) + elapsed_ms * speed / 50) as f32 % 360.0;
            hsv_to_rgb(hue, 1.0, 1.0)
        }

        Style::Chase(r, g, b) => {
            let perim = 2 * (w + h) as u64;
            let pos = perimeter_pos(px, py, w, h);
            // Head advances: one full lap every 3 seconds at speed=5
            let head = (elapsed_ms * speed * perim / 15000) % perim;
            // Signed trailing distance from head
            let dist = if pos <= head {
                head - pos
            } else {
                perim - (pos - head)
            };
            let tail = (perim / 8).max(4);
            if dist < tail {
                let v = 1.0 - dist as f32 / tail as f32;
                (
                    (r as f32 * v) as u8,
                    (g as f32 * v) as u8,
                    (b as f32 * v) as u8,
                )
            } else {
                (0, 0, 0)
            }
        }

        Style::Breathing(r, g, b) => {
            let t = elapsed_ms as f32 * speed as f32 / 3000.0;
            let v = (f32::sin(t * PI) * 0.5 + 0.5).clamp(0.0, 1.0);
            (
                (r as f32 * v) as u8,
                (g as f32 * v) as u8,
                (b as f32 * v) as u8,
            )
        }

        Style::Blink(r, g, b) => {
            let period_ms = 1000 / speed.clamp(1, 10);
            if (elapsed_ms / period_ms) % 2 == 0 {
                (r, g, b)
            } else {
                (0, 0, 0)
            }
        }

        Style::Alternating => {
            let perim = 2 * (w + h) as u64;
            let pos = perimeter_pos(px, py, w, h);
            let seg = (perim / 16).max(2);
            let phase = (elapsed_ms * speed / 400) % 2;
            if (pos / seg) % 2 == phase {
                (255, 0, 255)
            } else {
                (0, 0, 255)
            }
        }

        Style::Sparkle(r, g, b) => {
            // Deterministic "random" sparkle: hash position + time bucket
            let time_bucket = elapsed_ms * speed / 200;
            let hash = (px as u64)
                .wrapping_mul(2654435761)
                .wrapping_add((py as u64).wrapping_mul(2246822519))
                .wrapping_add(time_bucket.wrapping_mul(1442695040888963407));
            let bright = ((hash >> 56) & 0xff) as u8;
            if bright > 180 {
                (r, g, b)
            } else {
                (
                    (r as u32 * bright as u32 / 255) as u8,
                    (g as u32 * bright as u32 / 255) as u8,
                    (b as u32 * bright as u32 / 255) as u8,
                )
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Pixel writer
// ---------------------------------------------------------------------------

#[inline]
fn write_pixel(data: &mut [u8], px: i32, py: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    if px < 0 || px >= w || py < 0 || py >= h {
        return;
    }
    let idx = ((py * w + px) * 4) as usize;
    if idx + 3 >= data.len() {
        return;
    }
    data[idx] = r;
    data[idx + 1] = g;
    data[idx + 2] = b;
    data[idx + 3] = 255;
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Draw the border described by `border` onto `fb`.
///
/// `elapsed_ms` drives all animations and should increase monotonically.
pub fn draw_border(fb: &mut Pixmap, border: &Border, elapsed_ms: u64) {
    let style = decode_style(border);
    if matches!(style, Style::None) {
        return;
    }

    let speed = border.speed.parse::<u64>().unwrap_or(5).clamp(1, 10);
    let w = fb.width() as i32;
    let h = fb.height() as i32;
    let t_max = THICKNESS.min((w / 2).min(h / 2));

    // Grab the pixel buffer once to avoid repeated mutable borrows.
    let data = fb.data_mut();

    for t in 0..t_max {
        // Top stripe (left → right)
        for x in 0..w {
            let (r, g, b) = compute_color(style, x, t, w, h, elapsed_ms, speed);
            write_pixel(data, x, t, w, h, r, g, b);
        }
        // Bottom stripe (right → left, matching clockwise perimeter)
        for x in 0..w {
            let (r, g, b) = compute_color(style, w - 1 - x, h - 1 - t, w, h, elapsed_ms, speed);
            write_pixel(data, x, h - 1 - t, w, h, r, g, b);
        }
        // Left stripe (bottom → top, excluding corners already drawn)
        for y in (t + 1)..(h - 1 - t) {
            let (r, g, b) = compute_color(style, t, y, w, h, elapsed_ms, speed);
            write_pixel(data, t, y, w, h, r, g, b);
        }
        // Right stripe (top → bottom, excluding corners already drawn)
        for y in (t + 1)..(h - 1 - t) {
            let (r, g, b) = compute_color(style, w - 1 - t, y, w, h, elapsed_ms, speed);
            write_pixel(data, w - 1 - t, y, w, h, r, g, b);
        }
    }
}
