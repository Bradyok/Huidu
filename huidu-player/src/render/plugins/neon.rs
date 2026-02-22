/// Neon decoration content renderer (libneon_plugin.so equivalent).
///
/// Renders 36 animated glowing shapes on the LED display.
/// Shape index reference (from images/Border/*.png in HDPlayer):
///
///  0  Circle / ellipse           12  8-point star / starburst
///  1  Hexagon                    13  Three linked circles
///  2  Pentagon                   14  Four oval row
///  3  Diamond (rotated square)   15  Four interlocked rings
///  4  Triangle (up)              16  Shield / badge
///  5  Triangle (down)            17  Shield (smaller)
///  6  Heart                      18  Teardrop
///  7  Thumbs up                  19  Apple
///  8  Speech bubble              20  Trophy / cup
///  9  Square / rectangle         21  Fish
/// 10  Arrow (right)              22  Flag / wave
/// 11  5-point star               23  Car silhouette
///
/// 24  Plain rectangle border     30  Puzzle-piece corners
/// 25  Double-line rectangle      31  Clover / trefoil
/// 26  Rounded rectangle          32  Butterfly
/// 27  Crossed-corner rectangle   33  Maple leaf
/// 28  Floral-corner rectangle    34  Teardrop (alt)
/// 29  Snowflake-corner rectangle 35  Comet / shooting star
use std::f32::consts::PI;
use std::path::Path;

use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ContentItem, NeonContent};
use crate::render::plugins::ContentRenderer;

pub struct NeonRenderer;

impl NeonRenderer {
    pub fn new() -> Self {
        Self
    }

    fn render_neon(&self, neon: &NeonContent, target: &mut Pixmap, elapsed_ms: u64) {
        let w = target.width() as i32;
        let h = target.height() as i32;
        if w < 4 || h < 4 {
            return;
        }

        let speed = neon.speed.clamp(1, 10) as f32;

        // Resolve base colour.
        let (r, g, b) = if neon.color.eq_ignore_ascii_case("rainbow") {
            let hue = (elapsed_ms as f32 * speed / 5000.0 * 360.0) % 360.0;
            hsv_to_rgb(hue, 1.0, 1.0)
        } else if let Some(rest) = neon.color.strip_prefix("gradient:").or_else(|| neon.color.strip_prefix("Gradient:")) {
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            if parts.len() == 2 {
                let (r1, g1, b1) = parse_color(parts[0]);
                let (r2, g2, b2) = parse_color(parts[1]);
                let t = (elapsed_ms as f32 * speed / 5000.0).fract();
                let lerp = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t) as u8;
                (lerp(r1, r2), lerp(g1, g2), lerp(b1, b2))
            } else {
                parse_color(&neon.color)
            }
        } else {
            parse_color(&neon.color)
        };

        // Breathing pulse (single-color mode only).
        let (r, g, b) = if neon.single_color {
            let pulse = (elapsed_ms as f32 * speed * 0.002).sin() * 0.25 + 0.75;
            (
                (r as f32 * pulse).min(255.0) as u8,
                (g as f32 * pulse).min(255.0) as u8,
                (b as f32 * pulse).min(255.0) as u8,
            )
        } else {
            (r, g, b)
        };

        draw_shape(neon.index, target, w, h, r, g, b, elapsed_ms, neon.speed);
    }
}

impl ContentRenderer for NeonRenderer {
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
        let neon = match item {
            ContentItem::Neon(n) => n,
            _ => return false,
        };
        self.render_neon(neon, target, elapsed_ms);
        true
    }
}

