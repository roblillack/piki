// Markdown Converter
// Converts between StructuredDocument and Markdown text format
// Markdown is used purely as a storage/serialization format

use super::markdown_ast::{ASTNode, Document as ASTDocument, NodeType};
use super::markdown_parser::parse_markdown;
use super::structured_document::*;

const MAX_LINE_WIDTH: usize = 80;
const BR_TOKEN: &str = "<<__FLIKI_BR__>>";
const NEWLINE_TOKEN: &str = "<<__FLIKI_NL__>>";

/// Convert markdown text to a StructuredDocument
pub fn markdown_to_document(markdown: &str) -> StructuredDocument {
    let ast_doc = parse_markdown(markdown);
    ast_to_structured(&ast_doc)
}

/// Convert a StructuredDocument to markdown text
pub fn document_to_markdown(doc: &StructuredDocument) -> String {
    let mut output = String::new();

    let mut is_first_block = true;
    let mut prev_list_ordered: Option<bool> = None;

    for block in doc.blocks() {
        let current_list_ordered = match &block.block_type {
            BlockType::ListItem { ordered, .. } => Some(*ordered),
            _ => None,
        };

        if !is_first_block {
            match (prev_list_ordered, current_list_ordered) {
                (Some(prev), Some(curr)) if prev == curr => output.push('\n'),
                _ => output.push_str("\n\n"),
            }
        }

        let block_markdown = match &block.block_type {
            BlockType::Paragraph => {
                wrap_text_with_indent(&inline_content_to_markdown(&block.content), "", "")
            }
            BlockType::Heading { level } => {
                let mut heading = "#".repeat(*level as usize);
                heading.push(' ');
                heading.push_str(&inline_content_to_markdown(&block.content));
                heading
            }
            BlockType::CodeBlock { language } => {
                let mut code = String::from("```");
                if let Some(lang) = language {
                    code.push_str(lang);
                }
                code.push('\n');
                code.push_str(&block.to_plain_text());
                code.push_str("\n```");
                code
            }
            BlockType::BlockQuote => {
                wrap_text_with_indent(&inline_content_to_markdown(&block.content), "> ", "> ")
            }
            BlockType::ListItem {
                ordered,
                number,
                checkbox,
            } => {
                let inline = inline_content_to_markdown(&block.content);
                let (initial_indent, subsequent_indent) =
                    list_item_indents(*ordered, *number, *checkbox);
                wrap_text_with_indent(&inline, &initial_indent, &subsequent_indent)
            }
        };

        output.push_str(&block_markdown);

        is_first_block = false;
        prev_list_ordered = current_list_ordered;
    }

    output
}

/// Convert inline content to markdown
fn inline_content_to_markdown(content: &[InlineContent]) -> String {
    let mut output = String::new();

    for (idx, item) in content.iter().enumerate() {
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
            InlineContent::HardBreak => {
                let trailing_hard_break = content[idx..]
                    .iter()
                    .all(|c| matches!(c, InlineContent::HardBreak));
                let next_is_hard_break = content
                    .get(idx + 1)
                    .map(|c| matches!(c, InlineContent::HardBreak))
                    .unwrap_or(false);
                let prev_is_hard_break =
                    idx > 0 && matches!(content.get(idx - 1), Some(InlineContent::HardBreak));
                let part_of_hard_break_run = prev_is_hard_break || next_is_hard_break;

                if trailing_hard_break || part_of_hard_break_run {
                    output.push_str("<br>");
                    if trailing_hard_break && !next_is_hard_break {
                        output.push('\n');
                    }
                } else {
                    output.push_str("  \n");
                }
            }
        }
    }

    output
}

