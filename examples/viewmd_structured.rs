// viewmd_structured - Markdown file viewer with structured editor
// Usage: cargo run --example viewmd_structured [--edit] <filename>

use fliki_rs::fltk_structured_rich_display::create_structured_rich_display_widget;
use fliki_rs::markdown_converter::{document_to_markdown, markdown_to_document};
use fliki_rs::structured_document::DocumentPosition;
use fliki_rs::text_display::{style_attr, StyleTableEntry};
use fltk::{prelude::*, *};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

const DEFAULT_FONT_SIZE: u8 = 14;

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} [--edit] <filename>", args[0]);
        eprintln!("Example: {} README.md", args[0]);
        eprintln!("         {} --edit README.md (enable editing)", args[0]);
        process::exit(1);
    }

    // Check for --edit flag
    let edit_mode = args.contains(&"--edit".to_string());
    let filename_arg = args
        .iter()
        .skip(1)
        .find(|a| *a != "--edit")
        .expect("Filename required");

    let filename = PathBuf::from(filename_arg);

    // Read the file
    let contents = match fs::read_to_string(&filename) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!("Error reading file '{}': {}", filename.display(), err);
            process::exit(1);
        }
    };

    // Create the application
    let app = app::App::default();

    // Create main window
    let mut wind = window::Window::default()
        .with_size(800, 600)
        .with_label(&format!(
            "ViewMD (Structured{}) - {}",
            if edit_mode { " Edit" } else { "" },
            filename.display()
        ))
        .center_screen();

    // Create structured rich display widget
    let (mut display_widget, display) = create_structured_rich_display_widget(
        5,   // x
        5,   // y
        790, // width
        590, // height
        edit_mode, // edit mode
    );

    // Convert markdown to structured document
    let doc = markdown_to_document(&contents);
    {
        let mut d = display.borrow_mut();
        *d.editor_mut().document_mut() = doc;
        d.editor_mut().set_cursor(DocumentPosition::start());
    }

    // Set up style table
    let style_table = vec![
        // Style 0 - STYLE_PLAIN
        StyleTableEntry {
            color: 0x000000FF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 1 - STYLE_BOLD
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 2 - STYLE_ITALIC
        StyleTableEntry {
            color: 0x000000FF,
            font: 2,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 3 - STYLE_BOLD_ITALIC
        StyleTableEntry {
            color: 0x000000FF,
            font: 3,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 4 - STYLE_CODE
        StyleTableEntry {
            color: 0x0064C8FF,
            font: 4,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 5 - STYLE_LINK
        StyleTableEntry {
            color: 0x0000FFFF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 6 - STYLE_HEADER1
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE + 6,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 7 - STYLE_HEADER2
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE + 4,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 8 - STYLE_HEADER3
        StyleTableEntry {
            color: 0x000000FF,
            font: 1,
            size: DEFAULT_FONT_SIZE + 2,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 9 - STYLE_QUOTE
        StyleTableEntry {
            color: 0x640000FF,
            font: 10,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style 10 - STYLE_LINK_HOVER
        StyleTableEntry {
            color: 0x0000FFFF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: 0xD3D3D3FF,
        },
    ];

    display.borrow_mut().set_style_table(style_table);
    display.borrow_mut().set_padding(10, 10, 25, 25);

    // Set widget color
    display_widget.set_color(enums::Color::from_rgb(255, 255, 245));
    display_widget.set_frame(enums::FrameType::FlatBox);

    // Handle window resize
    wind.handle({
        let mut widget_handle = display_widget.clone();
        move |w, event| match event {
            enums::Event::Resize => {
                let new_w = w.w() - 10;
                let new_h = w.h() - 10;
                widget_handle.resize(5, 5, new_w, new_h);
                true
            }
            _ => false,
        }
    });

    wind.make_resizable(true);
    wind.end();
    wind.show();

    display_widget.take_focus().ok();

    app.run().unwrap();
}
