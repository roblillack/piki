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

    /// Find the content element and offset within it for a given block offset (static version)
    fn find_content_at_offset_static(content: &[InlineContent], offset: usize) -> (usize, usize) {
        let mut current_offset = 0;

        for (idx, item) in content.iter().enumerate() {
            let item_len = item.text_len();
            if current_offset + item_len > offset {
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
                if content_offset > 0 && content_offset < run.len() {
                    let (left_run, right_run) = run.split_at(content_offset);
                    left.push(InlineContent::Text(left_run));
                    right.remove(0);
                    right.insert(0, InlineContent::Text(right_run));
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
