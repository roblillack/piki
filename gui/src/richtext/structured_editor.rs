// Structured Editor
//
// Editing operations over a `tdoc::Document` (the authoritative document tree). Cursor
// and selection are tree-path positions (`DocumentPosition { path, offset }`). Intra-leaf
// text editing reuses the `Block` inline helpers via a transient round-trip
// (spans -> InlineContent runs -> edit -> spans). Tree navigation goes through `tree_walk`.
//
// PHASE 1 SCOPE: storage, positions, cursor/selection, movement, intra-leaf insert/delete,
// undo/redo, load. Structural ops (block-spanning splits/merges, list/quote/style toggles,
// links, moves, paste of multi-paragraph content) are stubbed and land in Phase 2/3 — see
// the `// TODO(phase2)` markers.

use super::inline_convert::{inline_to_spans, spans_to_inline};
use super::markdown_converter::markdown_to_document;
use super::structured_document::{
    Block, BlockType, InlineContent, TextRun, TextStyle, normalize_plain_text,
};
use super::tree_edit;
use super::tree_path::PathSegment;
use super::tree_path::{DocumentPosition, TreePath};
use super::tree_walk::{self, LeafInfo};
use std::time::{Duration, Instant};
use tdoc::Document;
use tdoc::inline::Span;
use tdoc::paragraph::{ChecklistItem, Paragraph};

/// Result of an editing operation
pub type EditResult = Result<(), EditError>;

/// Errors that can occur during editing
#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    InvalidPosition,
    InvalidBlockIndex,
    EmptyDocument,
    ConversionFailed(String),
}

/// Maximum number of undo steps retained on the undo stack.
const MAX_UNDO_STEPS: usize = 200;

/// Idle gap after which the next typing/deletion starts a fresh undo step.
const UNDO_COALESCE_IDLE: Duration = Duration::from_secs(2);

/// Classifies an edit so consecutive edits of the same kind can be coalesced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoKind {
    Typing,
    Deleting,
    Other,
}

/// An immutable snapshot of the editable state, used for undo/redo.
#[derive(Debug, Clone)]
struct EditorSnapshot {
    tdoc: Document,
    cursor: DocumentPosition,
    selection: Option<(DocumentPosition, DocumentPosition)>,
}

/// The structured editor with cursor state.
pub struct StructuredEditor {
    tdoc: Document,
    cursor: DocumentPosition,
    selection: Option<(DocumentPosition, DocumentPosition)>,
    paragraph_cb: Option<Box<dyn FnMut(BlockType) + 'static>>,
    undo_stack: Vec<EditorSnapshot>,
    redo_stack: Vec<EditorSnapshot>,
    undo_baseline: EditorSnapshot,
    last_edit_kind: Option<UndoKind>,
    last_edit_time: Option<Instant>,
}

impl StructuredEditor {
    /// Create a new editor with an empty document.
    pub fn new() -> Self {
        Self::with_tdoc(Document::new())
    }

    /// Create an editor wrapping an existing tdoc document.
    pub fn with_tdoc(tdoc: Document) -> Self {
        let baseline = EditorSnapshot {
            tdoc: tdoc.clone(),
            cursor: DocumentPosition::start(),
            selection: None,
        };
        let mut editor = StructuredEditor {
            tdoc,
            cursor: DocumentPosition::start(),
            selection: None,
            paragraph_cb: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_baseline: baseline,
            last_edit_kind: None,
            last_edit_time: None,
        };
        editor.normalize_cursor();
        editor
    }

    /// The authoritative document tree.
    pub fn tdoc(&self) -> &Document {
        &self.tdoc
    }

    /// Mutable access to the authoritative document tree. Callers that mutate it should
    /// follow up with [`StructuredEditor::after_external_change`].
    pub fn tdoc_mut(&mut self) -> &mut Document {
        &mut self.tdoc
    }

    /// Replace the whole document (e.g. when loading a page). Resets the caret.
    pub fn set_tdoc(&mut self, tdoc: Document) {
        self.tdoc = tdoc;
        self.cursor = DocumentPosition::start();
        self.selection = None;
        self.normalize_cursor();
        self.trigger_paragraph_change();
    }

    /// Load markdown as the document, clearing undo history.
    pub fn load_markdown(&mut self, markdown: &str) {
        self.set_tdoc(markdown_to_document(markdown));
        self.reset_undo_history();
    }

    /// Re-clamp the caret after the document was mutated through `tdoc_mut`.
    pub fn after_external_change(&mut self) {
        self.normalize_cursor();
        self.trigger_paragraph_change();
    }

    pub fn set_paragraph_change_callback(
        &mut self,
        cb: Option<Box<dyn FnMut(BlockType) + 'static>>,
    ) {
        self.paragraph_cb = cb;
        self.trigger_paragraph_change();
    }

    fn trigger_paragraph_change(&mut self) {
        if self.paragraph_cb.is_some() {
            let block_type = self.block_type_at(&self.cursor.path);
            if let Some(cb) = self.paragraph_cb.as_mut() {
                cb(block_type);
            }
        }
    }

    // ----- Leaf helpers --------------------------------------------------------------

    fn leaves(&self) -> Vec<LeafInfo> {
        tree_walk::enumerate_leaves(&self.tdoc)
    }

    fn leaf_paths(&self) -> Vec<TreePath> {
        tree_walk::leaf_paths(&self.tdoc)
    }

    fn leaf_index(&self, path: &TreePath) -> Option<usize> {
        self.leaf_paths().iter().position(|p| p == path)
    }

    fn leaf_count(&self) -> usize {
        tree_walk::leaf_count(&self.tdoc)
    }

    fn leaf_text_len(&self, path: &TreePath) -> usize {
        tree_walk::leaf_text_len(&self.tdoc, path)
    }

    fn leaf_plain_text(&self, path: &TreePath) -> String {
        tree_walk::leaf_plain_text(&self.tdoc, path)
    }

    /// The presentation block type at the cursor (for menus / paragraph-style UI).
    pub fn current_block_type(&self) -> BlockType {
        self.block_type_at(&self.cursor.path)
    }

    /// The presentation block type at a path (`Paragraph` when the path is invalid).
    fn block_type_at(&self, path: &TreePath) -> BlockType {
        self.leaves()
            .iter()
            .find(|l| &l.path == path)
            .map(|info| tree_walk::leaf_block_type(&self.tdoc, info))
            .unwrap_or(BlockType::Paragraph)
    }

    fn is_table_leaf(&self, path: &TreePath) -> bool {
        matches!(self.block_type_at(path), BlockType::Table { .. })
    }

    /// Edit the inline runs of the leaf at `path` in place. The closure receives the
    /// leaf's content as flat runs; the result is written back as spans. No-op (and the
    /// closure still runs on a throwaway copy) for tables / invalid paths.
    fn edit_leaf<R>(&mut self, path: &TreePath, f: impl FnOnce(&mut Vec<InlineContent>) -> R) -> R {
        let mut content = tree_walk::leaf_inline(&self.tdoc, path);
        let result = f(&mut content);
        tree_walk::set_leaf_inline(&mut self.tdoc, path, &content);
        result
    }

    // ----- Undo / redo ---------------------------------------------------------------

