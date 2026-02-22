/// Shared network-fetch and text-render utilities for the web, RSS, and
/// external-data content plugins.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_skia::Pixmap;
use tracing::warn;

// ── HTTP fetch ────────────────────────────────────────────────────────────────

/// Synchronous HTTP GET.  Returns the response body, or `None` on error.
pub fn fetch_url_sync(url: &str) -> Option<String> {
    if url.is_empty() {
        return None;
    }
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("HuiduPlayer/1.0")
        .build()
        .ok()?;
    let resp = client.get(url).send().ok()?;
    if !resp.status().is_success() {
        warn!("HTTP {} fetching {}", resp.status(), url);
        return None;
    }
    resp.text().ok()
}

// ── HTML stripping ────────────────────────────────────────────────────────────

/// Strip HTML tags and decode common entities.
/// Not a full parser — adequate for typical LED-sign content.
pub fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let mut tag_buf = String::new();
    let mut chars = html.chars().peekable();

    while let Some(c) = chars.next() {
        if in_script {
            if c == '<' {
                tag_buf.clear();
                tag_buf.push(c);
            } else if !tag_buf.is_empty() {
                tag_buf.push(c);
                if tag_buf.to_ascii_lowercase().ends_with("</script>") {
                    in_script = false;
                    tag_buf.clear();
                }
            }
            continue;
        }

        match c {
            '<' => {
                in_tag = true;
                tag_buf.clear();
                tag_buf.push('<');
            }
            '>' if in_tag => {
                tag_buf.push('>');
                let inner = tag_buf
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .trim()
                    .to_ascii_lowercase();
                if inner.starts_with("script") {
                    in_script = true;
                }
                // Insert a space for block-level tags to preserve readability
                if inner.starts_with("br")
                    || inner.starts_with("/p")
                    || inner.starts_with("/div")
                    || inner.starts_with("/h")
                    || inner.starts_with("/li")
                    || inner.starts_with("p ")
                    || inner == "p"
                    || inner.starts_with("div")
                    || inner.starts_with("h1")
                    || inner.starts_with("h2")
                    || inner.starts_with("h3")
                    || inner.starts_with("li")
                {
                    out.push(' ');
                }
                in_tag = false;
                tag_buf.clear();
            }
            _ if in_tag => {
                tag_buf.push(c);
            }
            '&' => {
                let mut entity = String::new();
                for ec in chars.by_ref() {
                    if ec == ';' {
                        break;
                    }
                    entity.push(ec);
                    if entity.len() > 10 {
                        break; // not a valid entity
                    }
                }
                let decoded = match entity.to_ascii_lowercase().as_str() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "nbsp" | "#160" => " ",
                    "quot" | "#34" => "\"",
                    "apos" | "#39" => "'",
                    "mdash" | "#8212" => "-",
                    "ndash" | "#8211" => "-",
                    "hellip" | "#8230" => "...",
                    "copy" | "#169" => "(c)",
                    "reg" | "#174" => "(r)",
                    _ => "",
                };
                out.push_str(decoded);
            }
            _ => out.push(c),
        }
    }

    // Collapse whitespace
    let mut result = String::new();
    let mut prev_space = true; // trim leading whitespace
    for c in out.chars() {
        if c.is_whitespace() {
            if !prev_space {
                result.push(' ');
                prev_space = true;
            }
        } else {
            result.push(c);
            prev_space = false;
        }
    }
    result.trim_end().to_string()
}

// ── XML helpers ───────────────────────────────────────────────────────────────

/// Extract the text content of the *first* `<tag>…</tag>` found in `xml`.
/// Handles CDATA sections.
pub fn extract_xml_text(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let start = xml.find(&open)?;
    let after_open = xml[start..].find('>')? + start + 1;
    let end = xml[after_open..].find(&close)? + after_open;
    let raw = xml[after_open..end].trim();
    let text = if raw.starts_with("<![CDATA[") {
        raw.strip_prefix("<![CDATA[")?.trim_end().strip_suffix("]]>")?.to_string()
    } else {
        raw.to_string()
    };
    Some(text.trim().to_string())
}

