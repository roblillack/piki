use crate::content::{ContentLoader, ContentProvider};
use fltk::{enums::Color, window};
use rutle::structured_document::BlockType;
use rutle::tree_path::DocumentPosition;
use std::any::Any;

/// A minimal UI abstraction layer for a note editor/viewer.
///
/// It unifies the interactions needed by main.rs so different
/// implementations (e.g. StructuredRichUI) can be swapped without
/// changing app logic.
pub trait NoteUI: ContentProvider + ContentLoader + 'static {
    // Subscribe to content change notifications (debounced by the app).
    fn on_change(&mut self, f: Box<dyn FnMut() + 'static>);

    // Toggle/query read-only mode.
    fn set_readonly(&mut self, readonly: bool);
    fn is_readonly(&self) -> bool;

    // Scroll position in implementation-defined units (row/pixel).
    fn scroll_pos(&self) -> i32;
    fn set_scroll_pos(&mut self, pos: i32);

    // Caret position within the document. `cursor_pos` returns `None` for a
    // viewer with no caret concept (the default); `set_cursor_pos` is then a
    // no-op. Used to restore the caret when returning to a note.
    fn cursor_pos(&self) -> Option<DocumentPosition> {
        None
    }
    fn set_cursor_pos(&mut self, _pos: DocumentPosition) {}

    // Set background color and make resizable with a window.
    fn set_bg_color(&mut self, color: Color);
    fn set_resizable(&self, wind: &mut window::Window);

    // Install internal event handler to detect link clicks and cursor hints.
    fn on_link_click(&mut self, f: Box<dyn Fn(String) + 'static>);

    // Install handler for link hover; called with Some(dest) when hovering a link,
    // and None when not hovering any link. Default no-op.
    fn on_link_hover(&mut self, _f: Box<dyn Fn(Option<String>) + 'static>) {}

    // Optional restyle hook (no-op by default).
    fn restyle(&mut self) {}

    // Optional periodic tick with ms since app start (no-op by default).
    fn tick(&mut self, _ms_since_start: u64) {}

    // Downcasting support for accessing concrete implementations.
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    // Paragraph style change notification (structured editors can override).
    fn on_paragraph_style_change(&mut self, _f: Box<dyn FnMut(BlockType) + 'static>) {}

    // Hide the widget (called when switching editors).
    fn hide(&mut self);

    // Focus the widget.
    fn take_focus(&mut self) {}
}
