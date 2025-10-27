use crate::link_handler::{Link, extract_links};
use fltk::text::PositionType;
use fltk::{prelude::*, *};
use std::any::Any;

const DEFAULT_FONT_SIZE: i32 = 14;

// Style characters for different text styles
const STYLE_PLAIN: char = 'A';
const STYLE_BOLD: char = 'B';
const STYLE_ITALIC: char = 'C';
#[allow(dead_code)]
const STYLE_BOLD_ITALIC: char = 'D';
const STYLE_CODE: char = 'E';
const STYLE_LINK: char = 'F';
const STYLE_HEADER1: char = 'G';
const STYLE_HEADER2: char = 'H';
const STYLE_HEADER3: char = 'I';
const STYLE_QUOTE: char = 'J';
#[allow(dead_code)]
const STYLE_DIMMED: char = 'K';

pub struct MarkdownEditor {
    editor: text::TextEditor,
    buffer: text::TextBuffer,
    style_buffer: text::TextBuffer,
    links: Vec<Link>,
    readonly: bool,
}

impl MarkdownEditor {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        let buffer = text::TextBuffer::default();
        let style_buffer = text::TextBuffer::default();
        let mut editor = text::TextEditor::new(x, y, w, h, None);

        editor.set_buffer(buffer.clone());
        editor.set_frame(enums::FrameType::FlatBox);

        // Define style table
        let styles = vec![
            // STYLE_PLAIN
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::Helvetica,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_BOLD
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::HelveticaBold,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_ITALIC
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::HelveticaItalic,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_BOLD_ITALIC
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::HelveticaBoldItalic,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_CODE
            text::StyleTableEntry {
                color: enums::Color::from_rgb(0, 100, 200),
                font: enums::Font::Courier,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_LINK (note: underline not directly supported in style table)
            text::StyleTableEntry {
                color: enums::Color::Blue,
                font: enums::Font::Helvetica,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_HEADER1
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::HelveticaBold,
                size: DEFAULT_FONT_SIZE + 4,
            },
            // STYLE_HEADER2
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::HelveticaBold,
                size: DEFAULT_FONT_SIZE + 2,
            },
            // STYLE_HEADER3
            text::StyleTableEntry {
                color: enums::Color::Black,
                font: enums::Font::HelveticaBold,
                size: DEFAULT_FONT_SIZE + 2,
            },
            // STYLE_QUOTE
            text::StyleTableEntry {
                color: enums::Color::from_rgb(100, 0, 0),
                font: enums::Font::TimesItalic,
                size: DEFAULT_FONT_SIZE,
            },
            // STYLE_DIMMED
            text::StyleTableEntry {
                color: enums::Color::Gray0,
                font: enums::Font::Helvetica,
                size: DEFAULT_FONT_SIZE,
            },
        ];

        editor.set_highlight_data(style_buffer.clone(), styles);
        editor.wrap_mode(text::WrapMode::AtBounds, 0);

        let mut md_editor = MarkdownEditor {
            editor,
            buffer,
            style_buffer,
            links: Vec::new(),
            readonly: false,
        };

        // Set up auto-restyling on text changes
        md_editor.setup_auto_restyle();

