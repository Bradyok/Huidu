//! HDPlayer GUI — Rust/egui clone of the Huidu HDPlayer desktop application.
//!
//! Build:  cargo build --features gui --bin hdplayer-gui
//! Run:    ./target/debug/hdplayer-gui

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use hdplayer_client::{Client, DeviceDetails, DeviceInfo, Discovery, ProgramInfo};

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
}

impl ContentItem {
    pub fn guid(&self) -> &str {
        match self {
            Self::Text(t) => &t.guid, Self::Image(i) => &i.guid,
            Self::Video(v) => &v.guid, Self::Clock(c) => &c.guid,
            Self::Neon(n) => &n.guid, Self::QrCode(q) => &q.guid,
            Self::Calendar(c) => &c.guid, Self::Countdown(c) => &c.guid,
            Self::Table(t) => &t.guid,
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
}

impl Program {
    pub fn new(guid: String, name: String, screen_w: i32, screen_h: i32) -> Self {
        let area_guid = new_guid();
        let mut p = Self {
            guid, name, program_type: "normal".into(),
            play_duration_secs: 15, play_count: 0,
            border_index: 0, border_speed: 5, areas: Vec::new(),
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
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().subsec_nanos() as u64
        ^ SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs().wrapping_mul(6364136223846793005);
    format!("{:08X}-{:04X}-4{:03X}-{:04X}-{:012X}",
        t as u32, (t >> 32) as u16 & 0xFFFF,
        (t >> 48) as u16 & 0xFFF,
        0x8000u16 | ((t >> 60) as u16 & 0x3FFF),
        t.wrapping_mul(2862933555777941757).wrapping_add(3037000499) & 0xFFFFFFFFFFFF)
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
    for prefix in &[format!(" {}=\"", attr), format!("\t{}=\"", attr), format!("\n{}=\"", attr)] {
        if let Some(s) = xml.find(prefix.as_str()) {
            let vs = s + prefix.len();
            if let Some(e) = xml[vs..].find('"') { return Some(&xml[vs..vs+e]); }
        }
    }
    None
}
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

// ── XML GENERATION ────────────────────────────────────────────────────────────

pub fn generate_boo(project: &Project) -> String {
    let mut out = String::from("<?xml version='1.0' encoding='utf-8'?>\n");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    out.push_str(&format!("<screen timeStamps=\"{}\">\n\n", ts));

    for prog in &project.programs {
        out.push_str(&format!("  <program guid=\"{}\" name=\"{}\" type=\"{}\">\n",
            prog.guid, xml_escape(&prog.name), prog.program_type));

        if prog.play_duration_secs > 0 {
            let h = prog.play_duration_secs / 3600;
            let m = (prog.play_duration_secs % 3600) / 60;
            let s = prog.play_duration_secs % 60;
            out.push_str(&format!("    <playControl duration=\"{:02}:{:02}:{:02}\" count=\"0\"/>\n", h, m, s));
        } else {
            out.push_str(&format!("    <playControl count=\"{}\" disabled=\"false\"/>\n", prog.play_count.max(1)));
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
                        out.push_str(&format!("        <image guid=\"{}\" fit=\"{}\"/>\n", im.guid, fit));
                        if !fname.is_empty() { out.push_str(&format!("        <file name=\"{}\"/>\n", fname)); }
                    }
                    ContentItem::Video(v) => {
                        let fname = v.path.as_ref().and_then(|p| p.file_name())
                            .and_then(|n| n.to_str()).unwrap_or("");
                        out.push_str(&format!("        <video guid=\"{}\" aspectRatio=\"{}\"/>\n",
                            v.guid, if v.keep_aspect { 1 } else { 0 }));
                        if !fname.is_empty() { out.push_str(&format!("        <file name=\"{}\"/>\n", fname)); }
                    }
                    ContentItem::Clock(c) => {
                        out.push_str(&format!("        <clock guid=\"{}\" type=\"{}\" timezone=\"{}\">\n",
                            c.guid, if c.is_analog { 1 } else { 0 }, xml_escape(&c.timezone)));
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
    // Clock
    let mut s = xml;
    while let Some(ps) = s.find("<clock ") {
        let pe = s[ps..].find("</clock>").map(|e| ps+e+8)
            .or_else(|| s[ps..].find("/>").map(|e| ps+e+2)).unwrap_or(s.len());
        items.push(parse_clock(&s[ps..pe]));
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
        scroll_dir: 0, scroll_speed: 40, word_wrap: false, background,
    })
}

fn parse_clock(xml: &str) -> ContentItem {
    let guid = get_attr(xml, "guid").unwrap_or_default().to_string();
    let is_analog = get_attr(xml, "type").map(|v| v=="1"||v=="analog").unwrap_or(false);
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
        hand_color: [0,255,136], second_color: [255,68,0], dial_color: [13,26,13],
    })
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
    UploadScreenXml(String),
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
}

async fn worker_loop(
    mut req_rx: tokio::sync::mpsc::Receiver<Request>,
    resp_tx: std::sync::mpsc::Sender<Response>,
) {
    let mut client: Option<Client> = None;
    loop {
        let req = match req_rx.recv().await {
            Some(r) => r,
            None => break,
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
            Request::UploadScreenXml(xml) => {
                if let Some(c) = &mut client {
                    match c.add_program(&xml).await {
                        Ok(_) => {
                            let _ = resp_tx.send(Response::Ok("Upload complete".into()));
                            if let Ok(p) = c.get_all_programs().await { let _ = resp_tx.send(Response::Programs(p)); }
                        }
                        Err(e) => { let _ = resp_tx.send(Response::Error(format!("{e}"))); }
                    }
                } else {
                    let _ = resp_tx.send(Response::Error("Not connected".into()));
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

    // Device comms
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

    show_connect_dialog: bool,
    show_device_panel: bool,
    show_preview_window: bool,

    // Toast
    toast: Option<(String, Instant, bool)>,
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
            show_connect_dialog: false,
            show_device_panel: true,
            show_preview_window: false,
            toast: None,
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
                    self.toast_err(msg);
                }
                Response::Disconnected => {
                    self.connected = false;
                    self.device_info = None;
                    self.dev_programs.clear();
                    self.preview_texture = None;
                    self.toast_ok("Disconnected");
                }
            }
        }
    }

    // ── TOOLBAR ───────────────────────────────────────────────────────────────

    fn render_toolbar(&mut self, ui: &mut egui::Ui) {
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
            // Upload current program to device
            if ui.add_enabled(self.connected, egui::Button::new("Send to Device")).clicked() {
                let xml = generate_boo(&self.project);
                // strip <screen>...</screen> block for AddProgram
                self.send_req(Request::UploadScreenXml(xml));
                self.toast_ok("Uploading…");
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
                self.sel_area = None;
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
                                if self.sel_area == Some(ai) {
                                    self.sel_area = None;
                                    self.sel_item = None;
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
                                    if resp.clicked() { self.sel_item = Some(ii); }
                                    resp.context_menu(|ui| {
                                        if ui.button("Delete").clicked() {
                                            self.project.programs[pi].areas[ai].items.remove(ii);
                                            if self.sel_item == Some(ii) { self.sel_item = None; }
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

            // Click to select area
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

        ctx.request_repaint_after(Duration::from_millis(500));

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
