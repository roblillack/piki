use crate::content::{ContentLoader, ContentProvider};
use crate::fltk_structured_rich_display::FltkStructuredRichDisplay;
use crate::fltk_text_display::create_text_display_widget;
use crate::page_ui::PageUI;
use crate::richtext::markdown_converter::{document_to_markdown, markdown_to_document};
use crate::richtext::structured_document::BlockType;
use crate::richtext::structured_editor::StructuredEditor;
use crate::sourceedit::text_buffer::TextBuffer;
use crate::sourceedit::text_display::TextDisplay;
use fltk::{app, enums::Color, prelude::*, window};
use std::any::Any;
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

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn hide(&mut self) {
        self.group.hide();
    }
}

/// PageUI adapter for StructuredRichDisplay + FLTK Group wrapper
pub struct StructuredRichUI(pub FltkStructuredRichDisplay);

impl StructuredRichUI {
    pub fn new(x: i32, y: i32, w: i32, h: i32, edit_mode: bool) -> Self {
        Self(FltkStructuredRichDisplay::new(x, y, w, h, edit_mode))
    }

    pub fn has_selection(&self) -> bool {
        self.0.display.borrow().editor().selection().is_some()
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let result = {
            let mut disp = self.0.display.borrow_mut();
            disp.editor_mut().cut()
        };
        match result {
            Ok(text) => {
                self.0.notify_change();
                if text.is_empty() { None } else { Some(text) }
            }
            Err(_) => None,
        }
    }

    pub fn copy_selection(&self) -> Option<String> {
        let disp = self.0.display.borrow();
        let text = disp.editor().copy();
        if text.is_empty() { None } else { Some(text) }
    }

    pub fn paste_from_clipboard(&mut self) {
        let mut group = self.0.group.clone();
        app::paste(&mut group);
    }

    pub fn clear_formatting(&mut self) -> bool {
        self.apply_edit(|editor| editor.clear_formatting())
    }

    pub fn set_block_type(&mut self, block_type: BlockType) -> bool {
        self.apply_edit(move |editor| editor.set_block_type(block_type))
    }

    pub fn toggle_quote(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_quote())
    }

    pub fn toggle_code_block(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_code_block())
    }

    pub fn toggle_list(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_list())
    }

    pub fn toggle_checklist(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_checklist())
    }

    pub fn toggle_ordered_list(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_ordered_list())
    }

    pub fn toggle_bold(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_bold())
    }

    pub fn toggle_italic(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_italic())
    }

    pub fn toggle_code(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_code())
    }

    pub fn toggle_strikethrough(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_strikethrough())
    }

    pub fn toggle_underline(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_underline())
    }

    pub fn toggle_highlight(&mut self) -> bool {
        self.apply_edit(|editor| editor.toggle_highlight())
    }

    pub fn current_block_type(&self) -> Option<BlockType> {
        let disp = self.0.display.borrow();
        let editor = disp.editor();
        let blocks = editor.document().blocks();
        let idx = editor.cursor().block_index;
        blocks.get(idx).map(|b| b.block_type.clone())
    }

    fn apply_edit<F>(&mut self, edit: F) -> bool
    where
        F: FnOnce(&mut StructuredEditor) -> crate::richtext::structured_editor::EditResult,
    {
        let result = {
            let mut disp = self.0.display.borrow_mut();
            let editor = disp.editor_mut();
            edit(editor)
        };
        if result.is_ok() {
            self.0.notify_change();
            self.0.emit_paragraph_state();
            true
        } else {
            false
        }
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
        drop(disp);
        self.0.emit_paragraph_state();
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

    fn on_link_hover(&mut self, f: Box<dyn Fn(Option<String>) + 'static>) {
        self.0.set_link_hover_callback(Some(f));
    }

    fn on_paragraph_style_change(&mut self, f: Box<dyn FnMut(BlockType) + 'static>) {
        self.0.set_paragraph_callback(Some(f));
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn hide(&mut self) {
        self.0.group.hide();
    }
}
