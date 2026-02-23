//! HDPlayer GUI — Rust/egui clone of the Huidu HDPlayer desktop application.
//!
//! Build:  cargo build --features gui --bin hdplayer-gui
//! Run:    ./target/debug/hdplayer-gui

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use hdplayer::{Client, DeviceDetails, DeviceInfo, Discovery, ProgramInfo};

// ── CONSTANTS ─────────────────────────────────────────────────────────────────

const EFFECT_NAMES: &[&str] = &[
    "None", "Random", "Blinds H", "Blinds V", "Checkers", "Spiral",
    "Sweep", "Cross", "Diamond", "Rotate", "Flash", "Wipe H", "Wipe V",
    "Wipe D1", "Wipe D2", "Shutter H", "Shutter V", "Fade",
    "Push L", "Push R", "Push U", "Scroll L→", "Scroll R←", "Scroll U↑", "Scroll D↓",
    "Zoom In", "Zoom Out", "Mosaic", "Fire", "Stars",
];

const BORDER_NAMES: &[&str] = &[
    "None", "Solid White", "Solid Red", "Solid Green", "Solid Blue",
    "Solid Yellow", "Chase White", "Chase Color", "Rainbow", "Breathing",
    "Sparkle", "Comet", "Color Shift", "Twinkle",
];

const NEON_NAMES: &[&str] = &[
    "Arrow Up", "Arrow Down", "Arrow Left", "Arrow Right",
    "Square", "Circle", "Heart", "Diamond", "Star4", "Star5",
    "Star6", "Lightning", "Crown", "Flower", "Tree", "Snowflake",
    "Moon", "Sun", "Cloud", "Drop", "Fire", "Bell", "Music",
    "Peace", "Cross", "Infinity", "Wifi", "Camera", "Phone",
    "Car", "Plane", "Bicycle", "Smile", "Thumbs Up", "Comet",
];

// ── DATA MODEL ────────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct TextItem {
    pub guid: String,
    pub text: String,
    pub single_line: bool,
    pub font_name: String,
    pub font_size: u32,
    pub color: [u8; 3],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub align: u8,   // 0=left 1=center 2=right
    pub valign: u8,  // 0=top 1=middle 2=bottom
    pub effect_in: u32,
    pub effect_out: u32,
    pub effect_in_speed: u32,
    pub effect_out_speed: u32,
    pub duration_tenths: u32,
    pub scroll_dir: u8,   // 0=none 1=left 2=right 3=up 4=down
    pub scroll_speed: u32,
    pub word_wrap: bool,
    pub background: Option<[u8; 3]>,
}

#[derive(Clone, Debug)]
pub struct ImageItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub fit: u8, // 0=stretch 1=fill 2=center 3=fit
}

#[derive(Clone, Debug)]
pub struct VideoItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub keep_aspect: bool,
}

#[derive(Clone, Debug)]
pub struct ClockItem {
    pub guid: String,
    pub is_analog: bool,
    pub timezone: String,
    pub show_title: bool,
    pub title_text: String,
    pub title_color: [u8; 3],
    pub show_date: bool,
    pub date_format: u8,
    pub date_color: [u8; 3],
    pub show_week: bool,
    pub week_color: [u8; 3],
    pub show_time: bool,
    pub time_format: u8,
    pub time_color: [u8; 3],
    pub show_lunar: bool,
    pub lunar_color: [u8; 3],
    pub font_size: u32,
    pub hand_color: [u8; 3],
    pub second_color: [u8; 3],
    pub dial_color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct NeonItem {
    pub guid: String,
    pub index: u32, // 0-based index into NEON_NAMES
    pub color: [u8; 3],
    pub speed: u32,
    pub rainbow: bool,
}

#[derive(Clone, Debug)]
pub struct QrCodeItem {
    pub guid: String,
    pub data: String,
    pub fg: [u8; 3],
    pub bg: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct CalendarItem {
    pub guid: String,
    pub color: [u8; 3],
    pub today_color: [u8; 3],
    pub header_color: [u8; 3],
    pub font_size: u32,
}

#[derive(Clone, Debug)]
pub struct CountdownItem {
    pub guid: String,
    pub target: String,
    pub label: String,
    pub color: [u8; 3],
    pub font_size: u32,
    pub format: String,
}

#[derive(Clone, Debug)]
pub struct TableItem {
    pub guid: String,
    pub cols: usize,
    pub rows: Vec<Vec<String>>,
    pub header_row: bool,
    pub text_color: [u8; 3],
    pub header_bg: [u8; 3],
    pub font_size: u32,
}

#[derive(Clone, Debug)]
pub struct LiveStreamItem {
    pub guid: String,
    pub url: String,
    pub reconnect: bool,
    pub font_size: u32,
    pub color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct ModbusItem {
    pub guid: String,
    pub host: String,
    pub port: u16,
    pub slave: u8,
    pub register: u16,
    pub register_type: String,  // "holding" or "input"
    pub format: String,
    pub scale_str: String,      // e.g. "1.0" — stored as string to avoid float issues
    pub update_interval: u32,
    pub scroll_speed: u32,
    pub font_size: u32,
    pub color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct SensorItem {
    pub guid: String,
    pub sensor_type: String,   // "ds18b20" | "cpu_temp" | "dht22" | "generic_file"
    pub device: String,        // sysfs path
    pub format: String,        // e.g. "{value}°C"
    pub update_interval: u32,
    pub scroll_speed: u32,
    pub font_size: u32,
    pub color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct Text3DItem {
    pub guid: String,
    pub text: String,
    pub color: [u8; 3],
    pub depth_color: [u8; 3],
    pub font_size: f32,
    pub rotate_speed: f32,
    pub effect_3d: String,   // "rotate_y" | "rotate_x" | "pulse" | "wave"
}

#[derive(Clone, Debug)]
pub struct DocumentItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub page_duration: u32,
    pub fit: u8,             // 0=stretch 1=fill 2=center
    pub loop_pages: bool,
}

#[derive(Clone, Debug)]
pub enum ContentItem {
    Text(TextItem),
    Image(ImageItem),
    Video(VideoItem),
    Clock(ClockItem),
    Neon(NeonItem),
    QrCode(QrCodeItem),
    Calendar(CalendarItem),
    Countdown(CountdownItem),
    Table(TableItem),
    LiveStream(LiveStreamItem),
    Modbus(ModbusItem),
    Sensor(SensorItem),
    Text3D(Text3DItem),
    Document(DocumentItem),
}

impl ContentItem {
    pub fn guid(&self) -> &str {
        match self {
            Self::Text(t) => &t.guid, Self::Image(i) => &i.guid,
            Self::Video(v) => &v.guid, Self::Clock(c) => &c.guid,
            Self::Neon(n) => &n.guid, Self::QrCode(q) => &q.guid,
            Self::Calendar(c) => &c.guid, Self::Countdown(c) => &c.guid,
            Self::Table(t) => &t.guid,
            Self::LiveStream(ls) => &ls.guid, Self::Modbus(mb) => &mb.guid,
            Self::Sensor(sn) => &sn.guid, Self::Text3D(t3) => &t3.guid,
            Self::Document(dc) => &dc.guid,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Text(t) => if t.single_line { "Single-Line Text" } else { "Multi-Line Text" },
            Self::Image(_) => "Image", Self::Video(_) => "Video",
            Self::Clock(c) => if c.is_analog { "Analog Clock" } else { "Digital Clock" },
            Self::Neon(_) => "Neon Shape", Self::QrCode(_) => "QR Code",
            Self::Calendar(_) => "Calendar", Self::Countdown(_) => "Countdown",
            Self::Table(_) => "Table",
            Self::LiveStream(_) => "Live Stream", Self::Modbus(_) => "Modbus Data",
            Self::Sensor(_) => "Sensor", Self::Text3D(_) => "3D Text",
            Self::Document(_) => "Document",
        }
    }
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Text(t) => if t.single_line { "T→" } else { "T¶" },
            Self::Image(_) => "🖼", Self::Video(_) => "▶",
            Self::Clock(c) => if c.is_analog { "🕐" } else { "🕐" },
            Self::Neon(_) => "✨", Self::QrCode(_) => "▦",
            Self::Calendar(_) => "📅", Self::Countdown(_) => "⏳",
            Self::Table(_) => "⊞",
            Self::LiveStream(_) => "📡", Self::Modbus(_) => "⚙",
            Self::Sensor(_) => "🌡", Self::Text3D(_) => "3D",
            Self::Document(_) => "📄",
        }
    }
    pub fn new_text(guid: String, single_line: bool) -> Self {
        Self::Text(TextItem {
            guid, text: "Hello!".into(), single_line,
            font_name: String::new(), font_size: 14,
            color: [255, 255, 0], bold: false, italic: false, underline: false,
            align: 1, valign: 1,
            effect_in: 17, effect_out: 17, effect_in_speed: 3, effect_out_speed: 3,
            duration_tenths: 50, scroll_dir: 0, scroll_speed: 40,
            word_wrap: false, background: None,
        })
    }
    pub fn new_clock(guid: String) -> Self {
        Self::Clock(ClockItem {
            guid, is_analog: false, timezone: "+00:00".into(),
            show_title: false, title_text: String::new(), title_color: [255, 170, 0],
            show_date: true, date_format: 1, date_color: [0, 255, 136],
            show_week: true, week_color: [136, 255, 255],
            show_time: true, time_format: 1, time_color: [255, 255, 255],
            show_lunar: false, lunar_color: [255, 136, 255],
            font_size: 14, hand_color: [0, 255, 136],
            second_color: [255, 68, 0], dial_color: [13, 26, 13],
        })
    }
    pub fn new_neon(guid: String) -> Self {
        Self::Neon(NeonItem { guid, index: 6, color: [255, 0, 0], speed: 5, rainbow: true })
    }
    pub fn new_qr(guid: String) -> Self {
        Self::QrCode(QrCodeItem { guid, data: "https://example.com".into(), fg: [255,255,255], bg: [0,0,0] })
    }
    pub fn new_image(guid: String) -> Self {
        Self::Image(ImageItem { guid, path: None, fit: 0 })
    }
    pub fn new_video(guid: String) -> Self {
        Self::Video(VideoItem { guid, path: None, keep_aspect: true })
    }
    pub fn new_calendar(guid: String) -> Self {
        Self::Calendar(CalendarItem {
            guid, color: [204,204,204], today_color: [255,255,0],
            header_color: [0,170,255], font_size: 7,
        })
    }
    pub fn new_countdown(guid: String) -> Self {
        Self::Countdown(CountdownItem {
            guid, target: "2027-01-01 00:00:00".into(), label: "Until".into(),
            color: [0,255,136], font_size: 14, format: "D:H:M:S".into(),
        })
    }
    pub fn new_table(guid: String) -> Self {
        Self::Table(TableItem {
            guid, cols: 2, rows: vec![
                vec!["Header 1".into(), "Header 2".into()],
                vec!["Value A".into(), "Value B".into()],
            ],
            header_row: true, text_color: [255,255,255],
            header_bg: [34,51,102], font_size: 9,
        })
    }
    pub fn new_livestream(guid: String) -> Self {
        Self::LiveStream(LiveStreamItem {
            guid, url: "rtsp://".into(), reconnect: true,
            font_size: 14, color: [255,255,255],
        })
    }
    pub fn new_modbus(guid: String) -> Self {
        Self::Modbus(ModbusItem {
            guid, host: "192.168.1.10".into(), port: 502,
            slave: 1, register: 1, register_type: "holding".into(),
            format: "{value}".into(), scale_str: "1.0".into(),
            update_interval: 5, scroll_speed: 0,
            font_size: 14, color: [255,255,255],
        })
    }
    pub fn new_sensor(guid: String) -> Self {
        Self::Sensor(SensorItem {
            guid, sensor_type: "cpu_temp".into(), device: String::new(),
            format: "{value}°C".into(), update_interval: 30, scroll_speed: 0,
            font_size: 14, color: [255,255,255],
        })
    }
    pub fn new_text3d(guid: String) -> Self {
        Self::Text3D(Text3DItem {
            guid, text: "HELLO".into(),
            color: [255,68,0], depth_color: [136,34,0],
            font_size: 24.0, rotate_speed: 1.0, effect_3d: "rotate_y".into(),
        })
    }
    pub fn new_document(guid: String) -> Self {
        Self::Document(DocumentItem {
            guid, path: None, page_duration: 5, fit: 0, loop_pages: true,
        })
    }
}

// ── AREA + PROGRAM ────────────────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Area {
    pub guid: String,
    pub name: String,
    pub alpha: u8,
    pub x: i32, pub y: i32, pub w: i32, pub h: i32,
    pub items: Vec<ContentItem>,
}

impl Area {
    pub fn new(guid: String, name: String, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { guid, name, alpha: 255, x, y, w, h, items: Vec::new() }
    }
}

#[derive(Clone, Debug)]
pub struct Program {
    pub guid: String,
    pub name: String,
    pub program_type: String,
    pub play_duration_secs: u32,
    pub play_count: u32,
    pub border_index: u8,
    pub border_speed: u8,
    pub areas: Vec<Area>,
    // PlayControl schedule (empty string = no constraint)
    pub date_start: String,   // "YYYY-MM-DD" or ""
    pub date_end: String,     // "YYYY-MM-DD" or ""
    pub time_start: String,   // "HH:MM" or ""
    pub time_end: String,     // "HH:MM" or ""
    pub week_filter: [bool; 7], // Mon=0 … Sun=6; all true = no filter
    pub disabled: bool,
}

impl Program {
    pub fn new(guid: String, name: String, screen_w: i32, screen_h: i32) -> Self {
        let area_guid = new_guid();
        let mut p = Self {
            guid, name, program_type: "normal".into(),
            play_duration_secs: 15, play_count: 0,
            border_index: 0, border_speed: 5, areas: Vec::new(),
            date_start: String::new(), date_end: String::new(),
            time_start: String::new(), time_end: String::new(),
            week_filter: [true; 7], disabled: false,
        };
        p.areas.push(Area::new(area_guid, "Main".into(), 0, 0, screen_w, screen_h));
        p
    }
}

#[derive(Clone, Debug)]
pub struct Project {
    pub screen_w: i32,
    pub screen_h: i32,
    pub programs: Vec<Program>,
    pub path: Option<PathBuf>,
    pub modified: bool,
}

impl Project {
    pub fn new(w: i32, h: i32) -> Self {
        Self { screen_w: w, screen_h: h, programs: Vec::new(), path: None, modified: false }
    }
}

// ── HELPERS ───────────────────────────────────────────────────────────────────

fn new_guid() -> String {
    uuid::Uuid::new_v4().to_string().to_uppercase()
}

fn rgb_to_hex(c: [u8; 3]) -> String { format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]) }
fn hex_to_rgb(s: &str) -> [u8; 3] {
    let s = s.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(255);
        [r, g, b]
    } else { [255, 255, 255] }
}
fn to_c32(c: [u8; 3]) -> Color32 { Color32::from_rgb(c[0], c[1], c[2]) }
fn from_c32(c: Color32) -> [u8; 3] { [c.r(), c.g(), c.b()] }

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}
fn xml_unescape(s: &str) -> String {
    s.replace("&amp;", "&").replace("&lt;", "<").replace("&gt;", ">")
     .replace("&quot;", "\"").replace("&apos;", "'")
}
fn get_attr<'a>(xml: &'a str, attr: &str) -> Option<&'a str> {
    // Accept both double-quoted and single-quoted attribute values.
    for q in &['"', '\''] {
        let needle = format!("{}={}", attr, q);
        if let Some(s) = xml.find(&needle) {
            let vs = s + needle.len();
            if let Some(e) = xml[vs..].find(*q) { return Some(&xml[vs..vs+e]); }
        }
    }
    None
}
#[allow(dead_code)]
fn get_tag_text<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}>", tag);
    let close = format!("</{}>", tag);
    let s = xml.find(&open)? + open.len();
    let e = xml[s..].find(&close)?;
    Some(&xml[s..s+e])
}
fn get_attr_in_tag<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    let pat = format!("<{} ", tag);
    let s = xml.find(&pat)?;
    let e = xml[s..].find("/>").map(|e| s+e+2)
        .or_else(|| xml[s..].find('>').map(|e| s+e+1))?;
    get_attr(&xml[s..e], attr)
}

// ── MEDIA FILE COLLECTION ─────────────────────────────────────────────────────

/// Collect all media files referenced by the project.
/// Returns a deduplicated list of (device_filename, local_path) pairs.
fn collect_media_files(project: &Project) -> Vec<(String, PathBuf)> {
    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::new();
    for prog in &project.programs {
        for area in &prog.areas {
            for item in &area.items {
                let path_opt: Option<&PathBuf> = match item {
                    ContentItem::Image(im)    => im.path.as_ref(),
                    ContentItem::Video(v)     => v.path.as_ref(),
                    ContentItem::Document(dc) => dc.path.as_ref(),
                    _ => None,
                };
                if let Some(path) = path_opt {
                    if let Some(fname) = path.file_name().and_then(|n| n.to_str()) {
                        if seen.insert(fname.to_string()) {
                            files.push((fname.to_string(), path.clone()));
                        }
                    }
                }
            }
        }
    }
    files
}

// ── XML GENERATION ────────────────────────────────────────────────────────────