fn wrap_text_with_indent(text: &str, initial_indent: &str, subsequent_indent: &str) -> String {
    if text.is_empty() {
        return initial_indent.to_string();
    }

    if text.trim().is_empty() {
        return initial_indent.to_string();
    }

    if initial_indent.len() + text.len() <= MAX_LINE_WIDTH {
        if initial_indent.is_empty() {
            return text.to_string();
        }
        let mut line = initial_indent.to_string();
        line.push_str(text);
        return line;
    }

    let mut normalized = text.replace("<br>", &format!(" {} ", BR_TOKEN));
    normalized = normalized.replace('\n', &format!(" {} ", NEWLINE_TOKEN));

    let mut lines: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let mut is_first_line = true;

    for token in normalized.split_whitespace() {
        if token == BR_TOKEN {
            if !current_line.is_empty() {
                current_line.push_str("<br>");
                lines.push(std::mem::take(&mut current_line));
            } else if let Some(last) = lines.last_mut() {
                last.push_str("<br>");
            } else {
                lines.push("<br>".to_string());
            }
            is_first_line = false;
            continue;
        }

        if token == NEWLINE_TOKEN {
            lines.push(std::mem::take(&mut current_line));
            is_first_line = false;
            continue;
        }

        let word = token;
        loop {
            let indent_len = if is_first_line && current_line.is_empty() {
                initial_indent.len()
            } else {
                subsequent_indent.len()
            };
            let needed_len = if current_line.is_empty() {
                indent_len + word.len()
            } else {
                indent_len + current_line.len() + 1 + word.len()
            };

            if !current_line.is_empty() && needed_len > MAX_LINE_WIDTH {
                lines.push(std::mem::take(&mut current_line));
                is_first_line = false;
                continue;
            }

            if current_line.is_empty() && indent_len + word.len() > MAX_LINE_WIDTH {
                current_line.push_str(word);
            } else {
                if !current_line.is_empty() {
                    current_line.push(' ');
                }
                current_line.push_str(word);
            }

            break;
        }
    }

    if !current_line.is_empty() {
        lines.push(current_line);
    }

    if lines.is_empty() {
        return initial_indent.to_string();
    }

    let mut result = String::new();
    for (idx, line) in lines.into_iter().enumerate() {
        if idx > 0 {
            result.push('\n');
        }
        let indent = if idx == 0 {
            initial_indent
        } else {
            subsequent_indent
        };
        result.push_str(indent);
        result.push_str(&line);
    }

    result
}

fn list_item_indents(
    ordered: bool,
    number: Option<u64>,
    checkbox: Option<bool>,
) -> (String, String) {
    if let Some(checked) = checkbox {
        let marker = if checked { "[x] " } else { "[ ] " };
        let initial = format!("- {}", marker);
        let subsequent = " ".repeat(initial.len());
        return (initial, subsequent);
    }

    if ordered {
        let value = number.unwrap_or(1);
        let initial = format!("{}. ", value);
        let subsequent = " ".repeat(initial.len());
        return (initial, subsequent);
    }

    let initial = "- ".to_string();
    let subsequent = "  ".to_string();
    (initial, subsequent)
}

/// Convert AST to StructuredDocument
fn ast_to_structured(ast_doc: &ASTDocument) -> StructuredDocument {
    let mut doc = StructuredDocument::new();

    for child in &ast_doc.root.children {
        ast_node_to_blocks(child, &mut doc);
    }

    // Ensure at least one block exists
    if doc.is_empty() {
        doc.add_block(Block::paragraph());
    }

    doc
}