    fn current_snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            tdoc: self.tdoc.clone(),
            cursor: self.cursor.clone(),
            selection: self.selection.clone(),
        }
    }

    pub fn commit_undo_step(&mut self, kind: UndoKind, now: Instant) {
        let within_idle_window = self
            .last_edit_time
            .is_some_and(|t| now.saturating_duration_since(t) < UNDO_COALESCE_IDLE);
        self.last_edit_time = Some(now);
        self.record_step(kind, within_idle_window);
    }

    fn record_step(&mut self, kind: UndoKind, within_idle_window: bool) {
        if self.undo_baseline.tdoc == self.tdoc {
            self.undo_baseline.cursor = self.cursor.clone();
            self.undo_baseline.selection = self.selection.clone();
            return;
        }

        let coalesce = kind != UndoKind::Other
            && self.last_edit_kind == Some(kind)
            && within_idle_window
            && !self.undo_stack.is_empty();

        if !coalesce {
            self.undo_stack.push(self.undo_baseline.clone());
            if self.undo_stack.len() > MAX_UNDO_STEPS {
                self.undo_stack.remove(0);
            }
        }

        self.redo_stack.clear();
        self.undo_baseline = self.current_snapshot();
        self.last_edit_kind = Some(kind);
    }

    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty() || self.undo_baseline.tdoc != self.tdoc
    }

    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    pub fn undo(&mut self) -> bool {
        self.flush_pending_edit();
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        self.redo_stack.push(self.undo_baseline.clone());
        self.undo_baseline = previous.clone();
        self.last_edit_kind = None;
        self.restore_snapshot(previous);
        true
    }

    pub fn redo(&mut self) -> bool {
        self.flush_pending_edit();
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        self.undo_stack.push(self.undo_baseline.clone());
        self.undo_baseline = next.clone();
        self.last_edit_kind = None;
        self.restore_snapshot(next);
        true
    }

    pub fn reset_undo_history(&mut self) {
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.last_edit_kind = None;
        self.last_edit_time = None;
        self.undo_baseline = self.current_snapshot();
    }

    fn break_undo_coalescing(&mut self) {
        self.last_edit_kind = None;
    }

    fn flush_pending_edit(&mut self) {
        self.record_step(UndoKind::Other, false);
    }

    fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.tdoc = snapshot.tdoc;
        self.cursor = snapshot.cursor;
        self.selection = snapshot.selection;
        self.normalize_cursor();
        self.trigger_paragraph_change();
    }

    fn normalize_cursor(&mut self) {
        self.cursor = tree_walk::clamp_position(&self.tdoc, &self.cursor);
    }

    // ----- Cursor & selection --------------------------------------------------------

    pub fn cursor(&self) -> DocumentPosition {
        self.cursor.clone()
    }

    pub fn set_cursor(&mut self, pos: DocumentPosition) {
        self.break_undo_coalescing();
        self.cursor = tree_walk::clamp_position(&self.tdoc, &pos);
        self.selection = None;
        self.trigger_paragraph_change();
    }

    pub fn selection(&self) -> Option<(DocumentPosition, DocumentPosition)> {
        self.selection.clone()
    }

    pub fn set_selection(&mut self, start: DocumentPosition, end: DocumentPosition) {
        self.break_undo_coalescing();
        let start = tree_walk::clamp_position(&self.tdoc, &start);
        let end = tree_walk::clamp_position_forward(&self.tdoc, &end);
        self.selection = Some((start, end));
    }

    pub fn clear_selection(&mut self) {
        self.break_undo_coalescing();
        self.selection = None;
    }

    pub fn select_all(&mut self) {
        self.break_undo_coalescing();
        let paths = self.leaf_paths();
        let (Some(first), Some(last)) = (paths.first().cloned(), paths.last().cloned()) else {
            self.selection = None;
            return;
        };
        let start = DocumentPosition::at(first, 0);
        let end = DocumentPosition::at(last.clone(), self.leaf_text_len(&last));
        self.selection = Some((start, end.clone()));
        self.cursor = end;
        self.normalize_cursor();
    }

    pub fn extend_selection_to(&mut self, end: DocumentPosition) {
        self.break_undo_coalescing();
        let end = tree_walk::clamp_position(&self.tdoc, &end);
        if let Some((start, _)) = self.selection.clone() {
            self.selection = Some((start, end.clone()));
        } else {
            self.selection = Some((self.cursor.clone(), end.clone()));
        }
        self.cursor = end;
        self.normalize_cursor();
    }

    pub fn select_word_at(&mut self, pos: DocumentPosition) {
        let pos = tree_walk::clamp_position(&self.tdoc, &pos);
        let text = self.leaf_plain_text(&pos.path);
        if text.is_empty() || pos.offset >= text.len() {
            return;
        }

        let mut start = pos.offset;
        let mut end = pos.offset;

        while start > 0 {
            let ch = text[..start].chars().next_back().unwrap();
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                break;
            }
            start = text[..start]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }

        for (_, ch) in text[end..].char_indices() {
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                break;
            }
            end = text[..end]
                .chars()
                .next()
                .map(|c| end + c.len_utf8())
                .unwrap_or(end);
        }

        if start == end {
            end = text[end..]
                .chars()
                .next()
                .map(|c| end + c.len_utf8())
                .unwrap_or(end);
        }

        let start_pos = DocumentPosition::at(pos.path.clone(), start);
        let end_pos = DocumentPosition::at(pos.path, end);
        self.set_selection(start_pos, end_pos.clone());
        self.cursor = tree_walk::clamp_position_forward(&self.tdoc, &end_pos);
    }

    pub fn select_line_at(&mut self, pos: DocumentPosition) {
        let pos = tree_walk::clamp_position(&self.tdoc, &pos);
        let len = self.leaf_text_len(&pos.path);
        let start_pos = DocumentPosition::at(pos.path.clone(), 0);
        let end_pos = DocumentPosition::at(pos.path, len);
        self.set_selection(start_pos, end_pos.clone());
        self.cursor = tree_walk::clamp_position_forward(&self.tdoc, &end_pos);
    }

    // ----- Insertion -----------------------------------------------------------------

    pub fn insert_text(&mut self, text: &str) -> EditResult {
        if self.leaf_count() == 0 {
            self.tdoc
                .add_paragraph(Paragraph::new_text().with_content(inline_to_spans(&[
                    InlineContent::Text(TextRun::plain(text)),
                ])));
            self.cursor = DocumentPosition::at(TreePath::root(0), text.len());
            return Ok(());
        }

        // Replace any selection first (intra-leaf for now).
        if self.selection.is_some() {
            self.delete_selection()?;
        }

        let path = self.cursor.path.clone();
        // Tables are read-only.
        if self.is_table_leaf(&path) {
            return Ok(());
        }

        let offset = self.cursor.offset;
        self.edit_leaf(&path, |content| insert_into_content(content, offset, text));
        self.cursor.offset = offset + text.len();
        Ok(())
    }

    pub fn insert_hard_break(&mut self) -> EditResult {
        if self.leaf_count() == 0 {
            self.tdoc.add_paragraph(
                Paragraph::new_text().with_content(inline_to_spans(&[InlineContent::HardBreak])),
            );
            self.cursor = DocumentPosition::at(TreePath::root(0), 1);
            return Ok(());
        }

        if self.selection.is_some() {
            self.delete_selection()?;
        }

        let path = self.cursor.path.clone();
        if self.is_table_leaf(&path) {
            return Ok(());
        }
        let offset = self.cursor.offset;
        self.edit_leaf(&path, |content| {
            let mut block = Block::paragraph();
            block.content = std::mem::take(content);
            let right = block.split_content_at(offset);
            block.content.push(InlineContent::HardBreak);
            block.content.extend(right);
            *content = block.content;
        });
        self.cursor.offset = offset + 1;
        Ok(())
    }

    /// Split the current leaf into a new sibling. TODO(phase2): full tree-aware split that
    /// continues lists / quotes and outdents empty items. Phase 1 handles only the
    /// top-level paragraph case; otherwise it is a no-op.
    pub fn insert_newline(&mut self) -> EditResult {
        if self.leaf_count() == 0 {
            self.tdoc.add_paragraph(Paragraph::new_text());
            self.tdoc.add_paragraph(Paragraph::new_text());
            self.cursor = DocumentPosition::at(TreePath::root(1), 0);
            return Ok(());
        }
        if self.selection.is_some() {
            self.delete_selection()?;
        }
        let path = self.cursor.path.clone();
        if self.is_table_leaf(&path) {
            return Ok(());
        }
        // TODO(phase3): Enter on an empty list/checklist item outdents / exits the list.
        let offset = self.cursor.offset;
        if let Some(new_path) = tree_edit::split_leaf(&mut self.tdoc, &path, offset) {
            self.cursor = DocumentPosition::at(new_path, 0);
        }
        self.normalize_cursor();
        Ok(())
    }

    // ----- Deletion ------------------------------------------------------------------

    pub fn delete_backward(&mut self) -> EditResult {
        if self.leaf_count() == 0 {
            return Err(EditError::EmptyDocument);
        }
        if self.selection.is_some() {
            return self.delete_selection();
        }

        let path = self.cursor.path.clone();
        let offset = self.cursor.offset;

        if self.is_table_leaf(&path) {
            return Ok(());
        }
        if offset == 0 {
            // TODO(phase3): outdent a nested list item before merging.
            if let Some((pos_path, pos_off)) = tree_edit::merge_with_previous(&mut self.tdoc, &path)
            {
                self.cursor = DocumentPosition::at(pos_path, pos_off);
                self.normalize_cursor();
            }
            return Ok(());
        }

        let prev = tree_walk::previous_grapheme_position(
            &self.tdoc,
            &DocumentPosition::at(path.clone(), offset),
        )
        .offset;
        if prev < offset {
            self.delete_intra_leaf_range(&path, prev, offset);
            self.cursor.offset = prev;
        }
        self.normalize_cursor();
        Ok(())
    }

    pub fn delete_backward_bytes(&mut self, byte_count: usize) -> Result<bool, EditError> {
        if byte_count == 0 {
            return Ok(false);
        }
        if self.leaf_count() == 0 {
            return Err(EditError::EmptyDocument);
        }
        self.normalize_cursor();

        let path = self.cursor.path.clone();
        let end = self.cursor.offset;
        let text = self.leaf_plain_text(&path);
        // Walk back grapheme by grapheme within this leaf until we've covered byte_count.
        let mut start = end;
        let mut remaining = byte_count;
        while start > 0 && remaining > 0 {
            let prev = tree_walk::previous_grapheme_position(
                &self.tdoc,
                &DocumentPosition::at(path.clone(), start),
            )
            .offset;
            if prev >= start {
                break;
            }
            let removed = start - prev;
            start = prev;
            remaining = remaining.saturating_sub(removed);
        }
        let _ = text;
        if start == end {
            return Ok(false);
        }
        self.set_selection(
            DocumentPosition::at(path.clone(), start),
            DocumentPosition::at(path, end),
        );
        self.delete_selection()?;
        Ok(true)
    }

    pub fn delete_forward(&mut self) -> EditResult {
        if self.leaf_count() == 0 {
            return Err(EditError::EmptyDocument);
        }
        if self.selection.is_some() {
            return self.delete_selection();
        }

        let path = self.cursor.path.clone();
        let offset = self.cursor.offset;
        let len = self.leaf_text_len(&path);

        if self.is_table_leaf(&path) {
            return Ok(());
        }
        if offset >= len {
            // Merge the following leaf into this one (cursor stays at the join point).
            if let Some(next_path) = tree_walk::next_leaf_path(&self.tdoc, &path)
                && !self.is_table_leaf(&next_path)
            {
                tree_edit::merge_with_previous(&mut self.tdoc, &next_path);
                self.cursor = DocumentPosition::at(path, len);
                self.normalize_cursor();
            }
            return Ok(());
        }

        let next = tree_walk::next_grapheme_position(
            &self.tdoc,
            &DocumentPosition::at(path.clone(), offset),
        )
        .offset;
        if next > offset {
            self.edit_leaf(&path, |content| {
                let mut block = Block::paragraph();
                block.content = std::mem::take(content);
                block.delete_text_range(offset, next);
                *content = block.content;
            });
        }
        self.normalize_cursor();
        Ok(())
    }

    pub fn delete_word_backward(&mut self) -> EditResult {
        if self.leaf_count() == 0 {
            return Err(EditError::EmptyDocument);
        }
        if self.selection.is_some() {
            return self.delete_selection();
        }
        if self.is_table_leaf(&self.cursor.path.clone()) {
            return self.delete_backward();
        }
        let from = self.cursor.clone();
        let to = self.word_left_position(&from);
        if to == from || to.path != from.path {
            // Cross-leaf word delete is handled as a plain backspace in Phase 1.
            return self.delete_backward();
        }
        self.delete_intra_leaf_range(&to.path, to.offset, from.offset);
        self.cursor = to;
        self.normalize_cursor();
        self.selection = None;
        Ok(())
    }

    pub fn delete_word_forward(&mut self) -> EditResult {
        if self.leaf_count() == 0 {
            return Err(EditError::EmptyDocument);
        }
        if self.selection.is_some() {
            return self.delete_selection();
        }
        if self.is_table_leaf(&self.cursor.path.clone()) {
            return self.delete_forward();
        }
        let from = self.cursor.clone();
        let to = self.word_right_position(&from);
        if to == from || to.path != from.path {
            return self.delete_forward();
        }
        self.delete_intra_leaf_range(&from.path, from.offset, to.offset);
        self.normalize_cursor();
        self.selection = None;
        Ok(())
    }

    pub fn delete_selection(&mut self) -> EditResult {
        let Some((a, b)) = self.selection.clone() else {
            return Ok(());
        };
        let (start, end) = if a <= b { (a, b) } else { (b, a) };

        if start.path == end.path {
            self.delete_intra_leaf_range(&start.path, start.offset, end.offset);
            self.cursor = start.clone();
        } else if let (Some(s_idx), Some(e_idx)) =
            (self.leaf_index(&start.path), self.leaf_index(&end.path))
        {
            // Truncate the start leaf to its head, append the end leaf's tail, then remove
            // every leaf from start+1 through end (re-querying the next leaf each time so
            // container pruning can't invalidate stale paths).
            let end_len = self.leaf_text_len(&end.path);
            let end_runs = tree_walk::leaf_inline(&self.tdoc, &end.path);
            let end_tail = extract_inline_range(&end_runs, end.offset, end_len);
            let start_off = start.offset;
            self.edit_leaf(&start.path, |content| {
                let mut block = Block::paragraph();
                block.content = std::mem::take(content);
                let len = block.text_len();
                block.delete_text_range(start_off, len);
                block.content.extend(end_tail);
                block.normalize_content();
                *content = block.content;
            });
            for _ in 0..(e_idx.saturating_sub(s_idx)) {
                if let Some(next) = tree_walk::next_leaf_path(&self.tdoc, &start.path) {
                    tree_edit::remove_node_at(&mut self.tdoc, &next);
                } else {
                    break;
                }
            }
            self.cursor = start.clone();
        } else {
            self.cursor = start.clone();
        }
        self.selection = None;
        self.normalize_cursor();
        Ok(())
    }

    fn delete_intra_leaf_range(&mut self, path: &TreePath, start: usize, end: usize) {
        if start >= end {
            return;
        }
        self.edit_leaf(path, |content| {
            let mut block = Block::paragraph();
            block.content = std::mem::take(content);
            block.delete_text_range(start, end);
            *content = block.content;
        });
    }

    // ----- Movement ------------------------------------------------------------------

    fn prev_leaf(&self, path: &TreePath) -> Option<TreePath> {
        tree_walk::prev_leaf_path(&self.tdoc, path)
    }

    fn next_leaf(&self, path: &TreePath) -> Option<TreePath> {
        tree_walk::next_leaf_path(&self.tdoc, path)
    }

    pub fn move_cursor_left(&mut self) {
        self.break_undo_coalescing();
        let new = self.position_left(&self.cursor.clone());
        self.cursor = new;
        self.normalize_cursor();
        self.selection = None;
    }

    pub fn move_cursor_right(&mut self) {
        self.break_undo_coalescing();
        let new = self.position_right(&self.cursor.clone());
        self.cursor = new;
        self.normalize_cursor();
        self.selection = None;
    }

    fn position_left(&self, pos: &DocumentPosition) -> DocumentPosition {
        if pos.offset > 0 {
            tree_walk::previous_grapheme_position(&self.tdoc, pos)
        } else if let Some(prev) = self.prev_leaf(&pos.path) {
            let len = self.leaf_text_len(&prev);
            DocumentPosition::at(prev, len)
        } else {
            pos.clone()
        }
    }

    fn position_right(&self, pos: &DocumentPosition) -> DocumentPosition {
        let len = self.leaf_text_len(&pos.path);
        if pos.offset < len {
            tree_walk::next_grapheme_position(&self.tdoc, pos)
        } else if let Some(next) = self.next_leaf(&pos.path) {
            DocumentPosition::at(next, 0)
        } else {
            pos.clone()
        }
    }

    pub fn move_cursor_up(&mut self) {
        self.break_undo_coalescing();
        if let Some(prev) = self.prev_leaf(&self.cursor.path) {
            let len = self.leaf_text_len(&prev);
            self.cursor = DocumentPosition::at(prev, self.cursor.offset.min(len));
            self.normalize_cursor();
        }
        self.selection = None;
    }

    pub fn move_cursor_down(&mut self) {
        self.break_undo_coalescing();
        if let Some(next) = self.next_leaf(&self.cursor.path) {
            let len = self.leaf_text_len(&next);
            self.cursor = DocumentPosition::at(next, self.cursor.offset.min(len));
            self.normalize_cursor();
        }
        self.selection = None;
    }

    pub fn move_cursor_to_line_start(&mut self) {
        self.break_undo_coalescing();
        self.cursor.offset = 0;
        self.normalize_cursor();
        self.selection = None;
    }

    pub fn move_cursor_to_line_end(&mut self) {
        self.break_undo_coalescing();
        self.cursor.offset = self.leaf_text_len(&self.cursor.path.clone());
        self.normalize_cursor();
        self.selection = None;
    }

    pub fn move_word_right(&mut self) {
        self.break_undo_coalescing();
        self.cursor = self.word_right_position(&self.cursor.clone());
        self.selection = None;
    }

    pub fn move_word_left(&mut self) {
        self.break_undo_coalescing();
        self.cursor = self.word_left_position(&self.cursor.clone());
        self.selection = None;
    }

    pub fn move_word_right_extend(&mut self) {
        let new = self.word_right_position(&self.cursor.clone());
        if new != self.cursor {
            self.extend_selection_to(new);
        }
    }

    pub fn move_word_left_extend(&mut self) {
        let new = self.word_left_position(&self.cursor.clone());
        if new != self.cursor {
            self.extend_selection_to(new);
        }
    }

    fn word_right_position(&self, pos: &DocumentPosition) -> DocumentPosition {
        let text = self.leaf_plain_text(&pos.path);
        let mut i = pos.offset.min(text.len());
        if i >= text.len() {
            if let Some(next) = self.next_leaf(&pos.path) {
                return DocumentPosition::at(next, 0);
            }
            return tree_walk::clamp_position(&self.tdoc, pos);
        }
        while i < text.len() {
            let ch = text[i..].chars().next().unwrap();
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                i += ch.len_utf8();
            } else {
                break;
            }
        }
        while i < text.len() {
            let ch = text[i..].chars().next().unwrap();
            if !(ch.is_whitespace() || ch.is_ascii_punctuation()) {
                i += ch.len_utf8();
            } else {
                break;
            }
        }
        tree_walk::clamp_position_forward(&self.tdoc, &DocumentPosition::at(pos.path.clone(), i))
    }

    fn word_left_position(&self, pos: &DocumentPosition) -> DocumentPosition {
        let text = self.leaf_plain_text(&pos.path);
        let mut i = pos.offset.min(text.len());
        if i == 0 {
            if let Some(prev) = self.prev_leaf(&pos.path) {
                let len = self.leaf_text_len(&prev);
                return DocumentPosition::at(prev, len);
            }
            return tree_walk::clamp_position(&self.tdoc, pos);
        }
        while i > 0 {
            let (prev_i, ch) = text[..i].char_indices().next_back().unwrap();
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                i = prev_i;
            } else {
                break;
            }
        }
        while i > 0 {
            let (prev_i, ch) = text[..i].char_indices().next_back().unwrap();
            if !(ch.is_whitespace() || ch.is_ascii_punctuation()) {
                i = prev_i;
            } else {
                break;
            }
        }
        tree_walk::clamp_position(&self.tdoc, &DocumentPosition::at(pos.path.clone(), i))
    }

    pub fn move_cursor_left_extend(&mut self) {
        self.normalize_cursor();
        let new = self.position_left(&self.cursor.clone());
        if new != self.cursor {
            self.extend_selection_to(new);
        }
    }

    pub fn move_cursor_right_extend(&mut self) {
        self.normalize_cursor();
        let new = self.position_right(&self.cursor.clone());
        if new != self.cursor {
            self.extend_selection_to(new);
        }
    }

    pub fn move_cursor_up_extend(&mut self) {
        if let Some(prev) = self.prev_leaf(&self.cursor.path) {
            let len = self.leaf_text_len(&prev);
            let new = DocumentPosition::at(prev, self.cursor.offset.min(len));
            self.extend_selection_to(new);
        }
    }

    pub fn move_cursor_down_extend(&mut self) {
        if let Some(next) = self.next_leaf(&self.cursor.path) {
            let len = self.leaf_text_len(&next);
            let new = DocumentPosition::at(next, self.cursor.offset.min(len));
            self.extend_selection_to(new);
        }
    }

    pub fn move_cursor_to_line_start_extend(&mut self) {
        let new = DocumentPosition::at(self.cursor.path.clone(), 0);
        if new != self.cursor {
            self.extend_selection_to(new);
        }
    }

    pub fn move_cursor_to_line_end_extend(&mut self) {
        let new = DocumentPosition::at(
            self.cursor.path.clone(),
            self.leaf_text_len(&self.cursor.path.clone()),
        );
        if new != self.cursor {
            self.extend_selection_to(new);
        }
    }

    // ----- Text extraction -----------------------------------------------------------

    pub fn text_in_range(&self, start: DocumentPosition, end: DocumentPosition) -> String {
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };
        let paths = self.leaf_paths();
        let mut out = String::new();
        let mut started = false;
        for path in &paths {
            if *path < start.path || *path > end.path {
                continue;
            }
            let text = self.leaf_plain_text(path);
            let from = if *path == start.path {
                start.offset.min(text.len())
            } else {
                0
            };
            let to = if *path == end.path {
                end.offset.min(text.len())
            } else {
                text.len()
            };
            if from < to {
                if started {
                    out.push_str("\n\n");
                }
                out.push_str(&text[from..to]);
                started = true;
            }
        }
        out
    }

    pub fn get_selection_text(&self) -> String {
        let Some((start, end)) = self.selection.clone() else {
            return String::new();
        };
        self.text_in_range(start, end)
    }

    /// The current selection as a standalone document. TODO(phase2): preserve block types
    /// across a multi-leaf selection; Phase 1 yields plain text paragraphs.
    pub fn get_selection_document(&self) -> Option<Document> {
        let (a, b) = self.selection.clone()?;
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let paths = self.leaf_paths();
        let mut paragraphs: Vec<Paragraph> = Vec::new();
        for path in &paths {
            if *path < start.path || *path > end.path {
                continue;
            }
            let content = tree_walk::leaf_inline(&self.tdoc, path);
            let len: usize = content.iter().map(|c| c.text_len()).sum();
            let from = if *path == start.path {
                start.offset.min(len)
            } else {
                0
            };
            let to = if *path == end.path {
                end.offset.min(len)
            } else {
                len
            };
            if from >= to {
                continue;
            }
            let runs = extract_inline_range(&content, from, to);
            paragraphs.push(Paragraph::new_text().with_content(inline_to_spans(&runs)));
        }
        if paragraphs.is_empty() {
            None
        } else {
            Some(Document::new().with_paragraphs(paragraphs))
        }
    }

    pub fn cut(&mut self) -> Result<String, EditError> {
        let text = self.get_selection_text();
        if !text.is_empty() {
            self.delete_selection()?;
        }
        Ok(text)
    }

    pub fn copy(&self) -> String {
        self.get_selection_text()
    }

    pub fn paste(&mut self, text: &str) -> EditResult {
        let normalized = normalize_plain_text(text);
        if normalized.is_empty() {
            return Ok(());
        }
        if !normalized.contains('\n') {
            return self.insert_text(&normalized);
        }
        // Multi-line plain text: each line becomes its own paragraph.
        let paragraphs = normalized
            .split('\n')
            .map(|line| Paragraph::new_text().with_content(vec![Span::new_text(line)]))
            .collect();
        let doc = Document::new().with_paragraphs(paragraphs);
        self.insert_document(&doc)
    }

    // ----- Structural ops (TODO(phase2/3)) -------------------------------------------

    /// Cycle the current line: Paragraph → H1 → H2 → H3 → Paragraph.
    pub fn toggle_heading(&mut self) -> EditResult {
        let next = match self.current_block_type() {
            BlockType::Heading { level: 1 } => BlockType::Heading { level: 2 },
            BlockType::Heading { level: 2 } => BlockType::Heading { level: 3 },
            BlockType::Heading { .. } => BlockType::Paragraph,
            _ => BlockType::Heading { level: 1 },
        };
        self.set_block_type(next)
    }

    /// Insert an arbitrary inline element at the cursor (within the current leaf).
    pub fn insert_inline_at_cursor(&mut self, inline: InlineContent) -> EditResult {
        if self.leaf_count() == 0 {
            let len = inline.text_len();
            self.tdoc
                .add_paragraph(Paragraph::new_text().with_content(inline_to_spans(&[inline])));
            self.cursor = DocumentPosition::at(TreePath::root(0), len);
            return Ok(());
        }
        if self.selection.is_some() {
            self.delete_selection()?;
        }
        let path = self.cursor.path.clone();
        if self.is_table_leaf(&path) {
            return Ok(());
        }
        let offset = self.cursor.offset;
        let inserted_len = inline.text_len();
        self.edit_leaf(&path, |content| {
            let mut block = Block::paragraph();
            block.content = std::mem::take(content);
            let right = block.split_content_at(offset);
            block.content.push(inline);
            block.content.extend(right);
            block.normalize_content();
            *content = block.content;
        });
        self.cursor.offset = offset + inserted_len;
        self.selection = None;
        Ok(())
    }

    /// Replace the current selection with a link.
    pub fn replace_selection_with_link(&mut self, destination: &str, text: &str) -> EditResult {
        self.delete_selection()?;
        self.insert_link_at_cursor(destination, text)
    }

    /// Insert a link at the cursor.
    pub fn insert_link_at_cursor(&mut self, destination: &str, text: &str) -> EditResult {
        let link_inline = InlineContent::Link {
            link: super::structured_document::Link {
                destination: destination.to_string(),
                title: None,
            },
            content: vec![InlineContent::Text(TextRun::plain(text))],
        };
        self.insert_inline_at_cursor(link_inline)
    }

    /// Edit an existing link at the given leaf path + inline index.
    pub fn edit_link_at(
        &mut self,
        path: TreePath,
        inline_index: usize,
        destination: &str,
        text: &str,
    ) -> EditResult {
        let dest = destination.to_string();
        let text = text.to_string();
        let ok = self.edit_leaf(&path, |content| {
            if let Some(InlineContent::Link {
                link,
                content: inner,
            }) = content.get_mut(inline_index)
            {
                link.destination = dest.clone();
                *inner = vec![InlineContent::Text(TextRun::plain(text.clone()))];
                true
            } else {
                false
            }
        });
        if ok {
            Ok(())
        } else {
            Err(EditError::InvalidPosition)
        }
    }

    /// Remove (unwrap) a link at the given leaf path + inline index, keeping its text.
    pub fn remove_link_at(&mut self, path: TreePath, inline_index: usize) -> EditResult {
        let ok = self.edit_leaf(&path, |content| {
            if inline_index >= content.len() {
                return false;
            }
            if let InlineContent::Link { content: inner, .. } = content.remove(inline_index) {
                for (i, item) in inner.into_iter().enumerate() {
                    content.insert(inline_index + i, item);
                }
                true
            } else {
                false
            }
        });
        if ok {
            Ok(())
        } else {
            Err(EditError::InvalidPosition)
        }
    }

    pub fn toggle_bold(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| s.bold = !s.bold)
    }
    pub fn toggle_italic(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| s.italic = !s.italic)
    }
    pub fn toggle_code(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| s.code = !s.code)
    }
    pub fn toggle_strikethrough(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| s.strikethrough = !s.strikethrough)
    }
    pub fn toggle_underline(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| s.underline = !s.underline)
    }
    pub fn toggle_highlight(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| s.highlight = !s.highlight)
    }
    pub fn clear_formatting(&mut self) -> EditResult {
        self.apply_style_to_selection(|s| *s = TextStyle::default())
    }

    /// Apply a style mutation to every run within the current selection, across leaves.
    fn apply_style_to_selection(&mut self, mut apply: impl FnMut(&mut TextStyle)) -> EditResult {
        let Some((a, b)) = self.selection.clone() else {
            return Ok(());
        };
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        for path in self.leaf_paths() {
            if path < start.path || path > end.path {
                continue;
            }
            let len = self.leaf_text_len(&path);
            let from = if path == start.path {
                start.offset.min(len)
            } else {
                0
            };
            let to = if path == end.path {
                end.offset.min(len)
            } else {
                len
            };
            if from >= to {
                continue;
            }
            self.edit_leaf(&path, |content| {
                let (before, selected, after) = split_content_for_style(content, from, to);
                let styled = map_style_on_runs(selected, &mut apply);
                *content = before.into_iter().chain(styled).chain(after).collect();
            });
        }
        Ok(())
    }

    pub fn toggle_list(&mut self) -> EditResult {
        self.toggle_list_kind(false, false)
    }
    pub fn toggle_checklist(&mut self) -> EditResult {
        self.toggle_list_kind(false, true)
    }
    pub fn toggle_ordered_list(&mut self) -> EditResult {
        self.toggle_list_kind(true, false)
    }

    pub fn toggle_quote(&mut self) -> EditResult {
        // In a quote already → unwrap; otherwise wrap the cursor's top-level block.
        if matches!(self.cursor.path.last(), Some(PathSegment::QuoteChild(_))) {
            self.unwrap_quote()
        } else {
            self.wrap_top_level_in_quote()
        }
    }

    pub fn toggle_code_block(&mut self) -> EditResult {
        if matches!(self.current_block_type(), BlockType::CodeBlock { .. }) {
            self.apply_variant_over_selection(|s| Paragraph::new_text().with_content(s))
        } else {
            self.apply_variant_over_selection(|s| Paragraph::new_code_block().with_content(s))
        }
    }

    /// Set the paragraph style at the cursor/selection. Heading/CodeBlock/Paragraph are
    /// in-place leaf-variant changes; BlockQuote/ListItem route to the structural toggles.
    pub fn set_block_type(&mut self, block_type: BlockType) -> EditResult {
        match block_type {
            BlockType::Paragraph => {
                // "Paragraph" also exits a block quote (mirrors the flat block-type model).
                if matches!(self.cursor.path.last(), Some(PathSegment::QuoteChild(_))) {
                    self.unwrap_quote()
                } else {
                    self.apply_variant_over_selection(|s| Paragraph::new_text().with_content(s))
                }
            }
            BlockType::Heading { level } => {
                let level = level.clamp(1, 3);
                self.apply_variant_over_selection(move |s| make_header(level, s))
            }
            BlockType::CodeBlock { .. } => {
                self.apply_variant_over_selection(|s| Paragraph::new_code_block().with_content(s))
            }
            BlockType::BlockQuote => self.wrap_top_level_in_quote(),
            BlockType::ListItem {
                ordered, checkbox, ..
            } => self.toggle_list_kind(ordered, checkbox.is_some()),
            BlockType::Table { .. } => Ok(()),
        }
    }

    // ----- Structural helpers (top-level focus) --------------------------------------

    /// The normalized (start, end) of the selection, or the collapsed cursor.
    fn selection_or_cursor_range(&self) -> (DocumentPosition, DocumentPosition) {
        match self.selection.clone() {
            Some((a, b)) => {
                if a <= b {
                    (a, b)
                } else {
                    (b, a)
                }
            }
            None => (self.cursor.clone(), self.cursor.clone()),
        }
    }

    /// Apply an in-place leaf-variant change to every leaf in the selection/cursor range.
    fn apply_variant_over_selection(
        &mut self,
        make: impl Fn(Vec<Span>) -> Paragraph,
    ) -> EditResult {
        let (start, end) = self.selection_or_cursor_range();
        for path in self.leaf_paths() {
            if path < start.path || path > end.path {
                continue;
            }
            tree_edit::replace_leaf_variant(&mut self.tdoc, &path, &make);
        }
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(())
    }

    /// The cursor's top-level paragraph index, if the cursor is at the top level.
    fn cursor_top_index(&self) -> Option<usize> {
        match self.cursor.path.segments() {
            [PathSegment::Paragraph(i)] => Some(*i),
            _ => None,
        }
    }

    fn wrap_top_level_in_quote(&mut self) -> EditResult {
        let Some(i) = self.cursor_top_index() else {
            return Ok(());
        };
        let offset = self.cursor.offset;
        let para = self.tdoc.paragraphs.remove(i);
        self.tdoc
            .paragraphs
            .insert(i, Paragraph::new_quote().with_children(vec![para]));
        self.cursor =
            DocumentPosition::at(TreePath::root(i).child(PathSegment::QuoteChild(0)), offset);
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(())
    }

    fn unwrap_quote(&mut self) -> EditResult {
        let segs = self.cursor.path.segments().to_vec();
        let [PathSegment::Paragraph(i), PathSegment::QuoteChild(c)] = segs.as_slice() else {
            return Ok(());
        };
        let (i, c) = (*i, *c);
        let offset = self.cursor.offset;
        let Some(Paragraph::Quote { children }) = self.tdoc.paragraphs.get(i).cloned() else {
            return Ok(());
        };
        self.tdoc.paragraphs.remove(i);
        let count = children.len();
        for (k, child) in children.into_iter().enumerate() {
            self.tdoc.paragraphs.insert(i + k, child);
        }
        let target = (i + c.min(count.saturating_sub(1))).min(self.tdoc.paragraphs.len());
        self.cursor = DocumentPosition::at(TreePath::root(target), offset);
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(())
    }

    /// Toggle a list of the given kind over the contiguous top-level selection. If the
    /// cursor is already in a top-level list, unwrap it back to paragraphs.
    fn toggle_list_kind(&mut self, ordered: bool, checklist: bool) -> EditResult {
        // Unwrap if the cursor sits in a top-level list/checklist.
        if let [PathSegment::Paragraph(i), rest @ ..] = self.cursor.path.segments()
            && !rest.is_empty()
            && matches!(
                self.tdoc.paragraphs.get(*i),
                Some(
                    Paragraph::OrderedList { .. }
                        | Paragraph::UnorderedList { .. }
                        | Paragraph::Checklist { .. }
                )
            )
        {
            return self.unwrap_list_at(*i);
        }

        // Otherwise wrap the selected top-level paragraphs into one list.
        let (start, end) = self.selection_or_cursor_range();
        let (Some(s), Some(e)) = (top_index(&start.path), top_index(&end.path)) else {
            return Ok(());
        };
        if s > e || e >= self.tdoc.paragraphs.len() {
            return Ok(());
        }
        let cursor_rel = self
            .cursor_top_index()
            .map(|ci| ci.saturating_sub(s))
            .unwrap_or(0);
        let offset = self.cursor.offset;
        let drained: Vec<Paragraph> = self.tdoc.paragraphs.drain(s..=e).collect();

        let new_node = if checklist {
            let items = drained
                .into_iter()
                .map(|p| ChecklistItem::new(false).with_content(p.content().to_vec()))
                .collect();
            Paragraph::new_checklist().with_checklist_items(items)
        } else {
            let entries: Vec<Vec<Paragraph>> = drained
                .into_iter()
                .map(|p| vec![Paragraph::new_text().with_content(p.content().to_vec())])
                .collect();
            if ordered {
                Paragraph::new_ordered_list().with_entries(entries)
            } else {
                Paragraph::new_unordered_list().with_entries(entries)
            }
        };
        self.tdoc.paragraphs.insert(s, new_node);

        let leaf_seg = if checklist {
            PathSegment::ChecklistItem(cursor_rel)
        } else {
            PathSegment::ListEntry {
                entry: cursor_rel,
                para: 0,
            }
        };
        self.cursor = DocumentPosition::at(TreePath::root(s).child(leaf_seg), offset);
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(())
    }

    /// Replace the top-level list/checklist at index `i` with its items as paragraphs.
    fn unwrap_list_at(&mut self, i: usize) -> EditResult {
        let offset = self.cursor.offset;
        // Which item does the cursor sit in?
        let cursor_item = match self.cursor.path.segments() {
            [_, PathSegment::ListEntry { entry, .. }] => *entry,
            [_, PathSegment::ChecklistItem(c)] => *c,
            _ => 0,
        };
        let Some(node) = self.tdoc.paragraphs.get(i).cloned() else {
            return Ok(());
        };
        let paragraphs: Vec<Paragraph> = match node {
            Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries } => entries
                .into_iter()
                .map(|entry| {
                    Paragraph::new_text().with_content(
                        entry
                            .first()
                            .map(|p| p.content().to_vec())
                            .unwrap_or_default(),
                    )
                })
                .collect(),
            Paragraph::Checklist { items } => items
                .into_iter()
                .map(|item| Paragraph::new_text().with_content(item.content))
                .collect(),
            _ => return Ok(()),
        };
        self.tdoc.paragraphs.remove(i);
        let count = paragraphs.len();
        for (k, p) in paragraphs.into_iter().enumerate() {
            self.tdoc.paragraphs.insert(i + k, p);
        }
        let target = i + cursor_item.min(count.saturating_sub(1));
        self.cursor = DocumentPosition::at(TreePath::root(target), offset);
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(())
    }

    pub fn toggle_checkmark_at(&mut self, path: TreePath) -> Result<bool, EditError> {
        tree_edit::toggle_checkmark(&mut self.tdoc, &path).ok_or(EditError::InvalidPosition)
    }

    pub fn toggle_current_checkmark(&mut self) -> Result<bool, EditError> {
        let path = self.cursor.path.clone();
        tree_edit::toggle_checkmark(&mut self.tdoc, &path).ok_or(EditError::InvalidPosition)
    }

    /// Swap the cursor's top-level block with the previous one. TODO(phase3): reorder
    /// within lists/quotes.
    pub fn move_blocks_up(&mut self) -> Result<bool, EditError> {
        let Some(i) = self.cursor_top_index() else {
            return Ok(false);
        };
        if i == 0 {
            return Ok(false);
        }
        self.tdoc.paragraphs.swap(i - 1, i);
        self.cursor = DocumentPosition::at(TreePath::root(i - 1), self.cursor.offset);
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(true)
    }

    pub fn move_blocks_down(&mut self) -> Result<bool, EditError> {
        let Some(i) = self.cursor_top_index() else {
            return Ok(false);
        };
        if i + 1 >= self.tdoc.paragraphs.len() {
            return Ok(false);
        }
        self.tdoc.paragraphs.swap(i, i + 1);
        self.cursor = DocumentPosition::at(TreePath::root(i + 1), self.cursor.offset);
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(true)
    }

    pub fn indent_list_item(&mut self) -> EditResult {
        Ok(()) // TODO(phase3)
    }

    pub fn outdent_list_item(&mut self) -> EditResult {
        Ok(()) // TODO(phase3)
    }

    /// Insert a document fragment (e.g. from the clipboard) at the cursor. Splits the
    /// current top-level paragraph and splices the fragment's paragraphs between the
    /// halves. TODO(phase3): splice within lists/quotes (currently only at top level).
    pub fn insert_document(&mut self, document: &Document) -> EditResult {
        if document.paragraphs.is_empty() {
            return Ok(());
        }
        if self.selection.is_some() {
            self.delete_selection()?;
        }
        if self.leaf_count() == 0 {
            for p in &document.paragraphs {
                self.tdoc.add_paragraph(p.clone());
            }
            let last = self.tdoc.paragraphs.len().saturating_sub(1);
            let path = TreePath::root(last);
            let len = self.leaf_text_len(&path);
            self.cursor = DocumentPosition::at(path, len);
            self.selection = None;
            self.trigger_paragraph_change();
            return Ok(());
        }
        let Some(i) = self.cursor_top_index() else {
            // Fallback for non-top-level cursors: insert as plain text.
            let mut buf = Vec::new();
            let _ = tdoc::markdown::write(&mut buf, document);
            let text = String::from_utf8_lossy(&buf).into_owned();
            return self.insert_text(text.trim_end_matches('\n'));
        };

        let offset = self.cursor.offset;
        let path = TreePath::root(i);
        let runs = tree_walk::leaf_inline(&self.tdoc, &path);
        let (left, right) = {
            let mut block = Block::paragraph();
            block.content = runs;
            let r = block.split_content_at(offset);
            (block.content, r)
        };
        tree_walk::set_leaf_inline(&mut self.tdoc, &path, &left);

        let mut frag = document.paragraphs.clone();

        // If the current block and the first fragment paragraph are both plain text, merge
        // the first fragment into the current block (so "A|B" + paste "X\nY" → "AX", "YB").
        if matches!(self.tdoc.paragraphs.get(i), Some(Paragraph::Text { .. }))
            && matches!(frag.first(), Some(Paragraph::Text { .. }))
        {
            let first = frag.remove(0);
            let mut merged = left.clone();
            merged.extend(spans_to_inline(first.content()));
            tree_walk::set_leaf_inline(&mut self.tdoc, &path, &merged);
        }

        // Insert the remaining fragment paragraphs after the current block.
        let mut last_idx = i;
        for (k, p) in frag.into_iter().enumerate() {
            self.tdoc.paragraphs.insert(i + 1 + k, p);
            last_idx = i + 1 + k;
        }

        // The cursor lands at the join point: the end of the last block's own content,
        // before the right-hand remainder is appended.
        let last_path = TreePath::root(last_idx);
        let cursor_off = self.leaf_text_len(&last_path);

        // Append the right-hand remainder to the last block (or as a new paragraph if that
        // block can't hold inline content directly).
        if right.iter().map(|c| c.text_len()).sum::<usize>() > 0 {
            if tree_walk::leaf_spans(&self.tdoc, &last_path).is_some()
                && !matches!(
                    self.tdoc.paragraphs.get(last_idx),
                    Some(Paragraph::Table { .. })
                )
            {
                let mut content = tree_walk::leaf_inline(&self.tdoc, &last_path);
                content.extend(right);
                let mut block = Block::paragraph();
                block.content = content;
                block.normalize_content();
                tree_walk::set_leaf_inline(&mut self.tdoc, &last_path, &block.content);
            } else {
                self.tdoc.paragraphs.insert(
                    last_idx + 1,
                    Paragraph::new_text().with_content(inline_to_spans(&right)),
                );
            }
        }

        self.cursor = DocumentPosition::at(last_path, cursor_off);
        self.selection = None;
        self.normalize_cursor();
        self.trigger_paragraph_change();
        Ok(())
    }
}

