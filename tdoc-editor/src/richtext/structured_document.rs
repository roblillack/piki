// Block & inline content types.
//
// These are the editor/display's *transient* working representation of a single leaf:
// a `BlockType` (presentation kind) plus a flat `Vec<InlineContent>` of styled runs.
// The authoritative document is the `tdoc::Document` tree owned by the editor; a leaf's
// spans are converted to/from this flat form (see `inline_convert`) only while laying it
// out or editing it. Positions live in `tree_path`; tree navigation in `tree_walk`.

use std::borrow::Cow;
use std::cmp::min;

/// Text styling (semantic, not syntactic)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TextStyle {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
    pub strikethrough: bool,
    pub underline: bool,
    pub highlight: bool,
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
    HardBreak, // Hard break (explicit newline)
}

impl InlineContent {
    /// Get the plain text length of this inline content
    pub fn text_len(&self) -> usize {
        match self {
            InlineContent::Text(run) => run.len(),
            InlineContent::Link { content, .. } => content.iter().map(|c| c.text_len()).sum(),
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
            InlineContent::HardBreak => "\n".to_string(),
        }
    }
}

/// A single cell within a [`TableRow`]. Mirrors `tdoc`'s `TableCell`: it holds
/// inline content and a flag distinguishing header cells from data cells.
#[derive(Debug, Clone, PartialEq)]
pub struct TableCell {
    pub is_header: bool,
    pub content: Vec<InlineContent>,
}

impl TableCell {
    pub fn new(is_header: bool, content: Vec<InlineContent>) -> Self {
        TableCell { is_header, content }
    }

    /// Flatten the cell's content to plain text.
    pub fn to_plain_text(&self) -> String {
        self.content.iter().map(|c| c.to_plain_text()).collect()
    }
}

/// A single row within a [`BlockType::Table`]. Mirrors `tdoc`'s `TableRow`.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRow {
    pub cells: Vec<TableCell>,
}

impl TableRow {
    pub fn new(cells: Vec<TableCell>) -> Self {
        TableRow { cells }
    }
}

/// Block-level content types. Built transiently per leaf during layout/editing.
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
        /// 1-based ordinal for ordered lists, derived from tree position.
        number: Option<u64>,
        checkbox: Option<bool>,
        /// Nesting depth (0 = top-level item).
        depth: usize,
    },
    /// A table. The rows live here rather than in [`Block::content`]; a table
    /// block keeps its `content` empty. Tables are read-only for now.
    Table {
        rows: Vec<TableRow>,
    },
}

/// A block of content
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub block_type: BlockType,
    pub content: Vec<InlineContent>,
}

impl Block {
    pub fn new(block_type: BlockType) -> Self {
        Block {
            block_type,
            content: Vec::new(),
        }
    }

    pub fn paragraph() -> Self {
        Self::new(BlockType::Paragraph)
    }

    pub fn heading(level: u8) -> Self {
        Self::new(BlockType::Heading {
            level: level.clamp(1, 6),
        })
    }

    pub fn table(rows: Vec<TableRow>) -> Self {
        Self::new(BlockType::Table { rows })
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
                            let right_run = right;
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
                    InlineContent::HardBreak => {
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
        self.content = Self::normalize_inline_vec(content);
    }

    /// Split this block's content at a flattened text offset, returning the right part.
    /// The left part remains in self.
    pub fn split_content_at(&mut self, offset: usize) -> Vec<InlineContent> {
        fn split_vec(
            content: &[InlineContent],
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
                    InlineContent::HardBreak => {
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

    fn normalize_inline_vec(content: Vec<InlineContent>) -> Vec<InlineContent> {
        let mut normalized: Vec<InlineContent> = Vec::with_capacity(content.len());
        for item in content.into_iter() {
            match item {
                InlineContent::Text(run) => {
                    if run.text.is_empty() {
                        continue;
                    }
                    match normalized.last_mut() {
                        Some(InlineContent::Text(prev_run)) if prev_run.style == run.style => {
                            prev_run.text.push_str(&run.text);
                        }
                        _ => normalized.push(InlineContent::Text(run)),
                    }
                }
                InlineContent::Link {
                    link,
                    content: inner,
                } => {
                    let normalized_inner = Self::normalize_inline_vec(inner);
                    if normalized_inner.is_empty() {
                        continue;
                    }
                    // Coalesce with a preceding link to the same destination so a
                    // link split by a partial-range style edit reads as one link.
                    match normalized.last_mut() {
                        Some(InlineContent::Link {
                            link: prev_link,
                            content: prev_content,
                        }) if *prev_link == link => {
                            prev_content.extend(normalized_inner);
                            let merged = std::mem::take(prev_content);
                            *prev_content = Self::normalize_inline_vec(merged);
                        }
                        _ => normalized.push(InlineContent::Link {
                            link,
                            content: normalized_inner,
                        }),
                    }
                }
                InlineContent::HardBreak => {
                    normalized.push(InlineContent::HardBreak);
                }
            }
        }
        normalized
    }

    pub fn normalize_content(&mut self) {
        let content = std::mem::take(&mut self.content);
        self.content = Self::normalize_inline_vec(content);
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

/// Normalize clipboard/plain text input so CRLF/CR line endings become `\n`.
pub(crate) fn normalize_plain_text(text: &str) -> Cow<'_, str> {
    if text.contains('\r') {
        let mut normalized = String::with_capacity(text.len());
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\r' {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                normalized.push('\n');
            } else {
                normalized.push(ch);
            }
        }
        Cow::Owned(normalized)
    } else {
        Cow::Borrowed(text)
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
        let block = Block::paragraph()
            .with_plain_text("hello")
            .with_text(" world", TextStyle::bold());

        assert_eq!(block.text_len(), 11);
        assert_eq!(block.to_plain_text(), "hello world");
    }

    #[test]
    fn test_block_split_and_delete() {
        let mut block = Block::paragraph().with_plain_text("Hello world");
        let right = block.split_content_at(5);
        assert_eq!(block.to_plain_text(), "Hello");
        let mut right_block = Block::paragraph();
        right_block.content = right;
        assert_eq!(right_block.to_plain_text(), " world");

        let mut del = Block::paragraph().with_plain_text("Hello world");
        del.delete_text_range(5, 11);
        assert_eq!(del.to_plain_text(), "Hello");
    }
}
