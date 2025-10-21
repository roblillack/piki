// Structured Editor
// Provides editing operations on a StructuredDocument
// Completely independent of markdown syntax

use super::structured_document::*;
use std::cmp::min;

/// Result of an editing operation
pub type EditResult = Result<(), EditError>;

/// Errors that can occur during editing
#[derive(Debug, Clone, PartialEq)]
pub enum EditError {
    InvalidPosition,
    InvalidBlockIndex,
    EmptyDocument,
}

/// The structured editor with cursor state
pub struct StructuredEditor {
    document: StructuredDocument,
    cursor: DocumentPosition,
    selection: Option<(DocumentPosition, DocumentPosition)>, // (start, end)
    paragraph_cb: Option<Box<dyn FnMut(BlockType) + 'static>>,
}

impl StructuredEditor {
    /// Renumber a contiguous run of ordered list items starting at `start_index` with `start_number`.
    fn renumber_ordered_from(&mut self, start_index: usize, start_number: u64) {
        let mut n = start_number;
        let blocks_len = self.document.block_count();
        if start_index >= blocks_len {
            return;
        }
        let blocks = self.document.blocks_mut();
        let mut i = start_index;
        while i < blocks.len() {
            match blocks[i].block_type {
                BlockType::ListItem { ordered: true, .. } => {
                    blocks[i].block_type = BlockType::ListItem {
                        ordered: true,
                        number: Some(n),
                        checkbox: None,
                    };
                    n += 1;
                    i += 1;
                }
                _ => break,
            }
        }
    }
    /// Create a new editor with an empty document
    pub fn new() -> Self {
        StructuredEditor {
            document: StructuredDocument::new(),
            cursor: DocumentPosition::start(),
            selection: None,
            paragraph_cb: None,
        }
    }

    /// Create an editor with an existing document
    pub fn with_document(document: StructuredDocument) -> Self {
        StructuredEditor {
            document,
            cursor: DocumentPosition::start(),
            selection: None,
            paragraph_cb: None,
        }
    }

    /// Get the document
    pub fn document(&self) -> &StructuredDocument {
        &self.document
    }

    /// Get mutable document
    pub fn document_mut(&mut self) -> &mut StructuredDocument {
        &mut self.document
    }

    pub fn set_paragraph_change_callback(
        &mut self,
        cb: Option<Box<dyn FnMut(BlockType) + 'static>>,
    ) {
        self.paragraph_cb = cb;
        self.trigger_paragraph_change();
    }

    fn trigger_paragraph_change(&mut self) {
        println!("Triggering paragraph change");
        if let Some(cb) = self.paragraph_cb.as_mut() {
            println!("Calling paragraph change callback");
            let block_type = self
                .document
                .blocks()
                .get(self.cursor.block_index)
                .map(|b| b.block_type.clone())
                .unwrap_or(BlockType::Paragraph);
            cb(block_type);
        }
    }

    fn normalize_cursor(&mut self) {
        self.cursor = self.document.clamp_position(self.cursor);
    }

    /// Get cursor position
    pub fn cursor(&self) -> DocumentPosition {
        self.cursor
    }

    /// Set cursor position (will be clamped to valid range)
    pub fn set_cursor(&mut self, pos: DocumentPosition) {
        self.cursor = self.document.clamp_position(pos);
        self.selection = None; // Clear selection when moving cursor
        self.trigger_paragraph_change();
    }

    /// Get selection range
    pub fn selection(&self) -> Option<(DocumentPosition, DocumentPosition)> {
        self.selection
    }

    /// Set selection range
    pub fn set_selection(&mut self, start: DocumentPosition, end: DocumentPosition) {
        let start = self.document.clamp_position(start);
        let end = self.document.clamp_position_forward(end);
        self.selection = Some((start, end));
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.selection = None;
    }

    /// Select all content in the document
    pub fn select_all(&mut self) {
        if self.document.block_count() == 0 {
            self.selection = None;
            return;
        }
        let start = DocumentPosition::new(0, 0);
        let last_idx = self.document.block_count() - 1;
        let end = {
            let blocks = self.document.blocks();
            let last_len = blocks[last_idx].text_len();
            DocumentPosition::new(last_idx, last_len)
        };
        self.selection = Some((start, end));
        self.cursor = end;
        self.normalize_cursor();
    }

    /// Start or extend selection from current cursor position to a new position
    /// This is used for shift+movement and mouse drag selection
    pub fn extend_selection_to(&mut self, end: DocumentPosition) {
        let end = self.document.clamp_position(end);

        if let Some((start, _)) = self.selection {
            // Already have a selection - keep the original start, update end
            self.selection = Some((start, end));
        } else {
            // Start new selection from current cursor position
            self.selection = Some((self.cursor, end));
        }

        // Update cursor to the end position
        self.cursor = end;
        self.normalize_cursor();
    }

    /// Collect unique block indices covered by the current selection (if any).
    /// Returns an empty vector when there is no active selection or the selection is collapsed.
    fn collect_selection_block_indices(&self) -> Vec<usize> {
        let block_count = self.document.block_count();
        if block_count == 0 {
            return Vec::new();
        }

        let Some((mut start, mut end)) = self.selection else {
            return Vec::new();
        };

        if start.block_index > end.block_index
            || (start.block_index == end.block_index && start.offset > end.offset)
        {
            std::mem::swap(&mut start, &mut end);
        }

        if start.block_index == end.block_index && start.offset == end.offset {
            return Vec::new();
        }

        let start = self.document.clamp_position(start);
        let end = self.document.clamp_position_forward(end);

        let start_idx = start.block_index.min(block_count - 1);
        let end_idx = end.block_index.min(block_count - 1);

        let mut indices: Vec<usize> = (start_idx..=end_idx).collect();

        if let Some(&last) = indices.last() {
            if start_idx != end_idx && last == end_idx {
                let include_end = if end.offset == 0 {
                    self.document
                        .blocks()
                        .get(end_idx)
                        .map(|block| block.is_empty())
                        .unwrap_or(false)
                } else {
                    true
                };
                if !include_end {
                    indices.pop();
                }
            }
        }

        indices.sort_unstable();
        indices.dedup();
        indices
    }

    /// Select the word at the given position
    pub fn select_word_at(&mut self, pos: DocumentPosition) {
        let pos = self.document.clamp_position(pos);
        let blocks = self.document.blocks();

        if pos.block_index >= blocks.len() {
            return;
        }

        let block = &blocks[pos.block_index];
        let text = block.to_plain_text();

        if text.is_empty() || pos.offset >= text.len() {
            // Empty block or cursor at end - select nothing
            return;
        }

        // Find word boundaries
        let mut start = pos.offset;
        let mut end = pos.offset;

        // Move start backward to beginning of word
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

        // Move end forward to end of word
        let mut chars = text[end..].char_indices();
        while let Some((_, ch)) = chars.next() {
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                break;
            }
            end = text[..end]
                .chars()
                .next()
                .map(|c| end + c.len_utf8())
                .unwrap_or(end);
        }

        // If we're on whitespace, extend to include it
        if start == end {
            end = text[end..]
                .chars()
                .next()
                .map(|c| end + c.len_utf8())
                .unwrap_or(end);
        }

        let start_pos = DocumentPosition::new(pos.block_index, start);
        let end_pos = DocumentPosition::new(pos.block_index, end);

