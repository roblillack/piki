// Markdown Converter
// Converts between StructuredDocument and Markdown text format using tdoc.

use super::structured_document::StructuredDocument;
use super::tdoc_bridge::{structured_to_tdoc, tdoc_to_structured};
use std::io::Cursor;
use tdoc::markdown;

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
