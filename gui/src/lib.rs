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
// richtext}`) now lives in the shared `rutle` crate. rutle renamed several items
// (`DrawContext`→`RenderContext`, `StructuredEditor`→`Editor`,
// `StructuredRichDisplay`→`Renderer`) and flattened the `richtext` module to its
// crate root, so this is a thin façade that re-exports rutle under piki's
// original module paths/names — the rest of piki-gui and its tests keep
// compiling unchanged.
pub use rutle::theme;

pub mod draw_context {
    pub use rutle::render_context::{FontStyle, FontType, RenderContext as DrawContext};
}

pub mod richtext {
    pub use rutle::{inline_convert, reveal, structured_document, tree_edit, tree_path, tree_walk};

    pub mod structured_editor {
        pub use rutle::editor::{EditError, EditResult, Editor as StructuredEditor, UndoKind};
    }

    pub mod structured_rich_display {
        pub use rutle::renderer::{Renderer as StructuredRichDisplay, SearchMatch};
    }

    // rutle works on `tdoc::Document` directly and leaves (de)serialization to
    // `tdoc`, keeping these thin wrappers test-only. Re-provide the ones piki
    // needs for the clipboard and page load/save.
    pub mod markdown_converter {
        use std::io::Cursor;
        use tdoc::{Document, html, markdown};

        /// Parse markdown text into a [`tdoc::Document`]. Empty document on error.
        pub fn markdown_to_document(src: &str) -> Document {
            markdown::parse(Cursor::new(src.as_bytes())).unwrap_or_else(|_| Document::new())
        }

        /// Serialize a [`tdoc::Document`] into markdown text.
        pub fn document_to_markdown(doc: &Document) -> String {
            let mut buffer: Vec<u8> = Vec::new();
            if let Err(err) = markdown::write(&mut buffer, doc) {
                eprintln!("Failed to serialize document to markdown: {}", err);
                return String::new();
            }
            String::from_utf8(buffer).unwrap_or_default()
        }

        /// Serialize a [`tdoc::Document`] into an HTML fragment.
        pub fn document_to_html(doc: &Document) -> String {
            let mut buffer: Vec<u8> = Vec::new();
            if let Err(err) = html::write(&mut buffer, doc) {
                eprintln!("Failed to serialize document to HTML: {}", err);
                return String::new();
            }
            String::from_utf8(buffer).unwrap_or_default()
        }
    }
}
