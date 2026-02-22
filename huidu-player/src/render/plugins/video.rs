/// Video content renderer plugin.
///
/// Uses ffmpeg (CLI) to decode video files into raw RGBA frames, then plays
/// them back by selecting the correct frame on each render call based on
/// `elapsed_ms`.  Frames are decoded once at first use and cached in memory.
///
/// Limits:
///   - Output framerate is fixed at 25 fps (40 ms/frame).
///   - At most MAX_FRAMES frames are decoded per video (caps very long clips).
///   - Frames are scaled to the area dimensions at decode time.
///   - Aspect-ratio mode (`aspectRatio="true"`) letterboxes/pillarboxes.
///
/// If ffmpeg is not on PATH the renderer falls back to a static placeholder.
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use tiny_skia::{Pixmap, PixmapPaint, Transform};
use tracing::{debug, info, warn};

use crate::program::model::ContentItem;
use crate::render::plugins::ContentRenderer;

/// Output framerate used for all decoded videos.
const OUTPUT_FPS: u64 = 25;
/// ms per decoded frame.
const FRAME_MS: u64 = 1000 / OUTPUT_FPS;
/// Maximum frames decoded per video (~72 s at 25 fps).
const MAX_FRAMES: usize = 1800;

// ── Decoded video cache ───────────────────────────────────────────────────────

/// All decoded frames for one video at one resolution.
struct DecodedVideo {
    width: u32,
    height: u32,
    /// Raw premultiplied-RGBA pixel data, one `Vec<u8>` per frame.
    frames: Vec<Vec<u8>>,
}

impl DecodedVideo {
    /// Select the frame that should be displayed at `elapsed_ms`, looping.
    fn frame_at(&self, elapsed_ms: u64) -> &[u8] {
        let idx = ((elapsed_ms / FRAME_MS) as usize) % self.frames.len();
        &self.frames[idx]
    }
}

/// Cache key: file path + target dimensions (aspect-ratio flag changes dims).
#[derive(Hash, PartialEq, Eq, Clone)]
struct VideoKey {
    path: String,
    width: u32,
    height: u32,
}

// ── Renderer ─────────────────────────────────────────────────────────────────

pub struct VideoRenderer {
    cache: HashMap<VideoKey, Option<DecodedVideo>>,
}

impl VideoRenderer {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    /// Return cached video (or decode it now).  Returns `None` if ffmpeg is
    /// unavailable or the file cannot be decoded.
    fn get_video(
        &mut self,
        filename: &str,
        program_dir: &Path,
        target_w: u32,
        target_h: u32,
        aspect_ratio: bool,
    ) -> Option<&DecodedVideo> {
        let video_path = program_dir.join(filename);
        let key = VideoKey {
            path: video_path.to_string_lossy().into_owned(),
            width: target_w,
            height: target_h,
        };

        if !self.cache.contains_key(&key) {
            let decoded = decode_video(&video_path, target_w, target_h, aspect_ratio);
            self.cache.insert(key.clone(), decoded);
        }

        self.cache.get(&key).and_then(|v| v.as_ref())
    }
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
        elapsed_ms: u64,
        program_dir: &Path,
    ) -> bool {
        let video = match item {
            ContentItem::Video(v) => v,
            _ => return false,
        };

        let decoded = match self.get_video(
            &video.file.name,
            program_dir,
            width,
            height,
            video.aspect_ratio,
        ) {
            Some(d) => d,
            None => return false,
        };

        // Build a Pixmap view over the selected frame and blit it.
        let frame_data = decoded.frame_at(elapsed_ms);
        if let Some(frame_pixmap) =
            Pixmap::from_vec(frame_data.to_vec(), tiny_skia::IntSize::from_wh(decoded.width, decoded.height).unwrap())
        {
            target.draw_pixmap(
                0, 0,
                frame_pixmap.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
            true
        } else {
            false
        }
    }
}

// ── ffmpeg decoding ───────────────────────────────────────────────────────────

