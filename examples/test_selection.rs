use fliki_rs::fltk_text_display::create_text_display_widget;
use fliki_rs::text_buffer::TextBuffer;
use fltk::prelude::*;
use std::cell::RefCell;
use std::rc::Rc;

fn main() {
    let app = fltk::app::App::default();
    let mut window = fltk::window::Window::default()
        .with_size(800, 600)
        .with_label("Text Selection Test");

    // Create text buffer with sample content
    let buffer = Rc::new(RefCell::new(TextBuffer::new()));
    buffer.borrow_mut().insert(
        0,
        "Text Selection & Copy Test\n\n\
        Try the following:\n\
        - Click and drag to select text\n\
        - Double-click to select a word\n\
        - Triple-click to select a line\n\
        - Shift-click to extend selection\n\
        - Right-click selected text for context menu\n\
        - Ctrl-C (Cmd-C on Mac) to copy selection\n\
        - Ctrl-A (Cmd-A on Mac) to select all\n\n\
        Lorem ipsum dolor sit amet, consectetur adipiscing elit.\n\
        Sed do eiusmod tempor incididunt ut labore et dolore magna aliqua.\n\
        Ut enim ad minim veniam, quis nostrud exercitation ullamco.\n\
        Laboris nisi ut aliquip ex ea commodo consequat.\n\n\
        Duis aute irure dolor in reprehenderit in voluptate velit.\n\
        Esse cillum dolore eu fugiat nulla pariatur.\n\
        Excepteur sint occaecat cupidatat non proident.\n\
        Sunt in culpa qui officia deserunt mollit anim id est laborum.",
    );

    // Create text display widget
    let (_widget, text_display) = create_text_display_widget(10, 10, 780, 580);

    // Set the buffer
    text_display.borrow_mut().set_buffer(buffer);
    text_display.borrow_mut().set_textsize(14);

    window.end();
    window.show();

    app.run().unwrap();
}
