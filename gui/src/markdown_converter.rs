//! Markdown/HTML (de)serialization between text and [`tdoc::Document`].
//!
//! rutle works on `tdoc::Document` directly and leaves (de)serialization to
//! `tdoc`. These thin wrappers are the entry points piki-gui needs for the
//! clipboard and note load/save.

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