/// Decode up to MAX_FRAMES frames from `video_path` at the given target
/// resolution using ffmpeg.  Returns `None` if ffmpeg is unavailable or the
/// file cannot be read.
fn decode_video(
    video_path: &Path,
    target_w: u32,
    target_h: u32,
    aspect_ratio: bool,
) -> Option<DecodedVideo> {
    if !video_path.exists() {
        warn!("Video file not found: {}", video_path.display());
        return Some(make_placeholder(target_w, target_h));
    }

    // Build the scale filter.  With aspect_ratio we letterbox/pillarbox;
    // without it we stretch to fill exactly.
    let vf = if aspect_ratio {
        format!(
            "scale={w}:{h}:force_original_aspect_ratio=decrease,\
             pad={w}:{h}:(ow-iw)/2:(oh-ih)/2:black,\
             fps={fps}",
            w = target_w,
            h = target_h,
            fps = OUTPUT_FPS,
        )
    } else {
        format!(
            "scale={w}:{h},fps={fps}",
            w = target_w,
            h = target_h,
            fps = OUTPUT_FPS,
        )
    };

    info!(
        "Decoding video {} at {}x{} (aspect_ratio={}) …",
        video_path.display(), target_w, target_h, aspect_ratio
    );

    let mut child = match Command::new("ffmpeg")
        .args([
            "-i",
            &video_path.to_string_lossy(),
            "-vf",
            &vf,
            "-vframes",
            &MAX_FRAMES.to_string(),
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => {
            warn!("ffmpeg not available — video playback disabled");
            return Some(make_placeholder(target_w, target_h));
        }
    };

    let frame_bytes = (target_w * target_h * 4) as usize;
    let mut frames: Vec<Vec<u8>> = Vec::new();

    if let Some(mut stdout) = child.stdout.take() {
        let mut buf = vec![0u8; frame_bytes];
        loop {
            match read_exact_or_eof(&mut stdout, &mut buf) {
                ReadResult::Full => {
                    // Convert straight RGBA → premultiplied RGBA (tiny-skia).
                    let mut premul = buf.clone();
                    for px in premul.chunks_exact_mut(4) {
                        let a = px[3] as f32 / 255.0;
                        px[0] = (px[0] as f32 * a) as u8;
                        px[1] = (px[1] as f32 * a) as u8;
                        px[2] = (px[2] as f32 * a) as u8;
                    }
                    frames.push(premul);
                    if frames.len() >= MAX_FRAMES {
                        break;
                    }
                }
                ReadResult::Eof => break,
                ReadResult::Error => break,
            }
        }
    }

    let _ = child.wait();

    if frames.is_empty() {
        debug!("ffmpeg produced no frames for {}", video_path.display());
        return Some(make_placeholder(target_w, target_h));
    }

    info!(
        "Decoded {} frames from {} ({}x{})",
        frames.len(),
        video_path.display(),
        target_w,
        target_h,
    );

    Some(DecodedVideo {
        width: target_w,
        height: target_h,
        frames,
    })
}

// ── I/O helpers ──────────────────────────────────────────────────────────────

enum ReadResult {
    Full,
    Eof,
    Error,
}

/// Read exactly `buf.len()` bytes.  Returns Eof if the stream ends before the
/// buffer is filled (partial frame = discard), Error on I/O failure.
fn read_exact_or_eof(reader: &mut dyn Read, buf: &mut [u8]) -> ReadResult {
    let mut filled = 0;
    while filled < buf.len() {
        match reader.read(&mut buf[filled..]) {
            Ok(0) => return ReadResult::Eof,
            Ok(n) => filled += n,
            Err(_) => return ReadResult::Error,
        }
    }
    ReadResult::Full
}

// ── Placeholder ───────────────────────────────────────────────────────────────

/// Return a single-frame "VIDEO" placeholder used when ffmpeg is not available.
fn make_placeholder(w: u32, h: u32) -> DecodedVideo {
    let mut pixels = vec![0u8; (w * h * 4) as usize];

    // Dark charcoal background
    for px in pixels.chunks_exact_mut(4) {
        px[0] = 25;
        px[1] = 25;
        px[2] = 30;
        px[3] = 255;
    }

    // Simple play-triangle in the centre
    let cx = w as i32 / 2;
    let cy = h as i32 / 2;
    let sz = (h as i32 / 4).max(4);
    for dy in -sz..=sz {
        let half = sz - dy.abs();
        for dx in (-sz / 3)..((-sz / 3) + half) {
            let px = cx + dx;
            let py = cy + dy;
            if px >= 0 && px < w as i32 && py >= 0 && py < h as i32 {
                let i = ((py * w as i32 + px) * 4) as usize;
                pixels[i] = 180;
                pixels[i + 1] = 180;
                pixels[i + 2] = 180;
                pixels[i + 3] = 255;
            }
        }
    }

    DecodedVideo {
        width: w,
        height: h,
        frames: vec![pixels],
    }
}
