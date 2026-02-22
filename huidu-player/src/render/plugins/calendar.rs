/// Monthly calendar grid content renderer plugin.
///
/// Renders the current month as a 7-column grid (Sun–Sat).
/// Today's date cell is highlighted.  The header shows "Month YYYY".
/// Weekday abbreviations appear in a subdued colour above the grid.
///
/// Layout (all in pixels scaled to the area):
///   Row 0: Month + Year header (centred)
///   Row 1: Sun Mon Tue Wed Thu Fri Sat
///   Rows 2-7: day numbers (up to 6 weeks)
use std::path::Path;
use chrono::{Datelike, Local, NaiveDate};
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, CalendarContent, ContentItem};
use crate::render::plugins::ContentRenderer;

pub struct CalendarRenderer {
    font: rusttype::Font<'static>,
}

impl CalendarRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self { font }
    }

    fn render_calendar(&self, cal: &CalendarContent, target: &mut Pixmap) {
        let now = Local::now().naive_local();
        let year = now.year();
        let month = now.month();
        let today_day = now.day();

        let tw = target.width() as i32;
        let th = target.height() as i32;

        // 8 rows total: 1 header + 1 weekday + up to 6 week rows
        let row_h = th / 8;
        let col_w = tw / 7;

        let font_size = cal.font_size;
        let scale = rusttype::Scale::uniform(font_size);
        let v_metrics = self.font.v_metrics(scale);
        let ascent = v_metrics.ascent;

        let (col_r, col_g, col_b) = parse_color(&cal.color);
        let (tod_r, tod_g, tod_b) = parse_color(&cal.today_color);
        let (hdr_r, hdr_g, hdr_b) = parse_color(&cal.header_color);
        let (wkd_r, wkd_g, wkd_b) = parse_color(&cal.weekday_color);

        // -- Header: "January 2025" --
        let month_name = [
            "January", "February", "March", "April", "May", "June",
            "July", "August", "September", "October", "November", "December",
        ][(month - 1) as usize];
        let header = format!("{} {}", month_name, year);
        self.draw_text_centered(
            target,
            &header,
            scale,
            ascent,
            0,
            row_h / 2,
            tw,
            hdr_r, hdr_g, hdr_b,
        );

        // -- Weekday row --
        let weekdays = ["Su", "Mo", "Tu", "We", "Th", "Fr", "Sa"];
        for (i, wd) in weekdays.iter().enumerate() {
            let x = i as i32 * col_w + col_w / 2;
            self.draw_text_centered(
                target,
                wd,
                scale,
                ascent,
                x - col_w / 2,
                row_h + row_h / 2,
                col_w,
                wkd_r, wkd_g, wkd_b,
            );
        }

        // -- Day grid --
        // Find the day-of-week for the 1st of this month (0=Sun, 6=Sat)
        let first = match NaiveDate::from_ymd_opt(year, month, 1) {
            Some(d) => d,
            None => return, // shouldn't happen: year/month come from Local::now()
        };
        // chrono: Mon=1 … Sun=7; we want Sun=0
        let start_col = (first.weekday().num_days_from_sunday()) as i32;

        // Days in the month
        let days_in_month = days_in_month(year, month);

        for day in 1..=days_in_month {
            let cell = start_col + (day as i32 - 1);
            let row = cell / 7;
            let col = cell % 7;

            let x = col * col_w;
            let y = (row + 2) * row_h; // +2: skip header + weekday rows

            let (r, g, b) = if day == today_day {
                // Highlight background for today
                fill_rect(target, x, y, col_w, row_h, 40, 40, 0, 180);
                (tod_r, tod_g, tod_b)
            } else {
                (col_r, col_g, col_b)
            };

            self.draw_text_centered(
                target,
                &day.to_string(),
                scale,
                ascent,
                x,
                y + row_h / 2,
                col_w,
                r, g, b,
            );
        }
    }

    /// Draw text centred horizontally within `[x, x+width)` at vertical mid `cy`.
    #[allow(clippy::too_many_arguments)]
    fn draw_text_centered(
        &self,
        target: &mut Pixmap,
        text: &str,
        scale: rusttype::Scale,
        ascent: f32,
        x: i32,
        cy: i32,
        width: i32,
        r: u8,
        g: u8,
        b: u8,
    ) {
        let glyphs: Vec<_> = self
            .font
            .layout(text, scale, rusttype::point(0.0, ascent))
            .collect();

        let text_w = glyphs
            .last()
            .and_then(|g| g.pixel_bounding_box())
            .map(|bb| bb.max.x)
            .unwrap_or(0);

        let ox = x + (width - text_w) / 2;
        let oy = cy - (ascent / 2.0) as i32;

        let tw = target.width() as i32;
        let th = target.height() as i32;
        let data = target.data_mut();

        for glyph in &glyphs {
            if let Some(bb) = glyph.pixel_bounding_box() {
                glyph.draw(|gx, gy, v| {
                    let px = ox + bb.min.x + gx as i32;
                    let py = oy + bb.min.y + gy as i32;
                    if px < 0 || px >= tw || py < 0 || py >= th {
                        return;
                    }
                    let alpha = (v * 255.0) as u8;
                    if alpha == 0 {
                        return;
                    }
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
}

impl ContentRenderer for CalendarRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        _width: u32,
        _height: u32,
        _elapsed_ms: u64,
        _program_dir: &Path,
    ) -> bool {
        let cal = match item {
            ContentItem::Calendar(c) => c,
            _ => return false,
        };
        self.render_calendar(cal, target);
        true
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn days_in_month(year: i32, month: u32) -> u32 {
    let next_month = if month == 12 { 1 } else { month + 1 };
    let next_year = if month == 12 { year + 1 } else { year };
    match (
        NaiveDate::from_ymd_opt(next_year, next_month, 1),
        NaiveDate::from_ymd_opt(year, month, 1),
    ) {
        (Some(next), Some(cur)) => next.signed_duration_since(cur).num_days() as u32,
        _ => 30, // fallback; only reached if caller passes invalid year/month
    }
}

/// Fill a rectangle with a semi-transparent colour (additive blend into target).
#[allow(clippy::too_many_arguments)]
fn fill_rect(target: &mut Pixmap, x: i32, y: i32, w: i32, h: i32, r: u8, g: u8, b: u8, a: u8) {
    let tw = target.width() as i32;
    let th = target.height() as i32;
    let data = target.data_mut();
    let af = a as f32 / 255.0;
    for dy in 0..h {
        for dx in 0..w {
            let px = x + dx;
            let py = y + dy;
            if px < 0 || px >= tw || py < 0 || py >= th {
                continue;
            }
            let idx = ((py * tw + px) * 4) as usize;
            let da = data[idx + 3] as f32 / 255.0;
            let oa = af + da * (1.0 - af);
            if oa > 0.0 {
                data[idx]     = ((r as f32 * af + data[idx]     as f32 * da * (1.0 - af)) / oa) as u8;
                data[idx + 1] = ((g as f32 * af + data[idx + 1] as f32 * da * (1.0 - af)) / oa) as u8;
                data[idx + 2] = ((b as f32 * af + data[idx + 2] as f32 * da * (1.0 - af)) / oa) as u8;
                data[idx + 3] = (oa * 255.0) as u8;
            }
        }
    }
}