/// Build a header paragraph of the given level (1-3) carrying `spans`.
fn make_header(level: u8, spans: Vec<Span>) -> Paragraph {
    match level {
        1 => Paragraph::new_header1(),
        2 => Paragraph::new_header2(),
        _ => Paragraph::new_header3(),
    }
    .with_content(spans)
}

/// The top-level paragraph index a path addresses, if it is a top-level leaf.
fn top_index(path: &TreePath) -> Option<usize> {
    match path.segments() {
        [PathSegment::Paragraph(i)] => Some(*i),
        _ => None,
    }
}

impl Default for StructuredEditor {
    fn default() -> Self {
        Self::new()
    }
}

/// Insert `text` into a flat inline-content vector at byte `offset`, preserving the style
/// of the run the cursor sits in and keeping insertions at link edges outside the link.
fn insert_into_content(content: &mut Vec<InlineContent>, offset: usize, text: &str) {
    let (idx, content_offset) = find_content_at_offset(content, offset);
    if idx >= content.len() {
        content.push(InlineContent::Text(TextRun::plain(text)));
        return;
    }
    match &mut content[idx] {
        InlineContent::Text(run) => run.insert_text(content_offset, text),
        InlineContent::Link { content: inner, .. } => {
            let link_len: usize = inner.iter().map(|c| c.text_len()).sum();
            if content_offset == 0 {
                if idx > 0
                    && let InlineContent::Text(prev) = &mut content[idx - 1]
                {
                    let prev_len = prev.len();
                    prev.insert_text(prev_len, text);
                } else {
                    content.insert(idx, InlineContent::Text(TextRun::plain(text)));
                }
            } else if content_offset >= link_len {
                if idx + 1 < content.len()
                    && let InlineContent::Text(next) = &mut content[idx + 1]
                {
                    next.insert_text(0, text);
                } else {
                    content.insert(idx + 1, InlineContent::Text(TextRun::plain(text)));
                }
            } else {
                let (inner_idx, inner_off) = find_content_at_offset(inner, content_offset);
                if inner_idx >= inner.len() {
                    inner.push(InlineContent::Text(TextRun::plain(text)));
                } else if let InlineContent::Text(run) = &mut inner[inner_idx] {
                    run.insert_text(inner_off, text);
                } else {
                    inner.insert(inner_idx, InlineContent::Text(TextRun::plain(text)));
                }
            }
        }
        InlineContent::HardBreak => {
            if content_offset == 0 {
                content.insert(idx, InlineContent::Text(TextRun::plain(text)));
            } else if idx + 1 < content.len()
                && let InlineContent::Text(run) = &mut content[idx + 1]
            {
                run.insert_text(0, text);
            } else {
                content.insert(idx + 1, InlineContent::Text(TextRun::plain(text)));
            }
        }
    }
}

