/// GIF animation renderer plugin.
/// Decodes GIF frames and cycles through them with proper timing.
use std::collections::HashMap;
use std::path::Path;
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use tracing::{debug, warn};

use crate::program::model::ContentItem;
use crate::render::plugins::ContentRenderer;

struct GifData {
    frames: Vec<GifFrame>,
    total_duration_ms: u64,
}

struct GifFrame {
    pixmap: Pixmap,
    delay_ms: u64,
    cumulative_ms: u64,
}

pub struct GifRenderer {
    cache: HashMap<String, GifData>,
}

impl GifRenderer {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    fn load_gif(&mut self, filename: &str, program_dir: &Path) -> Option<&GifData> {
        if self.cache.contains_key(filename) {
            return self.cache.get(filename);
        }

        let path = program_dir.join(filename);
        debug!("Loading GIF: {}", path.display());

        let file = match std::fs::File::open(&path) {
            Ok(f) => f,
            Err(e) => {
                warn!("Failed to open GIF {}: {}", path.display(), e);
                return None;
            }
        };

        let mut decoder = match gif::DecodeOptions::new().read_info(file) {
            Ok(d) => d,
            Err(e) => {
                warn!("Failed to decode GIF {}: {}", path.display(), e);
                return None;
            }
        };

        let width = decoder.width() as u32;
        let height = decoder.height() as u32;

        let mut frames = Vec::new();
        let mut cumulative = 0u64;

        // Composite canvas for handling disposal methods
        let mut canvas = Pixmap::new(width, height).unwrap();
        canvas.fill(tiny_skia::Color::TRANSPARENT);

        while let Ok(Some(frame)) = decoder.read_next_frame() {
            let delay_ms = (frame.delay as u64) * 10; // GIF delay is in centiseconds
            let delay_ms = if delay_ms == 0 { 100 } else { delay_ms }; // Default 100ms

            let fw = frame.width as u32;
            let fh = frame.height as u32;
            let fx = frame.left as i32;
            let fy = frame.top as i32;

            // Create frame pixmap from RGBA buffer
            if let Some(mut frame_pixmap) = Pixmap::new(fw, fh) {
                let data = frame_pixmap.data_mut();
                let src = &frame.buffer;
                for i in 0..(fw * fh) as usize {
                    let si = i * 4;
                    if si + 3 < src.len() {
                        let a = src[si + 3] as f32 / 255.0;
                        data[si] = (src[si] as f32 * a) as u8;
                        data[si + 1] = (src[si + 1] as f32 * a) as u8;
                        data[si + 2] = (src[si + 2] as f32 * a) as u8;
                        data[si + 3] = src[si + 3];
                    }
                }

                // Composite frame onto canvas
                canvas.draw_pixmap(
                    fx, fy,
                    frame_pixmap.as_ref(),
                    &PixmapPaint::default(),
                    Transform::identity(),
                    None,
                );
            }

            // Save a snapshot of the current canvas
            if let Some(mut snapshot) = Pixmap::new(width, height) {
                snapshot.data_mut().copy_from_slice(canvas.data());
                frames.push(GifFrame {
                    pixmap: snapshot,
                    delay_ms,
                    cumulative_ms: cumulative,
                });
            }

            cumulative += delay_ms;
        }

        if frames.is_empty() {
            warn!("GIF has no frames: {}", path.display());
            return None;
        }

        debug!("Loaded GIF: {} frames, {}ms total", frames.len(), cumulative);

        let data = GifData {
            total_duration_ms: cumulative,
            frames,
        };
        self.cache.insert(filename.to_string(), data);
        self.cache.get(filename)
    }
}

impl ContentRenderer for GifRenderer {
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
        let gif_content = match item {
            ContentItem::Gif(g) => g,
            _ => return false,
        };

        let filename = &gif_content.file.name;
        let gif_data = match self.load_gif(filename, program_dir) {
            Some(d) => d,
            None => return false,
        };

        if gif_data.frames.is_empty() {
            return false;
        }

        // Find current frame based on elapsed time
        let loop_time = elapsed_ms % gif_data.total_duration_ms;
        let frame_idx = gif_data
            .frames
            .iter()
            .rposition(|f| loop_time >= f.cumulative_ms)
            .unwrap_or(0);

        let frame = &gif_data.frames[frame_idx];

        // Scale and draw onto target
        let src_w = frame.pixmap.width() as f32;
        let src_h = frame.pixmap.height() as f32;
        let scale_x = width as f32 / src_w;
        let scale_y = height as f32 / src_h;

        target.draw_pixmap(
            0, 0,
            frame.pixmap.as_ref(),
            &PixmapPaint::default(),
            Transform::from_scale(scale_x, scale_y),
            None,
        );

        true
    }
}
