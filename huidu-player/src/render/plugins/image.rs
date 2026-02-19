/// Image content renderer plugin.
/// Loads PNG/JPG/BMP images and renders them to the area surface.
use std::collections::HashMap;
use std::path::Path;
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use tracing::{debug, warn};

use crate::program::model::ContentItem;
use crate::render::plugins::ContentRenderer;

pub struct ImageRenderer {
    /// Cache of loaded images by filename
    cache: HashMap<String, Pixmap>,
}

impl ImageRenderer {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn load_image(&mut self, filename: &str, program_dir: &Path) -> Option<&Pixmap> {
        if self.cache.contains_key(filename) {
            return self.cache.get(filename);
        }

        let path = program_dir.join(filename);
        debug!("Loading image: {}", path.display());

        match image::open(&path) {
            Ok(img) => {
                let rgba = img.to_rgba8();
                let (w, h) = (rgba.width(), rgba.height());
                // tiny-skia expects premultiplied alpha RGBA
                if let Some(mut pixmap) = Pixmap::new(w, h) {
                    let data = pixmap.data_mut();
                    for (i, pixel) in rgba.pixels().enumerate() {
                        let a = pixel[3] as f32 / 255.0;
                        data[i * 4] = (pixel[0] as f32 * a) as u8;
                        data[i * 4 + 1] = (pixel[1] as f32 * a) as u8;
                        data[i * 4 + 2] = (pixel[2] as f32 * a) as u8;
                        data[i * 4 + 3] = pixel[3];
                    }
                    self.cache.insert(filename.to_string(), pixmap);
                    return self.cache.get(filename);
                }
            }
            Err(e) => {
                warn!("Failed to load image {}: {}", path.display(), e);
            }
        }
        None
    }
}

impl ContentRenderer for ImageRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        _elapsed_ms: u64,
        program_dir: &Path,
    ) -> bool {
        let img_content = match item {
            ContentItem::Image(img) => img,
            _ => return false,
        };

        let filename = &img_content.file.name;
        let fit_mode = &img_content.fit;

        let src_pixmap = match self.load_image(filename, program_dir) {
            Some(p) => p,
            None => return false,
        };

        let src_w = src_pixmap.width() as f32;
        let src_h = src_pixmap.height() as f32;
        let dst_w = width as f32;
        let dst_h = height as f32;

        // Calculate transform based on fit mode
        let transform = match fit_mode.as_str() {
            "stretch" => {
                // Stretch to fill area (may distort)
                Transform::from_scale(dst_w / src_w, dst_h / src_h)
                    .post_translate(x as f32, y as f32)
            }
            "fill" => {
                // Scale to fill, maintaining aspect ratio (may crop)
                let scale = (dst_w / src_w).max(dst_h / src_h);
                let sx = (dst_w - src_w * scale) / 2.0;
                let sy = (dst_h - src_h * scale) / 2.0;
                Transform::from_scale(scale, scale).post_translate(x as f32 + sx, y as f32 + sy)
            }
            "center" => {
                // Center without scaling
                let sx = (dst_w - src_w) / 2.0;
                let sy = (dst_h - src_h) / 2.0;
                Transform::from_translate(x as f32 + sx, y as f32 + sy)
            }
            _ => {
                // Default: fit (scale to fit, maintaining aspect ratio)
                let scale = (dst_w / src_w).min(dst_h / src_h);
                let sx = (dst_w - src_w * scale) / 2.0;
                let sy = (dst_h - src_h * scale) / 2.0;
                Transform::from_scale(scale, scale).post_translate(x as f32 + sx, y as f32 + sy)
            }
        };

        // Draw the image onto the target
        target.draw_pixmap(
            0,
            0,
            src_pixmap.as_ref(),
            &PixmapPaint::default(),
            transform,
            None,
        );

        true
    }
}