        self.set_selection(start_pos, end_pos);
        self.cursor = self.document.clamp_position_forward(end_pos);
    }

    /// Select the entire line (block) at the given position
    pub fn select_line_at(&mut self, pos: DocumentPosition) {
        let pos = self.document.clamp_position(pos);
        let blocks = self.document.blocks();

        if pos.block_index >= blocks.len() {
            return;
        }

        let block = &blocks[pos.block_index];
        let start_pos = DocumentPosition::new(pos.block_index, 0);
        let end_pos = DocumentPosition::new(pos.block_index, block.text_len());

        self.set_selection(start_pos, end_pos);
        self.cursor = self.document.clamp_position_forward(end_pos);
    }

    /// Insert text at cursor position
    pub fn insert_text(&mut self, text: &str) -> EditResult {
        if self.document.is_empty() {
            // Create a new paragraph if document is empty
            let block = Block::paragraph(0).with_plain_text(text);
            self.document.add_block(block);
            self.cursor = DocumentPosition::new(0, text.len());
            return Ok(());
        }

        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        // Delete selection first if there is one
        if self.selection.is_some() {
            self.delete_selection()?;
        }

        let offset = self.cursor.offset;

        // Find the inline content and offset within it - need to do this before borrowing
        let (content_idx, content_offset) = {
            let blocks = self.document.blocks();
            let block = &blocks[block_index];
            Self::find_content_at_offset_static(&block.content, offset)
        };

        // Precompute inner indices if we're inside a link to avoid borrow issues
        let inner_within_link: Option<(usize, usize)> = {
            let blocks = self.document.blocks();
            let block = &blocks[block_index];
            if content_idx < block.content.len() {
                if let InlineContent::Link { content, .. } = &block.content[content_idx] {
                    let (inner_idx, inner_off) =
                        Self::find_content_at_offset_static(&content, content_offset);
                    Some((inner_idx, inner_off))
                } else {
                    None
                }
            } else {
                None
            }
        };

        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];

        // Handle different inline content types
        if content_idx >= block.content.len() {
            // Append to end
            block
                .content
                .push(InlineContent::Text(TextRun::plain(text)));
        } else {
            match &mut block.content[content_idx] {
                InlineContent::Text(run) => {
                    run.insert_text(content_offset, text);
                }
                InlineContent::Link { content, .. } => {
                    // Special handling at link edges: if the cursor is exactly at the
                    // start or end of the link, insert outside the link rather than
                    // into its inner content.
                    let link_len: usize = content.iter().map(|c| c.text_len()).sum();

                    if content_offset == 0 {
                        // Insert before the link. If there is a previous text run,
                        // append into it; otherwise insert a fresh text run.
                        if content_idx > 0 {
                            if let InlineContent::Text(prev_run) =
                                &mut block.content[content_idx - 1]
                            {
                                let prev_len = prev_run.len();
                                prev_run.insert_text(prev_len, text);
                            } else {
                                block
                                    .content
                                    .insert(content_idx, InlineContent::Text(TextRun::plain(text)));
                            }
                        } else {
                            block
                                .content
                                .insert(content_idx, InlineContent::Text(TextRun::plain(text)));
                        }
                    } else if content_offset >= link_len {
                        // Insert after the link. If there is a following text run,
                        // prepend into it; otherwise insert a fresh text run.
                        if content_idx + 1 < block.content.len() {
                            if let InlineContent::Text(next_run) =
                                &mut block.content[content_idx + 1]
                            {
                                next_run.insert_text(0, text);
                            } else {
                                block.content.insert(
                                    content_idx + 1,
                                    InlineContent::Text(TextRun::plain(text)),
                                );
                            }
                        } else {
                            block
                                .content
                                .push(InlineContent::Text(TextRun::plain(text)));
                        }
                    } else {
                        // Insert within the link's inner content so typing stays inside the link
                        let (inner_idx, inner_off) =
                            inner_within_link.unwrap_or((content.len(), 0));
                        if inner_idx >= content.len() {
                            content.push(InlineContent::Text(TextRun::plain(text)));
                        } else {
                            match &mut content[inner_idx] {
                                InlineContent::Text(run) => run.insert_text(inner_off, text),
                                _ => content
                                    .insert(inner_idx, InlineContent::Text(TextRun::plain(text))),
                            }
                        }
                    }
                }
                InlineContent::HardBreak => {
                    // Insert relative to the hard break depending on cursor offset:
                    // offset 0 -> before break, offset >= break length -> after break.
                    if content_offset == 0 {
                        block
                            .content
                            .insert(content_idx, InlineContent::Text(TextRun::plain(text)));
                    } else {
                        // After the hard break: merge with following text run when possible.
                        if content_idx + 1 < block.content.len() {
                            match &mut block.content[content_idx + 1] {
                                InlineContent::Text(run) => run.insert_text(0, text),
                                _ => block.content.insert(
                                    content_idx + 1,
                                    InlineContent::Text(TextRun::plain(text)),
                                ),
                            }
                        } else {
                            block
                                .content
                                .push(InlineContent::Text(TextRun::plain(text)));
                        }
                    }
                }
            }
        }

        // Move cursor forward
        self.cursor.offset += text.len();

        Ok(())
    }

    /// Insert a newline at cursor (creates new paragraph or continues list)
    pub fn insert_newline(&mut self) -> EditResult {
        if self.document.is_empty() {
            // Create two paragraphs if document is empty
            self.document.add_block(Block::paragraph(0));
            self.document.add_block(Block::paragraph(0));
            self.cursor = DocumentPosition::new(1, 0);
            return Ok(());
        }

        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        // Delete selection first if there is one
        if self.selection.is_some() {
            self.delete_selection()?;
        }

        let offset = self.cursor.offset;

        // Get block type and check conditions before mut borrow
        let (block_type, is_empty) = {
            let blocks = self.document.blocks();
            let current_block = &blocks[block_index];
            (current_block.block_type.clone(), current_block.is_empty())
        };

        // Check if we're in a list item
        if let BlockType::ListItem {
            ordered,
            number,
            checkbox,
        } = &block_type
        {
            // Check if list item is empty
            if is_empty || offset == 0 {
                // Convert to paragraph to exit list
                let blocks = self.document.blocks_mut();
                blocks[block_index].block_type = BlockType::Paragraph;
                self.cursor.offset = 0;
                return Ok(());
            }

            // Split the current list item at the cursor, preserving link structure
            let right_content = {
                let blocks = self.document.blocks_mut();
                let block = &mut blocks[block_index];
                block.split_content_at(offset)
            };

            // Create new list item with the right-side content
            let new_number = if *ordered { number.unwrap_or(1) + 1 } else { 0 };
            let new_checkbox = if checkbox.is_some() {
                Some(false)
            } else {
                None
            };
            let mut new_item = Block::new(
                0,
                BlockType::ListItem {
                    ordered: *ordered,
                    number: if *ordered { Some(new_number) } else { None },
                    checkbox: new_checkbox,
                },
            );
            new_item.content = right_content;

            self.document.insert_block(block_index + 1, new_item);
            self.cursor = DocumentPosition::new(block_index + 1, 0);

            // If ordered, renumber the new item and all following ordered items to continue the sequence
            if *ordered {
                // Determine the correct number for the current (left) item if missing
                let cur_left_num = number.unwrap_or_else(|| {
                    // Walk backwards to find start of run and compute index
                    let blocks = self.document.blocks();
                    let mut start = block_index;
                    while start > 0 {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[start - 1].block_type
                        {
                            start -= 1;
                        } else {
                            break;
                        }
                    }
                    let first = match blocks[start].block_type {
                        BlockType::ListItem {
                            ordered: true,
                            number,
                            ..
                        } => number.unwrap_or(1),
                        _ => 1,
                    };
                    let idx_in_run = block_index - start;
                    first + idx_in_run as u64
                });

                // Renumber from the newly inserted block onward
                let start_n = cur_left_num + 1;
                self.renumber_ordered_from(block_index + 1, start_n);
            }
        } else {
            // Regular paragraph split: split the block at the cursor, preserving link structure
            let right_content = {
                let blocks = self.document.blocks_mut();
                let block = &mut blocks[block_index];
                block.split_content_at(offset)
            };

            // Create new paragraph with remaining content (right side)
            let mut new_para = Block::paragraph(0);
            new_para.content = right_content;

            self.document.insert_block(block_index + 1, new_para);
            self.cursor = DocumentPosition::new(block_index + 1, 0);
        }

        Ok(())
    }

    /// Insert an explicit hard line break at the current position (within the block)
    pub fn insert_hard_break(&mut self) -> EditResult {
        if self.document.is_empty() {
            let mut block = Block::paragraph(0);
            block.content.push(InlineContent::HardBreak);
            self.document.add_block(block);
            self.cursor = DocumentPosition::new(0, 1);
            return Ok(());
        }

        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        if self.selection.is_some() {
            self.delete_selection()?;
        }

        let offset = self.cursor.offset;
        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];

        let right_content = block.split_content_at(offset);
        block.content.push(InlineContent::HardBreak);
        block.content.extend(right_content);

        self.cursor.offset += 1;

        Ok(())
    }

    /// Delete character before cursor (backspace)
    pub fn delete_backward(&mut self) -> EditResult {
        if self.document.is_empty() {
            return Err(EditError::EmptyDocument);
        }

        // If there's a selection, delete it
        if self.selection.is_some() {
            return self.delete_selection();
        }

        let block_index = self.cursor.block_index;
        let offset = self.cursor.offset;

        if offset == 0 {
            // At start of block - merge with previous block
            if block_index == 0 {
                return Ok(()); // At start of document, nothing to delete
            }

            // Capture types before mutation
            let (prev_type, cur_type) = {
                let blocks = self.document.blocks();
                (
                    blocks[block_index - 1].block_type.clone(),
                    blocks[block_index].block_type.clone(),
                )
            };

            let mut renumber_after: Option<(usize, u64)> = None;

            // Perform merge
            {
                let blocks = self.document.blocks_mut();
                let current_block = blocks.remove(block_index);
                let prev_block = &mut blocks[block_index - 1];
                let prev_len = prev_block.text_len();
                // Merge content
                prev_block.content.extend(current_block.content);
                prev_block.normalize_content();
                self.cursor = DocumentPosition::new(block_index - 1, prev_len);
            }

            // Post-merge renumbering rules for ordered lists
            match (prev_type, cur_type) {
                // Merged two ordered list items: renumber following items to close the gap
                (
                    BlockType::ListItem {
                        ordered: true,
                        number: prev_num,
                        ..
                    },
                    BlockType::ListItem { ordered: true, .. },
                ) => {
                    let start_index = block_index; // after removal, this is the first following block
                    let start_n = prev_num.unwrap_or(1) + 1;
                    renumber_after = Some((start_index, start_n));
                }
                // Merged paragraph into previous ordered item: if next is ordered run, renumber it to continue
                (
                    BlockType::ListItem {
                        ordered: true,
                        number: prev_num,
                        ..
                    },
                    BlockType::Paragraph,
                ) => {
                    let start_index = block_index; // next block after prev
                    let start_n = prev_num.unwrap_or(1) + 1;
                    renumber_after = Some((start_index, start_n));
                }
                // Merged ordered item into previous paragraph: following ordered run should reset to start at 1
                (BlockType::Paragraph, BlockType::ListItem { ordered: true, .. }) => {
                    let start_index = block_index - 0; // after removal, next block index is same value
                    renumber_after = Some((start_index, 1));
                }
                _ => {}
            }

            if let Some((start_idx, start_n)) = renumber_after {
                self.renumber_ordered_from(start_idx, start_n);
            }
        } else {
            // Delete a single character within this block, respecting UTF-8 and nested links
            if let Some(prev_grapheme_start) =
                self.document.previous_grapheme_offset(block_index, offset)
            {
                if prev_grapheme_start < offset {
                    let blocks = self.document.blocks_mut();
                    let block = &mut blocks[block_index];
                    block.delete_text_range(prev_grapheme_start, offset);
                    self.cursor.offset = prev_grapheme_start;
                }
            }
        }
        self.normalize_cursor();
        Ok(())
    }

    /// Delete up to `byte_count` bytes immediately before the cursor (used for IME composition).
    /// Returns `Ok(true)` when any content was removed.
    pub fn delete_backward_bytes(&mut self, mut byte_count: usize) -> Result<bool, EditError> {
        if byte_count == 0 {
            return Ok(false);
        }

        if self.document.is_empty() {
            return Err(EditError::EmptyDocument);
        }

        // Ensure cursor is on a valid grapheme boundary before calculating ranges.
        self.normalize_cursor();

        let original_cursor = self.cursor;
        let mut start = original_cursor;

        {
            let doc = self.document();
            if doc.block_count() == 0 {
                return Ok(false);
            }

            // Clamp to valid block in case document shrank unexpectedly
            if start.block_index >= doc.block_count() {
                start.block_index = doc.block_count() - 1;
                start.offset = doc.blocks()[start.block_index].text_len();
            }

            while byte_count > 0 {
                if start.offset == 0 {
                    if start.block_index == 0 {
                        break;
                    }
                    start.block_index -= 1;
                    start.offset = doc.blocks()[start.block_index].text_len();
                    continue;
                }

                if let Some(prev_offset) =
                    doc.previous_grapheme_offset(start.block_index, start.offset)
                {
                    let removed = start.offset - prev_offset;
                    start.offset = prev_offset;

                    if removed == 0 {
                        break;
                    }

                    if removed > byte_count {
                        byte_count = 0;
                    } else {
                        byte_count -= removed;
                    }
                } else {
                    break;
                }
            }
        }

        if start == original_cursor {
            return Ok(false);
        }

        self.set_selection(start, original_cursor);
        self.delete_selection()?;
        Ok(true)
    }

    /// Delete character at cursor (delete key)
    pub fn delete_forward(&mut self) -> EditResult {
        if self.document.is_empty() {
            return Err(EditError::EmptyDocument);
        }

        // If there's a selection, delete it
        if self.selection.is_some() {
            return self.delete_selection();
        }

        let block_index = self.cursor.block_index;
        let offset = self.cursor.offset;
        let block_count = self.document.block_count();

        if block_index >= block_count {
            return Err(EditError::InvalidBlockIndex);
        }

        let block_len = {
            let blocks = self.document.blocks();
            blocks[block_index].text_len()
        };

        if offset >= block_len {
            if block_index >= block_count - 1 {
                return Ok(()); // At end of document, nothing to delete
            }

            let (cur_type, next_type) = {
                let blocks = self.document.blocks();
                let cur = blocks[block_index].block_type.clone();
                let nxt = blocks[block_index + 1].block_type.clone();
                (cur, nxt)
            };

            {
                let blocks = self.document.blocks_mut();
                let next_block = blocks.remove(block_index + 1);
                let block = &mut blocks[block_index];
                block.content.extend(next_block.content);
                block.normalize_content();
            }

            match (cur_type, next_type) {
                // Current and next were ordered: renumber following items to close the gap
                (
                    BlockType::ListItem {
                        ordered: true,
                        number: cur_num,
                        ..
                    },
                    BlockType::ListItem { ordered: true, .. },
                ) => {
                    let start_index = block_index + 1;
                    let start_n = cur_num.unwrap_or(1) + 1;
                    self.renumber_ordered_from(start_index, start_n);
                }
                // Current paragraph merged with next ordered: following ordered run should reset numbering from 1
                (BlockType::Paragraph, BlockType::ListItem { ordered: true, .. }) => {
                    let start_index = block_index + 1;
                    self.renumber_ordered_from(start_index, 1);
                }
                // Current ordered merged with next paragraph: if there is an ordered run after, continue numbering
                (
                    BlockType::ListItem {
                        ordered: true,
                        number: cur_num,
                        ..
                    },
                    BlockType::Paragraph,
                ) => {
                    let start_index = block_index + 1;
                    self.renumber_ordered_from(start_index, cur_num.unwrap_or(1) + 1);
                }
                _ => {}
            }

            self.normalize_cursor();
            return Ok(());
        }

        let next_grapheme_end = self
            .document
            .next_grapheme_offset(block_index, offset)
            .unwrap_or(offset);

        if next_grapheme_end > offset {
            let blocks = self.document.blocks_mut();
            let block = &mut blocks[block_index];
            block.delete_text_range(offset, next_grapheme_end);
        }

        self.normalize_cursor();
        Ok(())
    }

    /// Delete word before cursor (Alt/Ctrl+Backspace)
    pub fn delete_word_backward(&mut self) -> EditResult {
        if self.document.is_empty() {
            return Err(EditError::EmptyDocument);
        }

        // If there's a selection, delete it
        if self.selection.is_some() {
            return self.delete_selection();
        }

        let from = self.cursor;
        let to = self.word_left_position(from);

        // Nothing to delete
        if to == from {
            return Ok(());
        }

        self.document.delete_range(to, from);
        self.cursor = to;
        self.normalize_cursor();
        self.selection = None;
        Ok(())
    }

    /// Delete word at/after cursor (Alt/Ctrl+Delete)
    pub fn delete_word_forward(&mut self) -> EditResult {
        if self.document.is_empty() {
            return Err(EditError::EmptyDocument);
        }

        // If there's a selection, delete it
        if self.selection.is_some() {
            return self.delete_selection();
        }

        let from = self.cursor;
        let to = self.word_right_position(from);

        // Nothing to delete
        if to == from {
            return Ok(());
        }

        self.document.delete_range(from, to);
        // Cursor stays at original start
        self.normalize_cursor();
        self.selection = None;
        Ok(())
    }

    /// Delete the current selection
    pub fn delete_selection(&mut self) -> EditResult {
        let Some((start, end)) = self.selection else {
            return Ok(());
        };
        // Ensure start <= end in document order
        let (start, end) = if start.block_index < end.block_index
            || (start.block_index == end.block_index && start.offset <= end.offset)
        {
            (start, end)
        } else {
            (end, start)
        };

        // Delegate range deletion to document, which handles intra- and inter-block cases
        self.document.delete_range(start, end);

        self.cursor = start;
        self.selection = None;
        self.normalize_cursor();

        Ok(())
    }

    /// Move cursor left by one character
    pub fn move_cursor_left(&mut self) {
        if self.document.block_count() == 0 {
            self.cursor = DocumentPosition::start();
            self.selection = None;
            return;
        }

        if self.cursor.block_index >= self.document.block_count() {
            self.cursor.block_index = self.document.block_count() - 1;
            let blocks = self.document.blocks();
            self.cursor.offset = blocks[self.cursor.block_index].text_len();
        }

        if self.cursor.offset > 0 {
            self.cursor = self.document.previous_grapheme_position(self.cursor);
        } else if self.cursor.block_index > 0 {
            // Move to end of previous block
            self.cursor.block_index -= 1;
            let blocks = self.document.blocks();
            self.cursor.offset = blocks[self.cursor.block_index].text_len();
        }
        self.normalize_cursor();
        self.selection = None;
    }

    /// Move cursor right by one character
    pub fn move_cursor_right(&mut self) {
        let block_count = self.document.block_count();
        if block_count == 0 {
            self.cursor = DocumentPosition::start();
            self.selection = None;
            return;
        }

        if self.cursor.block_index >= block_count {
            self.cursor.block_index = block_count - 1;
            let blocks = self.document.blocks();
            self.cursor.offset = blocks[self.cursor.block_index].text_len();
        }

        let blocks = self.document.blocks();
        let block_len = blocks[self.cursor.block_index].text_len();

        if self.cursor.offset < block_len {
            self.cursor = self.document.next_grapheme_position(self.cursor);
        } else if self.cursor.block_index < block_count - 1 {
            // Move to start of next block
            self.cursor.block_index += 1;
            self.cursor.offset = 0;
        }
        self.normalize_cursor();
        self.selection = None;
    }

    /// Move cursor up (to previous block)
    pub fn move_cursor_up(&mut self) {
        if self.cursor.block_index > 0 {
            self.cursor.block_index -= 1;
            let blocks = self.document.blocks();
            let new_block_len = blocks[self.cursor.block_index].text_len();
            self.cursor.offset = self.cursor.offset.min(new_block_len);
            self.normalize_cursor();
        }
        self.selection = None;
    }

    /// Move cursor down (to next block)
    pub fn move_cursor_down(&mut self) {
        let block_count = self.document.block_count();
        if block_count == 0 {
            self.cursor = DocumentPosition::start();
            self.selection = None;
            return;
        }
        if self.cursor.block_index + 1 < block_count {
            self.cursor.block_index += 1;
            let blocks = self.document.blocks();
            let new_block_len = blocks[self.cursor.block_index].text_len();
            self.cursor.offset = self.cursor.offset.min(new_block_len);
            self.normalize_cursor();
        }
        self.selection = None;
    }

    /// Move cursor to start of current block
    pub fn move_cursor_to_line_start(&mut self) {
        self.cursor.offset = 0;
        self.normalize_cursor();
        self.selection = None;
    }

    /// Move cursor to end of current block
    pub fn move_cursor_to_line_end(&mut self) {
        let blocks = self.document.blocks();
        if self.cursor.block_index < blocks.len() {
            self.cursor.offset = blocks[self.cursor.block_index].text_len();
            self.normalize_cursor();
        }
        self.selection = None;
    }

    /// Move cursor right by one word
    pub fn move_word_right(&mut self) {
        let new_pos = self.word_right_position(self.cursor);
        self.cursor = new_pos;
        self.selection = None;
    }

    /// Move cursor left by one word
    pub fn move_word_left(&mut self) {
        let new_pos = self.word_left_position(self.cursor);
        self.cursor = new_pos;
        self.selection = None;
    }

    /// Extend selection by moving right by one word
    pub fn move_word_right_extend(&mut self) {
        let new_pos = self.word_right_position(self.cursor);
        if new_pos != self.cursor {
            self.extend_selection_to(new_pos);
        }
    }

    /// Extend selection by moving left by one word
    pub fn move_word_left_extend(&mut self) {
        let new_pos = self.word_left_position(self.cursor);
        if new_pos != self.cursor {
            self.extend_selection_to(new_pos);
        }
    }

    /// Compute next word-right position from a given position
    fn word_right_position(&self, pos: DocumentPosition) -> DocumentPosition {
        let blocks = self.document.blocks();
        if pos.block_index >= blocks.len() {
            return self.document.clamp_position(pos);
        }
        let text = blocks[pos.block_index].to_plain_text();
        let mut i = pos.offset.min(text.len());
        if i >= text.len() {
            if pos.block_index + 1 < blocks.len() {
                return DocumentPosition::new(pos.block_index + 1, 0);
            }
            return self.document.clamp_position(pos);
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
        self.document
            .clamp_position_forward(DocumentPosition::new(pos.block_index, i))
    }

    /// Compute next word-left position from a given position
    fn word_left_position(&self, pos: DocumentPosition) -> DocumentPosition {
        let blocks = self.document.blocks();
        if pos.block_index >= blocks.len() {
            return self.document.clamp_position(pos);
        }
        let text = blocks[pos.block_index].to_plain_text();
        let mut i = pos.offset.min(text.len());
        if i == 0 {
            if pos.block_index > 0 {
                return DocumentPosition::new(
                    pos.block_index - 1,
                    blocks[pos.block_index - 1].text_len(),
                );
            }
            return self.document.clamp_position(pos);
        }
        while i > 0 {
            let (prev_i, ch) = {
                let mut it = text[..i].char_indices();
                let (prev_idx, prev_ch) = it.next_back().unwrap();
                (prev_idx, prev_ch)
            };
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                i = prev_i;
            } else {
                break;
            }
        }
        while i > 0 {
            let (prev_i, ch) = {
                let mut it = text[..i].char_indices();
                let (prev_idx, prev_ch) = it.next_back().unwrap();
                (prev_idx, prev_ch)
            };
            if !(ch.is_whitespace() || ch.is_ascii_punctuation()) {
                i = prev_i;
            } else {
                break;
            }
        }
        self.document
            .clamp_position(DocumentPosition::new(pos.block_index, i))
    }

    // Selection-extending movement methods (for Shift+arrow keys)

    /// Move cursor left by one character, extending selection
    pub fn move_cursor_left_extend(&mut self) {
        self.normalize_cursor();
        let new_pos = if self.document.block_count() == 0 {
            self.cursor
        } else if self.cursor.offset > 0 {
            self.document.previous_grapheme_position(self.cursor)
        } else if self.cursor.block_index > 0 {
            // Move to end of previous block
            let blocks = self.document.blocks();
            DocumentPosition::new(
                self.cursor.block_index - 1,
                blocks[self.cursor.block_index - 1].text_len(),
            )
        } else {
            self.cursor
        };

        if new_pos != self.cursor {
            self.extend_selection_to(new_pos);
        }
    }

    /// Move cursor right by one character, extending selection
    pub fn move_cursor_right_extend(&mut self) {
        self.normalize_cursor();
        let block_count = self.document.block_count();
        if block_count == 0 {
            return;
        }

        let current_index = self.cursor.block_index.min(block_count - 1);
        let blocks = self.document.blocks();
        let block_len = blocks[current_index].text_len();
        let new_pos = if self.cursor.offset < block_len {
            self.document.next_grapheme_position(self.cursor)
        } else if current_index + 1 < block_count {
            // Move to start of next block
            DocumentPosition::new(current_index + 1, 0)
        } else {
            self.cursor
        };

        if new_pos != self.cursor {
            self.extend_selection_to(new_pos);
        }
    }

    /// Move cursor up (to previous block), extending selection
    pub fn move_cursor_up_extend(&mut self) {
        if self.cursor.block_index > 0 {
            let blocks = self.document.blocks();
            let new_block_len = blocks[self.cursor.block_index - 1].text_len();
            let new_pos = DocumentPosition::new(
                self.cursor.block_index - 1,
                self.cursor.offset.min(new_block_len),
            );
            self.extend_selection_to(new_pos);
        }
    }

    /// Move cursor down (to next block), extending selection
    pub fn move_cursor_down_extend(&mut self) {
        let blocks = self.document.blocks();
        if self.cursor.block_index < blocks.len() - 1 {
            let new_block_len = blocks[self.cursor.block_index + 1].text_len();
            let new_pos = DocumentPosition::new(
                self.cursor.block_index + 1,
                self.cursor.offset.min(new_block_len),
            );
            self.extend_selection_to(new_pos);
        }
    }

    /// Move cursor to start of current block, extending selection
    pub fn move_cursor_to_line_start_extend(&mut self) {
        let new_pos = DocumentPosition::new(self.cursor.block_index, 0);
        if new_pos != self.cursor {
            self.extend_selection_to(new_pos);
        }
    }

    /// Move cursor to end of current block, extending selection
    pub fn move_cursor_to_line_end_extend(&mut self) {
        let blocks = self.document.blocks();
        if self.cursor.block_index < blocks.len() {
            let new_pos = DocumentPosition::new(
                self.cursor.block_index,
                blocks[self.cursor.block_index].text_len(),
            );
            if new_pos != self.cursor {
                self.extend_selection_to(new_pos);
            }
        }
    }

    /// Toggle heading level (cycles through plain → H1 → H2 → H3 → plain)
    /// Also removes list status if present
    pub fn toggle_heading(&mut self) -> EditResult {
        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];

        // Cycle through heading levels
        block.block_type = match &block.block_type {
            BlockType::Paragraph => BlockType::Heading { level: 1 },
            BlockType::Heading { level: 1 } => BlockType::Heading { level: 2 },
            BlockType::Heading { level: 2 } => BlockType::Heading { level: 3 },
            BlockType::Heading { level: 3 } => BlockType::Paragraph,
            BlockType::Heading { level } => BlockType::Heading {
                level: (*level % 3) + 1,
            },
            BlockType::ListItem { .. } => BlockType::Heading { level: 1 },
            BlockType::CodeBlock { .. } => BlockType::Heading { level: 1 },
            BlockType::BlockQuote => BlockType::Heading { level: 1 },
        };

        Ok(())
    }

    /// Insert an inline element at the current cursor position
    pub fn insert_inline_at_cursor(&mut self, inline: InlineContent) -> EditResult {
        if self.document.is_empty() {
            let mut block = Block::paragraph(0);
            block.content.push(inline);
            let text_len = block.text_len();
            self.document.add_block(block);
            self.cursor = DocumentPosition::new(0, text_len);
            return Ok(());
        }

        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        // Delete selection first if there is one
        if self.selection.is_some() {
            self.delete_selection()?;
        }

        let offset = self.cursor.offset;
        let (left, right) = {
            let blocks = self.document.blocks();
            let block = &blocks[block_index];
            Self::split_content_at_static(&block.content, offset)
        };

        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];
        block.content = left;
        block.content.push(inline);
        block.content.extend(right);

        // Advance cursor by inserted inline's text length
        let inserted_len = match block.content.get(block.content.len().saturating_sub(1)) {
            Some(InlineContent::Text(run)) => run.len(),
            Some(InlineContent::Link { content, .. }) => content.iter().map(|c| c.text_len()).sum(),
            Some(InlineContent::HardBreak) => 1,
            _ => 0,
        };
        self.cursor.offset = offset + inserted_len;
        self.selection = None;
        Ok(())
    }

    /// Replace current selection with a link (destination + text)
    pub fn replace_selection_with_link(&mut self, destination: &str, text: &str) -> EditResult {
        // Delete selection, which moves cursor to start of selection
        self.delete_selection()?;
        self.insert_link_at_cursor(destination, text)
    }

    /// Insert a link at the cursor
    pub fn insert_link_at_cursor(&mut self, destination: &str, text: &str) -> EditResult {
        let link_inline = InlineContent::Link {
            link: Link {
                destination: destination.to_string(),
                title: None,
            },
            content: vec![InlineContent::Text(TextRun::plain(text))],
        };
        self.insert_inline_at_cursor(link_inline)
    }

    /// Edit an existing link at the given block + inline index
    pub fn edit_link_at(
        &mut self,
        block_index: usize,
        inline_index: usize,
        destination: &str,
        text: &str,
    ) -> EditResult {
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }
        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];
        if inline_index >= block.content.len() {
            return Err(EditError::InvalidPosition);
        }
        if let InlineContent::Link { link, content } = &mut block.content[inline_index] {
            link.destination = destination.to_string();
            *content = vec![InlineContent::Text(TextRun::plain(text))];
            Ok(())
        } else {
            Err(EditError::InvalidPosition)
        }
    }

    /// Remove (unwrap) a link at the given block + inline index, preserving its text content
    pub fn remove_link_at(&mut self, block_index: usize, inline_index: usize) -> EditResult {
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }
        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];
        if inline_index >= block.content.len() {
            return Err(EditError::InvalidPosition);
        }
        if let InlineContent::Link { content, .. } = block.content.remove(inline_index) {
            // Splice inner content in place of the link
            for (i, item) in content.into_iter().enumerate() {
                block.content.insert(inline_index + i, item);
            }
            Ok(())
        } else {
            Err(EditError::InvalidPosition)
        }
    }

    /// Extract plain text for a document range
    pub fn text_in_range(&self, start: DocumentPosition, end: DocumentPosition) -> String {
        let doc = self.document();
        if doc.block_count() == 0 {
            return String::new();
        }
        let mut s = String::new();
        let (mut a, mut b) = (start, end);
        if a.block_index > b.block_index || (a.block_index == b.block_index && a.offset > b.offset)
        {
            std::mem::swap(&mut a, &mut b);
        }
        for bi in a.block_index..=b.block_index {
            let block = &doc.blocks()[bi];
            let text = block.to_plain_text();
            let from = if bi == a.block_index {
                a.offset.min(text.len())
            } else {
                0
            };
            let to = if bi == b.block_index {
                b.offset.min(text.len())
            } else {
                text.len()
            };
            if from < to {
                if !s.is_empty() {
                    s.push_str("\n\n");
                }
                s.push_str(&text[from..to]);
            }
        }
        s
    }

    /// Toggle bold style on the current selection
    pub fn toggle_bold(&mut self) -> EditResult {
        self.toggle_style_attribute(|style| {
            style.bold = !style.bold;
        })
    }

    /// Toggle italic style on the current selection
    pub fn toggle_italic(&mut self) -> EditResult {
        self.toggle_style_attribute(|style| {
            style.italic = !style.italic;
        })
    }

    /// Toggle a style attribute on the current selection
    fn toggle_style_attribute<F>(&mut self, mut apply_style: F) -> EditResult
    where
        F: FnMut(&mut TextStyle),
    {
        let Some((start, end)) = self.selection else {
            return Ok(());
        };

        // Ensure start <= end
        let (start, end) = if start.block_index < end.block_index
            || (start.block_index == end.block_index && start.offset <= end.offset)
        {
            (start, end)
        } else {
            (end, start)
        };

        // Single-block selection
        if start.block_index == end.block_index {
            let block_index = start.block_index;
            if block_index >= self.document.block_count() {
                return Err(EditError::InvalidBlockIndex);
            }

            // Get the content and split it into three parts: before, selected, after
            let (content_before, selected_content, content_after) = {
                let blocks = self.document.blocks();
                let block = &blocks[block_index];
                Self::split_content_for_style(&block.content, start.offset, end.offset)
            };

            // Apply style to the selected content (recursively for nested structures)
            let styled_content = Self::map_style_on_runs(selected_content, &mut apply_style);

            // Reconstruct the block content
            let blocks = self.document.blocks_mut();
            let block = &mut blocks[block_index];
            block.content = content_before;
            block.content.extend(styled_content);
            block.content.extend(content_after);
            return Ok(());
        }

        // Multi-block selection: style tail of start, all middle, head of end
        let blocks_len = self.document.block_count();
        if start.block_index >= blocks_len || end.block_index >= blocks_len {
            return Err(EditError::InvalidBlockIndex);
        }

        // Start block: from start.offset to end of block
        {
            let blocks = self.document.blocks();
            let block = &blocks[start.block_index];
            let block_len = block.text_len();
            let (before, selected, after) =
                Self::split_content_for_style(&block.content, start.offset, block_len);
            let styled = Self::map_style_on_runs(selected, &mut apply_style);
            let blocks = self.document.blocks_mut();
            let block_mut = &mut blocks[start.block_index];
            block_mut.content = before
                .into_iter()
                .chain(styled.into_iter())
                .chain(after.into_iter())
                .collect();
        }

        // Middle blocks
        if end.block_index > start.block_index + 1 {
            for i in (start.block_index + 1)..end.block_index {
                let styled = {
                    let blocks = self.document.blocks();
                    let b = &blocks[i];
                    Self::map_style_on_runs(b.content.clone(), &mut apply_style)
                };
                let blocks = self.document.blocks_mut();
                blocks[i].content = styled;
            }
        }

        // End block: from 0 to end.offset
        {
            let (before, selected, after) = {
                let blocks = self.document.blocks();
                let block = &blocks[end.block_index];
                Self::split_content_for_style(&block.content, 0, end.offset)
            };
            let styled = Self::map_style_on_runs(selected, &mut apply_style);
            let blocks = self.document.blocks_mut();
            let block_mut = &mut blocks[end.block_index];
            block_mut.content = before
                .into_iter()
                .chain(styled.into_iter())
                .chain(after.into_iter())
                .collect();
        }

        Ok(())
    }

    /// Split content into three parts: before selection, within selection, after selection
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
                // Entirely before selection
                before.push(item.clone());
            } else if item_start >= end_offset {
                // Entirely after selection
                after.push(item.clone());
            } else if item_start >= start_offset && item_end <= end_offset {
                // Entirely within selection
                selected.push(item.clone());
            } else {
                // Partially overlaps - need to split
                match item {
                    InlineContent::Text(run) => {
                        let text = &run.text;

                        // Calculate offsets within this run
                        let sel_start_in_run = start_offset.saturating_sub(item_start);
                        let sel_end_in_run = end_offset.saturating_sub(item_start).min(item_len);

                        if sel_start_in_run > 0 {
                            // Part before selection
                            let mut before_run = run.clone();
                            before_run.text = text[..sel_start_in_run].to_string();
                            before.push(InlineContent::Text(before_run));
                        }

                        if sel_end_in_run > sel_start_in_run {
                            // Part in selection
                            let mut selected_run = run.clone();
                            selected_run.text = text[sel_start_in_run..sel_end_in_run].to_string();
                            selected.push(InlineContent::Text(selected_run));
                        }

                        if sel_end_in_run < item_len {
                            // Part after selection
                            let mut after_run = run.clone();
                            after_run.text = text[sel_end_in_run..].to_string();
                            after.push(InlineContent::Text(after_run));
                        }
                    }
                    _ => {
                        // For non-text items, include them in the appropriate section
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

    /// Toggle list status (on/off)
    /// When turning list on, removes heading if present
    pub fn toggle_list(&mut self) -> EditResult {
        let block_count = self.document.block_count();
        if block_count == 0 {
            return Err(EditError::InvalidBlockIndex);
        }

        let selection_blocks = self.collect_selection_block_indices();
        if !selection_blocks.is_empty() {
            let blocks_snapshot = self.document.blocks();
            let all_plain_bullets = selection_blocks.iter().all(|&idx| {
                matches!(
                    blocks_snapshot.get(idx).map(|b| &b.block_type),
                    Some(BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: None,
                    })
                )
            });

            if all_plain_bullets {
                return self.set_block_type(BlockType::Paragraph);
            }

            return self.set_block_type(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            });
        }

        let block_index = self.cursor.block_index;
        if block_index >= block_count {
            return Err(EditError::InvalidBlockIndex);
        }

        // Special case: If currently in an ordered list, convert the entire adjacent
        // ordered list run to unordered bullets (switching list types).
        let convert_ordered_to_bullets = {
            let blocks = self.document.blocks();
            matches!(
                blocks.get(block_index).map(|b| &b.block_type),
                Some(BlockType::ListItem { ordered: true, .. })
            )
        };

        if convert_ordered_to_bullets {
            // Find contiguous ordered list run around the current block
            let (start, end) = {
                let blocks = self.document.blocks();
                let mut start = block_index;
                while start > 0 {
                    if let BlockType::ListItem { ordered: true, .. } = blocks[start - 1].block_type
                    {
                        start -= 1;
                    } else {
                        break;
                    }
                }
                let mut end = block_index;
                while end + 1 < blocks.len() {
                    if let BlockType::ListItem { ordered: true, .. } = blocks[end + 1].block_type {
                        end += 1;
                    } else {
                        break;
                    }
                }
                (start, end)
            };

            let blocks = self.document.blocks_mut();
            for i in start..=end {
                blocks[i].block_type = BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: None,
                };
            }
            self.trigger_paragraph_change();
            return Ok(());
        }

        // Special case: converting a checklist run to plain bullets
        let convert_checklist_to_bullets = {
            let blocks = self.document.blocks();
            matches!(
                blocks.get(block_index).map(|b| &b.block_type),
                Some(BlockType::ListItem {
                    ordered: false,
                    checkbox: Some(_),
                    ..
                })
            )
        };

        if convert_checklist_to_bullets {
            let (start, end) = {
                let blocks = self.document.blocks();
                let mut start = block_index;
                while start > 0 {
                    if let BlockType::ListItem {
                        ordered: false,
                        checkbox: Some(_),
                        ..
                    } = blocks[start - 1].block_type
                    {
                        start -= 1;
                    } else {
                        break;
                    }
                }
                let mut end = block_index;
                while end + 1 < blocks.len() {
                    if let BlockType::ListItem {
                        ordered: false,
                        checkbox: Some(_),
                        ..
                    } = blocks[end + 1].block_type
                    {
                        end += 1;
                    } else {
                        break;
                    }
                }
                (start, end)
            };

            let blocks = self.document.blocks_mut();
            for i in start..=end {
                blocks[i].block_type = BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: None,
                };
            }
            self.trigger_paragraph_change();
            return Ok(());
        }

        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];

        // Toggle bullet list on/off for non-ordered contexts
        block.block_type = match &block.block_type {
            BlockType::ListItem { ordered: false, .. } => BlockType::Paragraph,
            BlockType::ListItem { ordered: true, .. } => BlockType::ListItem {
                // Already handled above (convert range) but keep safe fallback
                ordered: false,
                number: None,
                checkbox: None,
            },
            BlockType::Paragraph | BlockType::Heading { .. } => BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            },
            BlockType::CodeBlock { .. } => BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            },
            BlockType::BlockQuote => BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            },
        };

        self.trigger_paragraph_change();
        Ok(())
    }

    /// Toggle checklist status (on/off) for current block/run
    /// Converts ordered runs to checklists, bullets to checklists, and removes checklist state when toggled off.
    pub fn toggle_checklist(&mut self) -> EditResult {
        let block_count = self.document.block_count();
        if block_count == 0 {
            return Err(EditError::InvalidBlockIndex);
        }

        let selection_blocks = self.collect_selection_block_indices();
        if !selection_blocks.is_empty() {
            let blocks_snapshot = self.document.blocks();
            let all_checklists = selection_blocks.iter().all(|&idx| {
                matches!(
                    blocks_snapshot.get(idx).map(|b| &b.block_type),
                    Some(BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: Some(_),
                    })
                )
            });

            if all_checklists {
                return self.set_block_type(BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: None,
                });
            }

            return self.set_block_type(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(false),
            });
        }

        let block_index = self.cursor.block_index;
        if block_index >= block_count {
            return Err(EditError::InvalidBlockIndex);
        }

        let current_type = {
            let blocks = self.document.blocks();
            blocks[block_index].block_type.clone()
        };

        match current_type {
            BlockType::ListItem { ordered: true, .. } => {
                // Convert contiguous ordered run to checklist items
                let (start, end) = {
                    let blocks = self.document.blocks();
                    let mut start = block_index;
                    while start > 0 {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[start - 1].block_type
                        {
                            start -= 1;
                        } else {
                            break;
                        }
                    }
                    let mut end = block_index;
                    while end + 1 < blocks.len() {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[end + 1].block_type
                        {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    (start, end)
                };

                let blocks = self.document.blocks_mut();
                for i in start..=end {
                    blocks[i].block_type = BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: Some(false),
                    };
                }
                self.trigger_paragraph_change();
                Ok(())
            }
            BlockType::ListItem {
                ordered: false,
                checkbox: Some(_),
                ..
            } => {
                // Toggling off a checklist item returns it to a paragraph
                let blocks = self.document.blocks_mut();
                blocks[block_index].block_type = BlockType::Paragraph;
                self.trigger_paragraph_change();
                Ok(())
            }
            BlockType::ListItem {
                ordered: false,
                checkbox: None,
                ..
            } => {
                // Convert contiguous bullet run to checklist items
                let (start, end) = {
                    let blocks = self.document.blocks();
                    let mut start = block_index;
                    while start > 0 {
                        if let BlockType::ListItem {
                            ordered: false,
                            checkbox: None,
                            ..
                        } = blocks[start - 1].block_type
                        {
                            start -= 1;
                        } else {
                            break;
                        }
                    }
                    let mut end = block_index;
                    while end + 1 < blocks.len() {
                        if let BlockType::ListItem {
                            ordered: false,
                            checkbox: None,
                            ..
                        } = blocks[end + 1].block_type
                        {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    (start, end)
                };

                let blocks = self.document.blocks_mut();
                for i in start..=end {
                    blocks[i].block_type = BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: Some(false),
                    };
                }
                self.trigger_paragraph_change();
                Ok(())
            }
            BlockType::Paragraph
            | BlockType::Heading { .. }
            | BlockType::CodeBlock { .. }
            | BlockType::BlockQuote => {
                let blocks = self.document.blocks_mut();
                blocks[block_index].block_type = BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: Some(false),
                };
                self.trigger_paragraph_change();
                Ok(())
            }
        }
    }

    /// Toggle the checkmark state for a specific checklist block.
    pub fn toggle_checkmark_at(&mut self, block_index: usize) -> Result<bool, EditError> {
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }
        let blocks = self.document.blocks_mut();
        if let BlockType::ListItem {
            checkbox: Some(state),
            ..
        } = &mut blocks[block_index].block_type
        {
            *state = !*state;
            return Ok(true);
        }
        Ok(false)
    }

    /// Toggle the checkmark state for the block at the current cursor position.
    pub fn toggle_current_checkmark(&mut self) -> Result<bool, EditError> {
        let block_index = self.cursor.block_index;
        self.toggle_checkmark_at(block_index)
    }

    fn ordered_start_number_for_selection(&self, selection_blocks: &[usize]) -> u64 {
        if selection_blocks.is_empty() {
            return 1;
        }

        let blocks = self.document.blocks();
        if blocks.is_empty() {
            return 1;
        }

        let first_idx = selection_blocks[0].min(blocks.len() - 1);

        if let Some(BlockType::ListItem {
            ordered: true,
            number,
            ..
        }) = blocks.get(first_idx).map(|b| &b.block_type)
        {
            return number.unwrap_or(1);
        }

        if first_idx == 0 {
            return 1;
        }

        if let Some(BlockType::ListItem { ordered: true, .. }) =
            blocks.get(first_idx - 1).map(|b| &b.block_type)
        {
            let mut run_start = first_idx - 1;
            while run_start > 0 {
                if let BlockType::ListItem { ordered: true, .. } = blocks[run_start - 1].block_type
                {
                    run_start -= 1;
                } else {
                    break;
                }
            }

            let first_number = match blocks[run_start].block_type {
                BlockType::ListItem {
                    ordered: true,
                    number,
                    ..
                } => number.unwrap_or(1),
                _ => 1,
            };
            let run_len = first_idx - run_start;
            return first_number + run_len as u64;
        }

        1
    }

    /// Toggle ordered list (on/off) and handle conversion from bullets when applicable.
    /// Special case: If currently in an unordered list, convert all adjacent bullet
    /// list items into ordered list items with sequential numbering starting at 1.
    pub fn toggle_ordered_list(&mut self) -> EditResult {
        let block_count = self.document.block_count();
        if block_count == 0 {
            return Err(EditError::InvalidBlockIndex);
        }

        let selection_blocks = self.collect_selection_block_indices();
        if !selection_blocks.is_empty() {
            let blocks_snapshot = self.document.blocks();
            let all_ordered = selection_blocks.iter().all(|&idx| {
                matches!(
                    blocks_snapshot.get(idx).map(|b| &b.block_type),
                    Some(BlockType::ListItem { ordered: true, .. })
                )
            });

            if all_ordered {
                let result = self.set_block_type(BlockType::Paragraph);
                if result.is_ok() {
                    if let Some(&last_idx) = selection_blocks.last() {
                        if last_idx + 1 < block_count {
                            self.renumber_ordered_from(last_idx + 1, 1);
                        }
                    }
                }
                return result;
            }

            let start_number = self.ordered_start_number_for_selection(&selection_blocks);
            return self.set_block_type(BlockType::ListItem {
                ordered: true,
                number: Some(start_number),
                checkbox: None,
            });
        }

        let block_index = self.cursor.block_index;
        if block_index >= block_count {
            return Err(EditError::InvalidBlockIndex);
        }

        let current_type = {
            let blocks = self.document.blocks();
            blocks[block_index].block_type.clone()
        };

        match current_type {
            BlockType::ListItem { ordered: true, .. } => {
                // We are inside an ordered run. Capture run bounds first.
                let (run_start, run_end, _first_num) = {
                    let blocks = self.document.blocks();
                    // Find run start
                    let mut start = block_index;
                    while start > 0 {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[start - 1].block_type
                        {
                            start -= 1;
                        } else {
                            break;
                        }
                    }
                    // Find run end
                    let mut end = block_index;
                    while end + 1 < blocks.len() {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[end + 1].block_type
                        {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    // Determine starting number (default 1)
                    let first_num = match blocks[start].block_type {
                        BlockType::ListItem {
                            ordered: true,
                            number,
                            ..
                        } => number.unwrap_or(1),
                        _ => 1,
                    };
                    (start, end, first_num)
                };

                // Turn off ordered list for current block (becomes paragraph)
                {
                    let blocks = self.document.blocks_mut();
                    blocks[block_index].block_type = BlockType::Paragraph;
                }

                // Renumber the following part of the run to start from 1 (new list)
                if block_index + 1 <= run_end {
                    let mut n: u64 = 1;
                    let blocks = self.document.blocks_mut();
                    for i in (block_index + 1)..=run_end {
                        if let BlockType::ListItem { ordered: true, .. } = blocks[i].block_type {
                            blocks[i].block_type = BlockType::ListItem {
                                ordered: true,
                                number: Some(n),
                                checkbox: None,
                            };
                            n += 1;
                        } else {
                            break;
                        }
                    }
                }
                self.trigger_paragraph_change();
                Ok(())
            }
            BlockType::ListItem { ordered: false, .. } => {
                // Convert adjacent bullets to ordered list with numbering
                let (start, end) = {
                    let blocks = self.document.blocks();
                    let mut start = block_index;
                    while start > 0 {
                        if let BlockType::ListItem { ordered: false, .. } =
                            blocks[start - 1].block_type
                        {
                            start -= 1;
                        } else {
                            break;
                        }
                    }
                    let mut end = block_index;
                    while end + 1 < blocks.len() {
                        if let BlockType::ListItem { ordered: false, .. } =
                            blocks[end + 1].block_type
                        {
                            end += 1;
                        } else {
                            break;
                        }
                    }
                    (start, end)
                };

                let mut num = 1u64;
                let blocks = self.document.blocks_mut();
                for i in start..=end {
                    blocks[i].block_type = BlockType::ListItem {
                        ordered: true,
                        number: Some(num),
                        checkbox: None,
                    };
                    num += 1;
                }
                self.trigger_paragraph_change();
                Ok(())
            }
            BlockType::Paragraph
            | BlockType::Heading { .. }
            | BlockType::CodeBlock { .. }
            | BlockType::BlockQuote => {
                // Determine neighbors to decide numbering and renumber following run
                let (prev_is_ord, next_is_ord) = {
                    let blocks = self.document.blocks();
                    let prev = if block_index > 0 {
                        matches!(
                            blocks[block_index - 1].block_type,
                            BlockType::ListItem { ordered: true, .. }
                        )
                    } else {
                        false
                    };
                    let next = if block_index + 1 < blocks.len() {
                        matches!(
                            blocks[block_index + 1].block_type,
                            BlockType::ListItem { ordered: true, .. }
                        )
                    } else {
                        false
                    };
                    (prev, next)
                };

                // Compute the number for the current item
                let current_number = if prev_is_ord {
                    // Determine previous run bounds and last number
                    let (prev_start, prev_end, prev_first) = {
                        let blocks = self.document.blocks();
                        // prev_end is block_index - 1 and is ordered
                        let mut start = block_index - 1;
                        while start > 0 {
                            if let BlockType::ListItem { ordered: true, .. } =
                                blocks[start - 1].block_type
                            {
                                start -= 1;
                            } else {
                                break;
                            }
                        }
                        let first = match blocks[start].block_type {
                            BlockType::ListItem {
                                ordered: true,
                                number,
                                ..
                            } => number.unwrap_or(1),
                            _ => 1,
                        };
                        (start, block_index - 1, first)
                    };
                    let prev_len = prev_end - prev_start + 1;
                    Some(prev_first + prev_len as u64)
                } else {
                    Some(1)
                };

                // Set current block to ordered with computed number
                {
                    let blocks = self.document.blocks_mut();
                    blocks[block_index].block_type = BlockType::ListItem {
                        ordered: true,
                        number: current_number,
                        checkbox: None,
                    };
                }

                // If there is a following ordered run, renumber it to continue
                if next_is_ord {
                    // Find following run end
                    let (next_start, next_end) = {
                        let blocks = self.document.blocks();
                        let start = block_index + 1; // guaranteed ordered
                        let mut end = start;
                        while end + 1 < blocks.len() {
                            if let BlockType::ListItem { ordered: true, .. } =
                                blocks[end + 1].block_type
                            {
                                end += 1;
                            } else {
                                break;
                            }
                        }
                        (start, end)
                    };

                    // Start numbering from current_number + 1
                    let mut n = current_number.unwrap_or(1) + 1;
                    let blocks = self.document.blocks_mut();
                    for i in next_start..=next_end {
                        blocks[i].block_type = BlockType::ListItem {
                            ordered: true,
                            number: Some(n),
                            checkbox: None,
                        };
                        n += 1;
                    }
                }
                self.trigger_paragraph_change();
                Ok(())
            }
        }
    }

    /// Toggle quote status (on/off) for current block
    /// If current block is a quote, switch to paragraph; otherwise set to quote
    pub fn toggle_quote(&mut self) -> EditResult {
        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        let blocks = self.document.blocks_mut();
        blocks[block_index].block_type = match blocks[block_index].block_type {
            BlockType::BlockQuote => BlockType::Paragraph,
            _ => BlockType::BlockQuote,
        };

        self.trigger_paragraph_change();
        Ok(())
    }

    /// Toggle code block status (on/off) for current block
    /// If current block is a code block, switch to paragraph; otherwise set to code block (no language)
    pub fn toggle_code_block(&mut self) -> EditResult {
        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        let blocks = self.document.blocks_mut();
        blocks[block_index].block_type = match &blocks[block_index].block_type {
            BlockType::CodeBlock { .. } => BlockType::Paragraph,
            _ => BlockType::CodeBlock { language: None },
        };

        self.trigger_paragraph_change();
        Ok(())
    }

    /// Set the block type for the current block or all blocks covered by the selection.
    pub fn set_block_type(&mut self, block_type: BlockType) -> EditResult {
        let block_count = self.document.block_count();
        if block_count == 0 {
            return Err(EditError::InvalidBlockIndex);
        }

        let mut target_blocks = self.collect_selection_block_indices();
        if target_blocks.is_empty() {
            let cursor_index = self.cursor.block_index;
            if cursor_index >= block_count {
                return Err(EditError::InvalidBlockIndex);
            }
            target_blocks.push(cursor_index);
        }

        target_blocks.sort_unstable();
        target_blocks.dedup();

        if target_blocks.iter().any(|&idx| idx >= block_count) {
            return Err(EditError::InvalidBlockIndex);
        }

        let ordered_conversion = matches!(block_type, BlockType::ListItem { ordered: true, .. });
        let ordered_start_number = if let BlockType::ListItem {
            ordered: true,
            number,
            ..
        } = &block_type
        {
            number.unwrap_or(1)
        } else {
            0
        };

        let new_types: Vec<BlockType> = (0..target_blocks.len())
            .map(|i| match &block_type {
                BlockType::Paragraph => BlockType::Paragraph,
                BlockType::Heading { level } => BlockType::Heading { level: *level },
                BlockType::BlockQuote => BlockType::BlockQuote,
                BlockType::CodeBlock { language } => BlockType::CodeBlock {
                    language: language.clone(),
                },
                BlockType::ListItem {
                    ordered, checkbox, ..
                } => {
                    if *ordered {
                        BlockType::ListItem {
                            ordered: true,
                            number: Some(ordered_start_number + i as u64),
                            checkbox: *checkbox,
                        }
                    } else {
                        BlockType::ListItem {
                            ordered: false,
                            number: None,
                            checkbox: *checkbox,
                        }
                    }
                }
            })
            .collect();

        {
            let blocks = self.document.blocks_mut();
            for (idx, new_type) in target_blocks.iter().copied().zip(new_types.into_iter()) {
                blocks[idx].block_type = new_type;
            }
        }

        if ordered_conversion {
            if let Some(last_idx) = target_blocks.last().copied() {
                if last_idx + 1 < block_count {
                    let next_number = ordered_start_number + target_blocks.len() as u64;
                    self.renumber_ordered_from(last_idx + 1, next_number);
                }
            }
        }

        self.trigger_paragraph_change();
        Ok(())
    }

    /// Toggle code style on the current selection
    pub fn toggle_code(&mut self) -> EditResult {
        self.toggle_style_attribute(|style| {
            style.code = !style.code;
        })
    }

    /// Toggle strikethrough style on the current selection
    pub fn toggle_strikethrough(&mut self) -> EditResult {
        self.toggle_style_attribute(|style| {
            style.strikethrough = !style.strikethrough;
        })
    }

    /// Toggle underline style on the current selection
    pub fn toggle_underline(&mut self) -> EditResult {
        self.toggle_style_attribute(|style| {
            style.underline = !style.underline;
        })
    }

    /// Toggle highlight style on the current selection
    pub fn toggle_highlight(&mut self) -> EditResult {
        self.toggle_style_attribute(|style| {
            style.highlight = !style.highlight;
        })
    }

    /// Clear all inline formatting on the current selection
    /// Removes bold, italic, code, strikethrough, underline, and highlight
    pub fn clear_formatting(&mut self) -> EditResult {
        let Some((start, end)) = self.selection else {
            return Ok(());
        };

        // Ensure start <= end
        let (start, end) = if start.block_index < end.block_index
            || (start.block_index == end.block_index && start.offset <= end.offset)
        {
            (start, end)
        } else {
            (end, start)
        };

        // Single-block selection case
        if start.block_index == end.block_index {
            let block_index = start.block_index;
            if block_index >= self.document.block_count() {
                return Err(EditError::InvalidBlockIndex);
            }
            let (before, selected, after) = {
                let blocks = self.document.blocks();
                let block = &blocks[block_index];
                Self::split_content_for_style(&block.content, start.offset, end.offset)
            };
            let mut clear = |style: &mut TextStyle| {
                *style = TextStyle::default();
            };
            let cleared = Self::map_style_on_runs(selected, &mut clear);
            let blocks = self.document.blocks_mut();
            let block_mut = &mut blocks[block_index];
            block_mut.content = before
                .into_iter()
                .chain(cleared.into_iter())
                .chain(after.into_iter())
                .collect();
            return Ok(());
        }

        // Multi-block selection: clear tail of start, all middle, head of end
        let blocks_len = self.document.block_count();
        if start.block_index >= blocks_len || end.block_index >= blocks_len {
            return Err(EditError::InvalidBlockIndex);
        }

        let mut clear = |style: &mut TextStyle| {
            *style = TextStyle::default();
        };

        // Start block
        {
            let (before, selected, after) = {
                let blocks = self.document.blocks();
                let block = &blocks[start.block_index];
                let len = block.text_len();
                Self::split_content_for_style(&block.content, start.offset, len)
            };
            let cleared = Self::map_style_on_runs(selected, &mut clear);
            let blocks = self.document.blocks_mut();
            let block_mut = &mut blocks[start.block_index];
            block_mut.content = before
                .into_iter()
                .chain(cleared.into_iter())
                .chain(after.into_iter())
                .collect();
        }

        // Middle blocks
        if end.block_index > start.block_index + 1 {
            for i in (start.block_index + 1)..end.block_index {
                let cleared_vec = {
                    let blocks = self.document.blocks();
                    let b = &blocks[i];
                    Self::map_style_on_runs(b.content.clone(), &mut clear)
                };
                let blocks = self.document.blocks_mut();
                blocks[i].content = cleared_vec;
            }
        }

        // End block
        {
            let (before, selected, after) = {
                let blocks = self.document.blocks();
                let block = &blocks[end.block_index];
                Self::split_content_for_style(&block.content, 0, end.offset)
            };
            let cleared = Self::map_style_on_runs(selected, &mut clear);
            let blocks = self.document.blocks_mut();
            let block_mut = &mut blocks[end.block_index];
            block_mut.content = before
                .into_iter()
                .chain(cleared.into_iter())
                .chain(after.into_iter())
                .collect();
        }

        Ok(())
    }

    /// Get the selected text as plain text
    pub fn get_selection_text(&self) -> String {
        let Some((start, end)) = self.selection else {
            return String::new();
        };

        // Ensure start <= end
        let (start, end) = if start.block_index < end.block_index
            || (start.block_index == end.block_index && start.offset <= end.offset)
        {
            (start, end)
        } else {
            (end, start)
        };

        if start.block_index == end.block_index {
            // Selection within single block
            let blocks = self.document.blocks();
            if start.block_index >= blocks.len() {
                return String::new();
            }
            let block = &blocks[start.block_index];
            let text = block.to_plain_text();
            if start.offset < text.len() {
                let end_offset = end.offset.min(text.len());
                return text[start.offset..end_offset].to_string();
            }
        } else {
            // Selection across multiple blocks
            let blocks = self.document.blocks();
            let mut result = String::new();

            for block_idx in start.block_index..=end.block_index.min(blocks.len() - 1) {
                let block = &blocks[block_idx];
                let text = block.to_plain_text();

                if block_idx == start.block_index {
                    // First block - from start.offset to end
                    result.push_str(&text[start.offset..]);
                } else if block_idx == end.block_index {
                    // Last block - from beginning to end.offset
                    let end_offset = end.offset.min(text.len());
                    result.push_str(&text[..end_offset]);
                } else {
                    // Middle block - entire text
                    result.push_str(&text);
                }

                // Add newline between blocks (except after the last one)
                if block_idx < end.block_index {
                    result.push('\n');
                }
            }

            return result;
        }

        String::new()
    }

    /// Cut the selected text (copy and delete)
    pub fn cut(&mut self) -> Result<String, EditError> {
        let text = self.get_selection_text();
        if !text.is_empty() {
            self.delete_selection()?;
        }
        Ok(text)
    }

    /// Copy the selected text
    pub fn copy(&self) -> String {
        self.get_selection_text()
    }

    /// Paste text at cursor position (or replace selection)
    pub fn paste(&mut self, text: &str) -> EditResult {
        if let Some((start, end)) = self.selection {
            // Replace selection using document-level range replace to support multi-paragraph pastes
            self.document.replace_range(start, end, text);
            // Position cursor immediately after the inserted text
            // If multiple paragraphs were inserted, move to start of the last inserted paragraph.
            let paragraphs: Vec<&str> = text.split("\n\n").collect();
            let insert_block = start
                .block_index
                .min(self.document.block_count().saturating_sub(1));
            let (block_index, offset) = if paragraphs.len() <= 1 {
                // Single paragraph inserted into existing block at start.offset
                let inserted_len = paragraphs.first().map(|s| s.len()).unwrap_or(0);
                let left_len = start
                    .offset
                    .min(self.document.blocks()[insert_block].text_len());
                (insert_block, left_len + inserted_len)
            } else {
                // Multiple paragraphs: last inserted paragraph is placed in a new block
                let last_block = (insert_block + paragraphs.len() - 1)
                    .min(self.document.block_count().saturating_sub(1));
                let last_len = paragraphs.last().map(|s| s.len()).unwrap_or(0);
                (last_block, last_len)
            };
            let block_len = self.document.blocks()[block_index].text_len();
            self.cursor = DocumentPosition::new(block_index, min(offset, block_len));
            self.selection = None;
            Ok(())
        } else {
            // Insert at cursor position
            self.insert_text(text)
        }
    }

    /// Find the content element and offset within it for a given block offset (static version)
    fn find_content_at_offset_static(content: &[InlineContent], offset: usize) -> (usize, usize) {
        let mut current_offset = 0;

        for (idx, item) in content.iter().enumerate() {
            let item_len = item.text_len();
            // Use >= so that cursor at end of a run can still delete backward
            if current_offset + item_len >= offset {
                return (idx, offset - current_offset);
            }
            current_offset += item_len;
        }

        // Past the end - return position after last element
        (content.len(), 0)
    }

    /// Find the content element and offset within it for a given block offset
    fn find_content_at_offset(&self, content: &[InlineContent], offset: usize) -> (usize, usize) {
        Self::find_content_at_offset_static(content, offset)
    }

    /// Split content at a given offset (static version)
    fn split_content_at_static(
        content: &[InlineContent],
        offset: usize,
    ) -> (Vec<InlineContent>, Vec<InlineContent>) {
        let (idx, content_offset) = Self::find_content_at_offset_static(content, offset);

        let mut left = content[..idx].to_vec();
        let mut right = content[idx..].to_vec();

        // Handle split within a text run
        if idx < content.len() {
            if let Some(InlineContent::Text(run)) = content.get(idx) {
                if content_offset > 0 {
                    if content_offset == run.len() {
                        // Cursor at end of run - entire run goes to left
                        left.push(InlineContent::Text(run.clone()));
                        right.remove(0);
                    } else if content_offset < run.len() {
                        // Cursor in middle of run - split it
                        let (left_run, right_run) = run.split_at(content_offset);
                        left.push(InlineContent::Text(left_run));
                        right.remove(0);
                        right.insert(0, InlineContent::Text(right_run));
                    }
                }
            }
        }

        (left, right)
    }

    /// Split content at a given offset
    fn split_content_at(
        &self,
        content: &[InlineContent],
        offset: usize,
    ) -> (Vec<InlineContent>, Vec<InlineContent>) {
        Self::split_content_at_static(content, offset)
    }

    /// Recursively apply a style-mapping function to all text runs in a vector of inline content
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
                InlineContent::Link { link, content } => {
                    let mapped = Self::map_style_on_runs(content, apply);
                    InlineContent::Link {
                        link,
                        content: mapped,
                    }
                }
                other => other,
            })
            .collect()
    }
}

