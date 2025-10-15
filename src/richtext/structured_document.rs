// Structured Document Model
// A document representation completely independent of markdown syntax
// Markdown is only used as a storage/serialization format

use std::cmp::{max, min};
use std::fmt;

/// Unique identifier for document elements
pub type ElementId = usize;

/// Text styling (semantic, not syntactic)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub highlight: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            bold: false,
            italic: false,
            code: false,
            strikethrough: false,
            underline: false,
            highlight: false,
        }
    }
}

impl TextStyle {
    pub fn plain() -> Self {
        Self::default()
    }

    pub fn bold() -> Self {
        TextStyle {
            bold: true,
            ..Default::default()
        }
    }

    pub fn italic() -> Self {
        TextStyle {
            italic: true,
            ..Default::default()
        }
    }

    pub fn code() -> Self {
        TextStyle {
            code: true,
            ..Default::default()
        }
    }
}

/// A run of styled text (a contiguous piece of text with uniform styling)
#[derive(Debug, Clone, PartialEq)]
pub struct TextRun {
    pub text: String,
    pub style: TextStyle,
}

impl TextRun {
    pub fn new(text: impl Into<String>, style: TextStyle) -> Self {
        TextRun {
            text: text.into(),
            style,
        }
    }

    pub fn plain(text: impl Into<String>) -> Self {
        Self::new(text, TextStyle::plain())
    }

    pub fn len(&self) -> usize {
        self.text.len()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Split this text run at the given character offset
    /// Returns (left_run, right_run)
    pub fn split_at(&self, offset: usize) -> (TextRun, TextRun) {
        let (left, right) = self.text.split_at(offset);
        (
            TextRun::new(left, self.style),
            TextRun::new(right, self.style),
        )
    }

    /// Insert text at the given offset
    pub fn insert_text(&mut self, offset: usize, text: &str) {
        self.text.insert_str(offset, text);
    }

    /// Delete text in the given range [start..end)
    pub fn delete_range(&mut self, start: usize, end: usize) {
        self.text.drain(start..end);
    }
}

/// Link destination
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub destination: String,
    pub title: Option<String>,
}

/// Inline content (can appear within a block)
#[derive(Debug, Clone, PartialEq)]
pub enum InlineContent {
    Text(TextRun),
    Link {
        link: Link,
        content: Vec<InlineContent>,
    },
    LineBreak, // Soft break (becomes space on wrap)
    HardBreak, // Hard break (explicit newline)
}

impl InlineContent {
    /// Get the plain text length of this inline content
    pub fn text_len(&self) -> usize {
        match self {
            InlineContent::Text(run) => run.len(),
            InlineContent::Link { content, .. } => content.iter().map(|c| c.text_len()).sum(),
            InlineContent::LineBreak => 1, // Treated as single character
            InlineContent::HardBreak => 1,
        }
    }

    /// Flatten to plain text
    pub fn to_plain_text(&self) -> String {
        match self {
            InlineContent::Text(run) => run.text.clone(),
            InlineContent::Link { content, .. } => {
                content.iter().map(|c| c.to_plain_text()).collect()
            }
            InlineContent::LineBreak => " ".to_string(),
            InlineContent::HardBreak => "\n".to_string(),
        }
    }
}

/// Block-level content types
#[derive(Debug, Clone, PartialEq)]
pub enum BlockType {
    Paragraph,
    Heading {
        level: u8,
    }, // 1-6
    CodeBlock {
        language: Option<String>,
    },
    BlockQuote,
    ListItem {
        ordered: bool,
        number: Option<u64>,
        checkbox: Option<bool>,
    },
}

/// A block of content
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub id: ElementId,
    pub block_type: BlockType,
    pub content: Vec<InlineContent>,
}

impl Block {
    pub fn new(id: ElementId, block_type: BlockType) -> Self {
        Block {
            id,
            block_type,
            content: Vec::new(),
        }
    }

    pub fn paragraph(id: ElementId) -> Self {
        Self::new(id, BlockType::Paragraph)
    }

    pub fn heading(id: ElementId, level: u8) -> Self {
        Self::new(
            id,
            BlockType::Heading {
                level: level.clamp(1, 6),
            },
        )
    }

    pub fn with_text(mut self, text: impl Into<String>, style: TextStyle) -> Self {
        self.content
            .push(InlineContent::Text(TextRun::new(text, style)));
        self
    }

