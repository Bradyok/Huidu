pub mod analog_clock;
pub mod calendar;
pub mod clock;
pub mod countdown;
pub mod gif;
pub mod image;
pub mod neon;
pub mod qrcode;
pub mod table;
pub mod text;
pub mod video;
pub mod weather;

use tiny_skia::Pixmap;

use crate::program::model::ContentItem;

/// Trait for content renderer plugins
pub trait ContentRenderer {
    #[allow(clippy::too_many_arguments)]
    fn render(
        &mut self,
        item: &ContentItem,
        target: &mut Pixmap,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        elapsed_ms: u64,
        program_dir: &std::path::Path,
    ) -> bool;
}
