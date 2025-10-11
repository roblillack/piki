// Structured Document Model
// A document representation completely independent of markdown syntax
// Markdown is only used as a storage/serialization format

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
}

impl Default for TextStyle {
    fn default() -> Self {
        TextStyle {
            bold: false,
            italic: false,
            code: false,
            strikethrough: false,
        }
    }
}

impl TextStyle {
    pub fn plain() -> Self {
        Self::default()
    }

    pub fn bold() -> Self {
        TextStyle { bold: true, ..Default::default() }
    }

    pub fn italic() -> Self {
        TextStyle { italic: true, ..Default::default() }
    }

    pub fn code() -> Self {
        TextStyle { code: true, ..Default::default() }
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
    Link { link: Link, content: Vec<InlineContent> },
    LineBreak, // Soft break (becomes space on wrap)
    HardBreak, // Hard break (explicit newline)
}

impl InlineContent {
    /// Get the plain text length of this inline content
    pub fn text_len(&self) -> usize {
        match self {
            InlineContent::Text(run) => run.len(),
            InlineContent::Link { content, .. } => {
                content.iter().map(|c| c.text_len()).sum()
            }
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
    Heading { level: u8 }, // 1-6
    CodeBlock { language: Option<String> },
    BlockQuote,
    ListItem { ordered: bool, number: Option<u64> },
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
        Self::new(id, BlockType::Heading { level: level.clamp(1, 6) })
    }

    pub fn with_text(mut self, text: impl Into<String>, style: TextStyle) -> Self {
        self.content.push(InlineContent::Text(TextRun::new(text, style)));
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
        self.content.is_empty() || self.content.iter().all(|c| match c {
            InlineContent::Text(run) => run.text.trim().is_empty(),
            _ => false,
        })
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
        DocumentPosition { block_index, offset }
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
                BlockType::CodeBlock { language } => {
                    write!(f, "CodeBlock({:?})", language)?
                }
                BlockType::BlockQuote => write!(f, "BlockQuote")?,
                BlockType::ListItem { ordered, number } => {
                    write!(f, "ListItem({}{})",
                        if *ordered { "ordered" } else { "unordered" },
                        if let Some(n) = number { format!(", #{}", n) } else { String::new() }
                    )?
                }
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
}
