/// Video content renderer plugin.
/// Displays the first frame of a video as a still image.
/// Full video decoding would require gstreamer or ffmpeg integration.
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use tracing::{debug, warn};

use crate::program::model::ContentItem;
use crate::render::plugins::ContentRenderer;

pub struct VideoRenderer {
    /// Cache first frame thumbnails
    thumbnails: HashMap<String, Option<Pixmap>>,
}

impl VideoRenderer {
    pub fn new() -> Self {
        Self {
            thumbnails: HashMap::new(),
        }
    }

    fn get_thumbnail(&mut self, filename: &str, program_dir: &Path) -> Option<&Pixmap> {
        if !self.thumbnails.contains_key(filename) {
            let thumb = self.extract_first_frame(filename, program_dir);
            self.thumbnails.insert(filename.to_string(), thumb);
        }
        self.thumbnails.get(filename).and_then(|t| t.as_ref())
    }

    /// Try to extract the first frame using ffmpeg CLI
    fn extract_first_frame(&self, filename: &str, program_dir: &Path) -> Option<Pixmap> {
        let video_path = program_dir.join(filename);
        if !video_path.exists() {
            warn!("Video file not found: {}", video_path.display());
            return None;
        }

        // Try ffmpeg to extract first frame as PNG to temp file
        let temp_path = std::env::temp_dir().join(format!("huidu_thumb_{}.png", md5_hash(filename)));

        let result = Command::new("ffmpeg")
            .args([
                "-y",
                "-i", &video_path.to_string_lossy(),
                "-vframes", "1",
                "-f", "image2",
                &temp_path.to_string_lossy(),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();

        match result {
            Ok(status) if status.success() => {
                debug!("Extracted video thumbnail: {}", temp_path.display());
                // Load the extracted PNG
                match image::open(&temp_path) {
                    Ok(img) => {
                        let rgba = img.to_rgba8();
                        let (w, h) = (rgba.width(), rgba.height());
                        if let Some(mut pixmap) = Pixmap::new(w, h) {
                            let data = pixmap.data_mut();
                            for (i, pixel) in rgba.pixels().enumerate() {
                                let a = pixel[3] as f32 / 255.0;
                                data[i * 4] = (pixel[0] as f32 * a) as u8;
                                data[i * 4 + 1] = (pixel[1] as f32 * a) as u8;
                                data[i * 4 + 2] = (pixel[2] as f32 * a) as u8;
                                data[i * 4 + 3] = pixel[3];
                            }
                            let _ = std::fs::remove_file(&temp_path);
                            return Some(pixmap);
                        }
                    }
                    Err(e) => warn!("Failed to load extracted frame: {}", e),
                }
                let _ = std::fs::remove_file(&temp_path);
            }
            Ok(_) => {
                debug!("ffmpeg failed to extract frame from {}", filename);
            }
            Err(_) => {
                debug!("ffmpeg not available, video thumbnail extraction disabled");
            }
        }

        // Fallback: render a "VIDEO" placeholder
        self.render_placeholder(filename)
    }

    fn render_placeholder(&self, filename: &str) -> Option<Pixmap> {
        let w = 320u32;
        let h = 240u32;
        let mut pixmap = Pixmap::new(w, h)?;

        // Fill with dark gray
        let data = pixmap.data_mut();
        for i in 0..(w * h) as usize {
            data[i * 4] = 30;     // R
            data[i * 4 + 1] = 30; // G
            data[i * 4 + 2] = 30; // B
            data[i * 4 + 3] = 255;
        }

        // Draw a simple play triangle in the center
        let cx = w as i32 / 2;
        let cy = h as i32 / 2;
        let size = 30i32;
        for y in (cy - size)..=(cy + size) {
            let dy = (y - cy).abs();
            let half_w = size - dy;
            for x in (cx - size / 3)..(cx - size / 3 + half_w) {
                if x >= 0 && x < w as i32 && y >= 0 && y < h as i32 {
                    let idx = ((y * w as i32 + x) * 4) as usize;
                    data[idx] = 200;
                    data[idx + 1] = 200;
                    data[idx + 2] = 200;
                    data[idx + 3] = 255;
                }
            }
        }

        Some(pixmap)
    }
}

fn md5_hash(s: &str) -> String {
    format!("{:x}", md5::compute(s.as_bytes()))
}

impl ContentRenderer for VideoRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        width: u32,
        height: u32,
        _elapsed_ms: u64,
        program_dir: &Path,
    ) -> bool {
        let video = match item {
            ContentItem::Video(v) => v,
            _ => return false,
        };

        let thumb = match self.get_thumbnail(&video.file.name, program_dir) {
            Some(t) => t,
            None => return false,
        };

        let scale_x = width as f32 / thumb.width() as f32;
        let scale_y = height as f32 / thumb.height() as f32;

        let (sx, sy) = if video.aspect_ratio {
            let s = scale_x.min(scale_y);
            (s, s)
        } else {
            (scale_x, scale_y)
        };

        let offset_x = ((width as f32 - thumb.width() as f32 * sx) / 2.0) as i32;
        let offset_y = ((height as f32 - thumb.height() as f32 * sy) / 2.0) as i32;

        target.draw_pixmap(
            offset_x, offset_y,
            thumb.as_ref(),
            &PixmapPaint::default(),
            Transform::from_scale(sx, sy),
            None,
        );

        true
    }
}