pub fn generate_boo(project: &Project) -> String {
    let mut out = String::from("<?xml version='1.0' encoding='utf-8'?>\n");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    out.push_str(&format!("<screen timeStamps=\"{}\">\n\n", ts));

    for prog in &project.programs {
        out.push_str(&format!("  <program guid=\"{}\" name=\"{}\" type=\"{}\">\n",
            prog.guid, xml_escape(&prog.name), prog.program_type));

        // ── playControl ──────────────────────────────────────────────────────
        let has_date = !prog.date_start.is_empty() || !prog.date_end.is_empty();
        let has_time = !prog.time_start.is_empty() || !prog.time_end.is_empty();
        let week_not_all = prog.week_filter.iter().any(|&b| !b);
        let has_schedule = has_date || has_time || week_not_all || prog.disabled;

        if prog.play_duration_secs > 0 {
            let h = prog.play_duration_secs / 3600;
            let m = (prog.play_duration_secs % 3600) / 60;
            let s = prog.play_duration_secs % 60;
            let disabled_attr = if prog.disabled { " disabled=\"true\"" } else { "" };
            if has_schedule {
                out.push_str(&format!("    <playControl duration=\"{:02}:{:02}:{:02}\" count=\"0\"{disabled_attr}>\n", h, m, s));
            } else {
                out.push_str(&format!("    <playControl duration=\"{:02}:{:02}:{:02}\" count=\"0\"/>\n", h, m, s));
            }
        } else {
            let disabled_attr = if prog.disabled { " disabled=\"true\"" } else { " disabled=\"false\"" };
            if has_schedule {
                out.push_str(&format!("    <playControl count=\"{}\"{disabled_attr}>\n", prog.play_count.max(1)));
            } else {
                out.push_str(&format!("    <playControl count=\"{}\"{disabled_attr}/>\n", prog.play_count.max(1)));
            }
        }
        if has_schedule {
            if has_date {
                out.push_str(&format!("      <date start=\"{}\" end=\"{}\"/>\n",
                    prog.date_start, prog.date_end));
            }
            if has_time {
                out.push_str(&format!("      <time start=\"{}\" end=\"{}\"/>\n",
                    prog.time_start, prog.time_end));
            }
            if week_not_all {
                let bits: String = prog.week_filter.iter().map(|&b| if b { '1' } else { '0' }).collect();
                out.push_str(&format!("      <week enable=\"{}\"/>\n", bits));
            }
            out.push_str("    </playControl>\n");
        }

        if prog.border_index > 0 {
            out.push_str(&format!("    <border index=\"{}\" speed=\"{}\"/>\n",
                prog.border_index, prog.border_speed));
        }

        for area in &prog.areas {
            out.push_str(&format!(
                "    <area guid=\"{}\" name=\"{}\" alpha=\"{}\">\n      <rectangle x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n      <resources>\n",
                area.guid, xml_escape(&area.name), area.alpha, area.x, area.y, area.w, area.h));

            for item in &area.items {
                match item {
                    ContentItem::Text(t) => {
                        out.push_str(&format!("        <text guid=\"{}\" singleLine=\"{}\"",
                            t.guid, t.single_line));
                        if let Some(bg) = t.background {
                            out.push_str(&format!(" background=\"{}\"", rgb_to_hex(bg)));
                        }
                        if t.word_wrap {
                            out.push_str(" wordWrap=\"true\"");
                        }
                        if t.scroll_dir > 0 {
                            let dir = match t.scroll_dir { 1 => "left", 2 => "right", 3 => "up", _ => "down" };
                            out.push_str(&format!(" scrollDir=\"{}\" scrollSpeed=\"{}\"", dir, t.scroll_speed));
                        }
                        out.push_str(">\n");
                        out.push_str(&format!("          <string>{}</string>\n", xml_escape(&t.text)));
                        let al = ["left", "center", "right"][t.align as usize % 3];
                        let va = ["top", "middle", "bottom"][t.valign as usize % 3];
                        out.push_str(&format!("          <style align=\"{}\" valign=\"{}\"/>\n", al, va));
                        let mut fa = format!("size=\"{}\" color=\"{}\"", t.font_size, rgb_to_hex(t.color));
                        if t.bold { fa.push_str(" bold=\"true\""); }
                        if t.italic { fa.push_str(" italic=\"true\""); }
                        if t.underline { fa.push_str(" underline=\"true\""); }
                        if !t.font_name.is_empty() { fa.push_str(&format!(" name=\"{}\"", xml_escape(&t.font_name))); }
                        out.push_str(&format!("          <font {}/>\n", fa));
                        out.push_str(&format!(
                            "          <effect in=\"{}\" out=\"{}\" inSpeed=\"{}\" outSpeed=\"{}\" duration=\"{}\"/>\n",
                            t.effect_in, t.effect_out, t.effect_in_speed, t.effect_out_speed, t.duration_tenths));
                        out.push_str("        </text>\n");
                    }
                    ContentItem::Image(im) => {
                        let fit = ["stretch","fill","center","fit"][im.fit as usize % 4];
                        let fname = im.path.as_ref().and_then(|p| p.file_name())
                            .and_then(|n| n.to_str()).unwrap_or("");
                        out.push_str(&format!("        <image guid=\"{}\" fit=\"{}\">\n", im.guid, fit));
                        out.push_str("          <effect in=\"17\" out=\"17\" inSpeed=\"3\" outSpeed=\"3\" duration=\"50\"/>\n");
                        if !fname.is_empty() {
                            out.push_str(&format!("          <file name=\"{}\"/>\n", fname));
                        }
                        out.push_str("        </image>\n");
                    }
                    ContentItem::Video(v) => {
                        let fname = v.path.as_ref().and_then(|p| p.file_name())
                            .and_then(|n| n.to_str()).unwrap_or("");
                        out.push_str(&format!("        <video guid=\"{}\" aspectRatio=\"{}\">\n",
                            v.guid, if v.keep_aspect { "true" } else { "false" }));
                        if !fname.is_empty() {
                            out.push_str(&format!("          <file name=\"{}\"/>\n", fname));
                        }
                        out.push_str("        </video>\n");
                    }
                    ContentItem::Clock(c) => {
                        if c.is_analog {
                            // Analog clock uses a distinct tag in the player model
                            out.push_str(&format!(
                                "        <analogClock guid=\"{}\" timezone=\"{}\" dialColor=\"{}\" handColor=\"{}\" secondColor=\"{}\"/>\n",
                                c.guid, xml_escape(&c.timezone),
                                rgb_to_hex(c.dial_color), rgb_to_hex(c.hand_color), rgb_to_hex(c.second_color)));
                        } else {
                            out.push_str(&format!("        <clock guid=\"{}\" type=\"digital\" timezone=\"{}\">\n",
                                c.guid, xml_escape(&c.timezone)));
                            if c.show_title {
                                out.push_str(&format!("          <title value=\"{}\" color=\"{}\" display=\"true\"/>\n",
                                    xml_escape(&c.title_text), rgb_to_hex(c.title_color)));
                            }
                            if c.show_date {
                                out.push_str(&format!("          <date format=\"{}\" color=\"{}\" display=\"true\"/>\n",
                                    c.date_format, rgb_to_hex(c.date_color)));
                            }
                            if c.show_week {
                                out.push_str(&format!("          <week color=\"{}\" display=\"true\"/>\n",
                                    rgb_to_hex(c.week_color)));
                            }
                            if c.show_time {
                                out.push_str(&format!("          <time format=\"{}\" color=\"{}\" display=\"true\"/>\n",
                                    c.time_format, rgb_to_hex(c.time_color)));
                            }
                            if c.show_lunar {
                                out.push_str(&format!("          <lunarCalendar color=\"{}\" display=\"true\"/>\n",
                                    rgb_to_hex(c.lunar_color)));
                            }
                            out.push_str("        </clock>\n");
                        }
                    }
                    ContentItem::Neon(n) => {
                        let col = if n.rainbow { "rainbow".into() } else { rgb_to_hex(n.color) };
                        out.push_str(&format!("        <neon guid=\"{}\" index=\"{}\" color=\"{}\" speed=\"{}\" singleColor=\"{}\"/>\n",
                            n.guid, n.index + 1, col, n.speed, !n.rainbow));
                    }
                    ContentItem::QrCode(q) => {
                        out.push_str(&format!("        <qrCode guid=\"{}\" data=\"{}\" fgColor=\"{}\" bgColor=\"{}\"/>\n",
                            q.guid, xml_escape(&q.data), rgb_to_hex(q.fg), rgb_to_hex(q.bg)));
                    }
                    ContentItem::Calendar(c) => {
                        out.push_str(&format!(
                            "        <calendar guid=\"{}\" color=\"{}\" todayColor=\"{}\" headerColor=\"{}\" fontSize=\"{}\"/>\n",
                            c.guid, rgb_to_hex(c.color), rgb_to_hex(c.today_color),
                            rgb_to_hex(c.header_color), c.font_size));
                    }
                    ContentItem::Countdown(c) => {
                        out.push_str(&format!(
                            "        <countdownTimer guid=\"{}\" target=\"{}\" format=\"{}\" label=\"{}\" color=\"{}\" fontSize=\"{}\"/>\n",
                            c.guid, xml_escape(&c.target), xml_escape(&c.format),
                            xml_escape(&c.label), rgb_to_hex(c.color), c.font_size));
                    }
                    ContentItem::Table(t) => {
                        out.push_str(&format!(
                            "        <table guid=\"{}\" cols=\"{}\" rows=\"{}\">\n          <style textColor=\"{}\" headerBgColor=\"{}\" fontSize=\"{}\" headerRow=\"{}\"/>\n",
                            t.guid, t.cols, t.rows.len(),
                            rgb_to_hex(t.text_color), rgb_to_hex(t.header_bg),
                            t.font_size, t.header_row));
                        for row in &t.rows {
                            out.push_str("          <row>");
                            for cell in row { out.push_str(&format!("<cell>{}</cell>", xml_escape(cell))); }
                            out.push_str("</row>\n");
                        }
                        out.push_str("        </table>\n");
                    }
                    ContentItem::LiveStream(ls) => {
                        out.push_str(&format!(
                            "        <liveStream guid=\"{}\" url=\"{}\" reconnect=\"{}\">\n",
                            ls.guid, xml_escape(&ls.url), ls.reconnect));
                        out.push_str(&format!(
                            "          <font size=\"{}\" color=\"{}\"/>\n",
                            ls.font_size, rgb_to_hex(ls.color)));
                        out.push_str("          <effect in=\"17\" out=\"17\" inSpeed=\"3\" outSpeed=\"3\" duration=\"50\"/>\n");
                        out.push_str("        </liveStream>\n");
                    }
                    ContentItem::Modbus(mb) => {
                        out.push_str(&format!(
                            "        <modbus guid=\"{}\" host=\"{}\" port=\"{}\" slave=\"{}\" register=\"{}\" type=\"{}\" format=\"{}\" scale=\"{}\" updateInterval=\"{}\" scrollSpeed=\"{}\">\n",
                            mb.guid, xml_escape(&mb.host), mb.port, mb.slave, mb.register,
                            mb.register_type, xml_escape(&mb.format), mb.scale_str,
                            mb.update_interval, mb.scroll_speed));
                        out.push_str(&format!(
                            "          <font size=\"{}\" color=\"{}\"/>\n",
                            mb.font_size, rgb_to_hex(mb.color)));
                        out.push_str("        </modbus>\n");
                    }
                    ContentItem::Sensor(sn) => {
                        out.push_str(&format!(
                            "        <sensor guid=\"{}\" type=\"{}\" device=\"{}\" format=\"{}\" updateInterval=\"{}\" scrollSpeed=\"{}\">\n",
                            sn.guid, sn.sensor_type, xml_escape(&sn.device),
                            xml_escape(&sn.format), sn.update_interval, sn.scroll_speed));
                        out.push_str(&format!(
                            "          <font size=\"{}\" color=\"{}\"/>\n",
                            sn.font_size, rgb_to_hex(sn.color)));
                        out.push_str("        </sensor>\n");
                    }
                    ContentItem::Text3D(t3) => {
                        out.push_str(&format!(
                            "        <text3D guid=\"{}\" text=\"{}\" color=\"{}\" depthColor=\"{}\" fontSize=\"{:.1}\" rotateSpeed=\"{:.2}\" effect3d=\"{}\"/>\n",
                            t3.guid, xml_escape(&t3.text),
                            rgb_to_hex(t3.color), rgb_to_hex(t3.depth_color),
                            t3.font_size, t3.rotate_speed, t3.effect_3d));
                    }
                    ContentItem::Document(doc) => {
                        let fname = doc.path.as_ref()
                            .and_then(|p| p.file_name())
                            .and_then(|n| n.to_str())
                            .unwrap_or("");
                        let fit = ["stretch","fill","center"][doc.fit as usize % 3];
                        out.push_str(&format!(
                            "        <document guid=\"{}\" file=\"{}\" pageDuration=\"{}\" fit=\"{}\" loopPages=\"{}\">\n",
                            doc.guid, fname, doc.page_duration, fit, doc.loop_pages));
                        out.push_str("          <effect in=\"17\" out=\"17\" inSpeed=\"3\" outSpeed=\"3\" duration=\"50\"/>\n");
                        out.push_str("        </document>\n");
                    }
                }
            }
            out.push_str("      </resources>\n    </area>\n");
        }
        out.push_str("  </program>\n\n");
    }
    out.push_str("</screen>\n");
    out
}

// Full AddProgram SDK envelope for sending to device
pub fn generate_sdk_envelope(project: &Project) -> String {
    let guid = new_guid();
    let screen_xml = generate_boo(project);
    // Strip the <?xml?> header and wrap <screen>...</screen> in SDK envelope
    let inner = screen_xml.trim_start_matches("<?xml version='1.0' encoding='utf-8'?>\n");
    format!("<?xml version='1.0' encoding='utf-8'?>\n<sdk guid=\"{}\">\n<in method=\"AddProgram\">\n{}</in>\n</sdk>\n",
        guid, inner)
}

// ── XML PARSING ───────────────────────────────────────────────────────────────

pub fn parse_boo(xml: &str) -> Option<Project> {
    let mut programs = Vec::new();
    let mut max_w = 128i32;
    let mut max_h = 64i32;
    let mut search = xml;
    while let Some(ps) = search.find("<program ") {
        let pe = search[ps..].find("</program>").map(|e| ps + e + 10).unwrap_or(search.len());
        if let Some(prog) = parse_program(&search[ps..pe], &mut max_w, &mut max_h) {
            programs.push(prog);
        }
        search = &search[pe.min(search.len())..];
    }
    if programs.is_empty() { return None; }
    Some(Project { screen_w: max_w, screen_h: max_h, programs, path: None, modified: false })
}

fn parse_program(xml: &str, max_w: &mut i32, max_h: &mut i32) -> Option<Program> {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    let name = get_attr(xml, "name").map(|s| xml_unescape(s)).unwrap_or_else(|| "Program".into());
    let program_type = get_attr(xml, "type").unwrap_or("normal").to_string();
    let border_index = get_attr(xml, "index").and_then(|v| v.parse().ok()).unwrap_or(0u8);
    let border_speed = get_attr(xml, "speed").and_then(|v| v.parse().ok()).unwrap_or(5u8);

    let play_duration_secs = get_attr(xml, "duration").and_then(|d| {
        let p: Vec<&str> = d.split(':').collect();
        if p.len() == 3 {
            Some(p[0].parse::<u32>().ok()? * 3600 + p[1].parse::<u32>().ok()? * 60 + p[2].parse::<u32>().ok()?)
        } else { None }
    }).unwrap_or(15);

    let disabled = get_attr(xml, "disabled").map(|v| v == "true").unwrap_or(false);

    // Parse PlayControl schedule sub-elements
    let (date_start, date_end) = if let Some(ds) = xml.find("<date ") {
        let de = xml[ds..].find("/>").map(|e| ds+e+2).unwrap_or(xml.len());
        let dt = &xml[ds..de];
        (
            get_attr(dt, "start").unwrap_or("").to_string(),
            get_attr(dt, "end").unwrap_or("").to_string(),
        )
    } else { (String::new(), String::new()) };

    let (time_start, time_end) = {
        // Find <time start="..." end="..."/> (PlayControl time range),
        // skipping <time format=...> clock sub-elements.
        let mut found = None;
        let mut s = xml;
        while let Some(p) = s.find("<time ") {
            let end_off = s[p..].find("/>").map(|e| p+e+2).unwrap_or(s.len());
            let t = &s[p..end_off];
            if get_attr(t, "start").is_some() || get_attr(t, "end").is_some() {
                found = Some((
                    get_attr(t, "start").unwrap_or("").to_string(),
                    get_attr(t, "end").unwrap_or("").to_string(),
                ));
                break;
            }
            s = &s[end_off.min(s.len())..];
        }
        found.unwrap_or_default()
    };

    let week_filter: [bool; 7] = if let Some(ws) = xml.find("<week ") {
        let we = xml[ws..].find("/>").map(|e| ws+e+2).unwrap_or(xml.len());
        let wt = &xml[ws..we];
        let enable = get_attr(wt, "enable").unwrap_or("1111111");
        let mut arr = [true; 7];
        for (i, c) in enable.chars().take(7).enumerate() {
            arr[i] = c != '0';
        }
        arr
    } else { [true; 7] };

    let mut areas = Vec::new();
    let mut search = xml;
    while let Some(as_) = search.find("<area ") {
        let ae = search[as_..].find("</area>").map(|e| as_ + e + 7).unwrap_or(search.len());
        if let Some(area) = parse_area(&search[as_..ae]) {
            *max_w = (*max_w).max(area.x + area.w);
            *max_h = (*max_h).max(area.y + area.h);
            areas.push(area);
        }
        search = &search[ae.min(search.len())..];
    }

    Some(Program {
        guid: if guid.is_empty() { new_guid() } else { guid },
        name, program_type, play_duration_secs, play_count: 1,
        border_index, border_speed, areas,
        date_start, date_end, time_start, time_end, week_filter, disabled,
    })
}

