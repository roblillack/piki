// Structured Rich Text Display
// A rendering and interaction widget for StructuredDocument
// Completely decoupled from markdown syntax

use crate::structured_document::*;
use crate::structured_editor::*;
use crate::text_display::{DrawContext, StyleTableEntry};

/// Layout information for a rendered line
#[derive(Debug, Clone)]
struct LayoutLine {
    /// Y position of the line's baseline
    y: i32,
    /// Height of the line
    height: i32,
    /// Block index this line belongs to
    block_index: usize,
    /// Character offset range within the block [start, end)
    char_start: usize,
    char_end: usize,
    /// Visual elements on this line
    runs: Vec<VisualRun>,
}

/// A visual run of text with styling
#[derive(Debug, Clone)]
struct VisualRun {
    /// Display text
    text: String,
    /// X position
    x: i32,
    /// Style index
    style_idx: u8,
    /// Block index this belongs to
    block_index: usize,
    /// Character range within block
    char_range: (usize, usize),
    /// Inline content index (for link detection)
    inline_index: Option<usize>,
}

/// Rich Text Display for Structured Documents
pub struct StructuredRichDisplay {
    // Position and size
    x: i32,
    y: i32,
    w: i32,
    h: i32,

    // Editor (contains document and cursor)
    editor: StructuredEditor,

    // Layout cache
    layout_lines: Vec<LayoutLine>,
    layout_valid: bool,

    // Scrolling
    scroll_offset: i32,

    // Styling
    style_table: Vec<StyleTableEntry>,

    // Font settings
    text_font: u8,
    text_size: u8,
    text_color: u32,
    background_color: u32,

    // Padding
    padding_top: i32,
    padding_bottom: i32,
    padding_left: i32,
    padding_right: i32,

    // Font metrics
    line_height: i32,

    // Cursor display
    cursor_visible: bool,
    cursor_color: u32,

    // Link hover state
    hovered_link: Option<(usize, usize)>, // (block_index, inline_index)
}