    pub fn with_plain_text(mut self, text: impl Into<String>) -> Self {
        self.content.push(InlineContent::Text(TextRun::plain(text)));
        self
    }

    /// Get the total text length of this block
    pub fn text_len(&self) -> usize {
        self.content.iter().map(|c| c.text_len()).sum()
    }

    /// Get plain text content
    pub fn to_plain_text(&self) -> String {
        self.content.iter().map(|c| c.to_plain_text()).collect()
    }

    /// Check if this block is empty (no content)
    pub fn is_empty(&self) -> bool {
        self.content.is_empty()
            || self.content.iter().all(|c| match c {
                InlineContent::Text(run) => run.text.trim().is_empty(),
                _ => false,
            })
    }

    /// Delete text in [start..end) within this block's flattened content
    pub fn delete_text_range(&mut self, start: usize, end: usize) {
        if start >= end {
            return;
        }

        fn delete_in_vec(content: &mut Vec<InlineContent>, start: usize, end: usize) {
            let mut new_content: Vec<InlineContent> = Vec::new();
            let mut pos = 0usize;

            for mut item in content.drain(..) {
                let len = item.text_len();

                // Completely before deletion range
                if pos + len <= start {
                    new_content.push(item);
                    pos += len;
                    continue;
                }

                // Completely after deletion range
                if pos >= end {
                    new_content.push(item);
                    pos += len;
                    continue;
                }

                // Overlap exists
                match &mut item {
                    InlineContent::Text(run) => {
                        let local_start = start.saturating_sub(pos);
                        let local_end = min(len, end.saturating_sub(pos));

                        // left part
                        if local_start > 0 {
                            let (left, right) = run.split_at(local_start);
                            // right may still contain part to delete; adjust it
                            let mut right_run = right;
                            let del_len = local_end.saturating_sub(local_start);
                            if del_len >= right_run.len() {
                                // delete entire right
                                new_content.push(InlineContent::Text(left));
                            } else {
                                // delete middle from right_run
                                let (mid_left, mid_right) = right_run.split_at(del_len);
                                let _ = mid_left; // dropped (deleted)
                                if !left.is_empty() {
                                    new_content.push(InlineContent::Text(left));
                                }
                                if !mid_right.is_empty() {
                                    new_content.push(InlineContent::Text(mid_right));
                                }
                            }
                        } else {
                            // Deletion starts at or before this item
                            // We need to remove a prefix of this run and keep the remaining suffix.
                            let del_in_this = min(len, end.saturating_sub(pos));
                            if del_in_this >= len {
                                // Entire run is deleted; push nothing
                            } else {
                                // Keep the suffix after the deleted prefix
                                let (_deleted, leftover) = run.split_at(del_in_this);
                                if !leftover.is_empty() {
                                    new_content.push(InlineContent::Text(leftover));
                                }
                            }
                        }
                    }
                    InlineContent::Link {
                        link,
                        content: inner,
                    } => {
                        let local_start = start.saturating_sub(pos);
                        let local_end = min(len, end.saturating_sub(pos));
                        // Recurse inside link content for the overlapping region
                        delete_in_vec(inner, local_start, local_end);
                        if !inner.is_empty()
                            && inner.iter().map(|c| c.text_len()).sum::<usize>() > 0
                        {
                            new_content.push(InlineContent::Link {
                                link: link.clone(),
                                content: inner.clone(),
                            });
                        }
                    }
                    InlineContent::LineBreak | InlineContent::HardBreak => {
                        // If this break is within the deletion range, drop it
                        let local_start = start.saturating_sub(pos);
                        if local_start >= 1 {
                            // deletion starts after this single-char item
                            new_content.push(item);
                        } // else: it's deleted
                    }
                }

                pos += len;
            }

            *content = new_content;
        }

        let len = self.text_len();
        let start = min(start, len);
        let end = min(end, len);
        let mut content = std::mem::take(&mut self.content);
        delete_in_vec(&mut content, start, end);
        self.content = content;
    }

