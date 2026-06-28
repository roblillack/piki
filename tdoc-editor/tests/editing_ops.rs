//! Behavior tests for the shared editor's mutation operations.
//!
//! Ported in spirit from Pure's editor test coverage: each test drives a public
//! `StructuredEditor` operation and asserts the result via either the
//! marker-agnostic `current_block_type()` query or a markdown round-trip. This
//! hardens the shared core that both Piki (FLTK) and Pure (Ratatui) depend on.

use tdoc_editor::richtext::markdown_converter::document_to_markdown;
use tdoc_editor::{BlockType, DocumentPosition, StructuredEditor};

fn editor_with(markdown: &str) -> StructuredEditor {
    let mut e = StructuredEditor::default();
    e.load_markdown(markdown);
    e
}

fn md(e: &StructuredEditor) -> String {
    document_to_markdown(e.tdoc())
}

// ----- text insertion / deletion -------------------------------------------------

#[test]
fn insert_text_inserts_at_cursor() {
    let mut e = editor_with("ab\n");
    e.set_cursor(DocumentPosition::new(0, 1));
    e.insert_text("X").unwrap();
    assert!(md(&e).contains("aXb"), "{}", md(&e));
}

#[test]
fn insert_newline_splits_paragraph() {
    let mut e = editor_with("ab\n");
    e.set_cursor(DocumentPosition::new(0, 1));
    e.insert_newline().unwrap();
    assert_eq!(e.tdoc().paragraphs.len(), 2, "{}", md(&e));
}

#[test]
fn delete_backward_at_start_merges_paragraphs() {
    let mut e = editor_with("a\n\nb\n");
    assert_eq!(e.tdoc().paragraphs.len(), 2);
    // Move to the start of the second paragraph and backspace.
    e.move_cursor_down();
    e.delete_backward().unwrap();
    assert_eq!(e.tdoc().paragraphs.len(), 1, "{}", md(&e));
    assert!(md(&e).contains("ab"), "{}", md(&e));
}

// ----- block type changes --------------------------------------------------------

#[test]
fn toggle_heading_makes_heading() {
    let mut e = editor_with("text\n");
    e.toggle_heading().unwrap();
    assert!(
        matches!(e.current_block_type(), BlockType::Heading { level: 1 }),
        "{:?}",
        e.current_block_type()
    );
}

#[test]
fn toggle_unordered_list() {
    let mut e = editor_with("item\n");
    e.toggle_list().unwrap();
    assert!(
        matches!(
            e.current_block_type(),
            BlockType::ListItem { ordered: false, .. }
        ),
        "{:?}",
        e.current_block_type()
    );
}

#[test]
fn toggle_ordered_list() {
    let mut e = editor_with("item\n");
    e.toggle_ordered_list().unwrap();
    assert!(
        matches!(
            e.current_block_type(),
            BlockType::ListItem { ordered: true, .. }
        ),
        "{:?}",
        e.current_block_type()
    );
}

#[test]
fn toggle_checklist_then_check() {
    let mut e = editor_with("task\n");
    e.toggle_checklist().unwrap();
    assert!(
        matches!(
            e.current_block_type(),
            BlockType::ListItem {
                checkbox: Some(false),
                ..
            }
        ),
        "{:?}",
        e.current_block_type()
    );
    e.toggle_current_checkmark().unwrap();
    assert!(
        matches!(
            e.current_block_type(),
            BlockType::ListItem {
                checkbox: Some(true),
                ..
            }
        ),
        "{:?}",
        e.current_block_type()
    );
}

#[test]
fn indent_then_outdent_round_trips() {
    let mut e = editor_with("- one\n- two\n");
    let before = md(&e);
    e.move_cursor_down(); // into the second item
    e.indent_list_item().unwrap();
    assert_ne!(md(&e), before, "indent should change structure");
    e.outdent_list_item().unwrap();
    assert_eq!(md(&e), before, "outdent should restore the original");
}

// ----- inline styles & links -----------------------------------------------------

#[test]
fn toggle_bold_wraps_selection() {
    let mut e = editor_with("hello\n");
    e.select_all();
    e.toggle_bold().unwrap();
    assert!(md(&e).contains("**"), "{}", md(&e));
}

#[test]
fn link_replaces_selection() {
    let mut e = editor_with("click\n");
    e.select_all();
    e.replace_selection_with_link("https://example.com", "click")
        .unwrap();
    assert!(
        md(&e).contains("[click](https://example.com)"),
        "{}",
        md(&e)
    );
}

// ----- clipboard / selection -----------------------------------------------------

#[test]
fn get_selection_text_returns_selected_text() {
    let mut e = editor_with("hello\n");
    e.select_all();
    assert_eq!(e.get_selection_text(), "hello");
}

#[test]
fn selection_document_preserves_heading_level() {
    // Regression for the ported structure-preserving clipboard: copying a heading
    // must keep it a heading, not degrade it to body text.
    let mut e = editor_with("# Title\n\nbody\n");
    e.select_all();
    let doc = e.get_selection_document().expect("non-empty selection");
    let out = document_to_markdown(&doc);
    assert!(out.contains("# Title"), "heading not preserved:\n{out}");
}

// ----- undo / redo ---------------------------------------------------------------

#[test]
fn undo_redo_round_trips_an_edit() {
    let mut e = editor_with("hello\n");
    e.move_cursor_to_line_end();
    e.insert_text("X").unwrap();
    assert!(md(&e).contains("helloX"));

    assert!(e.undo());
    assert!(
        md(&e).contains("hello") && !md(&e).contains("helloX"),
        "{}",
        md(&e)
    );

    assert!(e.redo());
    assert!(md(&e).contains("helloX"), "{}", md(&e));
}