// ── RSS / Atom parsing ────────────────────────────────────────────────────────

/// Parse RSS/Atom feed XML and return up to `max_items` titles joined by `separator`.
pub fn parse_rss_items(xml: &str, max_items: usize, separator: &str) -> String {
    let mut titles: Vec<String> = Vec::new();

    // Try RSS <item><title>
    let mut search = xml;
    while titles.len() < max_items {
        let Some(item_start) = search.find("<item") else { break };
        let rest = &search[item_start..];
        let Some(item_end) = rest.find("</item>") else { break };
        let item_xml = &rest[..item_end + 7];
        if let Some(title) = extract_xml_text(item_xml, "title") {
            let clean = strip_html(&title);
            if !clean.is_empty() {
                titles.push(clean);
            }
        }
        search = &rest[item_end + 7..];
    }

    // Fall back to Atom <entry><title>
    if titles.is_empty() {
        let mut search = xml;
        while titles.len() < max_items {
            let Some(e_start) = search.find("<entry") else { break };
            let rest = &search[e_start..];
            let Some(e_end) = rest.find("</entry>") else { break };
            let entry_xml = &rest[..e_end + 8];
            if let Some(title) = extract_xml_text(entry_xml, "title") {
                let clean = strip_html(&title);
                if !clean.is_empty() {
                    titles.push(clean);
                }
            }
            search = &rest[e_end + 8..];
        }
    }

    titles.join(separator)
}

// ── JSON path extraction ──────────────────────────────────────────────────────

/// Extract a value using a dot-separated path like `"weather.0.description"`.
/// Numbers are treated as array indices.
pub fn extract_json_path(json: &str, path: &str) -> Option<String> {
    let mut value: serde_json::Value = serde_json::from_str(json).ok()?;
    for part in path.split('.') {
        value = if let Ok(idx) = part.parse::<usize>() {
            value.get(idx)?.clone()
        } else {
            value.get(part)?.clone()
        };
    }
    Some(match &value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => value.to_string(),
    })
}

/// Try JSON path, then XML tag name, then strip+return raw body.
pub fn extract_path_value(body: &str, path: &str) -> String {
    if let Some(v) = extract_json_path(body, path) {
        return v;
    }
    if let Some(v) = extract_xml_text(body, path) {
        return strip_html(&v);
    }
    strip_html(body)
}

// ── Font loading ──────────────────────────────────────────────────────────────

/// Load the built-in DejaVuSans font.
pub fn load_font() -> rusttype::Font<'static> {
    let font_data = include_bytes!("../../../assets/DejaVuSans.ttf");
    rusttype::Font::try_from_bytes(font_data as &[u8]).expect("Failed to load built-in font")
}

// ── Pixel-level text blitting ─────────────────────────────────────────────────

/// Measure pixel width of `text` at `scale`.
pub fn measure_text(font: &rusttype::Font<'_>, text: &str, scale: rusttype::Scale) -> i32 {
    font.layout(text, scale, rusttype::point(0.0, 0.0))
        .filter_map(|g| g.pixel_bounding_box())
        .map(|bb| bb.max.x)
        .last()
        .unwrap_or(0)
}

