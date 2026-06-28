//! `tdoc-editor` — a rendering-agnostic structured editor for [`tdoc::Document`].
//!
//! This crate is the shared editing/layout core extracted from Piki's GUI. The
//! authoritative document is always a [`tdoc::Document`]; [`StructuredEditor`]
//! owns the tree and performs every mutation, while [`StructuredRichDisplay`]
//! lays the document out against a backend-agnostic [`DrawContext`].
//!
//! Frontends provide their own [`DrawContext`] implementation:
//! - Piki's GUI provides an FLTK backend (`FltkDrawContext`).
//! - Pure's TUI provides a terminal/cell backend (`RatatuiDrawContext`).
//! - Tests use an SVG or stub backend.
//!
//! The module names (`draw_context`, `theme`, `richtext`) are preserved from the
//! original Piki layout so existing consumers can re-export them unchanged.

pub mod draw_context;
pub mod richtext;
pub mod theme;

// Convenience re-exports for the most common entry points.
pub use draw_context::{DrawContext, FontStyle, FontType};
pub use richtext::structured_document::{
    Block, BlockType, InlineContent, Link, TableCell, TableRow, TextRun, TextStyle,
};
pub use richtext::structured_editor::{EditError, StructuredEditor, UndoKind};
pub use richtext::structured_rich_display::StructuredRichDisplay;
pub use richtext::tree_path::{DocumentPosition, PathSegment, TreePath};
pub use theme::{FontSettings, Theme};
