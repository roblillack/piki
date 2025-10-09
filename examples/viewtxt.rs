// viewtxt - Simple text file viewer using custom TextDisplay
// Usage: cargo run --example viewtxt <filename>

use fliki_rs::fltk_text_display::create_text_display_widget;
use fliki_rs::text_buffer::TextBuffer;
use fltk::{prelude::*, *};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::process;
use std::rc::Rc;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        eprintln!("Example: {} README.md", args[0]);
        process::exit(1);
    }

    let filename = &args[1];

    // Read the file
    let contents = match fs::read_to_string(filename) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("Error reading file '{}': {}", filename, err);
            process::exit(1);
        }
    };

    // Create the application
    let app = app::App::default();

    // Create main window
    let mut wind = window::Window::default()
        .with_size(800, 600)
        .with_label(&format!("ViewTxt - {}", filename))
        .center_screen();

    // Create text display widget with 5px padding
    let (mut text_widget, text_display) = create_text_display_widget(
        5,   // x
        5,   // y
        790, // width (800 - 10)
        590, // height (600 - 10)
    );

    // Set up the buffer
    let buffer = Rc::new(RefCell::new(TextBuffer::new()));
    buffer.borrow_mut().set_text(&contents);
    text_display.borrow_mut().set_buffer(buffer.clone());

    // Configure the text display
    text_display.borrow_mut().set_textfont(4); // Courier font
    text_display.borrow_mut().set_textsize(14);
    text_display.borrow_mut().set_textcolor(0x000000FF); // Black text
    text_display.borrow_mut().set_cursor_color(0x000000FF); // Black cursor
    text_display.borrow_mut().show_cursor(true);

    // Enable line numbers
    text_display.borrow_mut().set_linenumber_width(50);
    text_display.borrow_mut().set_linenumber_fgcolor(0x000000FF); // Black
    text_display.borrow_mut().set_linenumber_bgcolor(0xE0E0E0FF); // Light gray

    // Set widget color
    text_widget.set_color(enums::Color::from_rgb(255, 255, 255));

    // Handle window resize
    wind.handle({
        let mut text_widget_handle = text_widget.clone();
        move |w, event| {
            match event {
                enums::Event::Resize => {
                    // Resize the text widget (which will trigger its resize callback)
                    let new_w = w.w() - 10;
                    let new_h = w.h() - 10;
                    text_widget_handle.resize(5, 5, new_w, new_h);
                    true
                }
                _ => false,
            }
        }
    });

    wind.make_resizable(true);
    wind.end();
    wind.show();

    // Make the text widget take focus initially
    text_widget.take_focus().ok();

    app.run().unwrap();
}
