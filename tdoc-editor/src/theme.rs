use crate::draw_context::{FontStyle, FontType};

#[derive(Clone, Copy, Debug)]
pub struct FontSettings {
    pub font_type: FontType,
    pub font_style: FontStyle,
    pub font_size: u8,
    pub font_color: u32,
    pub background_color: Option<u32>,
}

pub struct Theme {
    pub background_color: u32,
    pub selection_color: u32,
    pub cursor_color: u32,

    pub quote_bar_color: u32,
    pub quote_bar_width: u32,

    pub table_border_color: u32,
    pub table_header_background: u32,

    pub link_color: u32,
    pub link_hover_background: u32,
    pub link_hover_color: u32,

    pub highlight_color: u32,
    pub search_highlight_color: u32,
    pub search_current_highlight_color: u32,

    pub padding_vertical: i32,
    pub padding_horizontal: i32,

    pub line_height: i32,

    /// Extra space above a heading (except the first block) and below every
    /// heading. Pixel values for the GUI; a cell backend sets these to 0.
    pub heading_top_margin: i32,
    pub heading_bottom_margin: i32,

    /// Trailing space after blocks, by kind. Pixel values for the GUI; a cell
    /// backend sets these small (or 0) so the document isn't sparse in a
    /// character grid. `code_block_padding` is the inset above/below code text.
    pub paragraph_spacing: i32,
    pub list_item_spacing: i32,
    pub quote_spacing: i32,
    pub code_block_padding: i32,

    /// Horizontal indent per quote nesting level, and the x-offset of the quote
    /// bar within that indent. Pixel values for the GUI; small for a cell grid.
    pub quote_indent: i32,
    pub quote_bar_offset: i32,

    /// Padding inside table cells (horizontal and vertical). Pixel values for
    /// the GUI; a cell backend uses tight values so rows/columns aren't huge.
    pub table_cell_padding_h: i32,
    pub table_cell_padding_v: i32,

    /// Whether underline/strikethrough are drawn as separate lines (pixel
    /// backends) or folded into the glyph attributes by the backend (cell
    /// backends set this `false` so decorations don't land on the wrong row).
    pub text_decoration_lines: bool,

    /// Center level-1 headings within the content column (classic Pure styled
    /// its document title this way). Off by default so other backends keep
    /// left-aligned headings.
    pub center_level1_headings: bool,

    /// Horizontal inset of code-block text past the block's left edge. Pixel
    /// value for the GUI; a cell backend uses a small value (a 10-pixel inset is
    /// 10 whole columns in a terminal).
    pub code_block_indent: i32,

    /// Render checklist markers as text (`[x] ` / `[ ] `) instead of a drawn
    /// square. Off by default (GUI draws the box); a cell backend turns this on
    /// so checkboxes read as the classic bracketed markers in one column run.
    pub checkbox_text: bool,

    /// Draw a rule (`=` for H2, `-` for H3) under level-2/3 headings. Off by
    /// default (GUI distinguishes headings by font size); a cell backend, which
    /// has no font sizes, turns this on so heading levels stay distinguishable.
    pub heading_underline: bool,

    /// Draw the quote bar as a literal `|` glyph (classic Pure used the ASCII
    /// pipe) instead of a drawn vertical line. Off by default so the GUI keeps
    /// its solid bar; a cell backend turns this on.
    pub quote_bar_as_text: bool,

    /// Color used for decorative rules drawn by the engine — heading underlines
    /// and code-block fences. Only consulted when `heading_underline` /
    /// `code_block_fence` are on (i.e. cell backends); defaults to the plain
    /// text color so it is harmless for the GUI.
    pub structural_color: u32,

    /// Draw a horizontal rule above and below code blocks (classic Pure fenced
    /// code this way in the terminal). Off by default; the GUI tints code text
    /// instead. The fence rows live in `code_block_padding`, so a backend that
    /// turns this on must keep that padding >= 1.
    pub code_block_fence: bool,

    /// When wrapping, ignore a word's trailing whitespace in the fit decision:
    /// a word whose glyphs fit stays on the line even if its trailing space
    /// would spill past the edge (the space is invisible there). Classic Pure
    /// did this — it held the inter-word space pending and dropped it at the
    /// break. Off by default so pixel backends wrap on the full token width.
    pub wrap_defer_trailing_space: bool,

