/// Transition effects for content item entrance/exit animations.
/// Implements the 30 effect types from the Huidu protocol.
use tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

/// Effect state for an area's content playlist
pub struct EffectState {
    /// Current content index in the area's resource list
    pub current_index: usize,
    /// Phase: Entering, Displaying, Exiting
    pub phase: EffectPhase,
    /// Progress within current phase (0.0 to 1.0)
    pub progress: f32,
    /// When the current phase started (ms)
    pub phase_start_ms: u64,
    /// Duration of display phase in ms (from effect.duration * 100)
    pub display_duration_ms: u64,
    /// Entrance effect type
    pub effect_in: u8,
    /// Exit effect type
    pub effect_out: u8,
    /// Entrance speed (0-8, lower = faster)
    pub in_speed: u8,
    /// Exit speed (0-8, lower = faster)
    pub out_speed: u8,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EffectPhase {
    Entering,
    Displaying,
    Exiting,
    Done,
}

impl EffectState {
    pub fn new(effect_in: u8, effect_out: u8, in_speed: u8, out_speed: u8, duration_tenths: u32) -> Self {
        Self {
            current_index: 0,
            phase: EffectPhase::Entering,
            progress: 0.0,
            phase_start_ms: 0,
            display_duration_ms: duration_tenths as u64 * 100,
            effect_in,
            effect_out,
            in_speed,
            out_speed,
        }
    }

    /// Get the transition duration in ms for a given speed (0=fastest, 8=slowest)
    fn transition_duration_ms(speed: u8) -> u64 {
        match speed {
            0 => 0,       // Instant
            1 => 200,
            2 => 400,
            3 => 600,
            4 => 800,
            5 => 1000,
            6 => 1500,
            7 => 2000,
            8 => 3000,
            _ => 500,
        }
    }

    /// Update the effect state based on elapsed time.
    /// Returns true if the content item should advance to the next one.
    pub fn update(&mut self, elapsed_ms: u64) -> bool {
        match self.phase {
            EffectPhase::Entering => {
                let dur = Self::transition_duration_ms(self.in_speed);
                if dur == 0 || self.effect_in == 0 {
                    self.progress = 1.0;
                    self.phase = EffectPhase::Displaying;
                    self.phase_start_ms = elapsed_ms;
                } else {
                    let elapsed_in_phase = elapsed_ms.saturating_sub(self.phase_start_ms);
                    self.progress = (elapsed_in_phase as f32 / dur as f32).min(1.0);
                    if self.progress >= 1.0 {
                        self.phase = EffectPhase::Displaying;
                        self.phase_start_ms = elapsed_ms;
                    }
                }
                false
            }
            EffectPhase::Displaying => {
                if self.display_duration_ms == 0 {
                    // Duration 0 means display forever (single item or continuous scroll)
                    return false;
                }
                let elapsed_in_phase = elapsed_ms.saturating_sub(self.phase_start_ms);
                if elapsed_in_phase >= self.display_duration_ms {
                    self.phase = EffectPhase::Exiting;
                    self.phase_start_ms = elapsed_ms;
                    self.progress = 0.0;
                }
                false
            }
            EffectPhase::Exiting => {
                let dur = Self::transition_duration_ms(self.out_speed);
                if dur == 0 || self.effect_out == 0 {
                    self.progress = 1.0;
                    self.phase = EffectPhase::Done;
                    return true;
                }
                let elapsed_in_phase = elapsed_ms.saturating_sub(self.phase_start_ms);
                self.progress = (elapsed_in_phase as f32 / dur as f32).min(1.0);
                if self.progress >= 1.0 {
                    self.phase = EffectPhase::Done;
                    return true;
                }
                false
            }
            EffectPhase::Done => true,
        }
    }

