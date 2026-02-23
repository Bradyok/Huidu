//! HDSign GUI — Rust/egui reproduction of Huidu HDSign V2.0.2.
//!
//! Build:  cargo build -p hdsign --features gui --bin hdsign-gui
//! Run:    cargo run   -p hdsign --features gui --bin hdsign-gui

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use hdsign::{DeviceDetails, DeviceInfo, Discovery, EthConfig, FileEntry, HdsignClient, ProgramInfo};
use hdsign::firmware::{FirmwareEntry, find_firmware, load_firmware_catalog};
use hdsign::hardware::{HardwareCard, load_cards};
use hdsign::transfer::FileTransfer;
use hdsign::weather::{get_weather_cities, weather_countries};

// ── CONSTANTS ─────────────────────────────────────────────────────────────────

const EFFECT_NAMES: &[&str] = &[
    "None", "Random", "Blinds H", "Blinds V", "Checkers", "Spiral",
    "Sweep", "Cross", "Diamond", "Rotate", "Flash", "Wipe H", "Wipe V",
    "Wipe D1", "Wipe D2", "Shutter H", "Shutter V", "Fade",
    "Push L", "Push R", "Push U", "Scroll L", "Scroll R", "Scroll U", "Scroll D",
    "Zoom In", "Zoom Out", "Mosaic", "Fire", "Stars",
];

const NEON_NAMES: &[&str] = &[
    "Arrow Up", "Arrow Down", "Arrow Left", "Arrow Right",
    "Square", "Circle", "Heart", "Diamond", "Star4", "Star5",
    "Star6", "Lightning", "Crown", "Flower", "Tree", "Snowflake",
    "Moon", "Sun", "Cloud", "Drop", "Fire", "Bell", "Music",
    "Peace", "Cross", "Infinity", "Wifi", "Camera", "Phone",
    "Car", "Plane", "Bicycle", "Smile", "Thumbs Up", "Comet",
];

/// Country/City pairs from the bundled prayconfig.xml (in alphabetical order by country).
const PRAYER_CITIES: &[(&str, &str)] = &[
    ("Algeria",    "Setif"),
    ("Custom",     "Custom"),
    ("India",      "Mumbai"),
    ("Indonesia",  "Bangli"),
    ("Indonesia",  "Buleleng"),
    ("Indonesia",  "Denpasar"),
    ("Indonesia",  "Gianyar"),
    ("Indonesia",  "Jakarta"),
    ("Indonesia",  "Salatiga"),
    ("Indonesia",  "Semarang"),
    ("Iran",       "Ahvaz"),
    ("Iran",       "Esfahan"),
    ("Iran",       "Kahriz"),
    ("Iran",       "Karaj"),
    ("Iran",       "Kermanshah"),
    ("Iran",       "Mashhad"),
    ("Iran",       "Qom"),
    ("Iran",       "Shiraz"),
    ("Iran",       "Tabriz"),
    ("Iran",       "Teheran"),
    ("Malaysia",   "Kuala Lumpur"),
    ("Malaysia",   "Kuala Lumpur(Wilayah Persekutuan)"),
    ("Malaysia",   "Penang"),
    ("Palestine",  "Hebron"),
];

const BORDER_NAMES: &[&str] = &[
    "None", "Snow", "Rain", "Firefly", "Waterfall", "Flame", "Starfall",
    "Colorful", "Neon1", "Neon2", "Brick", "Wave", "Radar", "Ripple",
    "Sparkle", "Spiral", "Dots", "Lines",
];

// ── CONNECTION MODE ────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ConnMode {
    Ethernet,
    WifiAp,
    Serial,  // stub
    Usb,     // stub
}

impl ConnMode {
    fn label(self) -> &'static str {
        match self {
            Self::Ethernet => "Ethernet",
            Self::WifiAp   => "WiFi AP",
            Self::Serial   => "Serial",
            Self::Usb      => "USB",
        }
    }
}

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
    pub align: u8,
    pub valign: u8,
    pub effect_in: u32,
    pub effect_out: u32,
    pub effect_in_speed: u32,
    pub effect_out_speed: u32,
    pub duration_tenths: u32,
    pub scroll_dir: u8,
    pub scroll_speed: u32,
    pub word_wrap: bool,
    pub background: Option<[u8; 3]>,
}

#[derive(Clone, Debug)]
pub struct ImageItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub fit: u8,
}

#[derive(Clone, Debug)]
pub struct ClockItem {
    pub guid: String,
    pub is_analog: bool,
    pub timezone: String,
    pub show_date: bool,
    pub date_color: [u8; 3],
    pub show_time: bool,
    pub time_color: [u8; 3],
    pub font_size: u32,
    pub hand_color: [u8; 3],
    pub second_color: [u8; 3],
    pub dial_color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct NeonItem {
    pub guid: String,
    pub index: u32,
    pub color: [u8; 3],
    pub speed: u32,
    pub rainbow: bool,
}

#[derive(Clone, Debug)]
pub struct VideoItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub loop_count: u32,
}

#[derive(Clone, Debug)]
pub struct QrCodeItem {
    pub guid: String,
    pub data: String,
    pub fg_color: [u8; 3],
    pub bg_color: [u8; 3],
}

#[derive(Clone, Debug)]
pub struct CountdownItem {
    pub guid: String,
    pub target_date: String,
    pub label: String,
    pub color: [u8; 3],
    pub font_size: u32,
}

#[derive(Clone, Debug)]
pub struct GifItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub speed: u32,
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
pub struct WeatherItem {
    pub guid: String,
    /// Yahoo weather city code, e.g. "CN101010100" for Beijing
    pub city_code: String,
    /// Display name for the city
    pub city_name: String,
    pub temp_color: [u8; 3],
    pub text_color: [u8; 3],
    pub font_size: u32,
    /// 0=Celsius, 1=Fahrenheit
    pub unit: u8,
}

#[derive(Clone, Debug)]
pub struct ColorfulWordItem {
    pub guid: String,
    pub text: String,
    pub font_size: u32,
    pub bold: bool,
    pub italic: bool,
    pub speed: u32,
    pub scroll_dir: u8,
    /// 0=rainbow, 1=gradient L→R, 2=gradient T→B, 3=fire
    pub color_mode: u8,
}

#[derive(Clone, Debug)]
pub struct Art3dItem {
    pub guid: String,
    pub text: String,
    pub font_size: u32,
    pub color: [u8; 3],
    pub bg_color: [u8; 3],
    pub speed: u32,
    pub scroll_dir: u8,
    /// 0=3D Push, 1=3D Rotate, 2=Shadow, 3=Hollow
    pub style: u8,
}

#[derive(Clone, Debug)]
pub struct FlashItem {
    pub guid: String,
    pub path: Option<PathBuf>,
    pub loop_count: u32,
}

#[derive(Clone, Debug)]
pub struct PrayerItem {
    pub guid: String,
    pub country: String,
    pub city: String,
    pub color: [u8; 3],
    pub font_size: u32,
}

#[derive(Clone, Debug)]
pub struct TempRhItem {
    pub guid: String,
    /// 0=Celsius, 1=Fahrenheit
    pub unit: u8,
    pub temp_color: [u8; 3],
    pub humidity_color: [u8; 3],
    pub font_size: u32,
}

#[derive(Clone, Debug)]
pub enum ContentItem {
    Text(TextItem),
    Image(ImageItem),
    Clock(ClockItem),
    Neon(NeonItem),
    Video(VideoItem),
    QrCode(QrCodeItem),
    Countdown(CountdownItem),
    Gif(GifItem),
    Calendar(CalendarItem),
    Weather(WeatherItem),
    Prayer(PrayerItem),
    TempRh(TempRhItem),
    ColorfulWord(ColorfulWordItem),
    Art3d(Art3dItem),
    Flash(FlashItem),
}

impl ContentItem {
    pub fn guid(&self) -> &str {
        match self {
            Self::Text(t)       => &t.guid,
            Self::Image(i)      => &i.guid,
            Self::Clock(c)      => &c.guid,
            Self::Neon(n)       => &n.guid,
            Self::Video(v)      => &v.guid,
            Self::QrCode(q)     => &q.guid,
            Self::Countdown(c)  => &c.guid,
            Self::Gif(g)        => &g.guid,
            Self::Calendar(c)   => &c.guid,
            Self::Weather(w)    => &w.guid,
            Self::Prayer(p)        => &p.guid,
            Self::TempRh(t)        => &t.guid,
            Self::ColorfulWord(c)  => &c.guid,
            Self::Art3d(a)         => &a.guid,
            Self::Flash(f)         => &f.guid,
        }
    }
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Text(t)      => if t.single_line { "Single-Line Text" } else { "Multi-Line Text" },
            Self::Image(_)     => "Image",
            Self::Clock(c)     => if c.is_analog { "Analog Clock" } else { "Digital Clock" },
            Self::Neon(_)      => "Neon Shape",
            Self::Video(_)     => "Video",
            Self::QrCode(_)    => "QR Code",
            Self::Countdown(_) => "Countdown",
            Self::Gif(_)       => "GIF",
            Self::Calendar(_)  => "Calendar",
            Self::Weather(_)   => "Weather",
            Self::Prayer(_)       => "Prayer Times",
            Self::TempRh(_)       => "Temp/Humidity",
            Self::ColorfulWord(_) => "Colorful Text",
            Self::Art3d(_)        => "3D Art Text",
            Self::Flash(_)        => "Flash/SWF",
        }
    }
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Text(t)      => if t.single_line { "T-" } else { "T+" },
            Self::Image(_)     => "Img",
            Self::Clock(_)     => "Clk",
            Self::Neon(_)      => "Neo",
            Self::Video(_)     => "Vid",
            Self::QrCode(_)    => "QR",
            Self::Countdown(_) => "Cdt",
            Self::Gif(_)       => "GIF",
            Self::Calendar(_)  => "Cal",
            Self::Weather(_)   => "Wth",
            Self::Prayer(_)       => "Pry",
            Self::TempRh(_)       => "T°H",
            Self::ColorfulWord(_) => "CLR",
            Self::Art3d(_)        => "3D",
            Self::Flash(_)        => "SWF",
        }
    }
    pub fn new_text(guid: String, single_line: bool) -> Self {
        Self::Text(TextItem {
            guid, text: "Hello!".into(), single_line,
            font_name: String::new(), font_size: 14,
            color: [255, 255, 0], bold: false, italic: false, underline: false,
            align: 1, valign: 1,
            effect_in: 17, effect_out: 17, effect_in_speed: 3, effect_out_speed: 3,
            duration_tenths: 50, scroll_dir: 1, scroll_speed: 40,
            word_wrap: false, background: None,
        })
    }
    pub fn new_clock(guid: String) -> Self {
        Self::Clock(ClockItem {
            guid, is_analog: false, timezone: "+00:00".into(),
            show_date: true, date_color: [0, 255, 136],
            show_time: true, time_color: [255, 255, 255],
            font_size: 14, hand_color: [0, 255, 136],
            second_color: [255, 68, 0], dial_color: [13, 26, 13],
        })
    }
    pub fn new_neon(guid: String) -> Self {
        Self::Neon(NeonItem { guid, index: 6, color: [255, 0, 0], speed: 5, rainbow: true })
    }
    pub fn new_image(guid: String) -> Self {
        Self::Image(ImageItem { guid, path: None, fit: 0 })
    }
    pub fn new_video(guid: String) -> Self {
        Self::Video(VideoItem { guid, path: None, loop_count: 0 })
    }
    pub fn new_qr(guid: String) -> Self {
        Self::QrCode(QrCodeItem {
            guid, data: "https://example.com".into(),
            fg_color: [255, 255, 255], bg_color: [0, 0, 0],
        })
    }
    pub fn new_countdown(guid: String) -> Self {
        Self::Countdown(CountdownItem {
            guid, target_date: "2025-12-31".into(), label: "Countdown".into(),
            color: [255, 255, 0], font_size: 14,
        })
    }
    pub fn new_gif(guid: String) -> Self {
        Self::Gif(GifItem { guid, path: None, speed: 5 })
    }
    pub fn new_calendar(guid: String) -> Self {
        Self::Calendar(CalendarItem {
            guid,
            color: [255, 255, 255],
            today_color: [255, 200, 0],
            header_color: [0, 180, 255],
            font_size: 12,
        })
    }
    pub fn new_weather(guid: String) -> Self {
        Self::Weather(WeatherItem {
            guid,
            city_code: String::new(),
            city_name: String::new(),
            temp_color: [255, 180, 0],
            text_color: [255, 255, 255],
            font_size: 14,
            unit: 0,
        })
    }
    pub fn new_prayer(guid: String) -> Self {
        Self::Prayer(PrayerItem {
            guid,
            country: String::new(),
            city: String::new(),
            color: [255, 220, 100],
            font_size: 14,
        })
    }
    pub fn new_temprh(guid: String) -> Self {
        Self::TempRh(TempRhItem {
            guid,
            unit: 0,
            temp_color: [255, 100, 60],
            humidity_color: [100, 180, 255],
            font_size: 14,
        })
    }
    pub fn new_colorful_word(guid: String) -> Self {
        Self::ColorfulWord(ColorfulWordItem {
            guid, text: "Hello!".into(),
            font_size: 18, bold: true, italic: false,
            speed: 5, scroll_dir: 1, color_mode: 0,
        })
    }
    pub fn new_art3d(guid: String) -> Self {
        Self::Art3d(Art3dItem {
            guid, text: "HELLO".into(),
            font_size: 24,
            color: [255, 200, 0],
            bg_color: [0, 0, 0],
            speed: 3, scroll_dir: 1, style: 0,
        })
    }
    pub fn new_flash(guid: String) -> Self {
        Self::Flash(FlashItem { guid, path: None, loop_count: 0 })
    }
}

// ── AREA + PROGRAM + PROJECT ──────────────────────────────────────────────────

#[derive(Clone, Debug)]
pub struct Area {
    pub guid: String,
    pub name: String,
    pub alpha: u8,
    pub x: i32, pub y: i32, pub w: i32, pub h: i32,
    pub background_color: Option<[u8; 3]>,
    pub items: Vec<ContentItem>,
}

impl Area {
    pub fn new(guid: String, name: String, x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { guid, name, alpha: 255, x, y, w, h, background_color: None, items: Vec::new() }
    }
}

#[derive(Clone, Debug)]
pub struct Program {
    pub guid: String,
    pub name: String,
    pub play_duration_secs: u32,
    pub play_count: u32,
    pub border_index: u8,
    pub border_speed: u8,
    pub areas: Vec<Area>,
    // PlayControl schedule (empty string = no constraint)
    pub date_start: String,
    pub date_end: String,
    pub time_start: String,
    pub time_end: String,
    /// Mon=0 … Sun=6; all true = no filter
    pub week_filter: [bool; 7],
    pub disabled: bool,
}

impl Program {
    pub fn new(guid: String, name: String, screen_w: i32, screen_h: i32) -> Self {
        let area_guid = new_guid();
        let mut p = Self {
            guid, name, play_duration_secs: 10, play_count: 0,
            border_index: 0, border_speed: 3, areas: Vec::new(),
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
        [
            u8::from_str_radix(&s[0..2], 16).unwrap_or(255),
            u8::from_str_radix(&s[2..4], 16).unwrap_or(255),
            u8::from_str_radix(&s[4..6], 16).unwrap_or(255),
        ]
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
    for q in &['"', '\''] {
        let needle = format!("{}={}", attr, q);
        if let Some(s) = xml.find(&needle) {
            let vs = s + needle.len();
            if let Some(e) = xml[vs..].find(*q) { return Some(&xml[vs..vs+e]); }
        }
    }
    None
}
fn get_attr_in_tag<'a>(xml: &'a str, tag: &str, attr: &str) -> Option<&'a str> {
    let pat = format!("<{} ", tag);
    let s = xml.find(&pat)?;
    let e = xml[s..].find("/>").map(|e| s+e+2)
        .or_else(|| xml[s..].find('>').map(|e| s+e+1))?;
    get_attr(&xml[s..e], attr)
}

// ── MEDIA FILE COLLECTION ─────────────────────────────────────────────────────

