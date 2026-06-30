//! Layout snapshot tests — monospace (IBM Plex Mono) backend.
//!
//! Same scenarios as the proportional suite (`layout_snapshots.rs`), driven
//! through [`common`] under [`FontMode::Monospace`], which snaps `text_width`
//! to an integer character-cell grid. This mirrors the metric regime Pure's
//! ratatui (terminal cell) backend imposes on the shared layout engine, so the
//! two suites together guard layout under both proportional and cell geometry.

mod common;

use common::FontMode;

const MODE: FontMode = FontMode::Monospace;

#[test]
fn richtext_basic_render() {
    insta::assert_binary_snapshot!(".svg", common::basic_render(MODE));
}

#[test]
fn complex_rendering() {
    insta::assert_binary_snapshot!(".svg", common::complex_rendering(MODE));
}

#[test]
fn selection_single_block() {
    insta::assert_binary_snapshot!(".svg", common::selection_single_block(MODE));
}

#[test]
fn cursor_positioning_middle_of_line() {
    insta::assert_binary_snapshot!(".svg", common::cursor_positioning_middle_of_line(MODE));
}

#[test]
fn table_render() {
    insta::assert_binary_snapshot!(".svg", common::table_render(MODE));
}

#[test]
fn table_force_wrap_long_tokens() {
    insta::assert_binary_snapshot!(".svg", common::table_force_wrap_long_tokens(MODE));
}

#[test]
fn selection_across_blocks() {
    insta::assert_binary_snapshot!(".svg", common::selection_across_blocks(MODE));
}
