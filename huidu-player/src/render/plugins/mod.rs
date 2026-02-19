pub mod clock;
pub mod gif;
pub mod image;
pub mod text;
pub mod video;

use tiny_skia::Pixmap;

use crate::program::model::ContentItem;

/// Trait for content renderer plugins
pub trait ContentRenderer {
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
