use crate::content::{ContentLoader, ContentProvider};
use crate::fltk_draw_context::FltkDrawContext;
use crate::fltk_structured_rich_display::FltkStructuredRichDisplay;
use crate::markdown_converter::document_to_markdown;
use crate::note_ui::NoteUI;
use fltk::{app, enums::Color, prelude::*, window};
use rutle::editor::Editor;
use rutle::renderer::SearchMatch;
use rutle::structured_document::BlockType;
use rutle::tree_path::{DocumentPosition, PathSegment, TreePath};
use std::any::Any;

/// Vertical breathing room, in pixels, kept above a heading when scrolling to a
/// section so it does not sit flush against the top edge of the viewport.
const ANCHOR_TOP_MARGIN: i32 = 12;

/// NoteUI adapter for rutle's `Renderer` + FLTK Group wrapper
pub struct StructuredRichUI(pub FltkStructuredRichDisplay);

impl StructuredRichUI {
    pub fn new(x: i32, y: i32, w: i32, h: i32, edit_mode: bool) -> Self {
        Self(FltkStructuredRichDisplay::new(x, y, w, h, edit_mode))
    }

    pub fn has_selection(&self) -> bool {
        self.0.display.borrow().editor().selection().is_some()
    }

    /// Cut the current selection to the system clipboard (HTML + Markdown).
    /// Returns `true` if there was a selection that was cut.
    pub fn cut_selection(&mut self) -> bool {
        let doc = self.0.display.borrow().editor().get_selection_document();
        let Some(doc) = doc else {
            return false;
        };
        crate::clipboard::copy_structured_to_system(&doc);
        {
            let mut disp = self.0.display.borrow_mut();
            let _ = disp.editor_mut().delete_selection();
        }
        self.0.notify_change();
        true
    }

    /// Copy the current selection to the system clipboard (HTML + Markdown).
    /// Returns `true` if there was a selection that was copied.
    pub fn copy_selection(&self) -> bool {
        let doc = self.0.display.borrow().editor().get_selection_document();
        match doc {
            Some(doc) => {
                crate::clipboard::copy_structured_to_system(&doc);
                true
            }
            None => false,
        }
    }

    pub fn paste_from_clipboard(&mut self) {
        let group = self.0.group.clone();
        app::paste(&group);
    }

    pub fn undo(&mut self) -> bool {
        let changed = {
            let mut disp = self.0.display.borrow_mut();
            disp.editor_mut().undo()
        };
        if changed {
            self.0.notify_change();
            self.0.emit_paragraph_state();
        }
        changed
    }

    pub fn redo(&mut self) -> bool {
        let changed = {
            let mut disp = self.0.display.borrow_mut();
            disp.editor_mut().redo()
        };
        if changed {
            self.0.notify_change();
            self.0.emit_paragraph_state();
        }
        changed
    }

    pub fn clear_formatting(&mut self) -> bool {
        self.apply_edit(|editor| editor.clear_formatting())
    }

    pub fn set_block_type(&mut self, block_type: BlockType) -> bool {
        self.apply_edit(move |editor| editor.set_block_type(block_type))
    }