impl StructuredRichDisplay {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        StructuredRichDisplay {
            x,
            y,
            w,
            h,
            editor: StructuredEditor::new(),
            layout_lines: Vec::new(),
            layout_valid: false,
            scroll_offset: 0,
            style_table: Vec::new(),
            text_font: 0,
            text_size: 14,
            text_color: 0x000000FF,
            background_color: 0xFFFFF5FF,
            padding_top: 10,
            padding_bottom: 10,
            padding_left: 25,
            padding_right: 25,
            line_height: 17,
            cursor_visible: true,
            cursor_color: 0x000000FF,
            hovered_link: None,
        }
    }

    /// Get the editor
    pub fn editor(&self) -> &StructuredEditor {
        &self.editor
    }

    /// Get mutable editor
    pub fn editor_mut(&mut self) -> &mut StructuredEditor {
        self.layout_valid = false;
        &mut self.editor
    }

    /// Set style table
    pub fn set_style_table(&mut self, table: Vec<StyleTableEntry>) {
        self.style_table = table;
    }

    /// Set padding
    pub fn set_padding(&mut self, top: i32, bottom: i32, left: i32, right: i32) {
        self.padding_top = top;
        self.padding_bottom = bottom;
        self.padding_left = left;
        self.padding_right = right;
        self.layout_valid = false;
    }

    /// Set scroll offset
    pub fn set_scroll(&mut self, offset: i32) {
        self.scroll_offset = offset.max(0);
    }

    /// Get scroll offset
    pub fn scroll_offset(&self) -> i32 {
        self.scroll_offset
    }

    /// Get content height
    pub fn content_height(&self) -> i32 {
        if let Some(last_line) = self.layout_lines.last() {
            last_line.y + last_line.height + self.padding_bottom
        } else {
            0
        }
    }

    /// Resize the widget
    pub fn resize(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.x = x;
        self.y = y;
        self.w = w;
        self.h = h;
        self.layout_valid = false;
    }

    pub fn x(&self) -> i32 { self.x }
    pub fn y(&self) -> i32 { self.y }
    pub fn w(&self) -> i32 { self.w }
    pub fn h(&self) -> i32 { self.h }

    /// Perform layout
    fn layout(&mut self, ctx: &mut dyn DrawContext) {
        if self.layout_valid {
            return;
        }

        self.layout_lines.clear();

        let content_width = self.w - self.padding_left - self.padding_right;
        let mut current_y = self.padding_top;

        // Clone blocks to avoid borrow checker issues
        let blocks = self.editor.document().blocks().to_vec();

        for (block_idx, block) in blocks.iter().enumerate() {
            current_y = self.layout_block(&block, block_idx, current_y, content_width, ctx);
        }

        self.layout_valid = true;
    }

    /// Layout a single block
    fn layout_block(
        &mut self,
        block: &Block,
        block_idx: usize,
        y: i32,
        width: i32,
        ctx: &mut dyn DrawContext,
    ) -> i32 {
        let start_x = self.padding_left;

        match &block.block_type {
            BlockType::Paragraph => {
                self.layout_inline_block(block, block_idx, y, start_x, width, self.line_height, ctx)
            }
            BlockType::Heading { level } => {
                let size = match level {
                    1 => self.text_size + 6,
                    2 => self.text_size + 4,
                    3 => self.text_size + 2,
                    _ => self.text_size,
                };
                let height = ((size as f32) * 1.3) as i32;
                let y_after = self.layout_inline_block(block, block_idx, y, start_x, width, height, ctx);
                y_after + 10 // Extra spacing after headings
            }
            BlockType::CodeBlock { .. } => {
                let text = block.to_plain_text();
                let lines: Vec<&str> = text.lines().collect();
                let style_idx = self.get_style_for_block_type(&block.block_type);
                let mut current_y = y + 5;

                for line in lines {
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: line.len(),
                        runs: vec![VisualRun {
                            text: line.to_string(),
                            x: start_x + 10,
                            style_idx,
                            block_index: block_idx,
                            char_range: (0, line.len()),
                            inline_index: None,
                        }],
                    });
                    current_y += self.line_height;
                }

                current_y + 10
            }
            BlockType::BlockQuote => {
                self.layout_inline_block(block, block_idx, y + 5, start_x + 20, width - 20, self.line_height, ctx) + 5
            }
            BlockType::ListItem { .. } => {
                // Indent: bullet is 1x text_size from left, text is another 1x text_size in
                let bullet_indent = self.text_size as i32;
                let text_indent = bullet_indent * 2;

                let mut runs = vec![VisualRun {
                    text: "• ".to_string(),
                    x: start_x + bullet_indent,
                    style_idx: 0,
                    block_index: block_idx,
                    char_range: (0, 0),
                    inline_index: None,
                }];

                // Layout the content with proper text indentation
                let (mut content_runs, y_after) = self.layout_inline_content(
                    &block.content,
                    &block.block_type,
                    block_idx,
                    y,
                    start_x + text_indent,
                    width - text_indent,
                    self.line_height,
                    ctx,
                );

                let mut current_y = y;

                // Merge bullet with first line
                if !content_runs.is_empty() && !content_runs[0].is_empty() {
                    runs.extend(content_runs[0].drain(..));
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: 0,
                        runs,
                    });
                    current_y += self.line_height;

                    // Add remaining lines
                    for line_runs in content_runs.iter().skip(1) {
                        self.layout_lines.push(LayoutLine {
                            y: current_y,
                            height: self.line_height,
                            block_index: block_idx,
                            char_start: 0,
                            char_end: 0,
                            runs: line_runs.clone(),
                        });
                        current_y += self.line_height;
                    }
                } else {
                    // Just bullet
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: 0,
                        runs,
                    });
                    current_y += self.line_height;
                }

                current_y + 2
            }
        }
    }

    /// Layout an inline block (paragraph, heading, etc.)
    fn layout_inline_block(
        &mut self,
        block: &Block,
        block_idx: usize,
        y: i32,
        start_x: i32,
        width: i32,
        line_height: i32,
        ctx: &mut dyn DrawContext,
    ) -> i32 {
        let (lines, y_after) = self.layout_inline_content(
            &block.content,
            &block.block_type,
            block_idx,
            y,
            start_x,
            width,
            line_height,
            ctx,
        );

        let mut current_y = y;
        for line_runs in lines {
            self.layout_lines.push(LayoutLine {
                y: current_y,
                height: line_height,
                block_index: block_idx,
                char_start: 0,
                char_end: 0,
                runs: line_runs,
            });
            current_y += line_height;
        }

        if current_y == y {
            // Empty block
            current_y + line_height
        } else {
            current_y + 5
        }
    }

    /// Layout inline content with word wrapping
    /// Returns (lines of runs, final_y)
    fn layout_inline_content(
        &mut self,
        content: &[InlineContent],
        block_type: &BlockType,
        block_idx: usize,
        y: i32,
        start_x: i32,
        width: i32,
        line_height: i32,
        ctx: &mut dyn DrawContext,
    ) -> (Vec<Vec<VisualRun>>, i32) {
        let mut lines: Vec<Vec<VisualRun>> = Vec::new();
        let mut current_line: Vec<VisualRun> = Vec::new();
        let mut current_x = start_x;
        let mut current_y = y;
        let mut char_offset = 0;

        // Determine if this is a heading - if so, use heading style for all text
        let block_style_override = match block_type {
            BlockType::Heading { .. } => Some(self.get_style_for_block_type(block_type)),
            _ => None,
        };

        for (inline_idx, item) in content.iter().enumerate() {
            match item {
                InlineContent::Text(run) => {
                    // Use block style for headings, otherwise use text style
                    let style_idx = if let Some(override_style) = block_style_override {
                        override_style
                    } else {
                        self.get_style_for_text_style(&run.style)
                    };
                    let (font, size) = self.get_font_for_style(style_idx);

                    // Word wrap
                    for word in run.text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_len = word_with_space.len();
                        let word_width = ctx.text_width(&word_with_space, font, size) as i32;

                        if current_x + word_width > start_x + width && current_x > start_x {
                            // Wrap to next line
                            lines.push(current_line);
                            current_line = Vec::new();
                            current_x = start_x;
                            current_y += line_height;
                        }

                        current_line.push(VisualRun {
                            text: word_with_space,
                            x: current_x,
                            style_idx,
                            block_index: block_idx,
                            char_range: (char_offset, char_offset + word_len),
                            inline_index: Some(inline_idx),
                        });

                        current_x += word_width;
                        char_offset += word_len;
                    }
                }
                InlineContent::Link { link: _, content: link_content } => {
                    // Render link content
                    // For simplicity, treat as styled text
                    let style_idx = 5; // STYLE_LINK
                    let text = link_content.iter()
                        .map(|c| c.to_plain_text())
                        .collect::<String>();

                    let (font, size) = self.get_font_for_style(style_idx);
                    let text_width = ctx.text_width(&text, font, size) as i32;

                    if current_x + text_width > start_x + width && current_x > start_x {
                        lines.push(current_line);
                        current_line = Vec::new();
                        current_x = start_x;
                        current_y += line_height;
                    }

                    current_line.push(VisualRun {
                        text: text.clone(),
                        x: current_x,
                        style_idx,
                        block_index: block_idx,
                        char_range: (char_offset, char_offset + text.len()),
                        inline_index: Some(inline_idx),
                    });

                    current_x += text_width;
                    char_offset += text.len();
                }
                InlineContent::LineBreak => {
                    current_line.push(VisualRun {
                        text: " ".to_string(),
                        x: current_x,
                        style_idx: 0,
                        block_index: block_idx,
                        char_range: (char_offset, char_offset + 1),
                        inline_index: Some(inline_idx),
                    });
                    current_x += ctx.text_width(" ", self.text_font, self.text_size) as i32;
                    char_offset += 1;
                }
                InlineContent::HardBreak => {
                    lines.push(current_line);
                    current_line = Vec::new();
                    current_x = start_x;
                    current_y += line_height;
                    char_offset += 1;
                }
            }
        }

        let is_empty = lines.is_empty();

        if !current_line.is_empty() {
            lines.push(current_line);
        }

        (lines, if is_empty { y } else { current_y })
    }

    /// Get style index for block type
    fn get_style_for_block_type(&self, block_type: &BlockType) -> u8 {
        match block_type {
            BlockType::Heading { level } => match level {
                1 => 6, // STYLE_HEADER1
                2 => 7, // STYLE_HEADER2
                _ => 8, // STYLE_HEADER3
            },
            BlockType::CodeBlock { .. } => 4, // STYLE_CODE
            BlockType::BlockQuote => 9,       // STYLE_QUOTE
            _ => 0,                           // STYLE_PLAIN
        }
    }

    /// Get style index for text style
    fn get_style_for_text_style(&self, style: &TextStyle) -> u8 {
        if style.code {
            4 // STYLE_CODE
        } else if style.bold && style.italic {
            3 // STYLE_BOLD_ITALIC
        } else if style.bold {
            1 // STYLE_BOLD
        } else if style.italic {
            2 // STYLE_ITALIC
        } else {
            0 // STYLE_PLAIN
        }
    }

    /// Get font for style
    fn get_font_for_style(&self, style_idx: u8) -> (u8, u8) {
        if (style_idx as usize) < self.style_table.len() {
            let style = &self.style_table[style_idx as usize];
            (style.font, style.size)
        } else {
            (self.text_font, self.text_size)
        }
    }

    /// Draw the widget
    pub fn draw(&mut self, ctx: &mut dyn DrawContext) {
        self.layout(ctx);

        // Draw background
        ctx.set_color(self.background_color);
        ctx.draw_rect_filled(self.x, self.y, self.w, self.h);

        // Set up clipping
        ctx.push_clip(self.x, self.y, self.w, self.h);

        // Draw visible lines
        let viewport_top = self.scroll_offset;
        let viewport_bottom = self.scroll_offset + self.h;

        for line in &self.layout_lines {
            if line.y + line.height < viewport_top || line.y > viewport_bottom {
                continue;
            }

            for run in &line.runs {
                // Check if this run is part of a hovered link
                let is_hovered = self.hovered_link.map_or(false, |(block_idx, inline_idx)| {
                    run.block_index == block_idx && run.inline_index == Some(inline_idx)
                });

                // Use hover style if applicable
                let mut style_idx = run.style_idx;
                if is_hovered && run.style_idx == 5 {
                    // Link style -> Link hover style
                    if self.style_table.len() > 10 {
                        style_idx = 10; // STYLE_LINK_HOVER
                    }
                }

                let style = if (style_idx as usize) < self.style_table.len() {
                    &self.style_table[style_idx as usize]
                } else {
                    continue;
                };

                ctx.set_font(style.font, style.size);
                ctx.set_color(style.color);

                let draw_y = self.y + line.y - self.scroll_offset + style.size as i32;
                let draw_x = self.x + run.x;

                // Draw background for hover (if link is hovered)
                if is_hovered {
                    let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;
                    ctx.set_color(style.bgcolor);
                    ctx.draw_rect_filled(draw_x, self.y + line.y - self.scroll_offset, text_width, line.height);
                    ctx.set_color(style.color); // Restore text color
                }

                ctx.draw_text(&run.text, draw_x, draw_y);

                // Draw underline if needed
                if style.attr & 0x0004 != 0 {
                    let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;
                    ctx.draw_line(draw_x, draw_y + 2, draw_x + text_width, draw_y + 2);
                }
            }
        }

        // Draw cursor
        if self.cursor_visible {
            if let Some((cx, cy, ch)) = self.get_cursor_visual_position() {
                let screen_y = self.y + cy - self.scroll_offset;
                let screen_x = self.x + cx;

                if screen_y >= self.y && screen_y < self.y + self.h {
                    ctx.set_color(self.cursor_color);
                    ctx.draw_line(screen_x, screen_y, screen_x, screen_y + ch);
                }
            }
        }

        ctx.pop_clip();
    }

    /// Get visual position of cursor (x, y, height) relative to widget
    fn get_cursor_visual_position(&self) -> Option<(i32, i32, i32)> {
        let cursor = self.editor.cursor();
        let doc = self.editor.document();

        if cursor.block_index >= doc.block_count() {
            return None;
        }

        // Find the layout line containing the cursor
        for line in &self.layout_lines {
            if line.block_index == cursor.block_index {
                // Find position within line
                if cursor.offset <= line.char_end {
                    let mut x = self.padding_left;

                    for run in &line.runs {
                        if cursor.offset >= run.char_range.0 && cursor.offset <= run.char_range.1 {
                            // Cursor is in this run
                            let offset_in_run = cursor.offset - run.char_range.0;
                            x = run.x + (offset_in_run as i32 * 8); // Approximate
                            return Some((x, line.y, line.height));
                        }

                        if cursor.offset > run.char_range.1 {
                            x = run.x + (run.text.len() as i32 * 8);
                        }
                    }

                    return Some((x, line.y, line.height));
                }
            }
        }

        // Default: top-left
        Some((self.padding_left, self.padding_top, self.line_height))
    }

    /// Convert x,y screen coordinates to document position
    pub fn xy_to_position(&self, x: i32, y: i32) -> DocumentPosition {
        let adjusted_y = y + self.scroll_offset;

        for line in &self.layout_lines {
            if adjusted_y >= line.y && adjusted_y < line.y + line.height {
                let block_index = line.block_index;
                let mut offset = 0;

                for run in &line.runs {
                    let run_end_x = run.x + (run.text.len() as i32 * 8);
                    if x >= run.x && x < run_end_x {
                        offset = run.char_range.0;
                        break;
                    }
                    if x >= run_end_x {
                        offset = run.char_range.1;
                    }
                }

                return DocumentPosition::new(block_index, offset);
            }
        }

        // Default: end of document
        let doc = self.editor.document();
        if doc.block_count() > 0 {
            DocumentPosition::new(doc.block_count() - 1, 0)
        } else {
            DocumentPosition::start()
        }
    }

    /// Set cursor visibility
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    /// Get cursor visibility
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Find link at given widget coordinates (relative to widget, not screen)
    /// Returns ((block_index, inline_index), destination) if a link is found
    pub fn find_link_at(&self, x: i32, y: i32) -> Option<((usize, usize), String)> {
        let adjusted_y = y + self.scroll_offset;
        let adjusted_x = x;

        // Find the line at this y position
        for line in &self.layout_lines {
            if adjusted_y >= line.y && adjusted_y < line.y + line.height {
                // Find the run at this x position
                for run in &line.runs {
                    // Estimate run width
                    let estimated_width = (run.text.len() as i32) * 8; // rough estimate

                    if adjusted_x >= run.x && adjusted_x < run.x + estimated_width {
                        // Check if this run has an inline_index pointing to a link
                        if let Some(inline_idx) = run.inline_index {
                            let doc = self.editor.document();
                            if run.block_index < doc.block_count() {
                                let block = &doc.blocks()[run.block_index];
                                if inline_idx < block.content.len() {
                                    if let InlineContent::Link { link, .. } = &block.content[inline_idx] {
                                        return Some(((run.block_index, inline_idx), link.destination.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Set hovered link (for hover highlighting)
    pub fn set_hovered_link(&mut self, link: Option<(usize, usize)>) {
        if self.hovered_link != link {
            self.hovered_link = link;
            // Don't invalidate layout, just trigger redraw
        }
    }

    /// Get hovered link
    pub fn hovered_link(&self) -> Option<(usize, usize)> {
        self.hovered_link
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_display_creation() {
        let display = StructuredRichDisplay::new(0, 0, 800, 600);
        assert_eq!(display.w(), 800);
        assert_eq!(display.h(), 600);
    }
}
