/// Rendering engine — composites areas onto a framebuffer using tiny-skia.
use anyhow::Result;
use std::path::Path;
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};
use tracing::debug;

use crate::program::model::{ContentItem, Program};
use crate::render::plugins::clock::ClockRenderer;
use crate::render::plugins::image::ImageRenderer;
use crate::render::plugins::text::TextRenderer;
use crate::render::plugins::ContentRenderer;

/// The main rendering engine
pub struct RenderEngine {
    /// Main framebuffer
    framebuffer: Pixmap,
    /// Per-area scratch surfaces (reused)
    area_surfaces: Vec<Pixmap>,
    /// Content renderers
    image_renderer: ImageRenderer,
    text_renderer: TextRenderer,
    clock_renderer: ClockRenderer,
    /// Frame counter
    frame: u64,
    /// Milliseconds per frame
    ms_per_frame: u64,
}

impl RenderEngine {
    pub fn new(width: u32, height: u32, fps: u32) -> Self {
        Self {
            framebuffer: Pixmap::new(width, height).expect("Failed to create framebuffer"),
            area_surfaces: Vec::new(),
            image_renderer: ImageRenderer::new(),
            text_renderer: TextRenderer::new(),
            clock_renderer: ClockRenderer::new(),
            frame: 0,
            ms_per_frame: 1000 / fps as u64,
        }
    }

    /// Render a complete frame for the given program.
    /// Returns the framebuffer pixel data (RGBA premultiplied).
    pub fn render_frame(&mut self, program: &Program, program_dir: &Path) -> &[u8] {
        let elapsed_ms = self.frame * self.ms_per_frame;

        // Clear framebuffer to black
        self.framebuffer.fill(Color::BLACK);

        // Ensure we have enough scratch surfaces
        while self.area_surfaces.len() < program.areas.len() {
            self.area_surfaces.push(Pixmap::new(1, 1).unwrap());
        }

        // Render each area — we need to split borrows carefully
        for (i, area) in program.areas.iter().enumerate() {
            let rect = &area.rectangle;
            let w = rect.width;
            let h = rect.height;

            if w == 0 || h == 0 {
                continue;
            }

            // Get or resize the scratch surface for this area
            let surface = &mut self.area_surfaces[i];
            if surface.width() != w || surface.height() != h {
                *surface = Pixmap::new(w, h).unwrap_or_else(|| {
                    debug!("Failed to create area surface {}x{}", w, h);
                    Pixmap::new(1, 1).unwrap()
                });
            }
            surface.fill(Color::TRANSPARENT);

            // Render each content item onto the area surface
            for item in &area.resources.items {
                match item {
                    ContentItem::Image(_) => {
                        self.image_renderer
                            .render(item, surface, 0, 0, w, h, elapsed_ms, program_dir);
                    }
                    ContentItem::Text(_) => {
                        self.text_renderer
                            .render(item, surface, 0, 0, w, h, elapsed_ms, program_dir);
                    }
                    ContentItem::Clock(_) => {
                        self.clock_renderer
                            .render(item, surface, 0, 0, w, h, elapsed_ms, program_dir);
                    }
                    ContentItem::Video(_) => {
                        debug!("Video rendering not yet implemented");
                    }
                    ContentItem::Gif(_) => {
                        debug!("GIF rendering not yet implemented");
                    }
                }
            }

            // Composite area surface onto main framebuffer
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

        self.frame += 1;
        self.framebuffer.data()
    }

    /// Get the raw RGBA pixel data of the current framebuffer
    pub fn pixels(&self) -> &[u8] {
        self.framebuffer.data()
    }

    /// Save current framebuffer as PNG
    pub fn save_png(&self, path: &Path) -> Result<()> {
        self.framebuffer
            .save_png(path)
            .map_err(|e| anyhow::anyhow!("Failed to save PNG: {}", e))
    }

    pub fn width(&self) -> u32 {
        self.framebuffer.width()
    }

    pub fn height(&self) -> u32 {
        self.framebuffer.height()
    }

    pub fn frame_count(&self) -> u64 {
        self.frame
    }
}
