// Markdown Converter
// Converts between StructuredDocument and Markdown text format using tdoc.

use super::structured_document::StructuredDocument;
use super::tdoc_bridge::{structured_to_tdoc, tdoc_to_structured};
use std::io::Cursor;
use tdoc::{html, markdown};

/// Convert markdown text to a [`StructuredDocument`].
pub fn markdown_to_document(markdown: &str) -> StructuredDocument {
    let mut cursor = Cursor::new(markdown.as_bytes());
    match markdown::parse(&mut cursor) {
        Ok(doc) => tdoc_to_structured(&doc),
        Err(_) => StructuredDocument::new(),
    }
}

/// Convert a [`StructuredDocument`] into markdown text.
pub fn document_to_markdown(doc: &StructuredDocument) -> String {
    let tdoc_doc = structured_to_tdoc(doc);
    let mut buffer: Vec<u8> = Vec::new();
    if let Err(err) = markdown::write(&mut buffer, &tdoc_doc) {
        eprintln!("Failed to serialize document to markdown: {}", err);
        return String::new();
    }
    String::from_utf8(buffer).unwrap_or_default()
}

/// Convert a [`StructuredDocument`] into an HTML fragment.
///
/// Unlike markdown, the HTML output can represent every inline style the
/// editor supports (e.g. underline and highlight), which makes it the richer
/// representation for the system clipboard's `text/html` flavor.
pub fn document_to_html(doc: &StructuredDocument) -> String {
    let tdoc_doc = structured_to_tdoc(doc);
    let mut buffer: Vec<u8> = Vec::new();
    if let Err(err) = html::write(&mut buffer, &tdoc_doc) {
        eprintln!("Failed to serialize document to HTML: {}", err);
        return String::new();
    }
    String::from_utf8(buffer).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::richtext::structured_document::{
        Block, BlockType, InlineContent, StructuredDocument, TextRun, TextStyle,
    };

    #[test]
    fn test_markdown_to_document_basic() {
        let md = "Hello **world**!";
        let doc = markdown_to_document(md);
        assert_eq!(doc.block_count(), 1);

        let block = &doc.blocks()[0];
        assert!(matches!(block.block_type, BlockType::Paragraph));
        assert!(
            block
                .content
                .iter()
                .any(|item| matches!(item, InlineContent::Text(run) if run.style.bold))
        );
    }

    #[test]
    fn test_markdown_to_document_checklist() {
        let md = "- [ ] Todo item\n- [x] Done item";
        let doc = markdown_to_document(md);
        assert_eq!(doc.block_count(), 2);

        for (idx, expected) in [Some(false), Some(true)].into_iter().enumerate() {
            match &doc.blocks()[idx].block_type {
                BlockType::ListItem {
                    ordered: false,
                    checkbox,
                    ..
                } => assert_eq!(*checkbox, expected),
                other => panic!("expected checklist item, got {:?}", other),
            }
        }
    }

    #[test]
    fn test_document_to_markdown_paragraph() {
        let mut doc = StructuredDocument::new();
        let mut block = Block::paragraph();
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Hello ")));
        block.content.push(InlineContent::Text(TextRun::new(
            "world",
            TextStyle::bold(),
        )));
        doc.add_block(block);

        let markdown = document_to_markdown(&doc);
        assert_eq!(markdown.trim(), "Hello **world**");
    }

    #[test]
    fn test_document_to_html_preserves_underline_and_highlight() {
        // Underline and highlight cannot be represented in Markdown, so the
        // HTML flavor is what carries them onto the clipboard.
        let mut doc = StructuredDocument::new();
        let mut block = Block::paragraph();
        block.content.push(InlineContent::Text(TextRun::new(
            "under",
            TextStyle {
                underline: true,
                ..Default::default()
            },
        )));
        block.content.push(InlineContent::Text(TextRun::new(
            "mark",
            TextStyle {
                highlight: true,
                ..Default::default()
            },
        )));
        doc.add_block(block);

        let html = document_to_html(&doc);
        assert!(html.contains("<u>under</u>"), "html was: {html}");
        assert!(html.contains("<mark>mark</mark>"), "html was: {html}");
    }

    #[test]
    fn test_document_to_html_ordered_list() {
        let mut doc = StructuredDocument::new();
        for (idx, text) in ["First", "Second"].into_iter().enumerate() {
            let mut block = Block::new(BlockType::ListItem {
                ordered: true,
                number: Some((idx + 1) as u64),
                checkbox: None,
            });
            block
                .content
                .push(InlineContent::Text(TextRun::plain(text)));
            doc.add_block(block);
        }

        let html = document_to_html(&doc);
        assert!(html.contains("<ol>"), "html was: {html}");
        assert!(html.contains("<li>"), "html was: {html}");
        assert!(html.contains("First"), "html was: {html}");
        assert!(html.contains("Second"), "html was: {html}");
    }

    #[test]
    fn test_markdown_table_parses_to_table_block() {
        // A markdown table parses into a single Table block whose rows/cells
        // mirror the source, with the first row's cells flagged as headers.
        let md = "| A | B |\n| --- | --- |\n| 1 | 2 |";
        let doc = markdown_to_document(md);

        assert_eq!(doc.block_count(), 1, "expected a single table block");
        let BlockType::Table { rows } = &doc.blocks()[0].block_type else {
            panic!(
                "expected a Table block, got {:?}",
                doc.blocks()[0].block_type
            );
        };

        assert_eq!(rows.len(), 2, "header row + one body row");
        assert_eq!(rows[0].cells.len(), 2);
        assert!(
            rows[0].cells.iter().all(|c| c.is_header),
            "first row cells should be headers"
        );
        assert_eq!(rows[0].cells[0].to_plain_text().trim(), "A");
        assert_eq!(rows[0].cells[1].to_plain_text().trim(), "B");

        assert!(
            rows[1].cells.iter().all(|c| !c.is_header),
            "body row cells should not be headers"
        );
        assert_eq!(rows[1].cells[0].to_plain_text().trim(), "1");
        assert_eq!(rows[1].cells[1].to_plain_text().trim(), "2");
    }

    #[test]
    fn test_table_round_trips_through_markdown() {
        // A table survives a structured -> markdown -> structured round trip.
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
    fn test_document_to_markdown_ordered_list() {
        let mut doc = StructuredDocument::new();
        for (idx, text) in ["First", "Second"].into_iter().enumerate() {
            let mut block = Block::new(BlockType::ListItem {
                ordered: true,
                number: Some((idx + 1) as u64),
                checkbox: None,
            });
            block
                .content
                .push(InlineContent::Text(TextRun::plain(text)));
            doc.add_block(block);
        }

        let markdown = document_to_markdown(&doc);
        assert!(
            markdown.trim() == "1. First\n2. Second",
            "unexpected markdown output: {markdown:?}"
        );
    }
}
