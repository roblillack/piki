// viewtxt_styled - Text file viewer with syntax highlighting
// Usage: cargo run --example viewtxt_styled <filename>

use fliki_rs::fltk_text_display::create_text_display_widget;
use fliki_rs::text_buffer::TextBuffer;
use fliki_rs::text_display::{style_attr, StyleTableEntry, WrapMode};
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
        eprintln!("Example: {} src/main.rs", args[0]);
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
        .with_size(1000, 700)
        .with_label(&format!("ViewTxt Styled - {}", filename))
        .center_screen();

    // Create text display widget with 5px padding
    let (mut text_widget, text_display) = create_text_display_widget(
        5,   // x
        5,   // y
        990, // width
        690, // height
    );

    // Set up the text buffer
    let buffer = Rc::new(RefCell::new(TextBuffer::new()));
    buffer.borrow_mut().set_text(&contents);
    text_display.borrow_mut().set_buffer(buffer.clone());

    // Set up style buffer for syntax highlighting
    let style_buffer = Rc::new(RefCell::new(TextBuffer::new()));

    // Create a simple style pattern based on file extension
    let styled_text = if filename.ends_with(".rs") {
        style_rust_code(&contents)
    } else if filename.ends_with(".md") {
        style_markdown(&contents)
    } else {
        style_plain(&contents)
    };

    style_buffer.borrow_mut().set_text(&styled_text);
    text_display.borrow_mut().set_style_buffer(style_buffer);

    // Define style table with different colors
    let style_table = vec![
        // Style A - Keywords (red)
        StyleTableEntry {
            color: 0xFF0000FF, // Red
            font: 4,           // Courier
            size: 14,
            attr: 0,
            bgcolor: 0xFFFFFFFF, // White background
        },
        // Style B - Strings (green)
        StyleTableEntry {
            color: 0x00AA00FF, // Green
            font: 4,
            size: 14,
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        },
        // Style C - Comments (blue)
        StyleTableEntry {
            color: 0x0000FFFF, // Blue
            font: 4,
            size: 14,
            attr: style_attr::UNDERLINE,
            bgcolor: 0xFFFFFFFF,
        },
        // Style D - Numbers (magenta)
        StyleTableEntry {
            color: 0xFF00FFFF, // Magenta
            font: 4,
            size: 14,
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        },
        // Style E - Functions (cyan, bold)
        StyleTableEntry {
            color: 0x00AAAAFF, // Cyan
            font: 5,           // Courier Bold
            size: 14,
            attr: 0,
            bgcolor: 0xFFFFFFFF,
        },
    ];

    text_display.borrow_mut().set_highlight_data(style_table);

    // Configure the text display
    text_display.borrow_mut().set_textfont(4); // Courier
    text_display.borrow_mut().set_textsize(14);
    text_display.borrow_mut().set_textcolor(0x000000FF); // Black text
    text_display.borrow_mut().set_cursor_color(0xFF0000FF); // Red cursor
    text_display.borrow_mut().show_cursor(true);

    // Enable text wrapping at window bounds
    text_display.borrow_mut().set_wrap_mode(WrapMode::AtBounds, 0);

    // Enable line numbers
    text_display.borrow_mut().set_linenumber_width(60);
    text_display.borrow_mut().set_linenumber_fgcolor(0x000000FF); // Black
    text_display.borrow_mut().set_linenumber_bgcolor(0xD0D0D0FF); // Gray

    // Set widget color
    text_widget.set_color(enums::Color::from_rgb(255, 255, 255));
    text_widget.set_frame(enums::FrameType::DownBox);

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

    text_widget.take_focus().ok();

    app.run().unwrap();
}

// Simple Rust syntax highlighter
fn style_rust_code(text: &str) -> String {
    let mut styled = String::with_capacity(text.len());

    let keywords = [
        "fn", "let", "mut", "pub", "use", "mod", "struct", "enum", "impl", "trait", "return", "if",
        "else", "match", "for", "while",
    ];

    // Process each character, keeping track of the current line for context
    // IMPORTANT: We need one style byte per UTF-8 byte, not per character!
    let mut current_line = String::new();
    for ch in text.chars() {
        let style_char = if ch == '\n' {
            current_line.clear();
            'A' // Newline gets default style
        } else {
            current_line.push(ch);

            // Simple heuristic styling
            if ch.is_numeric() {
                'D' // Numbers
            } else if ch.is_alphabetic() {
                // Check if current line contains a keyword (simplified)
                let mut is_keyword = false;
                for kw in &keywords {
                    if current_line.contains(kw) {
                        is_keyword = true;
                        break;
                    }
                }
                if is_keyword {
                    'A'
                } else {
                    'A'
                } // Keywords
            } else if ch == '"' || ch == '\'' {
                'B' // Strings
            } else if ch == '/' && current_line.trim_start().starts_with("//") {
                'C' // Comments
            } else {
                'A' // Default
            }
        };

        // Push the style character once for each byte in the UTF-8 encoding
        let byte_len = ch.len_utf8();
        for _ in 0..byte_len {
            styled.push(style_char);
        }
    }

    styled
}

// Simple Markdown syntax highlighter
fn style_markdown(text: &str) -> String {
    let mut styled = String::with_capacity(text.len());

    // Process each character, keeping track of the current line for context
    // IMPORTANT: We need one style byte per UTF-8 byte, not per character!
    let mut current_line = String::new();
    for ch in text.chars() {
        let style_char = if ch == '\n' {
            current_line.clear();
            'A' // Newline gets default style
        } else {
            current_line.push(ch);

            // Determine style based on line context
            if current_line.starts_with('#') {
                'E' // Headers
            } else if current_line.starts_with("```") {
                'D' // Code blocks
            } else if current_line.contains('*') || current_line.contains('_') {
                'B' // Emphasis
            } else {
                'A' // Normal text
            }
        };

        // Push the style character once for each byte in the UTF-8 encoding
        let byte_len = ch.len_utf8();
        for _ in 0..byte_len {
            styled.push(style_char);
        }
    }

    styled
}

// Plain text (no styling)
fn style_plain(text: &str) -> String {
    "A".repeat(text.len())
}