    /// Split this block's content at a flattened text offset, returning the right part.
    /// The left part remains in self.
    pub fn split_content_at(&mut self, offset: usize) -> Vec<InlineContent> {
        fn split_vec(
            content: &Vec<InlineContent>,
            offset: usize,
        ) -> (Vec<InlineContent>, Vec<InlineContent>) {
            let mut left: Vec<InlineContent> = Vec::new();
            let mut right: Vec<InlineContent> = Vec::new();
            let mut pos = 0usize;
            let mut done = false;

            for item in content.iter() {
                if done {
                    right.push(item.clone());
                    continue;
                }
                let len = item.text_len();
                if pos + len < offset {
                    left.push(item.clone());
                    pos += len;
                    continue;
                }
                if pos + len == offset {
                    left.push(item.clone());
                    pos += len;
                    done = true;
                    continue;
                }
                // offset falls within this item
                match item {
                    InlineContent::Text(run) => {
                        let local = offset - pos;
                        let (l, r) = run.split_at(local);
                        if !l.is_empty() {
                            left.push(InlineContent::Text(l));
                        }
                        if !r.is_empty() {
                            right.push(InlineContent::Text(r));
                        }
                    }
                    InlineContent::Link {
                        link,
                        content: inner,
                    } => {
                        let local = offset - pos;
                        let (l_inner, r_inner) = split_vec(inner, local);
                        if !l_inner.is_empty() {
                            left.push(InlineContent::Link {
                                link: link.clone(),
                                content: l_inner,
                            });
                        }
                        if !r_inner.is_empty() {
                            right.push(InlineContent::Link {
                                link: link.clone(),
                                content: r_inner,
                            });
                        }
                    }
                    InlineContent::LineBreak | InlineContent::HardBreak => {
                        let local = offset - pos; // 0..1
                        if local == 0 {
                            right.push(item.clone());
                        } else {
                            left.push(item.clone());
                        }
                    }
                }
                done = true;
            }

            (left, right)
        }

        let offset = min(offset, self.text_len());
        let (left, right) = split_vec(&self.content, offset);
        self.content = left;
        right
    }

    /// Insert plain text at a flattened text offset
    pub fn insert_plain_text(&mut self, offset: usize, text: &str) {
        let right = self.split_content_at(offset);
        if !text.is_empty() {
            self.content.push(InlineContent::Text(TextRun::plain(text)));
        }
        self.content.extend(right);
    }
}

/// Position within a document
/// This represents a logical cursor position in the structured content
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocumentPosition {
    pub block_index: usize,
    pub offset: usize, // Character offset within the block's flattened text
}

impl DocumentPosition {
    pub fn new(block_index: usize, offset: usize) -> Self {
        DocumentPosition {
            block_index,
            offset,
        }
    }

    pub fn start() -> Self {
        DocumentPosition::new(0, 0)
    }
}

/// The structured document
pub struct StructuredDocument {
    blocks: Vec<Block>,
    next_id: ElementId,
}

impl StructuredDocument {
    pub fn new() -> Self {
        StructuredDocument {
            blocks: Vec::new(),
            next_id: 1,
        }
    }

    /// Get a unique element ID
    fn next_id(&mut self) -> ElementId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Get blocks
    pub fn blocks(&self) -> &[Block] {
        &self.blocks
    }

    /// Get mutable blocks
    pub fn blocks_mut(&mut self) -> &mut Vec<Block> {
        &mut self.blocks
    }

    /// Add a block
    pub fn add_block(&mut self, mut block: Block) {
        if block.id == 0 {
            block.id = self.next_id();
        }
        self.blocks.push(block);
    }

    /// Insert a block at a specific position
    pub fn insert_block(&mut self, index: usize, mut block: Block) {
        if block.id == 0 {
            block.id = self.next_id();
        }
        self.blocks.insert(index, block);
    }

    /// Remove a block
    pub fn remove_block(&mut self, index: usize) -> Option<Block> {
        if index < self.blocks.len() {
            Some(self.blocks.remove(index))
        } else {
            None
        }
    }

    /// Get block count
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Find block by ID
    pub fn find_block(&self, id: ElementId) -> Option<&Block> {
        self.blocks.iter().find(|b| b.id == id)
    }

    /// Find block index by ID
    pub fn find_block_index(&self, id: ElementId) -> Option<usize> {
        self.blocks.iter().position(|b| b.id == id)
    }

    /// Validate and clamp a position to document bounds
    pub fn clamp_position(&self, pos: DocumentPosition) -> DocumentPosition {
        if self.blocks.is_empty() {
            return DocumentPosition::start();
        }

        let block_index = pos.block_index.min(self.blocks.len() - 1);
        let block = &self.blocks[block_index];
        let offset = pos.offset.min(block.text_len());

        DocumentPosition::new(block_index, offset)
    }

