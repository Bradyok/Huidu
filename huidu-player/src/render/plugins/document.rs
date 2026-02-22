/// Document/presentation renderer using LibreOffice for conversion.
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tiny_skia::Pixmap;

use crate::program::model::ContentItem;
use crate::render::plugins::ContentRenderer;
use crate::render::plugins::net_text::{draw_text_centered, load_font};

struct DocCache {
    pages: Vec<image::DynamicImage>,
    loaded: bool,
    /// Set when loaded=true but conversion failed
    error: Option<String>,
}

pub struct DocumentRenderer {
    font: rusttype::Font<'static>,
    cache: Arc<Mutex<HashMap<String, DocCache>>>,
}

impl DocumentRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl ContentRenderer for DocumentRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        width: u32,
        height: u32,
        elapsed_ms: u64,
        program_dir: &Path,
    ) -> bool {
        let content = match item {
            ContentItem::Document(c) => c,
            _ => return false,
        };

        let file_path = program_dir.join(&content.file);

        // Check cache: (loaded, page_count, error_msg)
        let pages_opt = {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.get(&content.guid).map(|d| (d.loaded, d.pages.len(), d.error.clone()))
        };

        match pages_opt {
            None => {
                // Not in cache; trigger background conversion
                let guid = content.guid.clone();
                let cache_clone = self.cache.clone();
                let path = file_path.clone();
                {
                    let mut cache = cache_clone.lock().unwrap_or_else(|e| e.into_inner());
                    cache.insert(guid.clone(), DocCache { pages: vec![], loaded: false, error: None });
                }
                std::thread::spawn(move || {
                    match convert_document(&path, &guid) {
                        Ok(pages) => {
                            let mut cache = cache_clone.lock().unwrap_or_else(|e| e.into_inner());
                            cache.insert(guid, DocCache { pages, loaded: true, error: None });
                        }
                        Err(msg) => {
                            tracing::warn!("Document conversion failed: {}", msg);
                            let mut cache = cache_clone.lock().unwrap_or_else(|e2| e2.into_inner());
                            cache.insert(guid, DocCache { pages: vec![], loaded: true, error: Some(msg) });
                        }
                    }
                });
                draw_text_centered(target, &format!("Loading: {}", content.file), &self.font, 10.0, 200, 200, 200);
            }
            Some((false, _, _)) => {
                draw_text_centered(target, "Converting...", &self.font, 10.0, 200, 200, 200);
            }
            Some((true, 0, Some(ref err))) => {
                draw_text_centered(target, err, &self.font, 9.0, 255, 100, 100);
            }
            Some((true, 0, None)) => {
                draw_text_centered(target, "No pages rendered", &self.font, 9.0, 255, 150, 100);
            }
            Some((true, page_count, _)) => {
                let page_ms = content.page_duration as u64 * 1000;
                let page_idx = if page_ms > 0 { (elapsed_ms / page_ms) as usize % page_count } else { 0 };

                let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(doc) = cache.get(&content.guid) {
                    if let Some(img) = doc.pages.get(page_idx) {
                        blit_image(img, target, width, height, &content.fit);
                    }
                }
            }
        }

        true
    }
}

/// Locate a LibreOffice executable, checking standard install locations.
fn find_libreoffice() -> Option<String> {
    let candidates = [
        "libreoffice",
        "soffice",
        "/usr/bin/libreoffice",
        "/usr/bin/soffice",
        "/usr/local/bin/libreoffice",
        // Snap package
        "/snap/bin/libreoffice",
        // Flatpak
        "/var/lib/flatpak/exports/bin/org.libreoffice.LibreOffice",
        // macOS
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    ];
    for cmd in &candidates {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(cmd.to_string());
        }
    }
    None
}

/// Check whether pdftoppm (poppler-utils) is available — fast PDF→PNG converter.
fn find_pdftoppm() -> bool {
    std::process::Command::new("pdftoppm")
        .arg("-v")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|_| true)
        .unwrap_or(false)
}

/// Extract trailing page number from a filename for natural/numeric sort.
///
/// Examples:
/// - "slide1.png"   → 1
/// - "slide10.png"  → 10  (lexicographic "slide10" < "slide2" but numeric 10 > 2)
/// - "page-001.png" → 1   (zero-padded pdftoppm output)
/// - "slide.png"    → 0   (single-page LibreOffice output, no number suffix)
fn page_sort_key(name: &str) -> u64 {
    let base = name.strip_suffix(".png").unwrap_or(name);
    // Collect trailing ASCII digits in reverse, then flip back
    let digits: String = base
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    digits.parse().unwrap_or(0)
}

