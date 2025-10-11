// Rich Text Display - AST-based rendering
// This is an alternative implementation of text display that uses a parsed AST
// instead of a flat text buffer with parallel style buffer

use crate::markdown_ast::{ASTNode, Document, NodeType};
use crate::markdown_parser::parse_markdown;
use crate::text_display::{DrawContext, StyleTableEntry};

/// Layout information for a rendered line
#[derive(Debug, Clone)]
struct LayoutLine {
    /// Y position of the line's baseline
    y: i32,
    /// Height of the line
    height: i32,
    /// Node ID that this line belongs to
    node_id: usize,
    /// Character start position in source
    char_start: usize,
    /// Character end position in source
    char_end: usize,
    /// Visual elements on this line (text runs with styling)
    runs: Vec<TextRun>,
}

/// A run of text with consistent styling
#[derive(Debug, Clone)]
struct TextRun {
    /// Text content
    text: String,
    /// X position
    x: i32,
    /// Style index (from style table)
    style_idx: u8,
    /// Node ID this run belongs to
    node_id: usize,
    /// Character range in source
    char_range: (usize, usize),
}

/// Rich Text Display Widget
/// Renders markdown from a parsed AST instead of flat text buffer
pub struct RichTextDisplay {
    // Position and size
    x: i32,
    y: i32,
    w: i32,
    h: i32,

    // Document and AST
    document: Option<Document>,

    // Layout cache
    layout_lines: Vec<LayoutLine>,
    layout_valid: bool,

    // Scrolling
    scroll_offset: i32, // Y offset in pixels
    visible_height: i32,

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

    // Link hover state
    hovered_node_id: Option<usize>,
}

