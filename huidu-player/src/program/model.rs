/// Data model for Huidu program format.
/// Based on reverse-engineering of BoxPlayer binaries and firmware XML analysis.
use serde::{Deserialize, Serialize};

/// Root element — a screen contains one or more programs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Screen {
    #[serde(rename = "@timeStamps", default)]
    pub timestamps: String,
    #[serde(rename = "program", default)]
    pub programs: Vec<Program>,
}

/// A program is one complete display composition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Program {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@type", default = "default_program_type")]
    pub program_type: String,
    #[serde(rename = "@flag", default)]
    pub flag: String,
    #[serde(rename = "border")]
    pub border: Option<Border>,
    #[serde(rename = "backgroundMusic")]
    pub background_music: Option<BackgroundMusic>,
    #[serde(rename = "playControl")]
    pub play_control: Option<PlayControl>,
    #[serde(rename = "area", default)]
    pub areas: Vec<Area>,
}

fn default_program_type() -> String {
    "normal".to_string()
}

/// Border/neon effect around the display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Border {
    #[serde(rename = "@index", default)]
    pub index: u32,
    #[serde(rename = "@effect", default)]
    pub effect: String,
    #[serde(rename = "@speed", default)]
    pub speed: String,
}

/// Background music track list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundMusic {
    #[serde(rename = "file", default)]
    pub files: Vec<FileRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileRef {
    #[serde(rename = "@name")]
    pub name: String,
}