fn parse_area(xml: &str) -> Option<Area> {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    let name = get_attr(xml, "name").map(|s| xml_unescape(s)).unwrap_or_else(|| "Area".into());
    let alpha = get_attr(xml, "alpha").and_then(|v| v.parse().ok()).unwrap_or(255u8);
    let x = get_attr(xml, "x").and_then(|v| v.parse().ok()).unwrap_or(0i32);
    let y = get_attr(xml, "y").and_then(|v| v.parse().ok()).unwrap_or(0i32);
    let w = get_attr(xml, "width").and_then(|v| v.parse().ok()).unwrap_or(128i32);
    let h = get_attr(xml, "height").and_then(|v| v.parse().ok()).unwrap_or(64i32);
    let mut area = Area::new(if guid.is_empty() { new_guid() } else { guid }, name, x, y, w, h);
    area.alpha = alpha;

    if let Some(rs) = xml.find("<resources>") {
        if let Some(re) = xml[rs..].find("</resources>") {
            area.items = parse_items(&xml[rs+11..rs+re]);
        }
    }
    Some(area)
}

fn parse_items(xml: &str) -> Vec<ContentItem> {
    let mut items = Vec::new();
    // Text
    let mut s = xml;
    while let Some(ps) = s.find("<text ") {
        let pe = s[ps..].find("</text>").map(|e| ps+e+7).unwrap_or(s.len());
        items.push(parse_text(&s[ps..pe]));
        s = &s[pe.min(s.len())..];
    }
    // Digital clock — <clock type="digital" ...>
    let mut s = xml;
    while let Some(ps) = s.find("<clock ") {
        let pe = s[ps..].find("</clock>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        items.push(parse_clock(&s[ps..pe]));
        s = &s[pe.min(s.len())..];
    }
    // Analog clock — <analogClock .../>  (separate tag in player model)
    let mut s = xml;
    while let Some(ps) = s.find("<analogClock ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2).unwrap_or(s.len());
        items.push(parse_analog_clock(&s[ps..pe]));
        s = &s[pe.min(s.len())..];
    }
    // Neon
    let mut s = xml;
    while let Some(ps) = s.find("<neon ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2).unwrap_or(s.len());
        let t = &s[ps..pe];
        let idx_1 = get_attr(t, "index").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1);
        let col_s = get_attr(t, "color").unwrap_or("rainbow");
        let rainbow = col_s.eq_ignore_ascii_case("rainbow");
        items.push(ContentItem::Neon(NeonItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            index: idx_1.saturating_sub(1),
            color: if rainbow { [255,0,0] } else { hex_to_rgb(col_s) },
            speed: get_attr(t, "speed").and_then(|v| v.parse().ok()).unwrap_or(5),
            rainbow,
        }));
        s = &s[pe.min(s.len())..];
    }
    // QR Code
    let mut s = xml;
    while let Some(ps) = s.find("<qrCode ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::QrCode(QrCodeItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            data: get_attr(t, "data").map(|s| xml_unescape(s)).unwrap_or_default(),
            fg: hex_to_rgb(get_attr(t, "fgColor").unwrap_or("#ffffff")),
            bg: hex_to_rgb(get_attr(t, "bgColor").unwrap_or("#000000")),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Calendar
    let mut s = xml;
    while let Some(ps) = s.find("<calendar ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Calendar(CalendarItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            color: hex_to_rgb(get_attr(t, "color").unwrap_or("#cccccc")),
            today_color: hex_to_rgb(get_attr(t, "todayColor").unwrap_or("#ffff00")),
            header_color: hex_to_rgb(get_attr(t, "headerColor").unwrap_or("#0088ff")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(7),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Countdown
    let mut s = xml;
    while let Some(ps) = s.find("<countdownTimer ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Countdown(CountdownItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            target: get_attr(t, "target").map(|s| xml_unescape(s)).unwrap_or_else(|| "2027-01-01 00:00:00".into()),
            label: get_attr(t, "label").map(|s| xml_unescape(s)).unwrap_or_default(),
            color: hex_to_rgb(get_attr(t, "color").unwrap_or("#00ff88")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(14),
            format: get_attr(t, "format").map(|s| xml_unescape(s)).unwrap_or_else(|| "D:H:M:S".into()),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Image — <image guid="..." fit="..."><effect .../><file name="..."/></image>
    let mut s = xml;
    while let Some(ps) = s.find("<image ") {
        let pe = s[ps..].find("</image>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fit = match get_attr(t, "fit").unwrap_or("stretch") {
            "fill" => 1, "center" => 2, "fit" => 3, _ => 0,
        };
        // Filename lives in <file name="..."/> child, not on <image> itself
        let fname = get_attr_in_tag(t, "file", "name").map(|n| xml_unescape(n));
        items.push(ContentItem::Image(ImageItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from),
            fit,
        }));
        s = &s[pe.min(s.len())..];
    }
    // Video — <video guid="..." aspectRatio="true"><file name="..."/></video>
    let mut s = xml;
    while let Some(ps) = s.find("<video ") {
        let pe = s[ps..].find("</video>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fname = get_attr_in_tag(t, "file", "name").map(|n| xml_unescape(n));
        items.push(ContentItem::Video(VideoItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from),
            keep_aspect: get_attr(t, "aspectRatio").map(|v| v == "true" || v == "1").unwrap_or(true),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Table — <table guid="..." cols="2"><style .../><row><cell>…</cell>…</row>…</table>
    let mut s = xml;
    while let Some(ps) = s.find("<table ") {
        let pe = s[ps..].find("</table>").map(|e| ps+e+8).unwrap_or(s.len());
        let t = &s[ps..pe];
        let cols = get_attr(t, "cols").and_then(|v| v.parse().ok()).unwrap_or(2usize);
        let text_color = hex_to_rgb(get_attr(t, "textColor").unwrap_or("#ffffff"));
        let header_bg = hex_to_rgb(get_attr(t, "headerBgColor").unwrap_or("#223366"));
        let font_size = get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(9u32);
        let header_row = get_attr(t, "headerRow").map(|v| v == "true").unwrap_or(true);
        let mut rows: Vec<Vec<String>> = Vec::new();
        let mut rs = t;
        while let Some(rps) = rs.find("<row>") {
            let rpe = rs[rps..].find("</row>").map(|e| rps+e+6).unwrap_or(rs.len());
            let row_xml = &rs[rps+5..rpe.saturating_sub(0)]; // inside <row>
            let mut row: Vec<String> = Vec::new();
            let mut cs = row_xml;
            while let Some(cps) = cs.find("<cell>") {
                let cpe = cs[cps..].find("</cell>").map(|e| cps+e+7).unwrap_or(cs.len());
                let cell_text = xml_unescape(&cs[cps+6..cps + cs[cps..].find("</cell>").unwrap_or(cs.len()-cps)]);
                row.push(cell_text);
                cs = &cs[cpe.min(cs.len())..];
            }
            while row.len() < cols { row.push(String::new()); }
            rows.push(row);
            rs = &rs[rpe.min(rs.len())..];
        }
        items.push(ContentItem::Table(TableItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            cols, rows, header_row, text_color, header_bg, font_size,
        }));
        s = &s[pe.min(s.len())..];
    }
    // LiveStream
    let mut s = xml;
    while let Some(ps) = s.find("<liveStream ") {
        let pe = s[ps..].find("</liveStream>").map(|e| ps+e+13)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::LiveStream(LiveStreamItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            url: get_attr(t, "url").map(|v| xml_unescape(v)).unwrap_or_default(),
            reconnect: get_attr(t, "reconnect").map(|v| v != "false").unwrap_or(true),
            font_size: get_attr_in_tag(t, "font", "size").and_then(|v| v.parse().ok()).unwrap_or(14),
            color: hex_to_rgb(get_attr_in_tag(t, "font", "color").unwrap_or("#ffffff")),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Modbus
    let mut s = xml;
    while let Some(ps) = s.find("<modbus ") {
        let pe = s[ps..].find("</modbus>").map(|e| ps+e+9)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Modbus(ModbusItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            host: get_attr(t, "host").map(|v| xml_unescape(v)).unwrap_or_default(),
            port: get_attr(t, "port").and_then(|v| v.parse().ok()).unwrap_or(502),
            slave: get_attr(t, "slave").and_then(|v| v.parse().ok()).unwrap_or(1),
            register: get_attr(t, "register").and_then(|v| v.parse().ok()).unwrap_or(1),
            register_type: get_attr(t, "type").unwrap_or("holding").to_string(),
            format: get_attr(t, "format").map(|v| xml_unescape(v)).unwrap_or_else(|| "{value}".into()),
            scale_str: get_attr(t, "scale").unwrap_or("1.0").to_string(),
            update_interval: get_attr(t, "updateInterval").and_then(|v| v.parse().ok()).unwrap_or(5),
            scroll_speed: get_attr(t, "scrollSpeed").and_then(|v| v.parse().ok()).unwrap_or(0),
            font_size: get_attr_in_tag(t, "font", "size").and_then(|v| v.parse().ok()).unwrap_or(14),
            color: hex_to_rgb(get_attr_in_tag(t, "font", "color").unwrap_or("#ffffff")),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Sensor
    let mut s = xml;
    while let Some(ps) = s.find("<sensor ") {
        let pe = s[ps..].find("</sensor>").map(|e| ps+e+9)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Sensor(SensorItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            sensor_type: get_attr(t, "type").unwrap_or("cpu_temp").to_string(),
            device: get_attr(t, "device").map(|v| xml_unescape(v)).unwrap_or_default(),
            format: get_attr(t, "format").map(|v| xml_unescape(v)).unwrap_or_else(|| "{value}".into()),
            update_interval: get_attr(t, "updateInterval").and_then(|v| v.parse().ok()).unwrap_or(30),
            scroll_speed: get_attr(t, "scrollSpeed").and_then(|v| v.parse().ok()).unwrap_or(0),
            font_size: get_attr_in_tag(t, "font", "size").and_then(|v| v.parse().ok()).unwrap_or(14),
            color: hex_to_rgb(get_attr_in_tag(t, "font", "color").unwrap_or("#ffffff")),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Text3D
    let mut s = xml;
    while let Some(ps) = s.find("<text3D ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2)
            .or_else(|| s[ps..].find("</text3D>").map(|e| ps+e+9)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Text3D(Text3DItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            text: get_attr(t, "text").map(|v| xml_unescape(v)).unwrap_or_default(),
            color: hex_to_rgb(get_attr(t, "color").unwrap_or("#ff4400")),
            depth_color: hex_to_rgb(get_attr(t, "depthColor").unwrap_or("#882200")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(20.0),
            rotate_speed: get_attr(t, "rotateSpeed").and_then(|v| v.parse().ok()).unwrap_or(1.0),
            effect_3d: get_attr(t, "effect3d").unwrap_or("rotate_y").to_string(),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Document
    let mut s = xml;
    while let Some(ps) = s.find("<document ") {
        let pe = s[ps..].find("</document>").map(|e| ps+e+11)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fname = get_attr(t, "file").map(|n| xml_unescape(n));
        let fit = match get_attr(t, "fit").unwrap_or("stretch") {
            "fill" => 1, "center" => 2, _ => 0,
        };
        items.push(ContentItem::Document(DocumentItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from),
            page_duration: get_attr(t, "pageDuration").and_then(|v| v.parse().ok()).unwrap_or(5),
            fit,
            loop_pages: get_attr(t, "loopPages").map(|v| v != "false").unwrap_or(true),
        }));
        s = &s[pe.min(s.len())..];
    }
    items
}

fn parse_text(xml: &str) -> ContentItem {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    let single_line = get_attr(xml, "singleLine").map(|v| v == "true").unwrap_or(false);
    let background = get_attr(xml, "background").map(|s| hex_to_rgb(s));
    let text = xml.find("<string>").and_then(|s| {
        xml[s+8..].find("</string>").map(|e| xml_unescape(&xml[s+8..s+8+e]))
    }).unwrap_or_default();
    let font_size = get_attr(xml, "size").and_then(|v| v.parse().ok()).unwrap_or(14u32);
    let color_s = get_attr(xml, "color").unwrap_or("#ffff00");
    let color = if color_s.starts_with('#') { hex_to_rgb(color_s) } else { [255,255,0] };
    ContentItem::Text(TextItem {
        guid, text, single_line, font_name: get_attr(xml, "name").map(|s| xml_unescape(s)).unwrap_or_default(),
        font_size, color,
        bold: get_attr(xml, "bold").map(|v| v=="true").unwrap_or(false),
        italic: get_attr(xml, "italic").map(|v| v=="true").unwrap_or(false),
        underline: get_attr(xml, "underline").map(|v| v=="true").unwrap_or(false),
        align: match get_attr(xml, "align").unwrap_or("center") { "left"=>0, "right"=>2, _=>1 },
        valign: match get_attr(xml, "valign").unwrap_or("middle") { "top"=>0, "bottom"=>2, _=>1 },
        effect_in: get_attr(xml, "in").and_then(|v| v.parse().ok()).unwrap_or(17),
        effect_out: get_attr(xml, "out").and_then(|v| v.parse().ok()).unwrap_or(17),
        effect_in_speed: get_attr(xml, "inSpeed").and_then(|v| v.parse().ok()).unwrap_or(3),
        effect_out_speed: get_attr(xml, "outSpeed").and_then(|v| v.parse().ok()).unwrap_or(3),
        duration_tenths: get_attr(xml, "duration").and_then(|v| v.parse().ok()).unwrap_or(50),
        scroll_dir: match get_attr(xml, "scrollDir").unwrap_or("none") {
            "left" => 1, "right" => 2, "up" => 3, "down" => 4, _ => 0
        },
        scroll_speed: get_attr(xml, "scrollSpeed").and_then(|v| v.parse().ok()).unwrap_or(40),
        word_wrap: get_attr(xml, "wordWrap").map(|v| v == "true").unwrap_or(false),
        background,
    })
}

fn parse_clock(xml: &str) -> ContentItem {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    // Accept old numeric format (0/1) and new string format (digital/dial/analog)
    let is_analog = get_attr(xml, "type")
        .map(|v| v == "1" || v == "dial" || v == "analog")
        .unwrap_or(false);
    ContentItem::Clock(ClockItem {
        guid, is_analog,
        timezone: get_attr(xml, "timezone").map(|s| xml_unescape(s)).unwrap_or_else(|| "+00:00".into()),
        show_title: xml.contains("<title "),
        title_text: get_attr_in_tag(xml, "title", "value").map(|s| xml_unescape(s)).unwrap_or_default(),
        title_color: hex_to_rgb(get_attr_in_tag(xml, "title", "color").unwrap_or("#ffaa00")),
        show_date: xml.contains("<date "),
        date_format: get_attr_in_tag(xml, "date", "format").and_then(|v| v.parse().ok()).unwrap_or(1),
        date_color: hex_to_rgb(get_attr_in_tag(xml, "date", "color").unwrap_or("#00ff88")),
        show_week: xml.contains("<week "),
        week_color: hex_to_rgb(get_attr_in_tag(xml, "week", "color").unwrap_or("#88ffff")),
        show_time: xml.contains("<time "),
        time_format: get_attr_in_tag(xml, "time", "format").and_then(|v| v.parse().ok()).unwrap_or(1),
        time_color: hex_to_rgb(get_attr_in_tag(xml, "time", "color").unwrap_or("#ffffff")),
        show_lunar: xml.contains("<lunarCalendar "),
        lunar_color: hex_to_rgb(get_attr_in_tag(xml, "lunarCalendar", "color").unwrap_or("#ff88ff")),
        font_size: 14,
        dial_color: hex_to_rgb(get_attr(xml, "dialColor").unwrap_or("#0a0a1e")),
        hand_color: hex_to_rgb(get_attr(xml, "handColor").unwrap_or("#ffffff")),
        second_color: hex_to_rgb(get_attr(xml, "secondColor").unwrap_or("#ff3c3c")),
    })
}

fn parse_analog_clock(xml: &str) -> ContentItem {
    let mut item = parse_clock(xml);
    if let ContentItem::Clock(ref mut c) = item { c.is_analog = true; }
    item
}

// ── DEVICE COMMS ──────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Request {
    Discover,
    Connect { host: String, port: u16 },
    Disconnect,
    RefreshPrograms,
    GetScreenshot,
    SetBrightness(u8),
    SetVolume(u8),
    SetRotation(u16),
    ScreenOn, ScreenOff, Reboot, SyncTime,
    SwitchProgram(String),
    DeleteProgram(String),
    /// Upload program XML plus any referenced media files.
    /// `files` is a list of (device_filename, local_path) pairs to send first.
    UploadProgram { xml: String, files: Vec<(String, PathBuf)> },
    /// Apply screen on/off schedule to device. Each entry is (on_time, off_time, days).
    SetScreenSchedule(Vec<(String, String, String)>),
    /// Apply brightness schedule to device. Each entry is (hour, min, level).
    SetBrightnessSchedule(Vec<(u8, u8, u8)>),
    /// Run native port-9528 firmware upgrade.
    UpgradeNative { file: PathBuf, host: String },
}

#[derive(Debug)]
enum Response {
    Devices(Vec<DeviceInfo>),
    Connected(DeviceDetails, Vec<ProgramInfo>),
    Programs(Vec<ProgramInfo>),
    Screenshot(Vec<u8>),
    Ok(String),
    Error(String),
    Disconnected,
    /// Firmware upgrade byte progress (bytes_sent, total_bytes).
    UpgradeProgress { bytes: u64, total: u64 },
    /// Human-readable upgrade phase string (e.g. "Transferring…", "Extracting…").
    UpgradePhase(String),
    /// Upgrade finished successfully.
    UpgradeComplete,
}

async fn worker_loop(
    mut req_rx: tokio::sync::mpsc::Receiver<Request>,
    resp_tx: std::sync::mpsc::Sender<Response>,
) {
    let mut client: Option<Client> = None;
    // Heartbeat every 25 s keeps the TCP connection alive through NAT / the server's 30 s idle window
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Consume the immediate first tick so we don't send a heartbeat before connecting
    heartbeat.tick().await;

    loop {
        let req = tokio::select! {
            // Periodic heartbeat — send while connected, clear client on failure
            _ = heartbeat.tick() => {
                if let Some(c) = &mut client {
                    if c.heartbeat().await.is_err() {
                        client = None;
                        let _ = resp_tx.send(Response::Disconnected);
                    }
                }
                continue;
            }
            req_opt = req_rx.recv() => {
                match req_opt {
                    Some(r) => r,
                    None => break,
                }
            }
        };

        match req {
            Request::Discover => {
                match Discovery::scan(Duration::from_secs(3)).await {
                    Ok(devs) => { let _ = resp_tx.send(Response::Devices(devs)); }
                    Err(e) => { let _ = resp_tx.send(Response::Error(format!("Scan: {e}"))); }
                }
            }
            Request::Connect { host, port } => {
                match Client::connect(&host, port).await {
                    Ok(mut c) => {
                        let info = c.get_device_info().await.unwrap_or_default();
                        let progs = c.get_all_programs().await.unwrap_or_default();
                        client = Some(c);
                        let _ = resp_tx.send(Response::Connected(info, progs));
                    }
                    Err(e) => { let _ = resp_tx.send(Response::Error(format!("Connect: {e}"))); }
                }
            }
            Request::Disconnect => {
                client = None;
                let _ = resp_tx.send(Response::Disconnected);
            }
            Request::RefreshPrograms => {
                if let Some(c) = &mut client {
                    match c.get_all_programs().await {
                        Ok(p) => { let _ = resp_tx.send(Response::Programs(p)); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::GetScreenshot => {
                if let Some(c) = &mut client {
                    match c.screenshot().await {
                        Ok(b) => { let _ = resp_tx.send(Response::Screenshot(b)); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetBrightness(v) => {
                if let Some(c) = &mut client {
                    match c.set_brightness(v).await {
                        Ok(_) => { let _ = resp_tx.send(Response::Ok(format!("Brightness → {v}"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetVolume(v) => {
                if let Some(c) = &mut client {
                    match c.set_volume(v).await {
                        Ok(_) => { let _ = resp_tx.send(Response::Ok(format!("Volume → {v}"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetRotation(v) => {
                if let Some(c) = &mut client {
                    match c.set_rotation(v).await {
                        Ok(_) => { let _ = resp_tx.send(Response::Ok(format!("Rotation → {v}°"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::ScreenOn => {
                if let Some(c) = &mut client {
                    let _ = c.screen_on().await.map(|_| resp_tx.send(Response::Ok("Screen ON".into())))
                        .map_err(|e| resp_tx.send(Response::Error(format!("{e}"))));
                }
            }
            Request::ScreenOff => {
                if let Some(c) = &mut client {
                    let _ = c.screen_off().await.map(|_| resp_tx.send(Response::Ok("Screen OFF".into())))
                        .map_err(|e| resp_tx.send(Response::Error(format!("{e}"))));
                }
            }
            Request::Reboot => {
                if let Some(c) = &mut client {
                    let _ = c.reboot().await.map(|_| resp_tx.send(Response::Ok("Rebooting…".into())))
                        .map_err(|e| resp_tx.send(Response::Error(format!("{e}"))));
                    client = None;
                }
            }
            Request::SyncTime => {
                if let Some(c) = &mut client {
                    let _ = c.sync_time().await.map(|_| resp_tx.send(Response::Ok("Time synced".into())))
                        .map_err(|e| resp_tx.send(Response::Error(format!("{e}"))));
                }
            }
            Request::SwitchProgram(guid) => {
                if let Some(c) = &mut client {
                    let _ = c.switch_program(&guid).await.map(|_| resp_tx.send(Response::Ok("Switched".into())))
                        .map_err(|e| resp_tx.send(Response::Error(format!("{e}"))));
                }
            }
            Request::DeleteProgram(guid) => {
                if let Some(c) = &mut client {
                    match c.delete_program(&guid).await {
                        Ok(_) => {
                            let _ = resp_tx.send(Response::Ok("Deleted".into()));
                            if let Ok(p) = c.get_all_programs().await { let _ = resp_tx.send(Response::Programs(p)); }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetScreenSchedule(entries) => {
                if let Some(c) = &mut client {
                    let refs: Vec<(&str, &str, &str)> = entries.iter()
                        .map(|(a, b, d)| (a.as_str(), b.as_str(), d.as_str()))
                        .collect();
                    match c.set_switch_time(&refs).await {
                        Ok(_) => { let _ = resp_tx.send(Response::Ok("Screen schedule applied".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetBrightnessSchedule(entries) => {
                if let Some(c) = &mut client {
                    match c.set_luminance_ploy(&entries).await {
                        Ok(_) => { let _ = resp_tx.send(Response::Ok("Brightness schedule applied".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::UploadProgram { xml, files } => {
                if let Some(c) = &mut client {
                    // Upload media files first
                    let mut upload_ok = true;
                    for (device_name, local_path) in &files {
                        match hdplayer::transfer::FileTransfer::from_file(local_path).await {
                            Ok(mut ft) => {
                                ft.filename = device_name.clone();
                                match c.upload_file(&ft, None).await {
                                    Ok(_) => {
                                        let _ = resp_tx.send(Response::Ok(
                                            format!("Uploaded: {}", device_name)));
                                    }
                                    Err(e) => {
                                        let _ = resp_tx.send(Response::Error(
                                            format!("File upload failed ({}): {}", device_name, e)));
                                        upload_ok = false;
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = resp_tx.send(Response::Error(
                                    format!("Cannot read file {}: {}", local_path.display(), e)));
                                upload_ok = false;
                                break;
                            }
                        }
                    }
                    // Upload program XML: try add first, fall back to update if add fails
                    if upload_ok {
                        let result = match c.add_program(&xml).await {
                            Ok(v) => Ok(v),
                            Err(_) => c.update_program(&xml).await,
                        };
                        match result {
                            Ok(_) => {
                                let _ = resp_tx.send(Response::Ok("Program published".into()));
                                if let Ok(p) = c.get_all_programs().await {
                                    let _ = resp_tx.send(Response::Programs(p));
                                }
                            }
                            Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                        }
                    }
                } else {
                    let _ = resp_tx.send(Response::Error("Not connected".into()));
                }
            }
            Request::UpgradeNative { file, host } => {
                let resp_prog = resp_tx.clone();
                let resp_phase = resp_tx.clone();
                let opts = hdplayer::upgrade::UpgradeOptions {
                    poll_interval: Duration::from_secs(5),
                    poll_timeout: Duration::from_secs(600),
                    progress: Some(Box::new(move |bytes, total| {
                        let _ = resp_prog.send(Response::UpgradeProgress { bytes, total });
                    })),
                    phase: Some(Box::new(move |msg: &str| {
                        let _ = resp_phase.send(Response::UpgradePhase(msg.to_string()));
                    })),
                };
                match hdplayer::upgrade::run_upgrade(&host, &file, opts).await {
                    Ok(()) => { let _ = resp_tx.send(Response::UpgradeComplete); }
                    Err(e) => { let _ = resp_tx.send(Response::Error(format!("Upgrade failed: {e}"))); }
                }
            }
        }
    }
}

// ── DRAG STATE ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragMode { Move, ResizeSE, ResizeE, ResizeS }

#[derive(Clone, Debug)]
struct DragState {
    area_idx: usize,
    mode: DragMode,
    orig: (i32, i32, i32, i32), // x,y,w,h
    start: Pos2,
}

// ── ADD CONTENT DIALOG ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum AddContentKind {
    TextSingle, TextMulti, Clock, AnalogClock,
    Neon, QrCode, Image, Video, Calendar, Countdown, Table,
    LiveStream, Modbus, Sensor, Text3D, Document,
}

// ── APP STATE ─────────────────────────────────────────────────────────────────

struct App {
    // Project
    project: Project,

    // Selection
    sel_prog: Option<usize>,
    sel_area: Option<usize>,
    sel_item: Option<usize>,

    // Canvas
    canvas_zoom: f32,
    drag: Option<DragState>,

    // Device comms — rt must be kept alive to prevent the runtime being dropped
    #[allow(dead_code)]
    rt: Arc<tokio::runtime::Runtime>,
    req_tx: tokio::sync::mpsc::Sender<Request>,
    resp_rx: mpsc::Receiver<Response>,

    // Device state
    discovered: Vec<DeviceInfo>,
    connected: bool,
    connecting: bool,
    device_info: Option<DeviceDetails>,
    dev_programs: Vec<ProgramInfo>,
    preview_texture: Option<egui::TextureHandle>,
    preview_last: Instant,
    screenshot_pending: bool,

    // Device UI
    manual_host: String,
    manual_port: String,
    brightness: u8,
    volume: u8,
    rotation: u16,

    // Dialogs
    show_new_prog: bool,
    new_prog_name: String,
    new_prog_w_s: String,
    new_prog_h_s: String,

    show_new_area: bool,
    new_area_name: String,
    new_area_x: String,
    new_area_y: String,
    new_area_w: String,
    new_area_h: String,

    show_add_content: bool,
    add_content_kind: AddContentKind,

    show_device_panel: bool,
    show_preview_window: bool,
    show_schedule_window: bool,

    // Schedule editor state
    /// Screen on/off schedule entries: (on_time "HH:MM", off_time "HH:MM", days [Mon-Sun bool×7])
    screen_sched: Vec<(String, String, [bool; 7])>,
    screen_sched_add_on: String,
    screen_sched_add_off: String,
    screen_sched_add_days: [bool; 7],
    /// Brightness schedule entries: (hour, minute, level 0-100)
    brightness_sched: Vec<(u8, u8, u8)>,
    brightness_sched_add_h: u8,
    brightness_sched_add_m: u8,
    brightness_sched_add_lvl: u8,

    // Inline item editor (double-click to open)
    show_item_editor: bool,
    item_editor_tab: u8,  // 0=Content 1=Font 2=Effects 3=Layout

    // Toast
    toast: Option<(String, Instant, bool)>,

    // Firmware upgrade window
    show_upgrade_window: bool,
    upgrade_file: String,
    upgrading: bool,
    upgrade_progress: Option<(u64, u64)>, // (bytes_sent, total_bytes)
    upgrade_phase: String,
}

impl App {
    fn new(_cc: &eframe::CreationContext) -> Self {
        let rt = Arc::new(tokio::runtime::Runtime::new().expect("tokio runtime"));
        let (req_tx, req_rx) = tokio::sync::mpsc::channel::<Request>(16);
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();

        let rt2 = rt.clone();
        std::thread::spawn(move || {
            rt2.block_on(worker_loop(req_rx, resp_tx));
        });

        Self {
            project: Project::new(128, 64),
            sel_prog: None, sel_area: None, sel_item: None,
            canvas_zoom: 4.0,
            drag: None,
            rt,
            req_tx, resp_rx,
            discovered: Vec::new(),
            connected: false, connecting: false,
            device_info: None, dev_programs: Vec::new(),
            preview_texture: None,
            preview_last: Instant::now() - Duration::from_secs(10),
            screenshot_pending: false,
            manual_host: String::new(),
            manual_port: "10001".into(),
            brightness: 100, volume: 50, rotation: 0,
            show_new_prog: false,
            new_prog_name: "New Program".into(),
            new_prog_w_s: "128".into(), new_prog_h_s: "64".into(),
            show_new_area: false,
            new_area_name: "Area".into(),
            new_area_x: "0".into(), new_area_y: "0".into(),
            new_area_w: "64".into(), new_area_h: "32".into(),
            show_add_content: false,
            add_content_kind: AddContentKind::TextSingle,
            show_device_panel: true,
            show_preview_window: false,
            show_schedule_window: false,
            screen_sched: Vec::new(),
            screen_sched_add_on: "08:00".into(),
            screen_sched_add_off: "22:00".into(),
            screen_sched_add_days: [true; 7],
            brightness_sched: Vec::new(),
            brightness_sched_add_h: 8,
            brightness_sched_add_m: 0,
            brightness_sched_add_lvl: 100,
            show_item_editor: false,
            item_editor_tab: 0,
            toast: None,
            show_upgrade_window: false,
            upgrade_file: String::new(),
            upgrading: false,
            upgrade_progress: None,
            upgrade_phase: String::new(),
        }
    }

    fn send_req(&self, req: Request) {
        if let Err(e) = self.req_tx.blocking_send(req) {
            eprintln!("send_req error: {e}");
        }
    }

    fn toast_ok(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now(), false));
    }
    fn toast_err(&mut self, msg: impl Into<String>) {
        self.toast = Some((msg.into(), Instant::now(), true));
    }

    fn handle_responses(&mut self, ctx: &egui::Context) {
        while let Ok(resp) = self.resp_rx.try_recv() {
            match resp {
                Response::Devices(devs) => {
                    self.discovered = devs;
                    self.connecting = false;
                    self.toast_ok(format!("Found {} device(s)", self.discovered.len()));
                }
                Response::Connected(info, progs) => {
                    self.brightness = info.brightness;
                    self.volume = info.volume;
                    self.rotation = info.rotation as u16;
                    self.device_info = Some(info);
                    self.dev_programs = progs;
                    self.connected = true;
                    self.connecting = false;
                    self.toast_ok("Connected");
                }
                Response::Programs(progs) => {
                    self.dev_programs = progs;
                }
                Response::Screenshot(bytes) => {
                    self.screenshot_pending = false;
                    if let Ok(img) = image::load_from_memory(&bytes) {
                        let rgba = img.to_rgba8();
                        let size = [rgba.width() as usize, rgba.height() as usize];
                        let ci = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_flat_samples().as_slice());
                        self.preview_texture = Some(ctx.load_texture("preview", ci, egui::TextureOptions::NEAREST));
                    }
                }
                Response::Ok(msg) => { self.toast_ok(msg); }
                Response::Error(msg) => {
                    self.connecting = false;
                    self.upgrading = false;
                    self.upgrade_progress = None;
                    self.upgrade_phase = String::new();
                    self.toast_err(msg);
                }
                Response::Disconnected => {
                    self.connected = false;
                    self.device_info = None;
                    self.dev_programs.clear();
                    self.preview_texture = None;
                    self.toast_ok("Disconnected");
                }
                Response::UpgradeProgress { bytes, total } => {
                    self.upgrade_progress = Some((bytes, total));
                    ctx.request_repaint();
                }
                Response::UpgradePhase(msg) => {
                    self.upgrade_phase = msg;
                    ctx.request_repaint();
                }
                Response::UpgradeComplete => {
                    self.upgrading = false;
                    self.upgrade_progress = Some((
                        self.upgrade_progress.map(|(_, t)| t).unwrap_or(0),
                        self.upgrade_progress.map(|(_, t)| t).unwrap_or(0),
                    ));
                    self.upgrade_phase = "Complete!".into();
                    self.toast_ok("Firmware upgrade complete!");
                    ctx.request_repaint();
                }
            }
        }
    }

    // ── TOOLBAR ───────────────────────────────────────────────────────────────

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        // ── Row 1: File + Publish ──────────────────────────────────────────
        ui.horizontal(|ui| {
            if ui.button("New Program").clicked() {
                self.new_prog_w_s = self.project.screen_w.to_string();
                self.new_prog_h_s = self.project.screen_h.to_string();
                self.show_new_prog = true;
            }
            if ui.button("Open…").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Program files", &["boo", "xml"]).pick_file()
                {
                    if let Ok(xml) = std::fs::read_to_string(&p) {
                        if let Some(mut proj) = parse_boo(&xml) {
                            proj.path = Some(p);
                            self.project = proj;
                            self.sel_prog = None;
                            self.sel_area = None;
                            self.sel_item = None;
                            self.toast_ok("Loaded");
                        } else {
                            self.toast_err("Failed to parse program file");
                        }
                    }
                }
            }
            if ui.button("Save…").clicked() {
                let path = self.project.path.clone().or_else(|| {
                    rfd::FileDialog::new()
                        .add_filter("Program files", &["boo", "xml"])
                        .set_file_name("program.boo")
                        .save_file()
                });
                if let Some(p) = path {
                    let xml = generate_boo(&self.project);
                    if std::fs::write(&p, &xml).is_ok() {
                        self.project.path = Some(p);
                        self.project.modified = false;
                        self.toast_ok("Saved");
                    } else {
                        self.toast_err("Save failed");
                    }
                }
            }
            ui.separator();

            // ── Publish button — always prominent ──────────────────────────
            let publish_label = if self.connected {
                RichText::new("  ▶  Publish to Screen  ").strong()
                    .color(Color32::BLACK)
            } else {
                RichText::new("  ▶  Publish to Screen  ").strong()
                    .color(Color32::from_gray(130))
            };
            let publish_btn = egui::Button::new(publish_label)
                .fill(if self.connected { Color32::from_rgb(40, 160, 60) } else { Color32::from_gray(60) });
            if ui.add_enabled(self.connected, publish_btn)
                .on_disabled_hover_text("Connect to a device first (bottom bar)")
                .clicked()
            {
                let xml = generate_boo(&self.project);
                // Collect all local media files referenced by the project
                let files = collect_media_files(&self.project);
                let missing: Vec<_> = files.iter()
                    .filter(|(_, p)| !p.exists())
                    .map(|(n, _)| n.clone())
                    .collect();
                if !missing.is_empty() {
                    self.toast_err(format!("Missing files: {}", missing.join(", ")));
                } else {
                    let n = files.len();
                    self.send_req(Request::UploadProgram { xml, files });
                    self.toast_ok(if n == 0 {
                        "Publishing program…".into()
                    } else {
                        format!("Uploading {} file(s) then publishing…", n)
                    });
                }
            }

            ui.separator();
            ui.label("Zoom:");
            ui.add(egui::Slider::new(&mut self.canvas_zoom, 1.0..=12.0).step_by(0.5));
            ui.separator();
            ui.toggle_value(&mut self.show_device_panel, "Device Panel");
            if self.connected {
                ui.toggle_value(&mut self.show_preview_window, "Live Preview");
            }
        });

        ui.separator();

        // ── Row 2: Insert content ──────────────────────────────────────────
        let can_insert = self.sel_prog.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Insert:").strong());
            ui.separator();

            let btns: &[(&str, AddContentKind)] = &[
                ("T  Text",       AddContentKind::TextSingle),
                ("T¶ Multi-Text", AddContentKind::TextMulti),
                ("🖼 Image",      AddContentKind::Image),
                ("▶ Video",       AddContentKind::Video),
                ("🕐 Clock",      AddContentKind::Clock),
                ("🕐 Analog",     AddContentKind::AnalogClock),
                ("✨ Neon",       AddContentKind::Neon),
                ("▦ QR Code",     AddContentKind::QrCode),
                ("📅 Calendar",   AddContentKind::Calendar),
                ("⏳ Countdown",  AddContentKind::Countdown),
                ("⊞ Table",       AddContentKind::Table),
                ("📡 Stream",     AddContentKind::LiveStream),
                ("⚙ Modbus",      AddContentKind::Modbus),
                ("🌡 Sensor",     AddContentKind::Sensor),
                ("3D Text",       AddContentKind::Text3D),
                ("📄 Document",   AddContentKind::Document),
            ];

            for (label, kind) in btns {
                let btn = egui::Button::new(*label);
                if ui.add_enabled(can_insert, btn)
                    .on_disabled_hover_text("Create a program first")
                    .clicked()
                {
                    self.insert_content(kind.clone());
                }
            }

            if !can_insert {
                ui.label(RichText::new("← Create a program first").color(Color32::from_gray(140)).italics());
            } else if self.sel_area.is_none() {
                ui.label(RichText::new("← Select or create an area to place content").color(Color32::from_gray(140)).italics());
            }
        });
    }

    /// Insert a content item into the selected area (auto-selects area 0 if none selected).
    fn insert_content(&mut self, kind: AddContentKind) {
        let pi = match self.sel_prog { Some(p) => p, None => return };

        // Auto-select area 0 if no area is selected
        if self.sel_area.is_none() {
            if !self.project.programs[pi].areas.is_empty() {
                self.sel_area = Some(0);
            } else {
                self.toast_err("No areas in this program — add an area first");
                return;
            }
        }
        let ai = self.sel_area.unwrap();

        let guid = new_guid();
        let item = match kind {
            AddContentKind::TextSingle  => ContentItem::new_text(guid, true),
            AddContentKind::TextMulti   => ContentItem::new_text(guid, false),
            AddContentKind::Clock       => ContentItem::new_clock(guid),
            AddContentKind::AnalogClock => {
                let mut c = ContentItem::new_clock(guid);
                if let ContentItem::Clock(ref mut cl) = c { cl.is_analog = true; }
                c
            }
            AddContentKind::Neon        => ContentItem::new_neon(guid),
            AddContentKind::QrCode      => ContentItem::new_qr(guid),
            AddContentKind::Image       => ContentItem::new_image(guid),
            AddContentKind::Video       => ContentItem::new_video(guid),
            AddContentKind::Calendar    => ContentItem::new_calendar(guid),
            AddContentKind::Countdown   => ContentItem::new_countdown(guid),
            AddContentKind::Table       => ContentItem::new_table(guid),
            AddContentKind::LiveStream  => ContentItem::new_livestream(guid),
            AddContentKind::Modbus      => ContentItem::new_modbus(guid),
            AddContentKind::Sensor      => ContentItem::new_sensor(guid),
            AddContentKind::Text3D      => ContentItem::new_text3d(guid),
            AddContentKind::Document    => ContentItem::new_document(guid),
        };
        let ii = self.project.programs[pi].areas[ai].items.len();
        self.project.programs[pi].areas[ai].items.push(item);
        self.sel_item = Some(ii);
        self.project.modified = true;
    }

    // ── PROGRAM TREE (LEFT PANEL) ─────────────────────────────────────────────

    fn render_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading("Programs");
        ui.separator();

        let prog_count = self.project.programs.len();
        for pi in 0..prog_count {
            let is_sel_prog = self.sel_prog == Some(pi);
            let prog_name = self.project.programs[pi].name.clone();
            let prog_type = self.project.programs[pi].program_type.clone();

            let label = format!("[{}] {}", if prog_type == "global" { "G" } else { "N" }, prog_name);
            let resp = ui.selectable_label(is_sel_prog, RichText::new(&label).strong());
            if resp.clicked() {
                self.sel_prog = Some(pi);
                // Auto-select first area so Insert buttons work immediately
                self.sel_area = if self.project.programs[pi].areas.is_empty() { None } else { Some(0) };
                self.sel_item = None;
            }
            resp.context_menu(|ui| {
                if ui.button("Delete Program").clicked() {
                    self.project.programs.remove(pi);
                    self.sel_prog = None;
                    self.sel_area = None;
                    self.sel_item = None;
                    self.project.modified = true;
                    ui.close_menu();
                }
                if ui.button("Duplicate").clicked() {
                    let mut dup = self.project.programs[pi].clone();
                    dup.guid = new_guid();
                    dup.name.push_str(" (copy)");
                    self.project.programs.insert(pi + 1, dup);
                    self.project.modified = true;
                    ui.close_menu();
                }
            });

            if is_sel_prog {
                let area_count = self.project.programs[pi].areas.len();
                for ai in 0..area_count {
                    let is_sel_area = self.sel_area == Some(ai);
                    let area_name = self.project.programs[pi].areas[ai].name.clone();
                    let area_rect = {
                        let a = &self.project.programs[pi].areas[ai];
                        format!("{}×{}", a.w, a.h)
                    };
                    ui.indent(format!("area_{pi}_{ai}"), |ui| {
                        let resp = ui.selectable_label(
                            is_sel_area,
                            format!("  ▸ {} [{}]", area_name, area_rect),
                        );
                        if resp.clicked() {
                            self.sel_area = Some(ai);
                            self.sel_item = None;
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Delete Area").clicked() {
                                self.project.programs[pi].areas.remove(ai);
                                match self.sel_area {
                                    Some(s) if s == ai => { self.sel_area = None; self.sel_item = None; }
                                    Some(s) if s > ai  => { self.sel_area = Some(s - 1); }
                                    _ => {}
                                }
                                self.project.modified = true;
                                ui.close_menu();
                            }
                        });

                        if is_sel_area {
                            let item_count = self.project.programs[pi].areas[ai].items.len();
                            for ii in 0..item_count {
                                let is_sel_item = self.sel_item == Some(ii);
                                let iname = format!("{} {}",
                                    self.project.programs[pi].areas[ai].items[ii].icon(),
                                    self.project.programs[pi].areas[ai].items[ii].type_name());
                                ui.indent(format!("item_{pi}_{ai}_{ii}"), |ui| {
                                    let resp = ui.selectable_label(is_sel_item, &iname);
                                    if resp.clicked() {
                                        self.sel_item = Some(ii);
                                    }
                                    if resp.double_clicked() {
                                        self.sel_item = Some(ii);
                                        self.show_item_editor = true;
                                        self.item_editor_tab = 0;
                                    }
                                    resp.context_menu(|ui| {
                                        if ui.button("Delete").clicked() {
                                            self.project.programs[pi].areas[ai].items.remove(ii);
                                            match self.sel_item {
                                                Some(s) if s == ii => { self.sel_item = None; }
                                                Some(s) if s > ii  => { self.sel_item = Some(s - 1); }
                                                _ => {}
                                            }
                                            self.project.modified = true;
                                            ui.close_menu();
                                        }
                                        if ii > 0 && ui.button("Move Up").clicked() {
                                            self.project.programs[pi].areas[ai].items.swap(ii, ii-1);
                                            ui.close_menu();
                                        }
                                        if ii + 1 < item_count && ui.button("Move Down").clicked() {
                                            self.project.programs[pi].areas[ai].items.swap(ii, ii+1);
                                            ui.close_menu();
                                        }
                                    });
                                });
                            }
                            if ui.small_button("+ Add Content").clicked() {
                                self.show_add_content = true;
                            }
                        }
                    });
                }
                if ui.small_button("+ Add Area").clicked() {
                    let sw = self.project.screen_w;
                    let sh = self.project.screen_h;
                    self.new_area_w = (sw / 2).to_string();
                    self.new_area_h = (sh / 2).to_string();
                    self.show_new_area = true;
                }
            }
        }

        ui.separator();
        if ui.button("+ New Program").clicked() {
            self.show_new_prog = true;
        }
    }

    // ── CANVAS (CENTER PANEL) ─────────────────────────────────────────────────

    fn render_canvas(&mut self, ui: &mut egui::Ui) {
        let zoom = self.canvas_zoom;
        let sw = self.project.screen_w as f32 * zoom;
        let sh = self.project.screen_h as f32 * zoom;

        let avail = ui.available_rect_before_wrap();
        let ox = (avail.min.x + (avail.width() - sw) / 2.0).max(avail.min.x + 4.0);
        let oy = (avail.min.y + (avail.height() - sh) / 2.0).max(avail.min.y + 4.0);
        let canvas_rect = Rect::from_min_size(Pos2::new(ox, oy), Vec2::new(sw, sh));

        let (resp, painter) = ui.allocate_painter(avail.size(), Sense::click_and_drag());

        // Draw checkerboard outside canvas
        painter.rect_filled(avail, 0.0, Color32::from_gray(50));
        // LED screen background
        painter.rect_filled(canvas_rect, 2.0, Color32::BLACK);
        painter.rect_stroke(canvas_rect, 2.0, Stroke::new(1.5, Color32::from_gray(100)), egui::StrokeKind::Outside);

        let to_screen = |lx: i32, ly: i32| Pos2::new(ox + lx as f32 * zoom, oy + ly as f32 * zoom);
        let to_rect = |lx: i32, ly: i32, lw: i32, lh: i32|
            Rect::from_min_size(to_screen(lx, ly), Vec2::new(lw as f32 * zoom, lh as f32 * zoom));

        // Draw areas for the selected program
        if let Some(pi) = self.sel_prog {
            if let Some(prog) = self.project.programs.get(pi) {
                for (ai, area) in prog.areas.iter().enumerate() {
                    let ar = to_rect(area.x, area.y, area.w, area.h);
                    let is_sel = self.sel_area == Some(ai);

                    // Area fill — different color per area index
                    let hue = (ai as f32 * 0.618033) % 1.0;
                    let col = egui::epaint::Hsva::new(hue, 0.6, 0.5, if is_sel { 0.5 } else { 0.3 });
                    painter.rect_filled(ar, 0.0, col);

                    // Border
                    let border_col = if is_sel { Color32::WHITE } else { Color32::from_gray(180) };
                    painter.rect_stroke(ar, 0.0, Stroke::new(if is_sel { 2.0 } else { 1.0 }, border_col), egui::StrokeKind::Outside);

                    // Area name label
                    let label_pos = Pos2::new(ar.min.x + 3.0, ar.min.y + 2.0);
                    painter.text(label_pos, egui::Align2::LEFT_TOP, &area.name,
                        egui::FontId::monospace(10.0), Color32::WHITE);

                    // Content type labels
                    for (ii, item) in area.items.iter().enumerate() {
                        let ty = format!("{} {}", item.icon(), item.type_name());
                        let iy = ar.min.y + 14.0 + ii as f32 * 12.0;
                        if iy + 12.0 < ar.max.y {
                            painter.text(Pos2::new(ar.min.x + 3.0, iy),
                                egui::Align2::LEFT_TOP, &ty,
                                egui::FontId::proportional(9.0), Color32::from_gray(220));
                        }
                    }

                    // Resize handle (SE corner) when selected
                    if is_sel {
                        let h_size = 8.0;
                        // SE
                        let se = Rect::from_min_size(
                            ar.max - Vec2::splat(h_size), Vec2::splat(h_size));
                        painter.rect_filled(se, 1.0, Color32::WHITE);
                        // E
                        let em = Pos2::new(ar.max.x - h_size, ar.center().y - h_size/2.0);
                        painter.rect_filled(Rect::from_min_size(em, Vec2::new(h_size, h_size)), 1.0, Color32::from_gray(200));
                        // S
                        let sm = Pos2::new(ar.center().x - h_size/2.0, ar.max.y - h_size);
                        painter.rect_filled(Rect::from_min_size(sm, Vec2::new(h_size, h_size)), 1.0, Color32::from_gray(200));
                    }
                }
            }
        }

        // Canvas interaction
        let pos = resp.interact_pointer_pos();
        if let Some(p) = pos {
            let from_screen = |sp: Pos2| (
                ((sp.x - ox) / zoom) as i32,
                ((sp.y - oy) / zoom) as i32,
            );

            if resp.drag_started() {
                if let Some(pi) = self.sel_prog {
                    if let Some(prog) = self.project.programs.get(pi) {
                        let (lx, ly) = from_screen(p);
                        // Check resize handles first, then move
                        for (ai, area) in prog.areas.iter().enumerate().rev() {
                            if self.sel_area != Some(ai) { continue; }
                            let ar = to_rect(area.x, area.y, area.w, area.h);
                            let h_size = 8.0;
                            let se = Rect::from_min_size(ar.max - Vec2::splat(h_size), Vec2::splat(h_size));
                            let e_r = Rect::from_min_size(Pos2::new(ar.max.x-h_size, ar.center().y-h_size/2.0), Vec2::new(h_size, h_size));
                            let s_r = Rect::from_min_size(Pos2::new(ar.center().x-h_size/2.0, ar.max.y-h_size), Vec2::new(h_size, h_size));

                            let mode = if se.contains(p) { Some(DragMode::ResizeSE) }
                                else if e_r.contains(p) { Some(DragMode::ResizeE) }
                                else if s_r.contains(p) { Some(DragMode::ResizeS) }
                                else if ar.contains(p) { Some(DragMode::Move) }
                                else { None };

                            if let Some(m) = mode {
                                self.drag = Some(DragState {
                                    area_idx: ai, mode: m,
                                    orig: (area.x, area.y, area.w, area.h), start: p,
                                });
                                break;
                            }
                        }

                        // Click to select area (no drag started)
                        if self.drag.is_none() {
                            for (ai, area) in prog.areas.iter().enumerate().rev() {
                                let ar = to_rect(area.x, area.y, area.w, area.h);
                                if ar.contains(p) {
                                    self.sel_area = Some(ai);
                                    self.sel_item = None;
                                    break;
                                }
                            }
                        }
                        let _ = lx; let _ = ly;
                    }
                }
            }

            if resp.dragged() {
                if let Some(ds) = &self.drag.clone() {
                    if let Some(prog) = self.project.programs.get_mut(self.sel_prog.unwrap_or(0)) {
                        if let Some(area) = prog.areas.get_mut(ds.area_idx) {
                            let delta = p - ds.start;
                            let dx = (delta.x / zoom) as i32;
                            let dy = (delta.y / zoom) as i32;
                            match ds.mode {
                                DragMode::Move => {
                                    area.x = (ds.orig.0 + dx).max(0).min(self.project.screen_w - area.w);
                                    area.y = (ds.orig.1 + dy).max(0).min(self.project.screen_h - area.h);
                                }
                                DragMode::ResizeSE => {
                                    area.w = (ds.orig.2 + dx).max(8).min(self.project.screen_w - area.x);
                                    area.h = (ds.orig.3 + dy).max(8).min(self.project.screen_h - area.y);
                                }
                                DragMode::ResizeE => {
                                    area.w = (ds.orig.2 + dx).max(8).min(self.project.screen_w - area.x);
                                }
                                DragMode::ResizeS => {
                                    area.h = (ds.orig.3 + dy).max(8).min(self.project.screen_h - area.y);
                                }
                            }
                            self.project.modified = true;
                        }
                    }
                }
            }

            if resp.drag_stopped() {
                self.drag = None;
            }

            // Double-click → open item editor for first item in that area
            if resp.double_clicked() {
                if let Some(pi) = self.sel_prog {
                    if let Some(prog) = self.project.programs.get(pi) {
                        for (ai, area) in prog.areas.iter().enumerate().rev() {
                            let ar = to_rect(area.x, area.y, area.w, area.h);
                            if ar.contains(p) {
                                self.sel_area = Some(ai);
                                if !area.items.is_empty() {
                                    self.sel_item = Some(0);
                                    self.show_item_editor = true;
                                    self.item_editor_tab = 0;
                                }
                                break;
                            }
                        }
                    }
                }
            }

            // Single click to select area
            if resp.clicked() && self.drag.is_none() {
                if let Some(pi) = self.sel_prog {
                    if let Some(prog) = self.project.programs.get(pi) {
                        let mut hit = false;
                        for (ai, area) in prog.areas.iter().enumerate().rev() {
                            let ar = to_rect(area.x, area.y, area.w, area.h);
                            if ar.contains(p) {
                                self.sel_area = Some(ai);
                                self.sel_item = None;
                                hit = true;
                                break;
                            }
                        }
                        if !hit {
                            self.sel_area = None;
                            self.sel_item = None;
                        }
                    }
                }
            }
        }

        // Zoom with scroll
        if resp.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                self.canvas_zoom = (self.canvas_zoom + scroll * 0.05).clamp(1.0, 16.0);
            }
        }

        // Ruler / coords overlay
        if let Some(pi) = self.sel_prog {
            if let Some(area) = self.sel_area.and_then(|ai| self.project.programs.get(pi)?.areas.get(ai)) {
                let info = format!("{} — ({},{}) {}×{}", area.name, area.x, area.y, area.w, area.h);
                painter.text(
                    Pos2::new(avail.min.x + 6.0, avail.max.y - 18.0),
                    egui::Align2::LEFT_BOTTOM, &info,
                    egui::FontId::monospace(11.0), Color32::from_rgba_premultiplied(200,200,200,200));
            }
        }
    }

    // ── PROPERTIES (RIGHT PANEL) ───────────────────────────────────────────────

    fn render_properties(&mut self, ui: &mut egui::Ui) {
        let Some(pi) = self.sel_prog else {
            ui.label("Select a program in the tree.");
            return;
        };
        let Some(prog) = self.project.programs.get_mut(pi) else { return; };

        if self.sel_area.is_none() {
            // Program properties
            ui.heading("Program");
            ui.separator();
            ui.label("Name:");
            ui.text_edit_singleline(&mut prog.name);
            ui.label("Type:");
            egui::ComboBox::from_id_salt("prog_type")
                .selected_text(&prog.program_type)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut prog.program_type, "normal".into(), "Normal");
                    ui.selectable_value(&mut prog.program_type, "global".into(), "Global");
                });
            ui.separator();
            ui.label("Play duration (s, 0=count):");
            ui.add(egui::DragValue::new(&mut prog.play_duration_secs).range(0..=3600));
            if prog.play_duration_secs == 0 {
                ui.label("Loop count:");
                ui.add(egui::DragValue::new(&mut prog.play_count).range(1..=999));
            }
            ui.separator();
            ui.label("Border:");
            egui::ComboBox::from_id_salt("border_type")
                .selected_text(BORDER_NAMES.get(prog.border_index as usize).copied().unwrap_or("?"))
                .show_ui(ui, |ui| {
                    for (i, &name) in BORDER_NAMES.iter().enumerate() {
                        ui.selectable_value(&mut prog.border_index, i as u8, name);
                    }
                });
            if prog.border_index > 0 {
                ui.label("Border speed:");
                ui.add(egui::Slider::new(&mut prog.border_speed, 1u8..=10).text(""));
            }
            ui.separator();
            // ── Playlist schedule ────────────────────────────────────────────
            ui.heading("Schedule (when to play)");
            ui.checkbox(&mut prog.disabled, "Disabled (skip this program)");
            ui.separator();
            ui.label(RichText::new("Date Range").strong());
            ui.horizontal(|ui| {
                ui.label("From:");
                ui.add(egui::TextEdit::singleline(&mut prog.date_start)
                    .desired_width(90.0).hint_text("YYYY-MM-DD"));
                ui.label("To:");
                ui.add(egui::TextEdit::singleline(&mut prog.date_end)
                    .desired_width(90.0).hint_text("YYYY-MM-DD"));
            });
            ui.label(RichText::new("Time Window").strong());
            ui.horizontal(|ui| {
                ui.label("From:");
                ui.add(egui::TextEdit::singleline(&mut prog.time_start)
                    .desired_width(60.0).hint_text("HH:MM"));
                ui.label("To:");
                ui.add(egui::TextEdit::singleline(&mut prog.time_end)
                    .desired_width(60.0).hint_text("HH:MM"));
            });
            ui.label(RichText::new("Weekdays").strong());
            let day_names = ["Mon","Tue","Wed","Thu","Fri","Sat","Sun"];
            ui.horizontal(|ui| {
                for (i, &n) in day_names.iter().enumerate() {
                    ui.checkbox(&mut prog.week_filter[i], n);
                }
            });
            ui.label(RichText::new("(Leave date/time empty for no restriction)").italics().color(Color32::from_gray(140)));
            return;
        }

        let ai = self.sel_area.unwrap();
        let Some(area) = prog.areas.get_mut(ai) else { return; };

        if self.sel_item.is_none() {
            // Area properties
            ui.heading("Area");
            ui.separator();
            ui.label("Name:"); ui.text_edit_singleline(&mut area.name);
            ui.label("Alpha:"); ui.add(egui::Slider::new(&mut area.alpha, 0u8..=255));
            ui.separator();
            ui.label("Position / Size:");
            ui.horizontal(|ui| {
                ui.label("X:"); ui.add(egui::DragValue::new(&mut area.x).range(0..=4096));
                ui.label("Y:"); ui.add(egui::DragValue::new(&mut area.y).range(0..=4096));
            });
            ui.horizontal(|ui| {
                ui.label("W:"); ui.add(egui::DragValue::new(&mut area.w).range(1..=4096));
                ui.label("H:"); ui.add(egui::DragValue::new(&mut area.h).range(1..=4096));
            });
            ui.separator();
            if ui.button("+ Add Content Item").clicked() {
                self.show_add_content = true;
            }
            return;
        }

        let ii = self.sel_item.unwrap();
        let Some(item) = area.items.get_mut(ii) else { return; };

        ui.heading(item.type_name());
        ui.separator();

        match item {
            ContentItem::Text(t) => render_text_props(ui, t),
            ContentItem::Image(im) => render_image_props(ui, im),
            ContentItem::Video(v) => render_video_props(ui, v),
            ContentItem::Clock(c) => render_clock_props(ui, c),
            ContentItem::Neon(n) => render_neon_props(ui, n),
            ContentItem::QrCode(q) => render_qr_props(ui, q),
            ContentItem::Calendar(c) => render_calendar_props(ui, c),
            ContentItem::Countdown(c) => render_countdown_props(ui, c),
            ContentItem::Table(t) => render_table_props(ui, t),
            ContentItem::LiveStream(ls) => render_livestream_props(ui, ls),
            ContentItem::Modbus(mb) => render_modbus_props(ui, mb),
            ContentItem::Sensor(sn) => render_sensor_props(ui, sn),
            ContentItem::Text3D(t3) => render_text3d_props(ui, t3),
            ContentItem::Document(dc) => render_document_props(ui, dc),
        }
    }

    // ── DEVICE PANEL (BOTTOM) ─────────────────────────────────────────────────

    fn render_device_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // Connection status
            if self.connected {
                ui.label(RichText::new("● CONNECTED").color(Color32::GREEN).strong());
                if let Some(info) = &self.device_info {
                    ui.label(format!("{} @ {} — {}×{} fw:{}",
                        info.device_name, info.ip_address,
                        info.screen_width, info.screen_height,
                        info.firmware_version));
                }
                ui.separator();
                ui.label("Brightness:");
                if ui.add(egui::Slider::new(&mut self.brightness, 0u8..=100).suffix("%")).drag_stopped() {
                    let v = self.brightness;
                    self.send_req(Request::SetBrightness(v));
                }
                ui.label("Vol:");
                if ui.add(egui::Slider::new(&mut self.volume, 0u8..=100).suffix("%")).drag_stopped() {
                    let v = self.volume;
                    self.send_req(Request::SetVolume(v));
                }
                ui.label("Rot:");
                for &angle in &[0u16, 90, 180, 270] {
                    let selected = self.rotation == angle;
                    if ui.selectable_label(selected, format!("{}°", angle)).clicked() && !selected {
                        self.rotation = angle;
                        self.send_req(Request::SetRotation(angle));
                    }
                }
                ui.separator();
                if ui.button("ON").clicked() { self.send_req(Request::ScreenOn); }
                if ui.button("OFF").clicked() { self.send_req(Request::ScreenOff); }
                if ui.button("Reboot").clicked() { self.send_req(Request::Reboot); }
                if ui.button("Sync Time").clicked() { self.send_req(Request::SyncTime); }
                ui.toggle_value(&mut self.show_schedule_window, "Schedules");
                ui.toggle_value(&mut self.show_upgrade_window, "Firmware");
                ui.separator();
                if ui.button("Disconnect").clicked() { self.send_req(Request::Disconnect); }
            } else {
                ui.label(RichText::new("○ OFFLINE").color(Color32::GRAY));
                ui.separator();
                if self.connecting {
                    ui.spinner();
                    ui.label("Scanning…");
                } else {
                    if ui.button("Scan Network").clicked() {
                        self.connecting = true;
                        self.send_req(Request::Discover);
                    }
                    ui.label("Host:");
                    ui.add(egui::TextEdit::singleline(&mut self.manual_host).desired_width(120.0).hint_text("192.168.1.x"));
                    ui.label("Port:");
                    ui.add(egui::TextEdit::singleline(&mut self.manual_port).desired_width(55.0));
                    if ui.button("Connect").clicked() && !self.manual_host.is_empty() {
                        let host = self.manual_host.clone();
                        let port = self.manual_port.parse().unwrap_or(10001);
                        self.connecting = true;
                        self.send_req(Request::Connect { host, port });
                    }
                    // Discovered devices dropdown
                    if !self.discovered.is_empty() {
                        egui::ComboBox::from_id_salt("disc_devices")
                            .selected_text("Discovered devices")
                            .show_ui(ui, |ui| {
                                let devs = self.discovered.clone();
                                for dev in &devs {
                                    let label = format!("{} @ {}", dev.name, dev.addr);
                                    if ui.button(&label).clicked() {
                                        self.manual_host = dev.addr.to_string();
                                        self.connecting = true;
                                        self.send_req(Request::Connect {
                                            host: dev.addr.to_string(),
                                            port: 10001,
                                        });
                                    }
                                }
                            });
                    }
                }
            }

            // Toast area (right side)
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some((msg, t, is_err)) = &self.toast {
                    if t.elapsed() < Duration::from_secs(4) {
                        let col = if *is_err { Color32::from_rgb(255,80,80) } else { Color32::from_rgb(100,220,100) };
                        ui.label(RichText::new(msg).color(col));
                    } else {
                        self.toast = None;
                    }
                }
            });
        });
    }

    // ── FIRMWARE UPGRADE WINDOW ───────────────────────────────────────────────

    fn render_upgrade_window(&mut self, ctx: &egui::Context) {
        if !self.show_upgrade_window { return; }

        let mut open = self.show_upgrade_window;
        egui::Window::new("Firmware Upgrade")
            .id(egui::Id::new("upgrade_window"))
            .default_size([480.0, 220.0])
            .resizable(false)
            .collapsible(false)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label("Upload Huidu firmware (.zbin) via native port-9528 protocol.");
                ui.separator();

                // File picker row
                ui.horizontal(|ui| {
                    ui.label("File:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.upgrade_file)
                            .desired_width(300.0)
                            .hint_text("BoxPlayer_Vx.x.x_MagicPlayer_Vx.x.x.zbin…"),
                    );
                    if ui.button("Browse…").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Firmware archive", &["gz", "tar", "bin", "zbin"])
                            .pick_file()
                        {
                            self.upgrade_file = p.to_string_lossy().to_string();
                        }
                    }
                });

                ui.add_space(6.0);

                // Progress bar — shown while upgrading or after completion
                if let Some((bytes, total)) = self.upgrade_progress {
                    let pct = if total > 0 { bytes as f32 / total as f32 } else { 0.0 };
                    let label = if total > 0 {
                        format!(
                            "{:.1} MB / {:.1} MB  ({:.1}%)",
                            bytes as f64 / 1_000_000.0,
                            total as f64 / 1_000_000.0,
                            pct * 100.0,
                        )
                    } else {
                        String::new()
                    };
                    ui.add(egui::ProgressBar::new(pct).text(label).animate(self.upgrading));
                    ui.add_space(4.0);
                }

                // Phase label
                if !self.upgrade_phase.is_empty() {
                    let col = if self.upgrade_phase == "Complete!" {
                        Color32::from_rgb(100, 220, 100)
                    } else if self.upgrading {
                        Color32::from_rgb(180, 180, 255)
                    } else {
                        Color32::GRAY
                    };
                    ui.label(RichText::new(&self.upgrade_phase).color(col));
                    ui.add_space(4.0);
                }

                ui.separator();

                // Start / spinner row
                ui.horizontal(|ui| {
                    let can_start = !self.upgrading
                        && !self.upgrade_file.is_empty()
                        && self.connected;

                    let btn = egui::Button::new(
                        RichText::new("Start Upgrade").color(Color32::WHITE)
                    )
                    .fill(Color32::from_rgb(30, 120, 30));

                    if ui.add_enabled(can_start, btn).clicked() {
                        let host = self.device_info
                            .as_ref()
                            .map(|i| i.ip_address.clone())
                            .unwrap_or_else(|| self.manual_host.clone());
                        self.upgrading = true;
                        self.upgrade_progress = Some((0, 0));
                        self.upgrade_phase = "Starting…".into();
                        self.send_req(Request::UpgradeNative {
                            file: PathBuf::from(&self.upgrade_file),
                            host,
                        });
                    }

                    if self.upgrading {
                        ui.spinner();
                        ui.label("Upgrade in progress — do not disconnect");
                    }

                    if !self.upgrading && self.upgrade_phase == "Complete!" {
                        if ui.button("Reset").clicked() {
                            self.upgrade_progress = None;
                            self.upgrade_phase = String::new();
                        }
                    }
                });
            });
        self.show_upgrade_window = open;
    }

    // ── DEVICE PROGRAMS SIDE WINDOW ───────────────────────────────────────────

    fn render_dev_programs(&mut self, ui: &mut egui::Ui) {
        ui.heading("Device Programs");
        ui.separator();
        if ui.small_button("Refresh").clicked() { self.send_req(Request::RefreshPrograms); }
        let progs = self.dev_programs.clone();
        for prog in &progs {
            ui.horizontal(|ui| {
                if prog.is_current { ui.label(RichText::new("▶").color(Color32::GREEN)); }
                else { ui.label("  "); }
                ui.label(&prog.name);
                if ui.small_button("Switch").clicked() { self.send_req(Request::SwitchProgram(prog.guid.clone())); }
                if ui.small_button("Delete").clicked() { self.send_req(Request::DeleteProgram(prog.guid.clone())); }
            });
        }
    }

    // ── ITEM EDITOR WINDOW ────────────────────────────────────────────────────

    fn render_item_editor(&mut self, ctx: &egui::Context) {
        if !self.show_item_editor { return; }
        let (pi, ai, ii) = match (self.sel_prog, self.sel_area, self.sel_item) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => { self.show_item_editor = false; return; }
        };

        // Extract what we need before the window (avoids borrow issues)
        let type_name = self.project.programs.get(pi)
            .and_then(|p| p.areas.get(ai))
            .and_then(|a| a.items.get(ii))
            .map(|i| i.type_name())
            .unwrap_or("Item");
        let title = format!("Edit — {}  (press X to close)", type_name);

        // item_editor_tab lives in self — extract before window
        let mut tab = self.item_editor_tab;

        let result = egui::Window::new(&title)
            .id(egui::Id::new("item_editor"))
            .default_size([480.0, 520.0])
            .resizable(true)
            .collapsible(false)
            .open(&mut self.show_item_editor)
            .show(ctx, |ui| {
                // Get item mutably inside the closure
                let item = match self.project.programs.get_mut(pi)
                    .and_then(|p| p.areas.get_mut(ai))
                    .and_then(|a| a.items.get_mut(ii))
                {
                    Some(i) => i,
                    None => return,
                };

                // Tab bar — only Text gets tabs; other types fill the whole window
                if matches!(item, ContentItem::Text(_)) {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut tab, 0, "✏  Content");
                        ui.selectable_value(&mut tab, 1, "A  Font");
                        ui.selectable_value(&mut tab, 2, "★  Effects");
                        ui.selectable_value(&mut tab, 3, "⊞  Layout");
                    });
                    ui.separator();
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    match item {
                        ContentItem::Text(t) => match tab {
                            0 => {
                                // ── Content ──────────────────────────────────
                                ui.label(RichText::new("Text:").strong());
                                if t.single_line {
                                    ui.add(egui::TextEdit::singleline(&mut t.text)
                                        .desired_width(f32::INFINITY)
                                        .font(egui::FontId::proportional(16.0)));
                                } else {
                                    ui.add(egui::TextEdit::multiline(&mut t.text)
                                        .desired_width(f32::INFINITY)
                                        .desired_rows(8)
                                        .font(egui::FontId::proportional(15.0)));
                                }
                                ui.add_space(4.0);
                                ui.checkbox(&mut t.single_line, "Single line / ticker");
                                ui.checkbox(&mut t.word_wrap, "Word wrap (multi-line only)");
                            }
                            1 => {
                                // ── Font ─────────────────────────────────────
                                egui::Grid::new("font_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                                    ui.label("Family:");
                                    ui.add(egui::TextEdit::singleline(&mut t.font_name)
                                        .hint_text("(system default)").desired_width(180.0));
                                    ui.end_row();
                                    ui.label("Size:");
                                    ui.add(egui::Slider::new(&mut t.font_size, 4u32..=200).suffix("pt"));
                                    ui.end_row();
                                    ui.label("Style:");
                                    ui.horizontal(|ui| {
                                        ui.checkbox(&mut t.bold, RichText::new("Bold").strong());
                                        ui.checkbox(&mut t.italic, RichText::new("Italic").italics());
                                        ui.checkbox(&mut t.underline, "Underline");
                                    });
                                    ui.end_row();
                                    ui.label("Color:");
                                    let mut c = to_c32(t.color);
                                    egui::color_picker::color_edit_button_srgba(ui, &mut c, egui::color_picker::Alpha::Opaque);
                                    t.color = from_c32(c);
                                    ui.end_row();
                                });
                                ui.separator();
                                let mut has_bg = t.background.is_some();
                                if ui.checkbox(&mut has_bg, "Background fill").changed() {
                                    t.background = if has_bg { Some([0,0,0]) } else { None };
                                }
                                if let Some(bg) = &mut t.background {
                                    color_edit(ui, "Background:", bg);
                                }
                            }
                            2 => {
                                // ── Effects ──────────────────────────────────
                                egui::Grid::new("fx_grid").num_columns(2).spacing([12.0, 8.0]).show(ui, |ui| {
                                    ui.label(RichText::new("Enter effect:").strong());
                                    let in_idx = (t.effect_in as usize).min(EFFECT_NAMES.len()-1);
                                    egui::ComboBox::from_id_salt("eff_in")
                                        .selected_text(EFFECT_NAMES[in_idx])
                                        .show_ui(ui, |ui| {
                                            for (i, &n) in EFFECT_NAMES.iter().enumerate() {
                                                ui.selectable_value(&mut t.effect_in, i as u32, n);
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("  Enter speed:");
                                    ui.add(egui::Slider::new(&mut t.effect_in_speed, 1u32..=10).text(""));
                                    ui.end_row();

                                    ui.label(RichText::new("Exit effect:").strong());
                                    let out_idx = (t.effect_out as usize).min(EFFECT_NAMES.len()-1);
                                    egui::ComboBox::from_id_salt("eff_out")
                                        .selected_text(EFFECT_NAMES[out_idx])
                                        .show_ui(ui, |ui| {
                                            for (i, &n) in EFFECT_NAMES.iter().enumerate() {
                                                ui.selectable_value(&mut t.effect_out, i as u32, n);
                                            }
                                        });
                                    ui.end_row();
                                    ui.label("  Exit speed:");
                                    ui.add(egui::Slider::new(&mut t.effect_out_speed, 1u32..=10).text(""));
                                    ui.end_row();

                                    ui.label("Hold time:");
                                    ui.add(egui::DragValue::new(&mut t.duration_tenths)
                                        .range(0..=9999).suffix(" ×0.1s"));
                                    ui.end_row();
                                });
                                ui.separator();
                                ui.label(RichText::new("Scroll direction:").strong());
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut t.scroll_dir, 0, "None");
                                    ui.selectable_value(&mut t.scroll_dir, 1, "← Left");
                                    ui.selectable_value(&mut t.scroll_dir, 2, "→ Right");
                                    ui.selectable_value(&mut t.scroll_dir, 3, "↑ Up");
                                    ui.selectable_value(&mut t.scroll_dir, 4, "↓ Down");
                                });
                                if t.scroll_dir > 0 {
                                    ui.add(egui::Slider::new(&mut t.scroll_speed, 1u32..=200)
                                        .text("Speed (px/s)"));
                                }
                            }
                            _ => {
                                // ── Layout ───────────────────────────────────
                                ui.label(RichText::new("Horizontal alignment:").strong());
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut t.align, 0, "⬅ Left");
                                    ui.selectable_value(&mut t.align, 1, "⬛ Center");
                                    ui.selectable_value(&mut t.align, 2, "➡ Right");
                                });
                                ui.add_space(8.0);
                                ui.label(RichText::new("Vertical alignment:").strong());
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut t.valign, 0, "⬆ Top");
                                    ui.selectable_value(&mut t.valign, 1, "⬛ Middle");
                                    ui.selectable_value(&mut t.valign, 2, "⬇ Bottom");
                                });
                            }
                        },
                        ContentItem::Clock(c)       => render_clock_props(ui, c),
                        ContentItem::Neon(n)        => render_neon_props(ui, n),
                        ContentItem::QrCode(q)      => render_qr_props(ui, q),
                        ContentItem::Image(im)      => render_image_props(ui, im),
                        ContentItem::Video(v)       => render_video_props(ui, v),
                        ContentItem::Calendar(c)    => render_calendar_props(ui, c),
                        ContentItem::Countdown(c)   => render_countdown_props(ui, c),
                        ContentItem::Table(t)       => render_table_props(ui, t),
                        ContentItem::LiveStream(ls) => render_livestream_props(ui, ls),
                        ContentItem::Modbus(mb)     => render_modbus_props(ui, mb),
                        ContentItem::Sensor(sn)     => render_sensor_props(ui, sn),
                        ContentItem::Text3D(t3)     => render_text3d_props(ui, t3),
                        ContentItem::Document(dc)   => render_document_props(ui, dc),
                    }
                });
            });

        // Persist the tab selection back (extracted before window to avoid borrow conflict)
        self.item_editor_tab = tab;
        // If window was closed via X button, result inner will be None
        if result.is_none() { self.show_item_editor = false; }
    }

    // ── DIALOGS ───────────────────────────────────────────────────────────────

    fn render_dialogs(&mut self, ctx: &egui::Context) {
        // New Program dialog
        if self.show_new_prog {
            let mut open = true;
            egui::Window::new("New Program")
                .collapsible(false).resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Name:"); ui.text_edit_singleline(&mut self.new_prog_name);
                    ui.horizontal(|ui| {
                        ui.label("Screen W:"); ui.add(egui::TextEdit::singleline(&mut self.new_prog_w_s).desired_width(60.0));
                        ui.label("× H:"); ui.add(egui::TextEdit::singleline(&mut self.new_prog_h_s).desired_width(60.0));
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            let w = self.new_prog_w_s.parse().unwrap_or(128i32).max(1);
                            let h = self.new_prog_h_s.parse().unwrap_or(64i32).max(1);
                            if self.project.programs.is_empty() {
                                self.project.screen_w = w;
                                self.project.screen_h = h;
                            }
                            let prog = Program::new(new_guid(), self.new_prog_name.clone(), w, h);
                            let idx = self.project.programs.len();
                            self.project.programs.push(prog);
                            self.sel_prog = Some(idx);
                            self.sel_area = None;
                            self.project.modified = true;
                            self.show_new_prog = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_new_prog = false; }
                    });
                });
            if !open { self.show_new_prog = false; }
        }

        // New Area dialog
        if self.show_new_area {
            let mut open = true;
            egui::Window::new("New Area")
                .collapsible(false).resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Name:"); ui.text_edit_singleline(&mut self.new_area_name);
                    ui.horizontal(|ui| {
                        ui.label("X:"); ui.add(egui::TextEdit::singleline(&mut self.new_area_x).desired_width(50.0));
                        ui.label("Y:"); ui.add(egui::TextEdit::singleline(&mut self.new_area_y).desired_width(50.0));
                    });
                    ui.horizontal(|ui| {
                        ui.label("W:"); ui.add(egui::TextEdit::singleline(&mut self.new_area_w).desired_width(50.0));
                        ui.label("H:"); ui.add(egui::TextEdit::singleline(&mut self.new_area_h).desired_width(50.0));
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            if let Some(pi) = self.sel_prog {
                                let x = self.new_area_x.parse().unwrap_or(0);
                                let y = self.new_area_y.parse().unwrap_or(0);
                                let w = self.new_area_w.parse().unwrap_or(64i32).max(1);
                                let h = self.new_area_h.parse().unwrap_or(32i32).max(1);
                                let ai = self.project.programs[pi].areas.len();
                                self.project.programs[pi].areas.push(Area::new(new_guid(), self.new_area_name.clone(), x, y, w, h));
                                self.sel_area = Some(ai);
                                self.project.modified = true;
                            }
                            self.show_new_area = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_new_area = false; }
                    });
                });
            if !open { self.show_new_area = false; }
        }

        // Add Content dialog
        if self.show_add_content {
            let mut open = true;
            egui::Window::new("Add Content")
                .collapsible(false).resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Content type:");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::TextSingle, "Single-Line Text");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::TextMulti, "Multi-Line Text");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Clock, "Digital Clock");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::AnalogClock, "Analog Clock");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Neon, "Neon Shape");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::QrCode, "QR Code");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Image, "Image");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Video, "Video");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Calendar, "Calendar");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Countdown, "Countdown");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Table, "Table");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::LiveStream, "Live Stream (RTSP/RTMP)");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Modbus, "Modbus Data");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Sensor, "Sensor (CPU temp / DS18B20 / DHT22)");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Text3D, "3D Text");
                    ui.radio_value(&mut self.add_content_kind, AddContentKind::Document, "Document / Presentation");
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Add").clicked() {
                            if let (Some(pi), Some(ai)) = (self.sel_prog, self.sel_area) {
                                let guid = new_guid();
                                let item = match self.add_content_kind {
                                    AddContentKind::TextSingle => ContentItem::new_text(guid, true),
                                    AddContentKind::TextMulti => ContentItem::new_text(guid, false),
                                    AddContentKind::Clock => ContentItem::new_clock(guid),
                                    AddContentKind::AnalogClock => {
                                        let mut c = ContentItem::new_clock(guid);
                                        if let ContentItem::Clock(ref mut cl) = c { cl.is_analog = true; }
                                        c
                                    }
                                    AddContentKind::Neon => ContentItem::new_neon(guid),
                                    AddContentKind::QrCode => ContentItem::new_qr(guid),
                                    AddContentKind::Image => ContentItem::new_image(guid),
                                    AddContentKind::Video => ContentItem::new_video(guid),
                                    AddContentKind::Calendar => ContentItem::new_calendar(guid),
                                    AddContentKind::Countdown => ContentItem::new_countdown(guid),
                                    AddContentKind::Table => ContentItem::new_table(guid),
                                    AddContentKind::LiveStream => ContentItem::new_livestream(guid),
                                    AddContentKind::Modbus     => ContentItem::new_modbus(guid),
                                    AddContentKind::Sensor     => ContentItem::new_sensor(guid),
                                    AddContentKind::Text3D     => ContentItem::new_text3d(guid),
                                    AddContentKind::Document   => ContentItem::new_document(guid),
                                };
                                let ii = self.project.programs[pi].areas[ai].items.len();
                                self.project.programs[pi].areas[ai].items.push(item);
                                self.sel_item = Some(ii);
                                self.project.modified = true;
                            }
                            self.show_add_content = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_add_content = false; }
                    });
                });
            if !open { self.show_add_content = false; }
        }

        // ── Item editor window (double-click to open, X to close) ──────────
        self.render_item_editor(ctx);

        // ── Schedule Editor window ─────────────────────────────────────────────
        if self.show_schedule_window && self.connected {
            let mut open = true;
            egui::Window::new("Device Schedules")
                .id(egui::Id::new("schedule_window"))
                .default_size([460.0, 520.0])
                .resizable(true)
                .collapsible(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    // ── Screen On/Off Schedule ───────────────────────────────
                    ui.heading("Screen On/Off Schedule");
                    ui.label("Each entry turns the screen on and off at set times on selected days.");
                    ui.separator();

                    let day_names = ["Mon","Tue","Wed","Thu","Fri","Sat","Sun"];
                    let sched = self.screen_sched.clone();
                    let mut remove_idx: Option<usize> = None;
                    for (i, (on, off, days)) in sched.iter().enumerate() {
                        let days_str: String = days.iter().zip(day_names.iter())
                            .filter(|(&d, _)| d)
                            .map(|(_, n)| *n)
                            .collect::<Vec<_>>().join(" ");
                        ui.horizontal(|ui| {
                            ui.label(format!("ON {on}  OFF {off}  — {days_str}"));
                            if ui.small_button("✕").clicked() { remove_idx = Some(i); }
                        });
                    }
                    if let Some(i) = remove_idx { self.screen_sched.remove(i); }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("ON:");
                        ui.add(egui::TextEdit::singleline(&mut self.screen_sched_add_on)
                            .desired_width(55.0).hint_text("08:00"));
                        ui.label("OFF:");
                        ui.add(egui::TextEdit::singleline(&mut self.screen_sched_add_off)
                            .desired_width(55.0).hint_text("22:00"));
                    });
                    ui.horizontal(|ui| {
                        for (i, &name) in day_names.iter().enumerate() {
                            ui.checkbox(&mut self.screen_sched_add_days[i], name);
                        }
                    });
                    if ui.button("+ Add Entry").clicked() {
                        let bits: String = self.screen_sched_add_days.iter()
                            .map(|&b| if b { '1' } else { '0' }).collect();
                        self.screen_sched.push((
                            self.screen_sched_add_on.clone(),
                            self.screen_sched_add_off.clone(),
                            self.screen_sched_add_days,
                        ));
                        let _ = bits;
                    }
                    ui.horizontal(|ui| {
                        let apply_btn = egui::Button::new(RichText::new("Apply to Device").strong())
                            .fill(Color32::from_rgb(40,120,200));
                        if ui.add(apply_btn).clicked() {
                            let entries: Vec<(String, String, String)> = self.screen_sched.iter()
                                .map(|(on, off, days)| {
                                    let bits: String = days.iter().map(|&b| if b {'1'} else {'0'}).collect();
                                    (on.clone(), off.clone(), bits)
                                }).collect();
                            self.send_req(Request::SetScreenSchedule(entries));
                        }
                        if ui.button("Clear All").clicked() {
                            self.screen_sched.clear();
                            self.send_req(Request::SetScreenSchedule(Vec::new()));
                        }
                    });

                    ui.add_space(12.0);
                    ui.separator();

                    // ── Brightness Schedule ──────────────────────────────────
                    ui.heading("Brightness Schedule");
                    ui.label("Set brightness level at specific times of day.");
                    ui.separator();

                    let bsched = self.brightness_sched.clone();
                    let mut bremove: Option<usize> = None;
                    for (i, &(h, m, lvl)) in bsched.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{h:02}:{m:02} → {lvl}%"));
                            if ui.small_button("✕").clicked() { bremove = Some(i); }
                        });
                    }
                    if let Some(i) = bremove { self.brightness_sched.remove(i); }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Time:");
                        ui.add(egui::DragValue::new(&mut self.brightness_sched_add_h).range(0..=23).suffix("h"));
                        ui.add(egui::DragValue::new(&mut self.brightness_sched_add_m).range(0..=59).suffix("m"));
                        ui.label("Level:");
                        ui.add(egui::Slider::new(&mut self.brightness_sched_add_lvl, 0u8..=100).suffix("%"));
                        if ui.button("+ Add").clicked() {
                            self.brightness_sched.push((
                                self.brightness_sched_add_h,
                                self.brightness_sched_add_m,
                                self.brightness_sched_add_lvl,
                            ));
                        }
                    });
                    ui.horizontal(|ui| {
                        let apply_btn = egui::Button::new(RichText::new("Apply to Device").strong())
                            .fill(Color32::from_rgb(40,120,200));
                        if ui.add(apply_btn).clicked() {
                            self.send_req(Request::SetBrightnessSchedule(self.brightness_sched.clone()));
                        }
                        if ui.button("Clear All").clicked() {
                            self.brightness_sched.clear();
                            self.send_req(Request::SetBrightnessSchedule(Vec::new()));
                        }
                    });
                });
            if !open { self.show_schedule_window = false; }
        }

        // Live Preview window
        if self.show_preview_window && self.connected {
            let mut open = true;
            egui::Window::new("Live Preview")
                .resizable(true).default_size([400.0, 240.0])
                .open(&mut open)
                .show(ctx, |ui| {
                    if let Some(tex) = &self.preview_texture {
                        let size = tex.size_vec2();
                        let avail = ui.available_size();
                        let scale = (avail.x / size.x).min(avail.y / size.y).min(1.0);
                        let display = size * scale;
                        ui.image((tex.id(), display));
                    } else {
                        ui.label("No preview yet…");
                    }
                    if self.screenshot_pending {
                        ui.spinner();
                    }
                });
            if !open { self.show_preview_window = false; }
        }
    }
}

// ── PROPERTY SUB-PANELS ───────────────────────────────────────────────────────

fn color_edit(ui: &mut egui::Ui, label: &str, rgb: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut c = to_c32(*rgb);
        egui::color_picker::color_edit_button_srgba(
            ui, &mut c, egui::color_picker::Alpha::Opaque);
        *rgb = from_c32(c);
    });
}

fn effect_combo(ui: &mut egui::Ui, label: &str, val: &mut u32) {
    let idx = (*val as usize).min(EFFECT_NAMES.len() - 1);
    egui::ComboBox::from_label(label)
        .selected_text(EFFECT_NAMES[idx])
        .show_ui(ui, |ui| {
            for (i, &name) in EFFECT_NAMES.iter().enumerate() {
                ui.selectable_value(val, i as u32, name);
            }
        });
}

fn render_text_props(ui: &mut egui::Ui, t: &mut TextItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label("Text content:");
        if t.single_line {
            ui.text_edit_singleline(&mut t.text);
        } else {
            ui.add(egui::TextEdit::multiline(&mut t.text).desired_rows(4));
        }
        ui.checkbox(&mut t.single_line, "Single line");
        ui.separator();
        ui.label("Font:");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut t.font_name);
            ui.add(egui::DragValue::new(&mut t.font_size).range(4..=200).prefix("pt "));
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut t.bold, "B");
            ui.checkbox(&mut t.italic, "I");
            ui.checkbox(&mut t.underline, "U");
        });
        color_edit(ui, "Color:", &mut t.color);
        ui.checkbox(&mut t.word_wrap, "Word wrap");
        ui.separator();
        ui.label("Alignment:");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut t.align, 0, "Left");
            ui.selectable_value(&mut t.align, 1, "Center");
            ui.selectable_value(&mut t.align, 2, "Right");
        });
        ui.horizontal(|ui| {
            ui.selectable_value(&mut t.valign, 0, "Top");
            ui.selectable_value(&mut t.valign, 1, "Middle");
            ui.selectable_value(&mut t.valign, 2, "Bottom");
        });
        ui.separator();
        ui.label("Effects:");
        effect_combo(ui, "In", &mut t.effect_in);
        ui.add(egui::Slider::new(&mut t.effect_in_speed, 1u32..=10).text("In speed"));
        effect_combo(ui, "Out", &mut t.effect_out);
        ui.add(egui::Slider::new(&mut t.effect_out_speed, 1u32..=10).text("Out speed"));
        ui.label("Duration (tenths of s):");
        ui.add(egui::DragValue::new(&mut t.duration_tenths).range(0..=9999));
        ui.separator();
        ui.label("Scroll direction:");
        ui.horizontal(|ui| {
            ui.selectable_value(&mut t.scroll_dir, 0, "None");
            ui.selectable_value(&mut t.scroll_dir, 1, "←");
            ui.selectable_value(&mut t.scroll_dir, 2, "→");
            ui.selectable_value(&mut t.scroll_dir, 3, "↑");
            ui.selectable_value(&mut t.scroll_dir, 4, "↓");
        });
        if t.scroll_dir > 0 {
            ui.add(egui::Slider::new(&mut t.scroll_speed, 1u32..=200).text("Speed"));
        }
        ui.separator();
        ui.label("Background (optional):");
        let mut has_bg = t.background.is_some();
        if ui.checkbox(&mut has_bg, "Enable background").changed() {
            t.background = if has_bg { Some([0,0,0]) } else { None };
        }
        if let Some(bg) = &mut t.background { color_edit(ui, "Bg color:", bg); }
    });
}