impl RichTextDisplay {
    /// Create a new RichTextDisplay
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        RichTextDisplay {
            x,
            y,
            w,
            h,
            document: None,
            layout_lines: Vec::new(),
            layout_valid: false,
            scroll_offset: 0,
            visible_height: h,
            style_table: Vec::new(),
            text_font: 0,
            text_size: 14,
            text_color: 0x000000FF,
            background_color: 0xFFFFF5FF,
            padding_top: 10,
            padding_bottom: 10,
            padding_left: 25,
            padding_right: 25,
            line_height: 17, // ~1.2 * 14
            hovered_node_id: None,
        }
    }

    /// Set markdown content (will parse and layout)
    pub fn set_markdown(&mut self, text: &str) {
        self.document = Some(parse_markdown(text));
        self.layout_valid = false;
    }

    /// Get the document
    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
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
            last_line.y + last_line.height
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
        self.visible_height = h;
        self.layout_valid = false;
    }

    /// Perform layout - compute line positions and text runs from AST
    fn layout(&mut self, ctx: &mut dyn DrawContext) {
        if self.layout_valid {
            return;
        }

        self.layout_lines.clear();

        let Some(ref doc) = self.document else {
            self.layout_valid = true;
            return;
        };

        // Calculate available width
        let content_width = self.w - self.padding_left - self.padding_right;
        let mut current_y = self.padding_top;

        // Clone children to avoid borrow checker issues
        let children = doc.root.children.clone();

        // Layout each child of the document root
        for child in &children {
            current_y = self.layout_node(child, self.padding_left, current_y, content_width, ctx);
        }

        self.layout_valid = true;
    }

    /// Layout a single AST node
    fn layout_node(
        &mut self,
        node: &ASTNode,
        x: i32,
        y: i32,
        width: i32,
        ctx: &mut dyn DrawContext,
    ) -> i32 {
        match &node.node_type {
            NodeType::Document => {
                let mut current_y = y;
                for child in &node.children {
                    current_y = self.layout_node(child, x, current_y, width, ctx);
                }
                current_y
            }

            NodeType::Paragraph => {
                let mut current_y = y;
                let mut current_x = x;
                let line_char_start = node.char_start;
                let mut line_char_end = node.char_start;

                // Layout inline content with multi-line support
                self.layout_inline_content_multiline(
                    node,
                    &mut current_x,
                    &mut current_y,
                    x,
                    width,
                    line_char_start,
                    &mut line_char_end,
                    ctx,
                );

                current_y + 5 // Add some spacing after paragraph
            }

            NodeType::Heading { level } => {
                // Calculate heading size
                let heading_size = match level {
                    1 => self.text_size + 6,
                    2 => self.text_size + 4,
                    3 => self.text_size + 2,
                    _ => self.text_size,
                };

                let heading_height = ((heading_size as f32) * 1.3) as i32;

                // Layout heading with word wrapping
                let text = node.flatten_text();
                let style_idx = self.get_style_idx_for_node(&node.node_type);

                // Get font for measuring
                let heading_font = if (style_idx as usize) < self.style_table.len() {
                    let style = &self.style_table[style_idx as usize];
                    (style.font, style.size)
                } else {
                    (1, heading_size) // Default to bold font
                };

                let mut current_y = y;
                let mut current_x = x;
                let mut line_runs: Vec<TextRun> = Vec::new();
                let mut line_start_y = current_y;

                // Word wrap the heading text
                for word in text.split_whitespace() {
                    let word_with_space = format!("{} ", word);
                    let word_width = ctx.text_width(&word_with_space, heading_font.0, heading_font.1) as i32;

                    // Check if word fits on current line
                    if current_x + word_width > x + width && current_x > x {
                        // Flush current line
                        if !line_runs.is_empty() {
                            self.layout_lines.push(LayoutLine {
                                y: line_start_y,
                                height: heading_height,
                                node_id: node.id,
                                char_start: node.char_start,
                                char_end: node.char_end,
                                runs: line_runs,
                            });
                            line_runs = Vec::new();
                        }

                        // Start new line
                        current_y += heading_height;
                        current_x = x;
                        line_start_y = current_y;
                    }

                    line_runs.push(TextRun {
                        text: word_with_space,
                        x: current_x,
                        style_idx,
                        node_id: node.id,
                        char_range: (node.char_start, node.char_end),
                    });

                    current_x += word_width;
                }

                // Flush final line
                if !line_runs.is_empty() {
                    self.layout_lines.push(LayoutLine {
                        y: line_start_y,
                        height: heading_height,
                        node_id: node.id,
                        char_start: node.char_start,
                        char_end: node.char_end,
                        runs: line_runs,
                    });
                    current_y += heading_height;
                }

                current_y + 10 // Extra spacing after headings
            }

            NodeType::CodeBlock { .. } => {
                let text = node.flatten_text();
                let lines: Vec<&str> = text.lines().collect();
                let style_idx = self.get_style_idx_for_node(&node.node_type);
                let mut current_y = y + 5; // Some padding above

                for line in lines {
                    let run = TextRun {
                        text: line.to_string(),
                        x: x + 10, // Indent code blocks
                        style_idx,
                        node_id: node.id,
                        char_range: (node.char_start, node.char_end),
                    };

                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        node_id: node.id,
                        char_start: node.char_start,
                        char_end: node.char_end,
                        runs: vec![run],
                    });

                    current_y += self.line_height;
                }

                current_y + 10 // Extra spacing after code
            }

            NodeType::BlockQuote => {
                let mut current_y = y + 5;
                let style_idx = self.get_style_idx_for_node(&node.node_type);

                for child in &node.children {
                    let text = child.flatten_text();
                    let run = TextRun {
                        text,
                        x: x + 20, // Indent quotes more
                        style_idx,
                        node_id: child.id,
                        char_range: (child.char_start, child.char_end),
                    };

                    self.layout_lines.push(LayoutLine {
                        y: current_y,
                        height: self.line_height,
                        node_id: child.id,
                        char_start: child.char_start,
                        char_end: child.char_end,
                        runs: vec![run],
                    });

                    current_y += self.line_height;
                }

                current_y + 5
            }

            NodeType::List { .. } => {
                let mut current_y = y;
                for child in &node.children {
                    current_y = self.layout_node(child, x + 20, current_y, width - 20, ctx);
                }
                current_y + 5
            }

            NodeType::ListItem => {
                let mut current_y = y;
                let mut current_x = x;
                let mut line_char_end = node.char_start;

                // Add bullet point as a separate line (will be combined with first line of content)
                let bullet_line_y = current_y;

                // Layout inline content with multi-line support (bullet will be added to first line)
                let before_layout_y = current_y;
                self.layout_inline_content_multiline_with_prefix(
                    node,
                    &mut current_x,
                    &mut current_y,
                    x,
                    width,
                    node.char_start,
                    &mut line_char_end,
                    Some("• "),
                    ctx,
                );

                // If no content was added, add just the bullet
                if current_y == before_layout_y {
                    self.layout_lines.push(LayoutLine {
                        y: bullet_line_y,
                        height: self.line_height,
                        node_id: node.id,
                        char_start: node.char_start,
                        char_end: node.char_start,
                        runs: vec![TextRun {
                            text: "• ".to_string(),
                            x: x - 15,
                            style_idx: 0,
                            node_id: node.id,
                            char_range: (node.char_start, node.char_start),
                        }],
                    });
                    current_y += self.line_height;
                }

                current_y + 2
            }

            _ => {
                // Default: treat as inline text
                let text = node.flatten_text();
                if !text.is_empty() {
                    let style_idx = self.get_style_idx_for_node(&node.node_type);
                    let run = TextRun {
                        text,
                        x,
                        style_idx,
                        node_id: node.id,
                        char_range: (node.char_start, node.char_end),
                    };

                    self.layout_lines.push(LayoutLine {
                        y,
                        height: self.line_height,
                        node_id: node.id,
                        char_start: node.char_start,
                        char_end: node.char_end,
                        runs: vec![run],
                    });
                }

                y + self.line_height
            }
        }
    }

    /// Layout inline content with automatic multi-line support
    /// Creates new LayoutLine entries when wrapping occurs
    fn layout_inline_content_multiline(
        &mut self,
        node: &ASTNode,
        current_x: &mut i32,
        current_y: &mut i32,
        start_x: i32,
        width: i32,
        line_char_start: usize,
        line_char_end: &mut usize,
        ctx: &mut dyn DrawContext,
    ) {
        self.layout_inline_content_multiline_with_prefix(
            node,
            current_x,
            current_y,
            start_x,
            width,
            line_char_start,
            line_char_end,
            None,
            ctx,
        );
    }

    /// Layout inline content with multi-line support and optional prefix (for list items)
    fn layout_inline_content_multiline_with_prefix(
        &mut self,
        node: &ASTNode,
        current_x: &mut i32,
        current_y: &mut i32,
        start_x: i32,
        width: i32,
        line_char_start: usize,
        line_char_end: &mut usize,
        prefix: Option<&str>,
        ctx: &mut dyn DrawContext,
    ) {
        let mut line_runs: Vec<TextRun> = Vec::new();
        let mut line_start_y = *current_y;
        let mut is_first_line = true;
        let mut completed_lines: Vec<LayoutLine> = Vec::new();

        // Add prefix to first line if provided
        if let Some(prefix_text) = prefix {
            line_runs.push(TextRun {
                text: prefix_text.to_string(),
                x: *current_x - 15,
                style_idx: 0,
                node_id: node.id,
                char_range: (line_char_start, line_char_start),
            });
        }

        // Helper closure to flush current line
        let mut flush_line = |runs: &mut Vec<TextRun>, y: i32, char_start: usize, char_end: usize, node_id: usize, height: i32, lines: &mut Vec<LayoutLine>| {
            if !runs.is_empty() {
                lines.push(LayoutLine {
                    y,
                    height,
                    node_id,
                    char_start,
                    char_end,
                    runs: runs.drain(..).collect(),
                });
            }
        };

        self.layout_inline_content_core(
            node,
            current_x,
            current_y,
            start_x,
            width,
            &mut line_runs,
            &mut line_start_y,
            &mut is_first_line,
            line_char_start,
            line_char_end,
            &mut flush_line,
            &mut completed_lines,
            ctx,
        );

        // Add all completed lines to layout_lines
        self.layout_lines.extend(completed_lines);

        // Flush any remaining runs
        if !line_runs.is_empty() {
            self.layout_lines.push(LayoutLine {
                y: line_start_y,
                height: self.line_height,
                node_id: node.id,
                char_start: line_char_start,
                char_end: *line_char_end,
                runs: line_runs,
            });
            *current_y += self.line_height;
        }
    }

    /// Core inline content layout logic with wrapping detection
    fn layout_inline_content_core<F>(
        &mut self,
        node: &ASTNode,
        current_x: &mut i32,
        current_y: &mut i32,
        start_x: i32,
        width: i32,
        line_runs: &mut Vec<TextRun>,
        line_start_y: &mut i32,
        is_first_line: &mut bool,
        line_char_start: usize,
        line_char_end: &mut usize,
        flush_line: &mut F,
        completed_lines: &mut Vec<LayoutLine>,
        ctx: &mut dyn DrawContext,
    ) where
        F: FnMut(&mut Vec<TextRun>, i32, usize, usize, usize, i32, &mut Vec<LayoutLine>),
    {
        for child in &node.children {
            match &child.node_type {
                NodeType::Text { content, .. } => {
                    let text = content.clone();
                    let style_idx = self.get_style_idx_for_node(&child.node_type);
                    let (font, size) = self.get_font_for_node(&child.node_type);

                    // Simple word wrapping
                    for word in text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_width = ctx.text_width(&word_with_space, font, size) as i32;

                        // Check if word fits on current line
                        if *current_x + word_width > start_x + width && *current_x > start_x {
                            // Flush current line before wrapping
                            flush_line(line_runs, *line_start_y, line_char_start, *line_char_end, child.id, self.line_height, completed_lines);

                            // Start new line
                            *current_y += self.line_height;
                            *current_x = start_x;
                            *line_start_y = *current_y;
                            *is_first_line = false;
                        }

                        line_runs.push(TextRun {
                            text: word_with_space,
                            x: *current_x,
                            style_idx,
                            node_id: child.id,
                            char_range: (child.char_start, child.char_end),
                        });

                        *current_x += word_width;
                        *line_char_end = child.char_end;
                    }
                }

                NodeType::Code { content } => {
                    let text = content.clone();
                    let style_idx = self.get_style_idx_for_node(&child.node_type);
                    let (font, size) = self.get_font_for_node(&child.node_type);

                    // Simple word wrapping for code
                    for word in text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_width = ctx.text_width(&word_with_space, font, size) as i32;

                        if *current_x + word_width > start_x + width && *current_x > start_x {
                            // Flush current line before wrapping
                            flush_line(line_runs, *line_start_y, line_char_start, *line_char_end, child.id, self.line_height, completed_lines);

                            *current_y += self.line_height;
                            *current_x = start_x;
                            *line_start_y = *current_y;
                            *is_first_line = false;
                        }

                        line_runs.push(TextRun {
                            text: word_with_space,
                            x: *current_x,
                            style_idx,
                            node_id: child.id,
                            char_range: (child.char_start, child.char_end),
                        });

                        *current_x += word_width;
                        *line_char_end = child.char_end;
                    }
                }

                NodeType::Link { .. } => {
                    let text = child.flatten_text();
                    let style_idx = self.get_style_idx_for_node(&child.node_type);
                    let (font, size) = self.get_font_for_node(&child.node_type);

                    // Simple word wrapping for links
                    for word in text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_width = ctx.text_width(&word_with_space, font, size) as i32;

                        if *current_x + word_width > start_x + width && *current_x > start_x {
                            // Flush current line before wrapping
                            flush_line(line_runs, *line_start_y, line_char_start, *line_char_end, child.id, self.line_height, completed_lines);

                            *current_y += self.line_height;
                            *current_x = start_x;
                            *line_start_y = *current_y;
                            *is_first_line = false;
                        }

                        line_runs.push(TextRun {
                            text: word_with_space,
                            x: *current_x,
                            style_idx,
                            node_id: child.id,
                            char_range: (child.char_start, child.char_end),
                        });

                        *current_x += word_width;
                        *line_char_end = child.char_end;
                    }
                }

                NodeType::SoftBreak => {
                    // Treat as space
                    line_runs.push(TextRun {
                        text: " ".to_string(),
                        x: *current_x,
                        style_idx: 0,
                        node_id: child.id,
                        char_range: (child.char_start, child.char_end),
                    });
                    *current_x += ctx.text_width(" ", self.text_font, self.text_size) as i32;
                }

                NodeType::HardBreak => {
                    // Flush current line and force new line
                    flush_line(line_runs, *line_start_y, line_char_start, *line_char_end, child.id, self.line_height, completed_lines);

                    *current_y += self.line_height;
                    *current_x = start_x;
                    *line_start_y = *current_y;
                    *is_first_line = false;
                }

                // Recursively handle container nodes
                _ if child.node_type.can_have_children() => {
                    self.layout_inline_content_core(
                        child,
                        current_x,
                        current_y,
                        start_x,
                        width,
                        line_runs,
                        line_start_y,
                        is_first_line,
                        line_char_start,
                        line_char_end,
                        flush_line,
                        completed_lines,
                        ctx,
                    );
                }

                _ => {}
            }
        }
    }

    /// Layout inline content (text, links, emphasis, etc.) with word wrapping
    /// DEPRECATED: Use layout_inline_content_multiline instead
    fn layout_inline_content(
        &mut self,
        node: &ASTNode,
        current_x: &mut i32,
        current_y: &mut i32,
        start_x: i32,
        width: i32,
        line_runs: &mut Vec<TextRun>,
        line_char_end: &mut usize,
        ctx: &mut dyn DrawContext,
    ) {
        for child in &node.children {
            match &child.node_type {
                NodeType::Text { content, .. } => {
                    let text = content.clone();

                    let style_idx = self.get_style_idx_for_node(&child.node_type);

                    // Simple word wrapping
                    for word in text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_width = ctx.text_width(&word_with_space, self.text_font, self.text_size) as i32;

                        // Check if word fits on current line
                        if *current_x + word_width > start_x + width && *current_x > start_x {
                            // Start new line
                            *current_y += self.line_height;
                            *current_x = start_x;
                        }

                        line_runs.push(TextRun {
                            text: word_with_space,
                            x: *current_x,
                            style_idx,
                            node_id: child.id,
                            char_range: (child.char_start, child.char_end),
                        });

                        *current_x += word_width;
                        *line_char_end = child.char_end;
                    }
                }

                NodeType::Code { content } => {
                    let text = content.clone();
                    let style_idx = self.get_style_idx_for_node(&child.node_type);

                    // Simple word wrapping for code
                    for word in text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_width = ctx.text_width(&word_with_space, self.text_font, self.text_size) as i32;

                        if *current_x + word_width > start_x + width && *current_x > start_x {
                            *current_y += self.line_height;
                            *current_x = start_x;
                        }

                        line_runs.push(TextRun {
                            text: word_with_space,
                            x: *current_x,
                            style_idx,
                            node_id: child.id,
                            char_range: (child.char_start, child.char_end),
                        });

                        *current_x += word_width;
                        *line_char_end = child.char_end;
                    }
                }

                NodeType::Link { .. } => {
                    let text = child.flatten_text();
                    let style_idx = self.get_style_idx_for_node(&child.node_type);

                    // Simple word wrapping for links
                    for word in text.split_whitespace() {
                        let word_with_space = format!("{} ", word);
                        let word_width = ctx.text_width(&word_with_space, self.text_font, self.text_size) as i32;

                        if *current_x + word_width > start_x + width && *current_x > start_x {
                            *current_y += self.line_height;
                            *current_x = start_x;
                        }

                        line_runs.push(TextRun {
                            text: word_with_space,
                            x: *current_x,
                            style_idx,
                            node_id: child.id,
                            char_range: (child.char_start, child.char_end),
                        });

                        *current_x += word_width;
                        *line_char_end = child.char_end;
                    }
                }

                NodeType::SoftBreak => {
                    // Treat as space
                    line_runs.push(TextRun {
                        text: " ".to_string(),
                        x: *current_x,
                        style_idx: 0,
                        node_id: child.id,
                        char_range: (child.char_start, child.char_end),
                    });
                    *current_x += ctx.text_width(" ", self.text_font, self.text_size) as i32;
                }

                NodeType::HardBreak => {
                    // Force new line
                    *current_y += self.line_height;
                    *current_x = start_x;
                }

                // Recursively handle container nodes
                _ if child.node_type.can_have_children() => {
                    self.layout_inline_content(
                        child,
                        current_x,
                        current_y,
                        start_x,
                        width,
                        line_runs,
                        line_char_end,
                        ctx,
                    );
                }

                _ => {}
            }
        }
    }

    /// Get style index for a node type
    fn get_style_idx_for_node(&self, node_type: &NodeType) -> u8 {
        self.get_style_idx_for_node_with_id(node_type, None)
    }

    /// Get style index for a node type, considering hover state
    fn get_style_idx_for_node_with_id(&self, node_type: &NodeType, node_id: Option<usize>) -> u8 {
        match node_type {
            NodeType::Heading { level } => match level {
                1 => 6, // STYLE_HEADER1
                2 => 7, // STYLE_HEADER2
                3 | _ => 8, // STYLE_HEADER3
            },
            NodeType::Code { .. } | NodeType::CodeBlock { .. } => 4, // STYLE_CODE
            NodeType::Link { .. } => {
                // Check if this link is hovered
                if let Some(id) = node_id {
                    if self.hovered_node_id == Some(id) {
                        return 10; // STYLE_LINK_HOVER (if available in style table)
                    }
                }
                5 // STYLE_LINK
            }
            NodeType::BlockQuote => 9, // STYLE_QUOTE
            NodeType::Text { style, .. } => {
                if style.bold && style.italic {
                    3 // STYLE_BOLD_ITALIC
                } else if style.bold {
                    1 // STYLE_BOLD
                } else if style.italic {
                    2 // STYLE_ITALIC
                } else if style.code {
                    4 // STYLE_CODE
                } else {
                    0 // STYLE_PLAIN
                }
            }
            _ => 0, // STYLE_PLAIN
        }
    }

    /// Get font and size for a node type (for text measurement)
    fn get_font_for_node(&self, node_type: &NodeType) -> (u8, u8) {
        let style_idx = self.get_style_idx_for_node(node_type);
        if (style_idx as usize) < self.style_table.len() {
            let style = &self.style_table[style_idx as usize];
            (style.font, style.size)
        } else {
            // Fallback to default
            (self.text_font, self.text_size)
        }
    }

    /// Draw the widget
    pub fn draw(&mut self, ctx: &mut dyn DrawContext) {
        // Perform layout if needed
        self.layout(ctx);

        // Draw background
        ctx.set_color(self.background_color);
        ctx.draw_rect_filled(self.x, self.y, self.w, self.h);

        // Set up clipping
        ctx.push_clip(self.x, self.y, self.w, self.h);

        // Draw visible lines
        let viewport_top = self.scroll_offset;
        let viewport_bottom = self.scroll_offset + self.visible_height;

        for line in &self.layout_lines {
            let line_top = line.y;
            let line_bottom = line.y + line.height;

            // Skip lines outside viewport
            if line_bottom < viewport_top || line_top > viewport_bottom {
                continue;
            }

            // Draw each text run
            for run in &line.runs {
                // Check if this run is part of a hovered link
                let is_hovered_link = if let Some(hovered_id) = self.hovered_node_id {
                    // Check if run's node is the hovered node or its descendant
                    if let Some(ref doc) = self.document {
                        if let Some(node) = self.find_node_by_id(&doc.root, run.node_id) {
                            node.id == hovered_id ||
                            self.find_parent_link(&doc.root, run.node_id)
                                .map(|n| n.id == hovered_id)
                                .unwrap_or(false)
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                } else {
                    false
                };

                // Get style, using hover style if applicable
                let mut style_idx = run.style_idx;
                if is_hovered_link && run.style_idx == 5 {
                    // Link style -> Link hover style
                    if self.style_table.len() > 10 {
                        style_idx = 10; // STYLE_LINK_HOVER
                    }
                }

                let style = if (style_idx as usize) < self.style_table.len() {
                    &self.style_table[style_idx as usize]
                } else {
                    // Fallback to default style
                    &StyleTableEntry {
                        color: self.text_color,
                        font: self.text_font,
                        size: self.text_size,
                        attr: 0,
                        bgcolor: self.background_color,
                    }
                };

                // Set font and color
                ctx.set_font(style.font, style.size);
                ctx.set_color(style.color);

                // Calculate draw position (accounting for scroll)
                let draw_y = self.y + line.y - self.scroll_offset + style.size as i32;
                let draw_x = self.x + run.x;

                // Draw background for hover (if link is hovered)
                if is_hovered_link {
                    let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;
                    ctx.set_color(style.bgcolor);
                    ctx.draw_rect_filled(draw_x, self.y + line.y - self.scroll_offset, text_width, line.height);
                    ctx.set_color(style.color); // Restore text color
                }

                // Draw text
                ctx.draw_text(&run.text, draw_x, draw_y);

                // Draw underline if needed
                if style.attr & 0x0004 != 0 {
                    // UNDERLINE flag
                    let text_width = ctx.text_width(&run.text, style.font, style.size) as i32;
                    ctx.draw_line(draw_x, draw_y + 2, draw_x + text_width, draw_y + 2);
                }
            }
        }

        ctx.pop_clip();
    }

    /// Convert x,y coordinates to character position
    pub fn xy_to_position(&self, x: i32, y: i32) -> usize {
        let adjusted_y = y + self.scroll_offset;

        // Find the line at this y position
        for line in &self.layout_lines {
            if adjusted_y >= line.y && adjusted_y < line.y + line.height {
                // Find the run at this x position
                for run in &line.runs {
                    let run_x_end = run.x + 100; // Approximate - would need proper text measurement
                    if x >= run.x && x < run_x_end {
                        return run.char_range.0;
                    }
                }
                return line.char_start;
            }
        }

        // Default to end of document
        if let Some(doc) = &self.document {
            doc.source.len()
        } else {
            0
        }
    }

    /// Find link node at given widget coordinates
    /// Returns (node_id, destination) if a link is found
    pub fn find_link_at(&self, x: i32, y: i32) -> Option<(usize, String)> {
        let adjusted_y = y + self.scroll_offset;
        let adjusted_x = x - self.x;

        // Find the line at this y position
        for line in &self.layout_lines {
            if adjusted_y >= line.y && adjusted_y < line.y + line.height {
                // Find the run at this x position
                for run in &line.runs {
                    // Get the actual width of this run's text
                    // For now, we'll estimate based on character count
                    let estimated_width = (run.text.len() as i32) * 8; // rough estimate

                    if adjusted_x >= run.x && adjusted_x < run.x + estimated_width {
                        // Check if this run belongs to a link node
                        if let Some(ref doc) = self.document {
                            if let Some(node) = self.find_node_by_id(&doc.root, run.node_id) {
                                if let NodeType::Link { destination, .. } = &node.node_type {
                                    return Some((node.id, destination.clone()));
                                }
                                // Check if this node is a child of a link
                                if let Some(link_node) = self.find_parent_link(&doc.root, run.node_id) {
                                    if let NodeType::Link { destination, .. } = &link_node.node_type {
                                        return Some((link_node.id, destination.clone()));
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

    /// Find a node by ID in the tree
    fn find_node_by_id<'a>(&self, node: &'a ASTNode, id: usize) -> Option<&'a ASTNode> {
        if node.id == id {
            return Some(node);
        }
        for child in &node.children {
            if let Some(found) = self.find_node_by_id(child, id) {
                return Some(found);
            }
        }
        None
    }

    /// Find the parent link node of a given node ID
    fn find_parent_link<'a>(&self, node: &'a ASTNode, child_id: usize) -> Option<&'a ASTNode> {
        if matches!(node.node_type, NodeType::Link { .. }) {
            // Check if any child matches
            for child in &node.children {
                if child.id == child_id || self.has_descendant(child, child_id) {
                    return Some(node);
                }
            }
        }
        // Recursively search children
        for child in &node.children {
            if let Some(link) = self.find_parent_link(child, child_id) {
                return Some(link);
            }
        }
        None
    }

    /// Check if a node has a descendant with given ID
    fn has_descendant(&self, node: &ASTNode, descendant_id: usize) -> bool {
        if node.id == descendant_id {
            return true;
        }
        for child in &node.children {
            if self.has_descendant(child, descendant_id) {
                return true;
            }
        }
        false
    }

    /// Set hovered link (for hover highlighting)
    pub fn set_hovered_link(&mut self, node_id: Option<usize>) {
        if self.hovered_node_id != node_id {
            self.hovered_node_id = node_id;
            // Invalidate layout to update hover highlighting
            self.layout_valid = false;
        }
    }

    /// Get hovered link node ID
    pub fn hovered_link(&self) -> Option<usize> {
        self.hovered_node_id
    }

    /// Get position and dimensions
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
}