/// Playback scheduling control
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayControl {
    #[serde(rename = "@duration", default)]
    pub duration: String,
    #[serde(rename = "@count", default)]
    pub count: u32,
    #[serde(rename = "@disabled", default)]
    pub disabled: bool,
    #[serde(rename = "date")]
    pub date: Option<DateRange>,
    #[serde(rename = "time")]
    pub time: Option<TimeRange>,
    #[serde(rename = "week")]
    pub week: Option<WeekFilter>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DateRange {
    #[serde(rename = "@start")]
    pub start: String,
    #[serde(rename = "@end")]
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeRange {
    #[serde(rename = "@start")]
    pub start: String,
    #[serde(rename = "@end")]
    pub end: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeekFilter {
    #[serde(rename = "@enable")]
    pub enable: String,
}

/// An area is a rectangular zone on the display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Area {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@alpha", default = "default_alpha")]
    pub alpha: u8,
    pub rectangle: Rectangle,
    #[serde(rename = "border")]
    pub border: Option<Border>,
    pub resources: Resources,
}

fn default_alpha() -> u8 {
    255
}

/// Position and size of an area
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rectangle {
    #[serde(rename = "@x", default)]
    pub x: i32,
    #[serde(rename = "@y", default)]
    pub y: i32,
    #[serde(rename = "@width")]
    pub width: u32,
    #[serde(rename = "@height")]
    pub height: u32,
}

/// Container for content items within an area
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resources {
    #[serde(rename = "$value", default)]
    pub items: Vec<ContentItem>,
}

/// A content item — the actual thing displayed in an area
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ContentItem {
    #[serde(rename = "image")]
    Image(ImageContent),
    #[serde(rename = "video")]
    Video(VideoContent),
    #[serde(rename = "text")]
    Text(TextContent),
    #[serde(rename = "clock")]
    Clock(ClockContent),
    #[serde(rename = "gif")]
    Gif(GifContent),
}

/// Transition/animation effect
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Effect {
    /// Effect type for entrance (0-29)
    #[serde(rename = "@in", default)]
    pub effect_in: u8,
    /// Effect type for exit (0-29)
    #[serde(rename = "@out", default)]
    pub effect_out: u8,
    /// Entrance speed (0=fastest, 8=slowest)
    #[serde(rename = "@inSpeed", default)]
    pub in_speed: u8,
    /// Exit speed (0=fastest, 8=slowest)
    #[serde(rename = "@outSpeed", default)]
    pub out_speed: u8,
    /// Display duration in tenths of seconds
    #[serde(rename = "@duration", default = "default_duration")]
    pub duration: u32,
}

fn default_duration() -> u32 {
    50 // 5 seconds
}

/// Effect type constants
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EffectType {
    ImmediateShow = 0,
    LeftParallelMove = 1,
    RightParallelMove = 2,
    UpParallelMove = 3,
    DownParallelMove = 4,
    LeftCover = 5,
    RightCover = 6,
    UpCover = 7,
    DownCover = 8,
    LeftUpCover = 9,
    RightUpCover = 10,
    LeftDownCover = 11,
    RightDownCover = 12,
    HorizontalDivide = 13,
    VerticalDivide = 14,
    HorizontalClose = 15,
    VerticalClose = 16,
    Fade = 17,
    HorizontalShutter = 18,
    VerticalShutter = 19,
    NotClearArea = 20,
    LeftSeriesMove = 21,
    RightSeriesMove = 22,
    UpSeriesMove = 23,
    DownSeriesMove = 24,
    Random = 25,
    HtLeftSeriesMove = 26,
    HtRightSeriesMove = 27,
    HtUpSeriesMove = 28,
    HtDownSeriesMove = 29,
}

// -- Content types --

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// fill, center, stretch, tile
    #[serde(rename = "@fit", default = "default_fit")]
    pub fit: String,
    pub effect: Option<Effect>,
    pub file: FileRef,
}

fn default_fit() -> String {
    "stretch".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@aspectRatio", default)]
    pub aspect_ratio: bool,
    pub file: FileRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@singleLine", default)]
    pub single_line: bool,
    #[serde(rename = "@background", default)]
    pub background: String,
    pub effect: Option<Effect>,
    pub style: Option<TextStyle>,
    pub string: Option<String>,
    pub font: Option<FontSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextStyle {
    /// left, center, right
    #[serde(rename = "@align", default = "default_align")]
    pub align: String,
    /// top, middle, bottom
    #[serde(rename = "@valign", default = "default_valign")]
    pub valign: String,
}

fn default_align() -> String {
    "center".to_string()
}
fn default_valign() -> String {
    "middle".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontSpec {
    #[serde(rename = "@name", default = "default_font_name")]
    pub name: String,
    #[serde(rename = "@size", default = "default_font_size")]
    pub size: f32,
    #[serde(rename = "@color", default = "default_color")]
    pub color: String,
    #[serde(rename = "@bold", default)]
    pub bold: bool,
    #[serde(rename = "@italic", default)]
    pub italic: bool,
    #[serde(rename = "@underline", default)]
    pub underline: bool,
}

fn default_font_name() -> String {
    "Arial".to_string()
}
fn default_font_size() -> f32 {
    12.0
}
fn default_color() -> String {
    "#ff0000".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// digital or dial
    #[serde(rename = "@type", default = "default_clock_type")]
    pub clock_type: String,
    #[serde(rename = "@timezone", default)]
    pub timezone: String,
    #[serde(rename = "@adjust", default)]
    pub adjust: String,
    pub title: Option<ClockField>,
    pub date: Option<ClockField>,
    pub week: Option<ClockField>,
    pub time: Option<ClockField>,
    #[serde(rename = "lunarCalendar")]
    pub lunar_calendar: Option<ClockField>,
}

fn default_clock_type() -> String {
    "digital".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClockField {
    #[serde(rename = "@value", default)]
    pub value: String,
    #[serde(rename = "@format", default)]
    pub format: String,
    #[serde(rename = "@color", default = "default_color")]
    pub color: String,
    #[serde(rename = "@display", default = "default_display")]
    pub display: bool,
}

fn default_display() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GifContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    pub effect: Option<Effect>,
    pub file: FileRef,
}

// -- Helpers --

/// Parse a hex color string (#RRGGBB) to (r, g, b)
pub fn parse_color(color: &str) -> (u8, u8, u8) {
    let s = color.trim_start_matches('#');
    if s.len() >= 6 {
        let r = u8::from_str_radix(&s[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&s[2..4], 16).unwrap_or(0);
        let b = u8::from_str_radix(&s[4..6], 16).unwrap_or(0);
        (r, g, b)
    } else {
        (255, 0, 0) // default red
    }
}