fn render_image_props(ui: &mut egui::Ui, im: &mut ImageItem) {
    let path_str = im.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
    ui.label(format!("File: {}", path_str));
    if ui.button("Browse…").clicked() {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Images", &["png","jpg","jpeg","bmp","gif"]).pick_file()
        { im.path = Some(p); }
    }
    ui.separator();
    ui.label("Fit mode:");
    ui.horizontal(|ui| {
        ui.selectable_value(&mut im.fit, 0, "Stretch");
        ui.selectable_value(&mut im.fit, 1, "Fill");
        ui.selectable_value(&mut im.fit, 2, "Center");
        ui.selectable_value(&mut im.fit, 3, "Fit");
    });
}

fn render_video_props(ui: &mut egui::Ui, v: &mut VideoItem) {
    let path_str = v.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
    ui.label(format!("File: {}", path_str));
    if ui.button("Browse…").clicked() {
        if let Some(p) = rfd::FileDialog::new()
            .add_filter("Videos", &["mp4","avi","mkv","mov","flv"]).pick_file()
        { v.path = Some(p); }
    }
    ui.checkbox(&mut v.keep_aspect, "Keep aspect ratio");
}

fn render_clock_props(ui: &mut egui::Ui, c: &mut ClockItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut c.is_analog, false, "Digital");
            ui.selectable_value(&mut c.is_analog, true, "Analog");
        });
        ui.label("Timezone (e.g. +08:00):"); ui.text_edit_singleline(&mut c.timezone);
        ui.add(egui::DragValue::new(&mut c.font_size).range(6..=200).prefix("Font size: "));
        ui.separator();
        if !c.is_analog {
            ui.checkbox(&mut c.show_title, "Show title");
            if c.show_title { ui.text_edit_singleline(&mut c.title_text); color_edit(ui, "Title color:", &mut c.title_color); }
            ui.checkbox(&mut c.show_date, "Show date");
            if c.show_date { color_edit(ui, "Date color:", &mut c.date_color); }
            ui.checkbox(&mut c.show_week, "Show weekday");
            if c.show_week { color_edit(ui, "Week color:", &mut c.week_color); }
            ui.checkbox(&mut c.show_time, "Show time");
            if c.show_time { color_edit(ui, "Time color:", &mut c.time_color); }
            ui.checkbox(&mut c.show_lunar, "Lunar calendar");
            if c.show_lunar { color_edit(ui, "Lunar color:", &mut c.lunar_color); }
        } else {
            color_edit(ui, "Dial color:", &mut c.dial_color);
            color_edit(ui, "Hand color:", &mut c.hand_color);
            color_edit(ui, "Second hand:", &mut c.second_color);
        }
    });
}

