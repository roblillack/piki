use crate::content::{ContentLoader, ContentProvider};
use crate::fltk_structured_rich_display::FltkStructuredRichDisplay;
use crate::fltk_text_display::create_text_display_widget;
use crate::page_ui::PageUI;
use crate::richtext::markdown_converter::{document_to_markdown, markdown_to_document};
use crate::sourceedit::text_buffer::TextBuffer;
use crate::sourceedit::text_display::TextDisplay;
use crate::sourceedit::text_display::{style_attr, StyleTableEntry};
use fltk::{enums::Color, prelude::*, window};
use std::cell::RefCell;
use std::rc::Rc;

/// PageUI adapter for TextDisplay + FLTK Group wrapper
pub struct TextDisplayUI {
    pub group: fltk::group::Group,
    pub display: Rc<RefCell<TextDisplay>>,
}

impl TextDisplayUI {
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        let (group, display) = create_text_display_widget(x, y, w, h);
        Self { group, display }
    }

    fn with_buffer<F: FnOnce(&mut TextBuffer)>(&self, f: F) {
        if let Some(buf_rc) = self.display.borrow().buffer() {
            f(&mut buf_rc.borrow_mut());
        }
    }
}

impl ContentProvider for TextDisplayUI {
    fn get_content(&self) -> String {
        if let Some(buf_rc) = self.display.borrow().buffer() {
            buf_rc.borrow().text()
        } else {
            String::new()
        }
    }
}

impl ContentLoader for TextDisplayUI {
    fn set_content_from_markdown(&mut self, markdown: &str) {
        use crate::sourceedit::text_buffer::TextBuffer;
        if let Some(buf_rc) = self.display.borrow().buffer() {
            buf_rc.borrow_mut().set_text(markdown);
        } else {
            let mut buf = TextBuffer::new();
            buf.set_text(markdown);
            let rc = Rc::new(RefCell::new(buf));
            self.display.borrow_mut().set_buffer(rc);
        }
    }
}

impl PageUI for TextDisplayUI {
    fn on_change(&mut self, mut f: Box<dyn FnMut() + 'static>) {
        if let Some(buf_rc) = self.display.borrow().buffer() {
            buf_rc
                .borrow_mut()
                .add_modify_callback(move |_, _, _, _, _| {
                    f();
                });
        }
    }

    fn set_readonly(&mut self, _readonly: bool) {}
    fn is_readonly(&self) -> bool {
        true
    }

    fn scroll_pos(&self) -> i32 {
        self.display.borrow().top_line_num() as i32
    }

    fn set_scroll_pos(&mut self, pos: i32) {
        let horiz = self.display.borrow().horiz_offset();
        self.display.borrow_mut().scroll(pos.max(0) as usize, horiz);
    }

    fn set_bg_color(&mut self, color: Color) {
        self.group.set_color(color);
    }

    fn set_resizable(&self, wind: &mut window::Window) {
        wind.resizable(&self.group);
    }

    fn on_link_click(&mut self, _f: Box<dyn Fn(String) + 'static>) {
        // TextDisplay adapter does not implement link detection.
    }
}

/// PageUI adapter for StructuredRichDisplay + FLTK Group wrapper
pub struct StructuredRichUI(pub FltkStructuredRichDisplay);