    /// Convert to plain text
    pub fn to_plain_text(&self) -> String {
        self.blocks
            .iter()
            .map(|b| b.to_plain_text())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Check if document is empty
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Create a simple document with one paragraph
    pub fn with_paragraph(text: impl Into<String>) -> Self {
        let mut doc = Self::new();
        let id = doc.next_id();
        let block = Block::paragraph(id).with_plain_text(text);
        doc.add_block(block);
        doc
    }

    /// Delete content in [start..end) across blocks.
    /// If the range spans multiple blocks, merges the tail of the end block into the start block
    /// and removes all fully-covered blocks in between.
    pub fn delete_range(&mut self, start: DocumentPosition, end: DocumentPosition) {
        if self.blocks.is_empty() {
            return;
        }
        let mut a = self.clamp_position(start);
        let mut b = self.clamp_position(end);
        // Ensure a <= b
        if (b.block_index < a.block_index)
            || (b.block_index == a.block_index && b.offset < a.offset)
        {
            std::mem::swap(&mut a, &mut b);
        }

        if a.block_index == b.block_index {
            let block = &mut self.blocks[a.block_index];
            block.delete_text_range(a.offset, b.offset);
            return;
        }

        // Delete tail of start block
        {
            let block = &mut self.blocks[a.block_index];
            let len = block.text_len();
            block.delete_text_range(a.offset, len);
        }

        // Delete head of end block and capture its remaining content
        let mut tail_content: Vec<InlineContent> = {
            let block = &mut self.blocks[b.block_index];
            let right = block.split_content_at(b.offset);
            // At this point, block contains left/head, right is tail we want to keep
            right
        };

        // Remove blocks between start+1 and end inclusive of the original end head block
        // After split, the end block now contains only head we deleted; we can remove it.
        let remove_start = a.block_index + 1;
        let remove_count = b.block_index - a.block_index; // number of blocks to remove starting at remove_start
        for _ in 0..remove_count {
            if remove_start < self.blocks.len() {
                self.blocks.remove(remove_start);
            }
        }

        // Append tail_content to the (now) start block
        if !tail_content.is_empty() {
            self.blocks[a.block_index]
                .content
                .extend(tail_content.drain(..));
        }
    }

    /// Replace content in [start..end) with plain text. Supports multi-paragraph text using \n\n separators.
    /// If the replacement spans multiple paragraphs, any tail content from the original end
    /// position is appended to the last inserted paragraph block.
    pub fn replace_range(&mut self, start: DocumentPosition, end: DocumentPosition, text: &str) {
        if self.blocks.is_empty() {
            // If empty, create a paragraph and insert
            let id = self.next_id();
            self.blocks.push(Block::paragraph(id));
        }

        // First, delete the target range
        let a = self.clamp_position(start);
        let b = self.clamp_position(end);
        let (start_pos, end_pos) = if (b.block_index < a.block_index)
            || (b.block_index == a.block_index && b.offset < a.offset)
        {
            (b, a)
        } else {
            (a, b)
        };

        // Perform deletion to normalize insertion point
        self.delete_range(start_pos, end_pos);

        // Determine insertion point in the normalized document
        let insert_block_index = start_pos
            .block_index
            .min(self.blocks.len().saturating_sub(1));
        let insert_offset = start_pos
            .offset
            .min(self.blocks[insert_block_index].text_len());

        if text.is_empty() {
            return;
        }

        // We want the content after the insertion point (which may include the tail from the
        // original end block) to end up after the LAST inserted paragraph. So split the current
        // block at the insertion point and hold on to the right side for later.
        let mut trailing_right = self.blocks[insert_block_index].split_content_at(insert_offset);

        // Insert paragraphs
        let paragraphs: Vec<&str> = text.split("\n\n").collect();

        // First paragraph goes into the (now split) current block
        if !paragraphs[0].is_empty() {
            self.blocks[insert_block_index]
                .content
                .push(InlineContent::Text(TextRun::plain(paragraphs[0])));
        }

        // Subsequent paragraphs become new blocks after the current block
        let mut last_block_index = insert_block_index;
        if paragraphs.len() > 1 {
            let mut insert_at = insert_block_index + 1;
            for p in paragraphs.iter().skip(1) {
                let mut block = Block::paragraph(0);
                if !p.is_empty() {
                    block.content.push(InlineContent::Text(TextRun::plain(*p)));
                }
                self.insert_block(insert_at, block);
                last_block_index = insert_at;
                insert_at += 1;
            }
        }

        // Append the trailing content to the last affected block
        if !trailing_right.is_empty() {
            let target = last_block_index;
            self.blocks[target].content.extend(trailing_right.drain(..));
        }
    }
}

impl Default for StructuredDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Display for StructuredDocument {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "StructuredDocument ({} blocks):", self.blocks.len())?;
        for (i, block) in self.blocks.iter().enumerate() {
            write!(f, "  [{}] ", i)?;
            match &block.block_type {
                BlockType::Paragraph => write!(f, "Paragraph")?,
                BlockType::Heading { level } => write!(f, "Heading(h{})", level)?,
                BlockType::CodeBlock { language } => write!(f, "CodeBlock({:?})", language)?,
                BlockType::BlockQuote => write!(f, "BlockQuote")?,
                BlockType::ListItem {
                    ordered,
                    number,
                    checkbox,
                } => write!(
                    f,
                    "ListItem({}{}{})",
                    if *ordered { "ordered" } else { "unordered" },
                    if let Some(n) = number {
                        format!(", #{}", n)
                    } else {
                        String::new()
                    },
                    if let Some(checked) = checkbox {
                        if *checked { ", checked" } else { ", unchecked" }.to_string()
                    } else {
                        String::new()
                    }
                )?,
            }
            writeln!(f, ": {:?}", block.to_plain_text())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_run() {
        let run = TextRun::plain("hello world");
        assert_eq!(run.len(), 11);

        let (left, right) = run.split_at(5);
        assert_eq!(left.text, "hello");
        assert_eq!(right.text, " world");
    }

    #[test]
    fn test_block_text_len() {
        let block = Block::paragraph(1)
            .with_plain_text("hello")
            .with_text(" world", TextStyle::bold());

        assert_eq!(block.text_len(), 11);
        assert_eq!(block.to_plain_text(), "hello world");
    }

    #[test]
    fn test_document_creation() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::paragraph(0).with_plain_text("First paragraph"));
        doc.add_block(Block::heading(0, 1).with_plain_text("A heading"));