    pub fn toggle_quote(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_quote())
    }

    pub fn toggle_code_block(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_code_block())
    }

    pub fn toggle_list(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_list())
    }

    pub fn toggle_checklist(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_checklist())
    }

    pub fn toggle_ordered_list(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_ordered_list())
    }

    pub fn toggle_bold(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_bold())
    }

    pub fn toggle_italic(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_italic())
    }

    pub fn toggle_code(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_code())
    }

    pub fn toggle_strikethrough(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_strikethrough())
    }

    pub fn toggle_underline(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_underline())
    }

    pub fn toggle_highlight(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_highlight())
    }

    pub fn current_block_type(&self) -> Option<BlockType> {
        let disp = self.0.display.borrow();
        Some(disp.editor().current_block_type())
    }

    /// The anchor slug of the heading the caret is currently inside, or `None`
    /// when the caret is not in a heading. Duplicate headings are disambiguated
    /// exactly as [`Self::scroll_to_anchor`] resolves them, so the slug links
    /// back to *this* heading and not an earlier namesake.
    pub fn current_heading_anchor(&self) -> Option<String> {
        let disp = self.0.display.borrow();
        let cursor = disp.editor().cursor();
        // A heading is always a top-level leaf, so its caret path starts with a
        // `Paragraph(i)` segment that indexes `Document.paragraphs`.
        let para = match cursor.path.segments().first() {
            Some(PathSegment::Paragraph(i)) => *i,
            _ => return None,
        };
        heading_anchor_map(disp.editor().document())
            .into_iter()
            .find(|(idx, _)| *idx == para)
            .map(|(_, anchor)| anchor)
    }

    /// Scroll so the heading whose anchor slug equals `anchor` sits near the top
    /// of the viewport. Returns `false` if no heading matches.
    pub fn scroll_to_anchor(&mut self, anchor: &str) -> bool {
        let target = {
            let disp = self.0.display.borrow();
            heading_anchor_map(disp.editor().document())
                .into_iter()
                .find(|(_, a)| a == anchor)
                .map(|(idx, _)| idx)
        };
        match target {
            Some(idx) => self.scroll_to_block(idx),
            None => false,
        }
    }

    /// Scroll so top-level block `block_index` sits near the top of the viewport.
    ///
    /// The renderer exposes no public block→pixel mapping, so this moves the
    /// caret into the target block and reuses the caret→pixel bridge
    /// (`cursor_content_y`) after a layout pass. Returns `false` if the index is
    /// out of range.
    pub fn scroll_to_block(&mut self, block_index: usize) -> bool {
        let mut ctx = FltkDrawContext::new(true, true);
        let mut disp = self.0.display.borrow_mut();
        if block_index >= disp.editor().document().paragraphs.len() {
            return false;
        }
        disp.editor_mut()
            .set_cursor(DocumentPosition::new(block_index, 0));
        // Lay out with real font metrics so `cursor_content_y` is populated.
        disp.ensure_cursor_visible(&mut ctx);
        if let Some((content_y, _line_h)) = disp.cursor_content_y(&mut ctx) {
            let max_scroll = (disp.content_height() - disp.h()).max(0);
            let target = (content_y - ANCHOR_TOP_MARGIN).max(0).min(max_scroll);
            disp.set_scroll(target);
        }
        drop(disp);
        self.0.group.redraw();
        true
    }

    /// Set horizontal padding (for write room mode)
    pub fn set_horizontal_padding(&mut self, padding: i32) {
        self.0.display.borrow_mut().set_horizontal_padding(padding);
        self.0.group.redraw();
    }

    /// Get current horizontal padding
    pub fn horizontal_padding(&self) -> i32 {
        self.0.display.borrow().horizontal_padding()
    }

    /// Whether reveal-codes mode is active (rutle's inline-style tags, e.g.
    /// `[Bold>`…`<Bold]`, shown inline).
    pub fn reveal_codes(&self) -> bool {
        self.0.display.borrow().reveal_codes()
    }

    /// Toggle reveal-codes mode, returning the new state. This is purely a view
    /// change — not a document edit — so it deliberately skips the change/undo
    /// machinery; the tags appearing inline are their own feedback.
    pub fn toggle_reveal_codes(&mut self) -> bool {
        let new_state = !self.0.display.borrow().reveal_codes();
        self.0.display.borrow_mut().set_reveal_codes(new_state);
        self.0.group.redraw();
        new_state
    }

    /// Resize the editor widget
    pub fn resize(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.0.group.resize(x, y, w, h);
        self.0.group.redraw();
    }

    /// Get current height
    pub fn height(&self) -> i32 {
        self.0.group.height()
    }

    /// Get current width
    pub fn width(&self) -> i32 {
        self.0.group.width()
    }

    /// Get current x position
    pub fn x(&self) -> i32 {
        self.0.group.x()
    }

    /// Get current y position
    pub fn y(&self) -> i32 {
        self.0.group.y()
    }

    // ==================== Search Methods ====================

    /// Perform a case-insensitive search for the given term
    pub fn search(&mut self, term: &str) -> usize {
        self.0.display.borrow_mut().search(term)
    }

    /// Clear the search state
    pub fn clear_search(&mut self) {
        self.0.display.borrow_mut().clear_search();
    }

    /// Get all search matches
    pub fn search_matches(&self) -> Vec<SearchMatch> {
        self.0.display.borrow().search_matches().to_vec()
    }

    /// Get the current match index
    pub fn search_current_index(&self) -> Option<usize> {
        self.0.display.borrow().search_current_index()
    }

    /// Move to the next match
    pub fn next_match(&mut self) -> bool {
        self.0.display.borrow_mut().next_match()
    }

    /// Move to the previous match
    pub fn prev_match(&mut self) -> bool {
        self.0.display.borrow_mut().prev_match()
    }

    /// Scroll to make the current match visible
    pub fn scroll_to_current_match(&mut self) {
        let mut ctx = FltkDrawContext::new(true, true);
        self.0
            .display
            .borrow_mut()
            .scroll_to_current_match(&mut ctx);
        self.0.group.redraw();
    }

    /// Focus the editor widget
    pub fn take_focus(&mut self) {
        let _ = self.0.group.take_focus();
    }

    fn apply_edit<F>(&mut self, edit: F) -> bool
    where
        F: FnOnce(&mut Editor) -> rutle::editor::EditResult,
    {
        let result = {
            let mut disp = self.0.display.borrow_mut();
            let editor = disp.editor_mut();
            edit(editor)
        };
        if result.is_ok() {
            self.0.notify_change();
            self.0.emit_paragraph_state();
            true
        } else {
            false
        }
    }
}