impl StructuredRichUI {
    pub fn new(x: i32, y: i32, w: i32, h: i32, edit_mode: bool) -> Self {
        let mut ui = FltkStructuredRichDisplay::new(x, y, w, h, edit_mode);
        {
            // Match examples/viewmd.rs style table and widget styling
            const DEFAULT_FONT_SIZE: u8 = 14;
            const HIGHLIGHT_COLOR: u32 = 0xFFFF00FF;

            // Base styles 0..10
            let mut styles: Vec<StyleTableEntry> = vec![
                // 0 Plain
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 0,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 1 Bold
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 1,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 2 Italic
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 2,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 3 Bold+Italic
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 3,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 4 Code
                StyleTableEntry {
                    color: 0x0064C8FF,
                    font: 4,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 5 Link
                StyleTableEntry {
                    color: 0x0000FFFF,
                    font: 0,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 6 Header1
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 1,
                    size: DEFAULT_FONT_SIZE + 6,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 7 Header2
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 1,
                    size: DEFAULT_FONT_SIZE + 4,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 8 Header3
                StyleTableEntry {
                    color: 0x000000FF,
                    font: 1,
                    size: DEFAULT_FONT_SIZE + 2,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 9 Quote
                StyleTableEntry {
                    color: 0x640000FF,
                    font: 10,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::BGCOLOR,
                    bgcolor: 0xFFFFF5FF,
                },
                // 10 Link hover
                StyleTableEntry {
                    color: 0x0000FFFF,
                    font: 0,
                    size: DEFAULT_FONT_SIZE,
                    attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
                    bgcolor: 0xD3D3D3FF,
                },
            ];

            // Decorated variants 11..42 like in example
            let base_fonts = [0, 1, 2, 3];
            for base in 0..4 {
                for decoration in 1..8 {
                    let underline = (decoration & 1) != 0;
                    let strikethrough = (decoration & 2) != 0;
                    let highlight = (decoration & 4) != 0;

                    let mut attr = style_attr::BGCOLOR;
                    if underline {
                        attr |= style_attr::UNDERLINE;
                    }
                    if strikethrough {
                        attr |= style_attr::STRIKE_THROUGH;
                    }
                    let bgcolor = if highlight {
                        HIGHLIGHT_COLOR
                    } else {
                        0xFFFFF5FF
                    };
                    styles.push(StyleTableEntry {
                        color: 0x000000FF,
                        font: base_fonts[base],
                        size: DEFAULT_FONT_SIZE,
                        attr,
                        bgcolor,
                    });
                }
            }

            ui.display.borrow_mut().set_style_table(styles);
            ui.display.borrow_mut().set_padding(10, 10, 25, 25);
            ui.group.set_color(Color::from_rgb(255, 255, 245));
            ui.group.set_frame(fltk::enums::FrameType::FlatBox);
        }
        Self(ui)
    }
}

impl ContentProvider for StructuredRichUI {
    fn get_content(&self) -> String {
        let disp = self.0.display.borrow();
        let doc = disp.editor().document();
        document_to_markdown(doc)
    }
}

impl ContentLoader for StructuredRichUI {
    fn set_content_from_markdown(&mut self, markdown: &str) {
        let mut disp = self.0.display.borrow_mut();
        let editor = disp.editor_mut();
        *editor.document_mut() = markdown_to_document(markdown);
        disp.set_scroll(0);
    }
}

impl PageUI for StructuredRichUI {
    fn on_change(&mut self, f: Box<dyn FnMut() + 'static>) {
        self.0.set_change_callback(Some(f));
    }

    fn set_readonly(&mut self, readonly: bool) {
        self.0.display.borrow_mut().set_cursor_visible(!readonly);
    }

    fn is_readonly(&self) -> bool {
        !self.0.display.borrow().cursor_visible()
    }

    fn scroll_pos(&self) -> i32 {
        self.0.display.borrow().scroll_offset()
    }

    fn set_scroll_pos(&mut self, pos: i32) {
        self.0.display.borrow_mut().set_scroll(pos.max(0));
    }

    fn set_bg_color(&mut self, color: Color) {
        self.0.group.set_color(color);
    }

    fn set_resizable(&self, wind: &mut window::Window) {
        wind.resizable(&self.0.group);
    }

    fn on_link_click(&mut self, f: Box<dyn Fn(String) + 'static>) {
        self.0.set_link_callback(Some(f));
    }

    fn tick(&mut self, ms_since_start: u64) {
        self.0.tick(ms_since_start);
    }
}
