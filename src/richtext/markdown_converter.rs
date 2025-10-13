// Markdown Converter
// Converts between StructuredDocument and Markdown text format
// Markdown is used purely as a storage/serialization format

use super::markdown_ast::{ASTNode, Document as ASTDocument, NodeType};
use super::markdown_parser::parse_markdown;
use super::structured_document::*;

/// Convert markdown text to a StructuredDocument
pub fn markdown_to_document(markdown: &str) -> StructuredDocument {
    let ast_doc = parse_markdown(markdown);
    ast_to_structured(&ast_doc)
}

/// Convert a StructuredDocument to markdown text
pub fn document_to_markdown(doc: &StructuredDocument) -> String {
    let mut output = String::new();

    for (i, block) in doc.blocks().iter().enumerate() {
        if i > 0 {
            output.push_str("\n\n");
        }

        match &block.block_type {
            BlockType::Paragraph => {
                output.push_str(&inline_content_to_markdown(&block.content));
            }
            BlockType::Heading { level } => {
                output.push_str(&"#".repeat(*level as usize));
                output.push(' ');
                output.push_str(&inline_content_to_markdown(&block.content));
            }
            BlockType::CodeBlock { language } => {
                output.push_str("```");
                if let Some(lang) = language {
                    output.push_str(lang);
                }
                output.push('\n');
                output.push_str(&block.to_plain_text());
                output.push_str("\n```");
            }
            BlockType::BlockQuote => {
                output.push_str("> ");
                output.push_str(&inline_content_to_markdown(&block.content));
            }
            BlockType::ListItem { ordered, number } => {
                if *ordered {
                    if let Some(n) = number {
                        output.push_str(&format!("{}. ", n));
                    } else {
                        output.push_str("1. ");
                    }
                } else {
                    output.push_str("- ");
                }
                output.push_str(&inline_content_to_markdown(&block.content));
            }
        }
    }

    output
}

/// Convert inline content to markdown
fn inline_content_to_markdown(content: &[InlineContent]) -> String {
    let mut output = String::new();

    for item in content {
        match item {
            InlineContent::Text(run) => {
                let text = &run.text;

                // Handle code specially (overrides other styles)
                let styled = if run.style.code {
                    format!("`{}`", text)
                } else {
                    // Build up style wrappers from outermost to innermost
                    let mut result = text.clone();

                    // Strikethrough (outermost)
                    if run.style.strikethrough {
                        result = format!("~~{}~~", result);
                    }

                    // Bold and/or italic
                    if run.style.bold && run.style.italic {
                        result = format!("***{}***", result);
                    } else if run.style.bold {
                        result = format!("**{}**", result);
                    } else if run.style.italic {
                        result = format!("*{}*", result);
                    }

                    // Underline (HTML tag)
                    if run.style.underline {
                        result = format!("<u>{}</u>", result);
                    }

                    // Highlight (HTML tag, outermost)
                    if run.style.highlight {
                        result = format!("<mark>{}</mark>", result);
                    }

                    result
                };
                output.push_str(&styled);
            }
            InlineContent::Link { link, content } => {
                output.push('[');
                output.push_str(&inline_content_to_markdown(content));
                output.push_str("](");
                output.push_str(&link.destination);
                if let Some(title) = &link.title {
                    output.push_str(" \"");
                    output.push_str(title);
                    output.push('"');
                }
                output.push(')');
            }
            InlineContent::LineBreak => {
                output.push(' ');
            }
            InlineContent::HardBreak => {
                output.push_str("  \n");
            }
        }
    }

    output
}

/// Convert AST to StructuredDocument
fn ast_to_structured(ast_doc: &ASTDocument) -> StructuredDocument {
    let mut doc = StructuredDocument::new();

    for child in &ast_doc.root.children {
        ast_node_to_blocks(child, &mut doc);
    }

    // Ensure at least one block exists
    if doc.is_empty() {
        doc.add_block(Block::paragraph(0));
    }

    doc
}

/// Convert an AST node to one or more blocks (handles List which contains multiple ListItems)
fn ast_node_to_blocks(node: &ASTNode, doc: &mut StructuredDocument) {
    match &node.node_type {
        NodeType::List { ordered, start } => {
            // Process each list item as a separate block
            for (idx, child) in node.children.iter().enumerate() {
                if let NodeType::ListItem = child.node_type {
                    let number = if *ordered {
                        Some(start + idx as u64)
                    } else {
                        None
                    };

                    let mut block = Block::new(
                        0,
                        BlockType::ListItem {
                            ordered: *ordered,
                            number,
                        },
                    );
                    block.content = ast_node_to_inline_content(child);
                    doc.add_block(block);
                }
            }
        }
        _ => {
            // Handle other block types
            if let Some(block) = ast_node_to_block(node, doc) {
                doc.add_block(block);
            }
        }
    }
}