fn render_neon_props(ui: &mut egui::Ui, n: &mut NeonItem) {
    ui.label("Neon shape:");
    egui::ComboBox::from_id_salt("neon_shape")
        .selected_text(NEON_NAMES.get(n.index as usize).copied().unwrap_or("?"))
        .show_ui(ui, |ui| {
            for (i, &name) in NEON_NAMES.iter().enumerate() {
                ui.selectable_value(&mut n.index, i as u32, name);
            }
        });
    ui.add(egui::Slider::new(&mut n.speed, 1u32..=10).text("Speed"));
    ui.checkbox(&mut n.rainbow, "Rainbow color");
    if !n.rainbow { color_edit(ui, "Color:", &mut n.color); }
}

fn render_qr_props(ui: &mut egui::Ui, q: &mut QrCodeItem) {
    ui.label("QR data / URL:"); ui.text_edit_multiline(&mut q.data);
    color_edit(ui, "Foreground:", &mut q.fg);
    color_edit(ui, "Background:", &mut q.bg);
}

fn render_calendar_props(ui: &mut egui::Ui, c: &mut CalendarItem) {
    color_edit(ui, "Text color:", &mut c.color);
    color_edit(ui, "Today:", &mut c.today_color);
    color_edit(ui, "Header:", &mut c.header_color);
    ui.add(egui::DragValue::new(&mut c.font_size).range(4..=100).prefix("Font size: "));
}