// ── Shape dispatcher ─────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn draw_shape(
    index: u32,
    target: &mut Pixmap,
    w: i32,
    h: i32,
    r: u8,
    g: u8,
    b: u8,
    elapsed_ms: u64,
    speed: u32,
) {
    let cx = w / 2;
    let cy = h / 2;
    let radius = (w.min(h) / 2 - 2).max(4);

    match index {
        // ── Geometric shapes ─────────────────────────────────────────────────
        0  => ellipse_outline(target, cx, cy, w / 2 - 2, h / 2 - 2, r, g, b, 2),
        1  => regular_polygon(target, cx, cy, 6, radius, 0.0,          r, g, b),
        2  => regular_polygon(target, cx, cy, 5, radius, -PI / 2.0,    r, g, b),
        3  => regular_polygon(target, cx, cy, 4, radius,  PI / 4.0,    r, g, b),
        4  => regular_polygon(target, cx, cy, 3, radius, -PI / 2.0,    r, g, b),
        5  => regular_polygon(target, cx, cy, 3, radius,  PI / 2.0,    r, g, b),
        6  => heart_outline(target, cx, cy, w, h, r, g, b),
        7  => thumbsup_outline(target, cx, cy, w, h, r, g, b),
        8  => speech_bubble(target, cx, cy, w, h, r, g, b),
        9  => rect_border(target, 2, r, g, b),
        10 => arrow_right(target, cx, cy, w, h, r, g, b),
        11 => star_outline(target, cx, cy, 5, radius, (radius as f32 * 0.40) as i32, r, g, b),
        12 => star_outline(target, cx, cy, 8, radius, (radius as f32 * 0.55) as i32, r, g, b),
        // ── Circles / rings ──────────────────────────────────────────────────
        13 => chain_circles(target, cx, cy, w, h, r, g, b, 3),
        14 => oval_row(target, cx, cy, w, h, r, g, b, 4),
        15 => interlocked_rings(target, cx, cy, w, h, r, g, b),
        // ── Complex shapes ───────────────────────────────────────────────────
        16 => shield(target, cx, cy, w, h, r, g, b),
        17 => shield(target, cx, cy, w * 3 / 4, h * 3 / 4, r, g, b),
        18 => teardrop(target, cx, cy, w, h, r, g, b),
        19 => apple(target, cx, cy, w, h, r, g, b),
        20 => trophy(target, cx, cy, w, h, r, g, b),
        21 => fish(target, cx, cy, w, h, r, g, b),
        22 => flag(target, cx, cy, w, h, r, g, b),
        23 => car(target, cx, cy, w, h, r, g, b),
        // ── Rectangular border variants ──────────────────────────────────────
        24 => rect_border(target, 2, r, g, b),
        25 => { rect_border(target, 1, r, g, b); rect_border_inset(target, 4, r, g, b); }
        26 => rounded_rect_border(target, 2, 8, r, g, b),
        27 => { rect_border(target, 2, r, g, b); corner_notch(target, 6, r, g, b); }
        28 => { rect_border(target, 2, r, g, b); corner_circles(target, 4, r, g, b); }
        29 => { rounded_rect_border(target, 2, 4, r, g, b); corner_circles(target, 5, r, g, b); }
        30 => { rect_border(target, 2, r, g, b); midpoint_ticks(target, 4, r, g, b); }
        31 => clover(target, cx, cy, w, h, r, g, b),
        32 => butterfly(target, cx, cy, w, h, r, g, b),
        33 => maple_leaf(target, cx, cy, w, h, r, g, b),
        34 => teardrop(target, cx, cy, w, h, r, g, b),
        35 => comet(target, cx, cy, w, h, r, g, b, elapsed_ms, speed),
        _  => ellipse_outline(target, cx, cy, w / 2 - 2, h / 2 - 2, r, g, b, 2),
    }
}

// ── Low-level pixel / line primitives ────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
#[inline]
fn put(data: &mut [u8], x: i32, y: i32, tw: i32, th: i32, r: u8, g: u8, b: u8) {
    if x < 0 || x >= tw || y < 0 || y >= th {
        return;
    }
    let i = ((y * tw + x) * 4) as usize;
    data[i]     = r;
    data[i + 1] = g;
    data[i + 2] = b;
    data[i + 3] = 255;
}