/// Alpha-blend a single-line text string at `(x_off, y_off)` into `data`.
/// Pixels outside the `tw × th` region are clipped.
fn blit_text_at(
    font: &rusttype::Font<'_>,
    text: &str,
    scale: rusttype::Scale,
    x_off: i32,
    y_off: i32,
    tw: i32,
    th: i32,
    r: u8,
    g: u8,
    b: u8,
    data: &mut [u8],
) {
    let v = font.v_metrics(scale);
    let glyphs: Vec<_> = font
        .layout(text, scale, rusttype::point(0.0, v.ascent))
        .collect();
    for glyph in &glyphs {
        if let Some(bb) = glyph.pixel_bounding_box() {
            glyph.draw(|gx, gy, cv| {
                let alpha = (cv * 255.0) as u8;
                if alpha == 0 {
                    return;
                }
                let px = x_off + bb.min.x + gx as i32;
                let py = y_off + bb.min.y + gy as i32;
                if px < 0 || px >= tw || py < 0 || py >= th {
                    return;
                }
                let idx = ((py * tw + px) * 4) as usize;
                let a = alpha as f32 / 255.0;
                let da = data[idx + 3] as f32 / 255.0;
                let oa = a + da * (1.0 - a);
                if oa > 0.0 {
                    data[idx] =
                        ((r as f32 * a + data[idx] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 1] =
                        ((g as f32 * a + data[idx + 1] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 2] =
                        ((b as f32 * a + data[idx + 2] as f32 * da * (1.0 - a)) / oa) as u8;
                    data[idx + 3] = (oa * 255.0) as u8;
                }
            });
        }
    }
}

// ── High-level draw helpers ───────────────────────────────────────────────────

/// Draw `text` as a seamlessly-looping left-scrolling ticker into `target`.
///
/// `scroll_speed` — pixels per second.
/// `elapsed_ms`   — wall-clock ms since program start.
pub fn draw_ticker(
    target: &mut Pixmap,
    text: &str,
    elapsed_ms: u64,
    scroll_speed: u32,
    font: &rusttype::Font<'_>,
    font_size: f32,
    r: u8,
    g: u8,
    b: u8,
) {
    let tw = target.width() as i32;
    let th = target.height() as i32;
    if tw <= 0 || th <= 0 || text.is_empty() || scroll_speed == 0 {
        return;
    }
    let scale = rusttype::Scale::uniform(font_size);
    let vm = font.v_metrics(scale);
    let text_h = (vm.ascent - vm.descent).ceil() as i32;
    let y_off = ((th - text_h) / 2).max(0);

    let text_w = measure_text(font, text, scale).max(1);
    let gap = (tw / 4).max(40).min(200);
    let cycle = text_w + gap;

    let pixels_per_ms = scroll_speed as f64 / 1000.0;
    let total_scrolled = (elapsed_ms as f64 * pixels_per_ms) as i32;
    let offset = total_scrolled % cycle;

    // First copy enters from right; second copy maintains seamless loop
    let x1 = tw - offset;
    let x2 = x1 + cycle;

    let data = target.data_mut();
    for x_start in [x1, x2] {
        if x_start < tw && x_start + text_w > 0 {
            blit_text_at(font, text, scale, x_start, y_off, tw, th, r, g, b, data);
        }
    }
}

/// Draw `text` centered (horizontally and vertically) in `target`.
pub fn draw_text_centered(
    target: &mut Pixmap,
    text: &str,
    font: &rusttype::Font<'_>,
    font_size: f32,
    r: u8,
    g: u8,
    b: u8,
) {
    let tw = target.width() as i32;
    let th = target.height() as i32;
    if tw <= 0 || th <= 0 || text.is_empty() {
        return;
    }
    let scale = rusttype::Scale::uniform(font_size);
    let vm = font.v_metrics(scale);
    let text_h = (vm.ascent - vm.descent).ceil() as i32;
    let text_w = measure_text(font, text, scale);
    let x_off = ((tw - text_w) / 2).max(0);
    let y_off = ((th - text_h) / 2).max(0);
    let data = target.data_mut();
    blit_text_at(font, text, scale, x_off, y_off, tw, th, r, g, b, data);
}

// ── Fetch-and-cache helper ────────────────────────────────────────────────────

/// `key → (value, fetched_at)` string cache.
pub type TextCache = Arc<Mutex<HashMap<String, (String, Instant)>>>;

/// Return a cached value for `key`, refreshing in a background thread when stale.
pub fn cached_fetch(
    cache: &TextCache,
    key: &str,
    max_age: Duration,
    fetch: impl FnOnce() -> Option<String> + Send + 'static,
) -> String {
    {
        let lock = cache.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((val, ts)) = lock.get(key) {
            if ts.elapsed() < max_age {
                return val.clone();
            }
        }
    }
    let cache_clone = cache.clone();
    let key_owned = key.to_string();
    std::thread::spawn(move || {
        if let Some(text) = fetch() {
            let mut lock = cache_clone.lock().unwrap_or_else(|e| e.into_inner());
            lock.insert(key_owned, (text, Instant::now()));
        }
    });
    // Return stale value (or empty string) while the background refresh runs
    let lock = cache.lock().unwrap_or_else(|e| e.into_inner());
    lock.get(key).map(|(v, _)| v.clone()).unwrap_or_default()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_html_tags() {
        assert_eq!(strip_html("<b>Hello</b>"), "Hello");
        assert_eq!(strip_html("<p>Foo</p><p>Bar</p>"), "Foo Bar");
        assert_eq!(strip_html("plain text"), "plain text");
    }

    #[test]
    fn test_strip_html_entities() {
        assert_eq!(strip_html("A &amp; B"), "A & B");
        assert_eq!(strip_html("&lt;tag&gt;"), "<tag>");
        assert_eq!(strip_html("Say &quot;hi&quot;"), "Say \"hi\"");
        // A bare &nbsp; is pure whitespace; leading whitespace is trimmed to ""
        assert_eq!(strip_html("&nbsp;"), "");
        // &nbsp; between words becomes a regular space
        assert_eq!(strip_html("A&nbsp;B"), "A B");
    }

    #[test]
    fn test_strip_html_collapses_whitespace() {
        assert_eq!(strip_html("  a  b  "), "a b");
        assert_eq!(strip_html("a\n\nb"), "a b");
    }

    #[test]
    fn test_extract_xml_text_simple() {
        let xml = "<feed><title>My Blog</title></feed>";
        assert_eq!(extract_xml_text(xml, "title").as_deref(), Some("My Blog"));
    }

    #[test]
    fn test_extract_xml_text_cdata() {
        let xml = "<item><title><![CDATA[Breaking news!]]></title></item>";
        assert_eq!(
            extract_xml_text(xml, "title").as_deref(),
            Some("Breaking news!")
        );
    }

    #[test]
    fn test_extract_xml_text_missing() {
        assert!(extract_xml_text("<item></item>", "title").is_none());
    }

    #[test]
    fn test_parse_rss_items_basic() {
        let xml = r#"<rss><channel>
            <item><title>First</title></item>
            <item><title>Second</title></item>
            <item><title>Third</title></item>
        </channel></rss>"#;
        let result = parse_rss_items(xml, 2, " | ");
        assert_eq!(result, "First | Second");
    }

    #[test]
    fn test_parse_rss_items_empty() {
        assert_eq!(parse_rss_items("<rss></rss>", 5, " | "), "");
    }

    #[test]
    fn test_parse_rss_items_atom() {
        let xml = r#"<feed>
            <entry><title>Atom One</title></entry>
            <entry><title>Atom Two</title></entry>
        </feed>"#;
        let result = parse_rss_items(xml, 10, ", ");
        assert_eq!(result, "Atom One, Atom Two");
    }

    #[test]
    fn test_extract_json_path_simple() {
        let json = r#"{"name":"Alice","age":30}"#;
        assert_eq!(extract_json_path(json, "name").as_deref(), Some("Alice"));
        assert_eq!(extract_json_path(json, "age").as_deref(), Some("30"));
    }

    #[test]
    fn test_extract_json_path_nested() {
        let json = r#"{"a":{"b":{"c":"deep"}}}"#;
        assert_eq!(
            extract_json_path(json, "a.b.c").as_deref(),
            Some("deep")
        );
    }

    #[test]
    fn test_extract_json_path_array_index() {
        let json = r#"{"items":["x","y","z"]}"#;
        assert_eq!(
            extract_json_path(json, "items.1").as_deref(),
            Some("y")
        );
    }

    #[test]
    fn test_extract_json_path_missing() {
        assert!(extract_json_path(r#"{"a":1}"#, "b").is_none());
        assert!(extract_json_path("not json", "a").is_none());
    }
}
