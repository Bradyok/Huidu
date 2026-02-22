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
    #[serde(rename = "table")]
    Table(TableContent),
    #[serde(rename = "weather")]
    Weather(WeatherContent),
    #[serde(rename = "analogClock")]
    AnalogClock(AnalogClockContent),
    #[serde(rename = "neon")]
    Neon(NeonContent),
    #[serde(rename = "qrCode")]
    QrCode(QrCodeContent),
    #[serde(rename = "calendar")]
    Calendar(CalendarContent),
    #[serde(rename = "countdownTimer")]
    Countdown(CountdownContent),
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

// -- Table content --

/// Table/grid content item (libtable_plugin.so)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Number of columns
    #[serde(rename = "@cols", default = "default_table_cols")]
    pub cols: u32,
    /// Number of rows
    #[serde(rename = "@rows", default = "default_table_rows")]
    pub rows: u32,
    pub effect: Option<Effect>,
    pub style: Option<TableStyle>,
    #[serde(rename = "row", default)]
    pub data_rows: Vec<TableRow>,
}

fn default_table_cols() -> u32 { 4 }
fn default_table_rows() -> u32 { 4 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableStyle {
    #[serde(rename = "@textColor", default = "default_color")]
    pub text_color: String,
    #[serde(rename = "@borderColor", default = "default_color")]
    pub border_color: String,
    #[serde(rename = "@headerBgColor", default)]
    pub header_bg_color: String,
    #[serde(rename = "@bgColor", default)]
    pub bg_color: String,
    #[serde(rename = "@fontSize", default = "default_font_size")]
    pub font_size: f32,
    #[serde(rename = "@headerRow", default)]
    pub header_row: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableRow {
    #[serde(rename = "cell", default)]
    pub cells: Vec<TableCell>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableCell {
    #[serde(rename = "$value", default)]
    pub text: String,
    #[serde(rename = "@color", default)]
    pub color: String,
    #[serde(rename = "@bold", default)]
    pub bold: bool,
}

// -- Weather content --

/// Weather widget content item (libweather_plugin.so)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// City name for display
    #[serde(rename = "@city", default)]
    pub city: String,
    /// City/location code (Huidu or OpenWeatherMap city ID)
    #[serde(rename = "@cityCode", default)]
    pub city_code: String,
    /// Language code ("en", "zh", etc.)
    #[serde(rename = "@lang", default = "default_weather_lang")]
    pub lang: String,
    /// Update interval in minutes
    #[serde(rename = "@updateInterval", default = "default_update_interval")]
    pub update_interval: u32,
    pub effect: Option<Effect>,
    pub font: Option<FontSpec>,
}

fn default_weather_lang() -> String { "en".to_string() }
fn default_update_interval() -> u32 { 30 }

// -- Analog clock --

/// Analog clock content item (uses clock face image files)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalogClockContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    #[serde(rename = "@timezone", default)]
    pub timezone: String,
    /// Background image filename
    #[serde(rename = "@bgImage", default)]
    pub bg_image: String,
    pub effect: Option<Effect>,
}

// -- Neon decoration --

/// Neon decoration content item (libneon_plugin.so equivalent).
/// Renders one of 36 animated glowing shapes as an LED display decoration.
/// Shape index reference: see `images/Border/*.png` in the HDPlayer installation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeonContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Shape selector 0-35
    #[serde(rename = "@index", default)]
    pub index: u32,
    /// Color: "#RRGGBB", "rainbow", or "gradient:#C1:#C2"
    #[serde(rename = "@color", default = "default_neon_color")]
    pub color: String,
    /// Animation speed 1-10 (higher = faster pulse/rainbow)
    #[serde(rename = "@speed", default = "default_neon_speed")]
    pub speed: u32,
    /// true = single color with breathing pulse; false = rainbow cycling
    #[serde(rename = "@singleColor", default = "default_true")]
    pub single_color: bool,
    pub effect: Option<Effect>,
}

fn default_neon_color() -> String { "#ff0088".to_string() }
fn default_neon_speed() -> u32 { 5 }
fn default_true() -> bool { true }

// -- QR Code --

/// QR code content item.
/// Renders a live QR code from a text/URL string.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QrCodeContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// The text/URL to encode into the QR code.
    #[serde(rename = "@data", default)]
    pub data: String,
    /// Foreground color (default black).
    #[serde(rename = "@fgColor", default = "default_qr_fg")]
    pub fg_color: String,
    /// Background color (default white).
    #[serde(rename = "@bgColor", default = "default_qr_bg")]
    pub bg_color: String,
    /// Border (quiet zone) in modules (default 1).
    #[serde(rename = "@border", default = "default_qr_border")]
    pub border: u32,
    pub effect: Option<Effect>,
}

fn default_qr_fg() -> String { "#000000".to_string() }
fn default_qr_bg() -> String { "#ffffff".to_string() }
fn default_qr_border() -> u32 { 1 }

// -- Calendar --

/// Monthly calendar grid content item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalendarContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Text color for regular days.
    #[serde(rename = "@color", default = "default_color")]
    pub color: String,
    /// Highlight color for today's date.
    #[serde(rename = "@todayColor", default = "default_today_color")]
    pub today_color: String,
    /// Header (month+year) color.
    #[serde(rename = "@headerColor", default = "default_header_color")]
    pub header_color: String,
    /// Weekday name row color.
    #[serde(rename = "@weekdayColor", default = "default_weekday_color")]
    pub weekday_color: String,
    /// Font size in pixels (default 8).
    #[serde(rename = "@fontSize", default = "default_cal_font_size")]
    pub font_size: f32,
    pub effect: Option<Effect>,
}

fn default_today_color() -> String { "#ffff00".to_string() }
fn default_header_color() -> String { "#00aaff".to_string() }
fn default_weekday_color() -> String { "#aaaaaa".to_string() }
fn default_cal_font_size() -> f32 { 8.0 }

// -- Countdown timer --

/// Countdown timer content item.
/// Counts down to a fixed target datetime and displays the remaining time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CountdownContent {
    #[serde(rename = "@guid")]
    pub guid: String,
    #[serde(rename = "@name", default)]
    pub name: String,
    /// Target datetime: "YYYY-MM-DD HH:MM:SS"
    #[serde(rename = "@target", default)]
    pub target: String,
    /// Display format: "D:H:M:S" | "H:M:S" | "M:S" | "S"
    #[serde(rename = "@format", default = "default_countdown_format")]
    pub format: String,
    /// Optional label shown above the timer (e.g. "Sale ends in")
    #[serde(rename = "@label", default)]
    pub label: String,
    /// Timer digit colour
    #[serde(rename = "@color", default = "default_color")]
    pub color: String,
    /// Label colour
    #[serde(rename = "@labelColor", default = "default_color")]
    pub label_color: String,
    /// Font size for the timer digits
    #[serde(rename = "@fontSize", default = "default_countdown_font_size")]
    pub font_size: f32,
    /// Colour to switch to when under this many seconds remain (0 = disabled)
    #[serde(rename = "@urgentSecs", default)]
    pub urgent_secs: u64,
    /// Colour used when under urgent_secs
    #[serde(rename = "@urgentColor", default = "default_urgent_color")]
    pub urgent_color: String,
    pub effect: Option<Effect>,
}

fn default_countdown_format() -> String { "H:M:S".to_string() }
fn default_countdown_font_size() -> f32 { 16.0 }
fn default_urgent_color() -> String { "#ff2200".to_string() }

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