/// Bresenham line, 1 px.
#[allow(clippy::too_many_arguments)]
fn bline(data: &mut [u8], mut x0: i32, mut y0: i32, x1: i32, y1: i32,
         tw: i32, th: i32, r: u8, g: u8, b: u8) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        put(data, x0, y0, tw, th, r, g, b);
        if x0 == x1 && y0 == y1 { break; }
        let e2 = 2 * err;
        if e2 > -dy { err -= dy; x0 += sx; }
        if e2 <  dx { err += dx; y0 += sy; }
    }
}

/// Thick line (draws 3 parallel 1-px lines — produces ~3px stroke).
#[allow(clippy::too_many_arguments)]
fn thick_line(target: &mut Pixmap, x0: i32, y0: i32, x1: i32, y1: i32, r: u8, g: u8, b: u8) {
    let tw = target.width() as i32;
    let th = target.height() as i32;
    let dx = (x1 - x0) as f32;
    let dy = (y1 - y0) as f32;
    let len = dx.hypot(dy).max(0.001);
    let ox = ((-dy / len) + 0.5) as i32;
    let oy = (( dx / len) + 0.5) as i32;
    let d = target.data_mut();
    bline(d, x0,    y0,    x1,    y1,    tw, th, r, g, b);
    bline(d, x0+ox, y0+oy, x1+ox, y1+oy, tw, th, r, g, b);
    bline(d, x0-ox, y0-oy, x1-ox, y1-oy, tw, th, r, g, b);
}

/// Connect a slice of (x,y) points with thick lines.
fn polyline(target: &mut Pixmap, pts: &[(i32, i32)], closed: bool, r: u8, g: u8, b: u8) {
    if pts.len() < 2 { return; }
    for i in 0..pts.len() - 1 {
        thick_line(target, pts[i].0, pts[i].1, pts[i+1].0, pts[i+1].1, r, g, b);
    }
    if closed {
        let last = pts.len() - 1;
        thick_line(target, pts[last].0, pts[last].1, pts[0].0, pts[0].1, r, g, b);
    }
}

/// Parametric ellipse outline via polyline.
#[allow(clippy::too_many_arguments)]
fn ellipse_outline(target: &mut Pixmap, cx: i32, cy: i32, rx: i32, ry: i32,
                   r: u8, g: u8, b: u8, _thickness: i32) {
    let steps = (rx.max(ry) * 4).clamp(32, 128);
    let pts: Vec<(i32, i32)> = (0..=steps)
        .map(|i| {
            let a = i as f32 * PI * 2.0 / steps as f32;
            (cx + (a.cos() * rx as f32) as i32, cy + (a.sin() * ry as f32) as i32)
        })
        .collect();
    polyline(target, &pts, false, r, g, b);
}

/// Regular N-sided polygon outline.
#[allow(clippy::too_many_arguments)]
fn regular_polygon(target: &mut Pixmap, cx: i32, cy: i32, sides: u32, radius: i32,
                   start: f32, r: u8, g: u8, b: u8) {
    let pts: Vec<(i32, i32)> = (0..sides)
        .map(|i| {
            let a = start + i as f32 * PI * 2.0 / sides as f32;
            (cx + (a.cos() * radius as f32) as i32, cy + (a.sin() * radius as f32) as i32)
        })
        .collect();
    polyline(target, &pts, true, r, g, b);
}

/// N-pointed star outline.
#[allow(clippy::too_many_arguments)]
fn star_outline(target: &mut Pixmap, cx: i32, cy: i32, points: u32,
                outer_r: i32, inner_r: i32, r: u8, g: u8, b: u8) {
    let pts: Vec<(i32, i32)> = (0..points * 2)
        .map(|i| {
            let a = i as f32 * PI / points as f32 - PI / 2.0;
            let rad = if i % 2 == 0 { outer_r } else { inner_r };
            (cx + (a.cos() * rad as f32) as i32, cy + (a.sin() * rad as f32) as i32)
        })
        .collect();
    polyline(target, &pts, true, r, g, b);
}

// ── Shape implementations ─────────────────────────────────────────────────────