        md_editor
    }

    /// Set up automatic restyling when text changes
    fn setup_auto_restyle(&mut self) {
        let mut style_buffer = self.style_buffer.clone();
        let mut buffer_clone = self.buffer.clone();

        // When text changes, update the style buffer to match
        buffer_clone.add_modify_callback(move |pos, n_inserted, n_deleted, _, _| {
            // First, adjust the style buffer size to match the text buffer
            if n_inserted > 0 {
                // Insert placeholder styles for new characters
                let new_styles: String = (0..n_inserted).map(|_| STYLE_PLAIN).collect();
                style_buffer.insert(pos, &new_styles);
            }
            if n_deleted > 0 {
                // Remove styles for deleted characters
                style_buffer.remove(pos, pos + n_deleted);
            }
        });
    }

    pub fn set_content(&mut self, content: &str) {
        self.buffer.set_text(content);
        self.links = extract_links(content);
        self.apply_styles();
    }

    pub fn get_content(&self) -> String {
        self.buffer.text()
    }

    pub fn update_links(&mut self) {
        let content = self.buffer.text();
        self.links = extract_links(&content);
        self.apply_styles();
    }

    /// Manually trigger a full re-style of the current content
    pub fn restyle(&mut self) {
        self.update_links();
    }

    #[allow(dead_code)]
    pub fn has_selection(&self) -> bool {
        !self.buffer.selection_text().is_empty()
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let selected = self.buffer.selection_text();
        if selected.is_empty() {
            None
        } else {
            self.editor.cut();
            Some(selected)
        }
    }

    pub fn copy_selection(&self) -> Option<String> {
        let selected = self.buffer.selection_text();
        if selected.is_empty() {
            None
        } else {
            Some(selected)
        }
    }

    pub fn paste_from_clipboard(&mut self) {
        let editor_clone = self.editor.clone();
        app::paste(&editor_clone);
    }

    /// Set read-only mode for the editor
    /// When read-only, text can be selected but not edited
    pub fn set_readonly(&mut self, readonly: bool) {
        self.readonly = readonly;

        // Visual indicator: slightly different background color for read-only
        if readonly {
            self.editor.set_color(enums::Color::from_rgb(245, 245, 245));
        } else {
            self.editor.set_color(enums::Color::from_rgb(255, 255, 245));
        }
    }

    /// Check if editor is in read-only mode
    pub fn is_readonly(&self) -> bool {
        self.readonly
    }

    /// Apply syntax highlighting styles to the text
    fn apply_styles(&mut self) {
        let content = self.buffer.text();
        let len = content.len();

        // Initialize style buffer with plain style
        let mut styles = vec![STYLE_PLAIN as u8; len];

        // Apply line-by-line styling
        for (line_idx, line) in content.lines().enumerate() {
            let line_start = content
                .lines()
                .take(line_idx)
                .map(|l| l.len() + 1) // +1 for newline
                .sum::<usize>();

            self.style_line(line, line_start, &mut styles);
        }

        // Apply link styling
        for link in &self.links {
            for style in styles.iter_mut().take(link.end.min(len)).skip(link.start) {
                *style = STYLE_LINK as u8;
            }
        }

        // Convert to string and set style buffer
        let style_text: String = styles.iter().map(|&b| b as char).collect();
        self.style_buffer.set_text(&style_text);
    }

    /// Style a single line based on Markdown syntax
    fn style_line(&self, line: &str, line_start: usize, styles: &mut [u8]) {
        let line_end = line_start + line.len();

        // Headers
        if line.starts_with("# ") {
            for style in styles.iter_mut().take(line_end).skip(line_start) {
                *style = STYLE_HEADER1 as u8;
            }
            return;
        } else if line.starts_with("## ") {
            for style in styles.iter_mut().take(line_end).skip(line_start) {
                *style = STYLE_HEADER2 as u8;
            }
            return;
        } else if line.starts_with("### ") {
            for style in styles.iter_mut().take(line_end).skip(line_start) {
                *style = STYLE_HEADER3 as u8;
            }
            return;
        }

        // Blockquotes
        if line.starts_with("> ") {
            for style in styles.iter_mut().take(line_end).skip(line_start) {
                *style = STYLE_QUOTE as u8;
            }
            return;
        }

        // Code blocks (indented with 4 spaces or tab)
        if line.starts_with("    ") || line.starts_with("\t") {
            for style in styles.iter_mut().take(line_end).skip(line_start) {
                *style = STYLE_CODE as u8;
            }
            return;
        }

        // Inline styles (bold, italic, code)
        self.apply_inline_styles(line, line_start, styles);
    }

    /// Apply inline styles like **bold**, *italic*, `code`
    fn apply_inline_styles(&self, line: &str, line_start: usize, styles: &mut [u8]) {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            // Code spans `code`
            if chars[i] == '`'
                && let Some(end) = chars[i + 1..].iter().position(|&c| c == '`')
            {
                let end_idx = i + 1 + end;
                for j in i..=end_idx {
                    if line_start + j < styles.len() {
                        styles[line_start + j] = STYLE_CODE as u8;
                    }
                }
                i = end_idx + 1;
                continue;
            }

            // Bold **text**
            if i + 1 < chars.len()
                && chars[i] == '*'
                && chars[i + 1] == '*'
                && let Some(end) = find_delimiter(&chars[i + 2..], "**")
            {
                let end_idx = i + 2 + end;
                for j in i..=end_idx + 1 {
                    if line_start + j < styles.len() {
                        styles[line_start + j] = STYLE_BOLD as u8;
                    }
                }
                i = end_idx + 2;
                continue;
            }

            // Italic *text*
            if chars[i] == '*'
                && let Some(end) = chars[i + 1..].iter().position(|&c| c == '*')
            {
                let end_idx = i + 1 + end;
                for j in i..=end_idx {
                    if line_start + j < styles.len() {
                        styles[line_start + j] = STYLE_ITALIC as u8;
                    }
                }
                i = end_idx + 1;
                continue;
            }

            i += 1;
        }
    }
}

