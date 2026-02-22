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

        // Check cache
        let pages_opt = {
            let cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
            cache.get(&content.guid).map(|d| (d.loaded, d.pages.len()))
        };

        match pages_opt {
            None => {
                // Not in cache; trigger background conversion
                let guid = content.guid.clone();
                let cache_clone = self.cache.clone();
                let path = file_path.clone();
                {
                    let mut cache = cache_clone.lock().unwrap_or_else(|e| e.into_inner());
                    cache.insert(guid.clone(), DocCache { pages: vec![], loaded: false });
                }
                std::thread::spawn(move || {
                    let pages = convert_document(&path);
                    let mut cache = cache_clone.lock().unwrap_or_else(|e| e.into_inner());
                    cache.insert(guid, DocCache { pages, loaded: true });
                });
                draw_text_centered(target, &format!("Loading: {}", content.file), &self.font, 10.0, 200, 200, 200);
            }
            Some((false, _)) => {
                draw_text_centered(target, "Converting...", &self.font, 10.0, 200, 200, 200);
            }
            Some((true, 0)) => {
                draw_text_centered(target, "LibreOffice required", &self.font, 9.0, 255, 100, 100);
            }
            Some((true, page_count)) => {
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

fn find_libreoffice() -> Option<&'static str> {
    for cmd in &["libreoffice", "soffice"] {
        if std::process::Command::new(cmd)
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return Some(cmd);
        }
    }
    None
}

fn convert_document(path: &Path) -> Vec<image::DynamicImage> {
    let Some(lo_cmd) = find_libreoffice() else {
        tracing::warn!("LibreOffice not found — document rendering unavailable");
        return vec![];
    };

    let out_dir = std::path::PathBuf::from("/tmp/huidu_doc_cache");
    let _ = std::fs::create_dir_all(&out_dir);

    let status = std::process::Command::new(lo_cmd)
        .args([
            "--headless",
            "--convert-to", "png",
            "--outdir", out_dir.to_str().unwrap_or("/tmp"),
            path.to_str().unwrap_or(""),
        ])
        .status();

    if status.map(|s| !s.success()).unwrap_or(true) {
        tracing::warn!("LibreOffice conversion failed for {:?}", path);
        return vec![];
    }

    // Collect generated PNG files sorted by name (page order)
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    let mut pages: Vec<(String, image::DynamicImage)> = std::fs::read_dir(&out_dir)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let s = name.to_string_lossy();
            s.starts_with(stem) && s.ends_with(".png")
        })
        .filter_map(|e| {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            image::open(&p).ok().map(|img| (name, img))
        })
        .collect();

    pages.sort_by(|a, b| a.0.cmp(&b.0));
    pages.into_iter().map(|(_, img)| img).collect()
}

fn blit_image(img: &image::DynamicImage, target: &mut Pixmap, width: u32, height: u32, fit: &str) {
    let resized = match fit {
        "fill" | "stretch" => img.resize_exact(width, height, image::imageops::FilterType::Lanczos3),
        "center" => {
            // Center without scaling
            img.clone()
        }
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
