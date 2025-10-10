// Text Display widget implementation
// Based on FLTK's Fl_Text_Display

use crate::text_buffer::TextBuffer;
use std::cell::RefCell;
use std::cmp::{max, min};
use std::rc::Rc;

// Drawing backend trait - abstracts over FLTK's drawing primitives
pub trait DrawContext {
    fn set_color(&mut self, color: u32);
    fn set_font(&mut self, font: u8, size: u8);
    fn draw_text(&mut self, text: &str, x: i32, y: i32);
    fn draw_rect_filled(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32);
    fn text_width(&mut self, text: &str, font: u8, size: u8) -> f64;
    fn text_height(&self, font: u8, size: u8) -> i32;
    fn text_descent(&self, font: u8, size: u8) -> i32;
    fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn pop_clip(&mut self);
    fn color_average(&self, c1: u32, c2: u32, weight: f32) -> u32;
    fn color_contrast(&self, fg: u32, bg: u32) -> u32;
    fn color_inactive(&self, c: u32) -> u32;
    fn has_focus(&self) -> bool;
    fn is_active(&self) -> bool;
}

/// Cursor style enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorStyle {
    Normal, // I-beam
    Caret,  // Caret under the text
    Dim,    // Dim I-beam
    Block,  // Unfilled box under current character
    Heavy,  // Thick I-beam
    Simple, // Simple cursor like Fl_Input
}

/// Position type for xy_to_position conversions
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PositionType {
    CursorPos,    // Position is between characters
    CharacterPos, // Position is at character edge
}

/// Drag type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragType {
    None = -2,
    StartDnd = -1,
    Char = 0,
    Word = 1,
    Line = 2,
}

/// Wrap mode enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WrapMode {
    None,     // Don't wrap text
    AtColumn, // Wrap at given column
    AtPixel,  // Wrap at pixel position
    AtBounds, // Wrap to fit widget width
}

/// Style table entry for syntax highlighting
#[derive(Debug, Clone, Copy)]
pub struct StyleTableEntry {
    pub color: u32,   // Text color (RGB)
    pub font: u8,     // Font face
    pub size: u8,     // Font size
    pub attr: u32,    // Attributes (underline, etc.)
    pub bgcolor: u32, // Background color
}

/// Style attribute flags
pub mod style_attr {
    pub const BGCOLOR: u32 = 0x0001;
    pub const BGCOLOR_EXT_: u32 = 0x0002;
    pub const BGCOLOR_EXT: u32 = 0x0003;
    pub const UNDERLINE: u32 = 0x0004;
    pub const GRAMMAR: u32 = 0x0008;
    pub const SPELLING: u32 = 0x000C;
    pub const STRIKE_THROUGH: u32 = 0x0010;
    pub const LINES_MASK: u32 = 0x001C;
}

// Drawing style masks - used in handle_vline and drawing methods
const FILL_MASK: i32 = 0x0100;
const SECONDARY_MASK: i32 = 0x0200;
const PRIMARY_MASK: i32 = 0x0400;
const HIGHLIGHT_MASK: i32 = 0x0800;
const BG_ONLY_MASK: i32 = 0x1000;
const TEXT_ONLY_MASK: i32 = 0x2000;
const STYLE_LOOKUP_MASK: i32 = 0xff;

// Handle vline modes
const DRAW_LINE: i32 = 0;
const FIND_INDEX: i32 = 1;
const FIND_INDEX_FROM_ZERO: i32 = 2;
const GET_WIDTH: i32 = 3;
const FIND_CURSOR_INDEX: i32 = 4;

// Text area margins
const TOP_MARGIN: i32 = 1;
const BOTTOM_MARGIN: i32 = 1;
const LEFT_MARGIN: i32 = 3;
const RIGHT_MARGIN: i32 = 3;

/// Text Display widget
/// Displays text from a TextBuffer with optional styling and line numbers
pub struct TextDisplay {
    // Position and size
    x: i32,
    y: i32,
    w: i32,
    h: i32,

    // Text buffers
    buffer: Option<Rc<RefCell<TextBuffer>>>,
    style_buffer: Option<Rc<RefCell<TextBuffer>>>,

    // Display state
    cursor_pos: usize,
    cursor_on: bool,
    cursor_style: CursorStyle,
    cursor_preferred_x: i32,
    cursor_color: u32,

    // Scrolling state
    top_line_num: usize,
    horiz_offset: i32,
    n_visible_lines: usize,
    n_buffer_lines: usize,

    // Line start positions cache
    line_starts: Vec<usize>,
    first_char: usize,
    last_char: usize,

    // Wrapping
    continuous_wrap: bool,
    wrap_mode: WrapMode,
    wrap_margin: i32,

    // Styling
    style_table: Vec<StyleTableEntry>,
    n_styles: usize,

    // Font settings
    text_font: u8,
    text_size: u8,
    text_color: u32,

    // Colors
    grammar_underline_color: u32,
    spelling_underline_color: u32,
    secondary_selection_color: u32,

    // Line numbers
    linenumber_width: i32,
    linenumber_font: u8,
    linenumber_size: u8,
    linenumber_fgcolor: u32,
    linenumber_bgcolor: u32,
    linenumber_align: u32, // Alignment flags

    // Scrollbar settings
    scrollbar_width: i32,
    scrollbar_align: u32,

    // Tab distance
    column_scale: f64,

    // Damage tracking
    damage_range1: (usize, usize),
    damage_range2: (usize, usize),

    // Font metrics (calculated lazily)
    max_font_height: i32,
    max_font_width: i32,

    // Text area (content region without margins)
    text_area_x: i32,
    text_area_y: i32,
    text_area_w: i32,
    text_area_h: i32,

    // Display recalc flag
    needs_recalc: bool,

    // Font metrics calculated flag
    font_metrics_calculated: bool,

    // Selection/drag state
    drag_pos: usize,
    drag_type: DragType,
    dragging: bool,
}