/// Find the inline element index and the offset within it for a flattened byte offset.
fn find_content_at_offset(content: &[InlineContent], offset: usize) -> (usize, usize) {
    let mut current = 0;
    for (idx, item) in content.iter().enumerate() {
        let len = item.text_len();
        if current + len >= offset {
            return (idx, offset - current);
        }
        current += len;
    }
    (content.len(), 0)
}

/// Split content into (before, within, after) the `[start, end)` byte range, splitting
/// the runs that straddle a boundary.
fn split_content_for_style(
    content: &[InlineContent],
    start_offset: usize,
    end_offset: usize,
) -> (Vec<InlineContent>, Vec<InlineContent>, Vec<InlineContent>) {
    let mut before = Vec::new();
    let mut selected = Vec::new();
    let mut after = Vec::new();
    let mut current_offset = 0;

    for item in content {
        let item_len = item.text_len();
        let item_start = current_offset;
        let item_end = current_offset + item_len;

        if item_end <= start_offset {
            before.push(item.clone());
        } else if item_start >= end_offset {
            after.push(item.clone());
        } else if item_start >= start_offset && item_end <= end_offset {
            selected.push(item.clone());
        } else {
            match item {
                InlineContent::Text(run) => {
                    let text = &run.text;
                    let sel_start_in_run = start_offset.saturating_sub(item_start);
                    let sel_end_in_run = end_offset.saturating_sub(item_start).min(item_len);
                    if sel_start_in_run > 0 {
                        let mut before_run = run.clone();
                        before_run.text = text[..sel_start_in_run].to_string();
                        before.push(InlineContent::Text(before_run));
                    }
                    if sel_end_in_run > sel_start_in_run {
                        let mut selected_run = run.clone();
                        selected_run.text = text[sel_start_in_run..sel_end_in_run].to_string();
                        selected.push(InlineContent::Text(selected_run));
                    }
                    if sel_end_in_run < item_len {
                        let mut after_run = run.clone();
                        after_run.text = text[sel_end_in_run..].to_string();
                        after.push(InlineContent::Text(after_run));
                    }
                }
                _ => {
                    if item_start < start_offset {
                        before.push(item.clone());
                    } else if item_start < end_offset {
                        selected.push(item.clone());
                    } else {
                        after.push(item.clone());
                    }
                }
            }
        }
        current_offset += item_len;
    }

    (before, selected, after)
}

