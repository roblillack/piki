// Library exports for piki
pub mod accents_menu;
pub mod clipboard;
pub mod content;
pub mod context_menu;
pub mod fltk_draw_context;
pub mod fltk_structured_rich_display;
pub mod link_editor;
pub mod link_handler;
pub mod live_share;
pub mod markdown_converter;
pub mod note_ui;
pub mod on_air_bar;
pub mod responsive_scrollbar;
pub mod rtf;
pub mod section_link;
pub mod ui_adapters;

// The structured editor/layout core lives in the shared `rutle` crate; piki-gui
// uses its types (`rutle::Renderer`, `rutle::Editor`, `rutle::RenderContext`, …)
// and modules (`rutle::structured_document`, `rutle::tree_walk`, …) directly.
