// Snapshot tests for StructuredRichDisplay (richtext) using SVG renderer

pub mod svg_draw_context;

use fliki_rs::draw_context::{FontStyle, FontType};
use fliki_rs::richtext::markdown_converter::markdown_to_document;
use fliki_rs::richtext::structured_document::DocumentPosition;
use fliki_rs::richtext::structured_rich_display::StructuredRichDisplay;
use fliki_rs::sourceedit::text_display::{StyleTableEntry, style_attr};

use crate::svg_draw_context::SvgDrawContext;

fn render_markdown_to_svg(markdown: &str, width: i32, height: i32) -> Vec<u8> {
    let mut display = StructuredRichDisplay::new(0, 0, width, height);

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