/// Convert an AST node to a Block
fn ast_node_to_block(node: &ASTNode, doc: &mut StructuredDocument) -> Option<Block> {
    let id = 0; // Will be assigned by document

    match &node.node_type {
        NodeType::Paragraph => {
            let mut block = Block::paragraph(id);
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        NodeType::Heading { level } => {
            let mut block = Block::heading(id, *level);
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        NodeType::CodeBlock { language, .. } => {
            let mut block = Block::new(
                id,
                BlockType::CodeBlock {
                    language: language.clone(),
                },
            );
            let text = node.flatten_text();
            block.content = vec![InlineContent::Text(TextRun::plain(text))];
            Some(block)
        }
        NodeType::BlockQuote => {
            let mut block = Block::new(id, BlockType::BlockQuote);
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        NodeType::ListItem => {
            // Determine if parent is ordered or unordered
            // For now, assume unordered
            let mut block = Block::new(
                id,
                BlockType::ListItem {
                    ordered: false,
                    number: None,
                },
            );
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        _ => None,
    }
}

/// Convert AST node children to inline content
fn ast_node_to_inline_content(node: &ASTNode) -> Vec<InlineContent> {
    let mut content = Vec::new();

    for child in &node.children {
        match &child.node_type {
            NodeType::Text {
                content: text,
                style,
            } => {
                let text_style = TextStyle {
                    bold: style.bold,
                    italic: style.italic,
                    code: style.code,
                    strikethrough: style.strikethrough,
                    underline: style.underline,
                    highlight: style.highlight,
                };
                content.push(InlineContent::Text(TextRun::new(text, text_style)));
            }
            NodeType::WikiLink { destination: _ } => {}
            NodeType::Code { content: text } => {
                content.push(InlineContent::Text(TextRun::new(text, TextStyle::code())));
            }
            NodeType::Link { destination, title } => {
                let link = Link {
                    destination: destination.clone(),
                    title: title.clone(),
                };
                let link_content = ast_node_to_inline_content(child);
                content.push(InlineContent::Link {
                    link,
                    content: link_content,
                });
            }
            NodeType::SoftBreak => {
                content.push(InlineContent::LineBreak);
            }
            NodeType::HardBreak => {
                content.push(InlineContent::HardBreak);
            }
            _ => {
                // Recursively process container nodes
                if child.node_type.can_have_children() {
                    content.extend(ast_node_to_inline_content(child));
                }
            }
        }
    }

    content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_to_document_paragraph() {
        let md = "Hello world";
        let doc = markdown_to_document(md);
        assert_eq!(doc.block_count(), 1);
        assert_eq!(doc.to_plain_text(), "Hello world");
    }

    #[test]
    fn test_markdown_to_document_heading() {
        let md = "# Heading 1\n\nSome text";
        let doc = markdown_to_document(md);
        assert_eq!(doc.block_count(), 2);

        if let BlockType::Heading { level } = doc.blocks()[0].block_type {
            assert_eq!(level, 1);
        } else {
            panic!("Expected heading");
        }
    }

    #[test]
    fn test_document_to_markdown_paragraph() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::paragraph(0).with_plain_text("Hello world"));

        let md = document_to_markdown(&doc);
        assert_eq!(md, "Hello world");
    }

    #[test]
    fn test_document_to_markdown_heading() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::heading(0, 1).with_plain_text("Title"));

        let md = document_to_markdown(&doc);
        assert_eq!(md, "# Title");
    }

    #[test]
    fn test_document_to_markdown_list() {
        let mut doc = StructuredDocument::new();
        doc.add_block(
            Block::new(
                0,
                BlockType::ListItem {
                    ordered: false,
                    number: None,
                },
            )
            .with_plain_text("Item 1"),
        );
        doc.add_block(
            Block::new(
                0,
                BlockType::ListItem {
                    ordered: false,
                    number: None,
                },
            )
            .with_plain_text("Item 2"),
        );

        let md = document_to_markdown(&doc);
        assert_eq!(md, "- Item 1\n\n- Item 2");
    }

    #[test]
    fn test_round_trip() {
        let original = "# Heading\n\nSome **bold** text.";
        let doc = markdown_to_document(original);
        let md = document_to_markdown(&doc);

        // Re-parse to verify structure is preserved
        let doc2 = markdown_to_document(&md);
        assert_eq!(doc.block_count(), doc2.block_count());
    }

    #[test]
    fn test_markdown_to_document_wikilink() {
        let md = "A [[WikiPage]] link";
        let doc = markdown_to_document(md);
        assert_eq!(doc.block_count(), 1);
        let block = &doc.blocks()[0];
        // Expect at least one InlineContent::Link
        let has_link = block
            .content
            .iter()
            .any(|c| matches!(c, InlineContent::Link { .. }));
        assert!(has_link);
    }
}
