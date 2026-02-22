/// Countdown timer content renderer plugin.
///
/// Counts down to a fixed target datetime and displays the remaining time.
/// Caches the rendered Pixmap and re-renders at most once per second.
///
/// XML example:
/// ```xml
/// <countdownTimer guid="ct-1"
///                 target="2025-12-31 23:59:59"
///                 format="D:H:M:S"
///                 label="New Year in"
///                 color="#00ff88"
///                 urgentSecs="60"
///                 urgentColor="#ff2200"
///                 fontSize="14"/>
/// ```
///
/// Format tokens:
///   D = days remaining (omitted if 0 when not in format string)
///   H = hours (00-23)
///   M = minutes (00-59)
///   S = seconds (00-59)
///
/// When the target is in the past the display shows "00:00:00" in the urgent
/// colour and the label changes to "DONE".
use chrono::{Local, NaiveDateTime};
use std::path::Path;
use tiny_skia::Pixmap;

use crate::program::model::{parse_color, ContentItem, CountdownContent};
use crate::render::plugins::ContentRenderer;

pub struct CountdownRenderer {
    font: rusttype::Font<'static>,
    cached_pixmap: Option<Pixmap>,
    cached_size: (u32, u32),
    cached_guid: String,
    cached_second: u64, // seconds since Unix epoch, to detect second boundary
}

impl CountdownRenderer {
    pub fn new() -> Self {
        let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
        let font = rusttype::Font::try_from_bytes(font_data as &[u8])
            .expect("Failed to load built-in font");
        Self {
            font,
            cached_pixmap: None,
            cached_size: (0, 0),
            cached_guid: String::new(),
            cached_second: u64::MAX,
        }
    }

    fn render_countdown(
        &mut self,
        cd: &CountdownContent,
        target: &mut Pixmap,
        width: u32,
        height: u32,
    ) {
        let now = Local::now();
        let current_epoch = now.timestamp() as u64;

        // ── Cache hit ────────────────────────────────────────────────────────
        if self.cached_guid == cd.guid
            && self.cached_second == current_epoch
            && self.cached_size == (width, height)
            && let Some(ref cached) = self.cached_pixmap {
                target.data_mut().copy_from_slice(cached.data());
                return;
            }

        // ── Compute remaining duration ────────────────────────────────────────
        let (remaining_secs, expired) = parse_target(&cd.target)
            .map(|target_dt| {
                let now_naive = now.naive_local();
                if target_dt > now_naive {
                    let diff = target_dt - now_naive;
                    (diff.num_seconds().max(0) as u64, false)
                } else {
                    (0u64, true)
                }
            })
            .unwrap_or((0, true));

        // ── Choose colour ────────────────────────────────────────────────────
        let (r, g, b) = if expired || (cd.urgent_secs > 0 && remaining_secs <= cd.urgent_secs) {
            parse_color(&cd.urgent_color)
        } else {
            parse_color(&cd.color)
        };
        let (lr, lg, lb) = parse_color(&cd.label_color);

        // ── Format the time string ───────────────────────────────────────────
        let timer_str = format_countdown(remaining_secs, &cd.format, expired);
        let label_str = if expired && !cd.label.is_empty() {
            "DONE".to_string()
        } else {
            cd.label.clone()
        };

        // ── Render ───────────────────────────────────────────────────────────
        let has_label = !label_str.is_empty();
        let timer_size = cd.font_size;
        let label_size = (timer_size * 0.6).max(6.0);

        let scale_t = rusttype::Scale::uniform(timer_size);
        let scale_l = rusttype::Scale::uniform(label_size);
        let vm_t = self.font.v_metrics(scale_t);
        let vm_l = self.font.v_metrics(scale_l);
        let lh_t = (vm_t.ascent - vm_t.descent + vm_t.line_gap).ceil() as i32;
        let lh_l = (vm_l.ascent - vm_l.descent + vm_l.line_gap).ceil() as i32;

        let total_h = lh_t + if has_label { lh_l } else { 0 };
        let start_y = ((height as i32 - total_h) / 2).max(0);

        let tw = target.width() as i32;
        let th = target.height() as i32;

        // Draw label (if present)
        if has_label {
            draw_text_centered(
                &self.font, target.data_mut(), tw, th,
                &label_str, scale_l, vm_l.ascent,
                start_y, width as i32, lr, lg, lb,
            );
        }

        // Draw timer
        let timer_y = start_y + if has_label { lh_l } else { 0 };
        draw_text_centered(
            &self.font, target.data_mut(), tw, th,
            &timer_str, scale_t, vm_t.ascent,
            timer_y, width as i32, r, g, b,
        );

        // ── Store cache ──────────────────────────────────────────────────────
        if let Some(mut pm) = Pixmap::new(width, height) {
            pm.data_mut().copy_from_slice(target.data());
            self.cached_pixmap = Some(pm);
        }
        self.cached_second = current_epoch;
        self.cached_size = (width, height);
        self.cached_guid = cd.guid.clone();
    }
}

impl ContentRenderer for CountdownRenderer {
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
        let cd = match item {
            ContentItem::Countdown(c) => c,
            _ => return false,
        };
        self.render_countdown(cd, target, width, height);
        true
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Parse "YYYY-MM-DD HH:MM:SS" into a NaiveDateTime.
fn parse_target(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s.trim(), "%Y-%m-%d %H:%M:%S").ok()
}

/// Format remaining seconds according to the format string.
fn format_countdown(secs: u64, format: &str, expired: bool) -> String {
    if expired {
        return match format.to_uppercase().as_str() {
            "D:H:M:S" | "DHMS" => "0d 00:00:00".to_string(),
            "H:M:S" | "HMS"    => "00:00:00".to_string(),
            "M:S"   | "MS"     => "00:00".to_string(),
            _                  => "00".to_string(),
        };
    }

    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins  = (secs % 3600) / 60;
    let s     = secs % 60;

    match format.to_uppercase().as_str() {
        "D:H:M:S" | "DHMS" => {
            if days > 0 {
                format!("{}d {:02}:{:02}:{:02}", days, hours, mins, s)
            } else {
                format!("{:02}:{:02}:{:02}", hours, mins, s)
            }
        }
        "H:M:S" | "HMS" => format!("{:02}:{:02}:{:02}", hours + days * 24, mins, s),
        "M:S"   | "MS"  => format!("{:02}:{:02}", mins + (hours + days * 24) * 60, s),
        _                => format!("{}", secs),
    }
}

/// Draw a text string centred horizontally within `[0, area_w)` at `y`.
#[allow(clippy::too_many_arguments)]
fn draw_text_centered(
    font: &rusttype::Font<'static>,
    data: &mut [u8],
    tw: i32,
    th: i32,
    text: &str,
    scale: rusttype::Scale,
    ascent: f32,
    y: i32,
    area_w: i32,
    r: u8,
    g: u8,
    b: u8,
) {
    let glyphs: Vec<_> = font
        .layout(text, scale, rusttype::point(0.0, ascent))
        .collect();

    let text_w = glyphs
        .last()
        .and_then(|g| g.pixel_bounding_box().map(|bb| bb.max.x))
        .unwrap_or(0);
    let x_off = ((area_w - text_w) / 2).max(0);

    for glyph in &glyphs {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, v| {
                let px = x_off + bb.min.x + gx as i32;
                let py = y + bb.min.y + gy as i32;
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
