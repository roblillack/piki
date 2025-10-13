// Markdown Parser - converts pulldown-cmark events into our AST
use crate::markdown_ast::*;
use pulldown_cmark::{Event, Parser, Tag, TagEnd, CowStr, Options};

/// Parse markdown text into an AST
pub fn parse_markdown(text: &str) -> Document {
    let mut doc = Document::new();
    doc.source = text.to_string();

    // Parse using pulldown-cmark with offset tracking and HTML enabled
    let mut options = Options::empty();
    options.insert(Options::ENABLE_STRIKETHROUGH);
    options.insert(Options::ENABLE_TABLES);
    // Enable wikilink parsing ([[Page]]) via pulldown-cmark option
    options.insert(Options::ENABLE_WIKILINKS);
    let parser = Parser::new_ext(text, options).into_offset_iter();

    // Stack to track open container nodes - start with just the document
    let mut node_stack: Vec<ASTNode> = Vec::new();

    // Stack to track current text style (for nested emphasis/strong)
    let mut style_stack: Vec<TextStyle> = vec![TextStyle::default()];

    for (event, range) in parser {
        match event {
            Event::Start(tag) => {
                // Update style stack for inline formatting
                match &tag {
                    Tag::Emphasis => {
                        let mut new_style = style_stack.last().copied().unwrap_or_default();
                        new_style.italic = true;
                        style_stack.push(new_style);
                    }
                    Tag::Strong => {
                        let mut new_style = style_stack.last().copied().unwrap_or_default();
                        new_style.bold = true;
                        style_stack.push(new_style);
                    }
                    Tag::Strikethrough => {
                        let mut new_style = style_stack.last().copied().unwrap_or_default();
                        new_style.strikethrough = true;
                        style_stack.push(new_style);
                    }
                    _ => {}
                }

                let node = create_node_from_tag(&mut doc, tag, range.start, range.end);
                node_stack.push(node);
            }

            Event::End(tag_end) => {
                // Pop style stack for inline formatting
                match &tag_end {
                    TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => {
                        if style_stack.len() > 1 {
                            style_stack.pop();
                        }
                    }
                    _ => {}
                }

                // Pop the completed node and add to parent
                if !node_stack.is_empty() {
                    let completed_node = node_stack.pop().unwrap();

                    // Verify tag matches
                    if verify_tag_match(&completed_node.node_type, &tag_end) {
                        // Update end position with actual range end
                        let mut node = completed_node;
                        node.char_end = range.end;

                        // Add to parent (or root if stack is empty)
                        if let Some(parent) = node_stack.last_mut() {
                            parent.add_child(node);
                        } else {
                            // No parent - add directly to root
                            doc.root.add_child(node);
                        }
                    } else {
                        // Tag mismatch - put it back (shouldn't happen with valid markdown)
                        node_stack.push(completed_node);
                    }
                }
            }

            Event::Text(text_content) => {
                let current_style = style_stack.last().copied().unwrap_or_default();
                let node = create_text_node_with_style(&mut doc, text_content, range.start, range.end, current_style);
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                } else {
                    // No container - add directly to root (shouldn't normally happen)
                    doc.root.add_child(node);
                }
            }

            Event::Code(code_content) => {
                let node = ASTNode::new(
                    doc.next_id(),
                    NodeType::Code {
                        content: code_content.to_string(),
                    },
                    range.start,
                    range.end,
                );
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                } else {
                    doc.root.add_child(node);
                }
            }

            Event::SoftBreak => {
                let node = ASTNode::new(
                    doc.next_id(),
                    NodeType::SoftBreak,
                    range.start,
                    range.end,
                );
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                } else {
                    doc.root.add_child(node);
                }
            }

            Event::HardBreak => {
                let node = ASTNode::new(
                    doc.next_id(),
                    NodeType::HardBreak,
                    range.start,
                    range.end,
                );
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                } else {
                    doc.root.add_child(node);
                }
            }

            Event::Rule => {
                let node = ASTNode::new(
                    doc.next_id(),
                    NodeType::ThematicBreak,
                    range.start,
                    range.end,
                );
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                } else {
                    doc.root.add_child(node);
                }
            }

            Event::Html(html) | Event::InlineHtml(html) => {
                // Handle <u> and <mark> HTML tags for underline and highlight
                let html_str = html.to_string();
                if html_str == "<u>" || html_str.starts_with("<u ") {
                    // Start underline
                    let mut new_style = style_stack.last().copied().unwrap_or_default();
                    new_style.underline = true;
                    style_stack.push(new_style);
                } else if html_str == "</u>" {
                    // End underline
                    if style_stack.len() > 1 {
                        style_stack.pop();
                    }
                } else if html_str == "<mark>" || html_str.starts_with("<mark ") {
                    // Start highlight
                    let mut new_style = style_stack.last().copied().unwrap_or_default();
                    new_style.highlight = true;
                    style_stack.push(new_style);
                } else if html_str == "</mark>" {
                    // End highlight
                    if style_stack.len() > 1 {
                        style_stack.pop();
                    }
                }
            }

            _ => {
                // Ignore other events for now (FootnoteReference, TaskListMarker, etc.)
            }
        }
    }

    // Update root's end position
    doc.root.char_end = text.len();

    // Note: wikilinks are parsed directly by pulldown-cmark when enabled above

    doc
}

