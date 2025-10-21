use crate::content::{ContentLoader, ContentProvider};
use crate::richtext::structured_document::BlockType;
use fltk::{enums::Color, window};
use std::any::Any;

/// A minimal UI abstraction layer for a page editor/viewer.
///
/// It unifies the interactions needed by main.rs so different
/// implementations (MarkdownEditor, TextDisplay, StructuredRichDisplay)
/// can be swapped without changing app logic.
pub trait PageUI: ContentProvider + ContentLoader + 'static {
    // Subscribe to content change notifications (debounced by the app).
    fn on_change(&mut self, f: Box<dyn FnMut() + 'static>);

    // Toggle/query read-only mode.
    fn set_readonly(&mut self, readonly: bool);
    fn is_readonly(&self) -> bool;

    // Scroll position in implementation-defined units (row/pixel).
    fn scroll_pos(&self) -> i32;
    fn set_scroll_pos(&mut self, pos: i32);

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
}