impl Default for StructuredEditor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_text() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        assert_eq!(editor.document().to_plain_text(), "Hello");
        assert_eq!(editor.cursor().offset, 5);
    }

    #[test]
    fn test_insert_text_multiple() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.insert_text(" world").unwrap();
        assert_eq!(editor.document().to_plain_text(), "Hello world");
    }

    #[test]
    fn test_delete_backward() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.delete_backward().unwrap();
        assert_eq!(editor.document().to_plain_text(), "Hell");
        assert_eq!(editor.cursor().offset, 4);
    }

    #[test]
    fn cursor_respects_grapheme_clusters() {
        let mut editor = StructuredEditor::new();
        let flag = "\u{1F1FA}\u{1F1F8}";

        editor.insert_text(flag).unwrap();
        assert_eq!(editor.cursor().offset, flag.len());

        editor.set_cursor(DocumentPosition::new(0, flag.len() / 2));
        assert_eq!(
            editor.cursor().offset,
            0,
            "cursor should snap to grapheme start"
        );

        editor.move_cursor_right();
        assert_eq!(editor.cursor().offset, flag.len());

        editor.move_cursor_left();
        assert_eq!(editor.cursor().offset, 0);
    }

    #[test]
    fn deletion_respects_grapheme_clusters() {
        let mut editor = StructuredEditor::new();
        let flag = "\u{1F1FA}\u{1F1F8}";

        editor.insert_text(flag).unwrap();
        editor.set_cursor(DocumentPosition::new(0, flag.len()));
        editor.delete_backward().unwrap();
        assert_eq!(editor.document().to_plain_text(), "");
        assert_eq!(editor.cursor().offset, 0);

        editor.insert_text(flag).unwrap();
        editor.set_cursor(DocumentPosition::new(0, 0));
        editor.delete_forward().unwrap();
        assert_eq!(editor.document().to_plain_text(), "");
        assert_eq!(editor.cursor().offset, 0);
    }

    #[test]
    fn selection_respects_grapheme_clusters() {
        let mut editor = StructuredEditor::new();
        let flag = "\u{1F1FA}\u{1F1F8}";

        editor.insert_text(flag).unwrap();
        editor.move_cursor_to_line_start();
        editor.move_cursor_right_extend();

        let (start, end) = editor
            .selection()
            .expect("selection should be created when extending");
        assert_eq!(start.offset, 0);
        assert_eq!(end.offset, flag.len());
        assert_eq!(editor.cursor().offset, flag.len());
    }

    #[test]
    fn test_insert_newline() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("World").unwrap();

        assert_eq!(editor.document().block_count(), 2);
        assert_eq!(editor.cursor().block_index, 1);
    }

    #[test]
    fn test_insert_hard_break_in_text() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("HelloWorld").unwrap();
        editor.set_cursor(DocumentPosition::new(0, 5));

        editor.insert_hard_break().unwrap();

        let block = &editor.document().blocks()[0];
        assert_eq!(block.to_plain_text(), "Hello\nWorld");
        assert!(matches!(block.content[1], InlineContent::HardBreak));
        assert_eq!(editor.cursor(), DocumentPosition::new(0, 6));
    }

    #[test]
    fn test_insert_text_after_trailing_hard_break_appends() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.insert_hard_break().unwrap();

        editor.insert_text("World").unwrap();

        let block = &editor.document().blocks()[0];
        assert_eq!(block.content.len(), 3, "Expected text, break, text runs");
        assert!(
            matches!(block.content[1], InlineContent::HardBreak),
            "Second inline should remain the hard break"
        );
        assert!(
            matches!(block.content[2], InlineContent::Text(_)),
            "Inserted text should appear after the hard break"
        );
        assert_eq!(block.to_plain_text(), "Hello\nWorld");
        assert_eq!(
            editor.cursor(),
            DocumentPosition::new(0, block.text_len()),
            "Cursor should advance past inserted text"
        );
    }

    #[test]
    fn test_cursor_movement() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();

        editor.move_cursor_left();
        assert_eq!(editor.cursor().offset, 4);

        editor.move_cursor_to_line_start();
        assert_eq!(editor.cursor().offset, 0);

        editor.move_cursor_to_line_end();
        assert_eq!(editor.cursor().offset, 5);
    }

    #[test]
    fn test_insert_text_inside_link() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("ab").unwrap();
        editor.insert_link_at_cursor("dest", "XY").unwrap();
        editor.insert_text("cd").unwrap();

        // Place caret between X and Y inside the link
        editor.set_cursor(DocumentPosition::new(0, 3));
        editor.insert_text("!").unwrap();

        assert_eq!(editor.document().to_plain_text(), "abX!Ycd");

        // Ensure the exclamation mark is inside the link, not outside
        let block = &editor.document().blocks()[0];
        // Content should be: Text("ab"), Link("X!Y"), Text("cd")
        assert!(matches!(block.content[0], InlineContent::Text(_)));
        if let InlineContent::Link { content, .. } = &block.content[1] {
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "X!Y");
        } else {
            panic!("Expected a link at index 1");
        }
    }

    #[test]
    fn test_insert_text_at_start_of_link_inserts_before() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("ab").unwrap();
        editor.insert_link_at_cursor("dest", "XY").unwrap();
        editor.insert_text("cd").unwrap();

        // Caret at the very start of the link (between b and X): ab|XYcd => offset 2
        editor.set_cursor(DocumentPosition::new(0, 2));
        editor.insert_text("!").unwrap();

        assert_eq!(editor.document().to_plain_text(), "ab!XYcd");
        // Ensure the exclamation mark is outside the link
        let block = &editor.document().blocks()[0];
        assert!(matches!(block.content[0], InlineContent::Text(_)));
        if let InlineContent::Link { content, .. } = &block.content[1] {
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "XY");
        } else {
            panic!("Expected a link at index 1");
        }
    }

    #[test]
    fn test_insert_text_at_end_of_link_inserts_after() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("ab").unwrap();
        editor.insert_link_at_cursor("dest", "XY").unwrap();
        editor.insert_text("cd").unwrap();

        // Caret at the very end of the link (between Y and c): abXY|cd => offset 4
        editor.set_cursor(DocumentPosition::new(0, 4));
        editor.insert_text("!").unwrap();

        assert_eq!(editor.document().to_plain_text(), "abXY!cd");
        // Ensure the exclamation mark is outside the link
        let block = &editor.document().blocks()[0];
        if let InlineContent::Link { content, .. } = &block.content[1] {
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "XY");
        } else {
            panic!("Expected a link at index 1");
        }
        assert!(matches!(block.content[2], InlineContent::Text(_)));
    }

    #[test]
    fn test_toggle_list_clears_checklist_run() {
        let mut editor = StructuredEditor::new();

        // Prepare three checklist items
        {
            let mut doc = StructuredDocument::new();
            for i in 0..3 {
                let mut block = Block::new(
                    0,
                    BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: Some(i % 2 == 0),
                    },
                );
                block
                    .content
                    .push(InlineContent::Text(TextRun::plain(format!("Item {i}"))));
                doc.add_block(block);
            }
            *editor.document_mut() = doc;
        }

        // Trigger bullet toggle on the middle checklist item
        editor.set_cursor(DocumentPosition::new(1, 0));
        editor.toggle_list().unwrap();

        let blocks = editor.document().blocks();
        assert!(blocks.iter().all(|block| {
            matches!(
                block.block_type,
                BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: None,
                }
            )
        }));
    }

    #[test]
    fn test_set_block_type_multi_block_bullet() {
        let mut editor = StructuredEditor::new();

        let mut doc = StructuredDocument::new();
        for text in ["One", "Two", "Three"] {
            let block = Block::paragraph(0).with_plain_text(text);
            doc.add_block(block);
        }
        let last_len = doc.blocks().last().unwrap().text_len();
        *editor.document_mut() = doc;

        editor.set_selection(
            DocumentPosition::new(0, 0),
            DocumentPosition::new(2, last_len),
        );
        editor
            .set_block_type(BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            })
            .unwrap();

        let blocks = editor.document().blocks();
        assert!(blocks.iter().all(|block| {
            matches!(
                block.block_type,
                BlockType::ListItem {
                    ordered: false,
                    number: None,
                    checkbox: None,
                }
            )
        }));
    }

    #[test]
    fn test_set_block_type_multi_block_ordered() {
        let mut editor = StructuredEditor::new();

        let mut doc = StructuredDocument::new();
        for text in ["One", "Two", "Three"] {
            let block = Block::paragraph(0).with_plain_text(text);
            doc.add_block(block);
        }
        let last_len = doc.blocks().last().unwrap().text_len();
        *editor.document_mut() = doc;

        editor.set_selection(
            DocumentPosition::new(0, 0),
            DocumentPosition::new(2, last_len),
        );
        editor
            .set_block_type(BlockType::ListItem {
                ordered: true,
                number: Some(1),
                checkbox: None,
            })
            .unwrap();

        let blocks = editor.document().blocks();
        for (i, block) in blocks.iter().enumerate() {
            match block.block_type {
                BlockType::ListItem {
                    ordered: true,
                    number,
                    checkbox: None,
                } => assert_eq!(number, Some(1 + i as u64)),
                ref other => panic!("unexpected block type: {:?}", other),
            }
        }
    }

    #[test]
    fn test_toggle_list_multi_block_selection() {
        let mut editor = StructuredEditor::new();

        let mut doc = StructuredDocument::new();
        for text in ["One", "Two", "Three"] {
            let block = Block::paragraph(0).with_plain_text(text);
            doc.add_block(block);
        }
        let last_len = doc.blocks().last().unwrap().text_len();
        *editor.document_mut() = doc;

        editor.set_selection(
            DocumentPosition::new(0, 0),
            DocumentPosition::new(2, last_len),
        );
        editor.toggle_list().unwrap();

        {
            let blocks = editor.document().blocks();
            assert!(blocks.iter().all(|block| {
                matches!(
                    block.block_type,
                    BlockType::ListItem {
                        ordered: false,
                        number: None,
                        checkbox: None,
                    }
                )
            }));
        }

        editor.set_selection(
            DocumentPosition::new(0, 0),
            DocumentPosition::new(2, last_len),
        );
        editor.toggle_list().unwrap();

        let blocks = editor.document().blocks();
        assert!(
            blocks
                .iter()
                .all(|block| matches!(block.block_type, BlockType::Paragraph))
        );
    }

    #[test]
    fn test_toggle_ordered_list_multi_block_selection() {
        let mut editor = StructuredEditor::new();

        let mut doc = StructuredDocument::new();
        for text in ["One", "Two", "Three"] {
            let block = Block::paragraph(0).with_plain_text(text);
            doc.add_block(block);
        }
        let last_len = doc.blocks().last().unwrap().text_len();
        *editor.document_mut() = doc;

        editor.set_selection(
            DocumentPosition::new(0, 0),
            DocumentPosition::new(2, last_len),
        );
        editor.toggle_ordered_list().unwrap();

        {
            let blocks = editor.document().blocks();
            for (i, block) in blocks.iter().enumerate() {
                match block.block_type {
                    BlockType::ListItem {
                        ordered: true,
                        number,
                        checkbox: None,
                    } => assert_eq!(number, Some(1 + i as u64)),
                    ref other => panic!("expected ordered list item, found {:?}", other),
                }
            }
        }

        editor.set_selection(
            DocumentPosition::new(0, 0),
            DocumentPosition::new(2, last_len),
        );
        editor.toggle_ordered_list().unwrap();

        let blocks = editor.document().blocks();
        assert!(
            blocks
                .iter()
                .all(|block| matches!(block.block_type, BlockType::Paragraph))
        );
    }

    #[test]
    fn test_toggle_ordered_list_continues_numbering_after_run() {
        let mut editor = StructuredEditor::new();

        let mut doc = StructuredDocument::new();

        doc.add_block(
            Block::new(
                0,
                BlockType::ListItem {
                    ordered: true,
                    number: Some(3),
                    checkbox: None,
                },
            )
            .with_plain_text("Item 3"),
        );
        doc.add_block(
            Block::new(
                0,
                BlockType::ListItem {
                    ordered: true,
                    number: Some(4),
                    checkbox: None,
                },
            )
            .with_plain_text("Item 4"),
        );
        doc.add_block(Block::paragraph(0).with_plain_text("Next"));
        doc.add_block(Block::paragraph(0).with_plain_text("Another"));

        let last_len = doc.blocks().last().unwrap().text_len();
        *editor.document_mut() = doc;

        editor.set_selection(
            DocumentPosition::new(2, 0),
            DocumentPosition::new(3, last_len),
        );
        editor.toggle_ordered_list().unwrap();

        let blocks = editor.document().blocks();
        assert_eq!(
            blocks[2].block_type,
            BlockType::ListItem {
                ordered: true,
                number: Some(5),
                checkbox: None
            }
        );
        assert_eq!(
            blocks[3].block_type,
            BlockType::ListItem {
                ordered: true,
                number: Some(6),
                checkbox: None
            }
        );
    }

    #[test]
    fn test_backspace_inside_link() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("ab").unwrap();
        editor.insert_link_at_cursor("dest", "XYZ").unwrap();
        editor.insert_text("cd").unwrap();

        // Caret after Y inside the link (abXY|Zcd => offset 4)
        editor.set_cursor(DocumentPosition::new(0, 4));
        editor.delete_backward().unwrap();

        assert_eq!(editor.document().to_plain_text(), "abXZcd");

        let block = &editor.document().blocks()[0];
        if let InlineContent::Link { content, .. } = &block.content[1] {
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "XZ");
        } else {
            panic!("Expected a link at index 1");
        }
    }

    #[test]
    fn test_delete_forward_inside_link() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("ab").unwrap();
        editor.insert_link_at_cursor("dest", "XYZ").unwrap();
        editor.insert_text("cd").unwrap();

        // Caret after X inside the link (abX|YZcd => offset 3)
        editor.set_cursor(DocumentPosition::new(0, 3));
        editor.delete_forward().unwrap();

        assert_eq!(editor.document().to_plain_text(), "abXZcd");

        let block = &editor.document().blocks()[0];
        assert_eq!(block.content.len(), 3);
        if let InlineContent::Link { content, .. } = &block.content[1] {
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "XZ");
        } else {
            panic!("Expected a link at index 1");
        }
    }

    #[test]
    fn test_enter_inside_link_splits_and_preserves_links() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("A ").unwrap();
        editor.insert_link_at_cursor("dest", "bc").unwrap();
        editor.insert_text(" D").unwrap();

        // Place caret between b and c inside the link: "A b|c D"
        editor.set_cursor(DocumentPosition::new(0, 3));
        editor.insert_newline().unwrap();

        // Two paragraphs now
        assert_eq!(editor.document().block_count(), 2);
        assert_eq!(editor.document().blocks()[0].to_plain_text(), "A b");
        assert_eq!(editor.document().blocks()[1].to_plain_text(), "c D");

        // Both sides should retain links with the same destination
        if let InlineContent::Link { link, content } = &editor.document().blocks()[0].content[1] {
            assert_eq!(link.destination, "dest");
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "b");
        } else {
            panic!("Expected a link in first paragraph after split");
        }

        if let InlineContent::Link { link, content } = &editor.document().blocks()[1].content[0] {
            assert_eq!(link.destination, "dest");
            let inner_text: String = content.iter().map(|c| c.to_plain_text()).collect();
            assert_eq!(inner_text, "c");
        } else {
            panic!("Expected a link at start of second paragraph after split");
        }
    }
    #[test]
    fn test_delete_selection_across_blocks() {
        let mut editor = StructuredEditor::new();
        // Build three paragraphs
        editor.insert_text("First para").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("Second").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("Third para").unwrap();

        // Select from inside first to inside third
        let start = DocumentPosition::new(0, 3);
        let end = DocumentPosition::new(2, 2);
        editor.set_selection(start, end);
        editor.delete_selection().unwrap();

        // Expect merged result
        assert_eq!(editor.document().block_count(), 1);
        assert_eq!(editor.document().blocks()[0].to_plain_text(), "Firird para");
        assert_eq!(editor.cursor(), DocumentPosition::new(0, 3));
    }

    #[test]
    fn test_toggle_bold_across_blocks() {
        let mut editor = StructuredEditor::new();
        // Build three paragraphs
        editor.insert_text("First para").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("Second").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("Third para").unwrap();

        // Select from inside first to inside third
        let start = DocumentPosition::new(0, 3); // "Fir|st para"
        let end = DocumentPosition::new(2, 2); // "Th|ird para"
        editor.set_selection(start, end);

        // Toggle bold
        editor.toggle_bold().unwrap();

        // Inspect styles
        let doc = editor.document();
        // First block should be split: "Fir" (plain) + "st para" (bold)
        let b0 = &doc.blocks()[0];
        let parts0: Vec<(String, bool)> = b0
            .content
            .iter()
            .filter_map(|c| {
                if let InlineContent::Text(run) = c {
                    Some((run.text.clone(), run.style.bold))
                } else {
                    None
                }
            })
            .collect();
        assert!(parts0.len() >= 2);
        assert_eq!(parts0[0].0, "Fir");
        assert_eq!(parts0[0].1, false);
        assert!(parts0[1].1); // bold

        // Middle block entire should be bold
        let b1 = &doc.blocks()[1];
        let parts1: Vec<bool> = b1
            .content
            .iter()
            .filter_map(|c| {
                if let InlineContent::Text(run) = c {
                    Some(run.style.bold)
                } else {
                    None
                }
            })
            .collect();
        assert!(!parts1.is_empty());
        assert!(parts1.into_iter().all(|b| b));

        // Last block should have first part bold, remainder plain
        let b2 = &doc.blocks()[2];
        let parts2: Vec<(String, bool)> = b2
            .content
            .iter()
            .filter_map(|c| {
                if let InlineContent::Text(run) = c {
                    Some((run.text.clone(), run.style.bold))
                } else {
                    None
                }
            })
            .collect();
        assert!(parts2.len() >= 2);
        assert_eq!(parts2[0].0, "Th");
        assert!(parts2[0].1);
        assert!(!parts2.last().unwrap().1);
    }

    #[test]
    fn test_select_all() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("World").unwrap();
        editor.select_all();
        let sel = editor.selection().unwrap();
        assert_eq!(sel.0, DocumentPosition::new(0, 0));
        assert_eq!(sel.1, DocumentPosition::new(1, 5));
        assert_eq!(editor.cursor(), DocumentPosition::new(1, 5));
    }

    #[test]
    fn test_word_navigation() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello  world").unwrap();
        // Cursor at end
        assert_eq!(editor.cursor(), DocumentPosition::new(0, 12));
        // Move left by word to start of "world"
        editor.move_word_left();
        assert_eq!(editor.cursor(), DocumentPosition::new(0, 7));
        // Extend right by word to end
        editor.move_word_right_extend();
        let sel = editor.selection().unwrap();
        assert_eq!(sel.0, DocumentPosition::new(0, 7));
        assert_eq!(sel.1, DocumentPosition::new(0, 12));
    }
}