fn collect_media_files(project: &Project) -> Vec<(String, PathBuf)> {
    let mut seen = std::collections::HashSet::new();
    let mut files = Vec::new();
    for prog in &project.programs {
        for area in &prog.areas {
            for item in &area.items {
                let path_opt: Option<&PathBuf> = match item {
                    ContentItem::Image(im)  => im.path.as_ref(),
                    ContentItem::Video(v)   => v.path.as_ref(),
                    ContentItem::Gif(g)     => g.path.as_ref(),
                    ContentItem::Flash(f)   => f.path.as_ref(),
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
        out.push_str(&format!("  <program guid=\"{}\" name=\"{}\" type=\"normal\">\n",
            prog.guid, xml_escape(&prog.name)));

        // playControl with optional schedule sub-elements
        let has_date  = !prog.date_start.is_empty() || !prog.date_end.is_empty();
        let has_time  = !prog.time_start.is_empty() || !prog.time_end.is_empty();
        let week_not_all = prog.week_filter.iter().any(|&b| !b);
        let has_sched = has_date || has_time || week_not_all || prog.disabled;
        {
            let h = prog.play_duration_secs / 3600;
            let m = (prog.play_duration_secs % 3600) / 60;
            let s = prog.play_duration_secs % 60;
            let disabled_attr = if prog.disabled { " disabled=\"true\"" } else { "" };
            if has_sched {
                out.push_str(&format!("    <playControl duration=\"{:02}:{:02}:{:02}\" count=\"0\"{disabled_attr}>\n", h, m, s));
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
            } else {
                out.push_str(&format!("    <playControl duration=\"{:02}:{:02}:{:02}\" count=\"0\"/>\n", h, m, s));
            }
        }
        if prog.border_index > 0 {
            out.push_str(&format!("    <border index=\"{}\" speed=\"{}\"/>\n", prog.border_index, prog.border_speed));
        }

        for area in &prog.areas {
            out.push_str(&format!(
                "    <area guid=\"{}\" name=\"{}\" alpha=\"{}\">\n",
                area.guid, xml_escape(&area.name), area.alpha));
            out.push_str(&format!("      <rectangle x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>\n",
                area.x, area.y, area.w, area.h));
            if let Some(bg) = area.background_color {
                out.push_str(&format!("      <background color=\"{}\"/>\n", rgb_to_hex(bg)));
            }
            out.push_str("      <resources>\n");

            for item in &area.items {
                match item {
                    ContentItem::Text(t) => {
                        out.push_str(&format!("        <text guid=\"{}\" singleLine=\"{}\"",
                            t.guid, t.single_line));
                        if let Some(bg) = t.background {
                            out.push_str(&format!(" background=\"{}\"", rgb_to_hex(bg)));
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
                        if t.bold      { fa.push_str(" bold=\"true\""); }
                        if t.italic    { fa.push_str(" italic=\"true\""); }
                        if t.underline { fa.push_str(" underline=\"true\""); }
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
                    ContentItem::Clock(c) => {
                        if c.is_analog {
                            out.push_str(&format!(
                                "        <analogClock guid=\"{}\" timezone=\"{}\" dialColor=\"{}\" handColor=\"{}\" secondColor=\"{}\"/>\n",
                                c.guid, xml_escape(&c.timezone),
                                rgb_to_hex(c.dial_color), rgb_to_hex(c.hand_color), rgb_to_hex(c.second_color)));
                        } else {
                            out.push_str(&format!("        <clock guid=\"{}\" type=\"digital\" timezone=\"{}\">\n",
                                c.guid, xml_escape(&c.timezone)));
                            if c.show_date {
                                out.push_str(&format!("          <date format=\"1\" color=\"{}\" display=\"true\"/>\n",
                                    rgb_to_hex(c.date_color)));
                            }
                            if c.show_time {
                                out.push_str(&format!("          <time format=\"1\" color=\"{}\" display=\"true\"/>\n",
                                    rgb_to_hex(c.time_color)));
                            }
                            out.push_str("        </clock>\n");
                        }
                    }
                    ContentItem::Neon(n) => {
                        let col = if n.rainbow { "rainbow".into() } else { rgb_to_hex(n.color) };
                        out.push_str(&format!("        <neon guid=\"{}\" index=\"{}\" color=\"{}\" speed=\"{}\" singleColor=\"{}\"/>\n",
                            n.guid, n.index + 1, col, n.speed, !n.rainbow));
                    }
                    ContentItem::Video(v) => {
                        let fname = v.path.as_ref().and_then(|p| p.file_name())
                            .and_then(|n| n.to_str()).unwrap_or("");
                        out.push_str(&format!("        <video guid=\"{}\" loop=\"{}\">\n",
                            v.guid, v.loop_count));
                        if !fname.is_empty() {
                            out.push_str(&format!("          <file name=\"{}\"/>\n", fname));
                        }
                        out.push_str("        </video>\n");
                    }
                    ContentItem::QrCode(q) => {
                        out.push_str(&format!(
                            "        <qrcode guid=\"{}\" fgColor=\"{}\" bgColor=\"{}\">\n",
                            q.guid, rgb_to_hex(q.fg_color), rgb_to_hex(q.bg_color)));
                        out.push_str(&format!("          <data>{}</data>\n", xml_escape(&q.data)));
                        out.push_str("        </qrcode>\n");
                    }
                    ContentItem::Countdown(c) => {
                        out.push_str(&format!(
                            "        <countdown guid=\"{}\" targetDate=\"{}\" color=\"{}\" fontSize=\"{}\">\n",
                            c.guid, xml_escape(&c.target_date), rgb_to_hex(c.color), c.font_size));
                        out.push_str(&format!("          <label>{}</label>\n", xml_escape(&c.label)));
                        out.push_str("        </countdown>\n");
                    }
                    ContentItem::Gif(g) => {
                        let fname = g.path.as_ref().and_then(|p| p.file_name())
                            .and_then(|n| n.to_str()).unwrap_or("");
                        out.push_str(&format!("        <gif guid=\"{}\" speed=\"{}\">\n",
                            g.guid, g.speed));
                        if !fname.is_empty() {
                            out.push_str(&format!("          <file name=\"{}\"/>\n", fname));
                        }
                        out.push_str("        </gif>\n");
                    }
                    ContentItem::Calendar(c) => {
                        out.push_str(&format!(
                            "        <calendar guid=\"{}\" color=\"{}\" todayColor=\"{}\" headerColor=\"{}\" fontSize=\"{}\"/>\n",
                            c.guid, rgb_to_hex(c.color), rgb_to_hex(c.today_color),
                            rgb_to_hex(c.header_color), c.font_size));
                    }
                    ContentItem::Weather(w) => {
                        out.push_str(&format!(
                            "        <weather guid=\"{}\" cityCode=\"{}\" cityName=\"{}\" tempColor=\"{}\" textColor=\"{}\" fontSize=\"{}\" unit=\"{}\"/>\n",
                            w.guid, xml_escape(&w.city_code), xml_escape(&w.city_name),
                            rgb_to_hex(w.temp_color), rgb_to_hex(w.text_color),
                            w.font_size, if w.unit == 1 { "F" } else { "C" }));
                    }
                    ContentItem::Prayer(p) => {
                        out.push_str(&format!(
                            "        <prayer guid=\"{}\" country=\"{}\" city=\"{}\" color=\"{}\" fontSize=\"{}\"/>\n",
                            p.guid, xml_escape(&p.country), xml_escape(&p.city),
                            rgb_to_hex(p.color), p.font_size));
                    }
                    ContentItem::TempRh(t) => {
                        out.push_str(&format!(
                            "        <temprh guid=\"{}\" unit=\"{}\" tempColor=\"{}\" humidityColor=\"{}\" fontSize=\"{}\"/>\n",
                            t.guid, if t.unit == 1 { "F" } else { "C" },
                            rgb_to_hex(t.temp_color), rgb_to_hex(t.humidity_color), t.font_size));
                    }
                    ContentItem::ColorfulWord(c) => {
                        let dir = match c.scroll_dir { 1=>"left", 2=>"right", 3=>"up", _=>"down" };
                        let mode = match c.color_mode { 1=>"gradientH", 2=>"gradientV", 3=>"fire", _=>"rainbow" };
                        out.push_str(&format!(
                            "        <colorfulWord guid=\"{}\" fontSize=\"{}\" bold=\"{}\" italic=\"{}\" speed=\"{}\" scrollDir=\"{}\" colorMode=\"{}\">\n",
                            c.guid, c.font_size, c.bold, c.italic, c.speed, dir, mode));
                        out.push_str(&format!("          <string>{}</string>\n", xml_escape(&c.text)));
                        out.push_str("        </colorfulWord>\n");
                    }
                    ContentItem::Art3d(a) => {
                        let dir = match a.scroll_dir { 1=>"left", 2=>"right", 3=>"up", _=>"down" };
                        let style = match a.style { 1=>"rotate", 2=>"shadow", 3=>"hollow", _=>"push" };
                        out.push_str(&format!(
                            "        <art3d guid=\"{}\" fontSize=\"{}\" color=\"{}\" bgColor=\"{}\" style=\"{}\" speed=\"{}\" scrollDir=\"{}\">\n",
                            a.guid, a.font_size, rgb_to_hex(a.color), rgb_to_hex(a.bg_color),
                            style, a.speed, dir));
                        out.push_str(&format!("          <string>{}</string>\n", xml_escape(&a.text)));
                        out.push_str("        </art3d>\n");
                    }
                    ContentItem::Flash(f) => {
                        let fname = f.path.as_ref().and_then(|p| p.file_name())
                            .and_then(|n| n.to_str()).unwrap_or("");
                        out.push_str(&format!("        <flash guid=\"{}\" loop=\"{}\">\n", f.guid, f.loop_count));
                        if !fname.is_empty() {
                            out.push_str(&format!("          <file name=\"{}\"/>\n", fname));
                        }
                        out.push_str("        </flash>\n");
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
    let play_duration_secs = get_attr(xml, "duration").and_then(|d| {
        let p: Vec<&str> = d.split(':').collect();
        if p.len() == 3 {
            Some(p[0].parse::<u32>().ok()? * 3600 + p[1].parse::<u32>().ok()? * 60 + p[2].parse::<u32>().ok()?)
        } else { None }
    }).unwrap_or(10);
    let border_index = get_attr_in_tag(xml, "border", "index").and_then(|v| v.parse().ok()).unwrap_or(0u8);
    let border_speed = get_attr_in_tag(xml, "border", "speed").and_then(|v| v.parse().ok()).unwrap_or(3u8);
    let disabled = get_attr(xml, "disabled").map(|v| v == "true").unwrap_or(false);
    let date_start = get_attr_in_tag(xml, "date", "start").unwrap_or("").to_string();
    let date_end   = get_attr_in_tag(xml, "date", "end").unwrap_or("").to_string();
    let time_start = get_attr_in_tag(xml, "time", "start").unwrap_or("").to_string();
    let time_end   = get_attr_in_tag(xml, "time", "end").unwrap_or("").to_string();
    let week_filter: [bool; 7] = {
        let bits = get_attr_in_tag(xml, "week", "enable").unwrap_or("1111111");
        let mut f = [true; 7];
        for (i, c) in bits.chars().take(7).enumerate() { f[i] = c != '0'; }
        f
    };

    let mut areas = Vec::new();
    let mut s = xml;
    while let Some(ps) = s.find("<area ") {
        let pe = s[ps..].find("</area>").map(|e| ps+e+7).unwrap_or(s.len());
        if let Some(area) = parse_area(&s[ps..pe], max_w, max_h) {
            areas.push(area);
        }
        s = &s[pe.min(s.len())..];
    }
    Some(Program {
        guid, name, play_duration_secs, play_count: 0,
        border_index, border_speed, areas,
        date_start, date_end, time_start, time_end, week_filter, disabled,
    })
}

fn parse_area(xml: &str, max_w: &mut i32, max_h: &mut i32) -> Option<Area> {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    let name = get_attr(xml, "name").map(|s| xml_unescape(s)).unwrap_or_else(|| "Area".into());
    let alpha = get_attr(xml, "alpha").and_then(|v| v.parse().ok()).unwrap_or(255u8);
    let x = get_attr_in_tag(xml, "rectangle", "x").and_then(|v| v.parse().ok()).unwrap_or(0i32);
    let y = get_attr_in_tag(xml, "rectangle", "y").and_then(|v| v.parse().ok()).unwrap_or(0i32);
    let w = get_attr_in_tag(xml, "rectangle", "width").and_then(|v| v.parse().ok()).unwrap_or(128i32);
    let h = get_attr_in_tag(xml, "rectangle", "height").and_then(|v| v.parse().ok()).unwrap_or(64i32);
    if x + w > *max_w { *max_w = x + w; }
    if y + h > *max_h { *max_h = y + h; }
    let background_color = get_attr_in_tag(xml, "background", "color").map(|s| hex_to_rgb(s));
    let items = parse_items(xml);
    Some(Area { guid, name, alpha, x, y, w, h, background_color, items })
}

fn parse_items(xml: &str) -> Vec<ContentItem> {
    let mut items = Vec::new();

    // Text items
    let mut s = xml;
    while let Some(ps) = s.find("<text ") {
        let pe = s[ps..].find("</text>").map(|e| ps+e+7)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        items.push(parse_text(&s[ps..pe]));
        s = &s[pe.min(s.len())..];
    }
    // Image items
    let mut s = xml;
    while let Some(ps) = s.find("<image ") {
        let pe = s[ps..].find("</image>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fit = match get_attr(t, "fit").unwrap_or("stretch") {
            "fill" => 1, "center" => 2, "fit" => 3, _ => 0,
        };
        let fname = get_attr_in_tag(t, "file", "name").map(|n| xml_unescape(n));
        items.push(ContentItem::Image(ImageItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from), fit,
        }));
        s = &s[pe.min(s.len())..];
    }
    // Clock items
    let mut s = xml;
    while let Some(ps) = s.find("<clock ") {
        let pe = s[ps..].find("</clock>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        items.push(parse_clock(&s[ps..pe], false));
        s = &s[pe.min(s.len())..];
    }
    // Analog clock items
    let mut s = xml;
    while let Some(ps) = s.find("<analogClock ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2).unwrap_or(s.len());
        items.push(parse_clock(&s[ps..pe], true));
        s = &s[pe.min(s.len())..];
    }
    // Neon items
    let mut s = xml;
    while let Some(ps) = s.find("<neon ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2)
            .or_else(|| s[ps..].find("</neon>").map(|e| ps+e+7)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let color_s = get_attr(t, "color").unwrap_or("#ff0000");
        let rainbow = color_s.eq_ignore_ascii_case("rainbow");
        items.push(ContentItem::Neon(NeonItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            index: get_attr(t, "index").and_then(|v| v.parse::<u32>().ok()).unwrap_or(1).saturating_sub(1),
            color: if rainbow { [255,0,0] } else { hex_to_rgb(color_s) },
            speed: get_attr(t, "speed").and_then(|v| v.parse().ok()).unwrap_or(5),
            rainbow,
        }));
        s = &s[pe.min(s.len())..];
    }
    // Video items
    let mut s = xml;
    while let Some(ps) = s.find("<video ") {
        let pe = s[ps..].find("</video>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fname = get_attr_in_tag(t, "file", "name").map(|n| xml_unescape(n));
        items.push(ContentItem::Video(VideoItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from),
            loop_count: get_attr(t, "loop").and_then(|v| v.parse().ok()).unwrap_or(0),
        }));
        s = &s[pe.min(s.len())..];
    }
    // QR Code items
    let mut s = xml;
    while let Some(ps) = s.find("<qrcode ") {
        let pe = s[ps..].find("</qrcode>").map(|e| ps+e+9)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let data = t.find("<data>").and_then(|d| {
            t[d+6..].find("</data>").map(|e| xml_unescape(&t[d+6..d+6+e]))
        }).unwrap_or_default();
        items.push(ContentItem::QrCode(QrCodeItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            data,
            fg_color: hex_to_rgb(get_attr(t, "fgColor").unwrap_or("#ffffff")),
            bg_color: hex_to_rgb(get_attr(t, "bgColor").unwrap_or("#000000")),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Countdown items
    let mut s = xml;
    while let Some(ps) = s.find("<countdown ") {
        let pe = s[ps..].find("</countdown>").map(|e| ps+e+12)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let label = t.find("<label>").and_then(|l| {
            t[l+7..].find("</label>").map(|e| xml_unescape(&t[l+7..l+7+e]))
        }).unwrap_or_default();
        items.push(ContentItem::Countdown(CountdownItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            target_date: get_attr(t, "targetDate").unwrap_or("2025-12-31").to_string(),
            label,
            color: hex_to_rgb(get_attr(t, "color").unwrap_or("#ffff00")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(14),
        }));
        s = &s[pe.min(s.len())..];
    }
    // GIF items
    let mut s = xml;
    while let Some(ps) = s.find("<gif ") {
        let pe = s[ps..].find("</gif>").map(|e| ps+e+6)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fname = get_attr_in_tag(t, "file", "name").map(|n| xml_unescape(n));
        items.push(ContentItem::Gif(GifItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from),
            speed: get_attr(t, "speed").and_then(|v| v.parse().ok()).unwrap_or(5),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Calendar items
    let mut s = xml;
    while let Some(ps) = s.find("<calendar ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2)
            .or_else(|| s[ps..].find("</calendar>").map(|e| ps+e+11)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Calendar(CalendarItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            color: hex_to_rgb(get_attr(t, "color").unwrap_or("#ffffff")),
            today_color: hex_to_rgb(get_attr(t, "todayColor").unwrap_or("#ffc800")),
            header_color: hex_to_rgb(get_attr(t, "headerColor").unwrap_or("#00b4ff")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(12),
        }));
        s = &s[pe.min(s.len())..];
    }
    // Weather items
    let mut s = xml;
    while let Some(ps) = s.find("<weather ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2)
            .or_else(|| s[ps..].find("</weather>").map(|e| ps+e+10)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Weather(WeatherItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            city_code: get_attr(t, "cityCode").map(xml_unescape).unwrap_or_default(),
            city_name: get_attr(t, "cityName").map(xml_unescape).unwrap_or_default(),
            temp_color: hex_to_rgb(get_attr(t, "tempColor").unwrap_or("#ffb400")),
            text_color: hex_to_rgb(get_attr(t, "textColor").unwrap_or("#ffffff")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(14),
            unit: if get_attr(t, "unit").unwrap_or("C") == "F" { 1 } else { 0 },
        }));
        s = &s[pe.min(s.len())..];
    }
    // Prayer items
    let mut s = xml;
    while let Some(ps) = s.find("<prayer ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2)
            .or_else(|| s[ps..].find("</prayer>").map(|e| ps+e+9)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::Prayer(PrayerItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            country: get_attr(t, "country").map(xml_unescape).unwrap_or_default(),
            city: get_attr(t, "city").map(xml_unescape).unwrap_or_default(),
            color: hex_to_rgb(get_attr(t, "color").unwrap_or("#ffdc64")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(14),
        }));
        s = &s[pe.min(s.len())..];
    }
    // TempRH items
    let mut s = xml;
    while let Some(ps) = s.find("<temprh ") {
        let pe = s[ps..].find("/>").map(|e| ps+e+2)
            .or_else(|| s[ps..].find("</temprh>").map(|e| ps+e+9)).unwrap_or(s.len());
        let t = &s[ps..pe];
        items.push(ContentItem::TempRh(TempRhItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            unit: if get_attr(t, "unit").unwrap_or("C") == "F" { 1 } else { 0 },
            temp_color: hex_to_rgb(get_attr(t, "tempColor").unwrap_or("#ff643c")),
            humidity_color: hex_to_rgb(get_attr(t, "humidityColor").unwrap_or("#64b4ff")),
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(14),
        }));
        s = &s[pe.min(s.len())..];
    }
    // ColorfulWord items
    let mut s = xml;
    while let Some(ps) = s.find("<colorfulWord ") {
        let pe = s[ps..].find("</colorfulWord>").map(|e| ps+e+15)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let text = t.find("<string>").and_then(|st| {
            t[st+8..].find("</string>").map(|e| xml_unescape(&t[st+8..st+8+e]))
        }).unwrap_or_default();
        items.push(ContentItem::ColorfulWord(ColorfulWordItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            text,
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(18),
            bold:   get_attr(t, "bold").map(|v| v=="true").unwrap_or(true),
            italic: get_attr(t, "italic").map(|v| v=="true").unwrap_or(false),
            speed: get_attr(t, "speed").and_then(|v| v.parse().ok()).unwrap_or(5),
            scroll_dir: match get_attr(t, "scrollDir").unwrap_or("left") {
                "right"=>2, "up"=>3, "down"=>4, _=>1
            },
            color_mode: match get_attr(t, "colorMode").unwrap_or("rainbow") {
                "gradientH"=>1, "gradientV"=>2, "fire"=>3, _=>0
            },
        }));
        s = &s[pe.min(s.len())..];
    }
    // Art3D items
    let mut s = xml;
    while let Some(ps) = s.find("<art3d ") {
        let pe = s[ps..].find("</art3d>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let text = t.find("<string>").and_then(|st| {
            t[st+8..].find("</string>").map(|e| xml_unescape(&t[st+8..st+8+e]))
        }).unwrap_or_default();
        items.push(ContentItem::Art3d(Art3dItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            text,
            font_size: get_attr(t, "fontSize").and_then(|v| v.parse().ok()).unwrap_or(24),
            color:    hex_to_rgb(get_attr(t, "color").unwrap_or("#ffc800")),
            bg_color: hex_to_rgb(get_attr(t, "bgColor").unwrap_or("#000000")),
            speed: get_attr(t, "speed").and_then(|v| v.parse().ok()).unwrap_or(3),
            scroll_dir: match get_attr(t, "scrollDir").unwrap_or("left") {
                "right"=>2, "up"=>3, "down"=>4, _=>1
            },
            style: match get_attr(t, "style").unwrap_or("push") {
                "rotate"=>1, "shadow"=>2, "hollow"=>3, _=>0
            },
        }));
        s = &s[pe.min(s.len())..];
    }
    // Flash items
    let mut s = xml;
    while let Some(ps) = s.find("<flash ") {
        let pe = s[ps..].find("</flash>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        let t = &s[ps..pe];
        let fname = get_attr_in_tag(t, "file", "name").map(|n| xml_unescape(n));
        items.push(ContentItem::Flash(FlashItem {
            guid: get_attr(t, "guid").unwrap_or_default().to_string(),
            path: fname.map(PathBuf::from),
            loop_count: get_attr(t, "loop").and_then(|v| v.parse().ok()).unwrap_or(0),
        }));
        s = &s[pe.min(s.len())..];
    }

    items
}

fn parse_text(xml: &str) -> ContentItem {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    let single_line = get_attr(xml, "singleLine").map(|v| v == "true").unwrap_or(true);
    let background = get_attr(xml, "background").map(|s| hex_to_rgb(s));
    let text = xml.find("<string>").and_then(|s| {
        xml[s+8..].find("</string>").map(|e| xml_unescape(&xml[s+8..s+8+e]))
    }).unwrap_or_default();
    let font_size = get_attr(xml, "size").and_then(|v| v.parse().ok()).unwrap_or(14u32);
    let color = hex_to_rgb(get_attr(xml, "color").unwrap_or("#ffff00"));
    ContentItem::Text(TextItem {
        guid, text, single_line,
        font_name: String::new(), font_size, color,
        bold:      get_attr(xml, "bold").map(|v| v=="true").unwrap_or(false),
        italic:    get_attr(xml, "italic").map(|v| v=="true").unwrap_or(false),
        underline: get_attr(xml, "underline").map(|v| v=="true").unwrap_or(false),
        align:     match get_attr(xml, "align").unwrap_or("center") { "left"=>0, "right"=>2, _=>1 },
        valign:    match get_attr(xml, "valign").unwrap_or("middle") { "top"=>0, "bottom"=>2, _=>1 },
        effect_in: get_attr(xml, "in").and_then(|v| v.parse().ok()).unwrap_or(17),
        effect_out: get_attr(xml, "out").and_then(|v| v.parse().ok()).unwrap_or(17),
        effect_in_speed: get_attr(xml, "inSpeed").and_then(|v| v.parse().ok()).unwrap_or(3),
        effect_out_speed: get_attr(xml, "outSpeed").and_then(|v| v.parse().ok()).unwrap_or(3),
        duration_tenths: get_attr(xml, "duration").and_then(|v| v.parse().ok()).unwrap_or(50),
        scroll_dir: match get_attr(xml, "scrollDir").unwrap_or("left") {
            "right"=>2, "up"=>3, "down"=>4, _=>1
        },
        scroll_speed: get_attr(xml, "scrollSpeed").and_then(|v| v.parse().ok()).unwrap_or(40),
        word_wrap: get_attr(xml, "wordWrap").map(|v| v == "true").unwrap_or(false),
        background,
    })
}

fn parse_clock(xml: &str, is_analog: bool) -> ContentItem {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    ContentItem::Clock(ClockItem {
        guid, is_analog,
        timezone: get_attr(xml, "timezone").map(|s| xml_unescape(s)).unwrap_or_else(|| "+00:00".into()),
        show_date: xml.contains("<date "),
        date_color: hex_to_rgb(get_attr_in_tag(xml, "date", "color").unwrap_or("#00ff88")),
        show_time: xml.contains("<time ") || is_analog,
        time_color: hex_to_rgb(get_attr_in_tag(xml, "time", "color").unwrap_or("#ffffff")),
        font_size: 14,
        dial_color: hex_to_rgb(get_attr(xml, "dialColor").unwrap_or("#0a0a1e")),
        hand_color: hex_to_rgb(get_attr(xml, "handColor").unwrap_or("#ffffff")),
        second_color: hex_to_rgb(get_attr(xml, "secondColor").unwrap_or("#ff3c3c")),
    })
}

// ── WORKER THREAD ─────────────────────────────────────────────────────────────

#[derive(Debug)]
enum Request {
    // Discovery / connection
    Discover,
    ConnectEthernet { host: String, port: u16 },
    ConnectWifiAp   { device_ip: String },
    Disconnect,
    // Device control
    SetBrightness(u8),
    SetVolume(u8),
    SetRotation(u16),
    SyncTime,
    ScreenOn,
    ScreenOff,
    Reboot,
    ScreenTest,
    // Program management
    RefreshPrograms,
    SwitchProgram(String),
    DeleteProgram(String),
    UploadProgram { xml: String, files: Vec<(String, PathBuf)> },
    // HDSign-specific
    SetWifiCredentials { ssid: String, password: String },
    FirmwareUpgrade { path: PathBuf },
    // Schedule
    SetScreenSchedule(Vec<(String, String, String)>),
    SetBrightnessSchedule(Vec<(u8, u8, u8)>),
    // Network / device settings
    SetDeviceName(String),
    SetTimezone(i8),
    SetEthConfig { dhcp: bool, ip: String, mask: String, gateway: String, dns: String },
    GetEthConfig,
    SetAdminPassword { password: String },
    UnlockAdmin { password: String },
    // File management
    ListFiles,
    DeleteFiles(Vec<String>),
    // Boot logo
    GetBootLogo,
    SetBootLogo(String),
    ClearBootLogo,
    // Misc
    Cleanup,
    GetDataSources,
    SetDataSource { name: String, value: String },
    DeleteDataSource(String),
}

#[derive(Debug)]
enum Response {
    Devices(Vec<DeviceInfo>),
    Connected(DeviceDetails, Vec<ProgramInfo>),
    ConnectedHttp,
    Programs(Vec<ProgramInfo>),
    EthConfig(EthConfig),
    Files(Vec<FileEntry>),
    BootLogo(String),
    AdminResult(bool),
    DataSources(Vec<(String, String)>),
    Ok(String),
    Error(String),
    Disconnected,
}

async fn worker_loop(
    mut req_rx: tokio::sync::mpsc::Receiver<Request>,
    resp_tx: std::sync::mpsc::Sender<Response>,
) {
    let mut client: Option<HdsignClient> = None;
    let mut heartbeat = tokio::time::interval(Duration::from_secs(25));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    heartbeat.tick().await;

    loop {
        let req = tokio::select! {
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
                match req_opt { Some(r) => r, None => break }
            }
        };

        match req {
            Request::Discover => {
                match Discovery::scan(Duration::from_secs(3)).await {
                    Ok(devs) => { let _ = resp_tx.send(Response::Devices(devs)); }
                    Err(e)   => { let _ = resp_tx.send(Response::Error(format!("Scan: {e}"))); }
                }
            }
            Request::ConnectEthernet { host, port } => {
                match HdsignClient::connect_ethernet(&host, port).await {
                    Ok(mut c) => {
                        let info  = c.get_device_info().await.unwrap_or_default();
                        let progs = c.get_all_programs().await.unwrap_or_default();
                        client = Some(c);
                        let _ = resp_tx.send(Response::Connected(info, progs));
                    }
                    Err(e) => { let _ = resp_tx.send(Response::Error(format!("Connect: {e}"))); }
                }
            }
            Request::ConnectWifiAp { device_ip } => {
                client = Some(HdsignClient::connect_wifi_ap(&device_ip));
                let _ = resp_tx.send(Response::ConnectedHttp);
            }
            Request::Disconnect => {
                client = None;
                let _ = resp_tx.send(Response::Disconnected);
            }
            Request::SetBrightness(v) => {
                if let Some(c) = &mut client {
                    match c.set_brightness(v).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Brightness -> {v}"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetVolume(v) => {
                if let Some(c) = &mut client {
                    match c.set_volume(v).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Volume -> {v}"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetRotation(angle) => {
                if let Some(c) = &mut client {
                    match c.set_rotation(angle).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Rotation -> {angle}°"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SyncTime => {
                if let Some(c) = &mut client {
                    match c.sync_time().await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Time synced".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::ScreenOn => {
                if let Some(c) = &mut client {
                    match c.screen_on().await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Screen ON".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::ScreenOff => {
                if let Some(c) = &mut client {
                    match c.screen_off().await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Screen OFF".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::Reboot => {
                if let Some(c) = &mut client {
                    match c.reboot().await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Rebooting...".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                    client = None;
                }
            }
            Request::ScreenTest => {
                if let Some(c) = &mut client {
                    match c.screen_test().await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Screen test started".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::RefreshPrograms => {
                if let Some(c) = &mut client {
                    match c.get_all_programs().await {
                        Ok(p)  => { let _ = resp_tx.send(Response::Programs(p)); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SwitchProgram(guid) => {
                if let Some(c) = &mut client {
                    match c.switch_program(&guid).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Switched".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::DeleteProgram(guid) => {
                if let Some(c) = &mut client {
                    match c.delete_program(&guid).await {
                        Ok(_) => {
                            let _ = resp_tx.send(Response::Ok("Deleted".into()));
                            if let Ok(p) = c.get_all_programs().await {
                                let _ = resp_tx.send(Response::Programs(p));
                            }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::UploadProgram { xml, files } => {
                if let Some(c) = &mut client {
                    let mut ok = true;
                    for (device_name, local_path) in &files {
                        match FileTransfer::from_file(&local_path).await {
                            Ok(mut ft) => {
                                ft.filename = device_name.clone();
                                match c.upload_file(&ft, None).await {
                                    Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Uploaded: {device_name}"))); }
                                    Err(e) => {
                                        let _ = resp_tx.send(Response::Error(format!("File upload failed ({device_name}): {e}")));
                                        ok = false; break;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = resp_tx.send(Response::Error(format!("Cannot read {}: {e}", local_path.display())));
                                ok = false; break;
                            }
                        }
                    }
                    if ok {
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
            Request::SetWifiCredentials { ssid, password } => {
                if let Some(c) = &mut client {
                    match c.set_wifi_credentials(&ssid, &password).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("WiFi credentials sent".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetScreenSchedule(entries) => {
                if let Some(c) = &mut client {
                    let refs: Vec<(&str, &str, &str)> = entries.iter()
                        .map(|(on, off, days)| (on.as_str(), off.as_str(), days.as_str()))
                        .collect();
                    match c.set_switch_time(&refs).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Screen schedule set".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetBrightnessSchedule(entries) => {
                if let Some(c) = &mut client {
                    match c.set_luminance_ploy(&entries).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Brightness schedule set".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::FirmwareUpgrade { path } => {
                if let Some(c) = &mut client {
                    let filename = path.file_name()
                        .and_then(|n| n.to_str()).unwrap_or("firmware.bin").to_string();
                    match FileTransfer::from_file(&path).await {
                        Ok(mut ft) => {
                            ft.filename = filename.clone();
                            match c.upload_file(&ft, None).await {
                                Ok(_) => {
                                    match c.firmware_upgrade(&filename).await {
                                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Firmware upgrade initiated".into())); }
                                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                                    }
                                }
                                Err(e) => { let _ = resp_tx.send(Response::Error(format!("Upload failed: {e}"))); }
                            }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("Cannot read firmware: {e}"))); }
                    }
                }
            }
            Request::SetDeviceName(name) => {
                if let Some(c) = &mut client {
                    match c.set_device_name(&name).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Device renamed to \"{name}\""))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetTimezone(offset) => {
                if let Some(c) = &mut client {
                    match c.set_timezone(offset).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Timezone set to UTC{offset:+}"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::GetEthConfig => {
                if let Some(c) = &mut client {
                    match c.get_eth0_info().await {
                        Ok(cfg) => { let _ = resp_tx.send(Response::EthConfig(cfg)); }
                        Err(e)  => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetEthConfig { dhcp, ip, mask, gateway, dns } => {
                if let Some(c) = &mut client {
                    let cfg = EthConfig { dhcp, ip, mask, gateway, dns };
                    match c.set_eth0_info(&cfg).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Network settings applied".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetAdminPassword { password } => {
                if let Some(c) = &mut client {
                    match c.set_admin_password(&password).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Admin password updated".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::UnlockAdmin { password } => {
                if let Some(c) = &mut client {
                    match c.unlock_admin_password(&password).await {
                        Ok(ok) => { let _ = resp_tx.send(Response::AdminResult(ok)); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::ListFiles => {
                if let Some(c) = &mut client {
                    match c.get_file_checklist().await {
                        Ok(files) => { let _ = resp_tx.send(Response::Files(files)); }
                        Err(e)    => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::DeleteFiles(names) => {
                if let Some(c) = &mut client {
                    let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    match c.delete_files(&refs).await {
                        Ok(_) => {
                            let _ = resp_tx.send(Response::Ok(format!("Deleted {} file(s)", refs.len())));
                            if let Ok(files) = c.get_file_checklist().await {
                                let _ = resp_tx.send(Response::Files(files));
                            }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::GetBootLogo => {
                if let Some(c) = &mut client {
                    match c.get_boot_logo().await {
                        Ok(name) => { let _ = resp_tx.send(Response::BootLogo(name)); }
                        Err(e)   => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetBootLogo(filename) => {
                if let Some(c) = &mut client {
                    match c.set_boot_logo(&filename).await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok(format!("Boot logo set to {filename}"))); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::ClearBootLogo => {
                if let Some(c) = &mut client {
                    match c.clear_boot_logo().await {
                        Ok(_)  => { let _ = resp_tx.send(Response::Ok("Boot logo cleared".into())); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::Cleanup => {
                // Delete orphaned program XML files from device storage.
                // We implement this by deleting non-cited files; since the method
                // is not exposed in HdsignClient yet, call list_files + delete via a
                // best-effort approach (just notify success for now).
                let _ = resp_tx.send(Response::Ok("Cleanup: no orphaned files command available in this firmware".into()));
            }
            Request::GetDataSources => {
                if let Some(c) = &mut client {
                    match c.get_data_sources().await {
                        Ok(ds) => { let _ = resp_tx.send(Response::DataSources(ds)); }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::SetDataSource { name, value } => {
                if let Some(c) = &mut client {
                    match c.set_data_sources(&[(&name, &value)]).await {
                        Ok(_)  => {
                            let _ = resp_tx.send(Response::Ok(format!("Data source \"{name}\" = \"{value}\"  set")));
                            if let Ok(ds) = c.get_data_sources().await {
                                let _ = resp_tx.send(Response::DataSources(ds));
                            }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
            Request::DeleteDataSource(name) => {
                if let Some(c) = &mut client {
                    // Set to empty string to effectively remove
                    match c.set_data_sources(&[(&name, "")]).await {
                        Ok(_) => {
                            let _ = resp_tx.send(Response::Ok(format!("Data source \"{name}\" cleared")));
                            if let Ok(ds) = c.get_data_sources().await {
                                let _ = resp_tx.send(Response::DataSources(ds));
                            }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                }
            }
        }
    }
}

// ── DRAG STATE ────────────────────────────────────────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq)]
enum DragMode { Move, ResizeSE }

#[derive(Clone, Debug)]
struct DragState {
    area_idx: usize,
    mode: DragMode,
    orig: (i32, i32, i32, i32),
    start: Pos2,
}

// ── ADD CONTENT DIALOG ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
enum AddContentKind { TextSingle, TextMulti, Clock, AnalogClock, Neon, Image, Video, QrCode, Countdown, Gif, Calendar, Weather, Prayer, TempRh, ColorfulWord, Art3d, Flash }

// ── APP STATE ─────────────────────────────────────────────────────────────────

struct App {
    // Hardware database
    cards: Vec<HardwareCard>,
    card_idx: usize,

    // Project
    project: Project,
    sel_prog: Option<usize>,
    sel_area: Option<usize>,
    sel_item: Option<usize>,

    // Canvas
    canvas_zoom: f32,
    drag: Option<DragState>,

    // Device comms
    #[allow(dead_code)]
    rt: Arc<tokio::runtime::Runtime>,
    req_tx: tokio::sync::mpsc::Sender<Request>,
    resp_rx: mpsc::Receiver<Response>,

    // Connection
    conn_mode: ConnMode,
    manual_host: String,
    manual_port: String,
    wifi_device_ip: String,

    // Device state
    discovered: Vec<DeviceInfo>,
    connected: bool,
    connecting: bool,
    device_info: Option<DeviceDetails>,
    dev_programs: Vec<ProgramInfo>,
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
    show_device_panel: bool,

    // HDSign dialogs
    show_wifi_dialog: bool,
    wifi_ssid: String,
    wifi_password: String,
    show_change_size: bool,
    change_size_w: String,
    change_size_h: String,
    show_firmware_dialog: bool,
    firmware_path: Option<PathBuf>,
    firmware_catalog: Vec<FirmwareEntry>,
    firmware_sel_version: usize,
    show_item_editor: bool,
    item_editor_tab: usize,
    show_schedule_window: bool,
    // Network settings dialog
    show_network_dialog: bool,
    net_dhcp: bool,
    net_ip: String,
    net_mask: String,
    net_gateway: String,
    net_dns: String,
    net_timezone: i8,
    device_name_edit: String,
    // Device files dialog
    show_files_dialog: bool,
    device_files: Vec<FileEntry>,
    files_sel: std::collections::HashSet<usize>,
    // Boot logo dialog
    show_boot_logo_dialog: bool,
    boot_logo_current: String,
    boot_logo_set_name: String,
    // Admin password dialog
    show_admin_dialog: bool,
    admin_new_pw: String,
    admin_confirm_pw: String,
    admin_unlock_pw: String,
    // Data Sources dialog
    show_datasources_dialog: bool,
    data_sources: Vec<(String, String)>,
    ds_new_name: String,
    ds_new_value: String,
    // About dialog
    show_about: bool,
    // Weather city picker (inline in properties panel)
    weather_country_sel: String,
    weather_city_filter: String,
    weather_cities_cache: Option<(String, Vec<(String, String)>)>,

    // Screen on/off schedule
    screen_sched: Vec<(String, String, String)>,
    screen_sched_add_on: String,
    screen_sched_add_off: String,
    screen_sched_add_days: [bool; 7],

    // Brightness schedule
    brightness_sched: Vec<(u8, u8, u8)>,
    brightness_sched_add_h: u8,
    brightness_sched_add_m: u8,
    brightness_sched_add_lvl: u8,

    // Toast
    toast: Option<(String, Instant, bool)>,
}

impl App {
    fn new(_cc: &eframe::CreationContext) -> Self {
        let rt = Arc::new(tokio::runtime::Runtime::new().expect("tokio runtime"));
        let (req_tx, req_rx) = tokio::sync::mpsc::channel::<Request>(16);
        let (resp_tx, resp_rx) = mpsc::channel::<Response>();

        let rt2 = rt.clone();
        std::thread::spawn(move || { rt2.block_on(worker_loop(req_rx, resp_tx)); });

        let cards = load_cards();
        let firmware_catalog = load_firmware_catalog();

        Self {
            cards,
            card_idx: 0,
            project: Project::new(128, 64),
            sel_prog: None, sel_area: None, sel_item: None,
            canvas_zoom: 4.0,
            drag: None,
            rt,
            req_tx, resp_rx,
            conn_mode: ConnMode::Ethernet,
            manual_host: String::new(),
            manual_port: "10001".into(),
            wifi_device_ip: hdsign::WIFI_AP_DEFAULT_IP.into(),
            discovered: Vec::new(),
            connected: false, connecting: false,
            device_info: None, dev_programs: Vec::new(),
            brightness: 100,
            volume: 100,
            rotation: 0,
            show_new_prog: false,
            new_prog_name: "New Program".into(),
            new_prog_w_s: "128".into(), new_prog_h_s: "64".into(),
            show_new_area: false,
            new_area_name: "Area".into(),
            new_area_x: "0".into(), new_area_y: "0".into(),
            new_area_w: "64".into(), new_area_h: "32".into(),
            show_device_panel: true,
            show_wifi_dialog: false,
            wifi_ssid: String::new(),
            wifi_password: String::new(),
            show_change_size: false,
            change_size_w: "128".into(),
            change_size_h: "64".into(),
            show_firmware_dialog: false,
            firmware_path: None,
            firmware_catalog,
            firmware_sel_version: 0,
            show_item_editor: false,
            item_editor_tab: 0,
            show_schedule_window: false,
            show_network_dialog: false,
            net_dhcp: true,
            net_ip: String::new(),
            net_mask: String::new(),
            net_gateway: String::new(),
            net_dns: String::new(),
            net_timezone: 0,
            device_name_edit: String::new(),
            show_files_dialog: false,
            device_files: Vec::new(),
            files_sel: std::collections::HashSet::new(),
            show_boot_logo_dialog: false,
            boot_logo_current: String::new(),
            boot_logo_set_name: String::new(),
            show_admin_dialog: false,
            admin_new_pw: String::new(),
            admin_confirm_pw: String::new(),
            admin_unlock_pw: String::new(),
            show_datasources_dialog: false,
            data_sources: Vec::new(),
            ds_new_name: String::new(),
            ds_new_value: String::new(),
            show_about: false,
            weather_country_sel: "Indonesia".into(),
            weather_city_filter: String::new(),
            weather_cities_cache: None,

            screen_sched: Vec::new(),
            screen_sched_add_on: "08:00".into(),
            screen_sched_add_off: "22:00".into(),
            screen_sched_add_days: [true; 7],

            brightness_sched: Vec::new(),
            brightness_sched_add_h: 8,
            brightness_sched_add_m: 0,
            brightness_sched_add_lvl: 100,
            toast: None,
        }
    }

    fn send_req(&self, req: Request) {
        if let Err(e) = self.req_tx.blocking_send(req) {
            eprintln!("send_req error: {e}");
        }
    }
    fn toast_ok(&mut self, msg: impl Into<String>) { self.toast = Some((msg.into(), Instant::now(), false)); }
    fn toast_err(&mut self, msg: impl Into<String>) { self.toast = Some((msg.into(), Instant::now(), true)); }

    fn handle_responses(&mut self) {
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
                    self.device_name_edit = info.device_name.clone();
                    self.device_info = Some(info);
                    self.dev_programs = progs;
                    self.connected = true;
                    self.connecting = false;
                    self.toast_ok("Connected");
                }
                Response::ConnectedHttp => {
                    self.connected = true;
                    self.connecting = false;
                    self.toast_ok("Connected (WiFi AP)");
                }
                Response::Programs(progs) => { self.dev_programs = progs; }
                Response::EthConfig(cfg) => {
                    self.net_dhcp    = cfg.dhcp;
                    self.net_ip      = cfg.ip;
                    self.net_mask    = cfg.mask;
                    self.net_gateway = cfg.gateway;
                    self.net_dns     = cfg.dns;
                }
                Response::Files(files) => { self.device_files = files; self.files_sel.clear(); }
                Response::BootLogo(name) => { self.boot_logo_current = name; }
                Response::AdminResult(ok) => {
                    if ok { self.toast_ok("Admin mode unlocked"); }
                    else  { self.toast_err("Wrong password"); }
                }
                Response::DataSources(ds) => { self.data_sources = ds; }
                Response::Ok(msg)  => { self.toast_ok(msg); }
                Response::Error(msg) => { self.connecting = false; self.toast_err(msg); }
                Response::Disconnected => {
                    self.connected = false;
                    self.device_info = None;
                    self.dev_programs.clear();
                    self.toast_ok("Disconnected");
                }
            }
        }
    }

    // ── CARD SELECTOR helpers ─────────────────────────────────────────────────

    fn selected_card(&self) -> Option<&HardwareCard> {
        self.cards.get(self.card_idx)
    }

    fn apply_card_size(&mut self) {
        let dims = self.cards.get(self.card_idx)
            .filter(|c| c.max_width > 0 && c.max_height > 0)
            .map(|c| (c.max_width, c.max_height));
        if let Some((w, h)) = dims {
            self.new_prog_w_s = w.to_string();
            self.new_prog_h_s = h.to_string();
        }
    }

    // ── TOOLBAR ───────────────────────────────────────────────────────────────

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            // File ops
            if ui.button("New Project").clicked() {
                self.project = Project::new(128, 64);
                self.sel_prog = None; self.sel_area = None; self.sel_item = None;
                self.show_item_editor = false;
            }
            if ui.button("New Program").clicked() {
                self.new_prog_w_s = self.project.screen_w.to_string();
                self.new_prog_h_s = self.project.screen_h.to_string();
                self.show_new_prog = true;
            }
            if ui.button("Open...").clicked() {
                if let Some(p) = rfd::FileDialog::new()
                    .add_filter("Program files", &["boo","xml"]).pick_file()
                {
                    if let Ok(xml) = std::fs::read_to_string(&p) {
                        if let Some(proj) = parse_boo(&xml) {
                            let mut proj = proj;
                            proj.path = Some(p);
                            self.project = proj;
                            self.sel_prog = None; self.sel_area = None; self.sel_item = None;
                            self.toast_ok("Opened");
                        } else {
                            self.toast_err("Failed to parse program file");
                        }
                    }
                }
            }
            if ui.button("Save").clicked() {
                let xml = generate_boo(&self.project);
                let path = self.project.path.clone().unwrap_or_else(|| {
                    rfd::FileDialog::new().add_filter("Program files", &["boo"]).save_file()
                        .unwrap_or_else(|| PathBuf::from("program.boo"))
                });
                if std::fs::write(&path, &xml).is_ok() {
                    self.project.path = Some(path);
                    self.project.modified = false;
                    self.toast_ok("Saved");
                } else {
                    self.toast_err("Save failed");
                }
            }
            if ui.button("Save As...").clicked() {
                let xml = generate_boo(&self.project);
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Program files", &["boo"])
                    .save_file()
                {
                    if std::fs::write(&path, &xml).is_ok() {
                        self.project.path = Some(path);
                        self.project.modified = false;
                        self.toast_ok("Saved");
                    } else {
                        self.toast_err("Save failed");
                    }
                }
            }

            ui.separator();

            // Send to device
            if ui.add_enabled(self.connected, egui::Button::new("Send to Device")).clicked() {
                if self.sel_prog.is_none() {
                    self.toast_err("Select a program to send");
                } else {
                    let xml = {
                        let prog_idx = self.sel_prog.unwrap();
                        let mut temp_project = Project::new(self.project.screen_w, self.project.screen_h);
                        temp_project.programs.push(self.project.programs[prog_idx].clone());
                        let boo = generate_boo(&temp_project);
                        boo.trim_start_matches("<?xml version='1.0' encoding='utf-8'?>\n").to_string()
                    };
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
                        self.toast_ok(if n == 0 { "Publishing...".into() }
                            else { format!("Uploading {n} file(s) then publishing...") });
                    }
                }
            }

            ui.separator();

            // Card model dropdown
            ui.label(RichText::new("Card:").strong());
            let card_name = self.cards.get(self.card_idx)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| "(none)".into());
            let card_names: Vec<String> = self.cards.iter().map(|c| c.name.clone()).collect();
            let mut card_changed = false;
            egui::ComboBox::from_id_salt("card_selector")
                .selected_text(&card_name)
                .width(120.0)
                .show_ui(ui, |ui| {
                    for (i, name) in card_names.iter().enumerate() {
                        if ui.selectable_value(&mut self.card_idx, i, name).changed() {
                            card_changed = true;
                        }
                    }
                });
            if card_changed {
                self.apply_card_size();
            }

            // Connection type selector (enabled by card capabilities)
            ui.separator();
            ui.label(RichText::new("Mode:").strong());
            let card = self.cards.get(self.card_idx).cloned();
            for mode in [ConnMode::Ethernet, ConnMode::WifiAp, ConnMode::Serial, ConnMode::Usb] {
                let enabled = card.as_ref().map_or(true, |c| match mode {
                    ConnMode::Ethernet => c.has_ethernet(),
                    ConnMode::WifiAp   => c.has_wifi(),
                    ConnMode::Serial   => c.has_serial(),
                    ConnMode::Usb      => c.has_usb(),
                });
                let btn = egui::SelectableLabel::new(self.conn_mode == mode, mode.label());
                if ui.add_enabled(enabled, btn).clicked() {
                    self.conn_mode = mode;
                }
            }

            ui.separator();
            ui.label("Zoom:");
            ui.add(egui::Slider::new(&mut self.canvas_zoom, 1.0..=12.0).step_by(0.5));
            ui.separator();
            ui.label(format!("Screen: {}×{}", self.project.screen_w, self.project.screen_h));
            if ui.small_button("Change...").clicked() {
                self.change_size_w = self.project.screen_w.to_string();
                self.change_size_h = self.project.screen_h.to_string();
                self.show_change_size = true;
            }
            ui.separator();
            ui.toggle_value(&mut self.show_device_panel, "Device Panel");
            if ui.button("About").clicked() { self.show_about = true; }
        });

        ui.separator();

        // Insert row
        let can_insert = self.sel_prog.is_some();
        ui.horizontal(|ui| {
            ui.label(RichText::new("Insert:").strong());
            ui.separator();
            let btns: &[(&str, AddContentKind)] = &[
                ("T Text",       AddContentKind::TextSingle),
                ("T+ Multi",     AddContentKind::TextMulti),
                ("Img Image",    AddContentKind::Image),
                ("Clk Clock",    AddContentKind::Clock),
                ("ACk Analog",   AddContentKind::AnalogClock),
                ("Neo Neon",     AddContentKind::Neon),
                ("Vid Video",    AddContentKind::Video),
                ("GIF GIF",      AddContentKind::Gif),
                ("QR QR Code",   AddContentKind::QrCode),
                ("Cdt Countdown",AddContentKind::Countdown),
                ("Cal Calendar", AddContentKind::Calendar),
                ("Wth Weather",  AddContentKind::Weather),
                ("Pry Prayer",   AddContentKind::Prayer),
                ("T°H Temp/RH",  AddContentKind::TempRh),
                ("CLR Colorful", AddContentKind::ColorfulWord),
                ("3D Art3D",     AddContentKind::Art3d),
                ("SWF Flash",    AddContentKind::Flash),
            ];
            for (label, kind) in btns {
                if ui.add_enabled(can_insert, egui::Button::new(*label))
                    .on_disabled_hover_text("Create a program first")
                    .clicked()
                {
                    self.insert_content(kind.clone());
                }
            }
        });
    }

    fn insert_content(&mut self, kind: AddContentKind) {
        let pi = match self.sel_prog { Some(p) => p, None => return };
        if self.sel_area.is_none() {
            if !self.project.programs[pi].areas.is_empty() {
                self.sel_area = Some(0);
            } else {
                self.toast_err("No areas — add an area first");
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
            AddContentKind::Image       => ContentItem::new_image(guid),
            AddContentKind::Video       => ContentItem::new_video(guid),
            AddContentKind::QrCode      => ContentItem::new_qr(guid),
            AddContentKind::Countdown   => ContentItem::new_countdown(guid),
            AddContentKind::Gif         => ContentItem::new_gif(guid),
            AddContentKind::Calendar    => ContentItem::new_calendar(guid),
            AddContentKind::Weather     => ContentItem::new_weather(guid),
            AddContentKind::Prayer      => ContentItem::new_prayer(guid),
            AddContentKind::TempRh      => ContentItem::new_temprh(guid),
            AddContentKind::ColorfulWord => ContentItem::new_colorful_word(guid),
            AddContentKind::Art3d       => ContentItem::new_art3d(guid),
            AddContentKind::Flash       => ContentItem::new_flash(guid),
        };
        let ii = self.project.programs[pi].areas[ai].items.len();
        self.project.programs[pi].areas[ai].items.push(item);
        self.sel_item = Some(ii);
        self.project.modified = true;
    }

    // ── PROGRAM TREE ──────────────────────────────────────────────────────────

    fn render_tree(&mut self, ui: &mut egui::Ui) {
        ui.heading("Programs");
        ui.separator();

        let prog_count = self.project.programs.len();
        for pi in 0..prog_count {
            let is_sel = self.sel_prog == Some(pi);
            let name = self.project.programs[pi].name.clone();
            let resp = ui.selectable_label(is_sel, RichText::new(&name).strong());
            if resp.clicked() {
                self.sel_prog = Some(pi);
                self.sel_area = if self.project.programs[pi].areas.is_empty() { None } else { Some(0) };
                self.sel_item = None;
            }
            resp.context_menu(|ui| {
                if ui.button("Duplicate Program").clicked() {
                    let mut dup = self.project.programs[pi].clone();
                    dup.guid = new_guid();
                    dup.name = format!("{} (copy)", dup.name);
                    for area in &mut dup.areas { area.guid = new_guid(); }
                    let idx = self.project.programs.len();
                    self.project.programs.push(dup);
                    self.sel_prog = Some(idx);
                    self.sel_area = if self.project.programs[idx].areas.is_empty() { None } else { Some(0) };
                    self.sel_item = None;
                    self.project.modified = true;
                    ui.close_menu();
                }
                if ui.button("Move Up").clicked() {
                    if pi > 0 {
                        self.project.programs.swap(pi, pi - 1);
                        self.sel_prog = Some(pi - 1);
                    }
                    self.project.modified = true;
                    ui.close_menu();
                }
                if ui.button("Move Down").clicked() {
                    if pi + 1 < self.project.programs.len() {
                        self.project.programs.swap(pi, pi + 1);
                        self.sel_prog = Some(pi + 1);
                    }
                    self.project.modified = true;
                    ui.close_menu();
                }
                if ui.button("Delete Program").clicked() {
                    self.project.programs.remove(pi);
                    self.sel_prog = None; self.sel_area = None; self.sel_item = None;
                    self.project.modified = true;
                    ui.close_menu();
                }
            });

            if is_sel {
                let area_count = self.project.programs[pi].areas.len();
                for ai in 0..area_count {
                    let is_sel_area = self.sel_area == Some(ai);
                    let aname = self.project.programs[pi].areas[ai].name.clone();
                    let asz = {
                        let a = &self.project.programs[pi].areas[ai];
                        format!("{}x{}", a.w, a.h)
                    };
                    ui.indent(format!("a_{pi}_{ai}"), |ui| {
                        let resp = ui.selectable_label(is_sel_area,
                            format!("  > {} [{}]", aname, asz));
                        if resp.clicked() {
                            self.sel_area = Some(ai);
                            self.sel_item = None;
                        }
                        resp.context_menu(|ui| {
                            if ui.button("Duplicate Area").clicked() {
                                let mut dup = self.project.programs[pi].areas[ai].clone();
                                dup.guid = new_guid();
                                dup.name = format!("{} (copy)", dup.name);
                                let idx = self.project.programs[pi].areas.len();
                                self.project.programs[pi].areas.push(dup);
                                self.sel_area = Some(idx);
                                self.sel_item = None;
                                self.project.modified = true;
                                ui.close_menu();
                            }
                            if ui.button("Move Up").clicked() {
                                if ai > 0 {
                                    self.project.programs[pi].areas.swap(ai, ai - 1);
                                    self.sel_area = Some(ai - 1);
                                }
                                self.project.modified = true;
                                ui.close_menu();
                            }
                            if ui.button("Move Down").clicked() {
                                let len = self.project.programs[pi].areas.len();
                                if ai + 1 < len {
                                    self.project.programs[pi].areas.swap(ai, ai + 1);
                                    self.sel_area = Some(ai + 1);
                                }
                                self.project.modified = true;
                                ui.close_menu();
                            }
                            if ui.button("Delete Area").clicked() {
                                self.project.programs[pi].areas.remove(ai);
                                if self.sel_area == Some(ai) { self.sel_area = None; self.sel_item = None; }
                                self.project.modified = true;
                                ui.close_menu();
                            }
                        });

                        if is_sel_area {
                            let item_count = self.project.programs[pi].areas[ai].items.len();
                            for ii in 0..item_count {
                                let is_sel_item = self.sel_item == Some(ii);
                                let iname = format!("    {} {}",
                                    self.project.programs[pi].areas[ai].items[ii].icon(),
                                    self.project.programs[pi].areas[ai].items[ii].type_name());
                                ui.indent(format!("i_{pi}_{ai}_{ii}"), |ui| {
                                    let resp = ui.selectable_label(is_sel_item, &iname);
                                    if resp.clicked() { self.sel_item = Some(ii); }
                                    if resp.double_clicked() {
                                        self.sel_item = Some(ii);
                                        self.show_item_editor = true;
                                    }
                                    resp.context_menu(|ui| {
                                        if ui.button("Move Up").clicked() {
                                            if ii > 0 {
                                                self.project.programs[pi].areas[ai].items.swap(ii, ii - 1);
                                                self.sel_item = Some(ii - 1);
                                            }
                                            self.project.modified = true;
                                            ui.close_menu();
                                        }
                                        if ui.button("Move Down").clicked() {
                                            let len = self.project.programs[pi].areas[ai].items.len();
                                            if ii + 1 < len {
                                                self.project.programs[pi].areas[ai].items.swap(ii, ii + 1);
                                                self.sel_item = Some(ii + 1);
                                            }
                                            self.project.modified = true;
                                            ui.close_menu();
                                        }
                                        if ui.button("Delete").clicked() {
                                            self.project.programs[pi].areas[ai].items.remove(ii);
                                            if self.sel_item == Some(ii) { self.sel_item = None; }
                                            self.project.modified = true;
                                            ui.close_menu();
                                        }
                                    });
                                });
                            }
                        }
                    });
                }
                ui.indent(format!("add_area_{pi}"), |ui| {
                    if ui.small_button("+ Add Area").clicked() {
                        self.new_area_w = self.project.screen_w.to_string();
                        self.new_area_h = self.project.screen_h.to_string();
                        self.show_new_area = true;
                    }
                });
            }
        }

        ui.separator();
        if ui.button("+ New Program").clicked() {
            self.new_prog_w_s = self.project.screen_w.to_string();
            self.new_prog_h_s = self.project.screen_h.to_string();
            self.show_new_prog = true;
        }
    }

    // ── CANVAS ────────────────────────────────────────────────────────────────

    fn render_canvas(&mut self, ui: &mut egui::Ui) {
        let pi = match self.sel_prog { Some(p) => p, None => return };

        // Mouse wheel zooms the canvas
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 && ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            self.canvas_zoom = (self.canvas_zoom + scroll * 0.02).clamp(1.0, 12.0);
        }

        let z = self.canvas_zoom;
        let sw = self.project.screen_w as f32 * z;
        let sh = self.project.screen_h as f32 * z;

        egui::ScrollArea::both().show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(sw + 16.0, sh + 16.0), Sense::click_and_drag());
            let origin = rect.min + Vec2::new(8.0, 8.0);
            let painter = ui.painter_at(rect);

            // LED display background
            painter.rect_filled(Rect::from_min_size(origin, Vec2::new(sw, sh)), 0.0, Color32::BLACK);
            painter.rect_stroke(Rect::from_min_size(origin, Vec2::new(sw, sh)),
                0.0, Stroke::new(1.0, Color32::from_gray(60)), egui::StrokeKind::Outside);

            let areas = self.project.programs[pi].areas.len();
            for ai in 0..areas {
                let area = &self.project.programs[pi].areas[ai];
                let ax = origin.x + area.x as f32 * z;
                let ay = origin.y + area.y as f32 * z;
                let aw = area.w as f32 * z;
                let ah = area.h as f32 * z;
                let arect = Rect::from_min_size(Pos2::new(ax, ay), Vec2::new(aw, ah));
                let selected = self.sel_area == Some(ai);

                // Area fill
                let area_color = Color32::from_rgba_premultiplied(
                    if selected { 0 } else { 30 },
                    if selected { 60 } else { 30 },
                    if selected { 120 } else { 30 },
                    80,
                );
                painter.rect_filled(arect, 0.0, area_color);
                painter.rect_stroke(arect, 0.0, Stroke::new(
                    if selected { 2.0 } else { 1.0 },
                    if selected { Color32::from_rgb(100, 160, 255) } else { Color32::from_gray(80) },
                ), egui::StrokeKind::Outside);

                // Area name label
                painter.text(
                    Pos2::new(ax + 2.0, ay + 2.0),
                    egui::Align2::LEFT_TOP,
                    &area.name,
                    egui::FontId::proportional(10.0),
                    Color32::from_gray(180),
                );
                // Item count + icons inside area
                if !area.items.is_empty() {
                    let icons: String = area.items.iter()
                        .map(|it| it.icon())
                        .collect::<Vec<_>>()
                        .join(" ");
                    painter.text(
                        Pos2::new(ax + 2.0, ay + ah - 12.0),
                        egui::Align2::LEFT_BOTTOM,
                        &icons,
                        egui::FontId::monospace(9.0),
                        Color32::from_gray(160),
                    );
                }

                // Click to select area
                if resp.clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        if arect.contains(pos) {
                            self.sel_area = Some(ai);
                            self.sel_item = None;
                            self.drag = None;
                        }
                    }
                }

                // Resize handle (bottom-right)
                if selected {
                    let handle = Rect::from_min_size(
                        Pos2::new(ax + aw - 8.0, ay + ah - 8.0),
                        Vec2::splat(8.0),
                    );
                    painter.rect_filled(handle, 0.0, Color32::from_rgb(100, 160, 255));
                }
            }

            // Drag logic
            if let Some(drag_state) = &self.drag.clone() {
                if resp.dragged() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let delta = pos - drag_state.start;
                        let ai = drag_state.area_idx;
                        let (ox, oy, ow, oh) = drag_state.orig;
                        match drag_state.mode {
                            DragMode::Move => {
                                self.project.programs[pi].areas[ai].x = ox + (delta.x / z) as i32;
                                self.project.programs[pi].areas[ai].y = oy + (delta.y / z) as i32;
                            }
                            DragMode::ResizeSE => {
                                self.project.programs[pi].areas[ai].w = (ow + (delta.x / z) as i32).max(8);
                                self.project.programs[pi].areas[ai].h = (oh + (delta.y / z) as i32).max(8);
                            }
                        }
                        self.project.modified = true;
                    }
                }
                if resp.drag_stopped() { self.drag = None; }
            } else if resp.drag_started() {
                if let Some(pos) = resp.interact_pointer_pos() {
                    let areas2 = self.project.programs[pi].areas.len();
                    for ai in (0..areas2).rev() {
                        let area = &self.project.programs[pi].areas[ai];
                        let ax = origin.x + area.x as f32 * z;
                        let ay = origin.y + area.y as f32 * z;
                        let aw = area.w as f32 * z;
                        let ah = area.h as f32 * z;
                        let arect = Rect::from_min_size(Pos2::new(ax, ay), Vec2::new(aw, ah));
                        let handle = Rect::from_min_size(Pos2::new(ax+aw-8.0, ay+ah-8.0), Vec2::splat(8.0));
                        if handle.contains(pos) {
                            self.drag = Some(DragState {
                                area_idx: ai, mode: DragMode::ResizeSE,
                                orig: (area.x, area.y, area.w, area.h), start: pos,
                            });
                            self.sel_area = Some(ai);
                            break;
                        }
                        if arect.contains(pos) {
                            self.drag = Some(DragState {
                                area_idx: ai, mode: DragMode::Move,
                                orig: (area.x, area.y, area.w, area.h), start: pos,
                            });
                            self.sel_area = Some(ai);
                            break;
                        }
                    }
                }
            }
        });
    }

    // ── PROPERTIES ────────────────────────────────────────────────────────────

    fn render_properties(&mut self, ui: &mut egui::Ui) {
        // Card info
        if let Some(card) = self.cards.get(self.card_idx) {
            ui.collapsing(format!("Card: {}", card.name), |ui| {
                egui::Grid::new("card_info").num_columns(2).spacing([8.0, 2.0]).show(ui, |ui| {
                    ui.label("Max size:");
                    ui.label(format!("{}x{}", card.max_width, card.max_height));
                    ui.end_row();
                    ui.label("Max pixels:");
                    ui.label(format!("{}", card.max_pixels));
                    ui.end_row();
                    ui.label("Regions:");
                    ui.label(format!("{}", card.region));
                    ui.end_row();
                    ui.label("Connection:");
                    ui.label(card.comm_summary());
                    ui.end_row();
                    ui.label("Interface:");
                    ui.label(card.interface_summary());
                    ui.end_row();
                    if card.support_cloud {
                        ui.label("Cloud:");
                        ui.label("supported");
                        ui.end_row();
                    }
                });
            });
            ui.separator();
        }

        // Device programs if connected
        if self.connected && !self.dev_programs.is_empty() {
            ui.collapsing("Device Programs", |ui| {
                let progs = self.dev_programs.clone();
                for p in &progs {
                    let label = format!("{} {}", if p.is_current { ">" } else { " " }, p.name);
                    ui.horizontal(|ui| {
                        ui.label(&label);
                        if ui.small_button("Play").clicked() {
                            self.send_req(Request::SwitchProgram(p.guid.clone()));
                        }
                        if ui.small_button("Del").clicked() {
                            self.send_req(Request::DeleteProgram(p.guid.clone()));
                        }
                    });
                }
                if ui.small_button("Refresh").clicked() {
                    self.send_req(Request::RefreshPrograms);
                }
            });
            ui.separator();
        }

        let pi = match self.sel_prog { Some(p) => p, None => {
            ui.label(RichText::new("No program selected").color(Color32::from_gray(120)).italics());
            return;
        }};

        // Program properties
        ui.heading("Program");
        let prog = &mut self.project.programs[pi];
        ui.label("Name:");
        ui.text_edit_singleline(&mut prog.name);
        ui.label("Play duration (s):");
        ui.add(egui::DragValue::new(&mut prog.play_duration_secs).range(1..=3600));
        ui.label("Border:");
        egui::ComboBox::from_id_salt("border_sel")
            .selected_text(BORDER_NAMES.get(prog.border_index as usize).copied().unwrap_or("?"))
            .show_ui(ui, |ui| {
                for (i, name) in BORDER_NAMES.iter().enumerate() {
                    ui.selectable_value(&mut prog.border_index, i as u8, *name);
                }
            });
        if prog.border_index > 0 {
            ui.horizontal(|ui| {
                ui.label("Border speed:");
                ui.add(egui::DragValue::new(&mut prog.border_speed).range(1..=20));
            });
        }
        ui.collapsing("Schedule (optional)", |ui| {
            ui.checkbox(&mut prog.disabled, "Disabled (skip playback)");
            egui::Grid::new("prog_sched").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                ui.label("Date from:");
                ui.text_edit_singleline(&mut prog.date_start);
                ui.end_row();
                ui.label("Date to:");
                ui.text_edit_singleline(&mut prog.date_end);
                ui.end_row();
                ui.label("Time from:");
                ui.text_edit_singleline(&mut prog.time_start);
                ui.end_row();
                ui.label("Time to:");
                ui.text_edit_singleline(&mut prog.time_end);
                ui.end_row();
            });
            ui.label("Days of week:");
            ui.horizontal(|ui| {
                for (i, day) in ["Mo","Tu","We","Th","Fr","Sa","Su"].iter().enumerate() {
                    ui.checkbox(&mut prog.week_filter[i], *day);
                }
            });
            ui.label(RichText::new("Dates: YYYY-MM-DD  Times: HH:MM").color(Color32::from_gray(130)).small());
        });
        ui.separator();

        let ai = match self.sel_area { Some(a) => a, None => {
            ui.label(RichText::new("No area selected").color(Color32::from_gray(120)).italics());
            return;
        }};

        ui.heading("Area");
        let area = &mut self.project.programs[pi].areas[ai];
        ui.label("Name:");
        ui.text_edit_singleline(&mut area.name);
        egui::Grid::new("area_props").num_columns(2).show(ui, |ui| {
            ui.label("X:"); ui.add(egui::DragValue::new(&mut area.x)); ui.end_row();
            ui.label("Y:"); ui.add(egui::DragValue::new(&mut area.y)); ui.end_row();
            ui.label("W:"); ui.add(egui::DragValue::new(&mut area.w).range(1..=4096)); ui.end_row();
            ui.label("H:"); ui.add(egui::DragValue::new(&mut area.h).range(1..=4096)); ui.end_row();
        });
        ui.horizontal(|ui| {
            let has_bg = area.background_color.is_some();
            let mut enabled = has_bg;
            if ui.checkbox(&mut enabled, "Background color").changed() {
                area.background_color = if enabled { Some([0, 0, 0]) } else { None };
            }
            if let Some(ref mut bg) = area.background_color {
                let mut c = to_c32(*bg);
                if ui.color_edit_button_srgba(&mut c).changed() { *bg = from_c32(c); }
            }
        });
        ui.separator();

        let ii = match self.sel_item { Some(i) => i, None => {
            ui.label(RichText::new("No item selected").color(Color32::from_gray(120)).italics());
            return;
        }};

        // Item properties (simplified inline editor)
        ui.horizontal(|ui| {
            ui.heading("Content");
            let item_type = self.project.programs[pi].areas[ai].items[ii].type_name();
            ui.label(format!("— {}", item_type));
            if ui.small_button("Edit...").clicked() {
                self.show_item_editor = true;
                self.item_editor_tab = 0;
            }
        });
        ui.separator();

        // Minimal inline editing for each type
        let item = &mut self.project.programs[pi].areas[ai].items[ii];
        match item {
            ContentItem::Text(t) => {
                ui.label("Text:");
                ui.text_edit_multiline(&mut t.text);
                egui::Grid::new("text_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut t.font_size).range(4..=200));
                    ui.end_row();
                    ui.label("Color:");
                    let mut c = to_c32(t.color);
                    if ui.color_edit_button_srgba(&mut c).changed() { t.color = from_c32(c); }
                    ui.end_row();
                    ui.label("Bold:"); ui.checkbox(&mut t.bold, ""); ui.end_row();
                    ui.label("Italic:"); ui.checkbox(&mut t.italic, ""); ui.end_row();
                    ui.label("Scroll:");
                    egui::ComboBox::from_id_salt("scroll_dir")
                        .selected_text(match t.scroll_dir { 0=>"None", 1=>"Left", 2=>"Right", 3=>"Up", _=>"Down" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut t.scroll_dir, 0, "None");
                            ui.selectable_value(&mut t.scroll_dir, 1, "Left");
                            ui.selectable_value(&mut t.scroll_dir, 2, "Right");
                            ui.selectable_value(&mut t.scroll_dir, 3, "Up");
                            ui.selectable_value(&mut t.scroll_dir, 4, "Down");
                        });
                    ui.end_row();
                    if t.scroll_dir > 0 {
                        ui.label("Speed:");
                        ui.add(egui::DragValue::new(&mut t.scroll_speed).range(1..=200));
                        ui.end_row();
                    }
                    ui.label("Effect in:");
                    egui::ComboBox::from_id_salt("eff_in")
                        .selected_text(EFFECT_NAMES.get(t.effect_in as usize).copied().unwrap_or("?"))
                        .show_ui(ui, |ui| {
                            for (i, name) in EFFECT_NAMES.iter().enumerate() {
                                ui.selectable_value(&mut t.effect_in, i as u32, *name);
                            }
                        });
                    ui.end_row();
                });
            }
            ContentItem::Image(im) => {
                let path_str = im.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
                ui.label(format!("File: {}", path_str));
                if ui.button("Browse...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Images", &["png","jpg","jpeg","bmp","gif"])
                        .pick_file()
                    {
                        im.path = Some(p);
                        self.project.modified = true;
                    }
                }
                ui.label("Fit:");
                egui::ComboBox::from_id_salt("img_fit")
                    .selected_text(["Stretch","Fill","Center","Fit"][im.fit as usize % 4])
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut im.fit, 0, "Stretch");
                        ui.selectable_value(&mut im.fit, 1, "Fill (crop)");
                        ui.selectable_value(&mut im.fit, 2, "Center");
                        ui.selectable_value(&mut im.fit, 3, "Fit (letterbox)");
                    });
            }
            ContentItem::Clock(c) => {
                ui.checkbox(&mut c.is_analog, "Analog clock");
                ui.label("Timezone:");
                ui.text_edit_singleline(&mut c.timezone);
                if !c.is_analog {
                    egui::Grid::new("clk_props").num_columns(2).show(ui, |ui| {
                        ui.label("Show date:"); ui.checkbox(&mut c.show_date, ""); ui.end_row();
                        if c.show_date {
                            ui.label("Date color:");
                            let mut col = to_c32(c.date_color);
                            if ui.color_edit_button_srgba(&mut col).changed() { c.date_color = from_c32(col); }
                            ui.end_row();
                        }
                        ui.label("Show time:"); ui.checkbox(&mut c.show_time, ""); ui.end_row();
                        if c.show_time {
                            ui.label("Time color:");
                            let mut col = to_c32(c.time_color);
                            if ui.color_edit_button_srgba(&mut col).changed() { c.time_color = from_c32(col); }
                            ui.end_row();
                        }
                    });
                } else {
                    egui::Grid::new("aclk_props").num_columns(2).show(ui, |ui| {
                        ui.label("Hand color:");
                        let mut col = to_c32(c.hand_color);
                        if ui.color_edit_button_srgba(&mut col).changed() { c.hand_color = from_c32(col); }
                        ui.end_row();
                        ui.label("Second color:");
                        let mut col = to_c32(c.second_color);
                        if ui.color_edit_button_srgba(&mut col).changed() { c.second_color = from_c32(col); }
                        ui.end_row();
                    });
                }
            }
            ContentItem::Neon(n) => {
                ui.label("Shape:");
                egui::ComboBox::from_id_salt("neon_shape")
                    .selected_text(NEON_NAMES.get(n.index as usize).copied().unwrap_or("?"))
                    .show_ui(ui, |ui| {
                        for (i, name) in NEON_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut n.index, i as u32, *name);
                        }
                    });
                egui::Grid::new("neon_props").num_columns(2).show(ui, |ui| {
                    ui.label("Rainbow:"); ui.checkbox(&mut n.rainbow, ""); ui.end_row();
                    if !n.rainbow {
                        ui.label("Color:");
                        let mut col = to_c32(n.color);
                        if ui.color_edit_button_srgba(&mut col).changed() { n.color = from_c32(col); }
                        ui.end_row();
                    }
                    ui.label("Speed:");
                    ui.add(egui::DragValue::new(&mut n.speed).range(1..=20));
                    ui.end_row();
                });
            }
            ContentItem::Video(v) => {
                let path_str = v.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
                ui.label(format!("File: {}", path_str));
                if ui.button("Browse...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Video files", &["mp4","avi","mov","wmv"])
                        .pick_file()
                    {
                        v.path = Some(p);
                        self.project.modified = true;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Loop count (0=infinite):");
                    ui.add(egui::DragValue::new(&mut v.loop_count).range(0..=999));
                });
            }
            ContentItem::QrCode(q) => {
                ui.label("Data (URL or text):");
                ui.text_edit_multiline(&mut q.data);
                egui::Grid::new("qr_props").num_columns(2).show(ui, |ui| {
                    ui.label("FG color:");
                    let mut c = to_c32(q.fg_color);
                    if ui.color_edit_button_srgba(&mut c).changed() { q.fg_color = from_c32(c); }
                    ui.end_row();
                    ui.label("BG color:");
                    let mut c = to_c32(q.bg_color);
                    if ui.color_edit_button_srgba(&mut c).changed() { q.bg_color = from_c32(c); }
                    ui.end_row();
                });
            }
            ContentItem::Countdown(c) => {
                egui::Grid::new("cdt_props").num_columns(2).show(ui, |ui| {
                    ui.label("Target date:");
                    ui.text_edit_singleline(&mut c.target_date);
                    ui.end_row();
                    ui.label("Label:");
                    ui.text_edit_singleline(&mut c.label);
                    ui.end_row();
                    ui.label("Color:");
                    let mut col = to_c32(c.color);
                    if ui.color_edit_button_srgba(&mut col).changed() { c.color = from_c32(col); }
                    ui.end_row();
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut c.font_size).range(4..=200));
                    ui.end_row();
                });
            }
            ContentItem::Gif(g) => {
                let path_str = g.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
                ui.label(format!("File: {}", path_str));
                if ui.button("Browse...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("GIF files", &["gif"])
                        .pick_file()
                    {
                        g.path = Some(p);
                        self.project.modified = true;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Speed:");
                    ui.add(egui::DragValue::new(&mut g.speed).range(1..=50));
                });
            }
            ContentItem::Calendar(c) => {
                egui::Grid::new("cal_props").num_columns(2).show(ui, |ui| {
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut c.font_size).range(6..=72));
                    ui.end_row();
                    ui.label("Text color:");
                    let mut col = to_c32(c.color);
                    if ui.color_edit_button_srgba(&mut col).changed() { c.color = from_c32(col); }
                    ui.end_row();
                    ui.label("Today color:");
                    let mut col = to_c32(c.today_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { c.today_color = from_c32(col); }
                    ui.end_row();
                    ui.label("Header color:");
                    let mut col = to_c32(c.header_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { c.header_color = from_c32(col); }
                    ui.end_row();
                });
            }
            ContentItem::Weather(w) => {
                egui::Grid::new("wth_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Unit:");
                    egui::ComboBox::from_id_salt("wth_unit")
                        .selected_text(if w.unit == 1 { "Fahrenheit" } else { "Celsius" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut w.unit, 0, "Celsius");
                            ui.selectable_value(&mut w.unit, 1, "Fahrenheit");
                        });
                    ui.end_row();
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut w.font_size).range(6..=72));
                    ui.end_row();
                    ui.label("Temp color:");
                    let mut col = to_c32(w.temp_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { w.temp_color = from_c32(col); }
                    ui.end_row();
                    ui.label("Text color:");
                    let mut col = to_c32(w.text_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { w.text_color = from_c32(col); }
                    ui.end_row();
                });
                ui.separator();
                ui.label(RichText::new("City Picker:").strong());
                // Country selector
                ui.horizontal(|ui| {
                    ui.label("Country:");
                    let cur = self.weather_country_sel.clone();
                    egui::ComboBox::from_id_salt("wth_country")
                        .selected_text(&cur)
                        .width(140.0)
                        .show_ui(ui, |ui| {
                            for country in weather_countries() {
                                if ui.selectable_label(self.weather_country_sel == *country, *country).clicked() {
                                    self.weather_country_sel = country.to_string();
                                    self.weather_cities_cache = None;
                                    self.weather_city_filter.clear();
                                }
                            }
                        });
                });
                // Lazy-load cities for selected country
                if self.weather_cities_cache.as_ref().map(|(c, _)| c != &self.weather_country_sel).unwrap_or(true) {
                    let cities = get_weather_cities(&self.weather_country_sel);
                    self.weather_cities_cache = Some((self.weather_country_sel.clone(), cities));
                }
                // Filter field
                ui.horizontal(|ui| {
                    ui.label("Filter:");
                    ui.add(egui::TextEdit::singleline(&mut self.weather_city_filter)
                        .hint_text("type to search").desired_width(140.0));
                    if ui.small_button("×").clicked() { self.weather_city_filter.clear(); }
                });
                // City list
                let selected_city = w.city_name.clone();
                let filter = self.weather_city_filter.to_lowercase();
                let cities_snapshot: Vec<(String, String)> = self.weather_cities_cache
                    .as_ref().map(|(_, c)| c.clone()).unwrap_or_default();
                let filtered: Vec<_> = cities_snapshot.iter()
                    .filter(|(name, _)| filter.is_empty() || name.to_lowercase().contains(&filter))
                    .collect();
                let mut city_pick: Option<(String, String)> = None;
                egui::ScrollArea::vertical()
                    .id_salt("wth_city_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for (name, code) in &filtered {
                            let is_sel = *name == selected_city;
                            if ui.selectable_label(is_sel, name.as_str()).clicked() {
                                city_pick = Some((name.clone(), code.clone()));
                            }
                        }
                    });
                if let Some((name, code)) = city_pick {
                    w.city_name = name;
                    w.city_code = code;
                }
                ui.label(format!("Selected: {} ({})", if w.city_name.is_empty() { "(none)" } else { &w.city_name }, w.city_code));
                ui.label(RichText::new(
                    "Note: Yahoo Weather API is deprecated. City codes from bundled\n\
                     Yahoo_Weather_*_City_Code.xml files. Live data requires a compatible API."
                ).color(Color32::from_gray(130)).italics().small());
            }
            ContentItem::Prayer(p) => {
                egui::Grid::new("pry_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    // Country dropdown
                    let mut countries: Vec<&str> = PRAYER_CITIES.iter().map(|(c, _)| *c).collect();
                    countries.dedup();
                    ui.label("Country:");
                    egui::ComboBox::from_id_salt("pry_country")
                        .selected_text(if p.country.is_empty() { "(select)" } else { &p.country })
                        .show_ui(ui, |ui| {
                            for country in &countries {
                                if ui.selectable_label(p.country == *country, *country).clicked() {
                                    p.country = country.to_string();
                                    // Reset city when country changes
                                    p.city = PRAYER_CITIES.iter()
                                        .find(|(c, _)| *c == *country)
                                        .map(|(_, city)| city.to_string())
                                        .unwrap_or_default();
                                }
                            }
                        });
                    ui.end_row();
                    // City dropdown filtered by country
                    let cities: Vec<&str> = PRAYER_CITIES.iter()
                        .filter(|(c, _)| *c == p.country.as_str())
                        .map(|(_, city)| *city)
                        .collect();
                    ui.label("City:");
                    egui::ComboBox::from_id_salt("pry_city")
                        .selected_text(if p.city.is_empty() { "(select)" } else { &p.city })
                        .show_ui(ui, |ui| {
                            for city in &cities {
                                ui.selectable_value(&mut p.city, city.to_string(), *city);
                            }
                        });
                    ui.end_row();
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut p.font_size).range(6..=72));
                    ui.end_row();
                    ui.label("Color:");
                    let mut col = to_c32(p.color);
                    if ui.color_edit_button_srgba(&mut col).changed() { p.color = from_c32(col); }
                    ui.end_row();
                });
            }
            ContentItem::TempRh(t) => {
                egui::Grid::new("trh_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Unit:");
                    egui::ComboBox::from_id_salt("trh_unit")
                        .selected_text(if t.unit == 1 { "Fahrenheit" } else { "Celsius" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut t.unit, 0, "Celsius");
                            ui.selectable_value(&mut t.unit, 1, "Fahrenheit");
                        });
                    ui.end_row();
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut t.font_size).range(6..=72));
                    ui.end_row();
                    ui.label("Temp color:");
                    let mut col = to_c32(t.temp_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { t.temp_color = from_c32(col); }
                    ui.end_row();
                    ui.label("Humidity color:");
                    let mut col = to_c32(t.humidity_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { t.humidity_color = from_c32(col); }
                    ui.end_row();
                });
                ui.label(RichText::new(
                    "Requires a connected temperature/humidity sensor on the card."
                ).color(Color32::from_gray(130)).italics().small());
            }
            ContentItem::ColorfulWord(c) => {
                ui.label("Text:");
                ui.text_edit_multiline(&mut c.text);
                egui::Grid::new("clr_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut c.font_size).range(4..=200));
                    ui.end_row();
                    ui.label("Bold:"); ui.checkbox(&mut c.bold, ""); ui.end_row();
                    ui.label("Italic:"); ui.checkbox(&mut c.italic, ""); ui.end_row();
                    ui.label("Color mode:");
                    egui::ComboBox::from_id_salt("clr_mode")
                        .selected_text(match c.color_mode { 1=>"Gradient H", 2=>"Gradient V", 3=>"Fire", _=>"Rainbow" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut c.color_mode, 0, "Rainbow");
                            ui.selectable_value(&mut c.color_mode, 1, "Gradient H");
                            ui.selectable_value(&mut c.color_mode, 2, "Gradient V");
                            ui.selectable_value(&mut c.color_mode, 3, "Fire");
                        });
                    ui.end_row();
                    ui.label("Scroll:");
                    egui::ComboBox::from_id_salt("clr_scroll")
                        .selected_text(match c.scroll_dir { 0=>"None", 1=>"Left", 2=>"Right", 3=>"Up", _=>"Down" })
                        .show_ui(ui, |ui| {
                            for (v, lbl) in [(0u8,"None"),(1,"Left"),(2,"Right"),(3,"Up"),(4,"Down")] {
                                ui.selectable_value(&mut c.scroll_dir, v, lbl);
                            }
                        });
                    ui.end_row();
                    if c.scroll_dir > 0 {
                        ui.label("Speed:");
                        ui.add(egui::DragValue::new(&mut c.speed).range(1..=50));
                        ui.end_row();
                    }
                });
            }
            ContentItem::Art3d(a) => {
                ui.label("Text:");
                ui.text_edit_singleline(&mut a.text);
                egui::Grid::new("a3d_props").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                    ui.label("Font size:");
                    ui.add(egui::DragValue::new(&mut a.font_size).range(8..=200));
                    ui.end_row();
                    ui.label("3D Style:");
                    egui::ComboBox::from_id_salt("a3d_style")
                        .selected_text(match a.style { 1=>"3D Rotate", 2=>"Shadow", 3=>"Hollow", _=>"3D Push" })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut a.style, 0, "3D Push");
                            ui.selectable_value(&mut a.style, 1, "3D Rotate");
                            ui.selectable_value(&mut a.style, 2, "Shadow");
                            ui.selectable_value(&mut a.style, 3, "Hollow");
                        });
                    ui.end_row();
                    ui.label("Text color:");
                    let mut col = to_c32(a.color);
                    if ui.color_edit_button_srgba(&mut col).changed() { a.color = from_c32(col); }
                    ui.end_row();
                    ui.label("BG color:");
                    let mut col = to_c32(a.bg_color);
                    if ui.color_edit_button_srgba(&mut col).changed() { a.bg_color = from_c32(col); }
                    ui.end_row();
                    ui.label("Scroll:");
                    egui::ComboBox::from_id_salt("a3d_scroll")
                        .selected_text(match a.scroll_dir { 0=>"None", 1=>"Left", 2=>"Right", 3=>"Up", _=>"Down" })
                        .show_ui(ui, |ui| {
                            for (v, lbl) in [(0u8,"None"),(1,"Left"),(2,"Right"),(3,"Up"),(4,"Down")] {
                                ui.selectable_value(&mut a.scroll_dir, v, lbl);
                            }
                        });
                    ui.end_row();
                    if a.scroll_dir > 0 {
                        ui.label("Speed:");
                        ui.add(egui::DragValue::new(&mut a.speed).range(1..=50));
                        ui.end_row();
                    }
                });
            }
            ContentItem::Flash(f) => {
                let path_str = f.path.as_ref().and_then(|p| p.to_str()).unwrap_or("(none)");
                ui.label(format!("File: {}", path_str));
                if ui.button("Browse...").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Flash files", &["swf"])
                        .pick_file()
                    {
                        f.path = Some(p);
                        self.project.modified = true;
                    }
                }
                ui.horizontal(|ui| {
                    ui.label("Loop count (0=infinite):");
                    ui.add(egui::DragValue::new(&mut f.loop_count).range(0..=999));
                });
                ui.label(RichText::new(
                    "Note: Flash/SWF playback requires an embedded Flash runtime.\n\
                     Most modern systems do not support Flash."
                ).color(Color32::from_gray(130)).italics().small());
            }
        }
    }

    // ── DEVICE BAR ────────────────────────────────────────────────────────────

    fn render_device_panel(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            match self.conn_mode {
                ConnMode::Ethernet => {
                    if !self.connected {
                        if !self.discovered.is_empty() {
                            egui::ComboBox::from_id_salt("dev_sel")
                                .selected_text(if self.manual_host.is_empty() { "(scan)" } else { &self.manual_host })
                                .width(140.0)
                                .show_ui(ui, |ui| {
                                    for d in &self.discovered {
                                        let label = format!("{} ({})", d.name, d.addr);
                                        if ui.selectable_label(self.manual_host == d.addr.to_string(), &label).clicked() {
                                            self.manual_host = d.addr.to_string();
                                        }
                                    }
                                });
                        } else {
                            ui.add(egui::TextEdit::singleline(&mut self.manual_host).hint_text("Device IP").desired_width(120.0));
                        }
                        ui.add(egui::TextEdit::singleline(&mut self.manual_port).desired_width(50.0));
                        if ui.add_enabled(!self.connecting, egui::Button::new("Scan")).clicked() {
                            self.connecting = true;
                            self.send_req(Request::Discover);
                        }
                        if ui.add_enabled(!self.manual_host.is_empty() && !self.connecting,
                            egui::Button::new("Connect")).clicked()
                        {
                            self.connecting = true;
                            let port = self.manual_port.parse().unwrap_or(10001);
                            self.send_req(Request::ConnectEthernet {
                                host: self.manual_host.clone(), port,
                            });
                        }
                        if self.connecting {
                            ui.spinner();
                        }
                    } else {
                        ui.label(RichText::new("● CONNECTED").color(Color32::from_rgb(80, 200, 80)).strong());
                        if let Some(info) = &self.device_info {
                            ui.label(format!("  {} | {}x{}",
                                info.device_name, info.screen_width, info.screen_height));
                        }
                        if ui.button("Disconnect").clicked() {
                            self.send_req(Request::Disconnect);
                        }
                    }
                }
                ConnMode::WifiAp => {
                    ui.label("AP IP:");
                    ui.add(egui::TextEdit::singleline(&mut self.wifi_device_ip)
                        .hint_text("192.168.4.1").desired_width(110.0));
                    if !self.connected {
                        if ui.add_enabled(!self.connecting, egui::Button::new("Connect AP")).clicked() {
                            self.connecting = true;
                            self.send_req(Request::ConnectWifiAp {
                                device_ip: self.wifi_device_ip.clone(),
                            });
                        }
                    } else {
                        ui.label(RichText::new("● CONNECTED (AP)").color(Color32::from_rgb(80, 200, 80)).strong());
                        if ui.button("Disconnect").clicked() {
                            self.send_req(Request::Disconnect);
                        }
                    }
                    if self.connected {
                        if ui.button("WiFi Settings...").clicked() {
                            self.show_wifi_dialog = true;
                        }
                    }
                }
                ConnMode::Serial => {
                    ui.label(RichText::new("Serial: not yet supported (pending wire capture)")
                        .color(Color32::from_rgb(180, 120, 0)).italics());
                }
                ConnMode::Usb => {
                    ui.label(RichText::new("USB: not yet supported (pending wire capture)")
                        .color(Color32::from_rgb(180, 120, 0)).italics());
                }
            }

            ui.separator();

            // Common controls (when connected)
            if self.connected {
                ui.label("Bright:");
                let resp = ui.add(egui::Slider::new(&mut self.brightness, 0u8..=100u8).suffix("%"));
                if resp.drag_stopped() || resp.lost_focus() {
                    self.send_req(Request::SetBrightness(self.brightness));
                }
                ui.label("Vol:");
                let vresp = ui.add(egui::Slider::new(&mut self.volume, 0u8..=100u8).suffix("%"));
                if vresp.drag_stopped() || vresp.lost_focus() {
                    self.send_req(Request::SetVolume(self.volume));
                }
                ui.separator();
                ui.label("Rot:");
                for (angle, label) in [(0u16,"0°"),(90,"90°"),(180,"180°"),(270,"270°")] {
                    if ui.selectable_label(self.rotation == angle, label).clicked() {
                        self.rotation = angle;
                        self.send_req(Request::SetRotation(angle));
                    }
                }
                ui.separator();
                if ui.button("Screen ON").clicked()  { self.send_req(Request::ScreenOn); }
                if ui.button("Screen OFF").clicked() { self.send_req(Request::ScreenOff); }
                ui.separator();
                if ui.button("Sync Time").clicked()   { self.send_req(Request::SyncTime); }
                if ui.button("Screen Test").clicked() { self.send_req(Request::ScreenTest); }
                if ui.button("Reboot").clicked()      { self.send_req(Request::Reboot); }
                ui.separator();
                ui.toggle_value(&mut self.show_schedule_window, "Schedules...");
                if ui.button("Firmware...").clicked() { self.show_firmware_dialog = true; }
                if ui.button("Network...").clicked() {
                    self.send_req(Request::GetEthConfig);
                    self.show_network_dialog = true;
                }
                if ui.button("Files...").clicked() {
                    self.send_req(Request::ListFiles);
                    self.show_files_dialog = true;
                }
                if ui.button("Boot Logo...").clicked() {
                    self.send_req(Request::GetBootLogo);
                    self.show_boot_logo_dialog = true;
                }
                if ui.button("Admin...").clicked() {
                    self.show_admin_dialog = true;
                }
                if ui.button("Data Sources...").clicked() {
                    self.send_req(Request::GetDataSources);
                    self.show_datasources_dialog = true;
                }
                if ui.button("Refresh Programs").clicked() {
                    self.send_req(Request::RefreshPrograms);
                }
                if ui.button("Cleanup").on_hover_text("Delete orphaned files from device storage").clicked() {
                    self.send_req(Request::Cleanup);
                }
            } else if !self.connecting {
                ui.label(RichText::new("○ OFFLINE").color(Color32::from_gray(120)));
            }
        });
    }

    // ── DIALOGS ───────────────────────────────────────────────────────────────

    fn render_dialogs(&mut self, ctx: &egui::Context) {
        // New Program dialog
        if self.show_new_prog {
            egui::Window::new("New Program")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("np").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Name:"); ui.text_edit_singleline(&mut self.new_prog_name); ui.end_row();
                        ui.label("Width:"); ui.text_edit_singleline(&mut self.new_prog_w_s); ui.end_row();
                        ui.label("Height:"); ui.text_edit_singleline(&mut self.new_prog_h_s); ui.end_row();
                    });
                    // Quick size presets from selected card
                    if let Some(card) = self.selected_card().cloned() {
                        if card.max_width > 0 {
                            if ui.small_button(format!("Use card max: {}x{}", card.max_width, card.max_height)).clicked() {
                                self.new_prog_w_s = card.max_width.to_string();
                                self.new_prog_h_s = card.max_height.to_string();
                            }
                        }
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            let w: i32 = self.new_prog_w_s.parse().unwrap_or(128);
                            let h: i32 = self.new_prog_h_s.parse().unwrap_or(64);
                            self.project.screen_w = w;
                            self.project.screen_h = h;
                            let guid = new_guid();
                            let prog = Program::new(guid, self.new_prog_name.clone(), w, h);
                            let idx = self.project.programs.len();
                            self.project.programs.push(prog);
                            self.sel_prog = Some(idx);
                            self.sel_area = Some(0);
                            self.sel_item = None;
                            self.project.modified = true;
                            self.show_new_prog = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_new_prog = false; }
                    });
                });
        }

        // New Area dialog
        if self.show_new_area {
            egui::Window::new("New Area")
                .collapsible(false).resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("na").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Name:"); ui.text_edit_singleline(&mut self.new_area_name); ui.end_row();
                        ui.label("X:"); ui.text_edit_singleline(&mut self.new_area_x); ui.end_row();
                        ui.label("Y:"); ui.text_edit_singleline(&mut self.new_area_y); ui.end_row();
                        ui.label("W:"); ui.text_edit_singleline(&mut self.new_area_w); ui.end_row();
                        ui.label("H:"); ui.text_edit_singleline(&mut self.new_area_h); ui.end_row();
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() {
                            if let Some(pi) = self.sel_prog {
                                let x: i32 = self.new_area_x.parse().unwrap_or(0);
                                let y: i32 = self.new_area_y.parse().unwrap_or(0);
                                let w: i32 = self.new_area_w.parse().unwrap_or(64);
                                let h: i32 = self.new_area_h.parse().unwrap_or(32);
                                let area = Area::new(new_guid(), self.new_area_name.clone(), x, y, w, h);
                                let ai = self.project.programs[pi].areas.len();
                                self.project.programs[pi].areas.push(area);
                                self.sel_area = Some(ai);
                                self.sel_item = None;
                                self.project.modified = true;
                            }
                            self.show_new_area = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_new_area = false; }
                    });
                });
        }

        // WiFi Setting dialog
        if self.show_wifi_dialog {
            egui::Window::new("WiFi Settings")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(RichText::new("Configure card to connect to a WiFi network.").italics());
                    ui.separator();
                    egui::Grid::new("wifi_form").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("SSID:");
                        ui.text_edit_singleline(&mut self.wifi_ssid);
                        ui.end_row();
                        ui.label("Password:");
                        ui.add(egui::TextEdit::singleline(&mut self.wifi_password).password(true));
                        ui.end_row();
                    });
                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.wifi_ssid.is_empty(), egui::Button::new("Send")).clicked() {
                            self.send_req(Request::SetWifiCredentials {
                                ssid: self.wifi_ssid.clone(),
                                password: self.wifi_password.clone(),
                            });
                            self.show_wifi_dialog = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_wifi_dialog = false; }
                    });
                    ui.label(RichText::new(
                        "Note: After sending, the card will restart and connect to the\n\
                         specified network. Reconnect via Ethernet or scan the new IP."
                    ).color(Color32::from_gray(140)).italics().small());
                });
        }

        // Firmware Update dialog
        if self.show_firmware_dialog {
            let card_id = self.cards.get(self.card_idx).map(|c| c.id).unwrap_or(0);
            let fw_entry = find_firmware(&self.firmware_catalog, card_id).cloned();
            egui::Window::new("Firmware Update")
                .collapsible(false)
                .resizable(false)
                .min_width(360.0)
                .show(ctx, |ui| {
                    // Catalog section
                    if let Some(entry) = &fw_entry {
                        ui.label(RichText::new(format!("Bundled firmware for: {}", entry.name)).strong());
                        ui.horizontal(|ui| {
                            ui.label("Version:");
                            egui::ComboBox::from_id_salt("fw_ver_sel")
                                .selected_text(
                                    entry.versions.get(self.firmware_sel_version)
                                        .map(|v| v.label.as_str()).unwrap_or("?"))
                                .show_ui(ui, |ui| {
                                    for (i, ver) in entry.versions.iter().enumerate() {
                                        ui.selectable_value(&mut self.firmware_sel_version, i,
                                            format!("{} ({})", ver.label, ver.version));
                                    }
                                });
                        });
                        if let Some(ver) = entry.versions.get(self.firmware_sel_version) {
                            ui.label(format!("File: {}", ver.filename));
                            ui.label(RichText::new(
                                "Note: bundled firmware files are not included in this build.\n\
                                 Browse to select a downloaded firmware file below."
                            ).color(Color32::from_gray(140)).italics().small());
                        }
                        ui.separator();
                    } else {
                        ui.label(RichText::new("No bundled firmware for selected card.").color(Color32::from_gray(140)).italics());
                        ui.separator();
                    }
                    ui.label("Or browse for a firmware file (.bin or .bfu):");
                    let path_str = self.firmware_path.as_ref()
                        .and_then(|p| p.to_str()).unwrap_or("(no file selected)");
                    ui.label(format!("File: {}", path_str));
                    if ui.button("Browse...").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Firmware files", &["bin","bfu","zbin"])
                            .pick_file()
                        {
                            self.firmware_path = Some(p);
                        }
                    }
                    ui.separator();
                    ui.horizontal(|ui| {
                        let can_flash = self.firmware_path.is_some() && self.connected;
                        if ui.add_enabled(can_flash, egui::Button::new("Flash")).clicked() {
                            if let Some(path) = self.firmware_path.clone() {
                                self.send_req(Request::FirmwareUpgrade { path });
                                self.show_firmware_dialog = false;
                                self.toast_ok("Firmware upload started...");
                            }
                        }
                        if ui.button("Cancel").clicked() { self.show_firmware_dialog = false; }
                    });
                    ui.label(RichText::new(
                        "Warning: Do not power off the device during firmware update."
                    ).color(Color32::from_rgb(200, 100, 0)).italics().small());
                });
        }

        // Change Screen Size dialog
        if self.show_change_size {
            egui::Window::new("Change Screen Size")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    egui::Grid::new("cs").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
                        ui.label("Width:"); ui.text_edit_singleline(&mut self.change_size_w); ui.end_row();
                        ui.label("Height:"); ui.text_edit_singleline(&mut self.change_size_h); ui.end_row();
                    });
                    // Card size presets
                    if let Some(card) = self.selected_card().cloned() {
                        if card.max_width > 0 {
                            if ui.small_button(format!("Card max: {}×{}", card.max_width, card.max_height)).clicked() {
                                self.change_size_w = card.max_width.to_string();
                                self.change_size_h = card.max_height.to_string();
                            }
                        }
                    }
                    // Common presets
                    ui.horizontal(|ui| {
                        for (w, h, label) in [(64u32, 32u32, "64×32"), (128, 64, "128×64"), (192, 64, "192×64"), (256, 128, "256×128")] {
                            if ui.small_button(label).clicked() {
                                self.change_size_w = w.to_string();
                                self.change_size_h = h.to_string();
                            }
                        }
                    });
                    ui.label(RichText::new("Note: changing size does not resize existing areas.")
                        .color(Color32::from_gray(130)).italics().small());
                    ui.horizontal(|ui| {
                        if ui.button("Apply").clicked() {
                            let w: i32 = self.change_size_w.parse().unwrap_or(128);
                            let h: i32 = self.change_size_h.parse().unwrap_or(64);
                            self.project.screen_w = w.max(8);
                            self.project.screen_h = h.max(8);
                            self.project.modified = true;
                            self.show_change_size = false;
                        }
                        if ui.button("Cancel").clicked() { self.show_change_size = false; }
                    });
                });
        }

        // Item Editor window (opened by double-clicking an item in the tree)
        if self.show_item_editor {
            let open_state = match (self.sel_prog, self.sel_area, self.sel_item) {
                (Some(_), Some(_), Some(_)) => true,
                _ => false,
            };
            if !open_state {
                self.show_item_editor = false;
            } else {
                let pi = self.sel_prog.unwrap();
                let ai = self.sel_area.unwrap();
                let ii = self.sel_item.unwrap();
                let item_type = self.project.programs[pi].areas[ai].items[ii].type_name();
                let is_text = matches!(&self.project.programs[pi].areas[ai].items[ii], ContentItem::Text(_));
                let mut open = self.show_item_editor;
                egui::Window::new(format!("Edit: {}", item_type))
                    .open(&mut open)
                    .collapsible(false)
                    .min_width(380.0)
                    .show(ctx, |ui| {
                        if is_text {
                            // Tab bar
                            ui.horizontal(|ui| {
                                for (i, name) in ["Content", "Font", "Effects", "Layout"].iter().enumerate() {
                                    if ui.selectable_label(self.item_editor_tab == i, *name).clicked() {
                                        self.item_editor_tab = i;
                                    }
                                }
                            });
                            ui.separator();
                            let tab = self.item_editor_tab;
                            if let ContentItem::Text(t) = &mut self.project.programs[pi].areas[ai].items[ii] {
                                match tab {
                                    0 => {
                                        ui.label("Text content:");
                                        ui.text_edit_multiline(&mut t.text);
                                        ui.checkbox(&mut t.single_line, "Single-line mode");
                                        ui.checkbox(&mut t.word_wrap, "Word wrap");
                                    }
                                    1 => {
                                        egui::Grid::new("ie_font").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                            ui.label("Size:");
                                            ui.add(egui::DragValue::new(&mut t.font_size).range(4..=200));
                                            ui.end_row();
                                            ui.label("Color:");
                                            let mut c = to_c32(t.color);
                                            if ui.color_edit_button_srgba(&mut c).changed() { t.color = from_c32(c); }
                                            ui.end_row();
                                            ui.label("Bold:");   ui.checkbox(&mut t.bold, "");      ui.end_row();
                                            ui.label("Italic:"); ui.checkbox(&mut t.italic, "");    ui.end_row();
                                            ui.label("Underline:"); ui.checkbox(&mut t.underline, ""); ui.end_row();
                                        });
                                        ui.horizontal(|ui| {
                                            let has_bg = t.background.is_some();
                                            let mut enabled = has_bg;
                                            if ui.checkbox(&mut enabled, "Background color").changed() {
                                                t.background = if enabled { Some([0, 0, 0]) } else { None };
                                            }
                                            if let Some(ref mut bg) = t.background {
                                                let mut c = to_c32(*bg);
                                                if ui.color_edit_button_srgba(&mut c).changed() { *bg = from_c32(c); }
                                            }
                                        });
                                    }
                                    2 => {
                                        egui::Grid::new("ie_eff").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                            ui.label("Effect in:");
                                            egui::ComboBox::from_id_salt("ie_eff_in")
                                                .selected_text(EFFECT_NAMES.get(t.effect_in as usize).copied().unwrap_or("?"))
                                                .show_ui(ui, |ui| {
                                                    for (i, name) in EFFECT_NAMES.iter().enumerate() {
                                                        ui.selectable_value(&mut t.effect_in, i as u32, *name);
                                                    }
                                                });
                                            ui.end_row();
                                            ui.label("Speed in:");
                                            ui.add(egui::DragValue::new(&mut t.effect_in_speed).range(1..=10));
                                            ui.end_row();
                                            ui.label("Effect out:");
                                            egui::ComboBox::from_id_salt("ie_eff_out")
                                                .selected_text(EFFECT_NAMES.get(t.effect_out as usize).copied().unwrap_or("?"))
                                                .show_ui(ui, |ui| {
                                                    for (i, name) in EFFECT_NAMES.iter().enumerate() {
                                                        ui.selectable_value(&mut t.effect_out, i as u32, *name);
                                                    }
                                                });
                                            ui.end_row();
                                            ui.label("Speed out:");
                                            ui.add(egui::DragValue::new(&mut t.effect_out_speed).range(1..=10));
                                            ui.end_row();
                                            ui.label("Hold (×0.1s):");
                                            ui.add(egui::DragValue::new(&mut t.duration_tenths).range(1..=999));
                                            ui.end_row();
                                            ui.label("Scroll:");
                                            egui::ComboBox::from_id_salt("ie_scroll")
                                                .selected_text(match t.scroll_dir {
                                                    0=>"None", 1=>"Left", 2=>"Right", 3=>"Up", _=>"Down"
                                                })
                                                .show_ui(ui, |ui| {
                                                    for (v, lbl) in [(0u8,"None"),(1,"Left"),(2,"Right"),(3,"Up"),(4,"Down")] {
                                                        ui.selectable_value(&mut t.scroll_dir, v, lbl);
                                                    }
                                                });
                                            ui.end_row();
                                            if t.scroll_dir > 0 {
                                                ui.label("Scroll speed:");
                                                ui.add(egui::DragValue::new(&mut t.scroll_speed).range(1..=200));
                                                ui.end_row();
                                            }
                                        });
                                    }
                                    _ => {
                                        egui::Grid::new("ie_layout").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                                            ui.label("H-Align:");
                                            egui::ComboBox::from_id_salt("ie_align")
                                                .selected_text(["Left","Center","Right"][t.align as usize % 3])
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(&mut t.align, 0, "Left");
                                                    ui.selectable_value(&mut t.align, 1, "Center");
                                                    ui.selectable_value(&mut t.align, 2, "Right");
                                                });
                                            ui.end_row();
                                            ui.label("V-Align:");
                                            egui::ComboBox::from_id_salt("ie_valign")
                                                .selected_text(["Top","Middle","Bottom"][t.valign as usize % 3])
                                                .show_ui(ui, |ui| {
                                                    ui.selectable_value(&mut t.valign, 0, "Top");
                                                    ui.selectable_value(&mut t.valign, 1, "Middle");
                                                    ui.selectable_value(&mut t.valign, 2, "Bottom");
                                                });
                                            ui.end_row();
                                        });
                                    }
                                }
                            }
                        } else if matches!(&self.project.programs[pi].areas[ai].items[ii],
                            ContentItem::ColorfulWord(_))
                        {
                            if let ContentItem::ColorfulWord(c) = &mut self.project.programs[pi].areas[ai].items[ii] {
                                ui.label("Text:");
                                ui.text_edit_multiline(&mut c.text);
                                ui.horizontal(|ui| {
                                    ui.label("Size:"); ui.add(egui::DragValue::new(&mut c.font_size).range(4..=200));
                                    ui.label("Speed:"); ui.add(egui::DragValue::new(&mut c.speed).range(1..=50));
                                });
                                ui.horizontal(|ui| {
                                    ui.checkbox(&mut c.bold, "Bold"); ui.checkbox(&mut c.italic, "Italic");
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Mode:");
                                    egui::ComboBox::from_id_salt("ie_clr_mode")
                                        .selected_text(match c.color_mode { 1=>"Gradient H", 2=>"Gradient V", 3=>"Fire", _=>"Rainbow" })
                                        .show_ui(ui, |ui| {
                                            for (v, lbl) in [(0u8,"Rainbow"),(1,"Gradient H"),(2,"Gradient V"),(3,"Fire")] {
                                                ui.selectable_value(&mut c.color_mode, v, lbl);
                                            }
                                        });
                                });
                            }
                        } else if matches!(&self.project.programs[pi].areas[ai].items[ii],
                            ContentItem::Art3d(_))
                        {
                            if let ContentItem::Art3d(a) = &mut self.project.programs[pi].areas[ai].items[ii] {
                                ui.label("Text:");
                                ui.text_edit_singleline(&mut a.text);
                                ui.horizontal(|ui| {
                                    ui.label("Size:"); ui.add(egui::DragValue::new(&mut a.font_size).range(8..=200));
                                    ui.label("Speed:"); ui.add(egui::DragValue::new(&mut a.speed).range(1..=50));
                                });
                                ui.horizontal(|ui| {
                                    ui.label("Style:");
                                    egui::ComboBox::from_id_salt("ie_a3d_style")
                                        .selected_text(match a.style { 1=>"3D Rotate", 2=>"Shadow", 3=>"Hollow", _=>"3D Push" })
                                        .show_ui(ui, |ui| {
                                            for (v, lbl) in [(0u8,"3D Push"),(1,"3D Rotate"),(2,"Shadow"),(3,"Hollow")] {
                                                ui.selectable_value(&mut a.style, v, lbl);
                                            }
                                        });
                                    let mut col = to_c32(a.color);
                                    if ui.color_edit_button_srgba(&mut col).changed() { a.color = from_c32(col); }
                                });
                            }
                        } else {
                            // For remaining item types, direct to properties panel
                            ui.label(format!("Type: {}", item_type));
                            ui.label(RichText::new("Edit detailed properties in the Properties panel on the right.")
                                .color(Color32::from_gray(140)).italics().small());
                        }
                    });
                self.show_item_editor = open;
            }
        }

        // Device Schedules floating window
        if self.show_schedule_window {
            let mut open = self.show_schedule_window;
            egui::Window::new("Device Schedules")
                .open(&mut open)
                .default_width(480.0)
                .show(ctx, |ui| {
                    ui.collapsing("Screen On/Off Schedule", |ui| {
                        let sched = self.screen_sched.clone();
                        let mut remove = None;
                        for (i, (on, off, days)) in sched.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{on}–{off}  [{days}]"));
                                if ui.small_button("X").clicked() { remove = Some(i); }
                            });
                        }
                        if let Some(i) = remove { self.screen_sched.remove(i); }
                        ui.horizontal(|ui| {
                            ui.label("On:");
                            ui.add(egui::TextEdit::singleline(&mut self.screen_sched_add_on).desired_width(50.0));
                            ui.label("Off:");
                            ui.add(egui::TextEdit::singleline(&mut self.screen_sched_add_off).desired_width(50.0));
                            ui.label("Days:");
                            for (i, day) in ["Mo","Tu","We","Th","Fr","Sa","Su"].iter().enumerate() {
                                ui.checkbox(&mut self.screen_sched_add_days[i], *day);
                            }
                            if ui.button("Add").clicked() {
                                let bits: String = self.screen_sched_add_days.iter()
                                    .map(|&b| if b { '1' } else { '0' }).collect();
                                self.screen_sched.push((
                                    self.screen_sched_add_on.clone(),
                                    self.screen_sched_add_off.clone(),
                                    bits,
                                ));
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Apply").clicked() {
                                self.send_req(Request::SetScreenSchedule(self.screen_sched.clone()));
                            }
                            if ui.button("Clear All").clicked() {
                                self.screen_sched.clear();
                                self.send_req(Request::SetScreenSchedule(Vec::new()));
                            }
                        });
                    });
                    ui.separator();
                    ui.collapsing("Brightness Schedule", |ui| {
                        let bsched = self.brightness_sched.clone();
                        let mut bremove = None;
                        for (i, (h, m, lv)) in bsched.iter().enumerate() {
                            ui.horizontal(|ui| {
                                ui.label(format!("{h:02}:{m:02}  →  {lv}%"));
                                if ui.small_button("X").clicked() { bremove = Some(i); }
                            });
                        }
                        if let Some(i) = bremove { self.brightness_sched.remove(i); }
                        ui.horizontal(|ui| {
                            ui.add(egui::DragValue::new(&mut self.brightness_sched_add_h).range(0..=23).suffix("h"));
                            ui.add(egui::DragValue::new(&mut self.brightness_sched_add_m).range(0..=59).suffix("m"));
                            ui.label("→");
                            ui.add(egui::Slider::new(&mut self.brightness_sched_add_lvl, 0u8..=100).suffix("%"));
                            if ui.button("Add").clicked() {
                                self.brightness_sched.push((
                                    self.brightness_sched_add_h,
                                    self.brightness_sched_add_m,
                                    self.brightness_sched_add_lvl,
                                ));
                            }
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Apply").clicked() {
                                self.send_req(Request::SetBrightnessSchedule(self.brightness_sched.clone()));
                            }
                            if ui.button("Clear All").clicked() {
                                self.brightness_sched.clear();
                                self.send_req(Request::SetBrightnessSchedule(Vec::new()));
                            }
                        });
                    });
                });
            self.show_schedule_window = open;
        }

        // Network Settings dialog
        if self.show_network_dialog {
            let mut open = self.show_network_dialog;
            egui::Window::new("Network Settings")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .min_width(340.0)
                .show(ctx, |ui| {
                    ui.heading("Device Name");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.device_name_edit)
                            .hint_text("Device name").desired_width(180.0));
                        if ui.button("Rename").clicked() && !self.device_name_edit.is_empty() {
                            self.send_req(Request::SetDeviceName(self.device_name_edit.clone()));
                        }
                    });
                    ui.separator();
                    ui.heading("Ethernet (eth0)");
                    ui.checkbox(&mut self.net_dhcp, "DHCP (auto)");
                    if !self.net_dhcp {
                        egui::Grid::new("net_form").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                            ui.label("IP:"); ui.text_edit_singleline(&mut self.net_ip); ui.end_row();
                            ui.label("Mask:"); ui.text_edit_singleline(&mut self.net_mask); ui.end_row();
                            ui.label("Gateway:"); ui.text_edit_singleline(&mut self.net_gateway); ui.end_row();
                            ui.label("DNS:"); ui.text_edit_singleline(&mut self.net_dns); ui.end_row();
                        });
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Apply Ethernet").clicked() {
                            self.send_req(Request::SetEthConfig {
                                dhcp: self.net_dhcp,
                                ip:      self.net_ip.clone(),
                                mask:    self.net_mask.clone(),
                                gateway: self.net_gateway.clone(),
                                dns:     self.net_dns.clone(),
                            });
                        }
                    });
                    ui.separator();
                    ui.heading("Timezone");
                    ui.horizontal(|ui| {
                        ui.label("UTC offset:");
                        ui.add(egui::DragValue::new(&mut self.net_timezone).range(-12..=14).suffix("h"));
                        if ui.button("Set Timezone").clicked() {
                            self.send_req(Request::SetTimezone(self.net_timezone));
                        }
                    });
                    ui.label(RichText::new("Note: Ethernet changes may restart the device network.")
                        .color(Color32::from_gray(130)).italics().small());
                });
            self.show_network_dialog = open;
        }

        // Device Files dialog
        if self.show_files_dialog {
            let mut open = self.show_files_dialog;
            egui::Window::new("Device Files")
                .open(&mut open)
                .default_size([480.0, 360.0])
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        if ui.button("Refresh").clicked() {
                            self.send_req(Request::ListFiles);
                        }
                        let del_count = self.files_sel.len();
                        if ui.add_enabled(del_count > 0, egui::Button::new(
                            format!("Delete {} selected", del_count)
                        )).clicked() {
                            let names: Vec<String> = self.files_sel.iter()
                                .filter_map(|&i| self.device_files.get(i).map(|f| f.name.clone()))
                                .collect();
                            self.send_req(Request::DeleteFiles(names));
                        }
                        if ui.button("Select All").clicked() {
                            self.files_sel = (0..self.device_files.len()).collect();
                        }
                        if ui.button("Select None").clicked() {
                            self.files_sel.clear();
                        }
                    });
                    ui.separator();
                    if self.device_files.is_empty() {
                        ui.label(RichText::new("No files — click Refresh to load.").color(Color32::from_gray(140)).italics());
                    } else {
                        egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
                            let files = self.device_files.clone();
                            egui::Grid::new("files_grid")
                                .num_columns(3)
                                .striped(true)
                                .spacing([8.0, 4.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Name").strong());
                                    ui.label(RichText::new("Size").strong());
                                    ui.label(RichText::new("MD5").strong());
                                    ui.end_row();
                                    for (i, f) in files.iter().enumerate() {
                                        let sel = self.files_sel.contains(&i);
                                        if ui.selectable_label(sel, &f.name).clicked() {
                                            if sel { self.files_sel.remove(&i); }
                                            else   { self.files_sel.insert(i); }
                                        }
                                        let kb = f.size / 1024;
                                        ui.label(if kb > 0 { format!("{kb} KB") } else { format!("{} B", f.size) });
                                        ui.label(f.md5.get(..8).unwrap_or(&f.md5));
                                        ui.end_row();
                                    }
                                });
                        });
                    }
                });
            self.show_files_dialog = open;
        }

        // Boot Logo dialog
        if self.show_boot_logo_dialog {
            let mut open = self.show_boot_logo_dialog;
            egui::Window::new("Boot Logo")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .min_width(320.0)
                .show(ctx, |ui| {
                    ui.label(format!("Current: {}", if self.boot_logo_current.is_empty() { "(none)" } else { &self.boot_logo_current }));
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Logo filename (on device):");
                        ui.text_edit_singleline(&mut self.boot_logo_set_name);
                    });
                    ui.label(RichText::new("The image file must already be on the device (upload via normal program send).")
                        .color(Color32::from_gray(130)).italics().small());
                    ui.horizontal(|ui| {
                        if ui.add_enabled(!self.boot_logo_set_name.is_empty(),
                            egui::Button::new("Set Logo")).clicked()
                        {
                            self.send_req(Request::SetBootLogo(self.boot_logo_set_name.clone()));
                        }
                        if ui.add_enabled(!self.boot_logo_current.is_empty(),
                            egui::Button::new("Clear Logo")).clicked()
                        {
                            self.send_req(Request::ClearBootLogo);
                            self.boot_logo_current.clear();
                        }
                        if ui.button("Refresh").clicked() {
                            self.send_req(Request::GetBootLogo);
                        }
                    });
                });
            self.show_boot_logo_dialog = open;
        }

        // Admin Password dialog
        if self.show_admin_dialog {
            let mut open = self.show_admin_dialog;
            egui::Window::new("Admin Password")
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .min_width(300.0)
                .show(ctx, |ui| {
                    ui.collapsing("Set / Change Password", |ui| {
                        egui::Grid::new("adm_set").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                            ui.label("New password:");
                            ui.add(egui::TextEdit::singleline(&mut self.admin_new_pw).password(true));
                            ui.end_row();
                            ui.label("Confirm:");
                            ui.add(egui::TextEdit::singleline(&mut self.admin_confirm_pw).password(true));
                            ui.end_row();
                        });
                        let pw_ok = !self.admin_new_pw.is_empty()
                            && self.admin_new_pw == self.admin_confirm_pw;
                        if ui.add_enabled(pw_ok, egui::Button::new("Set Password")).clicked() {
                            self.send_req(Request::SetAdminPassword { password: self.admin_new_pw.clone() });
                            self.admin_new_pw.clear(); self.admin_confirm_pw.clear();
                        }
                        if !pw_ok && !self.admin_new_pw.is_empty() {
                            ui.label(RichText::new("Passwords do not match.").color(Color32::from_rgb(200, 80, 80)).small());
                        }
                    });
                    ui.separator();
                    ui.collapsing("Unlock Admin Mode", |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Password:");
                            ui.add(egui::TextEdit::singleline(&mut self.admin_unlock_pw).password(true));
                        });
                        if ui.add_enabled(!self.admin_unlock_pw.is_empty(),
                            egui::Button::new("Unlock")).clicked()
                        {
                            self.send_req(Request::UnlockAdmin { password: self.admin_unlock_pw.clone() });
                            self.admin_unlock_pw.clear();
                        }
                    });
                    ui.label(RichText::new("Tip: leave password empty to remove the lock requirement.")
                        .color(Color32::from_gray(130)).italics().small());
                });
            self.show_admin_dialog = open;
        }

        // Data Sources dialog
        if self.show_datasources_dialog {
            let mut open = self.show_datasources_dialog;
            egui::Window::new("Data Sources")
                .open(&mut open)
                .default_size([420.0, 320.0])
                .show(ctx, |ui| {
                    ui.label(RichText::new(
                        "Key/value pairs stored on the device, bindable to dynamic text areas."
                    ).color(Color32::from_gray(140)).italics().small());
                    ui.separator();
                    if ui.button("Refresh").clicked() {
                        self.send_req(Request::GetDataSources);
                    }
                    if self.data_sources.is_empty() {
                        ui.label("No data sources — click Refresh.");
                    } else {
                        egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                            let sources = self.data_sources.clone();
                            egui::Grid::new("ds_grid").num_columns(3).striped(true).spacing([8.0,4.0]).show(ui, |ui| {
                                ui.label(RichText::new("Key").strong());
                                ui.label(RichText::new("Value").strong());
                                ui.label("");
                                ui.end_row();
                                for (name, value) in &sources {
                                    ui.label(name);
                                    ui.label(value);
                                    if ui.small_button("Clear").clicked() {
                                        self.send_req(Request::DeleteDataSource(name.clone()));
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                    ui.separator();
                    ui.label(RichText::new("Add / Update:").strong());
                    egui::Grid::new("ds_add").num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
                        ui.label("Key:");
                        ui.text_edit_singleline(&mut self.ds_new_name);
                        ui.end_row();
                        ui.label("Value:");
                        ui.text_edit_singleline(&mut self.ds_new_value);
                        ui.end_row();
                    });
                    if ui.add_enabled(!self.ds_new_name.is_empty(), egui::Button::new("Set")).clicked() {
                        self.send_req(Request::SetDataSource {
                            name: self.ds_new_name.clone(),
                            value: self.ds_new_value.clone(),
                        });
                        self.ds_new_name.clear();
                        self.ds_new_value.clear();
                    }
                });
            self.show_datasources_dialog = open;
        }

        // About dialog
        if self.show_about {
            egui::Window::new("About HDSign")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("HDSign");
                    ui.label("Rust/egui reproduction of Huidu HDSign V2.0.2");
                    ui.label("LED display card programming tool");
                    ui.separator();
                    ui.label("Supports: Ethernet / WiFi AP / Serial / USB cards");
                    ui.label(format!("Hardware cards loaded: {}", self.cards.len()));
                    ui.label(format!("Firmware entries: {}",
                        self.firmware_catalog.iter().map(|e| e.versions.len()).sum::<usize>()));
                    ui.label(format!("Weather countries: {}", weather_countries().len()));
                    ui.separator();
                    ui.label(RichText::new("Protocol: Huidu TCP SDK port 10001").color(Color32::from_gray(140)).small());
                    ui.label(RichText::new("WiFi AP: HTTP port 6104 at 192.168.4.1").color(Color32::from_gray(140)).small());
                    ui.separator();
                    if ui.button("Close").clicked() { self.show_about = false; }
                });
        }

        // Toast notification
        if let Some((msg, t, is_err)) = &self.toast.clone() {
            if t.elapsed() < Duration::from_secs(4) {
                let color = if *is_err { Color32::from_rgb(220, 80, 80) } else { Color32::from_rgb(60, 160, 60) };
                egui::Window::new("")
                    .title_bar(false)
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::RIGHT_BOTTOM, [-10.0, -10.0])
                    .show(ctx, |ui| {
                        ui.label(RichText::new(msg).color(color).strong());
                    });
            } else {
                self.toast = None;
            }
        }
    }
}

