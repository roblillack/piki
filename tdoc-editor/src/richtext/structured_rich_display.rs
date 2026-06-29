// Structured Rich Text Display
// A rendering and interaction widget for StructuredDocument
// Completely decoupled from markdown syntax

use super::structured_document::*;
use super::structured_editor::*;
use super::tree_path::{DocumentPosition, TreePath};
use super::tree_walk::{self, LeafInfo};
use crate::draw_context::DrawContext;
use crate::draw_context::FontStyle;
use crate::draw_context::FontType;
use crate::theme::{FontSettings, Theme};


/// A search match in the document
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchMatch {
    /// Block index where the match starts
    pub block_index: usize,
    /// Character offset within the block where match starts
    pub start_offset: usize,
    /// Character offset within the block where match ends (exclusive)
    pub end_offset: usize,
}

/// Layout information for a rendered line
#[derive(Debug, Clone)]
pub struct LayoutLine {
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

impl LayoutLine {}

/// Geometry for drawing a table's grid lines and header fills. Computed during
/// layout and consumed by `draw`. Coordinates are in layout space (relative to
/// the widget's content origin, before scroll translation).
#[derive(Debug, Clone)]
struct TableLayout {
    /// Document block index of the table.
    block_index: usize,
    /// X of each vertical grid line, left to right (length = columns + 1).
    col_x: Vec<i32>,
    /// Y of each horizontal grid line, top to bottom (length = rows + 1).
    row_y: Vec<i32>,
}

/// Finalize the current visual line: record its char range and wrap flag, then
/// move it into `lines`. Shared by the wrapping engine and `push_token_wrapped`.
fn push_line(
    lines: &mut Vec<Vec<VisualRun>>,
    ranges: &mut Vec<(usize, usize)>,
    wraps: &mut Vec<bool>,
    current_line: &mut Vec<VisualRun>,
    default_offset: usize,
    wrapped: bool,
) {
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
}

/// Build a [`VisualRun`] for content text with the given resolved style.
fn run_from_style(
    text: String,
    x: i32,
    width: i32,
    char_range: (usize, usize),
    inline_index: usize,
    block_idx: usize,
    style: ResolvedRunStyle,
) -> VisualRun {
    VisualRun {
        text,
        x,
        width,
        font_type: style.font_type,
        font_style: style.font_style,
        font_size: style.font_size,
        font_color: style.font_color,
        background_color: style.background_color,
        underline: style.underline,
        strikethrough: style.strikethrough,
        block_index: block_idx,
        char_range,
        inline_index: Some(inline_index),
        checklist: None,
    }
}

/// Place a single token (a word or a whole link) onto the current line, wrapping
/// to the next line when it doesn't fit.
///
/// When `break_long_words` is set and the token is wider than a full line, it is
/// split greedily at character boundaries so it can never bleed past the
/// available width — needed for narrow table cells. With the flag off, behavior
/// is identical to the original word wrapper (wrap once, then place whole).
#[allow(clippy::too_many_arguments)]
fn push_token_wrapped(
    ctx: &mut dyn DrawContext,
    lines: &mut Vec<Vec<VisualRun>>,
    line_ranges: &mut Vec<(usize, usize)>,
    line_wraps: &mut Vec<bool>,
    current_line: &mut Vec<VisualRun>,
    current_x: &mut i32,
    current_y: &mut i32,
    start_x: i32,
    width: i32,
    line_height: i32,
    break_long_words: bool,
    text: &str,
    char_base: usize,
    inline_index: usize,
    block_idx: usize,
    style: ResolvedRunStyle,
) {
    let (font, fstyle, size) = (style.font_type, style.font_style, style.font_size);
    let token_width = ctx.text_width(text, font, fstyle, size) as i32;

    let must_break = break_long_words && width > 0 && token_width > width;
    if !must_break {
        // Wrap to the next line if it doesn't fit and we're not already at the
        // line start, then place the whole token (original behavior).
        if *current_x + token_width > start_x + width && *current_x > start_x {
            push_line(
                lines,
                line_ranges,
                line_wraps,
                current_line,
                char_base,
                true,
            );
            *current_x = start_x;
            *current_y += line_height;
        }
        current_line.push(run_from_style(
            text.to_string(),
            *current_x,
            token_width,
            (char_base, char_base + text.len()),
            inline_index,
            block_idx,
            style,
        ));
        *current_x += token_width;
        return;
    }

    // The token is wider than a full line: break it greedily, character by
    // character, so it can never overflow into the neighbouring column.
    let indices: Vec<usize> = text.char_indices().map(|(i, _)| i).collect();
    let char_count = indices.len();
    let byte_end_of = |k: usize| {
        if k + 1 < char_count {
            indices[k + 1]
        } else {
            text.len()
        }
    };

    let mut seg_start = 0usize;
    let mut k = 0usize;
    while k < char_count {
        // Grow the chunk while it still fits from the current x.
        let mut fit_end = seg_start;
        let mut fit_k = k;
        while fit_k < char_count {
            let cand_end = byte_end_of(fit_k);
            let w = ctx.text_width(&text[seg_start..cand_end], font, fstyle, size) as i32;
            if *current_x + w > start_x + width {
                break;
            }
            fit_end = cand_end;
            fit_k += 1;
        }

        if fit_end > seg_start {
            let chunk = &text[seg_start..fit_end];
            let w = ctx.text_width(chunk, font, fstyle, size) as i32;
            current_line.push(run_from_style(
                chunk.to_string(),
                *current_x,
                w,
                (char_base + seg_start, char_base + fit_end),
                inline_index,
                block_idx,
                style,
            ));
            *current_x += w;
            seg_start = fit_end;
            k = fit_k;
        } else if *current_x == start_x {
            // Not even one character fits on a fresh line (column narrower than a
            // glyph). Force one through to guarantee forward progress.
            let cand_end = byte_end_of(k);
            let chunk = &text[seg_start..cand_end];
            let w = ctx.text_width(chunk, font, fstyle, size) as i32;
            current_line.push(run_from_style(
                chunk.to_string(),
                *current_x,
                w,
                (char_base + seg_start, char_base + cand_end),
                inline_index,
                block_idx,
                style,
            ));
            *current_x += w;
            seg_start = cand_end;
            k += 1;
        }

        // More to place: wrap to the next line.
        if seg_start < text.len() {
            push_line(
                lines,
                line_ranges,
                line_wraps,
                current_line,
                char_base + seg_start,
                true,
            );
            *current_x = start_x;
            *current_y += line_height;
        }
    }
}

/// Return a copy of inline content with every text run forced to bold. Used for
/// table header cells, which the renderer draws in bold like the tdoc CLI.
fn bolden_inline(content: &[InlineContent]) -> Vec<InlineContent> {
    content
        .iter()
        .map(|item| match item {
            InlineContent::Text(run) => {
                let mut style = run.style;
                style.bold = true;
                InlineContent::Text(TextRun::new(run.text.clone(), style))
            }
            InlineContent::Link { link, content } => InlineContent::Link {
                link: link.clone(),
                content: bolden_inline(content),
            },
            InlineContent::HardBreak => InlineContent::HardBreak,
        })
        .collect()
}

struct InlineContentLayout {
    lines: Vec<Vec<VisualRun>>,
    line_ranges: Vec<(usize, usize)>,
    line_wraps: Vec<bool>,
    y_after: i32,
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
    // Highlight not necessary, as we'll have a non-None background_color for that
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
    // Highlight not necessary, as we'll have a non-None background_color for that
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
    // Grid geometry for table blocks, parallel to layout_lines (cell text lives
    // in layout_lines; borders/header fills are drawn from this).
    table_layouts: Vec<TableLayout>,
    // The leaves of the current layout frame, in document order. `block_index` on
    // layout lines/runs is an index into this (a transient projection of the
    // authoritative tdoc tree, rebuilt every layout — not a parallel document model).
    layout_leaves: Vec<LeafInfo>,
    // The renderable block for each leaf, parallel to `layout_leaves`.
    layout_blocks: Vec<Block>,
    layout_valid: bool,

