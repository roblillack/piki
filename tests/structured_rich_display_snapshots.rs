// Snapshot tests for StructuredRichDisplay (richtext) using SVG renderer

pub mod svg_draw_context;

use fliki_rs::richtext::markdown_converter::markdown_to_document;
use fliki_rs::richtext::structured_document::DocumentPosition;
use fliki_rs::richtext::structured_rich_display::StructuredRichDisplay;
use fliki_rs::sourceedit::text_display::{StyleTableEntry, style_attr};

use crate::svg_draw_context::SvgDrawContext;

// Create a style table compatible with StructuredRichDisplay expectations
fn create_rich_style_table() -> Vec<StyleTableEntry> {
    const DEFAULT_FONT_SIZE: u8 = 14;
    const BG: u32 = 0xFFFFF5FF; // Match widget background
    const HIGHLIGHT_COLOR: u32 = 0xFFFF00FF;

    // Base styles 0..10
    let mut styles: Vec<StyleTableEntry> = vec![
        // 0 Plain
        StyleTableEntry {
            color: 0x000000FF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 1 Bold
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 2 Italic
        StyleTableEntry {
            color: 0x000000FF,
            font: 2,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 3 Bold+Italic
        StyleTableEntry {
            color: 0x000000FF,
            font: 3,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 4 Code
        StyleTableEntry {
            color: 0x0064C8FF,
            font: 4,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 5 Link
        StyleTableEntry {
            color: 0x0000FFFF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 6 Header1
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE + 6,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 7 Header2
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE + 4,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 8 Header3
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE + 2,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 9 Quote
        StyleTableEntry {
            color: 0x640000FF,
            font: 10,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: BG,
        },
        // 10 Link hover
        StyleTableEntry {
            color: 0x0000FFFF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: 0xD3D3D3FF,
        },
    ];

    // Decorated variants 11.. for underline/strike/highlight combinations on base 0..3
    let base_fonts = [0, 1, 2, 3];
    for base in 0..4 {
        for decoration in 1..8 {
            let underline = (decoration & 1) != 0;
            let strikethrough = (decoration & 2) != 0;
            let highlight = (decoration & 4) != 0;

            let mut attr = style_attr::BGCOLOR;
            if underline {
                attr |= style_attr::UNDERLINE;
            }
            if strikethrough {
                attr |= style_attr::STRIKE_THROUGH;
            }
            let bgcolor = if highlight { HIGHLIGHT_COLOR } else { BG };
            styles.push(StyleTableEntry {
                color: 0x000000FF,
                font: base_fonts[base],
                size: DEFAULT_FONT_SIZE,
                attr,
                bgcolor,
            });
        }
    }

    styles
}

fn render_markdown_to_svg(markdown: &str, width: i32, height: i32) -> Vec<u8> {
    let mut display = StructuredRichDisplay::new(0, 0, width, height);
    display.set_style_table(create_rich_style_table());

    // Load document into editor
    let doc = markdown_to_document(markdown);
    {
        let editor = display.editor_mut();
        *editor.document_mut() = doc;
    }

    // Render to SVG
    let mut ctx = SvgDrawContext::new(width, height);
    display.draw(&mut ctx);
    ctx.finish().as_bytes().to_vec()
}

#[test]
fn richtext_basic_render() {
    let md = "# Heading 1\n\nThis is a paragraph with a [link](https://example.com).";
    let svg = render_markdown_to_svg(md, 600, 220);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn test_complex_rendering() {
    let text = std::fs::read_to_string("./tests/data/example.md").unwrap();

    let svg = render_markdown_to_svg(&text, 400, 700);
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn selection_single_block() {
    let md = "This is selection test."; // Single paragraph
    let mut display = StructuredRichDisplay::new(0, 0, 500, 140);
    display.set_style_table(create_rich_style_table());

    // Load doc
    {
        let doc = markdown_to_document(md);
        let editor = display.editor_mut();
        *editor.document_mut() = doc;
        // Select the word "selection"
        let start = md.find("selection").unwrap();
        let end = start + "selection".len();
        editor.set_selection(
            DocumentPosition::new(0, start),
            DocumentPosition::new(0, end),
        );
    }

    // Render
    let mut ctx = SvgDrawContext::new(500, 140);
    display.draw(&mut ctx);
    let svg = ctx.finish().as_bytes().to_vec();
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn cursor_positioning_middle_of_line() {
    let md = "Paragraph with a cursor.";
    let mut display = StructuredRichDisplay::new(0, 0, 520, 140);
    display.set_style_table(create_rich_style_table());

    {
        let doc = markdown_to_document(md);
        let editor = display.editor_mut();
        *editor.document_mut() = doc;
        // Place cursor after word "with"
        let pos = md.find("with").unwrap() + "with".len();
        editor.set_cursor(DocumentPosition::new(0, pos));
    }

    let mut ctx = SvgDrawContext::new(520, 140);
    // SvgDrawContext defaults to has_focus = true; cursor should render
    display.draw(&mut ctx);
    let svg = ctx.finish().as_bytes().to_vec();
    insta::assert_binary_snapshot!(".svg", svg);
}

#[test]
fn selection_across_blocks() {
    let md = "# Title\n\nParagraph one with content.\n\nParagraph two continues here.";
    let mut display = StructuredRichDisplay::new(0, 0, 640, 260);
    display.set_style_table(create_rich_style_table());

    {
        let doc = markdown_to_document(md);
        let editor = display.editor_mut();
        *editor.document_mut() = doc;

        // Blocks: 0=Heading, 1=Para1, 2=Para2
        let para1 = "Paragraph one with content.";
        let para2 = "Paragraph two continues here.";
        let start = para1.find("with").unwrap() + 2; // mid-word for variety
        let end = para2.find("continues").unwrap() + "continues".len() - 2;
        editor.set_selection(
            DocumentPosition::new(1, start),
            DocumentPosition::new(2, end),
        );
    }

    let mut ctx = SvgDrawContext::new(640, 260);
    display.draw(&mut ctx);
    let svg = ctx.finish().as_bytes().to_vec();
    insta::assert_binary_snapshot!(".svg", svg);
}