/// Convert a document file to a vector of page images.
///
/// Uses a per-GUID temp subdirectory to avoid filename collisions between
/// different documents that happen to share a base name.
fn convert_document(path: &Path, guid: &str) -> Result<Vec<image::DynamicImage>, String> {
    // Sanitise GUID for use as a directory name (strip any path-unsafe chars)
    let safe_guid: String = guid.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).collect();

    // Per-GUID subdirectory inside the system temp dir
    let out_dir = std::env::temp_dir()
        .join("huidu_doc_cache")
        .join(&safe_guid);

    std::fs::create_dir_all(&out_dir)
        .map_err(|e| format!("Cannot create temp dir: {}", e))?;

    let ext = path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    // ── Fast path: pdftoppm for PDF files ────────────────────────────────────
    if ext == "pdf" && find_pdftoppm() {
        let prefix = out_dir.join("page");
        let ok = std::process::Command::new("pdftoppm")
            .args([
                "-r", "150",   // 150 DPI — good balance of quality and speed
                "-png",
                path.to_str().unwrap_or(""),
                prefix.to_str().unwrap_or(""),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false);

        if ok {
            let pages = collect_pages(&out_dir, "page", ".png");
            if !pages.is_empty() {
                tracing::info!("Document {:?}: {} page(s) via pdftoppm", path.file_name().unwrap_or_default(), pages.len());
                return Ok(pages);
            }
        }
        tracing::debug!("pdftoppm pass failed, falling back to LibreOffice");
    }

    // ── LibreOffice conversion ────────────────────────────────────────────────
    let lo_cmd = find_libreoffice()
        .ok_or_else(|| "LibreOffice not found — install libreoffice".to_string())?;

    let output = std::process::Command::new(&lo_cmd)
        .args([
            "--headless",
            "--convert-to", "png",
            "--outdir", out_dir.to_str().unwrap_or(""),
            path.to_str().unwrap_or(""),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("LibreOffice launch failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("LibreOffice error: {}", stderr.lines().next().unwrap_or("unknown")));
    }

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let pages = collect_pages(&out_dir, stem, ".png");

    if pages.is_empty() {
        Err(format!("No output from LibreOffice for {:?}", path.file_name().unwrap_or_default()))
    } else {
        tracing::info!("Document {:?}: {} page(s) via LibreOffice", path.file_name().unwrap_or_default(), pages.len());
        Ok(pages)
    }
}

/// Collect PNG files from `dir` whose names start with `stem` and end with `ext`,
/// sorted numerically by the trailing page number (natural sort).
///
/// Handles all naming conventions:
/// - LibreOffice single-page:  `stem.png`           → key 0
/// - LibreOffice multi-page:   `stem1.png` … `stem10.png`  → keys 1…10
/// - pdftoppm:                 `stem-1.png` … `stem-10.png` → keys 1…10
fn collect_pages(dir: &Path, stem: &str, ext: &str) -> Vec<image::DynamicImage> {
    let mut pages: Vec<(u64, image::DynamicImage)> = std::fs::read_dir(dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.starts_with(stem) && s.ends_with(ext)
        })
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            let key = page_sort_key(&name);
            image::open(&path).ok().map(|img| (key, img))
        })
        .collect();

    pages.sort_by_key(|(k, _)| *k);
    pages.into_iter().map(|(_, img)| img).collect()
}

/// Blit (with fit mode) a decoded page image into the tiny-skia Pixmap.
fn blit_image(img: &image::DynamicImage, target: &mut Pixmap, width: u32, height: u32, fit: &str) {
    let resized = match fit {
        "fill" | "stretch" => img.resize_exact(width, height, image::imageops::FilterType::Lanczos3),
        "center" => img.clone(),
        _ => img.resize(width, height, image::imageops::FilterType::Lanczos3),
    };

    let rgba = resized.to_rgba8();
    let (iw, ih) = (rgba.width(), rgba.height());
    let x_off = ((width as i32 - iw as i32) / 2).max(0) as u32;
    let y_off = ((height as i32 - ih as i32) / 2).max(0) as u32;
    let data = target.data_mut();

    for y in 0..ih.min(height) {
        for x in 0..iw.min(width) {
            let pixel = rgba.get_pixel(x, y);
            let [r, g, b, a] = pixel.0;
            let dx = x + x_off;
            let dy = y + y_off;
            if dx >= width || dy >= height { continue; }
            let idx = ((dy * width + dx) * 4) as usize;
            let af = a as f32 / 255.0;
            data[idx]     = (r as f32 * af) as u8;
            data[idx + 1] = (g as f32 * af) as u8;
            data[idx + 2] = (b as f32 * af) as u8;
            data[idx + 3] = a;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::page_sort_key;

    #[test]
    fn natural_sort_order() {
        // Simulates LibreOffice multi-page output — "slide10" must sort after "slide9"
        let mut names = vec![
            "slide10.png",
            "slide2.png",
            "slide1.png",
            "slide9.png",
        ];
        names.sort_by_key(|n| page_sort_key(n));
        assert_eq!(names, ["slide1.png", "slide2.png", "slide9.png", "slide10.png"]);
    }

    #[test]
    fn single_page_no_number() {
        // Single-page LibreOffice output has no trailing number → key 0
        assert_eq!(page_sort_key("presentation.png"), 0);
    }

    #[test]
    fn pdftoppm_hyphen_format() {
        // pdftoppm outputs "prefix-001.png", "prefix-010.png"
        let mut names = vec!["page-010.png", "page-001.png", "page-002.png"];
        names.sort_by_key(|n| page_sort_key(n));
        assert_eq!(names, ["page-001.png", "page-002.png", "page-010.png"]);
    }
}
