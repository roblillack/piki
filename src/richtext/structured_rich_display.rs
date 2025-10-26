// Structured Rich Text Display
// A rendering and interaction widget for StructuredDocument
// Completely decoupled from markdown syntax

use super::structured_document::*;
use super::structured_editor::*;
use crate::draw_context::DrawContext;
use crate::draw_context::FontStyle;
use crate::draw_context::FontType;
use crate::theme::{FontSettings, Theme};

/// Layout information for a rendered line
#[derive(Debug, Clone)]
struct LayoutLine {
    /// Y position of the line's baseline
    y: i32,
    /// Height of the line
    height: i32,
    /// Default x position for the cursor when the line has no text runs
    base_x: i32,
    /// Block index this line belongs to
    block_index: usize,
    /// Character offset range within the block [start, end)
    char_start: usize,
    char_end: usize,
    /// Preferred visual end offset for cursor placement
    visual_char_end: usize,
    /// Visual elements on this line
    runs: Vec<VisualRun>,
}

impl LayoutLine {
    /// Check if the line has no text runs
    fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

/// Checklist marker rendering metadata
#[derive(Debug, Clone)]
struct ChecklistVisual {
    /// Whether the checklist item is checked
    checked: bool,
    /// Size of the checkbox square in pixels
    box_size: i32,
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
    /// Block index this belongs to
    block_index: usize,
    /// Character range within block
    char_range: (usize, usize),
    /// Inline content index (for link detection)
    inline_index: Option<usize>,
    /// Checklist rendering info (if this run is a checklist marker)
    checklist: Option<ChecklistVisual>,

    font_type: FontType,
    font_style: FontStyle,
    font_size: u8,
    font_color: u32,
    background_color: Option<u32>,
    underline: bool,
    strikethrough: bool,
    highlight: bool,
}

#[derive(Clone, Copy)]
struct ResolvedRunStyle {
    font_type: FontType,
    font_style: FontStyle,
    font_size: u8,
    font_color: u32,
    background_color: Option<u32>,
    underline: bool,
    strikethrough: bool,
    highlight: bool,
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

    // Cursor display
    cursor_visible: bool,
    // Cursor blink state
    blink_on: bool,
    blink_period_ms: u64,

    // Link hover state
    hovered_link: Option<(usize, usize)>, // (block_index, inline_index)

    // Sticky horizontal position for vertical navigation across proportional fonts
    cursor_preferred_line_offset: Option<usize>,

    // Theme
    theme: Theme,
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
            cursor_visible: true,
            blink_on: true,
            blink_period_ms: 1000, // 1s full period (500ms on/off)
            hovered_link: None,
            cursor_preferred_line_offset: None,
            theme: Theme::default(),
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
            last_line.y + last_line.height + self.theme.padding_vertical
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

        let content_width = self.w - 2 * self.theme.padding_horizontal;
        let mut current_y = self.theme.padding_vertical;

        // Clone blocks to avoid borrow checker issues
        let blocks = self.editor.document().blocks().to_vec();

        for (block_idx, block) in blocks.iter().enumerate() {
            current_y = self.layout_block(block, block_idx, current_y, content_width, ctx);
        }

        self.layout_valid = true;
    }

    /// Compute a precise character offset within a given line for a desired x position using font metrics.
    pub fn precise_offset_in_line(
        &self,
        line: &LayoutLine,
        x: i32,
        ctx: &mut dyn DrawContext,
    ) -> usize {
        let mut offset = line.char_start;
        for run in &line.runs {
            let run_end_x = run.x + run.width;
            if x < run.x {
                // Before this run: cursor snaps to beginning of this run
                return offset;
            }
            if x >= run.x && x < run_end_x {
                // Within this run: walk characters and measure
                let (font, fstyle, size) = (run.font_type, run.font_style, run.font_size);
                let mut last_offset = run.char_range.0;
                // Iterate char boundaries in the run's text
                for (i, _) in run
                    .text
                    .char_indices()
                    .chain(std::iter::once((run.text.len(), ' ')))
                {
                    let w = ctx.text_width(&run.text[..i], font, fstyle, size) as i32;
                    if run.x + w > x {
                        return last_offset;
                    }
                    last_offset = run.char_range.0 + i;
                }
                return run.char_range.1;
            }
            // After this run: advance offset
            offset = run.char_range.1;
        }
        offset
    }

    /// Find the index of the visual line containing the current cursor. If no exact match,
    /// returns the nearest line within the same block, otherwise None.
    fn is_last_visual_line_in_block(&self, line_index: usize) -> bool {
        if line_index >= self.layout_lines.len() {
            return true;
        }
        let block_idx = self.layout_lines[line_index].block_index;
        !self.layout_lines[line_index + 1..]
            .iter()
            .any(|line| line.block_index == block_idx)
    }

    fn offset_belongs_to_line(&self, line_index: usize, offset: usize) -> bool {
        if line_index >= self.layout_lines.len() {
            return false;
        }
        let line = &self.layout_lines[line_index];
        if line.char_start == line.char_end {
            return offset == line.char_start;
        }
        if offset < line.char_start {
            return false;
        }
        if offset < line.char_end {
            return true;
        }
        if offset == line.char_end && line.visual_char_end == line.char_end {
            return true;
        }
        self.is_last_visual_line_in_block(line_index) && offset == line.char_end
    }

