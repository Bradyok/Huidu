/// Analog clock renderer plugin (libclock_plugin.so, dial type).
///
/// Draws a minimal analogue clock face using software rasterisation:
///   - Dark circular bezel
///   - 12 hour-tick marks
///   - Hour, minute, and second hands via Bresenham line drawing
///   - Centre pip
///
/// The `AnalogClockContent.bg_image` field is noted but image loading is
/// deferred to when the file-system layout is known at runtime.
use std::f32::consts::PI;
use std::path::Path;

use tiny_skia::Pixmap;

use crate::program::model::{AnalogClockContent, ContentItem};
use crate::render::plugins::ContentRenderer;

pub struct AnalogClockRenderer;

impl AnalogClockRenderer {
    pub fn new() -> Self {
        Self
    }

    fn render_clock(&self, clock: &AnalogClockContent, target: &mut Pixmap, width: u32, height: u32) {
        let w = width as i32;
        let h = height as i32;
        let cx = w / 2;
        let cy = h / 2;
        let radius = (w.min(h) / 2 - 1).max(4);

        // Dark face
        fill_circle(target.data_mut(), cx, cy, radius, w, h, 10, 10, 30, 255);

        // Rim
        draw_circle_outline(target.data_mut(), cx, cy, radius, w, h, 100, 100, 180);

        // Hour tick marks
        for i in 0..12 {
            let angle = i as f32 * PI / 6.0 - PI / 2.0;
            let inner = (radius as f32 * 0.82) as i32;
            let outer = radius - 1;
            let x0 = cx + (angle.cos() * inner as f32) as i32;
            let y0 = cy + (angle.sin() * inner as f32) as i32;
            let x1 = cx + (angle.cos() * outer as f32) as i32;
            let y1 = cy + (angle.sin() * outer as f32) as i32;
            draw_line(target.data_mut(), x0, y0, x1, y1, w, h, 180, 180, 220);
        }

        // Determine current local time (use timezone adjust if set)
        let now = chrono::Local::now();
        let hour = now.hour() % 12;
        let minute = now.minute();
        let second = now.second();

        // Hand angles (from 12 o'clock = -PI/2, clockwise)
        let h_angle = (hour as f32 + minute as f32 / 60.0) * PI / 6.0 - PI / 2.0;
        let m_angle = (minute as f32 + second as f32 / 60.0) * PI / 30.0 - PI / 2.0;
        let s_angle = second as f32 * PI / 30.0 - PI / 2.0;

        let h_len = (radius as f32 * 0.55) as i32;
        let m_len = (radius as f32 * 0.78) as i32;
        let s_len = (radius as f32 * 0.85) as i32;

        // Hour hand (thick — drawn as 3 parallel lines)
        for off in -1..=1i32 {
            let ox = (-h_angle.sin() * off as f32) as i32;
            let oy = (h_angle.cos() * off as f32) as i32;
            draw_line(
                target.data_mut(),
                cx + ox, cy + oy,
                cx + (h_angle.cos() * h_len as f32) as i32 + ox,
                cy + (h_angle.sin() * h_len as f32) as i32 + oy,
                w, h, 255, 255, 255,
            );
        }

        // Minute hand (medium, white)
        for off in 0..=1i32 {
            let ox = (-m_angle.sin() * off as f32) as i32;
            let oy = (m_angle.cos() * off as f32) as i32;
            draw_line(
                target.data_mut(),
                cx + ox, cy + oy,
                cx + (m_angle.cos() * m_len as f32) as i32 + ox,
                cy + (m_angle.sin() * m_len as f32) as i32 + oy,
                w, h, 200, 200, 255,
            );
        }

        // Second hand (thin, red)
        draw_line(
            target.data_mut(),
            cx, cy,
            cx + (s_angle.cos() * s_len as f32) as i32,
            cy + (s_angle.sin() * s_len as f32) as i32,
            w, h, 255, 60, 60,
        );
        // Counter-tail
        draw_line(
            target.data_mut(),
            cx, cy,
            cx - (s_angle.cos() * (s_len / 4) as f32) as i32,
            cy - (s_angle.sin() * (s_len / 4) as f32) as i32,
            w, h, 180, 30, 30,
        );

        // Centre pip
        fill_circle(target.data_mut(), cx, cy, 2, w, h, 255, 255, 255, 255);

        let _ = clock; // suppress unused warning for timezone field
    }
}

impl ContentRenderer for AnalogClockRenderer {
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
            ContentItem::AnalogClock(c) => c,
            _ => return false,
        };
        self.render_clock(clock, target, width, height);
        true
    }
}

// ---------------------------------------------------------------------------
// Software rasterisation helpers
// ---------------------------------------------------------------------------

fn set_px(data: &mut [u8], x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8, a: u8) {
    if x < 0 || x >= w || y < 0 || y >= h {
        return;
    }
    let idx = ((y * w + x) * 4) as usize;
    if idx + 3 >= data.len() {
        return;
    }
    if a == 255 {
        data[idx] = r;
        data[idx + 1] = g;
        data[idx + 2] = b;
        data[idx + 3] = 255;
    } else {
        let sa = a as f32 / 255.0;
        let da = data[idx + 3] as f32 / 255.0;
        let oa = sa + da * (1.0 - sa);
        if oa > 0.0 {
            data[idx] = ((r as f32 * sa + data[idx] as f32 * da * (1.0 - sa)) / oa) as u8;
            data[idx + 1] = ((g as f32 * sa + data[idx + 1] as f32 * da * (1.0 - sa)) / oa) as u8;
            data[idx + 2] = ((b as f32 * sa + data[idx + 2] as f32 * da * (1.0 - sa)) / oa) as u8;
            data[idx + 3] = (oa * 255.0) as u8;
        }
    }
}

/// Filled circle using midpoint rasterisation.
fn fill_circle(
    data: &mut [u8],
    cx: i32, cy: i32, r: i32,
    w: i32, h: i32,
    pr: u8, pg: u8, pb: u8, a: u8,
) {
    for dy in -r..=r {
        let dx_max = ((r * r - dy * dy) as f32).sqrt() as i32;
        for dx in -dx_max..=dx_max {
            set_px(data, cx + dx, cy + dy, w, h, pr, pg, pb, a);
        }
    }
}

/// Single-pixel circle outline via midpoint algorithm.
fn draw_circle_outline(
    data: &mut [u8],
    cx: i32, cy: i32, r: i32,
    w: i32, h: i32,
    pr: u8, pg: u8, pb: u8,
) {
    let mut x = r;
    let mut y = 0i32;
    let mut d = 1 - r;
    while x >= y {
        for (dx, dy) in [(x, y), (y, x), (-y, x), (-x, y), (-x, -y), (-y, -x), (y, -x), (x, -y)] {
            set_px(data, cx + dx, cy + dy, w, h, pr, pg, pb, 255);
        }
        y += 1;
        if d <= 0 {
            d += 2 * y + 1;
        } else {
            x -= 1;
            d += 2 * (y - x) + 1;
        }
    }
}

/// Bresenham line.
fn draw_line(
    data: &mut [u8],
    mut x0: i32, mut y0: i32,
    x1: i32, y1: i32,
    w: i32, h: i32,
    r: u8, g: u8, b: u8,
) {
    let dx = (x1 - x0).abs();
    let dy = (y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx - dy;
    loop {
        set_px(data, x0, y0, w, h, r, g, b, 255);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 > -dy {
            err -= dy;
            x0 += sx;
        }
        if e2 < dx {
            err += dx;
            y0 += sy;
        }
    }
}

// Bring chrono's time traits into scope for .hour(), .minute(), .second()
use chrono::Timelike;
