// Markdown / HTML conversion.
//
// The authoritative document is a `tdoc::Document`, so these are thin wrappers over
// tdoc's own parsers/writers. HTML can represent every inline style the editor supports
// (e.g. underline and highlight), which Markdown cannot, so it is the richer flavor for
// the system clipboard's `text/html`.

use std::io::Cursor;
use tdoc::{Document, html, markdown};

/// Parse markdown text into a [`tdoc::Document`]. Returns an empty document on error.
pub fn markdown_to_document(markdown: &str) -> Document {
    let mut cursor = Cursor::new(markdown.as_bytes());
    markdown::parse(&mut cursor).unwrap_or_else(|_| Document::new())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tdoc::inline::{InlineStyle, Span};
    use tdoc::paragraph::Paragraph;

    fn first_text(doc: &Document) -> &[Span] {
        doc.paragraphs.first().map(|p| p.content()).unwrap_or(&[])
    }

    #[test]
    fn markdown_to_document_basic() {
        let doc = markdown_to_document("Hello **world**!");
        assert_eq!(doc.paragraphs.len(), 1);
        // Somewhere in the spans there is a Bold node.
        fn has_bold(spans: &[Span]) -> bool {
            spans
                .iter()
                .any(|s| s.style == InlineStyle::Bold || has_bold(&s.children))
        }
        assert!(has_bold(first_text(&doc)), "expected a bold span: {doc:?}");
    }

    #[test]
    fn checklist_parses_to_checklist_paragraph() {
        let doc = markdown_to_document("- [ ] Todo item\n- [x] Done item");
        assert!(
            doc.paragraphs
                .iter()
                .any(|p| matches!(p, Paragraph::Checklist { .. })),
            "expected a checklist paragraph: {doc:?}"
        );
    }

    #[test]
    fn document_to_markdown_bold_paragraph() {
        let doc = Document::new().with_paragraphs(vec![Paragraph::new_text().with_content(vec![
            Span::new_text("Hello "),
            Span::new_styled(InlineStyle::Bold).with_children(vec![Span::new_text("world")]),
        ])]);
        assert_eq!(document_to_markdown(&doc).trim(), "Hello **world**");
    }

    #[test]
    fn document_to_html_preserves_underline_and_highlight() {
        let doc = Document::new().with_paragraphs(vec![Paragraph::new_text().with_content(vec![
            Span::new_styled(InlineStyle::Underline).with_children(vec![Span::new_text("under")]),
            Span::new_styled(InlineStyle::Highlight).with_children(vec![Span::new_text("mark")]),
        ])]);
        let html = document_to_html(&doc);
        assert!(html.contains("<u>under</u>"), "html was: {html}");
        assert!(html.contains("<mark>mark</mark>"), "html was: {html}");
    }

    #[test]
    fn nested_list_round_trips_through_markdown() {
        let md = "- a\n    - b\n- c";
        let doc = markdown_to_document(md);
        let rendered = document_to_markdown(&doc);
        let reparsed = markdown_to_document(&rendered);
        assert_eq!(
            doc, reparsed,
            "nested list should be stable across a markdown round trip; rendered:\n{rendered}"
        );
    }

    #[test]
    fn table_round_trips_through_markdown() {
        let md = "| Name | Qty |\n| --- | --- |\n| Apples | 3 |\n| Pears | 12 |";
        let doc = markdown_to_document(md);
        let rendered = document_to_markdown(&doc);
        let reparsed = markdown_to_document(&rendered);
        assert_eq!(
            doc, reparsed,
            "table should be stable across a markdown round trip; rendered:\n{rendered}"
        );
    }

    #[test]
    fn ordered_list_round_trips_with_numbering() {
        let md = "1. First\n2. Second";
        let doc = markdown_to_document(md);
        assert_eq!(document_to_markdown(&doc).trim(), md);
    }
}
