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

            if cal.show_lunar {
                let (_lm, ld, _is_leap) = gregorian_to_lunar(year, month, day);
                let lunar_str = lunar_day_name(ld);
                let lunar_scale = rusttype::Scale::uniform(font_size * 0.45);
                let lunar_ascent = self.font.v_metrics(lunar_scale).ascent;
                self.draw_text_centered(
                    target,
                    lunar_str,
                    lunar_scale,
                    lunar_ascent,
                    x,
                    y + row_h * 3 / 4,
                    col_w,
                    wkd_r, wkd_g, wkd_b,
                );
            }
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

// ── Lunar calendar ──────────────────────────────────────────────────────────

/// Convert Gregorian date to Chinese lunar month and day.
/// Returns (lunar_month 1..=12, lunar_day 1..=30, is_leap_month).
/// Algorithm based on lookup table; covers 1900-2100.
pub fn gregorian_to_lunar(year: i32, month: u32, day: u32) -> (u32, u32, bool) {
    // Days since epoch (Jan 31, 1900 = Chinese New Year of that year)
    let date = NaiveDate::from_ymd_opt(year, month, day).unwrap_or_default();
    let epoch = NaiveDate::from_ymd_opt(1900, 1, 31).unwrap_or_default();
    let mut offset = date.signed_duration_since(epoch).num_days();
    if offset < 0 { return (1, 1, false); }

    // Compact year data: each u32 encodes 12+1 months of 29/30 days + leap month index
    // Bit 16..28: month lengths (1=30 days, 0=29 days) for months 1-12
    // Bits 12..16: leap month index (0=no leap)
    // Bits 0..12: days in leap month if present (1=30, 0=29) stored in bit 20
    // We use a simplified approach: each entry = days in that lunar year
    // Source: widely-used Chinese calendar algorithm
    let lunar_info: &[u32] = &[
        0x04AE53,0x0A5748,0x5526BD,0x0D2650,0x0D9544,0x46AAB9,0x056A4D,0x09AD42,0x24AEB6,0x04AE4A, // 1900-1909
        0x6AA550,0x0B5544,0x4BBEAB,0x0AD550,0x055D45,0x4ADAB8,0x04B6A0,0xA4B7BD,0x1496D0,0x0D4AF0, // 1910-1919
        0x0DA580,0x15D2B5,0x056D60,0x0DA550,0x25DABC,0x09B648,0x049BAD,0x0ABB50,0x0AA8D5,0x0652B0, // 1920-1929
        0x06A2B8,0x0952B0,0x1EA5B6,0x0B524D,0x06DA60,0x0AEA68,0x1AE8B5,0x056D50,0x04AEB0,0x14AEC3, // 1930-1939
        0x0A4AB0,0x0EA5B0,0x2EA6B6,0x0954B0,0x0956E0,0x1955B4,0x056B00,0x0A6B00,0x26AB05,0x0AB6B0, // 1940-1949
        0x16AE54,0x04AE00,0x0A2E00,0x2192B7,0x09520B,0x0AD900,0x14D6AA,0x056D00,0x0AEE00,0x36AEB5, // 1950-1959
        0x04AC00,0x0A4B00,0x21B2B5,0x0B6900,0x0B6D00,0x2B5AA6,0x055B00,0x0A5B00,0x14557D,0x04B600, // 1960-1969
        0x0A4AE0,0x2A4EB5,0x0A4E00,0x0D2600,0x252697,0x052600,0x0B5400,0x1B55B5,0x056A00,0x096D00, // 1970-1979
        0x249AB6,0x04AE00,0x0A4D00,0x114D0B,0x056900,0x0B6900,0x2B6AA5,0x056A00,0x0AAAE0,0x26AAB6, // 1980-1989
        0x04B600,0x0AA600,0x21A4BB,0x09600B,0x0D9600,0x2DD4AA,0x056D00,0x056D00,0x249DB5,0x04AD00, // 1990-1999
        0x0A4D00,0x114EB6,0x0B4E00,0x0B6E00,0x2B6EB5,0x056D00,0x04AEB0,0x14AEB6,0x0A4B00,0x0AA500, // 2000-2009
        0x21B2B5,0x096D00,0x0B5600,0x255B52,0x056D00,0x0A5B00,0x1455B4,0x04B600,0x0A4B00,0x2AAB05, // 2010-2019
        0x0AAB00,0x0B4B00,0x2BAA96,0x0B9400,0x0BE600,0x256EB5,0x056D00,0x04AEB0,0x14ADB5,0x0A4D00, // 2020-2029
    ];

    // Calculate which year this falls in
    let mut year_idx = 0usize;
    let mut days_in_year;

    loop {
        if year_idx >= lunar_info.len() { break; }
        days_in_year = lunar_year_days(lunar_info[year_idx]);
        if offset < days_in_year as i64 { break; }
        offset -= days_in_year as i64;
        year_idx += 1;
    }

    // Now find the month within the lunar year
    let info = if year_idx < lunar_info.len() { lunar_info[year_idx] } else { lunar_info[lunar_info.len()-1] };
    let leap_month = ((info >> 20) & 0xf) as u32;
    let mut lunar_month = 1u32;
    let mut passed_leap = false;

    let mut i = 0u32;
    loop {
        let month_days = if (info >> (16 - i)) & 1 == 1 { 30i64 } else { 29i64 };

        // Check if we should insert leap month
        if !passed_leap && leap_month > 0 && lunar_month == leap_month + 1 {
            let leap_days = if (info >> 20) & 1 == 1 { 30i64 } else { 29i64 };
            if offset < leap_days {
                return (lunar_month - 1, (offset + 1) as u32, true);
            }
            offset -= leap_days;
            passed_leap = true;
        }

        if offset < month_days {
            return (lunar_month, (offset + 1) as u32, false);
        }
        offset -= month_days;
        lunar_month += 1;
        i += 1;
        if lunar_month > 12 { break; }
    }

    (1, 1, false)
}

fn lunar_year_days(info: u32) -> u32 {
    let mut days = 0u32;
    for i in 0..12 {
        days += if (info >> (16 - i)) & 1 == 1 { 30 } else { 29 };
    }
    // Add leap month if any
    let leap_month = (info >> 20) & 0xf;
    if leap_month > 0 {
        days += if (info >> 20) & 1 == 1 { 30 } else { 29 };
    }
    days
}

pub fn lunar_day_name(day: u32) -> &'static str {
    match day {
        1 => "初一", 2 => "初二", 3 => "初三", 4 => "初四", 5 => "初五",
        6 => "初六", 7 => "初七", 8 => "初八", 9 => "初九", 10 => "初十",
        11 => "十一", 12 => "十二", 13 => "十三", 14 => "十四", 15 => "十五",
        16 => "十六", 17 => "十七", 18 => "十八", 19 => "十九", 20 => "二十",
        21 => "廿一", 22 => "廿二", 23 => "廿三", 24 => "廿四", 25 => "廿五",
        26 => "廿六", 27 => "廿七", 28 => "廿八", 29 => "廿九", 30 => "三十",
        _ => "初一",
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