/// Map each top-level heading to its unique anchor slug, in document order.
///
/// Uses rutle's tree helpers so link resolution stays in lockstep with how the
/// editor classifies blocks, and delegates slug/duplicate handling to
/// [`crate::section_link`] so generation and resolution share one algorithm.
fn heading_anchor_map(doc: &tdoc::Document) -> Vec<(usize, String)> {
    let mut indices = Vec::new();
    let mut texts = Vec::new();
    for i in 0..doc.paragraphs.len() {
        let path = TreePath::root(i);
        if matches!(
            rutle::tree_walk::effective_block_type(doc, &path),
            BlockType::Heading { .. }
        ) {
            texts.push(rutle::tree_walk::leaf_plain_text(doc, &path));
            indices.push(i);
        }
    }
    indices
        .into_iter()
        .zip(crate::section_link::heading_anchors(&texts))
        .collect()
}

impl ContentProvider for StructuredRichUI {
    fn get_content(&self) -> String {
        let disp = self.0.display.borrow();
        document_to_markdown(disp.editor().document())
    }
}

impl ContentLoader for StructuredRichUI {
    fn set_content_from_markdown(&mut self, markdown: &str) {
        let mut disp = self.0.display.borrow_mut();
        // Loading a different note starts a fresh undo history (set_document resets it).
        let doc = crate::markdown_converter::markdown_to_document(markdown);
        disp.editor_mut().set_document(doc);
        disp.set_scroll(0);
        drop(disp);
        self.0.emit_paragraph_state();
    }
}

impl NoteUI for StructuredRichUI {
    fn on_change(&mut self, f: Box<dyn FnMut() + 'static>) {
        self.0.set_change_callback(Some(f));
    }

    fn set_readonly(&mut self, readonly: bool) {
        self.0.display.borrow_mut().set_cursor_visible(!readonly);
    }

    fn is_readonly(&self) -> bool {
        !self.0.display.borrow().cursor_visible()
    }

    fn scroll_pos(&self) -> i32 {
        self.0.display.borrow().scroll_offset()
    }

    fn set_scroll_pos(&mut self, pos: i32) {
        self.0.display.borrow_mut().set_scroll(pos.max(0));
    }

    fn set_bg_color(&mut self, color: Color) {
        self.0.group.set_color(color);
    }

    fn set_resizable(&self, wind: &mut window::Window) {
        wind.resizable(&self.0.group);
    }

    fn on_link_click(&mut self, f: Box<dyn Fn(String) + 'static>) {
        self.0.set_link_callback(Some(f));
    }

    fn tick(&mut self, ms_since_start: u64) {
        self.0.tick(ms_since_start);
    }

    fn on_link_hover(&mut self, f: Box<dyn Fn(Option<String>) + 'static>) {
        self.0.set_link_hover_callback(Some(f));
    }

    fn on_paragraph_style_change(&mut self, f: Box<dyn FnMut(BlockType) + 'static>) {
        self.0.set_paragraph_callback(Some(f));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn hide(&mut self) {
        self.0.group.hide();
    }

    fn take_focus(&mut self) {
        let _ = self.0.group.take_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heading_anchor_map_slugs_and_dedup() {
        // Top-level headings (in tdoc `#`/`##`/`###` are all top-level blocks),
        // interleaved with a paragraph and with duplicate heading texts.
        let md = "# Overview\n\nsome text\n\n## Details\n\n## Details\n\n# Overview\n";
        let doc = crate::markdown_converter::markdown_to_document(md);

        let map = heading_anchor_map(&doc);
        let slugs: Vec<&str> = map.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(slugs, ["overview", "details", "details-1", "overview-1"]);

        // Every returned index really points at a heading block.
        for (idx, _) in &map {
            assert!(matches!(
                rutle::tree_walk::effective_block_type(&doc, &TreePath::root(*idx)),
                BlockType::Heading { .. }
            ));
        }
    }
}