/// Convert an AST node to one or more blocks (handles List which contains multiple ListItems)
fn ast_node_to_blocks(node: &ASTNode, doc: &mut StructuredDocument) {
    match &node.node_type {
        NodeType::List { ordered, start } => {
            // Process each list item as a separate block
            for (idx, child) in node.children.iter().enumerate() {
                if let NodeType::ListItem { checkbox } = &child.node_type {
                    let number = if *ordered {
                        Some(start + idx as u64)
                    } else {
                        None
                    };

                    let mut block = Block::new(BlockType::ListItem {
                        ordered: *ordered,
                        number,
                        checkbox: *checkbox,
                    });
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
fn ast_node_to_block(node: &ASTNode, _doc: &mut StructuredDocument) -> Option<Block> {
    match &node.node_type {
        NodeType::Paragraph => {
            let mut block = Block::paragraph();
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        NodeType::Heading { level } => {
            let mut block = Block::heading(*level);
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        NodeType::CodeBlock { language, .. } => {
            let mut block = Block::new(BlockType::CodeBlock {
                language: language.clone(),
            });
            let text = node.flatten_text();
            block.content = vec![InlineContent::Text(TextRun::plain(text))];
            Some(block)
        }
        NodeType::BlockQuote => {
            let mut block = Block::new(BlockType::BlockQuote);
            block.content = ast_node_to_inline_content(node);
            Some(block)
        }
        NodeType::ListItem { checkbox } => {
            // Determine if parent is ordered or unordered
            // For now, assume unordered
            let mut block = Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: *checkbox,
            });
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
                if let Some(InlineContent::Text(existing)) = content.last_mut()
                    && existing.style == text_style
                {
                    existing.text.push_str(text);
                    continue;
                }
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
                if let Some(InlineContent::Text(existing)) = content.last_mut() {
                    existing.text.push(' ');
                } else {
                    content.push(InlineContent::Text(TextRun::plain(" ")));
                }
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

    for idx in 1..content.len() {
        if matches!(content[idx - 1], InlineContent::HardBreak)
            && let InlineContent::Text(run) = &mut content[idx]
            && run.text.starts_with(' ')
        {
            run.text.remove(0);
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
        doc.add_block(Block::paragraph().with_plain_text("Hello world"));

        let md = document_to_markdown(&doc);
        assert_eq!(md, "Hello world");
    }

    #[test]
    fn test_document_to_markdown_heading() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::heading(1).with_plain_text("Title"));

        let md = document_to_markdown(&doc);
        assert_eq!(md, "# Title");
    }

    #[test]
    fn test_document_to_markdown_list() {
        let mut doc = StructuredDocument::new();
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            })
            .with_plain_text("Item 1"),
        );
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            })
            .with_plain_text("Item 2"),
        );

        let md = document_to_markdown(&doc);
        assert_eq!(md, "- Item 1\n- Item 2");
    }

    #[test]
    fn test_document_to_markdown_ordered_list_spacing() {
        let mut doc = StructuredDocument::new();
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: true,
                number: Some(1),
                checkbox: None,
            })
            .with_plain_text("First"),
        );
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: true,
                number: Some(2),
                checkbox: None,
            })
            .with_plain_text("Second"),
        );

        let md = document_to_markdown(&doc);
        assert_eq!(md, "1. First\n2. Second");
    }

    #[test]
    fn test_document_to_markdown_checklist_spacing() {
        let mut doc = StructuredDocument::new();
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(false),
            })
            .with_plain_text("Todo"),
        );
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(true),
            })
            .with_plain_text("Done"),
        );

        let md = document_to_markdown(&doc);
        assert_eq!(md, "- [ ] Todo\n- [x] Done");
    }

    #[test]
    fn test_document_to_markdown_list_spacing_between_types() {
        let mut doc = StructuredDocument::new();
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            })
            .with_plain_text("Bullet item"),
        );
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: true,
                number: Some(1),
                checkbox: None,
            })
            .with_plain_text("Numbered item"),
        );
        doc.add_block(Block::paragraph().with_plain_text("After list"));

        let md = document_to_markdown(&doc);
        assert_eq!(md, "- Bullet item\n\n1. Numbered item\n\nAfter list");
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
    fn test_document_to_markdown_checklist() {
        let mut doc = StructuredDocument::new();
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(false),
            })
            .with_plain_text("Todo"),
        );
        doc.add_block(Block::paragraph().with_plain_text(""));
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(true),
            })
            .with_plain_text("Done"),
        );

        let md = document_to_markdown(&doc);
        assert_eq!(md, "- [ ] Todo\n\n\n\n- [x] Done");
    }

    #[test]
    fn test_markdown_export_wraps_paragraph_without_modifying_structure() {
        let mut doc = StructuredDocument::new();
        let long_text = (0..6)
            .map(|_| "Lorem ipsum dolor sit amet, consectetur adipiscing elit.")
            .collect::<Vec<_>>()
            .join(" ");
        doc.add_block(Block::paragraph().with_plain_text(long_text));

        let markdown = document_to_markdown(&doc);
        let lines: Vec<&str> = markdown.lines().collect();
        assert!(
            lines.len() > 1,
            "expected wrapped output for long paragraph, got: {}",
            markdown
        );

        for line in &lines {
            assert!(
                line.len() <= MAX_LINE_WIDTH,
                "line exceeded {} characters: {}",
                MAX_LINE_WIDTH,
                line
            );
        }

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(round_tripped.blocks(), doc.blocks());
    }

    #[test]
    fn test_markdown_export_wraps_list_items_with_round_trip() {
        let mut doc = StructuredDocument::new();
        let long_text = (0..6)
            .map(|_| "Aliquam fermentum sapien nec quam feugiat, vitae placerat mauris luctus.")
            .collect::<Vec<_>>()
            .join(" ");
        doc.add_block(
            Block::new(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            })
            .with_plain_text(long_text),
        );

        let markdown = document_to_markdown(&doc);
        let lines: Vec<&str> = markdown.lines().collect();
        assert!(
            lines.len() > 1,
            "expected wrapped output for long list item, got: {}",
            markdown
        );
        assert!(
            lines.iter().skip(1).all(|line| line.starts_with("  ")),
            "expected subsequent lines to be indented with spaces: {:?}",
            lines
        );
        for line in &lines {
            assert!(
                line.len() <= MAX_LINE_WIDTH,
                "line exceeded {} characters: {}",
                MAX_LINE_WIDTH,
                line
            );
        }

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(round_tripped.blocks(), doc.blocks());
    }

    #[test]
    fn test_markdown_export_wraps_blockquotes_with_round_trip() {
        let mut doc = StructuredDocument::new();
        let long_text = (0..5)
            .map(|_| "Praesent a orci sed lorem cursus tempor id ut lectus.")
            .collect::<Vec<_>>()
            .join(" ");
        doc.add_block(Block::new(BlockType::BlockQuote).with_plain_text(long_text));

        let markdown = document_to_markdown(&doc);
        let lines: Vec<&str> = markdown.lines().collect();
        assert!(
            lines.len() > 1,
            "expected wrapped output for blockquote, got: {}",
            markdown
        );
        assert!(
            lines.iter().all(|line| line.starts_with("> ")),
            "expected each blockquote line to start with '> ': {:?}",
            lines
        );
        for line in &lines {
            assert!(
                line.len() <= MAX_LINE_WIDTH,
                "line exceeded {} characters: {}",
                MAX_LINE_WIDTH,
                line
            );
        }

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(round_tripped.blocks(), doc.blocks());
    }

    #[test]
    fn test_markdown_export_wraps_blockquote_with_html_breaks() {
        let mut doc = StructuredDocument::new();
        let mut block = Block::new(BlockType::BlockQuote);
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Lorem ipsum dolor sit amet, consectetur adipiscing elit. Curabitur elementum augue ut erat laoreet, at tristique leo laoreet.")));
        block.content.push(InlineContent::HardBreak);
        block.content.push(InlineContent::HardBreak);
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Vestibulum ante ipsum primis in faucibus orci luctus et ultrices posuere cubilia curae; Integer euismod, justo in consequat fermentum, est mi sodales justo, at aliquam est mauris a lectus.")));
        doc.add_block(block);

        let markdown = document_to_markdown(&doc);
        let lines: Vec<&str> = markdown.lines().collect();
        assert!(
            lines.len() > 2,
            "expected wrapped blockquote with multiple lines, got: {}",
            markdown
        );
        assert!(
            lines.iter().all(|line| line.starts_with("> ")),
            "expected blockquote lines to keep '> ' prefix: {:?}",
            lines
        );
        assert!(
            lines.iter().all(|line| line.len() <= MAX_LINE_WIDTH),
            "line exceeded {} characters: {:?}",
            MAX_LINE_WIDTH,
            lines
        );
        assert!(
            markdown.contains("<br>"),
            "expected HTML breaks to be preserved: {}",
            markdown
        );

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(round_tripped.blocks(), doc.blocks());
    }

    #[test]
    fn test_markdown_to_document_checklist() {
        let md = "- [ ] Todo\n\n- [x] Done";
        let doc = markdown_to_document(md);
        assert_eq!(doc.block_count(), 2);
        match doc.blocks()[0].block_type {
            BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(state),
            } => assert!(!state),
            _ => panic!("expected checklist"),
        }
        match doc.blocks()[1].block_type {
            BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(state),
            } => assert!(state),
            _ => panic!("expected checklist"),
        }
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

    #[test]
    fn test_double_hard_break_round_trip() {
        let mut doc = StructuredDocument::new();
        let mut block = Block::paragraph();
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Line 1")));
        block.content.push(InlineContent::HardBreak);
        block.content.push(InlineContent::HardBreak);
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Line 2")));
        doc.add_block(block);

        let markdown = document_to_markdown(&doc);
        assert_eq!(markdown, "Line 1<br><br>Line 2");

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(
            round_tripped.block_count(),
            1,
            "blocks after round trip: {:?}",
            round_tripped.blocks()
        );
        let block = &round_tripped.blocks()[0];
        assert_eq!(block.content, doc.blocks()[0].content);
    }

    #[test]
    fn test_double_hard_break_round_trip_in_list_item() {
        let mut doc = StructuredDocument::new();
        let mut block = Block::new(BlockType::ListItem {
            ordered: false,
            number: None,
            checkbox: None,
        });
        block
            .content
            .push(InlineContent::Text(TextRun::plain("First line")));
        block.content.push(InlineContent::HardBreak);
        block.content.push(InlineContent::HardBreak);
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Second line")));
        doc.add_block(block);

        let markdown = document_to_markdown(&doc);
        assert_eq!(markdown, "- First line<br><br>Second line");

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(
            round_tripped.block_count(),
            1,
            "blocks after round trip: {:?}",
            round_tripped.blocks()
        );
        match &round_tripped.blocks()[0].block_type {
            BlockType::ListItem { .. } => {}
            other => panic!("expected list item, got {:?}", other),
        }
        assert_eq!(round_tripped.blocks()[0].content, doc.blocks()[0].content);
    }

    #[test]
    fn test_trailing_double_hard_break_round_trip() {
        let mut doc = StructuredDocument::new();
        let mut block = Block::paragraph();
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Line 1")));
        block.content.push(InlineContent::HardBreak);
        block.content.push(InlineContent::HardBreak);
        doc.add_block(block);

        let markdown = document_to_markdown(&doc);
        assert_eq!(markdown, "Line 1<br><br>\n");

        let round_tripped = markdown_to_document(&markdown);
        assert_eq!(round_tripped.block_count(), 1);
        assert_eq!(round_tripped.blocks()[0].content, doc.blocks()[0].content);
    }
}
