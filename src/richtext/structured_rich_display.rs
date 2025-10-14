// Structured Rich Text Display
// A rendering and interaction widget for StructuredDocument
// Completely decoupled from markdown syntax

use super::structured_document::*;
use super::structured_editor::*;
use crate::sourceedit::text_display::{style_attr, DrawContext, StyleTableEntry};

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
    /// Width of the text
    width: i32,
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
    // Cursor blink state
    blink_on: bool,
    blink_period_ms: u64,

    // Selection rendering
    selection_color: u32,

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
            blink_on: true,
            blink_period_ms: 1000,       // 1s full period (500ms on/off)
            selection_color: 0xB4D5FEFF, // Light blue selection color
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

    /// Ensure the cursor's line is visible by adjusting scroll offset
    /// Minimally scrolls to bring the cursor line within the viewport with small margins
    pub fn ensure_cursor_visible(&mut self, ctx: &mut dyn DrawContext) {
        // Ensure layout is up to date
        self.layout(ctx);

        // Get cursor visual position (content coords)
        if let Some((_cx, cy, ch)) = self.get_cursor_visual_position(ctx) {
            let viewport_top = self.scroll_offset;
            let viewport_bottom = self.scroll_offset + self.h;

            // Provide a small comfort margin around the cursor line
            let margin_top = 8;
            let margin_bottom = 8;

            let mut new_scroll = self.scroll_offset;

            // If above viewport (with margin), scroll up
            if cy < viewport_top + margin_top {
                new_scroll = (cy - margin_top).max(0);
            }

            // If below viewport (with margin), scroll down
            if cy + ch > viewport_bottom - margin_bottom {
                new_scroll = (cy + ch + margin_bottom - self.h).max(0);
            }

            // Clamp to content height
            let max_scroll = (self.content_height() - self.h).max(0);
            if new_scroll > max_scroll {
                new_scroll = max_scroll;
            }

            self.scroll_offset = new_scroll;
        }
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

    pub fn x(&self) -> i32 {
        self.x
    }
    pub fn y(&self) -> i32 {
        self.y
    }
    pub fn w(&self) -> i32 {
        self.w
    }
    pub fn h(&self) -> i32 {
        self.h
    }

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
                // Add top margin for headings (unless it's the first block)
                let top_margin = if block_idx > 0 { 15 } else { 0 };
                let y_after = self.layout_inline_block(
                    block,
                    block_idx,
                    y + top_margin,
                    start_x,
                    width,
                    height,
                    ctx,
                );
                y_after + 10 // Extra spacing after headings
            }
            BlockType::CodeBlock { .. } => {
                let text = block.to_plain_text();
                let lines: Vec<&str> = text.lines().collect();
                let style_idx = self.get_style_for_block_type(&block.block_type);
                let (font, size) = self.get_font_for_style(style_idx);
                let mut current_y = y + 5;

                for line in lines {
                    let line_width = ctx.text_width(line, font, size) as i32;
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: line.len(),
                        runs: vec![VisualRun {
                            text: line.to_string(),
                            x: start_x + 10,
                            width: line_width,
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
                self.layout_inline_block(
                    block,
                    block_idx,
                    y + 5,
                    start_x + 20,
                    width - 20,
                    self.line_height,
                    ctx,
                ) + 5
            }
            BlockType::ListItem { .. } => {
                // Indent: bullet is 1x text_size from left, text is another 1x text_size in
                let bullet_indent = self.text_size as i32;
                let text_indent = bullet_indent * 2;

                let bullet_text = "• ";
                let bullet_width =
                    ctx.text_width(bullet_text, self.text_font, self.text_size) as i32;

                let mut runs = vec![VisualRun {
                    text: bullet_text.to_string(),
                    x: start_x + bullet_indent,
                    width: bullet_width,
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

                    // Calculate char range from content runs (skip bullet)
                    let char_start = runs
                        .iter()
                        .skip(1)
                        .map(|r| r.char_range.0)
                        .min()
                        .unwrap_or(0);
                    let char_end = runs
                        .iter()
                        .skip(1)
                        .map(|r| r.char_range.1)
                        .max()
                        .unwrap_or(0);

                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        block_index: block_idx,
                        char_start,
                        char_end,
                        runs,
                    });
                    current_y += self.line_height;

                    // Add remaining lines
                    for line_runs in content_runs.iter().skip(1) {
                        let char_start = line_runs.first().map(|r| r.char_range.0).unwrap_or(0);
                        let char_end = line_runs.last().map(|r| r.char_range.1).unwrap_or(0);

                        self.layout_lines.push(LayoutLine {
                            y: current_y,
                            height: self.line_height,
                            block_index: block_idx,
                            char_start,
                            char_end,
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

        if lines.is_empty() {
            // Empty block - create an empty layout line for cursor positioning
            self.layout_lines.push(LayoutLine {
                y: current_y,
                height: line_height,
                block_index: block_idx,
                char_start: 0,
                char_end: 0,
                runs: Vec::new(),
            });
            current_y += line_height;
        } else {
            for line_runs in lines {
                // Calculate char_start and char_end from the runs
                let char_start = line_runs.first().map(|r| r.char_range.0).unwrap_or(0);
                let char_end = line_runs.last().map(|r| r.char_range.1).unwrap_or(0);

                self.layout_lines.push(LayoutLine {
                    y: current_y,
                    height: line_height,
                    block_index: block_idx,
                    char_start,
                    char_end,
                    runs: line_runs,
                });
                current_y += line_height;
            }
        }

        current_y + 5
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

                    // Word wrap - track actual positions in original text
                    let text = &run.text;
                    let mut word_start = 0;
                    let mut in_word = false;
                    let mut leading_space_handled = false;

                    for (i, ch) in text
                        .char_indices()
                        .chain(std::iter::once((text.len(), ' ')))
                    {
                        let is_whitespace = ch.is_whitespace();

                        // Handle leading whitespace at the start of the run
                        if !leading_space_handled && is_whitespace && i == 0 {
                            // Find all leading whitespace
                            let mut space_end = 0;
                            for (idx, c) in text.char_indices() {
                                if c.is_whitespace() {
                                    space_end = idx + c.len_utf8();
                                } else {
                                    break;
                                }
                            }

                            if space_end > 0 {
                                let space_text = &text[..space_end];
                                let space_width = ctx.text_width(space_text, font, size) as i32;

                                current_line.push(VisualRun {
                                    text: space_text.to_string(),
                                    x: current_x,
                                    width: space_width,
                                    style_idx,
                                    block_index: block_idx,
                                    char_range: (char_offset, char_offset + space_end),
                                    inline_index: Some(inline_idx),
                                });

                                current_x += space_width;
                            }
                            leading_space_handled = true;
                        }

                        if in_word && (is_whitespace || i == text.len()) {
                            // End of word - extract word with trailing whitespace
                            let mut word_end = i;
                            // Include trailing whitespace in the word
                            while word_end < text.len()
                                && text[word_end..]
                                    .chars()
                                    .next()
                                    .map_or(false, |c| c.is_whitespace() && c != '\n')
                            {
                                word_end += text[word_end..].chars().next().unwrap().len_utf8();
                            }

                            let word_text = &text[word_start..word_end];
                            let word_width = ctx.text_width(word_text, font, size) as i32;

                            if current_x + word_width > start_x + width && current_x > start_x {
                                // Wrap to next line
                                lines.push(current_line);
                                current_line = Vec::new();
                                current_x = start_x;
                                current_y += line_height;
                            }

                            current_line.push(VisualRun {
                                text: word_text.to_string(),
                                x: current_x,
                                width: word_width,
                                style_idx,
                                block_index: block_idx,
                                char_range: (char_offset + word_start, char_offset + word_end),
                                inline_index: Some(inline_idx),
                            });

                            current_x += word_width;
                            in_word = false;
                        } else if !in_word && !is_whitespace {
                            // Start of new word
                            word_start = i;
                            in_word = true;
                        }
                    }

                    char_offset += text.len();
                }
                InlineContent::Link {
                    link: _,
                    content: link_content,
                } => {
                    // Render link content
                    // For simplicity, treat as styled text
                    let style_idx = 5; // STYLE_LINK
                    let text = link_content
                        .iter()
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
                        width: text_width,
                        style_idx,
                        block_index: block_idx,
                        char_range: (char_offset, char_offset + text.len()),
                        inline_index: Some(inline_idx),
                    });

                    current_x += text_width;
                    char_offset += text.len();
                }
                InlineContent::LineBreak => {
                    let space_width = ctx.text_width(" ", self.text_font, self.text_size) as i32;
                    current_line.push(VisualRun {
                        text: " ".to_string(),
                        x: current_x,
                        width: space_width,
                        style_idx: 0,
                        block_index: block_idx,
                        char_range: (char_offset, char_offset + 1),
                        inline_index: Some(inline_idx),
                    });
                    current_x += space_width;
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
    ///
    /// Style indices:
    /// - 0-10: Predefined (plain, bold, italic, bold+italic, code, link, headers, quote, link_hover)
    /// - 11-42: Computed based on style flags (base + decorations)
    ///   Formula: 11 + (base * 8) + decoration_flags
    ///   where base = 0 (plain), 1 (bold), 2 (italic), 3 (bold+italic)
    ///   and decoration_flags = (underline ? 1 : 0) | (strikethrough ? 2 : 0) | (highlight ? 4 : 0)
    fn get_style_for_text_style(&self, style: &TextStyle) -> u8 {
        if style.code {
            return 4; // STYLE_CODE
        }

        // Determine base style
        let base = if style.bold && style.italic {
            3
        } else if style.bold {
            1
        } else if style.italic {
            2
        } else {
            0
        };

        // Check if any decorations are present
        if !style.underline && !style.strikethrough && !style.highlight {
            return base; // Just base style (0, 1, 2, or 3)
        }

        // Compute decorated style index
        // Decoration bits: underline=1, strikethrough=2, highlight=4
        let decoration = (style.underline as u8)
            | ((style.strikethrough as u8) << 1)
            | ((style.highlight as u8) << 2);

        // Style table layout reserves indices 0..10 for base styles and link variants.
        // Decorated styles are appended as 7 entries per base style (for decoration values 1..7).
        // Therefore the correct index is 11 + base*7 + (decoration-1).
        11 + base * 7 + (decoration - 1)
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

    /// Check if a visual run intersects with the current selection
    /// Returns None if no selection, or Some((start_offset, end_offset)) relative to the run
    fn get_run_selection_range(&self, run: &VisualRun) -> Option<(usize, usize)> {
        let selection = self.editor.selection()?;
        let (sel_start, sel_end) = selection;

        // Normalize selection so start <= end
        let (sel_start, sel_end) = if sel_start.block_index < sel_end.block_index
            || (sel_start.block_index == sel_end.block_index && sel_start.offset <= sel_end.offset)
        {
            (sel_start, sel_end)
        } else {
            (sel_end, sel_start)
        };

        // Check if this run's block is within the selection
        if run.block_index < sel_start.block_index || run.block_index > sel_end.block_index {
            return None;
        }

        // Determine the selection range within this run
        let run_start = run.char_range.0;
        let run_end = run.char_range.1;

        let sel_start_offset = if run.block_index == sel_start.block_index {
            sel_start.offset
        } else {
            0
        };

        let sel_end_offset = if run.block_index == sel_end.block_index {
            sel_end.offset
        } else {
            usize::MAX
        };

        // Check if run intersects with selection
        if run_end <= sel_start_offset || run_start >= sel_end_offset {
            return None;
        }

        // Calculate the intersection
        let start_in_run = sel_start_offset
            .saturating_sub(run_start)
            .min(run_end - run_start);
        let end_in_run = sel_end_offset
            .saturating_sub(run_start)
            .min(run_end - run_start);

        Some((start_in_run, end_in_run))
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

                // Draw inline highlight background for styles that specify a bgcolor
                // (e.g., text highlight). Draw this first so selection can paint over it.
                {
                    let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;
                    if (style.attr & style_attr::BGCOLOR) != 0 {
                        ctx.set_color(style.bgcolor);
                        ctx.draw_rect_filled(
                            draw_x,
                            self.y + line.y - self.scroll_offset,
                            text_width,
                            line.height,
                        );
                        ctx.set_color(style.color); // Restore text color for text drawing
                    }
                }

                // Draw background for hover (if link is hovered)
                // Draw this BEFORE selection so selection remains visible on top
                if is_hovered {
                    let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;
                    ctx.set_color(style.bgcolor);
                    ctx.draw_rect_filled(
                        draw_x,
                        self.y + line.y - self.scroll_offset,
                        text_width,
                        line.height,
                    );
                    ctx.set_color(style.color); // Restore text color
                }

                // Draw selection highlight (if run is selected)
                // Draw AFTER hover so the selection rectangle is on top
                if let Some((sel_start, sel_end)) = self.get_run_selection_range(run) {
                    if sel_end > sel_start {
                        // Measure the text before and within selection
                        let text_before = if sel_start < run.text.len() {
                            &run.text[..sel_start]
                        } else {
                            &run.text
                        };
                        let text_selected = if sel_end <= run.text.len() {
                            &run.text[sel_start..sel_end]
                        } else if sel_start < run.text.len() {
                            &run.text[sel_start..]
                        } else {
                            ""
                        };

                        let before_width =
                            ctx.text_width(text_before, style.font, style.size) as i32;
                        let sel_width =
                            ctx.text_width(text_selected, style.font, style.size) as i32;

                        ctx.set_color(self.selection_color);
                        ctx.draw_rect_filled(
                            draw_x + before_width,
                            self.y + line.y - self.scroll_offset,
                            sel_width,
                            line.height,
                        );
                        ctx.set_color(style.color); // Restore text color
                    }
                }

                ctx.draw_text(&run.text, draw_x, draw_y);

                let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;

                // Draw underline if needed (0x0004 = UNDERLINE)
                if style.attr & 0x0004 != 0 {
                    ctx.draw_line(draw_x, draw_y + 2, draw_x + text_width, draw_y + 2);
                }

                // Draw strikethrough if needed (0x0010 = STRIKE_THROUGH)
                if style.attr & 0x0010 != 0 {
                    // Draw line through middle of text (roughly at half the font size)
                    let strike_y = draw_y - (style.size as i32) / 2;
                    ctx.draw_line(draw_x, strike_y, draw_x + text_width, strike_y);
                }
            }
        }

        // Draw cursor (only when widget has keyboard focus)
        if self.cursor_visible && ctx.has_focus() && self.blink_on {
            if let Some((cx, cy, ch)) = self.get_cursor_visual_position(ctx) {
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
    fn get_cursor_visual_position(&self, ctx: &mut dyn DrawContext) -> Option<(i32, i32, i32)> {
        let cursor = self.editor.cursor();
        let doc = self.editor.document();

        if cursor.block_index >= doc.block_count() {
            return None;
        }

        // Find the layout line containing the cursor
        for line in &self.layout_lines {
            if line.block_index == cursor.block_index {
                // Check if cursor falls within this line's character range
                if cursor.offset >= line.char_start && cursor.offset <= line.char_end {
                    let mut x = self.padding_left;

                    for run in &line.runs {
                        // Skip non-content runs (like list bullets with char_range (0,0))
                        if run.char_range.0 == run.char_range.1 && run.inline_index.is_none() {
                            continue;
                        }

                        if cursor.offset >= run.char_range.0 && cursor.offset <= run.char_range.1 {
                            // Cursor is in this run - measure actual text width
                            let offset_in_run = cursor.offset - run.char_range.0;
                            let (font, size) = self.get_font_for_style(run.style_idx);

                            // Measure the text up to the cursor position
                            let text_before_cursor = if offset_in_run < run.text.len() {
                                &run.text[..offset_in_run]
                            } else {
                                &run.text
                            };

                            let width_before =
                                ctx.text_width(text_before_cursor, font, size) as i32;
                            x = run.x + width_before;
                            return Some((x, line.y, line.height));
                        }

                        if cursor.offset > run.char_range.1 {
                            // Cursor is after this run - measure full run width
                            let (font, size) = self.get_font_for_style(run.style_idx);
                            x = run.x + ctx.text_width(&run.text, font, size) as i32;
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

        if self.layout_lines.is_empty() {
            return DocumentPosition::start();
        }

        // Helper to compute offset within a specific line based on x
        fn offset_in_line(line: &LayoutLine, x: i32) -> usize {
            let mut offset = line.char_start;
            for run in &line.runs {
                let run_end_x = run.x + run.width;
                if x >= run.x && x < run_end_x {
                    let click_offset_in_run = x - run.x;
                    let chars_in_run = run.char_range.1 - run.char_range.0;
                    if run.width > 0 && chars_in_run > 0 {
                        let char_pos = ((click_offset_in_run * chars_in_run as i32) / run.width)
                            .clamp(0, chars_in_run as i32 - 1)
                            as usize;
                        return run.char_range.0 + char_pos;
                    } else {
                        return run.char_range.0;
                    }
                }
                if x >= run_end_x {
                    offset = run.char_range.1;
                }
            }
            offset
        }

        // First try: direct hit on a line
        for line in &self.layout_lines {
            if adjusted_y >= line.y && adjusted_y < line.y + line.height {
                let block_index = line.block_index;
                let offset = offset_in_line(line, x);
                return DocumentPosition::new(block_index, offset);
            }
        }

        // No line directly under the cursor. Choose the nearest line vertically.
        // Find the previous line (the last line with y <= adjusted_y)
        let mut prev_idx: Option<usize> = None;
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.y <= adjusted_y {
                prev_idx = Some(i);
            } else {
                break;
            }
        }

        // Find the next line (first with y > adjusted_y)
        let next_idx = self
            .layout_lines
            .iter()
            .enumerate()
            .find(|(_, line)| line.y > adjusted_y)
            .map(|(i, _)| i);

        let target_idx = match (prev_idx, next_idx) {
            (Some(p), Some(n)) => {
                // Distance to bottom of previous vs top of next
                let prev_dist = adjusted_y - (self.layout_lines[p].y + self.layout_lines[p].height);
                let next_dist = self.layout_lines[n].y - adjusted_y;
                if prev_dist <= next_dist {
                    p
                } else {
                    n
                }
            }
            (Some(p), None) => p,
            (None, Some(n)) => n,
            (None, None) => 0,
        };

        let line = &self.layout_lines[target_idx];
        let block_index = line.block_index;
        let offset = offset_in_line(line, x);
        DocumentPosition::new(block_index, offset)
    }

    /// Set cursor visibility
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor_visible = visible;
    }

    /// Get cursor visibility
    pub fn cursor_visible(&self) -> bool {
        self.cursor_visible
    }

    /// Update blink state based on elapsed ms. Returns true if visual state changed.
    pub fn tick(&mut self, ms_since_start: u64) -> bool {
        // Compute a simple square-wave blink: 500ms on, 500ms off
        let half_period = (self.blink_period_ms / 2).max(1);
        let new_on = (ms_since_start / half_period) % 2 == 0;
        if new_on != self.blink_on {
            self.blink_on = new_on;
            return true;
        }
        false
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
                    if adjusted_x >= run.x && adjusted_x < run.x + run.width {
                        // Check if this run has an inline_index pointing to a link
                        if let Some(inline_idx) = run.inline_index {
                            let doc = self.editor.document();
                            if run.block_index < doc.block_count() {
                                let block = &doc.blocks()[run.block_index];
                                if inline_idx < block.content.len() {
                                    if let InlineContent::Link { link, .. } =
                                        &block.content[inline_idx]
                                    {
                                        return Some((
                                            (run.block_index, inline_idx),
                                            link.destination.clone(),
                                        ));
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

    /// Find a link at or adjacent to the current cursor position.
    ///
    /// Treats the cursor as "inside" the link when its offset lies within the
    /// link's text range, and also when it is exactly at the start (directly
    /// before) or exactly at the end (directly after) of the link.
    ///
    /// Returns ((block_index, inline_index), destination) if a link is found.
    pub fn find_link_near_cursor(&self) -> Option<((usize, usize), String)> {
        let cursor = self.editor.cursor();
        let doc = self.editor.document();

        if cursor.block_index >= doc.block_count() {
            return None;
        }

        let block = &doc.blocks()[cursor.block_index];
        let mut pos = 0usize;

        for (inline_idx, item) in block.content.iter().enumerate() {
            let len = item.text_len();
            if let InlineContent::Link { link, .. } = item {
                let start = pos;
                let end = pos + len; // end is exclusive for text, but we allow equality for adjacency

                // Cursor is within, or exactly at start/end (adjacent)
                if cursor.offset >= start && cursor.offset <= end {
                    return Some(((cursor.block_index, inline_idx), link.destination.clone()));
                }
            }
            pos += len;
        }

        None
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
