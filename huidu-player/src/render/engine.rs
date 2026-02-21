/// Rendering engine — composites areas onto a framebuffer using tiny-skia.
/// Handles content cycling with transition effects.
use anyhow::Result;
use std::path::Path;
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

use crate::program::model::{ContentItem, Program};
use crate::render::effects::{self, EffectPhase, EffectState};
use crate::render::plugins::clock::ClockRenderer;
use crate::render::plugins::gif::GifRenderer;
use crate::render::plugins::image::ImageRenderer;
use crate::render::plugins::text::TextRenderer;
use crate::render::plugins::video::VideoRenderer;
use crate::render::plugins::ContentRenderer;

/// Rotate raw RGBA pixel data by multiples of 90°.
fn rotate_pixels(src: &[u8], src_w: u32, src_h: u32, degrees: u16) -> (u32, u32, Vec<u8>) {
    let (dst_w, dst_h) = match degrees {
        90 | 270 => (src_h, src_w),
        _ => (src_w, src_h),
    };
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
    for y in 0..src_h {
        for x in 0..src_w {
            let si = ((y * src_w + x) * 4) as usize;
            let (dx, dy) = match degrees {
                90  => (src_h - 1 - y, x),
                180 => (src_w - 1 - x, src_h - 1 - y),
                270 => (y, src_w - 1 - x),
                _   => (x, y),
            };
            let di = ((dy * dst_w + dx) * 4) as usize;
            dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    (dst_w, dst_h, dst)
}

/// Per-area state for content cycling
struct AreaState {
    /// Which content item is currently displayed (index into resources)
    current_item: usize,
    /// Effect state for the current transition
    effect: EffectState,
}

pub struct RenderEngine {
    framebuffer: Pixmap,
    area_surfaces: Vec<Pixmap>,
    content_surfaces: Vec<Pixmap>,
    area_states: Vec<AreaState>,
    image_renderer: ImageRenderer,
    text_renderer: TextRenderer,
    clock_renderer: ClockRenderer,
    gif_renderer: GifRenderer,
    video_renderer: VideoRenderer,
    frame: u64,
    ms_per_frame: u64,
    /// Software brightness level (0-100)
    brightness: u8,
    /// Screen rotation in degrees (0 / 90 / 180 / 270)
    rotation: u16,
}

impl RenderEngine {
    pub fn new(width: u32, height: u32, fps: u32) -> Self {
        Self {
            framebuffer: Pixmap::new(width, height).expect("Failed to create framebuffer"),
            area_surfaces: Vec::new(),
            content_surfaces: Vec::new(),
            area_states: Vec::new(),
            image_renderer: ImageRenderer::new(),
            text_renderer: TextRenderer::new(),
            clock_renderer: ClockRenderer::new(),
            gif_renderer: GifRenderer::new(),
            video_renderer: VideoRenderer::new(),
            frame: 0,
            ms_per_frame: 1000 / fps as u64,
            brightness: 100,
            rotation: 0,
        }
    }

    pub fn set_brightness(&mut self, level: u8) {
        self.brightness = level.min(100);
    }

    pub fn set_rotation(&mut self, degrees: u16) {
        self.rotation = degrees % 360;
    }

    pub fn rotation(&self) -> u16 {
        self.rotation
    }

    /// Encode the current framebuffer as PNG bytes (with rotation applied).
    pub fn screenshot_png(&self) -> Vec<u8> {
        let (w, h, pixels) = if self.rotation != 0 {
            rotate_pixels(self.framebuffer.data(), self.framebuffer.width(), self.framebuffer.height(), self.rotation)
        } else {
            (self.framebuffer.width(), self.framebuffer.height(), self.framebuffer.data().to_vec())
        };
        // tiny-skia uses premultiplied RGBA; convert back to straight alpha for PNG
        let mut straight = pixels.clone();
        for chunk in straight.chunks_exact_mut(4) {
            let a = chunk[3] as f32 / 255.0;
            if a > 0.0 {
                chunk[0] = (chunk[0] as f32 / a).min(255.0) as u8;
                chunk[1] = (chunk[1] as f32 / a).min(255.0) as u8;
                chunk[2] = (chunk[2] as f32 / a).min(255.0) as u8;
            }
        }
        if let Some(img) = image::RgbaImage::from_raw(w, h, straight) {
            let mut cursor = std::io::Cursor::new(Vec::new());
            if img.write_to(&mut cursor, image::ImageFormat::Png).is_ok() {
                return cursor.into_inner();
            }
        }
        Vec::new()
    }

    /// Reset area states when a new program is loaded.
    /// `start_ms` should be the current render clock so that effect phases are
    /// anchored to the present rather than to time zero.
    pub fn reset_for_program(&mut self, program: &Program, start_ms: u64) {
        self.area_states.clear();
        for area in &program.areas {
            let items = &area.resources.items;
            let effect = if !items.is_empty() {
                get_effect_for_item(&items[0], start_ms)
            } else {
                EffectState::new(0, 0, 0, 0, 50, start_ms)
            };
            self.area_states.push(AreaState {
                current_item: 0,
                effect,
            });
        }
    }

    /// Render a complete frame for the given program.
    pub fn render_frame(&mut self, program: &Program, program_dir: &Path) -> &[u8] {
        let elapsed_ms = self.frame * self.ms_per_frame;

        // Initialize area states if needed
        if self.area_states.len() != program.areas.len() {
            self.reset_for_program(program, elapsed_ms);
        }

        self.framebuffer.fill(Color::BLACK);

        // Ensure scratch surfaces
        while self.area_surfaces.len() < program.areas.len() {
            self.area_surfaces.push(Pixmap::new(1, 1).unwrap());
        }
        while self.content_surfaces.len() < program.areas.len() {
            self.content_surfaces.push(Pixmap::new(1, 1).unwrap());
        }

        for (i, area) in program.areas.iter().enumerate() {
            let rect = &area.rectangle;
            let w = rect.width;
            let h = rect.height;
            if w == 0 || h == 0 {
                continue;
            }

            // Ensure surfaces are correct size
            let surface = &mut self.area_surfaces[i];
            if surface.width() != w || surface.height() != h {
                *surface = Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
            }
            surface.fill(Color::TRANSPARENT);

            let content_surface = &mut self.content_surfaces[i];
            if content_surface.width() != w || content_surface.height() != h {
                *content_surface =
                    Pixmap::new(w, h).unwrap_or_else(|| Pixmap::new(1, 1).unwrap());
            }

            let items = &area.resources.items;
            if items.is_empty() {
                continue;
            }

            // Update effect state and check if we should advance
            let area_state = &mut self.area_states[i];
            let should_advance = area_state.effect.update(elapsed_ms);

            if should_advance && items.len() > 1 {
                // Advance to next content item
                area_state.current_item = (area_state.current_item + 1) % items.len();
                let next_item = &items[area_state.current_item];
                let eff = get_effect_for_item(next_item, elapsed_ms);
                area_state.effect.reset(
                    eff.effect_in,
                    eff.effect_out,
                    eff.in_speed,
                    eff.out_speed,
                    (eff.display_duration_ms / 100) as u32,
                    elapsed_ms,
                );
            }

            let current_idx = area_state.current_item;
            let item = &items[current_idx];

            // Render content into content surface
            content_surface.fill(Color::TRANSPARENT);
            match item {
                ContentItem::Image(_) => {
                    self.image_renderer.render(
                        item, content_surface, 0, 0, w, h, elapsed_ms, program_dir,
                    );
                }
                ContentItem::Text(_) => {
                    self.text_renderer.render(
                        item, content_surface, 0, 0, w, h, elapsed_ms, program_dir,
                    );
                }
                ContentItem::Clock(_) => {
                    self.clock_renderer.render(
                        item, content_surface, 0, 0, w, h, elapsed_ms, program_dir,
                    );
                }
                ContentItem::Gif(_) => {
                    self.gif_renderer.render(
                        item, content_surface, 0, 0, w, h, elapsed_ms, program_dir,
                    );
                }
                ContentItem::Video(_) => {
                    self.video_renderer.render(
                        item, content_surface, 0, 0, w, h, elapsed_ms, program_dir,
                    );
                }
            }

            // Apply transition effect
            let effect_type = match area_state.effect.phase {
                EffectPhase::Entering => area_state.effect.effect_in,
                EffectPhase::Exiting => area_state.effect.effect_out,
                _ => 0,
            };

            effects::apply_effect(
                effect_type,
                area_state.effect.progress,
                area_state.effect.phase,
                content_surface,
                surface,
                w,
                h,
            );

            // Composite area onto framebuffer
            let alpha = area.alpha as f32 / 255.0;
            let paint = PixmapPaint {
                opacity: alpha,
                ..PixmapPaint::default()
            };
            self.framebuffer.draw_pixmap(
                rect.x,
                rect.y,
                surface.as_ref(),
                &paint,
                Transform::identity(),
                None,
            );
        }

        // Apply software brightness
        if self.brightness < 100 {
            let factor = self.brightness as f32 / 100.0;
            let data = self.framebuffer.data_mut();
            for chunk in data.chunks_exact_mut(4) {
                chunk[0] = (chunk[0] as f32 * factor) as u8;
                chunk[1] = (chunk[1] as f32 * factor) as u8;
                chunk[2] = (chunk[2] as f32 * factor) as u8;
            }
        }

        self.frame += 1;
        self.framebuffer.data()
    }

    pub fn pixels(&self) -> &[u8] {
        self.framebuffer.data()
    }

    pub fn save_png(&self, path: &Path) -> Result<()> {
        if self.rotation == 0 {
            self.framebuffer
                .save_png(path)
                .map_err(|e| anyhow::anyhow!("Failed to save PNG: {}", e))
        } else {
            let png = self.screenshot_png();
            std::fs::write(path, &png)?;
            Ok(())
        }
    }

    pub fn width(&self) -> u32 {
        self.framebuffer.width()
    }

    pub fn height(&self) -> u32 {
        self.framebuffer.height()
    }
}

/// Extract effect params from a content item, anchored to `start_ms`.
fn get_effect_for_item(item: &ContentItem, start_ms: u64) -> EffectState {
    let eff = match item {
        ContentItem::Image(i) => i.effect.as_ref(),
        ContentItem::Text(t) => t.effect.as_ref(),
        ContentItem::Gif(g) => g.effect.as_ref(),
        _ => None,
    };

    match eff {
        Some(e) => EffectState::new(e.effect_in, e.effect_out, e.in_speed, e.out_speed, e.duration, start_ms),
        None => EffectState::new(0, 0, 0, 0, 50, start_ms), // default 5 seconds, immediate
    }
}