// Implement the shared content trait so this editor can work
// with autosave and other generic content consumers.
impl piki::content::ContentProvider for MarkdownEditor {
    fn get_content(&self) -> String {
        self.get_content()
    }
}

impl piki::content::ContentLoader for MarkdownEditor {
    fn set_content_from_markdown(&mut self, markdown: &str) {
        self.set_content(markdown);
    }
}

impl piki::page_ui::PageUI for MarkdownEditor {
    fn on_change(&mut self, mut f: Box<dyn FnMut() + 'static>) {
        let mut w = self.editor.clone();
        w.set_trigger(enums::CallbackTrigger::Changed);
        w.set_callback(move |_| {
            f();
        });
    }

    fn set_readonly(&mut self, readonly: bool) {
        self.set_readonly(readonly);
    }

    fn is_readonly(&self) -> bool {
        self.is_readonly()
    }

    fn scroll_pos(&self) -> i32 {
        self.editor.scroll_row()
    }

    fn set_scroll_pos(&mut self, pos: i32) {
        self.editor.scroll(pos, 0);
    }

    fn set_bg_color(&mut self, color: enums::Color) {
        self.editor.set_color(color);
    }

    fn set_resizable(&self, wind: &mut window::Window) {
        wind.resizable(&self.editor);
    }

    fn on_link_click(&mut self, f: Box<dyn Fn(String) + 'static>) {
        use crate::link_handler::{extract_links, find_link_at_position};
        let cb = std::rc::Rc::new(f);
        let mut w = self.editor.clone();
        w.handle(move |widget, evt| match evt {
            enums::Event::Move => {
                let pos = widget.xy_to_position(
                    app::event_x() - widget.x(),
                    app::event_y() - widget.y(),
                    PositionType::Cursor,
                );
                let mut win = match widget.window() {
                    Some(win) => win,
                    None => return false,
                };
                let over_link = widget
                    .buffer()
                    .and_then(|b| {
                        let text = b.text();
                        let links = extract_links(&text);
                        find_link_at_position(&links, pos as usize).map(|_| ())
                    })
                    .is_some();
                if over_link {
                    win.set_cursor(enums::Cursor::Hand);
                    app::awake_callback(move || {
                        win.set_cursor(enums::Cursor::Hand);
                    });
                    true
                } else {
                    win.set_cursor(enums::Cursor::Arrow);
                    app::awake_callback(move || {
                        win.set_cursor(enums::Cursor::Arrow);
                    });
                    true
                }
            }
            enums::Event::Push => {
                if app::event_mouse_button() == app::MouseButton::Left {
                    let pos = widget.xy_to_position(
                        app::event_x() - widget.x(),
                        app::event_y() - widget.y(),
                        PositionType::Cursor,
                    );
                    if let Some(buf) = widget.buffer() {
                        let text = buf.text();
                        let links = extract_links(&text);
                        if let Some(link) = find_link_at_position(&links, pos as usize) {
                            let cb2 = cb.clone();
                            let dest = link.destination.clone();
                            (cb2)(dest);
                            return true;
                        }
                    }
                }
                false
            }
            _ => false,
        });
    }

    fn on_link_hover(&mut self, f: Box<dyn Fn(Option<String>) + 'static>) {
        use crate::link_handler::{extract_links, find_link_at_position};
        let cb = std::rc::Rc::new(f);
        let mut w = self.editor.clone();
        w.handle(move |widget, evt| match evt {
            enums::Event::Move | enums::Event::Enter | enums::Event::Drag => {
                let pos = widget.xy_to_position(
                    app::event_x() - widget.x(),
                    app::event_y() - widget.y(),
                    PositionType::Cursor,
                );
                if let Some(buf) = widget.buffer() {
                    let text = buf.text();
                    let links = extract_links(&text);
                    if let Some(link) = find_link_at_position(&links, pos as usize) {
                        let cb2 = cb.clone();
                        (cb2)(Some(link.destination.clone()));
                        return false;
                    }
                }
                let cb2 = cb.clone();
                (cb2)(None);
                false
            }
            _ => false,
        });
    }

    fn restyle(&mut self) {
        self.restyle();
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn hide(&mut self) {
        self.editor.hide();
    }
}

/// Helper function to find a delimiter in a character slice
fn find_delimiter(chars: &[char], delim: &str) -> Option<usize> {
    let delim_chars: Vec<char> = delim.chars().collect();
    let delim_len = delim_chars.len();

    (0..chars.len())
        .find(|&i| i + delim_len <= chars.len() && chars[i..i + delim_len] == delim_chars[..])
}