/// Create an AST node from a pulldown-cmark Tag
fn create_node_from_tag(doc: &mut Document, tag: Tag, start: usize, end: usize) -> ASTNode {
    let node_type = match tag {
        Tag::Paragraph => NodeType::Paragraph,

        Tag::Heading { level, .. } => NodeType::Heading {
            level: level as u8,
        },

        Tag::BlockQuote(_) => NodeType::BlockQuote,

        Tag::CodeBlock(kind) => {
            let (language, fence_info) = match kind {
                pulldown_cmark::CodeBlockKind::Indented => (None, String::new()),
                pulldown_cmark::CodeBlockKind::Fenced(info) => {
                    let info_str = info.to_string();
                    let lang = info_str.split_whitespace().next().map(String::from);
                    (lang, info_str)
                }
            };

            NodeType::CodeBlock {
                language,
                fence_info,
            }
        }

        Tag::List(start_number) => NodeType::List {
            ordered: start_number.is_some(),
            start: start_number.unwrap_or(1),
        },

        Tag::Item => NodeType::ListItem,

        Tag::Link { dest_url, title, .. } => NodeType::Link {
            destination: dest_url.to_string(),
            title: if title.is_empty() {
                None
            } else {
                Some(title.to_string())
            },
        },

        Tag::Image { dest_url, title, .. } => NodeType::Image {
            destination: dest_url.to_string(),
            title: if title.is_empty() {
                None
            } else {
                Some(title.to_string())
            },
        },

        Tag::Table(_) => NodeType::Table,
        Tag::TableHead => NodeType::TableHead,
        Tag::TableRow => NodeType::TableRow,
        Tag::TableCell => NodeType::TableCell { alignment: None },

        Tag::Emphasis => {
            // Create a wrapper node for emphasis (we'll handle this via TextStyle)
            // For now, treat as inline container
            NodeType::Paragraph // Placeholder - should be refined
        }

        Tag::Strong => {
            // Create a wrapper node for strong (we'll handle this via TextStyle)
            NodeType::Paragraph // Placeholder - should be refined
        }

        Tag::Strikethrough => {
            NodeType::Paragraph // Placeholder - should be refined
        }

        _ => NodeType::Paragraph, // Default fallback
    };

    ASTNode::new(doc.next_id(), node_type, start, end)
}

/// Create a text node with proper styling
fn create_text_node(doc: &mut Document, content: CowStr, start: usize, end: usize) -> ASTNode {
    create_text_node_with_style(doc, content, start, end, TextStyle::default())
}

/// Create a text node with explicit style
fn create_text_node_with_style(doc: &mut Document, content: CowStr, start: usize, end: usize, style: TextStyle) -> ASTNode {
    ASTNode::new(
        doc.next_id(),
        NodeType::Text {
            content: content.to_string(),
            style,
        },
        start,
        end,
    )
}

/// Verify that a tag end matches the node type
fn verify_tag_match(node_type: &NodeType, tag_end: &TagEnd) -> bool {
    match (node_type, tag_end) {
        (NodeType::Paragraph, TagEnd::Paragraph) => true,
        (NodeType::Heading { .. }, TagEnd::Heading(_)) => true,
        (NodeType::BlockQuote, TagEnd::BlockQuote(_)) => true,
        (NodeType::CodeBlock { .. }, TagEnd::CodeBlock) => true,
        (NodeType::List { .. }, TagEnd::List(_)) => true,
        (NodeType::ListItem, TagEnd::Item) => true,
        (NodeType::Link { .. }, TagEnd::Link) => true,
        (NodeType::Image { .. }, TagEnd::Image) => true,
        (NodeType::Table, TagEnd::Table) => true,
        (NodeType::TableHead, TagEnd::TableHead) => true,
        (NodeType::TableRow, TagEnd::TableRow) => true,
        (NodeType::TableCell { .. }, TagEnd::TableCell) => true,
        // For emphasis/strong/etc, we use Paragraph as placeholder, so match those too
        (NodeType::Paragraph, TagEnd::Strong) => true,
        (NodeType::Paragraph, TagEnd::Emphasis) => true,
        (NodeType::Paragraph, TagEnd::Strikethrough) => true,
        _ => false,
    }
}