/// Recursively apply a style mutation to every text run (descending into links).
fn map_style_on_runs<F>(items: Vec<InlineContent>, apply: &mut F) -> Vec<InlineContent>
where
    F: FnMut(&mut TextStyle),
{
    items
        .into_iter()
        .map(|item| match item {
            InlineContent::Text(mut run) => {
                apply(&mut run.style);
                InlineContent::Text(run)
            }
            InlineContent::Link { link, content } => InlineContent::Link {
                link,
                content: map_style_on_runs(content, apply),
            },
            other => other,
        })
        .collect()
}

/// Extract the inline runs covering `[start, end)` (flattened byte offsets).
fn extract_inline_range(content: &[InlineContent], start: usize, end: usize) -> Vec<InlineContent> {
    let mut head = Block::paragraph();
    head.content = content.to_vec();
    let tail = head.split_content_at(start);
    let mut result = Block::paragraph();
    result.content = tail;
    let _ = result.split_content_at(end.saturating_sub(start));
    result.content
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_text_into_empty_document() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        assert_eq!(editor.leaf_plain_text(&TreePath::root(0)), "Hello");
        assert_eq!(editor.cursor().offset, 5);
    }

    #[test]
    fn typing_continues_run_style() {
        // Loading bold markdown then typing inside keeps it one styled leaf.
        let mut editor = StructuredEditor::new();
        editor.load_markdown("**bold**");
        editor.set_cursor(DocumentPosition::at(TreePath::root(0), 2));
        editor.insert_text("X").unwrap();
        assert_eq!(editor.leaf_plain_text(&TreePath::root(0)), "boXld");
    }

    #[test]
    fn intra_leaf_delete_backward() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.delete_backward().unwrap();
        assert_eq!(editor.leaf_plain_text(&TreePath::root(0)), "Hell");
        assert_eq!(editor.cursor().offset, 4);
    }

    #[test]
    fn word_navigation_within_leaf() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("alpha beta gamma");
        editor.set_cursor(DocumentPosition::at(TreePath::root(0), 0));
        editor.move_word_right();
        assert_eq!(editor.cursor().offset, 5); // end of "alpha" (word-right stops after the word)
    }

    #[test]
    fn undo_redo_round_trips_typing() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.commit_undo_step(UndoKind::Typing, Instant::now());
        assert!(editor.undo());
        assert_eq!(editor.leaf_count(), 0);
        assert!(editor.redo());
        assert_eq!(editor.leaf_plain_text(&TreePath::root(0)), "Hello");
    }

    #[test]
    fn top_level_newline_splits_paragraph() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("HelloWorld").unwrap();
        editor.set_cursor(DocumentPosition::at(TreePath::root(0), 5));
        editor.insert_newline().unwrap();
        assert_eq!(editor.leaf_count(), 2);
        assert_eq!(editor.leaf_plain_text(&TreePath::root(0)), "Hello");
        assert_eq!(editor.leaf_plain_text(&TreePath::root(1)), "World");
    }

    fn md(editor: &StructuredEditor) -> String {
        super::super::markdown_converter::document_to_markdown(editor.tdoc())
            .trim()
            .to_string()
    }

    #[test]
    fn toggle_bold_over_selection() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("hello world");
        editor.set_selection(
            DocumentPosition::at(TreePath::root(0), 0),
            DocumentPosition::at(TreePath::root(0), 5),
        );
        editor.toggle_bold().unwrap();
        assert_eq!(md(&editor), "**hello** world");
        // Toggling again removes it.
        editor.toggle_bold().unwrap();
        assert_eq!(md(&editor), "hello world");
    }

    #[test]
    fn insert_and_remove_link() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("see ").unwrap();
        editor
            .insert_link_at_cursor("https://example.test", "here")
            .unwrap();
        assert_eq!(md(&editor), "see [here](https://example.test)");
        editor.remove_link_at(TreePath::root(0), 1).unwrap();
        assert_eq!(md(&editor), "see here");
    }

    #[test]
    fn heading_and_paragraph_conversion() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Title").unwrap();
        editor
            .set_block_type(BlockType::Heading { level: 2 })
            .unwrap();
        assert_eq!(md(&editor), "## Title");
        editor.set_block_type(BlockType::Paragraph).unwrap();
        assert_eq!(md(&editor), "Title");
    }

    #[test]
    fn toggle_list_wraps_and_unwraps() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("item").unwrap();
        editor.toggle_list().unwrap();
        assert_eq!(md(&editor), "- item");
        assert!(matches!(
            editor.current_block_type(),
            BlockType::ListItem { ordered: false, .. }
        ));
        editor.toggle_list().unwrap();
        assert_eq!(md(&editor), "item");
    }

    #[test]
    fn enter_in_list_creates_new_item() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("- one");
        let item = TreePath::root(0).child(PathSegment::ListEntry { entry: 0, para: 0 });
        editor.set_cursor(DocumentPosition::at(item, 3));
        editor.insert_newline().unwrap();
        editor.insert_text("two").unwrap();
        assert_eq!(md(&editor), "- one\n- two");
    }

    #[test]
    fn backspace_merges_list_item_into_previous() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("- one\n- two");
        let second = TreePath::root(0).child(PathSegment::ListEntry { entry: 1, para: 0 });
        editor.set_cursor(DocumentPosition::at(second, 0));
        editor.delete_backward().unwrap();
        assert_eq!(md(&editor), "- onetwo");
    }

    #[test]
    fn ordered_list_renumbers_automatically() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("1. one\n2. two\n3. three");
        // Delete the middle item by merging it into the first.
        let second = TreePath::root(0).child(PathSegment::ListEntry { entry: 1, para: 0 });
        editor.set_cursor(DocumentPosition::at(second, 0));
        editor.delete_backward().unwrap();
        // "onetwo" then "three" → renumbered 1, 2.
        assert_eq!(md(&editor), "1. onetwo\n2. three");
    }

    #[test]
    fn toggle_quote_wraps_and_unwraps() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("quoted").unwrap();
        editor.toggle_quote().unwrap();
        assert_eq!(md(&editor), "> quoted");
        editor.toggle_quote().unwrap();
        assert_eq!(md(&editor), "quoted");
    }

    #[test]
    fn toggle_checkmark_round_trips() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("- [ ] task");
        let item = TreePath::root(0).child(PathSegment::ChecklistItem(0));
        editor.set_cursor(DocumentPosition::at(item, 0));
        assert_eq!(editor.toggle_current_checkmark(), Ok(true));
        assert_eq!(md(&editor), "- [x] task");
    }

    #[test]
    fn move_block_down_swaps_with_next() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("first\n\nsecond");
        editor.set_cursor(DocumentPosition::at(TreePath::root(0), 0));
        assert_eq!(editor.move_blocks_down(), Ok(true));
        assert_eq!(md(&editor), "second\n\nfirst");
    }

    #[test]
    fn multi_paragraph_paste_splits_blocks() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("AB").unwrap();
        editor.set_cursor(DocumentPosition::at(TreePath::root(0), 1));
        editor.paste("X\nY").unwrap();
        // "A|B" + "X\nY": first line merges into the current block, last line takes the tail.
        assert_eq!(md(&editor), "AX\n\nYB");
    }

    #[test]
    fn cross_leaf_delete_selection_merges() {
        let mut editor = StructuredEditor::new();
        editor.load_markdown("hello\n\nworld");
        editor.set_selection(
            DocumentPosition::at(TreePath::root(0), 3),
            DocumentPosition::at(TreePath::root(1), 2),
        );
        editor.delete_selection().unwrap();
        assert_eq!(md(&editor), "helrld");
    }
}