    /// Columns to subtract from the wrappable content width, beyond the
    /// horizontal padding. Classic Pure reserved one trailing column so the
    /// end-of-line cursor stays inside the text area (its `wrap_limit` was
    /// `wrap_width - 1`); a cell backend sets this to 1 to wrap at the same
    /// point. The GUI's caret needs no such column, so this defaults to 0 and
    /// leaves pixel layout unchanged.
    pub wrap_width_reduction: i32,

    /// Comfort margin, in `line_height` units, kept between the cursor and the
    /// top/bottom edge of the viewport when auto-scrolling to follow the cursor
    /// (see [`StructuredRichDisplay::ensure_cursor_visible`]). The GUI uses a
    /// small pixel value; a cell backend sets this to 1 line so the document
    /// scrolls only once the cursor reaches the very edge, the way classic Pure
    /// did.
    pub cursor_scroll_margin: i32,

    /// Use classic-Pure block spacing: instead of each block adding its own
    /// trailing space (the additive GUI model), the gap before a block is
    /// `max(1, previous block's bottom margin, this block's top margin)` with
    /// heading margins H1=(3,3) H2=(3,2) H3=(2,1) and every other block (0,0).
    /// Off by default so the GUI keeps its additive pixel spacing. A cell
    /// backend turns this on and zeroes the per-block spacing fields so the two
    /// models don't both apply.
    pub classic_block_spacing: bool,

    pub header_level_1: FontSettings,
    pub header_level_2: FontSettings,
    pub header_level_3: FontSettings,
    pub plain_text: FontSettings,
    pub quote_text: FontSettings,
    pub code_text: FontSettings,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            background_color: 0xFFFFF5FF,               // Off-white background
            selection_color: 0xB4D5FEFF,                // Light blue selection color
            cursor_color: 0x000000FF,                   // Black cursor
            quote_bar_color: 0xCCCCCCFF,                // Light gray quote bar
            quote_bar_width: 4,                         // Width of the quote bar
            table_border_color: 0xBBBBBBFF,             // Gray table grid lines
            table_header_background: 0xEEEEE5FF,        // Subtle header row fill
            link_color: 0x0000EEFF,                     // Standard blue link color
            link_hover_background: 0xDDDDDDFF,          // Light gray hover background
            link_hover_color: 0x0000AAFF,               // Darker blue link color
            highlight_color: 0xFFFF00FF,                // Yellow highlight color
            search_highlight_color: 0xFFE4B5FF,         // Light orange for search matches
            search_current_highlight_color: 0xFFA500FF, // Orange for current match

            padding_vertical: 10,
            padding_horizontal: 25,
            line_height: 17,
            heading_top_margin: 15,
            heading_bottom_margin: 10,
            paragraph_spacing: 5,
            list_item_spacing: 2,
            quote_spacing: 5,
            code_block_padding: 5,
            quote_indent: 20,
            quote_bar_offset: 12,
            table_cell_padding_h: 6,
            table_cell_padding_v: 3,
            text_decoration_lines: true,
            center_level1_headings: false,
            code_block_indent: 10,
            checkbox_text: false,
            heading_underline: false,
            quote_bar_as_text: false,
            structural_color: 0x000000FF,
            code_block_fence: false,
            wrap_defer_trailing_space: false,
            wrap_width_reduction: 0,
            cursor_scroll_margin: 8,
            classic_block_spacing: false,
            header_level_1: FontSettings {
                font_type: FontType::Heading,
                font_style: FontStyle::Bold,
                font_size: 24,
                font_color: 0x000000FF,
                background_color: None,
            },
            header_level_2: FontSettings {
                font_type: FontType::Heading,
                font_style: FontStyle::Bold,
                font_size: 20,
                font_color: 0x000000FF,
                background_color: None,
            },
            header_level_3: FontSettings {
                font_type: FontType::Heading,
                font_style: FontStyle::Bold,
                font_size: 18,
                font_color: 0x000000FF,
                background_color: None,
            },
            plain_text: FontSettings {
                font_type: FontType::Content,
                font_style: FontStyle::Regular,
                font_size: 14,
                font_color: 0x000000FF,
                background_color: None,
            },
            quote_text: FontSettings {
                font_type: FontType::Content,
                font_style: FontStyle::Italic,
                font_size: 14,
                font_color: 0x555555FF,
                background_color: None,
            },
            code_text: FontSettings {
                font_type: FontType::Code,
                font_style: FontStyle::Regular,
                font_size: 14,
                font_color: 0x0064C8FF,
                background_color: None,
            },
        }
    }
}