/// Heart (parametric: x=16·sin³t, y=13·cos·t − 5·cos·2t − 2·cos·3t − cos·4t).
#[allow(clippy::too_many_arguments)]
fn heart_outline(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let scale = (w as f32 / 34.0).min(h as f32 / 30.0) * 0.85;
    let steps = 64;
    let pts: Vec<(i32, i32)> = (0..=steps)
        .map(|i| {
            let t = i as f32 * PI * 2.0 / steps as f32;
            let hx = 16.0 * t.sin().powi(3);
            let hy = 13.0 * t.cos() - 5.0 * (2.0*t).cos() - 2.0 * (3.0*t).cos() - (4.0*t).cos();
            (cx + (hx * scale) as i32, cy - (hy * scale) as i32)
        })
        .collect();
    polyline(target, &pts, false, r, g, b);
}

/// Simplified thumbs-up from line segments.
#[allow(clippy::too_many_arguments)]
fn thumbsup_outline(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let lx = cx - w / 4;
    let rx_  = cx + w / 4;
    let ty = cy - h / 3;
    let by = cy + h / 3;
    // Thumb shaft (angled rectangle top)
    polyline(target, &[(lx, cy), (lx, ty), (rx_, ty), (rx_, cy)], true, r, g, b);
    // Fist block below
    polyline(target, &[(lx, cy), (lx, by), (rx_, by), (rx_, cy)], false, r, g, b);
    // Thumb extension
    thick_line(target, lx, ty, cx, ty - h / 4, r, g, b);
    thick_line(target, cx, ty - h / 4, rx_, ty, r, g, b);
}

/// Speech bubble (rounded-corner rect + tail at bottom-left).
#[allow(clippy::too_many_arguments)]
fn speech_bubble(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let m = 3;
    let bx = cx - w / 2 + m;
    let by = cy - h / 2 + m;
    let ex = cx + w / 2 - m;
    let ey = cy + h / 2 - h / 4;
    // Main box
    polyline(target, &[(bx, by), (ex, by), (ex, ey), (bx + w/4, ey), (bx, ey + h/4), (bx, by)], false, r, g, b);
}

/// Right-pointing arrow.
#[allow(clippy::too_many_arguments)]
fn arrow_right(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let lx = cx - w / 3;
    let rx_ = cx + w / 3;
    let hw = h / 5;
    let hh = h / 3;
    // Shaft
    polyline(target, &[(lx, cy - hw), (cx, cy - hw), (cx, cy + hw), (lx, cy + hw)], true, r, g, b);
    // Arrowhead
    polyline(target, &[(cx, cy - hh), (rx_, cy), (cx, cy + hh)], false, r, g, b);
    thick_line(target, cx, cy - hh, cx, cy + hh, r, g, b);
}

/// Rectangular border (all four sides, given margin).
fn rect_border(target: &mut Pixmap, m: i32, r: u8, g: u8, b: u8) {
    let w = target.width() as i32;
    let h = target.height() as i32;
    polyline(target, &[(m, m), (w-1-m, m), (w-1-m, h-1-m), (m, h-1-m)], true, r, g, b);
}

/// Inset rectangle (secondary line inside the border).
fn rect_border_inset(target: &mut Pixmap, m: i32, r: u8, g: u8, b: u8) {
    let w = target.width() as i32;
    let h = target.height() as i32;
    polyline(target, &[(m, m), (w-1-m, m), (w-1-m, h-1-m), (m, h-1-m)], true, r, g, b);
}

/// Rounded rectangle border (corners clipped diagonally).
fn rounded_rect_border(target: &mut Pixmap, m: i32, c: i32, r: u8, g: u8, b: u8) {
    let w = target.width() as i32;
    let h = target.height() as i32;
    let pts = [
        (m + c, m),     (w-1-m-c, m),
        (w-1-m, m + c), (w-1-m, h-1-m-c),
        (w-1-m-c, h-1-m), (m+c, h-1-m),
        (m, h-1-m-c),   (m, m+c),
    ];
    polyline(target, &pts, true, r, g, b);
}

