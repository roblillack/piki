// Library exports for piki
pub mod clipboard;
pub mod content;
pub mod context_menu;
pub mod fltk_draw_context;
pub mod fltk_structured_rich_display;
pub mod link_editor;
pub mod link_handler;
pub mod page_ui;
pub mod responsive_scrollbar;
pub mod rtf;
pub mod ui_adapters;

// The structured editor/layout core (formerly `crate::{draw_context, theme,
// richtext}`) now lives in the shared `tdoc-editor` crate. Re-export under the
// original paths so the rest of piki-gui and its tests keep compiling unchanged.
pub use tdoc_editor::draw_context;
pub use tdoc_editor::richtext;
pub use tdoc_editor::theme;