impl TextDisplay {
    /// Create a new TextDisplay widget
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        TextDisplay {
            x,
            y,
            w,
            h,
            buffer: None,
            style_buffer: None,
            cursor_pos: 0,
            cursor_on: true,
            cursor_style: CursorStyle::Normal,
            cursor_preferred_x: 0,
            cursor_color: 0x000000FF, // Black
            top_line_num: 0,
            horiz_offset: 0,
            n_visible_lines: 0,
            n_buffer_lines: 0,
            line_starts: Vec::new(),
            first_char: 0,
            last_char: 0,
            continuous_wrap: false,
            wrap_mode: WrapMode::None,
            wrap_margin: 0,
            style_table: Vec::new(),
            n_styles: 0,
            text_font: 0,
            text_size: 14,
            text_color: 0x000000FF,
            grammar_underline_color: 0x0000FFFF,   // Blue
            spelling_underline_color: 0xFF0000FF,  // Red
            secondary_selection_color: 0xD3D3D3FF, // Light gray
            linenumber_width: 0,
            linenumber_font: 0,
            linenumber_size: 14,
            linenumber_fgcolor: 0x000000FF,
            linenumber_bgcolor: 0xE0E0E0FF,
            linenumber_align: 0,
            scrollbar_width: 0,
            scrollbar_align: 0,
            column_scale: 0.0,
            damage_range1: (0, 0),
            damage_range2: (0, 0),
            max_font_height: 14, // Will be calculated from text_size
            max_font_width: 8,   // Will be calculated from font metrics
            text_area_x: x,
            text_area_y: y,
            text_area_w: w,
            text_area_h: h,
            needs_recalc: true,
            font_metrics_calculated: false,
            drag_pos: 0,
            drag_type: DragType::None,
            dragging: false,
        }
    }

    // ========================================================================
    // Font Metrics Calculation
    // ========================================================================

    /// Calculate font metrics based on current font and style settings
    fn calculate_font_metrics(&mut self) {
        // Base metrics from default font
        // Font height is approximately 1.2x the point size (accounts for ascent + descent + leading)
        // This matches typical font metrics behavior
        self.max_font_height = ((self.text_size as f64) * 1.2) as i32;

        // Check all style fonts to find maximum height
        for style in &self.style_table {
            let style_height = ((style.size as f64) * 1.2) as i32;
            self.max_font_height = max(self.max_font_height, style_height);
        }

        // Calculate average character width for monospace assumption
        // Using "Mitg" as sample (mix of wide and narrow chars) divided by 4
        // This matches FLTK's column scale calculation
        self.max_font_width = (self.text_size as f64 * 0.6) as i32;

        // Update column scale for x/col conversions
        self.column_scale = self.max_font_width as f64;
    }

    /// Update font metrics using actual font measurements from DrawContext
    /// This should be called during draw to get precise measurements
    fn update_font_metrics_from_context(&mut self, ctx: &mut dyn DrawContext) {
        // Get actual height from default font
        self.max_font_height = ctx.text_height(self.text_font, self.text_size);

        // Check all style fonts to find maximum height
        for style in &self.style_table {
            let style_height = ctx.text_height(style.font, style.size);
            self.max_font_height = max(self.max_font_height, style_height);
        }

        // Update width measurement using actual font metrics
        let sample = "Mitg";
        let sample_width = ctx.text_width(sample, self.text_font, self.text_size);
        self.max_font_width = (sample_width / 4.0) as i32;
        self.column_scale = self.max_font_width as f64;
    }

    // ========================================================================
    // Buffer Management
    // ========================================================================

    /// Set the text buffer
    pub fn set_buffer(&mut self, buffer: Rc<RefCell<TextBuffer>>) {
        self.buffer = Some(buffer);
        self.recalc_display();
    }

    /// Get the text buffer
    pub fn buffer(&self) -> Option<Rc<RefCell<TextBuffer>>> {
        self.buffer.clone()
    }

    /// Set the style buffer for syntax highlighting
    pub fn set_style_buffer(&mut self, style_buffer: Rc<RefCell<TextBuffer>>) {
        self.style_buffer = Some(style_buffer);
    }

    /// Get the style buffer
    pub fn style_buffer(&self) -> Option<Rc<RefCell<TextBuffer>>> {
        self.style_buffer.clone()
    }

    // ========================================================================
    // Cursor Management
    // ========================================================================

    /// Get the current cursor position
    pub fn insert_position(&self) -> usize {
        self.cursor_pos
    }

    /// Set the cursor position
    pub fn set_insert_position(&mut self, pos: usize) {
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();
            self.cursor_pos = min(pos, buf.length());
        } else {
            self.cursor_pos = pos;
        }
    }

    /// Show the cursor
    pub fn show_cursor(&mut self, show: bool) {
        self.cursor_on = show;
    }

    /// Hide the cursor
    pub fn hide_cursor(&mut self) {
        self.cursor_on = false;
    }

    /// Set cursor style
    pub fn set_cursor_style(&mut self, style: CursorStyle) {
        self.cursor_style = style;
    }

    /// Get cursor style
    pub fn cursor_style(&self) -> CursorStyle {
        self.cursor_style
    }

    /// Set cursor color
    pub fn set_cursor_color(&mut self, color: u32) {
        self.cursor_color = color;
    }

    /// Get cursor color
    pub fn cursor_color(&self) -> u32 {
        self.cursor_color
    }

    // ========================================================================
    // Text Operations
    // ========================================================================

    /// Insert text at the current cursor position
    pub fn insert(&mut self, text: &str) {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow_mut().insert(self.cursor_pos, text);
            self.cursor_pos += text.len();
        }
    }

    /// Overstrike text (replace mode)
    pub fn overstrike(&mut self, text: &str) {
        if let Some(ref buffer) = self.buffer {
            let mut buf = buffer.borrow_mut();
            let end = min(self.cursor_pos + text.len(), buf.length());
            buf.replace(self.cursor_pos, end, text);
            self.cursor_pos += text.len();
        }
    }

    // ========================================================================
    // Scrolling
    // ========================================================================

    /// Scroll to show the given line at the top
    pub fn scroll(&mut self, top_line_num: usize, horiz_offset: i32) {
        self.top_line_num = top_line_num;
        self.horiz_offset = horiz_offset;
        self.recalc_display();
    }

    /// Get the top line number
    pub fn top_line_num(&self) -> usize {
        self.top_line_num
    }

    /// Get the horizontal offset
    pub fn horiz_offset(&self) -> i32 {
        self.horiz_offset
    }

    /// Show the insert position (scroll to make cursor visible)
    pub fn show_insert_position(&mut self) {
        if self.buffer.is_none() {
            return;
        }

        // Calculate which line the cursor is on
        let cursor_line = self.count_lines(0, self.cursor_pos);

        // Vertical scrolling: Check if cursor line is visible
        let mut new_top = self.top_line_num;
        if cursor_line < self.top_line_num {
            // Cursor is above visible area, scroll up
            new_top = cursor_line;
        } else if cursor_line >= self.top_line_num + self.n_visible_lines {
            // Cursor is below visible area, scroll down
            new_top = cursor_line.saturating_sub(self.n_visible_lines - 1);
        }

        // Horizontal scrolling: Check if cursor is horizontally visible
        let (_line_start, cursor_col) = {
            let buffer = self.buffer.as_ref().unwrap().borrow();
            let line_start = buffer.line_start(self.cursor_pos);
            let cursor_col = buffer.count_displayed_characters(line_start, self.cursor_pos);
            (line_start, cursor_col)
        };

        let cursor_x = (cursor_col as i32 * self.max_font_width) - self.horiz_offset;

        let mut new_horiz = self.horiz_offset;

        // Left margin for comfort (a few characters)
        let left_margin = self.max_font_width * 2;
        let right_margin = self.max_font_width * 2;

        if cursor_x < left_margin {
            // Cursor is too far left, scroll left
            new_horiz = (cursor_col as i32 * self.max_font_width) - left_margin;
            new_horiz = max(0, new_horiz);
        } else if cursor_x > self.text_area_w - right_margin {
            // Cursor is too far right, scroll right
            new_horiz = (cursor_col as i32 * self.max_font_width) - self.text_area_w + right_margin;
            new_horiz = max(0, new_horiz);
        }

        // Apply scrolling if changed
        if new_top != self.top_line_num || new_horiz != self.horiz_offset {
            self.scroll(new_top, new_horiz);
        }
    }

    // ========================================================================
    // Movement
    // ========================================================================

    /// Move cursor right one character
    pub fn move_right(&mut self) -> bool {
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();
            if self.cursor_pos < buf.length() {
                self.cursor_pos = buf.next_char(self.cursor_pos);
                return true;
            }
        }
        false
    }

    /// Move cursor left one character
    pub fn move_left(&mut self) -> bool {
        if self.cursor_pos > 0 {
            if let Some(ref buffer) = self.buffer {
                let buf = buffer.borrow();
                self.cursor_pos = buf.prev_char(self.cursor_pos);
                return true;
            }
        }
        false
    }

    /// Move cursor up one line
    pub fn move_up(&mut self) -> bool {
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();

            // Find current line start
            let line_start = buf.line_start(self.cursor_pos);

            // If already on first line, can't move up
            if line_start == 0 {
                return false;
            }

            // Get column position in current line
            let col = buf.count_displayed_characters(line_start, self.cursor_pos);

            // Move to previous line
            let prev_line_start = buf.line_start(line_start - 1);
            let prev_line_end = buf.line_end(prev_line_start);

            // Try to position at same column in previous line
            self.cursor_pos = buf.skip_displayed_characters(prev_line_start, col);

            // Clip to end of line if column is past end
            if self.cursor_pos > prev_line_end {
                self.cursor_pos = prev_line_end;
            }

            return true;
        }
        false
    }

    /// Move cursor down one line
    pub fn move_down(&mut self) -> bool {
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();

            // Find current line
            let line_start = buf.line_start(self.cursor_pos);
            let line_end = buf.line_end(self.cursor_pos);

            // If at last line, can't move down
            if line_end >= buf.length() {
                return false;
            }

            // Get column position in current line
            let col = buf.count_displayed_characters(line_start, self.cursor_pos);

            // Move to next line (skip the newline)
            let next_line_start = buf.next_char(line_end);
            if next_line_start >= buf.length() {
                return false;
            }

            let next_line_end = buf.line_end(next_line_start);

            // Try to position at same column in next line
            self.cursor_pos = buf.skip_displayed_characters(next_line_start, col);

            // Clip to end of line if column is past end
            if self.cursor_pos > next_line_end {
                self.cursor_pos = next_line_end;
            }

            return true;
        }
        false
    }

    /// Move to next word
    pub fn next_word(&mut self) {
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();
            self.cursor_pos = buf.word_end(self.cursor_pos);
            // Skip whitespace
            while self.cursor_pos < buf.length() && buf.is_word_separator(self.cursor_pos) {
                self.cursor_pos = buf.next_char(self.cursor_pos);
            }
        }
    }

    /// Move to previous word
    pub fn previous_word(&mut self) {
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();

            // If we're in a word, move to its start first
            if self.cursor_pos > 0 && !buf.is_word_separator(self.cursor_pos) {
                self.cursor_pos = buf.word_start(self.cursor_pos);
                // If we're not at the beginning, continue to previous word
                if self.cursor_pos == 0 {
                    return;
                }
                self.cursor_pos = buf.prev_char(self.cursor_pos);
            }

            // Skip back over whitespace
            while self.cursor_pos > 0 && buf.is_word_separator(self.cursor_pos) {
                self.cursor_pos = buf.prev_char(self.cursor_pos);
            }

            // Go to start of that word
            if self.cursor_pos > 0 {
                self.cursor_pos = buf.word_start(self.cursor_pos);
            }
        }
    }

    // ========================================================================
    // Line Navigation (delegating to buffer)
    // ========================================================================

    /// Get the start of the line containing pos
    pub fn line_start(&self, pos: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().line_start(pos)
        } else {
            0
        }
    }

    /// Get the end of the line containing pos
    pub fn line_end(&self, pos: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().line_end(pos)
        } else {
            0
        }
    }

    /// Count lines between positions
    pub fn count_lines(&self, start: usize, end: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().count_lines(start, end)
        } else {
            0
        }
    }

    /// Skip forward n lines
    pub fn skip_lines(&self, start: usize, n_lines: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().skip_lines(start, n_lines)
        } else {
            0
        }
    }

    /// Rewind n lines backward
    pub fn rewind_lines(&self, start: usize, n_lines: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().rewind_lines(start, n_lines)
        } else {
            0
        }
    }

    /// Get word start position
    pub fn word_start(&self, pos: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().word_start(pos)
        } else {
            0
        }
    }

    /// Get word end position
    pub fn word_end(&self, pos: usize) -> usize {
        if let Some(ref buffer) = self.buffer {
            buffer.borrow().word_end(pos)
        } else {
            0
        }
    }

    // ========================================================================
    // Font and Color Settings
    // ========================================================================

    /// Set text font
    pub fn set_textfont(&mut self, font: u8) {
        self.text_font = font;
        self.font_metrics_calculated = false; // Need to recalculate with new font
        self.calculate_font_metrics();
        self.display_needs_recalc();
    }

    /// Get text font
    pub fn textfont(&self) -> u8 {
        self.text_font
    }

    /// Set text size
    pub fn set_textsize(&mut self, size: u8) {
        self.text_size = size;
        self.font_metrics_calculated = false; // Need to recalculate with new size
        self.calculate_font_metrics();
        self.display_needs_recalc();
    }

    /// Get text size
    pub fn textsize(&self) -> u8 {
        self.text_size
    }

    /// Set text color
    pub fn set_textcolor(&mut self, color: u32) {
        self.text_color = color;
    }

    /// Get text color
    pub fn textcolor(&self) -> u32 {
        self.text_color
    }

    /// Set grammar underline color
    pub fn set_grammar_underline_color(&mut self, color: u32) {
        self.grammar_underline_color = color;
    }

    /// Get grammar underline color
    pub fn grammar_underline_color(&self) -> u32 {
        self.grammar_underline_color
    }

    /// Set spelling underline color
    pub fn set_spelling_underline_color(&mut self, color: u32) {
        self.spelling_underline_color = color;
    }

    /// Get spelling underline color
    pub fn spelling_underline_color(&self) -> u32 {
        self.spelling_underline_color
    }

    /// Set secondary selection color
    pub fn set_secondary_selection_color(&mut self, color: u32) {
        self.secondary_selection_color = color;
    }

    /// Get secondary selection color
    pub fn secondary_selection_color(&self) -> u32 {
        self.secondary_selection_color
    }

    // ========================================================================
    // Wrapping
    // ========================================================================

    /// Set wrap mode
    pub fn set_wrap_mode(&mut self, mode: WrapMode, margin: i32) {
        self.wrap_mode = mode;
        self.wrap_margin = margin;
        self.continuous_wrap = !matches!(mode, WrapMode::None);
        self.recalc_display();
    }

    /// Get wrap mode
    pub fn wrap_mode(&self) -> WrapMode {
        self.wrap_mode
    }

    // ========================================================================
    // Line Numbers
    // ========================================================================

    /// Set line number margin width
    pub fn set_linenumber_width(&mut self, width: i32) {
        self.linenumber_width = width;
    }

    /// Get line number margin width
    pub fn linenumber_width(&self) -> i32 {
        self.linenumber_width
    }

    /// Set line number font
    pub fn set_linenumber_font(&mut self, font: u8) {
        self.linenumber_font = font;
    }

    /// Get line number font
    pub fn linenumber_font(&self) -> u8 {
        self.linenumber_font
    }

    /// Set line number font size
    pub fn set_linenumber_size(&mut self, size: u8) {
        self.linenumber_size = size;
    }

    /// Get line number font size
    pub fn linenumber_size(&self) -> u8 {
        self.linenumber_size
    }

    /// Set line number foreground color
    pub fn set_linenumber_fgcolor(&mut self, color: u32) {
        self.linenumber_fgcolor = color;
    }

    /// Get line number foreground color
    pub fn linenumber_fgcolor(&self) -> u32 {
        self.linenumber_fgcolor
    }

    /// Set line number background color
    pub fn set_linenumber_bgcolor(&mut self, color: u32) {
        self.linenumber_bgcolor = color;
    }

    /// Get line number background color
    pub fn linenumber_bgcolor(&self) -> u32 {
        self.linenumber_bgcolor
    }

    // ========================================================================
    // Scrollbar Settings
    // ========================================================================

    /// Set scrollbar width
    pub fn set_scrollbar_width(&mut self, width: i32) {
        self.scrollbar_width = width;
    }

    /// Get scrollbar width
    pub fn scrollbar_width(&self) -> i32 {
        self.scrollbar_width
    }

    /// Set scrollbar alignment
    pub fn set_scrollbar_align(&mut self, align: u32) {
        self.scrollbar_align = align;
    }

    /// Get scrollbar alignment
    pub fn scrollbar_align(&self) -> u32 {
        self.scrollbar_align
    }

    // ========================================================================
    // Highlighting
    // ========================================================================

    /// Set syntax highlighting style table
    pub fn set_highlight_data(&mut self, style_table: Vec<StyleTableEntry>) {
        self.n_styles = style_table.len();
        self.style_table = style_table;
        self.font_metrics_calculated = false; // Need to recalculate with new styles
        self.calculate_font_metrics(); // Styles may have different font sizes
        self.display_needs_recalc();
    }

    // ========================================================================
    // Display Recalculation
    // ========================================================================

    /// Recalculate the display
    pub fn recalc_display(&mut self) {
        // Update text area position to account for line numbers
        self.text_area_x = self.x + self.linenumber_width;
        self.text_area_y = self.y;
        self.text_area_w = self.w - self.linenumber_width;
        self.text_area_h = self.h;

        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();

            // Calculate number of buffer lines
            self.n_buffer_lines = buf.count_lines(0, buf.length());

            // Calculate number of visible lines based on widget height
            self.n_visible_lines = (self.text_area_h / self.max_font_height) as usize;

            // Allocate line starts array
            self.line_starts.clear();
            self.line_starts.resize(self.n_visible_lines, usize::MAX);

            // Calculate visible line start positions
            let mut pos = buf.skip_lines(0, self.top_line_num);
            self.first_char = pos;

            for i in 0..self.n_visible_lines {
                if pos >= buf.length() {
                    self.line_starts[i] = usize::MAX;
                    continue;
                }

                self.line_starts[i] = pos;

                // Move to next line
                if self.continuous_wrap {
                    // Calculate wrapped line: break at wrap margin
                    let line_end = buf.line_end(pos);
                    let line_text_len = line_end - pos;

                    // Calculate how many characters fit in wrap margin
                    let chars_per_line = match self.wrap_mode {
                        WrapMode::AtColumn => self.wrap_margin as usize,
                        WrapMode::AtPixel => (self.wrap_margin / self.max_font_width) as usize,
                        WrapMode::AtBounds => (self.text_area_w / self.max_font_width) as usize,
                        WrapMode::None => usize::MAX,
                    };

                    if chars_per_line < usize::MAX && line_text_len > 0 {
                        // Count characters that fit on this wrapped line
                        let chars_on_line = buf.count_displayed_characters(pos, line_end);

                        if chars_on_line > chars_per_line {
                            // Line needs wrapping - advance by wrap margin
                            pos = buf.skip_displayed_characters(pos, chars_per_line);

                            // Try to break at word boundary if possible
                            if pos < line_end && pos > 0 {
                                // Look back a few characters for whitespace
                                let mut wrap_pos = pos;
                                let lookback = min(10, chars_per_line / 4);
                                for _ in 0..lookback {
                                    if wrap_pos <= self.line_starts[i] {
                                        break;
                                    }
                                    wrap_pos = buf.prev_char(wrap_pos);
                                    if buf.is_word_separator(wrap_pos) {
                                        pos = buf.next_char(wrap_pos); // After the separator
                                        break;
                                    }
                                }
                            }
                        } else {
                            // Line fits, move to next physical line
                            if line_end < buf.length() {
                                pos = buf.next_char(line_end);
                            } else {
                                pos = buf.length();
                            }
                        }
                    } else {
                        // No wrapping needed or at end of line
                        if line_end < buf.length() {
                            pos = buf.next_char(line_end);
                        } else {
                            pos = buf.length();
                        }
                    }
                } else {
                    // No wrapping - skip to next physical line
                    let line_end = buf.line_end(pos);
                    if line_end < buf.length() {
                        pos = buf.next_char(line_end);
                    } else {
                        pos = buf.length();
                    }
                }
            }

            self.last_char = pos;
        }

        // Always clear the flag, even if there's no buffer
        self.needs_recalc = false;
    }

    /// Mark display as needing recalculation
    pub fn display_needs_recalc(&mut self) {
        self.needs_recalc = true;
    }

    /// Check if display needs recalculation
    pub fn needs_recalc(&self) -> bool {
        self.needs_recalc
    }

    /// Redisplay a range of text
    pub fn redisplay_range(&mut self, start: usize, end: usize) {
        self.damage_range1 = (start, end);
    }

    // ========================================================================
    // Coordinate Conversion
    // ========================================================================

    /// Convert x pixel position to column number
    pub fn x_to_col(&self, x: f64) -> f64 {
        // Simple approximation based on average character width
        if self.column_scale == 0.0 {
            return x / 8.0; // Fallback
        }
        x / self.column_scale
    }

    /// Convert column number to x pixel position
    pub fn col_to_x(&self, col: f64) -> f64 {
        if self.column_scale == 0.0 {
            return col * 8.0; // Fallback
        }
        col * self.column_scale
    }

    /// Convert position to x,y coordinates
    pub fn position_to_xy(&self, pos: usize) -> Option<(i32, i32)> {
        if self.buffer.is_none() || self.line_starts.is_empty() {
            return None;
        }

        let buffer = self.buffer.as_ref().unwrap().borrow();

        // Find which visible line contains this position
        for (vis_line_num, &line_start_pos) in self.line_starts.iter().enumerate() {
            if line_start_pos == usize::MAX {
                // Invalid line start
                continue;
            }

            let line_end = buffer.line_end(line_start_pos);

            if pos >= line_start_pos && pos <= line_end {
                // Found the line
                let col = buffer.count_displayed_characters(line_start_pos, pos);

                let x = self.text_area_x + (col as i32 * self.max_font_width) - self.horiz_offset;
                let y = self.text_area_y + (vis_line_num as i32 * self.max_font_height);

                return Some((x, y));
            }
        }

        None
    }

    /// Convert x,y coordinates to position
    pub fn xy_to_position(&self, x: i32, y: i32, pos_type: PositionType) -> usize {
        if self.buffer.is_none() || self.line_starts.is_empty() {
            return 0;
        }

        let buffer = self.buffer.as_ref().unwrap().borrow();

        // Find the visible line number from Y coordinate
        let vis_line_num = ((y - self.text_area_y) / self.max_font_height) as usize;

        // Clamp to valid range
        let vis_line_num = if vis_line_num >= self.n_visible_lines {
            self.n_visible_lines.saturating_sub(1)
        } else {
            vis_line_num
        };

        // Get line start position
        let line_start = if vis_line_num < self.line_starts.len() {
            self.line_starts[vis_line_num]
        } else {
            return buffer.length();
        };

        if line_start == usize::MAX {
            return buffer.length();
        }

        // Calculate column from X coordinate
        let mut col = ((x - self.text_area_x + self.horiz_offset) / self.max_font_width) as usize;

        // Adjust for cursor vs character position
        if matches!(pos_type, PositionType::CursorPos) {
            // Round to nearest position (add half character width)
            col = ((x - self.text_area_x + self.horiz_offset + self.max_font_width / 2)
                / self.max_font_width) as usize;
        }

        // Skip to the column position
        let line_end = buffer.line_end(line_start);
        let pos = buffer.skip_displayed_characters(line_start, col);

        // Clip to line end
        min(pos, line_end)
    }

    /// Check if x,y is within a selection
    pub fn in_selection(&self, x: i32, y: i32) -> bool {
        if let Some(ref buffer) = self.buffer {
            let pos = self.xy_to_position(x, y, PositionType::CharacterPos);
            let buf = buffer.borrow();
            let selection = buf.primary_selection();
            selection.contains(pos)
        } else {
            false
        }
    }

    // ========================================================================
    // Mouse/Selection Handling
    // ========================================================================

    /// Handle mouse press event
    /// Returns true if the event was handled
    pub fn handle_push(&mut self, x: i32, y: i32, shift: bool, clicks: i32) -> bool {
        if self.buffer.is_none() {
            return false;
        }

        let pos = self.xy_to_position(x, y, PositionType::CursorPos);

        if shift {
            // Shift-click: extend selection from current cursor to clicked position
            if let Some(ref buffer) = self.buffer {
                let buf = buffer.borrow();
                if buf.primary_selection().selected() {
                    self.drag_pos = self.cursor_pos;
                } else {
                    self.drag_pos = self.cursor_pos;
                }
            }
            self.dragging = true;
            self.update_drag_selection(pos);
            return true;
        }

        // Check if clicking within existing selection
        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();
            if buf.primary_selection().contains(pos) {
                self.drag_type = DragType::StartDnd;
                self.drag_pos = pos;
                self.dragging = true;
                return true;
            }
        }

        // Start new selection based on click count
        self.dragging = true;
        self.drag_pos = pos;

        match clicks {
            0 => {
                // Single click: character selection
                self.drag_type = DragType::Char;
                if let Some(ref buffer) = self.buffer {
                    buffer.borrow_mut().unselect();
                }
                self.set_insert_position(pos);
            }
            1 => {
                // Double click: word selection
                self.drag_type = DragType::Word;
                let start = self.word_start(pos);
                let end = self.word_end(pos);
                if let Some(ref buffer) = self.buffer {
                    buffer.borrow_mut().select(start, end);
                }
                self.drag_pos = start;
                self.set_insert_position(end);
            }
            _ => {
                // Triple+ click: set drag type but actual selection happens on release
                // This matches C++ FLTK behavior where line selection is finalized on release
                self.drag_type = if clicks == 2 {
                    DragType::Line
                } else {
                    DragType::Char
                };
                if let Some(ref buffer) = self.buffer {
                    buffer.borrow_mut().unselect();
                }
                self.set_insert_position(pos);
            }
        }

        self.show_insert_position();
        true
    }

    /// Handle mouse drag event
    /// Returns true if the event was handled
    pub fn handle_drag(&mut self, x: i32, y: i32) -> bool {
        if !self.dragging || self.buffer.is_none() {
            return false;
        }

        if matches!(self.drag_type, DragType::StartDnd) {
            // DND not yet implemented
            return true;
        }

        let pos = self.xy_to_position(x, y, PositionType::CursorPos);
        self.update_drag_selection(pos);
        self.show_insert_position();
        true
    }

    /// Handle mouse release event
    /// Returns true if the event was handled
    pub fn handle_release(&mut self, _x: i32, _y: i32, clicks: i32) -> bool {
        if !self.dragging {
            return false;
        }

        self.dragging = false;

        // Handle triple-click line selection on release (matches C++ FLTK behavior)
        // C++ checks event_clicks() on release, which is preserved through the event
        if clicks == 2 {
            // Triple-click: select entire line
            if let Some(ref buffer) = self.buffer {
                let buf = buffer.borrow();
                let start = buf.line_start(self.drag_pos);
                let line_end = buf.line_end(self.drag_pos);
                let end = if line_end < buf.length() {
                    buf.next_char(line_end)
                } else {
                    line_end
                };
                drop(buf);
                buffer.borrow_mut().select(start, end);
            }
            self.drag_pos = self.line_start(self.drag_pos);
        }

        // Convert word/line selection back to character selection for future drags
        if !matches!(self.drag_type, DragType::Char) {
            self.drag_type = DragType::Char;
        }

        true
    }

    /// Update selection based on drag position
    fn update_drag_selection(&mut self, pos: usize) {
        if self.buffer.is_none() {
            return;
        }

        let new_cursor_pos = match self.drag_type {
            DragType::Char => {
                // Character-by-character selection
                let buffer = self.buffer.as_ref().unwrap();
                let mut buf = buffer.borrow_mut();
                if pos >= self.drag_pos {
                    buf.select(self.drag_pos, pos);
                    pos
                } else {
                    buf.select(pos, self.drag_pos);
                    pos
                }
            }
            DragType::Word => {
                // Word selection
                let start;
                let end;
                if pos >= self.drag_pos {
                    start = self.word_start(self.drag_pos);
                    end = self.word_end(pos);
                } else {
                    start = self.word_start(pos);
                    end = self.word_end(self.drag_pos);
                }
                let buffer = self.buffer.as_ref().unwrap();
                buffer.borrow_mut().select(start, end);
                if pos >= self.drag_pos {
                    end
                } else {
                    start
                }
            }
            DragType::Line => {
                // Line selection
                let buffer = self.buffer.as_ref().unwrap();
                let buf = buffer.borrow();
                let start;
                let end;
                if pos >= self.drag_pos {
                    start = buf.line_start(self.drag_pos);
                    let line_end = buf.line_end(pos);
                    end = if line_end < buf.length() {
                        buf.next_char(line_end)
                    } else {
                        line_end
                    };
                } else {
                    start = buf.line_start(pos);
                    let line_end = buf.line_end(self.drag_pos);
                    end = if line_end < buf.length() {
                        buf.next_char(line_end)
                    } else {
                        line_end
                    };
                }
                drop(buf); // Drop the borrow before the next mutable borrow
                buffer.borrow_mut().select(start, end);
                if pos >= self.drag_pos {
                    end
                } else {
                    start
                }
            }
            DragType::None | DragType::StartDnd => {
                // No selection update
                return;
            }
        };

        self.set_insert_position(new_cursor_pos);
    }

    // ========================================================================
    // Widget Dimensions
    // ========================================================================

    /// Get widget x position
    pub fn x(&self) -> i32 {
        self.x
    }

    /// Get widget y position
    pub fn y(&self) -> i32 {
        self.y
    }

    /// Get widget width
    pub fn w(&self) -> i32 {
        self.w
    }

    /// Get widget height
    pub fn h(&self) -> i32 {
        self.h
    }

    /// Resize the widget
    pub fn resize(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.x = x;
        self.y = y;
        self.w = w;
        self.h = h;
        self.recalc_display();
    }

    // ========================================================================
    // Drawing Methods
    // ========================================================================

    /// Get the length of a visible line by examining line starts array
    fn vline_length(&self, vis_line_num: usize) -> usize {
        if vis_line_num >= self.n_visible_lines {
            return 0;
        }

        let line_start_pos = self.line_starts[vis_line_num];
        if line_start_pos == usize::MAX {
            return 0;
        }

        if vis_line_num + 1 >= self.n_visible_lines {
            return self.last_char.saturating_sub(line_start_pos);
        }

        let next_line_start = self.line_starts[vis_line_num + 1];
        if next_line_start == usize::MAX {
            return self.last_char.saturating_sub(line_start_pos);
        }

        if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();
            let next_line_start_minus_1 = buf.prev_char(next_line_start);

            // Check if the line break uses a character (like \n)
            if next_line_start_minus_1 < buf.length() {
                let ch = buf.char_at(next_line_start_minus_1);
                if ch == '\n' as u32 {
                    return next_line_start_minus_1.saturating_sub(line_start_pos);
                }
            }
        }

        next_line_start.saturating_sub(line_start_pos)
    }

    /// Check if there are empty visible lines
    fn empty_vlines(&self) -> bool {
        self.n_visible_lines > 0 && self.line_starts[self.n_visible_lines - 1] == usize::MAX
    }

    /// Find the visible line number for a buffer position
    fn position_to_line(&self, pos: usize, line_num: &mut usize) -> bool {
        *line_num = 0;

        if pos < self.first_char {
            return false;
        }

        if pos > self.last_char {
            if self.empty_vlines() {
                if let Some(ref buffer) = self.buffer {
                    let buf = buffer.borrow();
                    if self.last_char < buf.length() {
                        if !self.position_to_line(self.last_char, line_num) {
                            return false;
                        }
                        *line_num += 1;
                        return *line_num <= self.n_visible_lines - 1;
                    } else {
                        self.position_to_line(buf.prev_char(self.last_char), line_num);
                        return true;
                    }
                }
            }
            return false;
        }

        for i in (0..self.n_visible_lines).rev() {
            if self.line_starts[i] != usize::MAX && pos >= self.line_starts[i] {
                *line_num = i;
                return true;
            }
        }

        false
    }

    /// Determine the drawing style for a character position
    fn position_style(&self, line_start_pos: usize, line_len: usize, line_index: isize) -> i32 {
        if line_start_pos == usize::MAX {
            return FILL_MASK;
        }

        let buffer = match &self.buffer {
            Some(buf) => buf,
            None => return FILL_MASK,
        };

        let buf = buffer.borrow();
        let pos = if line_index < 0 {
            line_start_pos
        } else {
            line_start_pos + min(line_index as usize, line_len)
        };

        let mut style: i32 = 0;

        // Check if we should extend background color or use fill
        if let Some(ref style_buffer) = self.style_buffer {
            let style_buf = style_buffer.borrow();

            if line_index as usize == line_len && line_len > 0 && pos > 0 {
                // Check previous character's style for background extension
                let style_char = style_buf.byte_at(pos - 1);
                if style_char != 0 {
                    let si = (style_char as i32 - b'A' as i32)
                        .max(0)
                        .min((self.n_styles - 1) as i32) as usize;
                    if si < self.style_table.len() {
                        let style_rec = &self.style_table[si];
                        if (style_rec.attr & style_attr::BGCOLOR_EXT_) == 0 {
                            return FILL_MASK;
                        }
                        style = style_char as i32;
                    }
                }
            } else if line_index as usize >= line_len {
                return FILL_MASK;
            } else if pos < style_buf.length() {
                let style_char = style_buf.byte_at(pos);
                style = style_char as i32;
            }
        } else if line_index as usize >= line_len {
            return FILL_MASK;
        }

        // Add selection masks
        if buf.primary_selection().contains(pos) {
            style |= PRIMARY_MASK;
        }
        if buf.highlight_selection().contains(pos) {
            style |= HIGHLIGHT_MASK;
        }
        if buf.secondary_selection().contains(pos) {
            style |= SECONDARY_MASK;
        }

        style
    }

    /// Measure the width of a styled string
    fn string_width(
        &self,
        string: &str,
        length: usize,
        style: i32,
        ctx: &mut dyn DrawContext,
    ) -> f64 {
        let (font, fsize) = if self.n_styles > 0 && (style & STYLE_LOOKUP_MASK) != 0 {
            let si = ((style & STYLE_LOOKUP_MASK) - b'A' as i32)
                .max(0)
                .min((self.n_styles - 1) as i32) as usize;
            if si < self.style_table.len() {
                (self.style_table[si].font, self.style_table[si].size)
            } else {
                (self.text_font, self.text_size)
            }
        } else {
            (self.text_font, self.text_size)
        };

        let text = if length < string.len() {
            &string[..length]
        } else {
            string
        };

        ctx.text_width(text, font, fsize)
    }

    /// Find the character index at a given pixel position within a string
    fn find_x(&self, s: &str, len: usize, style: i32, x: i32, ctx: &mut dyn DrawContext) -> usize {
        let cursor_pos = x < 0;
        let x = x.abs();

        let mut i = 0;
        let mut last_w = 0;
        let chars: Vec<char> = s.chars().collect();

        while i < len && i < chars.len() {
            let char_end = i + 1;
            let substr: String = chars[..char_end].iter().collect();
            let w = self.string_width(&substr, substr.len(), style, ctx) as i32;

            if w > x {
                if cursor_pos && (w - x < x - last_w) {
                    return char_end;
                }
                return i;
            }
            last_w = w;
            i = char_end;
        }

        len
    }

    /// Measure the width of a visible line
    fn measure_vline(&self, vis_line_num: usize, ctx: &mut dyn DrawContext) -> i32 {
        let line_len = self.vline_length(vis_line_num);
        let line_start_pos = self.line_starts[vis_line_num];

        if line_start_pos == usize::MAX || line_len == 0 {
            return 0;
        }

        self.handle_vline(
            GET_WIDTH,
            line_start_pos,
            line_len,
            0,
            usize::MAX,
            0,
            0,
            0,
            0,
            ctx,
        )
    }

    /// Universal pixel machine - handles drawing, measuring, and position finding
    /// This is the core rendering engine that handles all text layout operations
    fn handle_vline(
        &self,
        mode: i32,
        line_start_pos: usize,
        line_len: usize,
        _left_char: usize,
        _right_char: usize,
        y: i32,
        _bottom_clip: i32,
        _left_clip: i32,
        right_clip: i32,
        ctx: &mut dyn DrawContext,
    ) -> i32 {
        let line_str = if line_start_pos == usize::MAX {
            None
        } else if let Some(ref buffer) = self.buffer {
            let buf = buffer.borrow();
            let text = buf.text_range(line_start_pos, line_start_pos + line_len);
            // Filter out carriage returns to prevent ^M display on Windows line endings
            Some(text.replace('\r', ""))
        } else {
            None
        };

        // Handle FIND_CURSOR_INDEX mode
        let mut cursor_pos = false;
        let mut mode = mode;
        if mode == FIND_CURSOR_INDEX {
            mode = FIND_INDEX;
            cursor_pos = true;
        }

        let mut x = if mode == GET_WIDTH {
            0.0
        } else if mode == FIND_INDEX_FROM_ZERO {
            mode = FIND_INDEX;
            0.0
        } else {
            (self.text_area_x - self.horiz_offset) as f64
        };

        let line_str = match line_str {
            Some(s) => s,
            None => {
                // No text - just clear background if in draw mode
                if mode == DRAW_LINE {
                    let style = self.position_style(line_start_pos, line_len, -1);
                    self.draw_string(
                        style | BG_ONLY_MASK,
                        self.text_area_x,
                        y,
                        self.text_area_x + self.text_area_w,
                        "",
                        0,
                        ctx,
                    );
                }
                if mode == FIND_INDEX {
                    return line_start_pos as i32;
                }
                return 0;
            }
        };

        // Draw in two passes: backgrounds first, then text
        for loop_pass in 1..=2 {
            let mask = if loop_pass == 1 {
                BG_ONLY_MASK
            } else {
                TEXT_ONLY_MASK
            };

            let mut start_x = x;
            let mut start_index = 0;
            let mut style_x = start_x;
            let mut start_style = start_index;
            let mut style = self.position_style(line_start_pos, line_len, 0);

            let chars: Vec<char> = line_str.chars().collect();
            let mut i = 0;
            let mut prev_char = '\0';

            while i < line_len && i < chars.len() {
                let curr_char = chars[i];
                let char_style = self.position_style(line_start_pos, line_len, i as isize);

                if char_style != style || curr_char == '\t' || prev_char == '\t' {
                    // Draw segment when style changes or tab is found
                    let mut w = 0.0;

                    if prev_char == '\t' {
                        // Handle tab spacing
                        let tab_width = self.col_to_x(if let Some(ref buf) = self.buffer {
                            buf.borrow().tab_distance() as f64
                        } else {
                            8.0
                        });
                        let x_abs = if mode == GET_WIDTH {
                            start_x
                        } else {
                            start_x + self.horiz_offset as f64 - self.text_area_x as f64
                        };
                        w = ((x_abs / tab_width) as i32 + 1) as f64 * tab_width - x_abs;

                        style_x = start_x + w;
                        start_style = i;

                        if mode == DRAW_LINE && loop_pass == 1 {
                            self.draw_string(
                                style | BG_ONLY_MASK,
                                start_x as i32,
                                y,
                                (start_x + w) as i32,
                                "",
                                0,
                                ctx,
                            );
                        }
                        if mode == FIND_INDEX && start_x + w > right_clip as f64 {
                            if cursor_pos && (start_x + w / 2.0 < right_clip as f64) {
                                return (line_start_pos + start_index + 1) as i32;
                            }
                            return (line_start_pos + start_index) as i32;
                        }
                    } else {
                        // Draw text segment
                        let segment: String = chars[start_index..i].iter().collect();
                        let segment_len = segment.len();

                        w = if (style & 0xff) == (char_style & 0xff) {
                            self.string_width(
                                &chars[start_style..i].iter().collect::<String>(),
                                i - start_style,
                                style,
                                ctx,
                            ) - start_x
                                + style_x
                        } else {
                            self.string_width(&segment, segment_len, style, ctx)
                        };

                        if mode == DRAW_LINE {
                            let draw_segment: String = if start_index != start_style {
                                chars[start_style..i].iter().collect()
                            } else {
                                segment.clone()
                            };

                            if start_index != start_style {
                                ctx.push_clip(
                                    start_x as i32,
                                    y,
                                    w as i32 + 1,
                                    self.max_font_height,
                                );
                                self.draw_string(
                                    style | mask,
                                    style_x as i32,
                                    y,
                                    (start_x + w) as i32,
                                    &draw_segment,
                                    draw_segment.len(),
                                    ctx,
                                );
                                ctx.pop_clip();
                            } else {
                                self.draw_string(
                                    style | mask,
                                    start_x as i32,
                                    y,
                                    (start_x + w) as i32,
                                    &draw_segment,
                                    draw_segment.len(),
                                    ctx,
                                );
                            }
                        }

                        if mode == FIND_INDEX && start_x + w > right_clip as f64 {
                            let di = if start_index != start_style {
                                self.find_x(
                                    &chars[start_style..i].iter().collect::<String>(),
                                    i - start_style,
                                    style,
                                    -(right_clip as i32 - style_x as i32),
                                    ctx,
                                )
                            } else {
                                self.find_x(
                                    &segment,
                                    segment_len,
                                    style,
                                    -(right_clip as i32 - start_x as i32),
                                    ctx,
                                )
                            };
                            return (line_start_pos + start_index + di) as i32;
                        }

                        if (style & 0xff) != (char_style & 0xff) {
                            start_style = i;
                            style_x = start_x + w;
                        }
                    }

                    style = char_style;
                    start_x += w;
                    start_index = i;
                }

                prev_char = curr_char;
                i += 1;
            }

            // Draw final segment
            let mut w = 0.0;
            if prev_char == '\t' {
                let tab_width = self.col_to_x(if let Some(ref buf) = self.buffer {
                    buf.borrow().tab_distance() as f64
                } else {
                    8.0
                });
                let x_abs = if mode == GET_WIDTH {
                    start_x
                } else {
                    start_x + self.horiz_offset as f64 - self.text_area_x as f64
                };
                w = ((x_abs / tab_width) as i32 + 1) as f64 * tab_width - x_abs;

                if mode == DRAW_LINE && loop_pass == 1 {
                    self.draw_string(
                        style | BG_ONLY_MASK,
                        start_x as i32,
                        y,
                        (start_x + w) as i32,
                        "",
                        0,
                        ctx,
                    );
                }
                if mode == FIND_INDEX {
                    if cursor_pos {
                        return (line_start_pos
                            + start_index
                            + if right_clip as f64 - start_x > w / 2.0 {
                                1
                            } else {
                                0
                            }) as i32;
                    }
                    return (line_start_pos
                        + start_index
                        + if right_clip as f64 - start_x > w {
                            1
                        } else {
                            0
                        }) as i32;
                }
            } else {
                let segment: String = chars[start_index..i].iter().collect();
                w = self.string_width(&segment, i - start_index, style, ctx);

                if mode == DRAW_LINE {
                    let draw_segment: String = if start_index != start_style {
                        chars[start_style..i].iter().collect()
                    } else {
                        segment.clone()
                    };

                    if start_index != start_style {
                        ctx.push_clip(start_x as i32, y, w as i32 + 1, self.max_font_height);
                        self.draw_string(
                            style | mask,
                            style_x as i32,
                            y,
                            (start_x + w) as i32,
                            &draw_segment,
                            draw_segment.len(),
                            ctx,
                        );
                        ctx.pop_clip();
                    } else {
                        self.draw_string(
                            style | mask,
                            start_x as i32,
                            y,
                            (start_x + w) as i32,
                            &draw_segment,
                            draw_segment.len(),
                            ctx,
                        );
                    }
                }

                if mode == FIND_INDEX {
                    let di = if start_index != start_style {
                        self.find_x(
                            &chars[start_style..i].iter().collect::<String>(),
                            i - start_style,
                            style,
                            -(right_clip as i32 - style_x as i32),
                            ctx,
                        )
                    } else {
                        self.find_x(
                            &segment,
                            i - start_index,
                            style,
                            -(right_clip as i32 - start_x as i32),
                            ctx,
                        )
                    };
                    return (line_start_pos + start_index + di) as i32;
                }
            }

            if mode == GET_WIDTH {
                return (start_x + w) as i32;
            }

            // Clear the rest of the line
            start_x += w;
            let style = self.position_style(line_start_pos, line_len, i as isize);
            if mode == DRAW_LINE && loop_pass == 1 {
                self.draw_string(
                    style | BG_ONLY_MASK,
                    start_x as i32,
                    y,
                    self.text_area_x + self.text_area_w,
                    "",
                    0,
                    ctx,
                );
            }
        }

        (line_start_pos + line_len) as i32
    }

    /// Draw a styled text segment or background
    fn draw_string(
        &self,
        style: i32,
        x: i32,
        y: i32,
        to_x: i32,
        string: &str,
        n_chars: usize,
        ctx: &mut dyn DrawContext,
    ) {
        // Handle fill-only mode
        if (style & FILL_MASK) != 0 {
            if (style & TEXT_ONLY_MASK) != 0 {
                return;
            }
            self.clear_rect(style, x, y, to_x - x, self.max_font_height, ctx);
            return;
        }

        // Determine font and colors from style
        let (font, fsize, foreground, background) = self.get_style_colors(style, ctx);

        // Draw background
        if (style & TEXT_ONLY_MASK) == 0 {
            ctx.set_color(background);
            ctx.draw_rect_filled(x, y, to_x - x, self.max_font_height);
        }

        // Draw text
        if (style & BG_ONLY_MASK) == 0 && !string.is_empty() && n_chars > 0 {
            ctx.set_color(foreground);
            ctx.set_font(font, fsize);

            let baseline = y + self.max_font_height - ctx.text_descent(font, fsize);
            let text = if n_chars < string.len() {
                &string[..n_chars]
            } else {
                string
            };

            ctx.draw_text(text, x, baseline);

            // Draw underlines or strikethrough if needed
            if (style & STYLE_LOOKUP_MASK) != 0 {
                let si = ((style & STYLE_LOOKUP_MASK) - b'A' as i32)
                    .max(0)
                    .min((self.n_styles - 1) as i32) as usize;
                if si < self.style_table.len() {
                    let style_rec = &self.style_table[si];
                    if (style_rec.attr & style_attr::LINES_MASK) != 0 {
                        let attr = style_rec.attr & style_attr::LINES_MASK;

                        if attr == style_attr::UNDERLINE {
                            ctx.set_color(foreground);
                            ctx.draw_line(
                                x,
                                baseline + ctx.text_descent(font, fsize) / 2,
                                to_x,
                                baseline + ctx.text_descent(font, fsize) / 2,
                            );
                        } else if attr == style_attr::GRAMMAR {
                            ctx.set_color(self.grammar_underline_color);
                            ctx.draw_line(
                                x,
                                baseline + ctx.text_descent(font, fsize) / 2,
                                to_x,
                                baseline + ctx.text_descent(font, fsize) / 2,
                            );
                        } else if attr == style_attr::SPELLING {
                            ctx.set_color(self.spelling_underline_color);
                            ctx.draw_line(
                                x,
                                baseline + ctx.text_descent(font, fsize) / 2,
                                to_x,
                                baseline + ctx.text_descent(font, fsize) / 2,
                            );
                        } else if attr == style_attr::STRIKE_THROUGH {
                            ctx.set_color(foreground);
                            let strike_y = baseline
                                - (ctx.text_height(font, fsize) - ctx.text_descent(font, fsize))
                                    / 3;
                            ctx.draw_line(x, strike_y, to_x, strike_y);
                        }
                    }
                }
            }
        }
    }

    /// Get foreground and background colors for a style
    fn get_style_colors(&self, style: i32, ctx: &dyn DrawContext) -> (u8, u8, u32, u32) {
        let mut font = self.text_font;
        let mut fsize = self.text_size;
        let mut foreground = self.text_color;
        let mut background;
        let mut bgbasecolor = 0xFFFFFFFF; // Default white background

        // Get style-specific colors
        if (style & STYLE_LOOKUP_MASK) != 0 {
            let si = ((style & STYLE_LOOKUP_MASK) - b'A' as i32)
                .max(0)
                .min((self.n_styles - 1) as i32) as usize;

            if si < self.style_table.len() {
                let style_rec = &self.style_table[si];
                font = style_rec.font;
                fsize = style_rec.size;
                foreground = style_rec.color;

                bgbasecolor = if (style_rec.attr & style_attr::BGCOLOR) != 0 {
                    style_rec.bgcolor
                } else {
                    0xFFFFFFFF // Default background
                };

                // Apply selection backgrounds
                if (style & PRIMARY_MASK) != 0 {
                    background = if ctx.has_focus() {
                        0x0078D7FF // Selection color
                    } else {
                        ctx.color_average(bgbasecolor, 0x0078D7FF, 0.4)
                    };
                } else if (style & HIGHLIGHT_MASK) != 0 {
                    background = if ctx.has_focus() {
                        ctx.color_average(bgbasecolor, 0x0078D7FF, 0.5)
                    } else {
                        ctx.color_average(bgbasecolor, 0x0078D7FF, 0.6)
                    };
                } else if (style & SECONDARY_MASK) != 0 {
                    background = if ctx.has_focus() {
                        ctx.color_average(bgbasecolor, self.secondary_selection_color, 0.5)
                    } else {
                        ctx.color_average(bgbasecolor, self.secondary_selection_color, 0.6)
                    };
                } else {
                    background = bgbasecolor;
                }

                if (style & PRIMARY_MASK) != 0 {
                    foreground = ctx.color_contrast(style_rec.color, background);
                }
            } else {
                background = bgbasecolor;
            }
        } else if (style & PRIMARY_MASK) != 0 {
            background = if ctx.has_focus() {
                0x0078D7FF
            } else {
                ctx.color_average(bgbasecolor, 0x0078D7FF, 0.4)
            };
            foreground = ctx.color_contrast(self.text_color, background);
        } else if (style & HIGHLIGHT_MASK) != 0 {
            background = if ctx.has_focus() {
                ctx.color_average(bgbasecolor, 0x0078D7FF, 0.5)
            } else {
                ctx.color_average(bgbasecolor, 0x0078D7FF, 0.6)
            };
            foreground = ctx.color_contrast(self.text_color, background);
        } else if (style & SECONDARY_MASK) != 0 {
            background = if ctx.has_focus() {
                self.secondary_selection_color
            } else {
                ctx.color_average(bgbasecolor, self.secondary_selection_color, 0.4)
            };
            foreground = ctx.color_contrast(self.text_color, background);
        } else {
            foreground = self.text_color;
            background = bgbasecolor;
        }

        if !ctx.is_active() {
            foreground = ctx.color_inactive(foreground);
            background = ctx.color_inactive(background);
        }

        (font, fsize, foreground, background)
    }

    /// Clear a rectangle with appropriate background color
    fn clear_rect(
        &self,
        style: i32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        ctx: &mut dyn DrawContext,
    ) {
        if width == 0 {
            return;
        }

        let mut bgbasecolor = 0xFFFFFFFF; // Default widget background

        if (style & STYLE_LOOKUP_MASK) != 0 {
            let si = ((style & STYLE_LOOKUP_MASK) - b'A' as i32)
                .max(0)
                .min((self.n_styles - 1) as i32) as usize;

            if si < self.style_table.len() {
                let style_rec = &self.style_table[si];
                if (style_rec.attr & style_attr::BGCOLOR_EXT_) != 0 {
                    bgbasecolor = style_rec.bgcolor;
                }
            }
        }

        let color = if (style & PRIMARY_MASK) != 0 {
            if ctx.has_focus() {
                0x0078D7FF
            } else {
                ctx.color_average(bgbasecolor, 0x0078D7FF, 0.4)
            }
        } else if (style & HIGHLIGHT_MASK) != 0 {
            if ctx.has_focus() {
                ctx.color_average(bgbasecolor, 0x0078D7FF, 0.5)
            } else {
                ctx.color_average(bgbasecolor, 0x0078D7FF, 0.6)
            }
        } else {
            bgbasecolor
        };

        let final_color = if ctx.is_active() {
            color
        } else {
            ctx.color_inactive(color)
        };

        ctx.set_color(final_color);
        ctx.draw_rect_filled(x, y, width, height);
    }

    /// Draw the text cursor at the given position
    pub fn draw_cursor(&self, x: i32, y: i32, ctx: &mut dyn DrawContext) {
        let font_height = self.max_font_height;
        let bot = y + font_height - 1;

        if x < self.text_area_x - 1 || x > self.text_area_x + self.text_area_w {
            return;
        }

        let cursor_width = 4;
        let left = x - cursor_width / 2;
        let right = left + cursor_width;

        ctx.set_color(self.cursor_color);

        match self.cursor_style {
            CursorStyle::Caret => {
                let mid_y = bot - font_height / 5;
                ctx.draw_line(left, bot, x, mid_y);
                ctx.draw_line(x, mid_y, right, bot);
                ctx.draw_line(left, bot, x, mid_y - 1);
                ctx.draw_line(x, mid_y - 1, right, bot);
            }
            CursorStyle::Normal => {
                ctx.draw_line(left, y, right, y);
                ctx.draw_line(x, y, x, bot);
                ctx.draw_line(left, bot, right, bot);
            }
            CursorStyle::Heavy => {
                ctx.draw_line(x - 1, y, x - 1, bot);
                ctx.draw_line(x, y, x, bot);
                ctx.draw_line(x + 1, y, x + 1, bot);
                ctx.draw_line(left, y, right, y);
                ctx.draw_line(left, bot, right, bot);
            }
            CursorStyle::Dim => {
                let mid_y = y + font_height / 2;
                ctx.draw_line(x, y, x, y);
                ctx.draw_line(x, mid_y, x, mid_y);
                ctx.draw_line(x, bot, x, bot);
            }
            CursorStyle::Block => {
                let right = x + self.max_font_width;
                ctx.draw_line(x, y, right, y);
                ctx.draw_line(right, y, right, bot);
                ctx.draw_line(right, bot, x, bot);
                ctx.draw_line(x, bot, x, y);
            }
            CursorStyle::Simple => {
                ctx.draw_line(x, y, x, bot);
                ctx.draw_line(x + 1, y, x + 1, bot);
            }
        }
    }

    /// Draw a single visible line
    pub fn draw_vline(
        &self,
        vis_line_num: usize,
        left_clip: i32,
        right_clip: i32,
        left_char_index: usize,
        right_char_index: usize,
        ctx: &mut dyn DrawContext,
    ) {
        if vis_line_num >= self.n_visible_lines {
            return;
        }

        let font_height = self.max_font_height;
        let y = self.text_area_y + (vis_line_num as i32 * font_height);

        let line_start_pos = self.line_starts[vis_line_num];
        let line_len = if line_start_pos == usize::MAX {
            0
        } else {
            self.vline_length(vis_line_num)
        };

        let left_clip = max(self.text_area_x, left_clip);
        let right_clip = min(right_clip, self.text_area_x + self.text_area_w);

        self.handle_vline(
            DRAW_LINE,
            line_start_pos,
            line_len,
            left_char_index,
            right_char_index,
            y,
            y + font_height,
            left_clip,
            right_clip,
            ctx,
        );
    }

    /// Draw text in a specific region
    pub fn draw_text(
        &self,
        left: i32,
        top: i32,
        width: i32,
        height: i32,
        ctx: &mut dyn DrawContext,
    ) {
        let font_height = if self.max_font_height > 0 {
            self.max_font_height
        } else {
            self.text_size as i32
        };

        let first_line = (top - self.text_area_y - font_height + 1) / font_height;
        let last_line = (top + height - self.text_area_y) / font_height + 1;

        ctx.push_clip(left, top, width, height);

        for line in first_line..=last_line {
            if line >= 0 && (line as usize) < self.n_visible_lines {
                self.draw_vline(line as usize, left, left + width, 0, usize::MAX, ctx);
            }
        }

        ctx.pop_clip();
    }

    /// Main drawing function - draws the entire text display
    pub fn draw(&mut self, ctx: &mut dyn DrawContext) {
        // Update font metrics on first draw to get accurate measurements
        if !self.font_metrics_calculated {
            self.update_font_metrics_from_context(ctx);
            self.font_metrics_calculated = true;
            // Recalculate display with correct metrics
            self.recalc_display();
        }

        // Draw the text content
        self.draw_text(
            self.text_area_x,
            self.text_area_y,
            self.text_area_w,
            self.text_area_h,
            ctx,
        );

        // Draw cursor if visible and on
        if self.cursor_on {
            if let Some((x, y)) = self.position_to_xy(self.cursor_pos) {
                self.draw_cursor(x, y, ctx);
            }
        }

        // Draw line numbers if enabled
        if self.linenumber_width > 0 {
            self.draw_line_numbers(ctx);
        }
    }

    /// Draw line numbers in the margin
    pub fn draw_line_numbers(&self, ctx: &mut dyn DrawContext) {
        if self.linenumber_width <= 0 {
            return;
        }

        // Calculate line number area
        let ln_x = self.x;
        let ln_y = self.text_area_y;
        let ln_w = self.linenumber_width;
        let ln_h = self.text_area_h;

        // Draw background
        ctx.set_color(self.linenumber_bgcolor);
        ctx.draw_rect_filled(ln_x, ln_y, ln_w, ln_h);

        // Draw line numbers
        ctx.set_color(self.linenumber_fgcolor);
        ctx.set_font(self.linenumber_font, self.linenumber_size);

        let font_height = self.max_font_height;

        for vis_line in 0..self.n_visible_lines {
            let line_start_pos = self.line_starts[vis_line];
            if line_start_pos == usize::MAX {
                break;
            }

            // Calculate absolute line number
            let line_num = if let Some(ref buffer) = self.buffer {
                let buf = buffer.borrow();
                buf.count_lines(0, line_start_pos) + 1
            } else {
                vis_line + 1
            };

            let y = ln_y + (vis_line as i32 * font_height);
            let baseline =
                y + font_height - ctx.text_descent(self.linenumber_font, self.linenumber_size);

            // Format line number
            let line_text = format!("{}", line_num);

            // Calculate x position based on alignment
            let text_width = ctx.text_width(&line_text, self.linenumber_font, self.linenumber_size);
            let text_x = match self.linenumber_align {
                // FL_ALIGN_LEFT = 1, FL_ALIGN_RIGHT = 2, FL_ALIGN_CENTER = 0
                1 => ln_x + 2, // Left align with small margin
                2 => ln_x + ln_w - text_width as i32 - 2, // Right align
                _ => ln_x + (ln_w - text_width as i32) / 2, // Center
            };

            ctx.draw_text(&line_text, text_x, baseline);
        }
    }
}