/// Small diagonal notches at each corner.
fn corner_notch(target: &mut Pixmap, size: i32, r: u8, g: u8, b: u8) {
    let w = target.width() as i32;
    let h = target.height() as i32;
    thick_line(target, 0,       0,       size,   size,   r, g, b);
    thick_line(target, w-1,     0,       w-1-size, size,   r, g, b);
    thick_line(target, 0,       h-1,     size,   h-1-size, r, g, b);
    thick_line(target, w-1,     h-1,     w-1-size, h-1-size, r, g, b);
}

/// Small circles at each corner.
fn corner_circles(target: &mut Pixmap, cr: i32, r: u8, g: u8, b: u8) {
    let w = target.width() as i32;
    let h = target.height() as i32;
    for (cx, cy) in &[(cr+1, cr+1), (w-cr-2, cr+1), (cr+1, h-cr-2), (w-cr-2, h-cr-2)] {
        ellipse_outline(target, *cx, *cy, cr, cr, r, g, b, 1);
    }
}

/// Midpoint ticks on each side (puzzle-like).
fn midpoint_ticks(target: &mut Pixmap, size: i32, r: u8, g: u8, b: u8) {
    let w = target.width() as i32;
    let h = target.height() as i32;
    let mx = w / 2;
    let my = h / 2;
    thick_line(target, mx - size/2, 0, mx + size/2, 0, r, g, b);
    thick_line(target, mx - size/2, h-1, mx + size/2, h-1, r, g, b);
    thick_line(target, 0, my - size/2, 0, my + size/2, r, g, b);
    thick_line(target, w-1, my - size/2, w-1, my + size/2, r, g, b);
}

/// Three horizontally linked circles.
#[allow(clippy::too_many_arguments)]
fn chain_circles(target: &mut Pixmap, cx: i32, cy: i32, w: i32, _h: i32, r: u8, g: u8, b: u8, n: i32) {
    let cr = w / (n * 3);
    let spacing = w / n;
    let start_x = cx - spacing * (n - 1) / 2;
    for i in 0..n {
        let x = start_x + i * spacing;
        ellipse_outline(target, x, cy, cr, cr, r, g, b, 2);
    }
}

/// Row of horizontal ovals.
#[allow(clippy::too_many_arguments)]
fn oval_row(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8, n: i32) {
    let ow = w / (n * 2);
    let oh = (h / 4).max(2);
    let spacing = w / n;
    let start_x = cx - spacing * (n - 1) / 2;
    for i in 0..n {
        let x = start_x + i * spacing;
        ellipse_outline(target, x, cy, ow, oh, r, g, b, 2);
    }
}

/// Four partially-overlapping rings arranged in a 2×2 grid.
#[allow(clippy::too_many_arguments)]
fn interlocked_rings(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let cr = (w.min(h) / 5).max(3);
    let off = cr * 3 / 4;
    for (dx, dy) in &[(-off, -off), (off, -off), (-off, off), (off, off)] {
        ellipse_outline(target, cx + dx, cy + dy, cr, cr, r, g, b, 2);
    }
}

/// Shield / badge (pentagon with a pointed bottom).
#[allow(clippy::too_many_arguments)]
fn shield(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let lx = cx - w / 3;
    let rx_ = cx + w / 3;
    let ty = cy - h / 3;
    let my = cy + h / 8;
    let by = cy + h / 3;
    polyline(target, &[(lx, ty), (rx_, ty), (rx_, my), (cx, by), (lx, my)], true, r, g, b);
}

/// Teardrop (circle body + triangle tip at bottom).
#[allow(clippy::too_many_arguments)]
fn teardrop(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let cr = (w.min(h) / 3).max(4);
    let top_cy = cy - h / 6;
    ellipse_outline(target, cx, top_cy, cr, cr, r, g, b, 2);
    // Taper down to point
    thick_line(target, cx - cr, top_cy, cx, cy + h / 3, r, g, b);
    thick_line(target, cx + cr, top_cy, cx, cy + h / 3, r, g, b);
}

