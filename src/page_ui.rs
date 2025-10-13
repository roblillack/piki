use crate::content::{ContentLoader, ContentProvider};
use fltk::{enums::Color, prelude::*, window};

/// A minimal UI abstraction layer for a page editor/viewer.
///
/// It unifies the interactions needed by main.rs so different
/// implementations (MarkdownEditor, TextDisplay, StructuredRichDisplay)
/// can be swapped without changing app logic.
pub trait PageUI: ContentProvider + ContentLoader {
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

    // Optional restyle hook (no-op by default).
    fn restyle(&mut self) {}
}
