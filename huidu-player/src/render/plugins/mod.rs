pub mod clock;
pub mod image;
pub mod text;

use tiny_skia::Pixmap;

use crate::program::model::ContentItem;

/// Trait for content renderer plugins
pub trait ContentRenderer {
    /// Render this content item onto the given pixmap at the given area bounds.
    /// Returns true if the content changed (needs redraw).
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
