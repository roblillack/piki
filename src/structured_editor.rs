// Structured Editor
// Provides editing operations on a StructuredDocument
// Completely independent of markdown syntax

use crate::structured_document::*;

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
}

impl StructuredEditor {
    /// Create a new editor with an empty document
    pub fn new() -> Self {
        StructuredEditor {
            document: StructuredDocument::new(),
            cursor: DocumentPosition::start(),
            selection: None,
        }
    }

    /// Create an editor with an existing document
    pub fn with_document(document: StructuredDocument) -> Self {
        StructuredEditor {
            document,
            cursor: DocumentPosition::start(),
            selection: None,
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

    /// Get cursor position
    pub fn cursor(&self) -> DocumentPosition {
        self.cursor
    }

    /// Set cursor position (will be clamped to valid range)
    pub fn set_cursor(&mut self, pos: DocumentPosition) {
        self.cursor = self.document.clamp_position(pos);
        self.selection = None; // Clear selection when moving cursor
    }

    /// Get selection range
    pub fn selection(&self) -> Option<(DocumentPosition, DocumentPosition)> {
        self.selection
    }

    /// Set selection range
    pub fn set_selection(&mut self, start: DocumentPosition, end: DocumentPosition) {
        let start = self.document.clamp_position(start);
        let end = self.document.clamp_position(end);
        self.selection = Some((start, end));
    }

    /// Clear selection
    pub fn clear_selection(&mut self) {
        self.selection = None;
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
            start = text[..start].char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
        }

        // Move end forward to end of word
        let mut chars = text[end..].char_indices();
        while let Some((_, ch)) = chars.next() {
            if ch.is_whitespace() || ch.is_ascii_punctuation() {
                break;
            }
            end = text[..end].chars().next().map(|c| end + c.len_utf8()).unwrap_or(end);
        }

        // If we're on whitespace, extend to include it
        if start == end {
            end = text[end..].chars().next().map(|c| end + c.len_utf8()).unwrap_or(end);
        }

        let start_pos = DocumentPosition::new(pos.block_index, start);
        let end_pos = DocumentPosition::new(pos.block_index, end);

        self.set_selection(start_pos, end_pos);
        self.cursor = end_pos;
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
        self.cursor = end_pos;
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

        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];

        // Handle different inline content types
        if content_idx >= block.content.len() {
            // Append to end
            block.content.push(InlineContent::Text(TextRun::plain(text)));
        } else {
            match &mut block.content[content_idx] {
                InlineContent::Text(run) => {
                    run.insert_text(content_offset, text);
                }
                InlineContent::Link { .. } | InlineContent::LineBreak | InlineContent::HardBreak => {
                    // Insert new text run before this element
                    block.content.insert(content_idx, InlineContent::Text(TextRun::plain(text)));
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
        let (block_type, is_empty, content) = {
            let blocks = self.document.blocks();
            let current_block = &blocks[block_index];
            (current_block.block_type.clone(), current_block.is_empty(), current_block.content.clone())
        };

        // Check if we're in a list item
        if let BlockType::ListItem { ordered, number } = &block_type {
            // Check if list item is empty
            if is_empty || offset == 0 {
                // Convert to paragraph to exit list
                let blocks = self.document.blocks_mut();
                blocks[block_index].block_type = BlockType::Paragraph;
                self.cursor.offset = 0;
                return Ok(());
            }

            // Split the current list item
            let (left_content, right_content) = Self::split_content_at_static(&content, offset);
            let blocks = self.document.blocks_mut();
            blocks[block_index].content = left_content;

            // Create new list item
            let new_number = if *ordered {
                number.map(|n| n + 1)
            } else {
                None
            };
            let mut new_item = Block::new(0, BlockType::ListItem {
                ordered: *ordered,
                number: new_number,
            });
            new_item.content = right_content;

            self.document.insert_block(block_index + 1, new_item);
            self.cursor = DocumentPosition::new(block_index + 1, 0);
        } else {
            // Regular paragraph split
            let (left_content, right_content) = Self::split_content_at_static(&content, offset);
            let blocks = self.document.blocks_mut();
            blocks[block_index].content = left_content;

            // Create new paragraph with remaining content
            let mut new_para = Block::paragraph(0);
            new_para.content = right_content;

            self.document.insert_block(block_index + 1, new_para);
            self.cursor = DocumentPosition::new(block_index + 1, 0);
        }

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

            let blocks = self.document.blocks_mut();
            let current_block = blocks.remove(block_index);
            let prev_block = &mut blocks[block_index - 1];
            let prev_len = prev_block.text_len();

            // Merge content
            prev_block.content.extend(current_block.content);

            self.cursor = DocumentPosition::new(block_index - 1, prev_len);
        } else {
            // Delete character within block
            let (content_idx, content_offset) = {
                let blocks = self.document.blocks();
                let block = &blocks[block_index];
                Self::find_content_at_offset_static(&block.content, offset)
            };

            let blocks = self.document.blocks_mut();
            let block = &mut blocks[block_index];

            if content_idx < block.content.len() {
                match &mut block.content[content_idx] {
                    InlineContent::Text(run) => {
                        if content_offset > 0 {
                            // Delete one character back
                            let char_boundary = run.text[..content_offset]
                                .char_indices()
                                .next_back()
                                .map(|(i, _)| i)
                                .unwrap_or(0);
                            run.delete_range(char_boundary, content_offset);
                            self.cursor.offset -= content_offset - char_boundary;

                            // Remove run if now empty
                            if run.is_empty() {
                                block.content.remove(content_idx);
                            }
                        }
                    }
                    _ => {
                        // Remove the element
                        block.content.remove(content_idx);
                        self.cursor.offset -= 1;
                    }
                }
            }
        }

        Ok(())
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

        let blocks = self.document.blocks_mut();
        let block = &blocks[block_index];

        if offset >= block.text_len() {
            // At end of block - merge with next block
            if block_index >= blocks.len() - 1 {
                return Ok(()); // At end of document, nothing to delete
            }

            let next_block = blocks.remove(block_index + 1);
            blocks[block_index].content.extend(next_block.content);
        } else {
            // Delete character within block
            let (content_idx, content_offset) = Self::find_content_at_offset_static(&blocks[block_index].content, offset);
            let block = &mut blocks[block_index];

            if content_idx < block.content.len() {
                match &mut block.content[content_idx] {
                    InlineContent::Text(run) => {
                        // Find next character boundary
                        let char_end = run.text[content_offset..]
                            .char_indices()
                            .nth(1)
                            .map(|(i, _)| content_offset + i)
                            .unwrap_or(run.len());

                        run.delete_range(content_offset, char_end);

                        // Remove run if now empty
                        if run.is_empty() {
                            block.content.remove(content_idx);
                        }
                    }
                    _ => {
                        // Remove the element
                        block.content.remove(content_idx);
                    }
                }
            }
        }

        Ok(())
    }

    /// Delete the current selection
    pub fn delete_selection(&mut self) -> EditResult {
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

        if start.block_index == end.block_index {
            // Selection within single block
            let (start_idx, start_off, end_idx, end_off) = {
                let blocks = self.document.blocks();
                let block = &blocks[start.block_index];
                let start_info = Self::find_content_at_offset_static(&block.content, start.offset);
                let end_info = Self::find_content_at_offset_static(&block.content, end.offset);
                (start_info.0, start_info.1, end_info.0, end_info.1)
            };

            let blocks = self.document.blocks_mut();
            let block = &mut blocks[start.block_index];

            if start_idx == end_idx {
                // Within single content element
                if let Some(InlineContent::Text(run)) = block.content.get_mut(start_idx) {
                    run.delete_range(start_off, end_off);
                    if run.is_empty() {
                        block.content.remove(start_idx);
                    }
                }
            } else {
                // Across multiple content elements
                block.content.drain(start_idx + 1..=end_idx);
                // TODO: Handle partial deletions in first and last elements
            }
        } else {
            // Selection across multiple blocks
            // TODO: Implement multi-block selection deletion
        }

        self.cursor = start;
        self.selection = None;

        Ok(())
    }

    /// Move cursor left by one character
    pub fn move_cursor_left(&mut self) {
        if self.cursor.offset > 0 {
            self.cursor.offset -= 1;
        } else if self.cursor.block_index > 0 {
            // Move to end of previous block
            self.cursor.block_index -= 1;
            let blocks = self.document.blocks();
            self.cursor.offset = blocks[self.cursor.block_index].text_len();
        }
        self.cursor = self.document.clamp_position(self.cursor);
        self.selection = None;
    }

    /// Move cursor right by one character
    pub fn move_cursor_right(&mut self) {
        let blocks = self.document.blocks();
        if self.cursor.block_index >= blocks.len() {
            return;
        }

        let block_len = blocks[self.cursor.block_index].text_len();

        if self.cursor.offset < block_len {
            self.cursor.offset += 1;
        } else if self.cursor.block_index < blocks.len() - 1 {
            // Move to start of next block
            self.cursor.block_index += 1;
            self.cursor.offset = 0;
        }
        self.cursor = self.document.clamp_position(self.cursor);
        self.selection = None;
    }

    /// Move cursor up (to previous block)
    pub fn move_cursor_up(&mut self) {
        if self.cursor.block_index > 0 {
            self.cursor.block_index -= 1;
            let blocks = self.document.blocks();
            let new_block_len = blocks[self.cursor.block_index].text_len();
            self.cursor.offset = self.cursor.offset.min(new_block_len);
        }
        self.selection = None;
    }

    /// Move cursor down (to next block)
    pub fn move_cursor_down(&mut self) {
        let blocks = self.document.blocks();
        if self.cursor.block_index < blocks.len() - 1 {
            self.cursor.block_index += 1;
            let new_block_len = blocks[self.cursor.block_index].text_len();
            self.cursor.offset = self.cursor.offset.min(new_block_len);
        }
        self.selection = None;
    }

    /// Move cursor to start of current block
    pub fn move_cursor_to_line_start(&mut self) {
        self.cursor.offset = 0;
        self.selection = None;
    }

    /// Move cursor to end of current block
    pub fn move_cursor_to_line_end(&mut self) {
        let blocks = self.document.blocks();
        if self.cursor.block_index < blocks.len() {
            self.cursor.offset = blocks[self.cursor.block_index].text_len();
        }
        self.selection = None;
    }

    // Selection-extending movement methods (for Shift+arrow keys)

    /// Move cursor left by one character, extending selection
    pub fn move_cursor_left_extend(&mut self) {
        let new_pos = if self.cursor.offset > 0 {
            DocumentPosition::new(self.cursor.block_index, self.cursor.offset - 1)
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
        let blocks = self.document.blocks();
        if self.cursor.block_index >= blocks.len() {
            return;
        }

        let block_len = blocks[self.cursor.block_index].text_len();
        let new_pos = if self.cursor.offset < block_len {
            DocumentPosition::new(self.cursor.block_index, self.cursor.offset + 1)
        } else if self.cursor.block_index < blocks.len() - 1 {
            // Move to start of next block
            DocumentPosition::new(self.cursor.block_index + 1, 0)
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
            BlockType::Heading { level } => BlockType::Heading { level: (*level % 3) + 1 },
            BlockType::ListItem { .. } => BlockType::Heading { level: 1 },
            BlockType::CodeBlock { .. } => BlockType::Heading { level: 1 },
            BlockType::BlockQuote => BlockType::Heading { level: 1 },
        };

        Ok(())
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

        // For now, only support styling within a single block
        if start.block_index != end.block_index {
            return Ok(());
        }

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

        // Apply style to the selected content
        let styled_content: Vec<InlineContent> = selected_content
            .into_iter()
            .map(|item| match item {
                InlineContent::Text(mut run) => {
                    apply_style(&mut run.style);
                    InlineContent::Text(run)
                }
                other => other,
            })
            .collect();

        // Reconstruct the block content
        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];
        block.content = content_before;
        block.content.extend(styled_content);
        block.content.extend(content_after);

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
        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        let blocks = self.document.blocks_mut();
        let block = &mut blocks[block_index];

        // Toggle list status
        block.block_type = match &block.block_type {
            BlockType::ListItem { .. } => BlockType::Paragraph,
            BlockType::Paragraph | BlockType::Heading { .. } => BlockType::ListItem {
                ordered: false,
                number: None,
            },
            BlockType::CodeBlock { .. } => BlockType::ListItem {
                ordered: false,
                number: None,
            },
            BlockType::BlockQuote => BlockType::ListItem {
                ordered: false,
                number: None,
            },
        };

        Ok(())
    }

    /// Set the block type for the current block
    pub fn set_block_type(&mut self, block_type: BlockType) -> EditResult {
        let block_index = self.cursor.block_index;
        if block_index >= self.document.block_count() {
            return Err(EditError::InvalidBlockIndex);
        }

        let blocks = self.document.blocks_mut();
        blocks[block_index].block_type = block_type;

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
        // If there's a selection, delete it first
        if self.selection.is_some() {
            self.delete_selection()?;
        }

        // Insert the pasted text
        // For now, we'll insert it as plain text at the cursor position
        // TODO: Handle multi-line pastes more intelligently
        self.insert_text(text)
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
    fn split_content_at_static(content: &[InlineContent], offset: usize) -> (Vec<InlineContent>, Vec<InlineContent>) {
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
    fn split_content_at(&self, content: &[InlineContent], offset: usize) -> (Vec<InlineContent>, Vec<InlineContent>) {
        Self::split_content_at_static(content, offset)
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
    fn test_insert_newline() {
        let mut editor = StructuredEditor::new();
        editor.insert_text("Hello").unwrap();
        editor.insert_newline().unwrap();
        editor.insert_text("World").unwrap();

        assert_eq!(editor.document().block_count(), 2);
        assert_eq!(editor.cursor().block_index, 1);
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
}
