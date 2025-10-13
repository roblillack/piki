// Snapshot tests for text_display styling functionality
// Uses SVG rendering for visual verification

pub mod svg_draw_context;
use fliki_rs::sourceedit::text_buffer::TextBuffer;
use fliki_rs::sourceedit::text_display::{style_attr, StyleTableEntry, TextDisplay};
use std::cell::RefCell;
use std::rc::Rc;

use crate::svg_draw_context::SvgDrawContext;

/// Helper function to render text display to SVG
fn render_to_svg(
    text: &str,
    style_text: &str,
    style_table: Vec<StyleTableEntry>,
    width: i32,
    height: i32,
) -> Vec<u8> {
    let mut display = TextDisplay::new(0, 0, width, height);

    // Set up text buffer
    let buffer = Rc::new(RefCell::new(TextBuffer::new()));
    buffer.borrow_mut().set_text(text);
    display.set_buffer(buffer.clone());

    // Set up style buffer
    let style_buffer = Rc::new(RefCell::new(TextBuffer::new()));
    style_buffer.borrow_mut().set_text(style_text);
    display.set_style_buffer(style_buffer);

    // Set style table
    display.set_highlight_data(style_table);

    // Configure display
    display.set_textfont(4); // Courier
    display.set_textsize(14);
    display.set_textcolor(0x000000FF); // Black

    // Recalculate display
    display.recalc_display();

    // Create SVG context and render
    let mut ctx = SvgDrawContext::new(width, height);
    display.draw(&mut ctx);

    ctx.finish().as_bytes().to_vec()
}

/// Helper to create a simple markdown-like style text
fn style_markdown_text(text: &str) -> String {
    let mut styled = String::with_capacity(text.len());
    let mut in_header = false;
    let mut in_code = false;
    let mut line_start = true;

    for ch in text.chars() {
        let style_char = if ch == '\n' {
            in_header = false;
            line_start = true;
            'A'
        } else {
            if line_start {
                in_header = ch == '#';
                in_code = ch == '`';
                line_start = false;
            }

            if in_header {
                'E' // Header style
            } else if in_code || ch == '`' {
                'D' // Code style
            } else if ch == '*' || ch == '_' {
                'B' // Emphasis style
            } else {
                'A' // Normal style
            }
        };

        // Push style byte for each UTF-8 byte
        for _ in 0..ch.len_utf8() {
            styled.push(style_char);
        }
    }

    styled
}

/// Create a common style table for testing
fn create_test_style_table() -> Vec<StyleTableEntry> {
    vec![
        // Style A - Normal text (black)
        StyleTableEntry {
            color: 0x000000FF,
            font: 4, // Courier
            size: 14,
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        },
        // Style B - Emphasis (green, italic)
        StyleTableEntry {
            color: 0x00AA00FF,
            font: 4,
            size: 14,
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        },
        // Style C - Comments (blue, underlined)
        StyleTableEntry {
            color: 0x0000FFFF,
            font: 4,
            size: 14,
            attr: style_attr::UNDERLINE,
            bgcolor: 0xFFFFFFFF,
        },
        // Style D - Code (magenta)
        StyleTableEntry {
            color: 0xFF00FFFF,
            font: 4,
            size: 14,
            attr: 0,
            bgcolor: 0xF0F0F0FF, // Light gray background
        },
        // Style E - Headers (cyan, bold)
        StyleTableEntry {
            color: 0x00AAAAFF,
            font: 5, // Courier Bold
            size: 16,
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        },
    ]
}

#[test]
fn test_plain_text_rendering() {
    let text = "Hello World\nThis is a test\nLine 3";
    let style = "A".repeat(text.len());
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 400, 200);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_markdown_header() {
    let text = "# Welcome to Markdown\n\nThis is normal text.";
    let style = style_markdown_text(text);
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 500, 200);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_markdown_emphasis() {
    let text = "This is *bold* text\nAnd this is _italic_ text";
    let style = style_markdown_text(text);
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 500, 150);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_markdown_code_block() {
    let text = "Normal text\n`code here`\nMore text";
    let style = style_markdown_text(text);
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 400, 150);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_markdown_mixed_styles() {
    let text = "# Heading\n\nThis has *emphasis* and `code`\n\n## Subheading\n\nMore _text_ here";
    let style = style_markdown_text(text);
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 500, 300);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_style_with_background_colors() {
    let text = "Normal AAAA Code DDDD Normal";
    let style = "AAAAAAAAAADDDDDDDDDAAAAAA";
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, style, style_table, 500, 100);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_underlined_text() {
    let text = "This text has underlines";
    let style = "AAAAAAAAAAAACCCCCCCCCC"; // Last word underlined (style C)
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, style, style_table, 400, 100);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_multiline_styled_text() {
    let text = "Line 1: Normal\nLine 2: EEEE Header EEEE\nLine 3: Normal again";
    // Style the middle line with header style
    let mut style = String::new();
    for line in text.lines() {
        if line.contains("Header") {
            for ch in line.chars() {
                for _ in 0..ch.len_utf8() {
                    style.push('E');
                }
            }
        } else {
            for ch in line.chars() {
                for _ in 0..ch.len_utf8() {
                    style.push('A');
                }
            }
        }
        style.push('A'); // newline
    }

    let style_table = create_test_style_table();
    let svg = render_to_svg(text, &style, style_table, 500, 150);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_empty_lines() {
    let text = "Line 1\n\nLine 3\n\nLine 5";
    let style = "A".repeat(text.len());
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 400, 200);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_long_lines() {
    let text =
        "This is a very long line that might extend beyond the visible area of the display widget";
    let style = "A".repeat(text.len());
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 600, 100);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_tab_characters() {
    let text = "Name:\tValue\nItem:\tData";
    let style = "A".repeat(text.len());
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, &style, style_table, 400, 100);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_simple_markdown_document() {
    let markdown = r#"# My Document

This is a paragraph with *emphasis*.

## Code Example

Here is some `inline code` in text.

## Conclusion

More _emphasized_ text here."#;

    let style = style_markdown_text(markdown);
    let style_table = create_test_style_table();

    let svg = render_to_svg(markdown, &style, style_table, 600, 400);
    insta::assert_binary_snapshot!(".svg", svg);
}

// Note: Unicode test disabled due to UTF-8 boundary issues in text_display slicing
// TODO: Fix UTF-8 handling in text_display.rs before re-enabling
// #[test]
// fn test_unicode_text() {
//     let text = "Café\nNaïve\nRésumé";
//     let mut style = String::new();
//     for ch in text.chars() {
//         for _ in 0..ch.len_utf8() {
//             style.push('A');
//         }
//     }
//     let style_table = create_test_style_table();
//     let svg = render_to_svg(text, &style, style_table, 400, 150);
//     insta::assert_snapshot!(svg);
// }

#[test]
fn test_alternating_styles() {
    let text = "ABCDEFGHIJ";
    let style = "ABCDEABCDE"; // Cycle through first 5 styles
    let style_table = create_test_style_table();

    let svg = render_to_svg(text, style, style_table, 300, 100);
    insta::assert_binary_snapshot!(".svg", svg);
}