        assert_eq!(doc.block_count(), 2);
    }

    #[test]
    fn test_position_clamping() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::paragraph(0).with_plain_text("hello"));

        let pos = DocumentPosition::new(0, 100);
        let clamped = doc.clamp_position(pos);
        assert_eq!(clamped.offset, 5); // Length of "hello"
    }

    #[test]
    fn test_delete_range_within_block() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::paragraph(0).with_plain_text("Hello world"));
        let start = DocumentPosition::new(0, 5);
        let end = DocumentPosition::new(0, 11);
        doc.delete_range(start, end);
        assert_eq!(doc.blocks()[0].to_plain_text(), "Hello");
    }

    #[test]
    fn test_delete_range_across_blocks_merges() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::paragraph(0).with_plain_text("First para"));
        doc.add_block(Block::paragraph(0).with_plain_text("Second"));
        doc.add_block(Block::paragraph(0).with_plain_text("Third para"));

        // Delete from after "Fir" in block 0 to after "Th" in block 2
        let start = DocumentPosition::new(0, 3); // "Fir|st para"
        let end = DocumentPosition::new(2, 2); // "Th|ird para"
        doc.delete_range(start, end);

        // Blocks between should be removed, and result should be "Fir" + "ird para"
        assert_eq!(doc.block_count(), 1);
        assert_eq!(doc.blocks()[0].to_plain_text(), "Firird para");
    }

    #[test]
    fn test_replace_range_across_blocks_with_paragraphs() {
        let mut doc = StructuredDocument::new();
        doc.add_block(Block::paragraph(0).with_plain_text("Hello one"));
        doc.add_block(Block::paragraph(0).with_plain_text("Hello two"));
        doc.add_block(Block::paragraph(0).with_plain_text("Hello three"));

        let start = DocumentPosition::new(0, 6); // at "Hello |one"
        let end = DocumentPosition::new(2, 5); // at "Hello |three"
        doc.replace_range(start, end, "X\n\nY");

        // Expect first block: "Hello " + "X" + tail of last after offset 5 was removed by replace
        assert_eq!(doc.blocks()[0].to_plain_text(), "Hello X");
        // New paragraph inserted after with "Y"
        assert_eq!(doc.blocks()[1].to_plain_text(), "Y three");
    }
}