    /// Reset for the next content item
    pub fn reset(&mut self, effect_in: u8, effect_out: u8, in_speed: u8, out_speed: u8, duration_tenths: u32, start_ms: u64) {
        self.phase = EffectPhase::Entering;
        self.progress = 0.0;
        self.phase_start_ms = start_ms;
        self.display_duration_ms = duration_tenths as u64 * 100;
        self.effect_in = effect_in;
        self.effect_out = effect_out;
        self.in_speed = in_speed;
        self.out_speed = out_speed;
    }
}

/// Apply a transition effect to a rendered content pixmap,
/// compositing it onto the target area surface.
pub fn apply_effect(
    effect_type: u8,
    progress: f32,
    phase: EffectPhase,
    content: &Pixmap,
    target: &mut Pixmap,
    width: u32,
    height: u32,
) {
    let p = match phase {
        EffectPhase::Entering => progress,
        EffectPhase::Exiting => 1.0 - progress,
        EffectPhase::Displaying => 1.0,
        EffectPhase::Done => return,
    };

    match effect_type {
        0 => {
            // Immediate show
            draw_full(content, target);
        }
        1 => {
            // Left parallel move (slide in from right)
            let offset = ((1.0 - p) * width as f32) as i32;
            target.draw_pixmap(
                offset, 0,
                content.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        2 => {
            // Right parallel move (slide in from left)
            let offset = -((1.0 - p) * width as f32) as i32;
            target.draw_pixmap(
                offset, 0,
                content.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        3 => {
            // Up parallel move (slide in from bottom)
            let offset = ((1.0 - p) * height as f32) as i32;
            target.draw_pixmap(
                0, offset,
                content.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        4 => {
            // Down parallel move (slide in from top)
            let offset = -((1.0 - p) * height as f32) as i32;
            target.draw_pixmap(
                0, offset,
                content.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        5..=8 => {
            // Cover from left/right/up/down (new content covers old)
            // For simplicity, same as parallel move
            let (dx, dy) = match effect_type {
                5 => (-((1.0 - p) * width as f32) as i32, 0),   // from left
                6 => (((1.0 - p) * width as f32) as i32, 0),    // from right
                7 => (0, -((1.0 - p) * height as f32) as i32),  // from top
                8 => (0, ((1.0 - p) * height as f32) as i32),   // from bottom
                _ => (0, 0),
            };
            target.draw_pixmap(
                dx, dy,
                content.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        9..=12 => {
            // Corner covers
            let (dx, dy) = match effect_type {
                9 =>  (-((1.0 - p) * width as f32) as i32, -((1.0 - p) * height as f32) as i32),
                10 => (((1.0 - p) * width as f32) as i32, -((1.0 - p) * height as f32) as i32),
                11 => (-((1.0 - p) * width as f32) as i32, ((1.0 - p) * height as f32) as i32),
                12 => (((1.0 - p) * width as f32) as i32, ((1.0 - p) * height as f32) as i32),
                _ => (0, 0),
            };
            target.draw_pixmap(
                dx, dy,
                content.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
        13 => {
            // Horizontal divide (open from center)
            let half = (p * width as f32 / 2.0) as i32;
            let center = width as i32 / 2;
            // Draw left half
            draw_region(content, target, center - half, 0, 0, 0, half as u32, height);
            // Draw right half
            draw_region(content, target, center, 0, center, 0, half as u32, height);
        }
        14 => {
            // Vertical divide (open from center)
            let half = (p * height as f32 / 2.0) as i32;
            let center = height as i32 / 2;
            draw_region(content, target, 0, center - half, 0, 0, width, half as u32);
            draw_region(content, target, 0, center, 0, center, width, half as u32);
        }
        15 => {
            // Horizontal close (close to center)
            let edge = ((1.0 - p) * width as f32 / 2.0) as i32;
            draw_region(content, target, edge, 0, edge, 0, width - 2 * edge as u32, height);
        }
        16 => {
            // Vertical close
            let edge = ((1.0 - p) * height as f32 / 2.0) as i32;
            draw_region(content, target, 0, edge, 0, edge, width, height - 2 * edge as u32);
        }
        17 => {
            // Fade
            let paint = PixmapPaint {
                opacity: p,
                ..PixmapPaint::default()
            };
            target.draw_pixmap(0, 0, content.as_ref(), &paint, Transform::identity(), None);
        }
        18 => {
            // Horizontal shutter (blinds)
            let num_blinds = 8u32;
            let blind_h = height / num_blinds;
            let visible = (p * blind_h as f32) as u32;
            for i in 0..num_blinds {
                let y = (i * blind_h) as i32;
                draw_region(content, target, 0, y, 0, y, width, visible);
            }
        }
        19 => {
            // Vertical shutter
            let num_blinds = 8u32;
            let blind_w = width / num_blinds;
            let visible = (p * blind_w as f32) as u32;
            for i in 0..num_blinds {
                let x = (i * blind_w) as i32;
                draw_region(content, target, x, 0, x, 0, visible, height);
            }
        }
        20 => {
            // Not clear area — draw without clearing
            draw_full(content, target);
        }
        21..=24 => {
            // Series move (continuous scroll) — handled by the content renderer itself
            // Just draw the full content
            draw_full(content, target);
        }
        25 => {
            // Random — pick a random effect based on time
            let pseudo_type = ((progress * 17.0) as u8 % 17) + 1;
            apply_effect(pseudo_type, progress, phase, content, target, width, height);
        }
        26..=29 => {
            // Head-to-tail series move — same as series move for now
            draw_full(content, target);
        }
        _ => {
            draw_full(content, target);
        }
    }
}

fn draw_full(content: &Pixmap, target: &mut Pixmap) {
    target.draw_pixmap(
        0, 0,
        content.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// Draw a rectangular region from content onto target
fn draw_region(
    content: &Pixmap,
    target: &mut Pixmap,
    dst_x: i32,
    dst_y: i32,
    src_x: i32,
    src_y: i32,
    w: u32,
    h: u32,
) {
    let cw = content.width() as i32;
    let tw = target.width() as i32;
    let th = target.height() as i32;
    let src_data = content.data();
    let dst_data = target.data_mut();

    for row in 0..h as i32 {
        let sy = src_y + row;
        let dy = dst_y + row;
        if sy < 0 || sy >= content.height() as i32 || dy < 0 || dy >= th {
            continue;
        }
        for col in 0..w as i32 {
            let sx = src_x + col;
            let dx = dst_x + col;
            if sx < 0 || sx >= cw || dx < 0 || dx >= tw {
                continue;
            }
            let si = ((sy * cw + sx) * 4) as usize;
            let di = ((dy * tw + dx) * 4) as usize;
            // Simple alpha-over compositing
            let sa = src_data[si + 3] as f32 / 255.0;
            if sa > 0.0 {
                let inv_sa = 1.0 - sa;
                dst_data[di] = (src_data[si] as f32 + dst_data[di] as f32 * inv_sa) as u8;
                dst_data[di + 1] = (src_data[si + 1] as f32 + dst_data[di + 1] as f32 * inv_sa) as u8;
                dst_data[di + 2] = (src_data[si + 2] as f32 + dst_data[di + 2] as f32 * inv_sa) as u8;
                dst_data[di + 3] = ((sa + dst_data[di + 3] as f32 / 255.0 * inv_sa) * 255.0) as u8;
            }
        }
    }
}