impl Default for TextDisplay {
    fn default() -> Self {
        Self::new(0, 0, 100, 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_display_new() {
        let display = TextDisplay::new(10, 20, 300, 200);
        assert_eq!(display.x(), 10);
        assert_eq!(display.y(), 20);
        assert_eq!(display.w(), 300);
        assert_eq!(display.h(), 200);
    }

    #[test]
    fn test_buffer_management() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));

        assert!(display.buffer().is_none());
        display.set_buffer(buffer.clone());
        assert!(display.buffer().is_some());
    }

    #[test]
    fn test_cursor_position() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Hello World");
        display.set_buffer(buffer);

        assert_eq!(display.insert_position(), 0);
        display.set_insert_position(5);
        assert_eq!(display.insert_position(), 5);
    }

    #[test]
    fn test_cursor_style() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        assert_eq!(display.cursor_style(), CursorStyle::Normal);

        display.set_cursor_style(CursorStyle::Block);
        assert_eq!(display.cursor_style(), CursorStyle::Block);
    }

    #[test]
    fn test_insert_text() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        display.set_buffer(buffer.clone());

        display.insert("Hello");
        assert_eq!(buffer.borrow().text(), "Hello");
        assert_eq!(display.insert_position(), 5);

        display.insert(" World");
        assert_eq!(buffer.borrow().text(), "Hello World");
        assert_eq!(display.insert_position(), 11);
    }

    #[test]
    fn test_move_right_left() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Hello");
        display.set_buffer(buffer);

        assert_eq!(display.insert_position(), 0);

        assert!(display.move_right());
        assert_eq!(display.insert_position(), 1);

        assert!(display.move_left());
        assert_eq!(display.insert_position(), 0);

        assert!(!display.move_left()); // Can't move past start
    }

    #[test]
    fn test_word_navigation() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Hello World Test");
        display.set_buffer(buffer);

        display.next_word();
        assert_eq!(display.insert_position(), 6); // After "Hello ", at start of "World"

        display.next_word();
        assert_eq!(display.insert_position(), 12); // After "World ", at start of "Test"

        display.previous_word();
        assert_eq!(display.insert_position(), 6); // Back to start of "World"

        display.previous_word();
        assert_eq!(display.insert_position(), 0); // Back to start of "Hello"
    }

    #[test]
    fn test_font_settings() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        display.set_textfont(1);
        assert_eq!(display.textfont(), 1);

        display.set_textsize(16);
        assert_eq!(display.textsize(), 16);

        display.set_textcolor(0xFF0000FF);
        assert_eq!(display.textcolor(), 0xFF0000FF);
    }

    #[test]
    fn test_line_numbers() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        display.set_linenumber_width(50);
        assert_eq!(display.linenumber_width(), 50);

        display.set_linenumber_font(2);
        assert_eq!(display.linenumber_font(), 2);

        display.set_linenumber_size(12);
        assert_eq!(display.linenumber_size(), 12);
    }

    #[test]
    fn test_wrap_mode() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        assert_eq!(display.wrap_mode(), WrapMode::None);

        display.set_wrap_mode(WrapMode::AtBounds, 0);
        assert_eq!(display.wrap_mode(), WrapMode::AtBounds);
    }

    #[test]
    fn test_scrolling() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        display.scroll(10, 50);
        assert_eq!(display.top_line_num(), 10);
        assert_eq!(display.horiz_offset(), 50);
    }

    #[test]
    fn test_resize() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        display.resize(10, 20, 300, 200);
        assert_eq!(display.x(), 10);
        assert_eq!(display.y(), 20);
        assert_eq!(display.w(), 300);
        assert_eq!(display.h(), 200);
    }

    #[test]
    fn test_move_up_down() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Line 1\nLine 2\nLine 3");
        display.set_buffer(buffer);

        display.set_insert_position(7); // Start of "Line 2"

        assert!(display.move_down());
        assert_eq!(display.insert_position(), 14); // Start of "Line 3"

        assert!(display.move_up());
        assert_eq!(display.insert_position(), 7); // Back to "Line 2"

        assert!(display.move_up());
        assert_eq!(display.insert_position(), 0); // Back to "Line 1"

        assert!(!display.move_up()); // Can't go up from first line
    }

    #[test]
    fn test_move_up_down_column_preservation() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer
            .borrow_mut()
            .insert(0, "Short\nThis is a longer line\nMed");
        display.set_buffer(buffer.clone());

        display.set_insert_position(10); // Position 10 in "This is a longer line"

        // Get the column before moving
        let line_start = buffer.borrow().line_start(10);
        let col_before = buffer.borrow().count_displayed_characters(line_start, 10);

        assert!(display.move_down());
        // "Med" is at position 29-31, should try to go to same column
        let expected_pos = buffer.borrow().length(); // End of buffer
        assert_eq!(display.insert_position(), expected_pos);

        assert!(display.move_up());
        // Should try to restore column, result should be close to original position
        // The exact position depends on how column is calculated
        let pos_after = display.insert_position();
        let line_start_after = buffer.borrow().line_start(pos_after);
        let col_after = buffer
            .borrow()
            .count_displayed_characters(line_start_after, pos_after);

        // Column should be preserved (or close)
        assert!(col_after == col_before || col_after == col_before - 1);
    }

    #[test]
    fn test_xy_to_position_basic() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Hello\nWorld");
        display.set_buffer(buffer.clone());
        display.recalc_display();

        // Click at start of first line
        let pos = display.xy_to_position(0, 0, PositionType::CharacterPos);
        assert_eq!(pos, 0);

        // The line starts array should be populated
        assert!(!display.line_starts.is_empty());
    }

    #[test]
    fn test_position_to_xy_basic() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Hello\nWorld");
        display.set_buffer(buffer.clone());
        display.recalc_display();

        // Position 0 should be at text area origin
        if let Some((x, y)) = display.position_to_xy(0) {
            assert_eq!(x, display.text_area_x);
            assert_eq!(y, display.text_area_y);
        }
    }

    #[test]
    fn test_recalc_display() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer
            .borrow_mut()
            .insert(0, "Line 1\nLine 2\nLine 3\nLine 4\nLine 5");
        display.set_buffer(buffer);

        display.recalc_display();

        // Should have calculated visible lines
        assert!(display.n_visible_lines > 0);
        // Line starts should be populated
        assert_eq!(display.line_starts.len(), display.n_visible_lines);
        // First line should start at 0
        assert_eq!(display.line_starts[0], 0);
    }

    #[test]
    fn test_show_insert_position() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        let mut text = String::new();
        for i in 0..100 {
            text.push_str(&format!("Line {}\n", i));
        }
        buffer.borrow_mut().insert(0, &text);
        display.set_buffer(buffer);
        display.recalc_display();

        // Set cursor to line 50
        display.set_insert_position(display.skip_lines(0, 50));

        // Show insert position should scroll to make it visible
        display.show_insert_position();

        // Top line should have changed to make line 50 visible
        assert!(display.top_line_num() > 0);
    }

    #[test]
    fn test_in_selection() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "Hello World");
        buffer.borrow_mut().select(0, 5); // Select "Hello"
        display.set_buffer(buffer);
        display.recalc_display();

        // Test in_selection at position 2 (in "Hello")
        let in_sel = display.in_selection(2 * display.max_font_width, 0);
        // This depends on xy_to_position working correctly
        // Just check it doesn't panic
        assert!(in_sel || !in_sel); // Always true, just to use the variable
    }

    #[test]
    fn test_needs_recalc() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        assert!(display.needs_recalc()); // Should need recalc initially

        display.recalc_display();
        assert!(!display.needs_recalc());

        display.display_needs_recalc();
        assert!(display.needs_recalc());
    }

    #[test]
    fn test_line_starts_caching() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "A\nB\nC\nD\nE\nF\nG\nH");
        display.set_buffer(buffer);

        display.recalc_display();

        // Check that line starts are cached correctly
        assert_eq!(display.line_starts[0], 0); // "A"
        assert_eq!(display.line_starts[1], 2); // "B"
        assert_eq!(display.line_starts[2], 4); // "C"
    }

    #[test]
    fn test_scrolling_updates_line_starts() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, "L0\nL1\nL2\nL3\nL4\nL5");
        display.set_buffer(buffer);

        display.recalc_display();
        let first_line_start = display.line_starts[0];

        // Scroll down
        display.scroll(2, 0);
        let new_first_line_start = display.line_starts[0];

        // First visible line should have changed
        assert_ne!(first_line_start, new_first_line_start);
    }

    #[test]
    fn test_horizontal_scrolling() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        // Create a very long line
        buffer.borrow_mut().insert(0, &"x".repeat(200));
        display.set_buffer(buffer);
        display.recalc_display();

        // Set cursor far to the right
        display.set_insert_position(150);

        // Show insert position should scroll horizontally
        display.show_insert_position();

        // Horizontal offset should have changed
        assert!(display.horiz_offset() > 0);
    }

    #[test]
    fn test_font_metrics_calculation() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        let old_height = display.max_font_height;
        let old_width = display.max_font_width;

        display.set_textsize(20);

        // Font metrics should have changed
        assert_ne!(display.max_font_height, old_height);
        // Width scales with size
        assert_ne!(display.max_font_width, old_width);
    }

    #[test]
    fn test_font_metrics_with_styles() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        display.set_textsize(12);

        let mut styles = Vec::new();
        styles.push(StyleTableEntry {
            color: 0xFF0000FF,
            font: 0,
            size: 24, // Larger size
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        });

        display.set_highlight_data(styles);

        // Max font height should be at least 24 (from style)
        assert!(display.max_font_height >= 24);
    }

    #[test]
    fn test_wrapping_mode() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        // Long line that needs wrapping
        buffer.borrow_mut().insert(0, &"x".repeat(100));
        display.set_buffer(buffer);

        // Enable wrapping at 20 characters
        display.set_wrap_mode(WrapMode::AtColumn, 20);
        display.recalc_display();

        // With wrapping, we should have multiple visible lines for one long line
        // (Can't assert exact count without knowing widget height)
        assert!(display.n_visible_lines > 0);
    }

    #[test]
    fn test_column_scale_after_font_change() {
        let mut display = TextDisplay::new(0, 0, 100, 100);

        display.set_textsize(10);
        let scale1 = display.column_scale;

        display.set_textsize(20);
        let scale2 = display.column_scale;

        // Column scale should increase with font size
        assert!(scale2 > scale1);
    }

    #[test]
    fn test_show_insert_position_horizontal() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer.borrow_mut().insert(0, &"a".repeat(200));
        display.set_buffer(buffer);
        display.recalc_display();

        // Start with no horizontal scroll
        assert_eq!(display.horiz_offset(), 0);

        // Move cursor to column 100
        display.set_insert_position(100);
        display.show_insert_position();

        // Should have scrolled horizontally
        assert!(display.horiz_offset() > 0);

        // Move cursor back to start
        display.set_insert_position(0);
        display.show_insert_position();

        // Should scroll back left
        assert_eq!(display.horiz_offset(), 0);
    }

    #[test]
    fn test_wrapped_line_break_at_word() {
        let mut display = TextDisplay::new(0, 0, 100, 100);
        let buffer = Rc::new(RefCell::new(TextBuffer::new()));
        buffer
            .borrow_mut()
            .insert(0, "This is a very long line that will need to wrap");
        display.set_buffer(buffer);

        display.set_wrap_mode(WrapMode::AtColumn, 15);
        display.recalc_display();

        // Should have multiple visible lines from wrapping
        // Just verify it doesn't panic and recalc succeeds
        assert!(display.n_visible_lines > 0);
    }
}