fn render_countdown_props(ui: &mut egui::Ui, c: &mut CountdownItem) {
    ui.label("Target (YYYY-MM-DD HH:MM:SS):"); ui.text_edit_singleline(&mut c.target);
    ui.label("Label:"); ui.text_edit_singleline(&mut c.label);
    ui.label("Format (D:H:M:S):"); ui.text_edit_singleline(&mut c.format);
    color_edit(ui, "Color:", &mut c.color);
    ui.add(egui::DragValue::new(&mut c.font_size).range(4..=200).prefix("Font size: "));
}

fn render_table_props(ui: &mut egui::Ui, t: &mut TableItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label("Cols:");
            let old_cols = t.cols;
            if ui.add(egui::DragValue::new(&mut t.cols).range(1..=20)).changed() {
                for row in &mut t.rows {
                    while row.len() < t.cols { row.push(String::new()); }
                    row.truncate(t.cols);
                }
                let _ = old_cols;
            }
        });
        ui.checkbox(&mut t.header_row, "First row is header");
        color_edit(ui, "Text color:", &mut t.text_color);
        color_edit(ui, "Header bg:", &mut t.header_bg);
        ui.add(egui::DragValue::new(&mut t.font_size).range(4..=100).prefix("Font: "));
        ui.separator();
        ui.label("Rows:");
        if ui.small_button("+ Add row").clicked() {
            t.rows.push(vec![String::new(); t.cols]);
        }
        let mut remove_idx = None;
        for (ri, row) in t.rows.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                for cell in row.iter_mut() {
                    ui.add(egui::TextEdit::singleline(cell).desired_width(70.0));
                }
                if ui.small_button("✕").clicked() { remove_idx = Some(ri); }
            });
        }
        if let Some(ri) = remove_idx { t.rows.remove(ri); }
    });
}

