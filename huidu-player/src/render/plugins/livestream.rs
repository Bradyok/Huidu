/// Live stream content renderer (RTSP/RTMP/HTTP video via ffmpeg subprocess).
use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tiny_skia::Pixmap;

use crate::program::model::{ContentItem, LiveStreamContent};
use crate::render::plugins::ContentRenderer;
use crate::render::plugins::net_text::{draw_text_centered, load_font};

struct StreamState {
    frames: VecDeque<Vec<u8>>,
    width: u32,
    height: u32,
    status: StreamStatus,
}

#[derive(PartialEq)]
enum StreamStatus {
    Idle,
    Running,
    Reconnecting,
    Failed,
}

pub struct LiveStreamRenderer {
    font: rusttype::Font<'static>,
    /// Per-GUID stream state: frame ring buffer + status
    streams: std::collections::HashMap<String, Arc<Mutex<StreamState>>>,
}

impl LiveStreamRenderer {
    pub fn new() -> Self {
        Self {
            font: load_font(),
            streams: std::collections::HashMap::new(),
        }
    }

    fn ensure_stream(&mut self, content: &LiveStreamContent, width: u32, height: u32) -> Arc<Mutex<StreamState>> {
        self.streams
            .entry(content.guid.clone())
            .or_insert_with(|| {
                let state = Arc::new(Mutex::new(StreamState {
                    frames: VecDeque::with_capacity(8),
                    width,
                    height,
                    status: StreamStatus::Idle,
                }));
                spawn_stream_reader(
                    content.url.clone(),
                    width,
                    height,
                    content.reconnect,
                    state.clone(),
                );
                state
            })
            .clone()
    }
}

fn spawn_stream_reader(url: String, width: u32, height: u32, reconnect: bool, state: Arc<Mutex<StreamState>>) {
    std::thread::spawn(move || {
        loop {
            {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.status = StreamStatus::Running;
            }

            let result = run_ffmpeg_stream(&url, width, height, &state);

            if !reconnect {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.status = StreamStatus::Failed;
                break;
            }

            {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                s.status = StreamStatus::Reconnecting;
            }

            tracing::warn!("Stream {} ended ({:?}), reconnecting in 10s", url, result);
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
    });
}

fn run_ffmpeg_stream(url: &str, width: u32, height: u32, state: &Arc<Mutex<StreamState>>) -> std::io::Result<()> {
    use std::io::Read;
    use std::process::{Command, Stdio};

    let mut child = Command::new("ffmpeg")
        .args([
            "-rtsp_transport", "tcp",
            "-i", url,
            "-vf", &format!("scale={}:{},fps=25", width, height),
            "-f", "rawvideo",
            "-pix_fmt", "rgba",
            "pipe:1",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    let frame_size = (width * height * 4) as usize;
    let mut stdout = child.stdout.take().ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "no stdout"))?;
    let mut buf = vec![0u8; frame_size];

    loop {
        match stdout.read_exact(&mut buf) {
            Ok(()) => {
                let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
                if s.frames.len() >= 8 {
                    s.frames.pop_front();
                }
                // Convert RGBA to premul RGBA for tiny-skia
                let mut frame = buf.clone();
                for chunk in frame.chunks_exact_mut(4) {
                    let a = chunk[3] as f32 / 255.0;
                    chunk[0] = (chunk[0] as f32 * a) as u8;
                    chunk[1] = (chunk[1] as f32 * a) as u8;
                    chunk[2] = (chunk[2] as f32 * a) as u8;
                }
                s.frames.push_back(frame);
            }
            Err(_) => break,
        }
    }

    let _ = child.wait();
    Ok(())
}

impl ContentRenderer for LiveStreamRenderer {
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        _x: i32,
        _y: i32,
        width: u32,
        height: u32,
        _elapsed_ms: u64,
        _program_dir: &Path,
    ) -> bool {
        let content = match item {
            ContentItem::LiveStream(c) => c,
            _ => return false,
        };

        let state_arc = self.ensure_stream(content, width, height);
        let state = state_arc.lock().unwrap_or_else(|e| e.into_inner());

        if let Some(frame) = state.frames.back() {
            // Blit frame to target (assumes same size)
            let target_data = target.data_mut();
            let copy_len = frame.len().min(target_data.len());
            target_data[..copy_len].copy_from_slice(&frame[..copy_len]);
        } else {
            // Show placeholder
            let msg = match state.status {
                StreamStatus::Reconnecting => "Reconnecting...".to_string(),
                StreamStatus::Failed => "Stream failed".to_string(),
                _ => format!("Stream: {}", content.url.chars().take(20).collect::<String>()),
            };
            drop(state);
            draw_text_centered(target, &msg, &self.font, 10.0, 255, 255, 0);
        }

        true
    }
}