/// Parse a single block (paragraph) - useful for incremental parsing
pub fn parse_block(text: &str, start_pos: usize) -> Option<ASTNode> {
    let mut doc = Document::new();
    let parser = Parser::new(text).into_offset_iter();

    let mut root: Option<ASTNode> = None;
    let mut node_stack: Vec<ASTNode> = Vec::new();

    for (event, range) in parser {
        match event {
            Event::Start(tag) => {
                let node = create_node_from_tag(&mut doc, tag, start_pos + range.start, start_pos + range.end);
                node_stack.push(node);
            }

            Event::End(_tag_end) => {
                if node_stack.len() >= 1 {
                    let completed_node = node_stack.pop().unwrap();
                    let mut completed_node = completed_node;
                    completed_node.char_end = start_pos + range.end;

                    if node_stack.is_empty() {
                        root = Some(completed_node);
                        break; // Only parse one block
                    } else if let Some(parent) = node_stack.last_mut() {
                        parent.add_child(completed_node);
                    }
                }
            }

            Event::Text(text_content) => {
                let node = create_text_node(&mut doc, text_content, start_pos + range.start, start_pos + range.end);
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                }
            }

            Event::Code(code_content) => {
                let node = ASTNode::new(
                    doc.next_id(),
                    NodeType::Code {
                        content: code_content.to_string(),
                    },
                    start_pos + range.start,
                    start_pos + range.end,
                );
                if let Some(parent) = node_stack.last_mut() {
                    parent.add_child(node);
                }
            }

            _ => {}
        }
    }

    root
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_paragraph() {
        let text = "This is a paragraph.";
        let doc = parse_markdown(text);

        assert_eq!(doc.root.children.len(), 1);
        assert!(matches!(doc.root.children[0].node_type, NodeType::Paragraph));
    }

    #[test]
    fn test_parse_heading() {
        let text = "# Heading 1\n\nSome text.";
        let doc = parse_markdown(text);

        assert!(doc.root.children.len() >= 1);
        assert!(matches!(
            doc.root.children[0].node_type,
            NodeType::Heading { level: 1 }
        ));
    }

    #[test]
    fn test_parse_link() {
        let text = "This is a [link](target.md).";
        let doc = parse_markdown(text);

        println!("{}", doc);

        // The paragraph should contain text and link nodes
        assert_eq!(doc.root.children.len(), 1);
        let para = &doc.root.children[0];
        assert!(matches!(para.node_type, NodeType::Paragraph));

        // Find the link node
        let has_link = para.children.iter().any(|child| {
            matches!(child.node_type, NodeType::Link { .. })
        });
        assert!(has_link);
    }

    #[test]
    fn test_parse_code_block() {
        let text = "```rust\nfn main() {}\n```";
        let doc = parse_markdown(text);

        assert_eq!(doc.root.children.len(), 1);
        match &doc.root.children[0].node_type {
            NodeType::CodeBlock { language, .. } => {
                assert_eq!(language.as_deref(), Some("rust"));
            }
            _ => panic!("Expected code block"),
        }
    }

    #[test]
    fn test_parse_list() {
        let text = "- Item 1\n- Item 2\n- Item 3";
        let doc = parse_markdown(text);

        assert_eq!(doc.root.children.len(), 1);
        match &doc.root.children[0].node_type {
            NodeType::List { ordered, .. } => {
                assert!(!ordered);
                assert_eq!(doc.root.children[0].children.len(), 3);
            }
            _ => panic!("Expected list"),
        }
    }

    #[test]
    fn test_parse_wikilink() {
        let text = "This has a [[WikiPage]] inside.";
        let doc = parse_markdown(text);

        assert_eq!(doc.root.children.len(), 1);
        let para = &doc.root.children[0];
        assert!(matches!(para.node_type, NodeType::Paragraph));

        // Find a Link child (wikilink parsed as standard link)
        let has_link = para.children.iter().any(|child| {
            matches!(child.node_type, NodeType::Link { .. })
        });
        assert!(has_link);
    }

    #[test]
    fn test_text_content() {
        let text = "Hello **world**!";
        let doc = parse_markdown(text);

        let full_text = doc.root.flatten_text();
        assert!(full_text.contains("Hello"));
        assert!(full_text.contains("world"));
    }

    #[test]
    fn test_position_tracking() {
        let text = "First paragraph.\n\nSecond paragraph.";
        let doc = parse_markdown(text);

        // Check that positions are tracked
        assert!(doc.root.char_end > 0);

        // First paragraph should start at 0
        if let Some(first) = doc.root.children.first() {
            assert_eq!(first.char_start, 0);
        }
    }
}
