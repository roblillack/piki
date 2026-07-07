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
    let markdown = String::from_utf8(buffer).unwrap_or_default();
    // An empty note is represented in the editor by a single empty paragraph
    // (see `StructuredRichUI::set_content_from_markdown`), which serializes to a
    // lone newline. Normalize that — and any whitespace-only document — back to
    // the empty string so a freshly created, untouched note stays empty on disk
    // and autosave does not persist a file for it.
    if markdown.trim().is_empty() {
        return String::new();
    }
    markdown
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The seeded empty paragraph a fresh note carries (so formatting commands
    /// work immediately) must serialize back to an empty string — otherwise an
    /// untouched new note would be persisted to disk as a lone newline.
    #[test]
    fn seeded_empty_paragraph_serializes_to_empty_string() {
        let mut doc = Document::new();
        doc.add_paragraph(tdoc::Paragraph::new_text());
        assert_eq!(document_to_markdown(&doc), "");
    }

    #[test]
    fn empty_document_serializes_to_empty_string() {
        assert_eq!(document_to_markdown(&Document::new()), "");
    }

    /// A note with real content is untouched — in particular its trailing
    /// newline is preserved, so existing notes are not spuriously rewritten.
    #[test]
    fn real_content_round_trips_unchanged() {
        let doc = markdown_to_document("# Title\n\nBody text\n");
        assert_eq!(document_to_markdown(&doc), "# Title\n\nBody text\n");
    }
}