    // Scrolling
    scroll_offset: i32,

    // Cursor display
    cursor_visible: bool,
    // Cursor blink state
    blink_on: bool,
    blink_period_ms: u64,
    // Most recent elapsed-ms value seen by `tick`; used as the time base when
    // re-anchoring the blink cycle on cursor movement.
    last_tick_ms: u64,
    // Elapsed-ms value at which the current blink cycle started (its "on" phase).
    blink_anchor_ms: u64,

    // Link hover state: the leaf path + inline index of the hovered link.
    hovered_link: Option<(TreePath, usize)>,

    // Sticky horizontal position for vertical navigation across proportional fonts
    cursor_preferred_line_offset: Option<usize>,

    // Search state
    search_term: String,
    search_matches: Vec<SearchMatch>,
    search_current_index: Option<usize>,

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
            table_layouts: Vec::new(),
            layout_leaves: Vec::new(),
            layout_blocks: Vec::new(),
            layout_valid: false,
            scroll_offset: 0,
            cursor_visible: true,
            blink_on: true,
            blink_period_ms: 1000, // 1s full period (500ms on/off)
            last_tick_ms: 0,
            blink_anchor_ms: 0,
            hovered_link: None,
            cursor_preferred_line_offset: None,
            search_term: String::new(),
            search_matches: Vec::new(),
            search_current_index: None,
            theme: Theme::default(),
        }
    }

    /// Get the editor
    pub fn editor(&self) -> &StructuredEditor {
        &self.editor
    }

    /// Read-only access to the active theme.
    pub fn theme(&self) -> &Theme {
        &self.theme
    }

    /// Replace the active theme (colors, font settings, and layout metrics such
    /// as `line_height` and padding). Frontends with a different coordinate
    /// system — e.g. a terminal that measures in character cells rather than
    /// pixels — install a theme with cell-appropriate metrics here. Invalidates
    /// the cached layout.
    pub fn set_theme(&mut self, theme: Theme) {
        self.theme = theme;
        self.layout_valid = false;
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

    /// Set horizontal padding (for write room mode)
    pub fn set_horizontal_padding(&mut self, padding: i32) {
        self.theme.padding_horizontal = padding;
        self.layout_valid = false;
    }

    /// Get current horizontal padding
    pub fn horizontal_padding(&self) -> i32 {
        self.theme.padding_horizontal
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
        self.table_layouts.clear();

        let content_width = self.w - 2 * self.theme.padding_horizontal;
        let mut current_y = self.theme.padding_vertical;

        // Project the authoritative tdoc tree into a flat list of leaves (in document
        // order) and a parallel list of renderable blocks. `block_index` on layout lines
        // is an index into these vecs.
        let (leaves, blocks) = {
            let tdoc = self.editor.tdoc();
            let leaves = tree_walk::enumerate_leaves(tdoc);
            let blocks: Vec<Block> = leaves
                .iter()
                .map(|info| Block {
                    block_type: tree_walk::leaf_block_type(tdoc, info),
                    content: tree_walk::leaf_inline(tdoc, &info.path),
                })
                .collect();
            (leaves, blocks)
        };

        for block_idx in 0..blocks.len() {
            current_y = self.layout_block(
                &blocks[block_idx],
                &blocks,
                block_idx,
                leaves[block_idx].quote_depth,
                leaves[block_idx].list_levels,
                current_y,
                content_width,
                ctx,
            );
        }

        self.layout_leaves = leaves;
        self.layout_blocks = blocks;
        self.layout_valid = true;
    }

    /// The tree path of the leaf at frame index `idx` (empty path if out of range).
    fn path_for_index(&self, idx: usize) -> TreePath {
        self.layout_leaves
            .get(idx)
            .map(|info| info.path.clone())
            .unwrap_or_default()
    }

    /// The frame index of the leaf at `path`, if present in the current layout frame.
    fn index_for_path(&self, path: &TreePath) -> Option<usize> {
        self.layout_leaves
            .iter()
            .position(|info| &info.path == path)
    }

    /// Build a position from a frame leaf index and a byte offset.
    fn pos_at_index(&self, idx: usize, offset: usize) -> DocumentPosition {
        DocumentPosition::at(self.path_for_index(idx), offset)
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

    /// Get the character offset within the visual line for a given position.
    fn visual_line_offset(&self, pos: DocumentPosition) -> Option<usize> {
        let target = self.index_for_path(&pos.path)?;
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.block_index == target && self.offset_belongs_to_line(i, pos.offset) {
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
        let cidx = self.index_for_path(&cursor.path)?;
        // First, look for a line in the same block whose char range contains the offset
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.block_index == cidx && self.offset_belongs_to_line(i, cursor.offset) {
                return Some(i);
            }
        }
        // Fallback: closest line in the same block by char range proximity
        let mut candidate: Option<(usize, usize)> = None; // (index, distance)
        for (i, line) in self.layout_lines.iter().enumerate() {
            if line.block_index == cidx {
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

        self.pos_at_index(line.block_index, line.char_start + effective_offset)
    }

    /// Whether the leaf at frame index `block_index` is a (read-only) table.
    fn block_is_table(&self, block_index: usize) -> bool {
        matches!(
            self.layout_blocks.get(block_index).map(|b| &b.block_type),
            Some(BlockType::Table { .. })
        )
    }

    /// Resolve the cursor position for a vertical move that landed on
    /// `target_idx`. A table is a single stop, so landing on any of its lines
    /// places the cursor at the table's start; other blocks honor the preferred
    /// (sticky) column.
    fn vertical_target_pos(&self, target_idx: usize) -> DocumentPosition {
        let line = &self.layout_lines[target_idx];
        if self.block_is_table(line.block_index) {
            self.pos_at_index(line.block_index, 0)
        } else {
            self.get_preferred_pos_for_line(line)
        }
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

        // Treat a table as a single stop: skip up over all of its lines to the
        // line directly above it.
        let cur_block = self.layout_lines[cur_idx].block_index;
        let target_idx = if self.block_is_table(cur_block) {
            let mut first = cur_idx;
            while first > 0 && self.layout_lines[first - 1].block_index == cur_block {
                first -= 1;
            }
            if first == 0 {
                return; // table is the first block; nothing above
            }
            first - 1
        } else {
            cur_idx - 1
        };

        let new_pos = self.vertical_target_pos(target_idx);
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

        let len = self.layout_lines.len();
        // Treat a table as a single stop: skip down over all of its lines to the
        // first line below it.
        let cur_block = self.layout_lines[cur_idx].block_index;
        let target_idx = if self.block_is_table(cur_block) {
            let mut t = cur_idx + 1;
            while t < len && self.layout_lines[t].block_index == cur_block {
                t += 1;
            }
            t
        } else {
            cur_idx + 1
        };

        if target_idx >= len {
            // Already at (or past) the last line
            return;
        }

        let new_pos = self.vertical_target_pos(target_idx);
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

        let cursor_block = self.index_for_path(&self.editor.cursor().path);
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
        if Some(line.block_index) != cursor_block {
            if extend {
                self.editor.move_cursor_to_line_start_extend();
            } else {
                self.editor.move_cursor_to_line_start();
            }
            return;
        }

        let new_pos = self.pos_at_index(line.block_index, line.char_start);
        if extend {
            self.editor.extend_selection_to(new_pos.clone());
        } else {
            self.editor.set_cursor(new_pos.clone());
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

        let cursor_block = self.index_for_path(&self.editor.cursor().path);
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
        if Some(line.block_index) != cursor_block {
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

        let new_pos = self.pos_at_index(line.block_index, target_offset);
        if extend {
            self.editor.extend_selection_to(new_pos.clone());
        } else {
            self.editor.set_cursor(new_pos.clone());
        }
        self.record_preferred_pos(new_pos);
    }

    /// Layout a single block. `blocks` is the full frame slice (for sibling scans such as
    /// ordered-list run detection); `block_idx` indexes it. `quote_depth`/`list_levels` come
    /// from the leaf and drive indentation independently of the (flat) block type.
    #[allow(clippy::too_many_arguments)]
    fn layout_block(
        &mut self,
        block: &Block,
        blocks: &[Block],
        block_idx: usize,
        quote_depth: usize,
        list_levels: usize,
        y: i32,
        width: i32,
        ctx: &mut dyn DrawContext,
    ) -> i32 {
        // Indentation is driven by the leaf's tree depths, not its (flat) block type: a
        // continuation paragraph, code block, or list item nested inside a quote keeps both
        // its quote indent and its list indent even though its `BlockType` records only one.
        let quote_indent = quote_depth as i32 * self.theme.quote_indent;
        let start_x = self.theme.padding_horizontal + quote_indent;
        let width = width - quote_indent;
        let default_line_height = self.theme.line_height;

        // Content that lives inside a list but is not itself a marker line (continuation
        // paragraphs, code blocks, nested quote text) aligns with the list item's content.
        let interior_x = if list_levels > 0 {
            let pf = self.theme.plain_text;
            let bullet_w = ctx.text_width("• ", pf.font_type, pf.font_style, pf.font_size) as i32;
            start_x + pf.font_size as i32 * list_levels as i32 + bullet_w
        } else {
            start_x
        };
        let interior_width = width - (interior_x - start_x);

        match &block.block_type {
            BlockType::Paragraph => self.layout_inline_block(
                block,
                block_idx,
                y,
                interior_x,
                interior_width,
                default_line_height,
                ctx,
            ),
            BlockType::Heading { level } => {
                let header_font = match level {
                    1 => self.theme.header_level_1,
                    2 => self.theme.header_level_2,
                    _ => self.theme.header_level_3,
                };
                let height =
                    (((header_font.font_size as f32) * 1.3) as i32).max(default_line_height);
                // Add top margin for headings (unless it's the first block)
                let top_margin = if block_idx > 0 {
                    self.theme.heading_top_margin
                } else {
                    0
                };
                let y_after = self.layout_inline_block(
                    block,
                    block_idx,
                    y + top_margin,
                    interior_x,
                    interior_width,
                    height,
                    ctx,
                );
                y_after + self.theme.heading_bottom_margin // Extra spacing after headings
            }
            BlockType::CodeBlock { .. } => {
                let text = block.to_plain_text();
                let lines: Vec<&str> = text.lines().collect();
                let f = self.theme.code_text;
                let mut current_y = y + self.theme.code_block_padding;
                let code_start_x = interior_x + 10;
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

                current_y + self.theme.code_block_padding * 2
            }
            BlockType::BlockQuote => {
                // The quote indent is already folded into `start_x`/`interior_x`; the
                // vertical bar(s) are drawn per line in draw() from the leaf's quote depth.
                self.layout_inline_block(
                    block,
                    block_idx,
                    y + self.theme.quote_spacing,
                    interior_x,
                    interior_width,
                    default_line_height,
                    ctx,
                ) + self.theme.quote_spacing
            }
            BlockType::ListItem {
                ordered,
                number,
                checkbox,
                depth,
            } => {
                let plain_font = self.theme.plain_text;

                // Base indent before the label, plus one extra step per nesting level.
                // (depth 0 keeps the original flat-list metrics.)
                let label_left_pad = plain_font.font_size as i32 * (*depth as i32 + 1);

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
                    block_index: block_idx,
                    char_range: (0, 0),
                    inline_index: None,
                    checklist: checklist_visual,
                }];

                // Layout the content with proper text indentation
                let layout = self.layout_inline_content(
                    &block.content,
                    &block.block_type,
                    block_idx,
                    y,
                    content_start_x,
                    width - (content_start_x - start_x),
                    default_line_height,
                    false,
                    ctx,
                );
                let (mut content_runs, content_ranges, content_wraps, _y_after) = (
                    layout.lines,
                    layout.line_ranges,
                    layout.line_wraps,
                    layout.y_after,
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

                current_y + self.theme.list_item_spacing
            }
            BlockType::Table { rows } => self.layout_table(block_idx, rows, y, start_x, width, ctx),
        }
    }

    /// Lay out a read-only table: compute column widths, wrap each cell, and
    /// emit one `LayoutLine` per physical text line plus a `TableLayout` holding
    /// the grid geometry for `draw`. Modeled on the tdoc CLI's bordered grid.
    fn layout_table(
        &mut self,
        block_idx: usize,
        rows: &[TableRow],
        y: i32,
        start_x: i32,
        width: i32,
        ctx: &mut dyn DrawContext,
    ) -> i32 {
        const BORDER: i32 = 1;
        let pad_h = self.theme.table_cell_padding_h; // horizontal padding inside each cell
        let pad_v = self.theme.table_cell_padding_v; // vertical padding inside each row
        let line_height = self.theme.line_height;

        let column_count = rows.iter().map(|r| r.cells.len()).max().unwrap_or(0);
        if rows.is_empty() || column_count == 0 {
            return y;
        }

        // Natural (single-line) content width per column, measured with the
        // cell's font (header cells are bold and thus a touch wider).
        let plain = self.theme.plain_text;
        let mut natural = vec![0i32; column_count];
        for row in rows {
            for (col, cell) in row.cells.iter().enumerate() {
                let style = if cell.is_header {
                    FontStyle::Bold
                } else {
                    plain.font_style
                };
                let text = cell.to_plain_text().replace('\n', " ");
                let w = ctx.text_width(&text, plain.font_type, style, plain.font_size) as i32;
                natural[col] = natural[col].max(w);
            }
        }

        // Fit the columns into the available width. Each column needs PAD_H on
        // both sides; vertical borders add BORDER around/between columns.
        let cols = column_count as i32;
        let structural = (cols + 1) * BORDER + cols * 2 * pad_h;
        let min_col = (plain.font_size as i32).max(8); // keep ~1 char visible
        let budget = (width - structural).max(cols * min_col);
        let total_natural: i32 = natural.iter().sum();

        let mut col_width = vec![0i32; column_count];
        if total_natural <= budget || total_natural == 0 {
            col_width.copy_from_slice(&natural);
        } else {
            // Shrink proportionally to fit, never below min_col.
            for (c, &nat) in natural.iter().enumerate() {
                let scaled = (nat as i64 * budget as i64 / total_natural as i64) as i32;
                col_width[c] = scaled.max(min_col);
            }
        }

        // Vertical grid-line positions: a border sits at each col_x; the cell
        // interior between consecutive lines is PAD_H + content + PAD_H.
        let mut col_x = vec![0i32; column_count + 1];
        col_x[0] = start_x;
        for c in 0..column_count {
            col_x[c + 1] = col_x[c] + BORDER + pad_h + col_width[c] + pad_h;
        }

        let mut row_y = Vec::with_capacity(rows.len() + 1);
        let mut current_y = y;

        for row in rows {
            row_y.push(current_y);

            // Wrap each cell to its column width, capturing its visual lines.
            let mut cell_lines: Vec<Vec<Vec<VisualRun>>> = Vec::with_capacity(column_count);
            for col in 0..column_count {
                let lines = match row.cells.get(col) {
                    Some(cell) if !cell.content.is_empty() => {
                        let content = if cell.is_header {
                            bolden_inline(&cell.content)
                        } else {
                            cell.content.clone()
                        };
                        let cell_x = col_x[col] + BORDER + pad_h;
                        self.layout_inline_content(
                            &content,
                            &BlockType::Paragraph,
                            block_idx,
                            0,
                            cell_x,
                            col_width[col],
                            line_height,
                            // Force-break tokens too wide for the column so cell
                            // content can't bleed into the next column.
                            true,
                            ctx,
                        )
                        .lines
                    }
                    _ => Vec::new(),
                };
                cell_lines.push(lines);
            }

            let row_line_count = cell_lines.iter().map(Vec::len).max().unwrap_or(0).max(1);

            // One LayoutLine per physical text line, gathering the matching
            // wrapped line from every cell so the shared draw loop renders them.
            let content_top = current_y + BORDER + pad_v;
            for li in 0..row_line_count {
                let mut runs: Vec<VisualRun> = Vec::new();
                for cell in &cell_lines {
                    if let Some(line) = cell.get(li) {
                        runs.extend(line.iter().cloned());
                    }
                }
                self.layout_lines.push(LayoutLine {
                    y: content_top + li as i32 * line_height,
                    height: line_height,
                    base_x: start_x,
                    block_index: block_idx,
                    char_start: 0,
                    char_end: 0,
                    visual_char_end: 0,
                    runs,
                });
            }

            current_y = content_top + row_line_count as i32 * line_height + pad_v;
        }

        row_y.push(current_y); // bottom border

        self.table_layouts.push(TableLayout {
            block_index: block_idx,
            col_x,
            row_y,
        });

        current_y + BORDER + 10
    }

    /// Layout an inline block (paragraph, heading, etc.)
    #[allow(clippy::too_many_arguments)]
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
        let layout = self.layout_inline_content(
            &block.content,
            &block.block_type,
            block_idx,
            y,
            start_x,
            width,
            line_height,
            false,
            ctx,
        );
        let (lines, line_ranges, line_wraps, _y_after) = (
            layout.lines,
            layout.line_ranges,
            layout.line_wraps,
            layout.y_after,
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
                .zip(line_ranges.into_iter().zip(line_wraps))
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

        current_y + self.theme.paragraph_spacing
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
    #[allow(clippy::too_many_arguments)]
    fn layout_inline_content(
        &mut self,
        content: &[InlineContent],
        block_type: &BlockType,
        block_idx: usize,
        y: i32,
        start_x: i32,
        width: i32,
        line_height: i32,
        break_long_words: bool,
        ctx: &mut dyn DrawContext,
    ) -> InlineContentLayout {
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
                            push_token_wrapped(
                                ctx,
                                &mut lines,
                                &mut line_ranges,
                                &mut line_wraps,
                                &mut current_line,
                                &mut current_x,
                                &mut current_y,
                                start_x,
                                width,
                                line_height,
                                break_long_words,
                                word_text,
                                char_offset + word_start,
                                inline_idx,
                                block_idx,
                                style,
                            );
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

                    push_token_wrapped(
                        ctx,
                        &mut lines,
                        &mut line_ranges,
                        &mut line_wraps,
                        &mut current_line,
                        &mut current_x,
                        &mut current_y,
                        start_x,
                        width,
                        line_height,
                        break_long_words,
                        &text,
                        char_offset,
                        inline_idx,
                        block_idx,
                        style,
                    );
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

        InlineContentLayout {
            lines,
            line_ranges,
            line_wraps,
            y_after: if is_empty { y } else { current_y },
        }
    }

    /// Check if a visual run intersects with the current selection
    /// Returns None if no selection, or Some((start_offset, end_offset)) relative to the run
    fn get_run_selection_range(&self, run: &VisualRun) -> Option<(usize, usize)> {
        let (a, b) = self.editor.selection()?;
        // Normalize selection so start <= end (document order).
        let (sel_start, sel_end) = if a <= b { (a, b) } else { (b, a) };
        let sel_start_idx = self.index_for_path(&sel_start.path)?;
        let sel_end_idx = self.index_for_path(&sel_end.path)?;

        // Check if this run's block is within the selection
        if run.block_index < sel_start_idx || run.block_index > sel_end_idx {
            return None;
        }

        // Determine the selection range within this run
        let run_start = run.char_range.0;
        let run_end = run.char_range.1;

        let sel_start_offset = if run.block_index == sel_start_idx {
            sel_start.offset
        } else {
            0
        };

        let sel_end_offset = if run.block_index == sel_end_idx {
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

    /// Draw the grid lines and header-cell backgrounds for every table block.
    /// Cell text itself is drawn by the normal `LayoutLine` run loop; this only
    /// paints the structural decoration around it.
    fn draw_tables(&self, ctx: &mut dyn DrawContext) {
        let viewport_top = self.scroll_offset;
        let viewport_bottom = self.scroll_offset + self.h;
        let blocks = &self.layout_blocks;

        for table in &self.table_layouts {
            let (top, bottom) = match (table.row_y.first(), table.row_y.last()) {
                (Some(&t), Some(&b)) => (t, b),
                _ => continue,
            };
            // Skip tables entirely outside the viewport.
            if bottom < viewport_top || top > viewport_bottom {
                continue;
            }
            let left = *table.col_x.first().unwrap();
            let right = *table.col_x.last().unwrap();

            let sx = self.x;
            let sy = self.y - self.scroll_offset;

            // Header-cell backgrounds, drawn first so text and grid sit on top.
            if let Some(BlockType::Table { rows }) =
                blocks.get(table.block_index).map(|b| &b.block_type)
            {
                ctx.set_color(self.theme.table_header_background);
                for (r, row) in rows.iter().enumerate() {
                    if r + 1 >= table.row_y.len() {
                        break;
                    }
                    let cell_top = sy + table.row_y[r];
                    let cell_h = table.row_y[r + 1] - table.row_y[r];
                    for (c, cell) in row.cells.iter().enumerate() {
                        if cell.is_header && c + 1 < table.col_x.len() {
                            let cx = sx + table.col_x[c];
                            let cw = table.col_x[c + 1] - table.col_x[c];
                            ctx.draw_rect_filled(cx, cell_top, cw, cell_h);
                        }
                    }
                }
            }

            // Grid lines.
            ctx.set_color(self.theme.table_border_color);
            for &x in &table.col_x {
                ctx.draw_line(sx + x, sy + top, sx + x, sy + bottom);
            }
            for &yv in &table.row_y {
                ctx.draw_line(sx + left, sy + yv, sx + right, sy + yv);
            }
        }
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

        // Table grid lines and header fills sit behind the cell text.
        self.draw_tables(ctx);

        for line in &self.layout_lines {
            if line.y + line.height < viewport_top || line.y > viewport_bottom {
                continue;
            }

            // Draw a vertical bar per enclosing quote level for any line inside a quote —
            // including list items, code blocks, and continuation paragraphs, whose flat
            // block type no longer records the quote. A short segment is drawn per line.
            if let Some(info) = self.layout_leaves.get(line.block_index) {
                ctx.set_color(self.theme.quote_bar_color);
                let bar_y1 = self.y + line.y - self.scroll_offset;
                let bar_y2 = bar_y1 + line.height;
                for level in 0..info.quote_depth as i32 {
                    let bar_x = self.x
                        + self.theme.padding_horizontal
                        + level * self.theme.quote_indent
                        + self.theme.quote_bar_offset;
                    ctx.draw_line(bar_x, bar_y1, bar_x, bar_y2);
                }
            }

            // Resolve the hovered link (if any) to this frame's leaf index for comparison.
            let hovered_idx = self
                .hovered_link
                .as_ref()
                .and_then(|(path, inline)| self.index_for_path(path).map(|idx| (idx, *inline)));

            for run in &line.runs {
                // Check if this run is part of a hovered link
                let is_hovered = hovered_idx.is_some_and(|(idx, inline)| {
                    run.block_index == idx && run.inline_index == Some(inline)
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
                    ctx.draw_checkbox(draw_x, box_y, box_size, checklist.checked);

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

                // Draw search highlight (if run contains a search match)
                // Draw BEFORE selection so selection takes priority
                if let Some((search_start, search_end, is_current)) =
                    self.get_run_search_highlight(run)
                    && search_end > search_start
                {
                    let text_before = if search_start < run.text.len() {
                        &run.text[..search_start]
                    } else {
                        &run.text
                    };
                    let text_match = if search_end <= run.text.len() {
                        &run.text[search_start..search_end]
                    } else if search_start < run.text.len() {
                        &run.text[search_start..]
                    } else {
                        ""
                    };

                    let before_width =
                        ctx.text_width(text_before, run.font_type, run.font_style, run.font_size)
                            as i32;
                    let match_width =
                        ctx.text_width(text_match, run.font_type, run.font_style, run.font_size)
                            as i32;

                    // Use different color for current match vs other matches
                    let highlight_color = if is_current {
                        self.theme.search_current_highlight_color
                    } else {
                        self.theme.search_highlight_color
                    };

                    ctx.set_color(highlight_color);
                    ctx.draw_rect_filled(draw_x + before_width, line_top, match_width, line.height);
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

                ctx.set_underline(run.underline);
                ctx.set_strikethrough(run.strikethrough);
                ctx.draw_text(&run.text, draw_x, draw_y);
                ctx.set_underline(false);
                ctx.set_strikethrough(false);

                let text_width =
                    ctx.text_width(&run.text, run.font_type, run.font_style, run.font_size) as i32;

                // Pixel backends draw decorations as separate lines; cell backends
                // fold them into the glyph attributes above instead.
                if self.theme.text_decoration_lines {
                    if run.underline {
                        ctx.draw_line(draw_x, draw_y + 2, draw_x + text_width, draw_y + 2);
                    }
                    if run.strikethrough {
                        // Line through the middle of the text (~half the font size).
                        let descent =
                            ctx.text_descent(run.font_type, run.font_style, run.font_size);
                        let strike_y = draw_y - (run.font_size as i32) / 2 + descent / 2 + 1;
                        ctx.set_color(0xaaaaaaff);
                        ctx.draw_rect_filled(draw_x, strike_y, text_width, 1);
                    }
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
                // Draw the caret as a 2px-wide bar rather than a 1px line so
                // it's easier to spot (a hairline caret is easy to lose,
                // especially on high-DPI displays).
                ctx.draw_rect_filled(screen_x, screen_y, 2, ch);
            }
        }

        ctx.pop_clip();
    }

    /// The cursor's on-screen position in widget coordinates, scroll-adjusted:
    /// `(x, y)` where `y` is already offset by the scroll position. Returns
    /// `None` if the cursor isn't on a laid-out line or is scrolled out of view.
    ///
    /// Intended for frontends that drive a hardware caret (e.g. a terminal's
    /// real cursor) instead of the engine-drawn one — pair with
    /// [`Self::set_cursor_visible(false)`](Self::set_cursor_visible) so the two
    /// don't both render. Call after [`draw`](Self::draw) (or
    /// [`ensure_cursor_visible`](Self::ensure_cursor_visible)) so the layout is
    /// current.
    pub fn cursor_screen_position(&self, ctx: &mut dyn DrawContext) -> Option<(i32, i32)> {
        let (cx, cy, _h) = self.get_cursor_visual_position(ctx)?;
        let screen_x = self.x + cx;
        let screen_y = self.y + cy - self.scroll_offset;
        if screen_y < self.y || screen_y >= self.y + self.h {
            return None;
        }
        Some((screen_x, screen_y))
    }

    /// Get visual position of cursor (x, y, height) relative to widget
    fn get_cursor_visual_position(&self, ctx: &mut dyn DrawContext) -> Option<(i32, i32, i32)> {
        let cursor = self.editor.cursor();
        let cur_idx = self.index_for_path(&cursor.path);

        // Find the layout line containing the cursor
        for (idx, line) in self.layout_lines.iter().enumerate() {
            if Some(line.block_index) != cur_idx {
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
    pub fn checklist_marker_hit(&self, x: i32, y: i32) -> Option<TreePath> {
        let adjusted_y = y + self.scroll_offset;

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
                        && let Some(block) = self.layout_blocks.get(line.block_index)
                        && matches!(
                            block.block_type,
                            BlockType::ListItem {
                                checkbox: Some(_),
                                ..
                            }
                        )
                    {
                        return Some(self.path_for_index(line.block_index));
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
                let offset = offset_in_line(line, x);
                return self.pos_at_index(line.block_index, offset);
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
        let offset = offset_in_line(line, x);
        self.pos_at_index(line.block_index, offset)
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
        self.last_tick_ms = ms_since_start;
        // Compute a simple square-wave blink: 500ms on, 500ms off, measured
        // from the current blink anchor (reset whenever the cursor moves).
        let half_period = (self.blink_period_ms / 2).max(1);
        let elapsed = ms_since_start.saturating_sub(self.blink_anchor_ms);
        let new_on = (elapsed / half_period).is_multiple_of(2);
        if new_on != self.blink_on {
            self.blink_on = new_on;
            return true;
        }
        false
    }

    /// Restart the blink cycle so the caret is shown immediately and stays on
    /// for a full half-period. Call this whenever the cursor moves so it's
    /// always visible at its new position, regardless of the current blink
    /// phase (which might otherwise have the caret hidden right now).
    pub fn reset_blink(&mut self) {
        self.blink_anchor_ms = self.last_tick_ms;
        self.blink_on = true;
    }

    /// Find link at given widget coordinates (relative to widget, not screen)
    /// Returns ((block_index, inline_index), destination) if a link is found
    pub fn find_link_at(&self, x: i32, y: i32) -> Option<((TreePath, usize), String)> {
        let adjusted_y = y + self.scroll_offset;
        let adjusted_x = x;

        // Find the line at this y position
        for line in &self.layout_lines {
            if adjusted_y >= line.y && adjusted_y < line.y + line.height {
                // Find the run at this x position
                for run in &line.runs {
                    if adjusted_x >= run.x && adjusted_x < run.x + run.width {
                        // Check if this run has an inline_index pointing to a link
                        if let Some(inline_idx) = run.inline_index
                            && let Some(block) = self.layout_blocks.get(run.block_index)
                            && inline_idx < block.content.len()
                            && let InlineContent::Link { link, .. } = &block.content[inline_idx]
                        {
                            return Some((
                                (self.path_for_index(run.block_index), inline_idx),
                                link.destination.clone(),
                            ));
                        }
                    }
                }
            }
        }
        None
    }

    /// Set hovered link (for hover highlighting)
    pub fn set_hovered_link(&mut self, link: Option<(TreePath, usize)>) {
        if self.hovered_link != link {
            self.hovered_link = link;
            // Don't invalidate layout, just trigger redraw
        }
    }

    /// Get hovered link
    pub fn hovered_link(&self) -> Option<(TreePath, usize)> {
        self.hovered_link.clone()
    }

    /// Find a link at or adjacent to the current cursor position.
    ///
    /// Treats the cursor as "inside" the link when its offset lies within the
    /// link's text range, and also when it is exactly at the start (directly
    /// before) or exactly at the end (directly after) of the link.
    ///
    /// Returns ((block_index, inline_index), destination) if a link is found.
    pub fn find_link_near_cursor(&self) -> Option<((TreePath, usize), String)> {
        let cursor = self.editor.cursor();
        let cur_idx = self.index_for_path(&cursor.path)?;
        let block = self.layout_blocks.get(cur_idx)?;
        let mut pos = 0usize;

        for (inline_idx, item) in block.content.iter().enumerate() {
            let len = item.text_len();
            if let InlineContent::Link { link, .. } = item {
                let start = pos;
                let end = pos + len; // end is exclusive for text, but we allow equality for adjacency

                // Cursor is within, or exactly at start/end (adjacent)
                if cursor.offset >= start && cursor.offset <= end {
                    return Some(((cursor.path.clone(), inline_idx), link.destination.clone()));
                }
            }
            pos += len;
        }

        None
    }

    // ==================== Search Methods ====================

    /// Perform a case-insensitive search for the given term.
    /// Updates the internal search state with all matches found.
    /// Returns the number of matches found.
    pub fn search(&mut self, term: &str) -> usize {
        self.search_term = term.to_string();
        self.search_matches.clear();
        self.search_current_index = None;

        if term.is_empty() {
            return 0;
        }

        let term_lower = term.to_lowercase();
        // `block_index` is the leaf's index in document order, matching layout frames.
        let leaves = tree_walk::enumerate_leaves(self.editor.tdoc());
        let mut matches = Vec::new();
        for (block_idx, info) in leaves.iter().enumerate() {
            let text = tree_walk::leaf_plain_text(self.editor.tdoc(), &info.path);
            let text_lower = text.to_lowercase();

            // Find all occurrences in this block
            let mut search_start = 0;
            while let Some(pos) = text_lower[search_start..].find(&term_lower) {
                let start_offset = search_start + pos;
                let end_offset = start_offset + term.len();
                matches.push(SearchMatch {
                    block_index: block_idx,
                    start_offset,
                    end_offset,
                });
                search_start = start_offset + 1; // Move past this match to find overlapping matches
            }
        }
        self.search_matches = matches;

        // If we found matches, set current index to 0
        if !self.search_matches.is_empty() {
            self.search_current_index = Some(0);
        }

        self.search_matches.len()
    }

    /// Clear the search state
    pub fn clear_search(&mut self) {
        self.search_term.clear();
        self.search_matches.clear();
        self.search_current_index = None;
    }

    /// Get the current search term
    pub fn search_term(&self) -> &str {
        &self.search_term
    }

    /// Get all search matches
    pub fn search_matches(&self) -> &[SearchMatch] {
        &self.search_matches
    }

    /// Get the current match index (0-based)
    pub fn search_current_index(&self) -> Option<usize> {
        self.search_current_index
    }

    /// Get the current search match
    pub fn current_search_match(&self) -> Option<&SearchMatch> {
        self.search_current_index
            .and_then(|idx| self.search_matches.get(idx))
    }

    /// Move to the next search match. Returns true if moved.
    pub fn next_match(&mut self) -> bool {
        if self.search_matches.is_empty() {
            return false;
        }

        let next_idx = match self.search_current_index {
            Some(idx) => (idx + 1) % self.search_matches.len(),
            None => 0,
        };

        self.search_current_index = Some(next_idx);
        true
    }

    /// Move to the previous search match. Returns true if moved.
    pub fn prev_match(&mut self) -> bool {
        if self.search_matches.is_empty() {
            return false;
        }

        let prev_idx = match self.search_current_index {
            Some(idx) => {
                if idx == 0 {
                    self.search_matches.len() - 1
                } else {
                    idx - 1
                }
            }
            None => self.search_matches.len() - 1,
        };

        self.search_current_index = Some(prev_idx);
        true
    }

    /// Scroll to make the current search match visible.
    /// Should be called after search() or next_match()/prev_match().
    pub fn scroll_to_current_match(&mut self, ctx: &mut dyn DrawContext) {
        let Some(current_match) = self.current_search_match().cloned() else {
            return;
        };

        // Ensure layout is up to date
        self.layout(ctx);

        // Find the layout line containing this match
        for line in &self.layout_lines {
            if line.block_index == current_match.block_index
                && current_match.start_offset >= line.char_start
                && current_match.start_offset < line.char_end
            {
                // Calculate the target scroll position to center the match in the viewport
                let line_center_y = line.y + line.height / 2;
                let viewport_center = self.h / 2;
                let target_scroll = (line_center_y - viewport_center).max(0);

                // Clamp to valid scroll range
                let max_scroll = (self.content_height() - self.h).max(0);
                self.scroll_offset = target_scroll.min(max_scroll);
                return;
            }
        }
    }

    /// Check if a visual run intersects with any search match.
    /// Returns Some((start_offset_in_run, end_offset_in_run, is_current)) if there's an intersection.
    fn get_run_search_highlight(&self, run: &VisualRun) -> Option<(usize, usize, bool)> {
        if self.search_matches.is_empty() {
            return None;
        }

        let current_idx = self.search_current_index;

        for (idx, search_match) in self.search_matches.iter().enumerate() {
            // Check if this match is in the same block as the run
            if search_match.block_index != run.block_index {
                continue;
            }

            // Check if match intersects with run's char range
            let run_start = run.char_range.0;
            let run_end = run.char_range.1;
            let match_start = search_match.start_offset;
            let match_end = search_match.end_offset;

            // Check for intersection
            if match_end <= run_start || match_start >= run_end {
                continue;
            }

            // Calculate intersection within the run
            let start_in_run = match_start
                .saturating_sub(run_start)
                .min(run_end - run_start);
            let end_in_run = match_end.saturating_sub(run_start).min(run_end - run_start);

            if end_in_run > start_in_run {
                let is_current = current_idx == Some(idx);
                return Some((start_in_run, end_in_run, is_current));
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw_context::{FontStyle, FontType};
    use crate::richtext::inline_convert::inline_to_spans;
    use crate::richtext::structured_document::{
        Block, BlockType, InlineContent, TableCell, TableRow, TextRun,
    };
    use crate::richtext::tree_path::{DocumentPosition, TreePath};
    use crate::richtext::tree_walk;
    use tdoc::Document;
    use tdoc::paragraph::{
        ChecklistItem, Paragraph, TableCell as TdocTableCell, TableRow as TdocTableRow,
    };

    /// Build a single tdoc paragraph from a transient `Block` (test convenience).
    fn block_to_paragraph(block: &Block) -> Paragraph {
        let spans = inline_to_spans(&block.content);
        match &block.block_type {
            BlockType::Paragraph => Paragraph::new_text().with_content(spans),
            BlockType::Heading { level } => match level {
                1 => Paragraph::new_header1().with_content(spans),
                2 => Paragraph::new_header2().with_content(spans),
                _ => Paragraph::new_header3().with_content(spans),
            },
            BlockType::CodeBlock { .. } => Paragraph::new_code_block().with_content(spans),
            BlockType::BlockQuote => Paragraph::new_quote()
                .with_children(vec![Paragraph::new_text().with_content(spans)]),
            BlockType::ListItem {
                ordered, checkbox, ..
            } => {
                if let Some(checked) = checkbox {
                    let item = ChecklistItem::new(*checked).with_content(spans);
                    Paragraph::new_checklist().with_checklist_items(vec![item])
                } else if *ordered {
                    Paragraph::new_ordered_list()
                        .with_entries(vec![vec![Paragraph::new_text().with_content(spans)]])
                } else {
                    Paragraph::new_unordered_list()
                        .with_entries(vec![vec![Paragraph::new_text().with_content(spans)]])
                }
            }
            BlockType::Table { rows } => Paragraph::Table {
                rows: rows
                    .iter()
                    .map(|r| TdocTableRow {
                        cells: r
                            .cells
                            .iter()
                            .map(|c| TdocTableCell {
                                is_header: c.is_header,
                                content: inline_to_spans(&c.content),
                            })
                            .collect(),
                    })
                    .collect(),
            },
        }
    }

    /// Byte length of the first leaf's plain text.
    fn leaf0_len(display: &StructuredRichDisplay) -> usize {
        tree_walk::leaf_text_len(display.editor().tdoc(), &TreePath::root(0))
    }

    #[derive(Default)]
    struct TestDrawContext {
        focus: bool,
        active: bool,
    }

    impl TestDrawContext {
        fn new_with_focus() -> Self {
            Self {
                focus: true,
                active: true,
            }
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
            self.active
        }
    }

    fn make_display_with_block(block: Block) -> StructuredRichDisplay {
        make_display_with_blocks(vec![block])
    }

    fn make_display_with_blocks(blocks: Vec<Block>) -> StructuredRichDisplay {
        let mut display = StructuredRichDisplay::new(0, 0, 400, 300);
        let doc = Document::new().with_paragraphs(blocks.iter().map(block_to_paragraph).collect());
        {
            let editor = display.editor_mut();
            editor.set_tdoc(doc);
            editor.set_cursor(DocumentPosition::new(0, 0));
        }
        display
    }

    fn table_block(rows: usize) -> Block {
        let rows = (0..rows)
            .map(|r| {
                TableRow::new(vec![TableCell::new(
                    false,
                    vec![InlineContent::Text(TextRun::plain(format!("r{r}")))],
                )])
            })
            .collect();
        Block::table(rows)
    }

    #[test]
    fn quote_nested_content_keeps_quote_and_list_indent() {
        // A quote containing a numbered list, a nested bullet, and a continuation
        // paragraph: every line must carry the quote indent/bar *and* its list indent.
        let md = "> Plain in quote\n>\n> 1. Numbered in quote\n>\n>    - Bullet in quote\n>\n>      Continuation in bullet";
        let doc = crate::richtext::markdown_converter::markdown_to_document(md);
        let mut display = StructuredRichDisplay::new(0, 0, 600, 400);
        display.editor_mut().set_tdoc(doc);
        let mut ctx = TestDrawContext::new_with_focus();
        display.layout(&mut ctx);

        // Text is split into word runs; match each line by a unique first word.
        let x_of = |needle: &str| -> i32 {
            display
                .layout_lines
                .iter()
                .flat_map(|l| &l.runs)
                .find(|r| r.text.starts_with(needle))
                .map(|r| r.x)
                .unwrap_or_else(|| panic!("no run starting with {needle:?}"))
        };

        let pad = display.theme.padding_horizontal;
        let quote_indent = display.theme.quote_indent;
        // The plain quote paragraph sits at exactly the quote indent.
        assert_eq!(x_of("Plain"), pad + quote_indent);
        // Deeper list nesting indents further; each is still past the quote indent.
        assert!(x_of("Numbered") > pad + quote_indent);
        assert!(x_of("Bullet") > x_of("Numbered"));
        // The continuation paragraph aligns with its bullet item's content (not far left).
        assert_eq!(x_of("Continuation"), x_of("Bullet"));

        // Every leaf in the quote reports quote_depth 1, so each gets a vertical bar.
        for info in &display.layout_leaves {
            assert_eq!(
                info.quote_depth, 1,
                "leaf {:?} lost its quote depth",
                info.path
            );
        }
    }

    #[test]
    fn nested_list_item_is_more_indented() {
        // A list item nested under another should lay out further to the right.
        let inner = Paragraph::new_unordered_list().with_entries(vec![vec![
            Paragraph::new_text().with_content(vec![tdoc::inline::Span::new_text("bbb")]),
        ]]);
        let outer = Paragraph::new_unordered_list().with_entries(vec![vec![
            Paragraph::new_text().with_content(vec![tdoc::inline::Span::new_text("aaa")]),
            inner,
        ]]);
        let doc = Document::new().with_paragraphs(vec![outer]);
        let mut display = StructuredRichDisplay::new(0, 0, 400, 300);
        display.editor_mut().set_tdoc(doc);
        let mut ctx = TestDrawContext::new_with_focus();
        display.layout(&mut ctx);

        let x_of = |needle: &str| -> i32 {
            display
                .layout_lines
                .iter()
                .flat_map(|l| &l.runs)
                .find(|r| r.text.contains(needle))
                .map(|r| r.x)
                .unwrap_or_else(|| panic!("no run containing {needle:?}"))
        };
        assert!(
            x_of("bbb") > x_of("aaa"),
            "nested item ({}) should be more indented than its parent ({})",
            x_of("bbb"),
            x_of("aaa")
        );
    }

    #[test]
    fn vertical_nav_treats_table_as_single_stop() {
        // A multi-row table produces several layout lines; Up/Down must treat
        // the whole table as one stop instead of stepping through each line.
        let mut ctx = TestDrawContext::new_with_focus();
        let mut display = make_display_with_blocks(vec![
            Block::paragraph().with_plain_text("one"),
            table_block(3),
            Block::paragraph().with_plain_text("two"),
        ]);
        // Force layout so the table contributes multiple layout lines.
        display.draw(&mut ctx);
        assert_eq!(display.editor().cursor().path, TreePath::root(0));

        // Down: land on the table once.
        display.move_cursor_visual_down(false, &mut ctx);
        assert_eq!(display.editor().cursor().path, TreePath::root(1));

        // Down again: skip the entire table to the paragraph below it.
        display.move_cursor_visual_down(false, &mut ctx);
        assert_eq!(display.editor().cursor().path, TreePath::root(2));

        // Up: back onto the table once.
        display.move_cursor_visual_up(false, &mut ctx);
        assert_eq!(display.editor().cursor().path, TreePath::root(1));

        // Up again: skip the entire table to the first paragraph.
        display.move_cursor_visual_up(false, &mut ctx);
        assert_eq!(display.editor().cursor().path, TreePath::root(0));
    }

    #[test]
    fn test_display_creation() {
        let display = StructuredRichDisplay::new(0, 0, 800, 600);
        assert_eq!(display.w(), 800);
        assert_eq!(display.h(), 600);
    }

    #[test]
    fn cursor_in_empty_blockquote_respects_indent() {
        let block = Block::new(BlockType::BlockQuote);
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, display.theme.padding_horizontal + 20);
    }

    #[test]
    fn cursor_in_empty_unordered_list_respects_content_indent() {
        let block = Block::new(BlockType::ListItem {
            ordered: false,
            number: None,
            checkbox: None,
            depth: 0,
        });
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
        // A single ordered item renders as "1." (ordinals are derived from tree position).
        let block = Block::new(BlockType::ListItem {
            ordered: true,
            number: Some(1),
            checkbox: None,
            depth: 0,
        });
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        let label_width = ctx.text_width(
            "1. ",
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
        let block = Block::new(BlockType::ListItem {
            ordered: false,
            number: None,
            checkbox: Some(false),
            depth: 0,
        });
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
        let block = Block::new(BlockType::CodeBlock { language: None });
        let mut display = make_display_with_block(block);
        let mut ctx = TestDrawContext::new_with_focus();

        display.layout(&mut ctx);
        let (x, _, _) = display.get_cursor_visual_position(&mut ctx).unwrap();
        assert_eq!(x, display.theme.padding_horizontal + 10);
    }

    #[test]
    fn trailing_hard_break_creates_empty_visual_line() {
        let mut block = Block::paragraph();
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

        let end_offset = leaf0_len(&display);
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
        let block = Block::paragraph().with_plain_text(
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

    #[test]
    fn hard_break_blank_line_has_nonzero_offset() {
        let mut block = Block::paragraph();
        block
            .content
            .push(InlineContent::Text(TextRun::plain("Hello")));
        block.content.push(InlineContent::HardBreak);
        block.content.push(InlineContent::HardBreak);
        block
            .content
            .push(InlineContent::Text(TextRun::plain("World")));
        let mut display = make_display_with_block(block);

        let mut ctx = TestDrawContext::new_with_focus();
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