fn render_livestream_props(ui: &mut egui::Ui, ls: &mut LiveStreamItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(RichText::new("Stream URL:").strong());
        ui.add(egui::TextEdit::singleline(&mut ls.url)
            .desired_width(f32::INFINITY)
            .hint_text("rtsp://192.168.1.x/stream  or  rtmp://..."));
        ui.label(RichText::new("Supported protocols: RTSP, RTMP, HLS (http://...m3u8)").italics().color(Color32::from_gray(140)));
        ui.separator();
        ui.checkbox(&mut ls.reconnect, "Auto-reconnect on stream loss");
        ui.separator();
        ui.label("Status/reconnect text font:");
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(&mut ls.font_size).range(4..=200).prefix("Size: "));
            color_edit(ui, "Color:", &mut ls.color);
        });
    });
}

fn render_modbus_props(ui: &mut egui::Ui, mb: &mut ModbusItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("modbus_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("PLC Host:");
            ui.add(egui::TextEdit::singleline(&mut mb.host).desired_width(160.0).hint_text("192.168.1.10"));
            ui.end_row();
            ui.label("Port:");
            ui.add(egui::DragValue::new(&mut mb.port).range(1..=65535));
            ui.end_row();
            ui.label("Slave ID:");
            ui.add(egui::DragValue::new(&mut mb.slave).range(1..=247));
            ui.end_row();
            ui.label("Register:");
            ui.add(egui::DragValue::new(&mut mb.register).range(0..=65535));
            ui.end_row();
            ui.label("Register type:");
            egui::ComboBox::from_id_salt("mb_reg_type")
                .selected_text(&mb.register_type)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut mb.register_type, "holding".into(), "Holding (FC03)");
                    ui.selectable_value(&mut mb.register_type, "input".into(), "Input (FC04)");
                });
            ui.end_row();
            ui.label("Format string:");
            ui.add(egui::TextEdit::singleline(&mut mb.format).desired_width(160.0).hint_text("{value}°C"));
            ui.end_row();
            ui.label("Scale (×value):");
            ui.add(egui::TextEdit::singleline(&mut mb.scale_str).desired_width(80.0).hint_text("1.0"));
            ui.end_row();
            ui.label("Poll interval (s):");
            ui.add(egui::DragValue::new(&mut mb.update_interval).range(1..=3600));
            ui.end_row();
            ui.label("Scroll speed (0=static):");
            ui.add(egui::DragValue::new(&mut mb.scroll_speed).range(0..=500));
            ui.end_row();
            ui.label("Font size:");
            ui.add(egui::DragValue::new(&mut mb.font_size).range(4..=200));
            ui.end_row();
        });
        color_edit(ui, "Text color:", &mut mb.color);
    });
}