    /// Get the character offset within the visual line for a given block index and offset.
    fn visual_line_offset(&self, pos: DocumentPosition) -> Option<usize> {
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.block_index == pos.block_index && self.offset_belongs_to_line(i, pos.offset) {
                return Some(pos.offset - line.char_start);
            }
        }
        None
    }

    fn compute_visual_char_end(&self, runs: &[VisualRun], char_end: usize, wrapped: bool) -> usize {
        if !wrapped {
            return char_end;
        }

        for run in runs.iter().rev() {
            if run.char_range.0 == run.char_range.1 {
                continue;
            }

            if run.text.is_empty() {
                continue;
            }

            let trimmed = run.text.trim_end_matches(|c: char| c.is_whitespace());
            if trimmed.len() == run.text.len() {
                return run.char_range.1;
            }

            if trimmed.is_empty() {
                continue;
            }

            let delta = run.text.len() - trimmed.len();
            return run.char_range.1.saturating_sub(delta);
        }

        char_end
    }

    fn current_line_index_for_cursor(&self) -> Option<usize> {
        let cursor = self.editor.cursor();
        // First, look for a line in the same block whose char range contains the offset
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.block_index == cursor.block_index
                && self.offset_belongs_to_line(i, cursor.offset)
            {
                return Some(i);
            }
        }
        // Fallback: closest line in the same block by char range proximity
        let mut candidate: Option<(usize, usize)> = None; // (index, distance)
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.block_index == cursor.block_index {
                let dist = if cursor.offset < line.char_start {
                    line.char_start - cursor.offset
                } else {
                    cursor.offset.saturating_sub(line.char_end)
                };
                candidate = match candidate {
                    Some((_, best_dist)) if best_dist <= dist => candidate,
                    _ => Some((i, dist)),
                };
            }
        }
        candidate.map(|(i, _)| i)
    }

    pub fn record_preferred_pos(&mut self, pos: DocumentPosition) {
        self.cursor_preferred_line_offset = self.visual_line_offset(pos);
    }

    fn get_preferred_pos_for_line(&self, line: &LayoutLine) -> DocumentPosition {
        let preferred_offset = self
            .cursor_preferred_line_offset
            .unwrap_or(self.visual_line_offset(self.editor.cursor()).unwrap_or(0));

        let line_visual_len = line.visual_char_end.saturating_sub(line.char_start);
        let effective_offset = preferred_offset.min(line_visual_len);

        println!(
            "Preferred offset: {}, line len: {}, line visual len: {}, effective offset: {}",
            preferred_offset,
            line.char_end - line.char_start,
            line_visual_len,
            effective_offset
        );

        DocumentPosition::new(line.block_index, line.char_start + effective_offset)
    }

    /// Move cursor one visual line up, using wrapped lines when applicable.
    /// When `extend` is true, extends the selection (Shift+Up behavior).
    pub fn move_cursor_visual_up(&mut self, extend: bool, ctx: &mut dyn DrawContext) {
        // Ensure layout is current for measurement
        self.layout(ctx);
        if self.layout_lines.is_empty() {
            if extend {
                self.editor.move_cursor_up_extend();
            } else {
                self.editor.move_cursor_up();
            }
            return;
        }

        let cursor = self.editor.cursor();
        let cur_idx = match self.current_line_index_for_cursor() {
            Some(i) => i,
            None => {
                if extend {
                    self.editor.move_cursor_up_extend();
                } else {
                    self.editor.move_cursor_up();
                }
                return;
            }
        };
        if cur_idx == 0 {
            // Already at the first line
            return;
        }
        let target_line = &self.layout_lines[cur_idx - 1];

        let new_pos = self.get_preferred_pos_for_line(target_line);
        if extend {
            self.editor.extend_selection_to(new_pos);
        } else {
            self.editor.set_cursor(new_pos);
        }
    }

    /// Move cursor one visual line down, using wrapped lines when applicable.
    /// When `extend` is true, extends the selection (Shift+Down behavior).
    pub fn move_cursor_visual_down(&mut self, extend: bool, ctx: &mut dyn DrawContext) {
        self.layout(ctx);
        if self.layout_lines.is_empty() {
            if extend {
                self.editor.move_cursor_down_extend();
            } else {
                self.editor.move_cursor_down();
            }
            return;
        }

        let cursor = self.editor.cursor();
        let cur_idx = match self.current_line_index_for_cursor() {
            Some(i) => i,
            None => {
                if extend {
                    self.editor.move_cursor_down_extend();
                } else {
                    self.editor.move_cursor_down();
                }
                return;
            }
        };
        if cur_idx + 1 >= self.layout_lines.len() {
            // Already at the last line
            return;
        }
        let target_line = &self.layout_lines[cur_idx + 1];
        let new_pos = self.get_preferred_pos_for_line(target_line);
        if extend {
            self.editor.extend_selection_to(new_pos);
        } else {
            self.editor.set_cursor(new_pos);
        }
    }

    /// Move cursor to the beginning of the current visual line.
    pub fn move_cursor_visual_line_start(&mut self, extend: bool, ctx: &mut dyn DrawContext) {
        self.layout(ctx);
        if self.layout_lines.is_empty() {
            if extend {
                self.editor.move_cursor_to_line_start_extend();
            } else {
                self.editor.move_cursor_to_line_start();
            }
            return;
        }

        let cursor_block = self.editor.cursor().block_index;
        let line_idx = match self.current_line_index_for_cursor() {
            Some(idx) => idx,
            None => {
                if extend {
                    self.editor.move_cursor_to_line_start_extend();
                } else {
                    self.editor.move_cursor_to_line_start();
                }
                return;
            }
        };
        let line = &self.layout_lines[line_idx];
        if line.block_index != cursor_block {
            if extend {
                self.editor.move_cursor_to_line_start_extend();
            } else {
                self.editor.move_cursor_to_line_start();
            }
            return;
        }

        let new_pos = DocumentPosition::new(line.block_index, line.char_start);
        if extend {
            self.editor.extend_selection_to(new_pos);
        } else {
            self.editor.set_cursor(new_pos);
        }
        self.record_preferred_pos(new_pos);
    }

    /// Move cursor to the end of the current visual line.
    pub fn move_cursor_visual_line_end_precise(&mut self, extend: bool, ctx: &mut dyn DrawContext) {
        self.layout(ctx);
        if self.layout_lines.is_empty() {
            if extend {
                self.editor.move_cursor_to_line_end_extend();
            } else {
                self.editor.move_cursor_to_line_end();
            }
            return;
        }

        let cursor_block = self.editor.cursor().block_index;
        let line_idx = match self.current_line_index_for_cursor() {
            Some(idx) => idx,
            None => {
                if extend {
                    self.editor.move_cursor_to_line_end_extend();
                } else {
                    self.editor.move_cursor_to_line_end();
                }
                return;
            }
        };
        let line = &self.layout_lines[line_idx];
        if line.block_index != cursor_block {
            if extend {
                self.editor.move_cursor_to_line_end_extend();
            } else {
                self.editor.move_cursor_to_line_end();
            }
            return;
        }

        let mut target_offset = line.char_end;
        while target_offset > line.char_start
            && !self.offset_belongs_to_line(line_idx, target_offset)
        {
            target_offset -= 1;
        }

        let new_pos = DocumentPosition::new(line.block_index, target_offset);
        if extend {
            self.editor.extend_selection_to(new_pos);
        } else {
            self.editor.set_cursor(new_pos);
        }
        self.record_preferred_pos(new_pos);
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
        let start_x = self.theme.padding_horizontal;
        let default_line_height = self.theme.line_height;

        match &block.block_type {
            BlockType::Paragraph => self.layout_inline_block(
                block,
                block_idx,
                y,
                start_x,
                width,
                default_line_height,
                ctx,
            ),
            BlockType::Heading { level } => {
                let header_font = match level {
                    1 => self.theme.header_level_1,
                    2 => self.theme.header_level_2,
                    _ => self.theme.header_level_3,
                };
                let height = ((header_font.font_size as f32) * 1.3) as i32;
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
                let f = self.theme.code_text;
                let mut current_y = y + 5;
                let code_start_x = start_x + 10;
                let is_empty = lines.is_empty();

                for line in &lines {
                    let line_width =
                        ctx.text_width(line, f.font_type, f.font_style, f.font_size) as i32;
                    let line_len = line.len();
                    let runs = vec![VisualRun {
                        text: (*line).to_string(),
                        x: code_start_x,
                        width: line_width,
                        font_type: f.font_type,
                        font_style: f.font_style,
                        font_size: f.font_size,
                        font_color: f.font_color,
                        background_color: f.background_color,
                        underline: false,
                        strikethrough: false,
                        highlight: false,
                        block_index: block_idx,
                        char_range: (0, line_len),
                        inline_index: None,
                        checklist: None,
                    }];
                    let visual_char_end = self.compute_visual_char_end(&runs, line_len, false);
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: default_line_height,
                        base_x: code_start_x,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: line_len,
                        visual_char_end,
                        runs,
                    });
                    current_y += default_line_height;
                }

                if is_empty {
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: default_line_height,
                        base_x: code_start_x,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: 0,
                        visual_char_end: 0,
                        runs: Vec::new(),
                    });
                    current_y += default_line_height;
                }

                current_y + 10
            }
            BlockType::BlockQuote => {
                // Layout quote content with left padding; drawing of the vertical bar
                // happens during draw() per line based on block type.
                self.layout_inline_block(
                    block,
                    block_idx,
                    y + 5,
                    start_x + 20,
                    width - 20,
                    default_line_height,
                    ctx,
                ) + 5
            }
            BlockType::ListItem {
                ordered,
                number,
                checkbox,
            } => {
                let plain_font = self.theme.plain_text;

                // Base indent padding before label (keeps list labels off the edge)
                let label_left_pad = plain_font.font_size as i32;

                let mut checklist_visual: Option<ChecklistVisual> = None;

                // Determine label text and padding width
                let (label_text, label_pad_width, content_start_x) = if let Some(checked) = checkbox
                {
                    let mut marker_box_size = (plain_font.font_size as i32).saturating_sub(4);
                    if marker_box_size < 8 {
                        marker_box_size = 8;
                    }
                    if marker_box_size > default_line_height {
                        marker_box_size = default_line_height;
                    }

                    let mut space_width = ctx.text_width(
                        " ",
                        plain_font.font_type,
                        plain_font.font_style,
                        plain_font.font_size,
                    ) as i32;
                    if space_width < 4 {
                        space_width = 4;
                    }

                    let label_pad_width = marker_box_size + space_width;
                    let content_start_x = start_x + label_left_pad + label_pad_width;

                    checklist_visual = Some(ChecklistVisual {
                        checked: *checked,
                        box_size: marker_box_size,
                    });

                    (String::new(), label_pad_width, content_start_x)
                } else if *ordered {
                    // Find contiguous ordered list run (adjacent siblings)
                    let doc = self.editor.document();
                    let blocks = doc.blocks();

                    // Find run start
                    let mut run_start = block_idx;
                    while run_start > 0 {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[run_start - 1].block_type
                        {
                            run_start -= 1;
                        } else {
                            break;
                        }
                    }
                    // Find run end
                    let mut run_end = block_idx;
                    while run_end + 1 < blocks.len() {
                        if let BlockType::ListItem { ordered: true, .. } =
                            blocks[run_end + 1].block_type
                        {
                            run_end += 1;
                        } else {
                            break;
                        }
                    }

                    // Determine starting number (default to 1 if None)
                    let first_num = match blocks[run_start].block_type {
                        BlockType::ListItem {
                            ordered: true,
                            number,
                            ..
                        } => number.unwrap_or(1),
                        _ => 1,
                    };

                    // Compute the maximum used number across the run
                    let mut max_num = first_num;
                    for (i, b) in blocks[run_start..=run_end].iter().enumerate() {
                        if let BlockType::ListItem {
                            ordered: true,
                            number,
                            ..
                        } = b.block_type
                        {
                            let n = number.unwrap_or(first_num + i as u64);
                            if n > max_num {
                                max_num = n;
                            }
                        }
                    }

                    #[cfg(test)]
                    mod tests {
                        use super::*;
                        use crate::draw_context::{FontStyle, FontType};
                        use crate::richtext::structured_document::{
                            Block, DocumentPosition, InlineContent, StructuredDocument, TextRun,
                        };

                        struct TestCtx {
                            focus: bool,
                            active: bool,
                        }

                        impl TestCtx {
                            fn new() -> Self {
                                Self {
                                    focus: true,
                                    active: true,
                                }
                            }
                        }

                        impl DrawContext for TestCtx {
                            fn set_color(&mut self, _color: u32) {}
                            fn set_font(&mut self, _font: FontType, _style: FontStyle, _size: u8) {}
                            fn draw_text(&mut self, _text: &str, _x: i32, _y: i32) {}
                            fn draw_rect_filled(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}
                            fn draw_line(&mut self, _x1: i32, _y1: i32, _x2: i32, _y2: i32) {}
                            fn text_width(
                                &mut self,
                                text: &str,
                                _font: FontType,
                                _style: FontStyle,
                                _size: u8,
                            ) -> f64 {
                                text.chars().count() as f64 * 8.0
                            }
                            fn text_height(
                                &self,
                                _font: FontType,
                                _style: FontStyle,
                                size: u8,
                            ) -> i32 {
                                size as i32 + 4
                            }
                            fn text_descent(
                                &self,
                                _font: FontType,
                                _style: FontStyle,
                                _size: u8,
                            ) -> i32 {
                                4
                            }
                            fn push_clip(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}
                            fn pop_clip(&mut self) {}
                            fn color_average(&self, c1: u32, _c2: u32, _weight: f32) -> u32 {
                                c1
                            }
                            fn color_contrast(&self, fg: u32, _bg: u32) -> u32 {
                                fg
                            }
                            fn color_inactive(&self, c: u32) -> u32 {
                                c
                            }
                            fn has_focus(&self) -> bool {
                                self.focus
                            }
                            fn is_active(&self) -> bool {
                                self.active
                            }
                        }

                        #[test]
                        fn hard_break_blank_line_has_nonzero_offset() {
                            let mut display = StructuredRichDisplay::new(0, 0, 400, 200);
                            let mut doc = StructuredDocument::new();
                            let mut block = Block::paragraph(0);
                            block
                                .content
                                .push(InlineContent::Text(TextRun::plain("Hello")));
                            block.content.push(InlineContent::HardBreak);
                            block.content.push(InlineContent::HardBreak);
                            block
                                .content
                                .push(InlineContent::Text(TextRun::plain("World")));
                            doc.add_block(block);

                            {
                                let editor = display.editor_mut();
                                *editor.document_mut() = doc;
                                editor.set_cursor(DocumentPosition::new(0, 0));
                            }

                            let mut ctx = TestCtx::new();
                            display.layout_valid = false;
                            display.layout(&mut ctx);

                            assert!(
                                display.layout_lines.len() >= 3,
                                "expected at least three visual lines, got {}",
                                display.layout_lines.len()
                            );

                            let zero_len_offsets: Vec<usize> = display
                                .layout_lines
                                .iter()
                                .filter(|line| line.char_start == line.char_end)
                                .map(|line| line.char_start)
                                .collect();

                            assert!(
                                zero_len_offsets.iter().any(|&offset| offset > 0),
                                "expected a zero-length line with non-zero offset, got offsets {:?}",
                                zero_len_offsets
                            );
                        }
                    }

                    // Pad width is width of the largest label text (max_num + ". ")
                    let max_label = format!("{}. ", max_num);
                    let label_pad_width = ctx.text_width(
                        &max_label,
                        plain_font.font_type,
                        plain_font.font_style,
                        plain_font.font_size,
                    ) as i32;

                    // Current label text
                    let idx_in_run = block_idx - run_start;
                    let cur_num = number.unwrap_or(first_num + idx_in_run as u64);
                    let label_text = format!("{}. ", cur_num);
                    let content_start_x = start_x + label_left_pad + label_pad_width;

                    (label_text, label_pad_width, content_start_x)
                } else {
                    // Unordered bullet label and fixed width
                    let bullet_text = "• ".to_string();
                    let bullet_width = ctx.text_width(
                        &bullet_text,
                        plain_font.font_type,
                        plain_font.font_style,
                        plain_font.font_size,
                    ) as i32;
                    let content_start_x = start_x + label_left_pad + bullet_width;
                    (bullet_text, bullet_width, content_start_x)
                };

                // Assemble label run
                let mut runs = vec![VisualRun {
                    text: label_text,
                    x: start_x + label_left_pad,
                    width: label_pad_width,
                    font_type: self.theme.plain_text.font_type,
                    font_style: self.theme.plain_text.font_style,
                    font_size: self.theme.plain_text.font_size,
                    font_color: self.theme.plain_text.font_color,
                    background_color: self.theme.plain_text.background_color,
                    underline: false,
                    strikethrough: false,
                    highlight: false,
                    block_index: block_idx,
                    char_range: (0, 0),
                    inline_index: None,
                    checklist: checklist_visual,
                }];

                // Layout the content with proper text indentation
                let (mut content_runs, content_ranges, content_wraps, _y_after) = self
                    .layout_inline_content(
                        &block.content,
                        &block.block_type,
                        block_idx,
                        y,
                        content_start_x,
                        width - (content_start_x - start_x),
                        default_line_height,
                        ctx,
                    );

                let mut current_y = y;

                // Merge bullet with first line
                if !content_runs.is_empty() && !content_runs[0].is_empty() {
                    let first_wrap = content_wraps.first().copied().unwrap_or(false);
                    let first_range = content_ranges.first().copied().unwrap_or((0, 0));
                    runs.append(&mut content_runs[0]);

                    // Calculate char range from content runs (skip bullet)
                    let char_start = runs
                        .iter()
                        .skip(1)
                        .map(|r| r.char_range.0)
                        .min()
                        .unwrap_or(first_range.0);
                    let char_end = runs
                        .iter()
                        .skip(1)
                        .map(|r| r.char_range.1)
                        .max()
                        .unwrap_or(first_range.1);

                    let visual_char_end = self.compute_visual_char_end(&runs, char_end, first_wrap);
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: default_line_height,
                        base_x: content_start_x,
                        block_index: block_idx,
                        char_start,
                        char_end,
                        visual_char_end,
                        runs,
                    });
                    current_y += default_line_height;

                    // Add remaining lines
                    for (idx, line_runs) in content_runs.iter().enumerate().skip(1) {
                        let (char_start, char_end) =
                            content_ranges.get(idx).copied().unwrap_or((0, 0));
                        let wrapped = content_wraps.get(idx).copied().unwrap_or(false);
                        let visual_char_end =
                            self.compute_visual_char_end(line_runs, char_end, wrapped);

                        self.layout_lines.push(LayoutLine {
                            y: current_y,
                            height: default_line_height,
                            base_x: content_start_x,
                            block_index: block_idx,
                            char_start,
                            char_end,
                            visual_char_end,
                            runs: line_runs.clone(),
                        });
                        current_y += default_line_height;
                    }
                } else {
                    // Just bullet
                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: default_line_height,
                        base_x: content_start_x,
                        block_index: block_idx,
                        char_start: 0,
                        char_end: 0,
                        visual_char_end: 0,
                        runs,
                    });
                    current_y += default_line_height;
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
        let (lines, line_ranges, line_wraps, _y_after) = self.layout_inline_content(
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
                base_x: start_x,
                block_index: block_idx,
                char_start: 0,
                char_end: 0,
                visual_char_end: 0,
                runs: Vec::new(),
            });
            current_y += line_height;
        } else {
            for (line_runs, ((char_start, char_end), wrapped)) in lines
                .into_iter()
                .zip(line_ranges.into_iter().zip(line_wraps.into_iter()))
            {
                let base_x = line_runs
                    .iter()
                    .filter(|r| !(r.char_range.0 == r.char_range.1 && r.inline_index.is_none()))
                    .map(|r| r.x)
                    .min()
                    .unwrap_or(start_x);

                let visual_char_end = self.compute_visual_char_end(&line_runs, char_end, wrapped);

                self.layout_lines.push(LayoutLine {
                    y: current_y,
                    height: line_height,
                    base_x,
                    block_index: block_idx,
                    char_start,
                    char_end,
                    visual_char_end,
                    runs: line_runs,
                });
                current_y += line_height;
            }
        }

        current_y + 5
    }

    fn resolve_text_run_style(
        &self,
        base_font: FontSettings,
        text_style: &TextStyle,
    ) -> ResolvedRunStyle {
        let settings = if text_style.code {
            self.theme.code_text
        } else {
            base_font
        };

        let base_is_bold = matches!(settings.font_style, FontStyle::Bold | FontStyle::BoldItalic);
        let base_is_italic = matches!(
            settings.font_style,
            FontStyle::Italic | FontStyle::BoldItalic
        );

        let final_bold = text_style.bold || base_is_bold;
        let final_italic = text_style.italic || base_is_italic;

        let font_style = match (final_bold, final_italic) {
            (true, true) => FontStyle::BoldItalic,
            (true, false) => FontStyle::Bold,
            (false, true) => FontStyle::Italic,
            (false, false) => FontStyle::Regular,
        };

        let highlight = text_style.highlight;
        let mut background_color = settings.background_color;
        if highlight {
            background_color = Some(self.theme.highlight_color);
        }

        ResolvedRunStyle {
            font_type: settings.font_type,
            font_style,
            font_size: settings.font_size,
            font_color: settings.font_color,
            background_color,
            underline: text_style.underline,
            strikethrough: text_style.strikethrough,
            highlight,
        }
    }

    fn resolve_link_run_style(&self, base_font: FontSettings) -> ResolvedRunStyle {
        let mut style = self.resolve_text_run_style(base_font, &TextStyle::plain());
        style.font_color = self.theme.link_color;
        style.underline = true;
        style
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
    ) -> (Vec<Vec<VisualRun>>, Vec<(usize, usize)>, Vec<bool>, i32) {
        let mut lines: Vec<Vec<VisualRun>> = Vec::new();
        let mut line_wraps: Vec<bool> = Vec::new();
        let mut line_ranges: Vec<(usize, usize)> = Vec::new();
        let mut current_line: Vec<VisualRun> = Vec::new();
        let mut current_x = start_x;
        let mut current_y = y;
        let mut char_offset = 0;

        let base_font = match block_type {
            BlockType::Heading { level } => match level {
                1 => self.theme.header_level_1,
                2 => self.theme.header_level_2,
                _ => self.theme.header_level_3,
            },
            BlockType::BlockQuote => self.theme.quote_text,
            BlockType::CodeBlock { .. } => self.theme.code_text,
            _ => self.theme.plain_text,
        };

        let push_line = |lines: &mut Vec<Vec<VisualRun>>,
                         ranges: &mut Vec<(usize, usize)>,
                         wraps: &mut Vec<bool>,
                         current_line: &mut Vec<VisualRun>,
                         default_offset: usize,
                         wrapped: bool| {
            let (start, end) = if let Some(first) = current_line.first() {
                let last_end = current_line
                    .last()
                    .map(|r| r.char_range.1)
                    .unwrap_or(first.char_range.1);
                (first.char_range.0, last_end)
            } else {
                (default_offset, default_offset)
            };
            ranges.push((start, end));
            wraps.push(wrapped);
            lines.push(std::mem::take(current_line));
        };

        let mut pending_empty_line = false;

        for (inline_idx, item) in content.iter().enumerate() {
            match item {
                InlineContent::Text(run) => {
                    pending_empty_line = false;
                    let style = self.resolve_text_run_style(base_font, &run.style);
                    let font = style.font_type;
                    let fstyle = style.font_style;
                    let size = style.font_size;

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
                                let space_width =
                                    ctx.text_width(space_text, font, fstyle, size) as i32;

                                current_line.push(VisualRun {
                                    text: space_text.to_string(),
                                    x: current_x,
                                    width: space_width,
                                    font_type: style.font_type,
                                    font_style: style.font_style,
                                    font_size: style.font_size,
                                    font_color: style.font_color,
                                    background_color: style.background_color,
                                    underline: style.underline,
                                    strikethrough: style.strikethrough,
                                    highlight: style.highlight,
                                    block_index: block_idx,
                                    char_range: (char_offset, char_offset + space_end),
                                    inline_index: Some(inline_idx),
                                    checklist: None,
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
                                    .is_some_and(|c| c.is_whitespace() && c != '\n')
                            {
                                word_end += text[word_end..].chars().next().unwrap().len_utf8();
                            }

                            let word_text = &text[word_start..word_end];
                            let word_width = ctx.text_width(word_text, font, fstyle, size) as i32;

                            if current_x + word_width > start_x + width && current_x > start_x {
                                // Wrap to next line
                                push_line(
                                    &mut lines,
                                    &mut line_ranges,
                                    &mut line_wraps,
                                    &mut current_line,
                                    char_offset,
                                    true,
                                );
                                current_x = start_x;
                                current_y += line_height;
                            }

                            current_line.push(VisualRun {
                                text: word_text.to_string(),
                                x: current_x,
                                width: word_width,
                                font_type: style.font_type,
                                font_style: style.font_style,
                                font_size: style.font_size,
                                font_color: style.font_color,
                                background_color: style.background_color,
                                underline: style.underline,
                                strikethrough: style.strikethrough,
                                highlight: style.highlight,
                                block_index: block_idx,
                                char_range: (char_offset + word_start, char_offset + word_end),
                                inline_index: Some(inline_idx),
                                checklist: None,
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
                    pending_empty_line = false;
                    // Render link content using link styling
                    let style = self.resolve_link_run_style(base_font);
                    let text = link_content
                        .iter()
                        .map(|c| c.to_plain_text())
                        .collect::<String>();

                    let text_width =
                        ctx.text_width(&text, style.font_type, style.font_style, style.font_size)
                            as i32;

                    if current_x + text_width > start_x + width && current_x > start_x {
                        push_line(
                            &mut lines,
                            &mut line_ranges,
                            &mut line_wraps,
                            &mut current_line,
                            char_offset,
                            true,
                        );
                        current_x = start_x;
                        current_y += line_height;
                    }

                    current_line.push(VisualRun {
                        text: text.clone(),
                        x: current_x,
                        width: text_width,
                        font_type: style.font_type,
                        font_style: style.font_style,
                        font_size: style.font_size,
                        font_color: style.font_color,
                        background_color: style.background_color,
                        underline: style.underline,
                        strikethrough: style.strikethrough,
                        highlight: style.highlight,
                        block_index: block_idx,
                        char_range: (char_offset, char_offset + text.len()),
                        inline_index: Some(inline_idx),
                        checklist: None,
                    });

                    current_x += text_width;
                    char_offset += text.len();
                }
                InlineContent::HardBreak => {
                    push_line(
                        &mut lines,
                        &mut line_ranges,
                        &mut line_wraps,
                        &mut current_line,
                        char_offset,
                        false,
                    );
                    current_x = start_x;
                    current_y += line_height;
                    char_offset += 1;
                    pending_empty_line = true;
                }
            }
        }

        let is_empty = lines.is_empty();

        if !current_line.is_empty() {
            push_line(
                &mut lines,
                &mut line_ranges,
                &mut line_wraps,
                &mut current_line,
                char_offset,
                false,
            );
        } else if pending_empty_line {
            // Preserve the trailing hard break by materializing an empty visual line.
            push_line(
                &mut lines,
                &mut line_ranges,
                &mut line_wraps,
                &mut current_line,
                char_offset,
                false,
            );
        }

        (
            lines,
            line_ranges,
            line_wraps,
            if is_empty { y } else { current_y },
        )
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
        ctx.set_color(self.theme.background_color);
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

            // If this line belongs to a BlockQuote, draw a vertical bar to the left.
            // We draw a short segment per line to keep implementation simple.
            if let Some(block) = self.editor.document().blocks().get(line.block_index)
                && let BlockType::BlockQuote = block.block_type
            {
                // Position the bar slightly left of the quote text indent (start_x + 20)
                let bar_x = self.x + self.theme.padding_horizontal + 12;
                let bar_y1 = self.y + line.y - self.scroll_offset;
                let bar_y2 = bar_y1 + line.height;
                ctx.set_color(self.theme.quote_bar_color);
                ctx.draw_line(bar_x, bar_y1, bar_x, bar_y2);
            }

            for run in &line.runs {
                // Check if this run is part of a hovered link
                let is_hovered = self.hovered_link.is_some_and(|(block_idx, inline_idx)| {
                    run.block_index == block_idx && run.inline_index == Some(inline_idx)
                });

                let descent = ctx.text_descent(run.font_type, run.font_style, run.font_size);
                ctx.set_font(run.font_type, run.font_style, run.font_size);
                ctx.set_color(run.font_color);

                let line_top = self.y + line.y - self.scroll_offset;
                let draw_x = self.x + run.x;

                if let Some(checklist) = &run.checklist {
                    let mut box_size = checklist.box_size;
                    if box_size > line.height {
                        box_size = line.height;
                    }
                    if box_size <= 0 {
                        continue;
                    }

                    let box_y = line_top + (line.height - box_size) / 2 + descent / 2;
                    let box_right = draw_x + box_size;
                    let box_bottom = box_y + box_size;

                    ctx.draw_line(draw_x, box_y, box_right, box_y);
                    ctx.draw_line(draw_x, box_y, draw_x, box_bottom);
                    ctx.draw_line(draw_x, box_bottom, box_right, box_bottom);
                    ctx.draw_line(box_right, box_y, box_right, box_bottom);

                    if checklist.checked {
                        let mut inset = ((box_size as f32) * 0.2).round() as i32;
                        if inset < 2 {
                            inset = 2;
                        }
                        if inset * 2 >= box_size {
                            inset = box_size / 2;
                        }
                        let x1 = draw_x + inset;
                        let y1 = box_y + inset;
                        let x2 = box_right - inset;
                        let y2 = box_bottom - inset;
                        ctx.draw_line(x1, y1, x2, y2);
                        ctx.draw_line(x1, y2, x2, y1);
                    }

                    continue;
                }

                let draw_y = line_top + run.font_size as i32;

                // Draw inline highlight background for styles that specify a bgcolor
                // (e.g., text highlight). Draw this first so selection can paint over it.
                {
                    let text_width =
                        ctx.text_width(&run.text, run.font_type, run.font_style, run.font_size)
                            as i32;
                    if let Some(col) = run.background_color {
                        ctx.set_color(col);
                        ctx.draw_rect_filled(draw_x, line_top, text_width, line.height);
                        ctx.set_color(run.font_color); // Restore text color for text drawing
                    }
                }

                // Draw background for hover (if link is hovered)
                // Draw this BEFORE selection so selection remains visible on top
                if is_hovered {
                    let text_width =
                        ctx.text_width(&run.text, run.font_type, run.font_style, run.font_size)
                            as i32;
                    ctx.set_color(self.theme.link_hover_background);
                    ctx.draw_rect_filled(draw_x, line_top, text_width, line.height);
                    ctx.set_color(run.font_color); // Restore text color
                }

                // Draw selection highlight (if run is selected)
                // Draw AFTER hover so the selection rectangle is on top
                if let Some((sel_start, sel_end)) = self.get_run_selection_range(run)
                    && sel_end > sel_start
                {
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
                        ctx.text_width(text_before, run.font_type, run.font_style, run.font_size)
                            as i32;
                    let sel_width =
                        ctx.text_width(text_selected, run.font_type, run.font_style, run.font_size)
                            as i32;

                    ctx.set_color(self.theme.selection_color);
                    ctx.draw_rect_filled(draw_x + before_width, line_top, sel_width, line.height);
                    ctx.set_color(run.font_color); // Restore text color
                }

                ctx.draw_text(&run.text, draw_x, draw_y);

                let text_width =
                    ctx.text_width(&run.text, run.font_type, run.font_style, run.font_size) as i32;

                // Draw underline if needed
                if run.underline {
                    ctx.draw_line(draw_x, draw_y + 2, draw_x + text_width, draw_y + 2);
                }

                // Draw strikethrough if needed
                if run.strikethrough {
                    // Draw line through middle of text (roughly at half the font size)
                    let descent = ctx.text_descent(run.font_type, run.font_style, run.font_size);
                    let strike_y = draw_y - (run.font_size as i32) / 2 + descent / 2 + 1;
                    ctx.set_color(0xaaaaaaff);
                    ctx.draw_rect_filled(draw_x, strike_y, text_width, 1);
                }
            }
        }

        // Draw cursor (only when widget has keyboard focus)
        if self.cursor_visible
            && ctx.has_focus()
            && self.blink_on
            && let Some((cx, cy, ch)) = self.get_cursor_visual_position(ctx)
        {
            let screen_y = self.y + cy - self.scroll_offset;
            let screen_x = self.x + cx;

            if screen_y >= self.y && screen_y < self.y + self.h {
                ctx.set_color(self.theme.cursor_color);
                ctx.draw_line(screen_x, screen_y, screen_x, screen_y + ch);
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
        for (idx, line) in self.layout_lines.iter().enumerate() {
            if line.block_index != cursor.block_index {
                continue;
            }
            if !self.offset_belongs_to_line(idx, cursor.offset) {
                continue;
            }

            let mut x = line.base_x;

            for run in &line.runs {
                // Skip non-content runs (like list bullets with char_range (0,0))
                if run.char_range.0 == run.char_range.1 && run.inline_index.is_none() {
                    continue;
                }

                if cursor.offset >= run.char_range.0 && cursor.offset <= run.char_range.1 {
                    // Cursor is in this run - measure actual text width
                    let offset_in_run = cursor.offset - run.char_range.0;
                    let (font, fstyle, size) = (run.font_type, run.font_style, run.font_size);

                    // Measure the text up to the cursor position
                    let text_before_cursor = if offset_in_run < run.text.len() {
                        &run.text[..offset_in_run]
                    } else {
                        &run.text
                    };

                    let width_before =
                        ctx.text_width(text_before_cursor, font, fstyle, size) as i32;
                    x = run.x + width_before;
                    return Some((x, line.y, line.height));
                }

                if cursor.offset > run.char_range.1 {
                    // Cursor is after this run - measure full run width
                    let (font, fstyle, size) = (run.font_type, run.font_style, run.font_size);
                    x = run.x + ctx.text_width(&run.text, font, fstyle, size) as i32;
                }
            }

            return Some((x, line.y, line.height));
        }

        // Default: top-left
        if let Some(first_line) = self.layout_lines.first() {
            Some((first_line.base_x, first_line.y, first_line.height))
        } else {
            Some((
                self.theme.padding_horizontal,
                self.theme.padding_vertical,
                self.theme.line_height,
            ))
        }
    }

    /// Determine if a point (widget coordinates) hits a checklist marker
    pub(crate) fn checklist_marker_hit(&self, x: i32, y: i32) -> Option<usize> {
        let adjusted_y = y + self.scroll_offset;
        let doc = self.editor.document();
        let blocks = doc.blocks();

        for line in &self.layout_lines {
            if adjusted_y < line.y || adjusted_y > line.y + line.height {
                continue;
            }

            for run in &line.runs {
                if let Some(checklist) = &run.checklist {
                    if checklist.box_size <= 0 {
                        continue;
                    }

                    let marker_start_x = run.x;
                    let marker_end_x = run.x + checklist.box_size;
                    // Allow a small tolerance around the marker for easier clicking
                    let tolerance = 2;
                    if x >= marker_start_x - tolerance
                        && x <= marker_end_x + tolerance
                        && let Some(block) = blocks.get(line.block_index)
                        && matches!(
                            block.block_type,
                            BlockType::ListItem {
                                checkbox: Some(_),
                                ..
                            }
                        )
                    {
                        return Some(line.block_index);
                    }
                }
            }
        }

        None
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
                if prev_dist <= next_dist { p } else { n }
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
                                if inline_idx < block.content.len()
                                    && let InlineContent::Link { link, .. } =
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
    use crate::richtext::structured_document::{
        Block, BlockType, DocumentPosition, InlineContent, StructuredDocument, TextRun,
    };

    #[derive(Default)]
    struct TestDrawContext {
        focus: bool,
    }

    impl TestDrawContext {
        fn new_with_focus() -> Self {
            Self { focus: true }
        }
    }

    impl DrawContext for TestDrawContext {
        fn set_color(&mut self, _color: u32) {}

        fn set_font(&mut self, _font: FontType, _style: FontStyle, _size: u8) {}

        fn draw_text(&mut self, _text: &str, _x: i32, _y: i32) {}

        fn draw_rect_filled(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}

        fn draw_line(&mut self, _x1: i32, _y1: i32, _x2: i32, _y2: i32) {}

        fn text_width(&mut self, text: &str, _font: FontType, _style: FontStyle, size: u8) -> f64 {
            // Simplistic width model: proportional to character count and font size
            text.chars().count() as f64 * (size as f64) * 0.6
        }

        fn text_height(&self, _font: FontType, _style: FontStyle, size: u8) -> i32 {
            size as i32
        }

        fn text_descent(&self, _font: FontType, _style: FontStyle, size: u8) -> i32 {
            ((size as f32) * 0.2).round() as i32
        }

        fn push_clip(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}

        fn pop_clip(&mut self) {}

        fn color_average(&self, c1: u32, _c2: u32, _weight: f32) -> u32 {
            c1
        }

        fn color_contrast(&self, fg: u32, _bg: u32) -> u32 {
            fg
        }

        fn color_inactive(&self, c: u32) -> u32 {
            c
        }

        fn has_focus(&self) -> bool {
            self.focus
        }

        fn is_active(&self) -> bool {
            true
        }
    }

    fn make_display_with_block(block: Block) -> StructuredRichDisplay {
        let mut display = StructuredRichDisplay::new(0, 0, 400, 300);
        {
            let editor = display.editor_mut();
            let mut doc = StructuredDocument::new();
            doc.add_block(block);
            *editor.document_mut() = doc;
            editor.set_cursor(DocumentPosition::new(0, 0));
        }
        display
    }

    #[test]
    fn test_display_creation() {
        let display = StructuredRichDisplay::new(0, 0, 800, 600);
        assert_eq!(display.w(), 800);
        assert_eq!(display.h(), 600);
    }

    #[test]
    fn cursor_in_empty_blockquote_respects_indent() {
        let block = Block::new(0, BlockType::BlockQuote);
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, display.theme.padding_horizontal + 20);
    }

    #[test]
    fn cursor_in_empty_unordered_list_respects_content_indent() {
        let block = Block::new(
            0,
            BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: None,
            },
        );
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        let bullet_width = ctx.text_width(
            "• ",
            display.theme.plain_text.font_type,
            display.theme.plain_text.font_style,
            display.theme.plain_text.font_size,
        ) as i32;
        let expected_x = display.theme.padding_horizontal
            + display.theme.plain_text.font_size as i32
            + bullet_width;

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, expected_x);
    }

    #[test]
    fn cursor_in_empty_ordered_list_respects_content_indent() {
        let block = Block::new(
            0,
            BlockType::ListItem {
                ordered: true,
                number: Some(3),
                checkbox: None,
            },
        );
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        let label_width = ctx.text_width(
            "3. ",
            display.theme.plain_text.font_type,
            display.theme.plain_text.font_style,
            display.theme.plain_text.font_size,
        ) as i32;
        let expected_x = display.theme.padding_horizontal
            + display.theme.plain_text.font_size as i32
            + label_width;

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, expected_x);
    }

    #[test]
    fn cursor_in_empty_checklist_respects_content_indent() {
        let block = Block::new(
            0,
            BlockType::ListItem {
                ordered: false,
                number: None,
                checkbox: Some(false),
            },
        );
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        let mut checkbox_size = (display.theme.plain_text.font_size as i32).saturating_sub(4);
        if checkbox_size < 8 {
            checkbox_size = 8;
        }
        if checkbox_size > display.theme.line_height {
            checkbox_size = display.theme.line_height;
        }

        let mut space_width = ctx.text_width(
            " ",
            display.theme.plain_text.font_type,
            display.theme.plain_text.font_style,
            display.theme.plain_text.font_size,
        ) as i32;
        if space_width < 4 {
            space_width = 4;
        }

        let expected_x = display.theme.padding_horizontal
            + display.theme.plain_text.font_size as i32
            + checkbox_size
            + space_width;

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, expected_x);
    }

    #[test]
    fn cursor_in_empty_code_block_respects_indent() {
        let block = Block::new(0, BlockType::CodeBlock { language: None });
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, display.theme.padding_horizontal + 10);
    }

    #[test]
    fn trailing_hard_break_creates_empty_visual_line() {
        let mut block = Block::paragraph(0);
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Hello")));
        block.content.push(InlineContent::HardBreak);

        let mut display = make_display_with_block(block);

        let mut layout_ctx = TestDrawContext::new_with_focus();
        display.layout(&mut layout_ctx);

        assert_eq!(
            display.layout_lines.len(),
            2,
            "Trailing hard break should produce two visual lines"
        );

        let end_offset = display.editor().document().blocks()[0].text_len();
        {
            let editor = display.editor_mut();
            editor.set_cursor(DocumentPosition::new(0, end_offset));
        }

        let trailing_line = display
            .layout_lines
            .last()
            .expect("Expected trailing layout line for hard break");
        assert_eq!(trailing_line.char_start, end_offset);
        assert_eq!(trailing_line.char_end, end_offset);

        let mut ctx = TestDrawContext::new_with_focus();
        let (cursor_x, cursor_y, _) = display
            .get_cursor_visual_position(&mut ctx)
            .expect("Cursor position should resolve with trailing hard break");
        assert_eq!(cursor_x, trailing_line.base_x);
        assert_eq!(cursor_y, trailing_line.y);
    }

    #[test]
    fn cursor_home_end_respect_visual_lines() {
        let block = Block::paragraph(0).with_plain_text(
            "This visual line wrapping test ensures Home and End stay within wraps.",
        );
        let mut display = make_display_with_block(block);
        display.resize(0, 0, 160, 200);

        let mut layout_ctx = TestDrawContext::new_with_focus();
        display.layout(&mut layout_ctx);

        assert!(
            display.layout_lines.len() >= 2,
            "Expected wrapped layout to produce multiple visual lines"
        );

        let first_line_start = display.layout_lines[0].char_start;
        let first_line_end = display.layout_lines[0].char_end;
        let second_line_start = display.layout_lines[1].char_start;
        let first_line_base_x = display.layout_lines[0].base_x;
        let first_line_y = display.layout_lines[0].y;
        let second_line_y = display.layout_lines[1].y;
        let second_line_base_x = display.layout_lines[1].base_x;

        {
            let editor = display.editor_mut();
            editor.set_cursor(DocumentPosition::new(0, first_line_start));
        }

        let mut ctx = TestDrawContext::new_with_focus();
        let mut expected_end_offset = first_line_end;
        while expected_end_offset > first_line_start
            && !display.offset_belongs_to_line(0, expected_end_offset)
        {
            expected_end_offset -= 1;
        }

        display.move_cursor_visual_line_end_precise(false, &mut ctx);
        assert_eq!(display.editor().cursor().offset, expected_end_offset);
        let (end_x, end_y, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(end_y, first_line_y);
        assert!(end_x > first_line_base_x);

        {
            let editor = display.editor_mut();
            editor.move_cursor_right();
        }
        let (wrapped_x, wrapped_y, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(display.editor().cursor().offset, second_line_start);
        assert_eq!(wrapped_y, second_line_y);
        assert_eq!(wrapped_x, second_line_base_x);

        let second_line_end = display.layout_lines[1].char_end;
        let sample_offset = (second_line_start + 3).min(second_line_end);
        {
            let editor = display.editor_mut();
            editor.set_cursor(DocumentPosition::new(0, sample_offset));
        }

        display.move_cursor_visual_line_start(false, &mut ctx);
        assert_eq!(display.editor().cursor().offset, second_line_start);
        let (start_x, start_y, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(start_y, second_line_y);
        assert_eq!(start_x, second_line_base_x);
    }
}