/// Simplified apple (circle + small notch + stem).
#[allow(clippy::too_many_arguments)]
fn apple(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let cr = (w.min(h) / 3).max(4);
    ellipse_outline(target, cx, cy, cr, cr, r, g, b, 2);
    // Notch at top center
    thick_line(target, cx - cr/3, cy - cr, cx + cr/3, cy - cr, r, g, b);
    // Stem
    thick_line(target, cx, cy - cr, cx + cr/3, cy - cr - h/6, r, g, b);
}

/// Trophy / cup (U-shape body + handles + base).
#[allow(clippy::too_many_arguments)]
fn trophy(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let hw = w / 4;
    let ty = cy - h / 3;
    let by = cy + h / 4;
    let base_y = cy + h / 3;
    // Cup body
    polyline(target, &[
        (cx - hw, ty), (cx - hw, by),
        (cx, base_y), (cx + hw, by), (cx + hw, ty),
    ], false, r, g, b);
    // Base platform
    thick_line(target, cx - hw, base_y, cx + hw, base_y, r, g, b);
    // Handles
    thick_line(target, cx - hw, ty + h/8, cx - hw - hw/2, ty + h/4, r, g, b);
    thick_line(target, cx + hw, ty + h/8, cx + hw + hw/2, ty + h/4, r, g, b);
}

/// Fish (lens body + triangular tail).
#[allow(clippy::too_many_arguments)]
fn fish(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let bw = w / 3;
    let bh = h / 3;
    // Body = horizontal ellipse
    ellipse_outline(target, cx - w/8, cy, bw, bh, r, g, b, 2);
    // Tail
    let tail_x = cx + bw;
    polyline(target, &[
        (tail_x, cy),
        (tail_x + w/4, cy - h/3),
        (tail_x + w/4, cy + h/3),
    ], true, r, g, b);
    // Eye dot
    let tw = target.width() as i32;
    let th = target.height() as i32;
    let d = target.data_mut();
    for dy in -1..=1i32 {
        for dx in -1..=1i32 {
            put(d, cx - bw/2, cy - bh/3 + dy, tw, th, r, g, b);
            let _ = dx;
        }
    }
}

/// Waving flag on a pole.
#[allow(clippy::too_many_arguments)]
fn flag(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let pole_x = cx - w / 4;
    let ty = cy - h / 3;
    let by = cy + h / 3;
    // Pole
    thick_line(target, pole_x, ty - h/6, pole_x, by, r, g, b);
    // Flag body (wave approximation)
    polyline(target, &[
        (pole_x, ty),
        (pole_x + w/3, ty + h/8),
        (pole_x + w/3*2, ty),
        (pole_x + w/3*2, ty + h/4),
        (pole_x + w/3, ty + h/4 + h/8),
        (pole_x, ty + h/4),
    ], true, r, g, b);
}

/// Car silhouette (rectangle body + two wheel arches).
#[allow(clippy::too_many_arguments)]
fn car(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let bw = w * 2 / 5;
    let bh = h / 5;
    let roof_w = w / 4;
    let roof_h = h / 4;
    // Body
    polyline(target, &[
        (cx - bw, cy),     (cx - bw, cy + bh),
        (cx + bw, cy + bh),(cx + bw, cy),
    ], true, r, g, b);
    // Cabin / roof
    polyline(target, &[
        (cx - bw + bw/3, cy),
        (cx - roof_w, cy - roof_h),
        (cx + roof_w, cy - roof_h),
        (cx + bw - bw/3, cy),
    ], false, r, g, b);
    // Wheels (circles below body)
    let wr = bh * 2 / 3;
    ellipse_outline(target, cx - bw/2, cy + bh + wr/2, wr, wr, r, g, b, 2);
    ellipse_outline(target, cx + bw/2, cy + bh + wr/2, wr, wr, r, g, b, 2);
}

/// Four-leaf clover (four circles around center).
#[allow(clippy::too_many_arguments)]
fn clover(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let cr = (w.min(h) / 5).max(3);
    let off = cr + cr / 2;
    for (dx, dy) in &[(-off, 0), (off, 0), (0, -off), (0, off)] {
        ellipse_outline(target, cx + dx, cy + dy, cr, cr, r, g, b, 2);
    }
    // Stem
    thick_line(target, cx, cy + off + cr, cx + cr, cy + h/3, r, g, b);
}

