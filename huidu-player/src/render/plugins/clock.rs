/// Clock content renderer plugin.
///
/// Renders digital clock with date/time/week/title/lunar fields.
///
/// ## Per-second Pixmap cache
/// At 60 fps the clock display is identical for all 60 frames within each
/// second.  The renderer caches the fully-rendered Pixmap and copies it on
/// cache hits, avoiding 59 out of 60 font-layout + rasterisation rounds.
use chrono::{Local, Timelike};
use std::path::Path;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ClockContent, ContentItem};
use crate::render::plugins::ContentRenderer;

pub struct ClockRenderer {
    font: rusttype::Font<'static>,
    /// Cached fully-rendered clock surface.
    cached_pixmap: Option<Pixmap>,
    /// Area dimensions that produced the cache.
    cached_size: (u32, u32),
    /// GUID of the clock content item that produced the cache.
    cached_guid: String,
    /// Wall-clock second (0-59) at which the cache was built.
    cached_second: u8,
}

impl ClockRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self {
            font,
            cached_pixmap: None,
            cached_size: (0, 0),
            cached_guid: String::new(),
            cached_second: 255, // force miss on first frame
        }
    }

    fn render_clock(
        &mut self,
        clock: &ClockContent,
        target: &mut Pixmap,
        width: u32,
        height: u32,
    ) {
        let now = Local::now();
        let current_second = now.second() as u8;

        // ── Cache hit: same clock, same second, same area size ───────────────
        if self.cached_guid == clock.guid
            && self.cached_second == current_second
            && self.cached_size == (width, height)
        {
            if let Some(ref cached) = self.cached_pixmap {
                // Fast memcpy — skips all font layout and rasterisation.
                target.data_mut().copy_from_slice(cached.data());
                return;
            }
        }

        // ── Cache miss: render fresh into `target` ───────────────────────────
        render_clock_into(&self.font, clock, target, width, height, &now);

        // ── Store rendered frame in cache ────────────────────────────────────
        let mut pm = match Pixmap::new(width, height) {
            Some(p) => p,
            None => return,
        };
        pm.data_mut().copy_from_slice(target.data());
        self.cached_pixmap = Some(pm);
        self.cached_second = current_second;
        self.cached_size = (width, height);
        self.cached_guid = clock.guid.clone();
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

// ── Rendering logic (called at most once per second) ─────────────────────────

fn render_clock_into(
    font: &rusttype::Font<'static>,
    clock: &ClockContent,
    target: &mut Pixmap,
    width: u32,
    height: u32,
    now: &chrono::DateTime<Local>,
) {
    let mut lines: Vec<(String, (u8, u8, u8))> = Vec::new();

    if let Some(ref f) = clock.title {
        if f.display && !f.value.is_empty() {
            lines.push((f.value.clone(), parse_color(&f.color)));
        }
    }
    if let Some(ref f) = clock.date {
        if f.display {
            let s = match f.format.as_str() {
                "2" => now.format("%m/%d/%Y").to_string(),
                "3" => now.format("%d/%m/%Y").to_string(),
                "4" => now.format("%b %d, %Y").to_string(),
                "5" => now.format("%d %b, %Y").to_string(),
                _   => now.format("%Y/%m/%d").to_string(),
            };
            lines.push((s, parse_color(&f.color)));
        }
    }
    if let Some(ref f) = clock.week {
        if f.display {
            let s = match f.format.as_str() {
                "2" => now.format("%A").to_string(),
                "3" => now.format("%a").to_string(),
                _   => now.format("%A").to_string(),
            };
            lines.push((s, parse_color(&f.color)));
        }
    }
    if let Some(ref f) = clock.time {
        if f.display {
            let s = match f.format.as_str() {
                "2" => now.format("%H:%M").to_string(),
                "3" => now.format("%I:%M:%S %p").to_string(),
                "4" => now.format("%I:%M %p").to_string(),
                _   => now.format("%H:%M:%S").to_string(),
            };
            lines.push((s, parse_color(&f.color)));
        }
    }
    if let Some(ref f) = clock.lunar_calendar {
        if f.display {
            lines.push((compute_lunar_date(now), parse_color(&f.color)));
        }
    }
    if lines.is_empty() {
        lines.push((now.format("%H:%M:%S").to_string(), (255, 255, 255)));
    }

    let font_size = (height as f32 / lines.len() as f32).min(height as f32 * 0.8);
    let scale = rusttype::Scale::uniform(font_size);
    let v_metrics = font.v_metrics(scale);
    let line_height = (v_metrics.ascent - v_metrics.descent + v_metrics.line_gap).ceil();
    let total_height = line_height * lines.len() as f32;
    let start_y = ((height as f32 - total_height) / 2.0).max(0.0);

    let tw = target.width() as i32;
    let th = target.height() as i32;
    let data = target.data_mut();

    for (i, (text, (r, g, b))) in lines.iter().enumerate() {
        let y_offset = start_y + i as f32 * line_height;
        let glyphs: Vec<_> = font
            .layout(text, scale, rusttype::point(0.0, y_offset + v_metrics.ascent))
            .collect();

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
                            let da = data[idx + 3] as f32 / 255.0;
                            let oa = a + da * (1.0 - a);
                            if oa > 0.0 {
                                data[idx]     = ((*r as f32 * a + data[idx]     as f32 * da * (1.0 - a)) / oa) as u8;
                                data[idx + 1] = ((*g as f32 * a + data[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
                                data[idx + 2] = ((*b as f32 * a + data[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
                                data[idx + 3] = (oa * 255.0) as u8;
                            }
                        }
                    }
                });
            }
        }
    }
}

// ── Lunar date approximation ──────────────────────────────────────────────────

fn compute_lunar_date(now: &chrono::DateTime<Local>) -> String {
    use chrono::NaiveDate;
    const CNY: &[(i32, u32, u32)] = &[
        (2020,1,25),(2021,2,12),(2022,2,1),(2023,1,22),
        (2024,2,10),(2025,1,29),(2026,2,17),(2027,2,6),
        (2028,1,26),(2029,2,13),(2030,2,3),(2031,1,23),
        (2032,2,11),(2033,1,31),(2034,2,19),(2035,2,8),
        (2036,1,28),(2037,2,15),(2038,2,4),(2039,1,24),
        (2040,2,12),(2041,1,31),(2042,2,22),(2043,2,10),
        (2044,1,30),(2045,2,17),
    ];
    let today = now.date_naive();
    let mut cny = NaiveDate::from_ymd_opt(2020, 1, 25).unwrap();
    for &(y, m, d) in CNY {
        if let Some(date) = NaiveDate::from_ymd_opt(y, m, d) {
            if date <= today { cny = date; }
        }
    }
    let days = (today - cny).num_days().max(0) as u32;
    format!("L:{}/{}", ((days / 30) + 1).min(13), (days % 30) + 1)
}