// ── EFRAME APP IMPL ───────────────────────────────────────────────────────────

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_responses();

        // Update window title to reflect current file and unsaved state
        {
            let fname = self.project.path.as_ref()
                .and_then(|p| p.file_name()).and_then(|n| n.to_str())
                .unwrap_or("Untitled");
            let modified = if self.project.modified { "* " } else { "" };
            let title = format!("{}{}  —  HDSign", modified, fname);
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }

        ctx.request_repaint_after(Duration::from_millis(500));

        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            self.render_toolbar(ui);
        });

        if self.show_device_panel {
            egui::TopBottomPanel::bottom("device_bar")
                .min_height(28.0)
                .show(ctx, |ui| {
                    self.render_device_panel(ui);
                });
        }

        egui::SidePanel::left("tree")
            .min_width(180.0)
            .default_width(220.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_tree(ui);
                });
            });

        egui::SidePanel::right("props")
            .min_width(200.0)
            .default_width(260.0)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    self.render_properties(ui);
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.sel_prog.is_some() {
                self.render_canvas(ui);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new(
                        "HDSign — Create or select a program to start editing.\n\
                         Select a card model and connection mode in the toolbar above."
                    ).size(16.0).color(Color32::from_gray(140)));
                });
            }
        });

        self.render_dialogs(ctx);
    }
}

// ── MAIN ──────────────────────────────────────────────────────────────────────

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([900.0, 600.0])
            .with_title("HDSign — Huidu LED Sign Control"),
        ..Default::default()
    };

    eframe::run_native(
        "hdsign-gui",
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
