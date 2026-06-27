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

use super::inline_convert::inline_to_spans;
use super::markdown_converter::markdown_to_document;
use super::structured_document::{Block, BlockType, InlineContent, TextRun, normalize_plain_text};
use super::tree_path::{DocumentPosition, TreePath};
use super::tree_walk::{self, LeafInfo};
use std::time::{Duration, Instant};
use tdoc::Document;
use tdoc::paragraph::Paragraph;

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
        if self.selection.is_some() {
            self.delete_selection()?;
        }
        let path = self.cursor.path.clone();
        let segs = path.segments();
        // Only top-level paragraphs are split in Phase 1.
        if segs.len() == 1
            && let super::tree_path::PathSegment::Paragraph(idx) = segs[0]
            && !self.is_table_leaf(&path)
        {
            let offset = self.cursor.offset;
            let right = self.edit_leaf(&path, |content| {
                let mut block = Block::paragraph();
                block.content = std::mem::take(content);
                let right = block.split_content_at(offset);
                *content = block.content;
                right
            });
            self.tdoc.paragraphs.insert(
                idx + 1,
                Paragraph::new_text().with_content(inline_to_spans(&right)),
            );
            self.cursor = DocumentPosition::at(TreePath::root(idx + 1), 0);
        }
        // TODO(phase2): list/quote/checklist splits.
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

        if offset == 0 {
            // TODO(phase2): tree-aware merge with the previous leaf / outdent.
            return Ok(());
        }
        if self.is_table_leaf(&path) {
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

        if offset >= len {
            // TODO(phase2): tree-aware merge with the next leaf / table removal.
            return Ok(());
        }
        if self.is_table_leaf(&path) {
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
            self.cursor = start;
        } else {
            // TODO(phase2): cross-leaf range deletion + merge.
            self.cursor = start;
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
        // TODO(phase2): multi-paragraph paste splits into sibling blocks. Phase 1 inserts
        // the text into the current leaf (newlines become hard breaks).
        let single_line = normalized.replace('\n', " ");
        self.insert_text(&single_line)
    }

    // ----- Structural ops (TODO(phase2/3)) -------------------------------------------

    pub fn toggle_heading(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn insert_inline_at_cursor(&mut self, _inline: InlineContent) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn replace_selection_with_link(&mut self, _destination: &str, _text: &str) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn insert_link_at_cursor(&mut self, _destination: &str, _text: &str) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn edit_link_at(
        &mut self,
        _path: TreePath,
        _inline_index: usize,
        _destination: &str,
        _text: &str,
    ) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn remove_link_at(&mut self, _path: TreePath, _inline_index: usize) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn toggle_bold(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_italic(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_code(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_strikethrough(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_underline(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_highlight(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn clear_formatting(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn toggle_list(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_checklist(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_ordered_list(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_quote(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }
    pub fn toggle_code_block(&mut self) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn set_block_type(&mut self, _block_type: BlockType) -> EditResult {
        Ok(()) // TODO(phase2)
    }

    pub fn toggle_checkmark_at(&mut self, _path: TreePath) -> Result<bool, EditError> {
        Ok(false) // TODO(phase3)
    }

    pub fn toggle_current_checkmark(&mut self) -> Result<bool, EditError> {
        Ok(false) // TODO(phase3)
    }

    pub fn move_blocks_up(&mut self) -> Result<bool, EditError> {
        Ok(false) // TODO(phase2)
    }

    pub fn move_blocks_down(&mut self) -> Result<bool, EditError> {
        Ok(false) // TODO(phase2)
    }

    pub fn indent_list_item(&mut self) -> EditResult {
        Ok(()) // TODO(phase3)
    }

    pub fn outdent_list_item(&mut self) -> EditResult {
        Ok(()) // TODO(phase3)
    }

    pub fn insert_document(&mut self, _document: &Document) -> EditResult {
        Ok(()) // TODO(phase2)
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
}