fn render_sensor_props(ui: &mut egui::Ui, sn: &mut SensorItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("sensor_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Sensor type:");
            egui::ComboBox::from_id_salt("sensor_type")
                .selected_text(&sn.sensor_type)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut sn.sensor_type, "cpu_temp".into(), "CPU temperature (/sys)");
                    ui.selectable_value(&mut sn.sensor_type, "ds18b20".into(), "DS18B20 1-Wire temp");
                    ui.selectable_value(&mut sn.sensor_type, "dht22".into(), "DHT22 via Python");
                    ui.selectable_value(&mut sn.sensor_type, "generic_file".into(), "Generic file (first line)");
                });
            ui.end_row();
            ui.label("Device path:");
            ui.add(egui::TextEdit::singleline(&mut sn.device)
                .desired_width(200.0)
                .hint_text("/sys/bus/w1/devices/28-xxxx"));
            ui.end_row();
            ui.label("Format string:");
            ui.add(egui::TextEdit::singleline(&mut sn.format)
                .desired_width(160.0).hint_text("{value}°C"));
            ui.end_row();
            ui.label("Poll interval (s):");
            ui.add(egui::DragValue::new(&mut sn.update_interval).range(1..=3600));
            ui.end_row();
            ui.label("Scroll speed (0=static):");
            ui.add(egui::DragValue::new(&mut sn.scroll_speed).range(0..=500));
            ui.end_row();
            ui.label("Font size:");
            ui.add(egui::DragValue::new(&mut sn.font_size).range(4..=200));
            ui.end_row();
        });
        color_edit(ui, "Text color:", &mut sn.color);
        ui.separator();
        ui.label(RichText::new("Device path is only needed for DS18B20 and generic_file.").italics().color(Color32::from_gray(140)));
    });
}

fn render_text3d_props(ui: &mut egui::Ui, t3: &mut Text3DItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(RichText::new("Text:").strong());
        ui.add(egui::TextEdit::singleline(&mut t3.text)
            .desired_width(f32::INFINITY)
            .font(egui::FontId::proportional(18.0)));
        ui.separator();
        egui::Grid::new("t3d_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Font size (px):");
            ui.add(egui::Slider::new(&mut t3.font_size, 4.0f32..=200.0).suffix("px"));
            ui.end_row();
            ui.label("Rotation speed:");
            ui.add(egui::Slider::new(&mut t3.rotate_speed, 0.0f32..=5.0).suffix("×"));
            ui.end_row();
            ui.label("3D effect:");
            egui::ComboBox::from_id_salt("t3d_effect")
                .selected_text(&t3.effect_3d)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut t3.effect_3d, "rotate_y".into(), "Rotate Y (spin)");
                    ui.selectable_value(&mut t3.effect_3d, "rotate_x".into(), "Rotate X (flip)");
                    ui.selectable_value(&mut t3.effect_3d, "pulse".into(), "Pulse (depth breathe)");
                    ui.selectable_value(&mut t3.effect_3d, "wave".into(), "Wave");
                });
            ui.end_row();
        });
        ui.separator();
        color_edit(ui, "Face color:", &mut t3.color);
        color_edit(ui, "Depth/shadow color:", &mut t3.depth_color);
    });
}

fn render_document_props(ui: &mut egui::Ui, dc: &mut DocumentItem) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        let path_str = dc.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
        ui.label(format!("File: {}", path_str));
        if ui.button("Browse…").clicked() {
            if let Some(p) = rfd::FileDialog::new()
                .add_filter("Documents", &["pdf","pptx","ppt","odp","docx","doc","odt","xlsx","ods","wps","dps","et"])
                .pick_file()
            {
                dc.path = Some(p);
            }
        }
        ui.label(RichText::new("Supported: PDF, PPTX/ODP/PPT, DOCX/ODT, XLSX/ODS, WPS/DPS\nRequires LibreOffice installed on the player device.").italics().color(Color32::from_gray(140)));
        ui.separator();
        egui::Grid::new("doc_grid").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            ui.label("Seconds per page:");
            ui.add(egui::DragValue::new(&mut dc.page_duration).range(1..=300));
            ui.end_row();
            ui.label("Fit mode:");
            egui::ComboBox::from_id_salt("doc_fit")
                .selected_text(["Stretch","Fill","Center"][dc.fit as usize % 3])
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut dc.fit, 0, "Stretch");
                    ui.selectable_value(&mut dc.fit, 1, "Fill (crop)");
                    ui.selectable_value(&mut dc.fit, 2, "Center (letterbox)");
                });
            ui.end_row();
            ui.label("Loop pages:");
            ui.checkbox(&mut dc.loop_pages, "");
            ui.end_row();
        });
    });
}

// ── EFRAME APP IMPL ───────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Handle background responses
        self.handle_responses(ctx);

        // Auto-screenshot refresh every 2s when preview is open
        if self.connected && self.show_preview_window
            && !self.screenshot_pending
            && self.preview_last.elapsed() > Duration::from_secs(2)
        {
            self.send_req(Request::GetScreenshot);
            self.screenshot_pending = true;
            self.preview_last = Instant::now();
        }

        // Repaint faster during active upgrade so the progress bar stays smooth
        let repaint_delay = if self.upgrading {
            Duration::from_millis(100)
        } else {
            Duration::from_millis(500)
        };
        ctx.request_repaint_after(repaint_delay);

        // Top toolbar
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.render_toolbar(ui);
        });

        // Bottom device panel
        if self.show_device_panel {
            egui::TopBottomPanel::bottom("device_bar")
                .min_height(28.0)
                .show(ctx, |ui| {
                    self.render_device_panel(ui);
                });
        }

        // Left: program tree
        egui::SidePanel::left("tree")
            .min_width(180.0)
            .default_width(220.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_tree(ui);
                });
            });

        // Right: properties
        egui::SidePanel::right("props")
            .min_width(200.0)
            .default_width(260.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    // Device programs if connected
                    if self.connected {
                        ui.collapsing("Device Programs", |ui| {
                            self.render_dev_programs(ui);
                        });
                        ui.separator();
                    }
                    self.render_properties(ui);
                });
            });

        // Center: canvas
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sel_prog.is_some() {
                self.render_canvas(ui);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Create or select a program to start editing.")
                        .size(18.0).color(Color32::from_gray(140)));
                });
            }
        });

        // Dialogs / windows
        self.render_dialogs(ctx);
        self.render_upgrade_window(ctx);
    }
}

// ── MAIN ──────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("HDPlayer — Huidu LED Sign Control"),
        ..Default::default()
    };

    eframe::run_native(
        "hdplayer-gui",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