/// Butterfly (two pairs of wing ellipses + body line).
#[allow(clippy::too_many_arguments)]
fn butterfly(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let ww = w / 3;
    let wh = h / 3;
    let off = ww * 3 / 4;
    // Top wings
    ellipse_outline(target, cx - off, cy - wh/2, ww, wh, r, g, b, 2);
    ellipse_outline(target, cx + off, cy - wh/2, ww, wh, r, g, b, 2);
    // Bottom wings (smaller)
    let bww = ww * 2 / 3;
    let bwh = wh * 2 / 3;
    ellipse_outline(target, cx - off*2/3, cy + wh/2, bww, bwh, r, g, b, 2);
    ellipse_outline(target, cx + off*2/3, cy + wh/2, bww, bwh, r, g, b, 2);
    // Body
    thick_line(target, cx, cy - h/3, cx, cy + h/3, r, g, b);
}

/// Simplified maple leaf (star + stem).
#[allow(clippy::too_many_arguments)]
fn maple_leaf(target: &mut Pixmap, cx: i32, cy: i32, w: i32, h: i32, r: u8, g: u8, b: u8) {
    let radius = (w.min(h) / 2 - 2).max(4);
    // 11-point star (many points = leaf-like)
    star_outline(target, cx, cy - h/8, 7, radius, (radius as f32 * 0.45) as i32, r, g, b);
    // Stem
    thick_line(target, cx, cy + radius/2, cx - w/8, cy + h/3, r, g, b);
    thick_line(target, cx, cy + radius/2, cx + w/8, cy + h/3, r, g, b);
}

/// Animated comet streak (circle + fading trail in opposite direction).
#[allow(clippy::too_many_arguments)]
fn comet(
    target: &mut Pixmap,
    cx: i32,
    cy: i32,
    w: i32,
    h: i32,
    r: u8,
    g: u8,
    b: u8,
    elapsed_ms: u64,
    speed: u32,
) {
    // Comet head orbits the center
    let period = 4000u64 / speed.max(1) as u64;
    let angle = (elapsed_ms % period) as f32 / period as f32 * PI * 2.0;
    let orb_r = (w.min(h) / 3).max(4);
    let hx = cx + (angle.cos() * orb_r as f32) as i32;
    let hy = cy + (angle.sin() * orb_r as f32) as i32;

    // Draw head (small circle)
    ellipse_outline(target, hx, hy, 3, 3, r, g, b, 1);

    // Draw tail (fading line toward opposite direction)
    let tail_len = 12;
    let tail_x = cx + (-(angle.cos()) * (orb_r + tail_len) as f32) as i32;
    let tail_y = cy + (-(angle.sin()) * (orb_r + tail_len) as f32) as i32;
    let tw = target.width() as i32;
    let th = target.height() as i32;

    let steps = tail_len;
    for i in 0..steps {
        let t = i as f32 / steps as f32;
        let x = hx + ((tail_x - hx) as f32 * t) as i32;
        let y = hy + ((tail_y - hy) as f32 * t) as i32;
        let alpha = ((1.0 - t) * 200.0) as u8;
        // Manual alpha blend into framebuffer
        if x >= 0 && x < tw && y >= 0 && y < th {
            let idx = ((y * tw + x) * 4) as usize;
            let d = target.data_mut();
            let a = alpha as f32 / 255.0;
            let da = d[idx + 3] as f32 / 255.0;
            let oa = a + da * (1.0 - a);
            if oa > 0.0 {
                d[idx]     = ((r as f32 * a + d[idx]     as f32 * da * (1.0 - a)) / oa) as u8;
                d[idx + 1] = ((g as f32 * a + d[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
                d[idx + 2] = ((b as f32 * a + d[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
                d[idx + 3] = (oa * 255.0) as u8;
            }
        }
    }
}

// ── Colour utilities ──────────────────────────────────────────────────────────

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
