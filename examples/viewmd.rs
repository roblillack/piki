// viewmd - Markdown file viewer with clickable links
// Usage: cargo run --example viewmd <filename>

use fliki_rs::fltk_text_display::create_text_display_widget;
use fliki_rs::link_handler::{extract_links, find_link_at_position, Link};
use fliki_rs::text_buffer::TextBuffer;
use fliki_rs::text_display::{style_attr, PositionType, StyleTableEntry, WrapMode};
use fltk::app::{event_mouse_button, event_x, event_y, MouseButton};
use fltk::{prelude::*, *};
use std::cell::RefCell;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process;
use std::rc::Rc;

const DEFAULT_FONT_SIZE: u8 = 14;

// Style characters for different text styles
const STYLE_PLAIN: char = 'A';
const STYLE_BOLD: char = 'B';
const STYLE_ITALIC: char = 'C';
const STYLE_BOLD_ITALIC: char = 'D';
const STYLE_CODE: char = 'E';
const STYLE_LINK: char = 'F';
const STYLE_HEADER1: char = 'G';
const STYLE_HEADER2: char = 'H';
const STYLE_HEADER3: char = 'I';
const STYLE_QUOTE: char = 'J';
const STYLE_LINK_HOVER: char = 'K'; // Link with hover background

fn main() {
    let args: Vec<String> = env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <filename>", args[0]);
        eprintln!("Example: {} README.md", args[0]);
        process::exit(1);
    }

    let filename = PathBuf::from(&args[1]);

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
        .with_label(&format!("ViewMD - {}", filename.display()))
        .center_screen();

    // Create text display widget with 5px padding
    let (mut text_widget, text_display) = create_text_display_widget(
        5,   // x
        5,   // y
        790, // width
        590, // height
    );

    // Set up the text buffer
    let buffer = Rc::new(RefCell::new(TextBuffer::new()));
    buffer.borrow_mut().set_text(&contents);
    text_display.borrow_mut().set_buffer(buffer.clone());

    // Extract links from the content
    let links = Rc::new(RefCell::new(extract_links(&contents)));

    // Track which link is currently being hovered (stores start and end positions)
    let hovered_link: Rc<RefCell<Option<(usize, usize)>>> = Rc::new(RefCell::new(None));

    // Set up style buffer for syntax highlighting
    let style_buffer = Rc::new(RefCell::new(TextBuffer::new()));
    let styled_text = style_markdown(&contents, &links.borrow());
    style_buffer.borrow_mut().set_text(&styled_text);
    text_display.borrow_mut().set_style_buffer(style_buffer.clone());

    // Define style table matching main application
    let style_table = vec![
        // Style A - STYLE_PLAIN
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 0,                // Helvetica
            size: DEFAULT_FONT_SIZE,
            attr: 0,
            bgcolor: 0xFFFFF5FF,    // Light yellow background (255, 255, 245)
        },
        // Style B - STYLE_BOLD
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 1,                // HelveticaBold
            size: DEFAULT_FONT_SIZE,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style C - STYLE_ITALIC
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 2,                // HelveticaItalic
            size: DEFAULT_FONT_SIZE,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style D - STYLE_BOLD_ITALIC
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 3,                // HelveticaBoldItalic
            size: DEFAULT_FONT_SIZE,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style E - STYLE_CODE
        StyleTableEntry {
            color: 0x0064C8FF,      // Blue (0, 100, 200)
            font: 4,                // Courier
            size: DEFAULT_FONT_SIZE,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style F - STYLE_LINK
        StyleTableEntry {
            color: 0x0000FFFF,      // Blue
            font: 0,                // Helvetica
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE,
            bgcolor: 0xFFFFF5FF,
        },
        // Style G - STYLE_HEADER1
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 1,                // HelveticaBold
            size: DEFAULT_FONT_SIZE + 4,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style H - STYLE_HEADER2
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 1,                // HelveticaBold
            size: DEFAULT_FONT_SIZE + 2,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style I - STYLE_HEADER3
        StyleTableEntry {
            color: 0x000000FF,      // Black
            font: 1,                // HelveticaBold
            size: DEFAULT_FONT_SIZE + 2,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style J - STYLE_QUOTE
        StyleTableEntry {
            color: 0x640000FF,      // Dark red (100, 0, 0)
            font: 10,               // TimesItalic
            size: DEFAULT_FONT_SIZE,
            attr: 0,
            bgcolor: 0xFFFFF5FF,
        },
        // Style K - STYLE_LINK_HOVER (link with gray background when hovered)
        StyleTableEntry {
            color: 0x0000FFFF,      // Blue
            font: 0,                // Helvetica
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE,
            bgcolor: 0xD3D3D3FF,    // Light gray background (211, 211, 211)
        },
    ];

    text_display.borrow_mut().set_highlight_data(style_table);

    // Configure the text display
    text_display.borrow_mut().set_textfont(0); // Helvetica
    text_display.borrow_mut().set_textsize(DEFAULT_FONT_SIZE);
    text_display.borrow_mut().set_textcolor(0x000000FF); // Black text
    text_display.borrow_mut().set_cursor_color(0x000000FF); // Black cursor
    text_display.borrow_mut().show_cursor(true);

    // Enable text wrapping at window bounds
    text_display.borrow_mut().set_wrap_mode(WrapMode::AtBounds, 0);

    // Disable line numbers (like main application)
    text_display.borrow_mut().set_linenumber_width(0);

    // Set widget color to match main application
    text_widget.set_color(enums::Color::from_rgb(255, 255, 245));
    text_widget.set_frame(enums::FrameType::FlatBox);

    // Store current file directory for resolving relative links
    let current_dir = Rc::new(
        filename
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    );

    // Handle mouse movement for cursor changes and link following
    text_widget.handle({
        let links = links.clone();
        let text_display = text_display.clone();
        let buffer = buffer.clone();
        let style_buffer = style_buffer.clone();
        let current_dir = current_dir.clone();
        let hovered_link = hovered_link.clone();
        let mut wind_clone = wind.clone();
        let mut widget_clone = text_widget.clone();

        move |widget, evt| match evt {
            enums::Event::Move => {
                let x = event_x() - widget.x();
                let y = event_y() - widget.y();
                let pos = text_display.borrow().xy_to_position(x, y, PositionType::CursorPos);

                // Check if cursor is over a link
                if let Some(link) = find_link_at_position(&links.borrow(), pos) {
                    wind_clone.set_cursor(enums::Cursor::Hand);

                    // Update hovered link and restyle if it changed
                    let new_hover = Some((link.start, link.end));
                    let mut current_hover = hovered_link.borrow_mut();
                    if *current_hover != new_hover {
                        *current_hover = new_hover;

                        // Update style buffer with hover styling
                        let content = buffer.borrow().text();
                        let styled_text = style_markdown_with_hover(&content, &links.borrow(), *current_hover);
                        style_buffer.borrow_mut().set_text(&styled_text);
                        widget_clone.redraw();
                    }
                } else {
                    wind_clone.set_cursor(enums::Cursor::Arrow);

                    // Clear hovered link if it was set
                    let mut current_hover = hovered_link.borrow_mut();
                    if current_hover.is_some() {
                        *current_hover = None;

                        // Update style buffer without hover styling
                        let content = buffer.borrow().text();
                        let styled_text = style_markdown_with_hover(&content, &links.borrow(), None);
                        style_buffer.borrow_mut().set_text(&styled_text);
                        widget_clone.redraw();
                    }
                }
                true
            }
            enums::Event::Push => {
                if event_mouse_button() == MouseButton::Left {
                    let x = event_x() - widget.x();
                    let y = event_y() - widget.y();
                    let pos = text_display.borrow().xy_to_position(x, y, PositionType::CursorPos);

                    // Check if we clicked on a link
                    if let Some(link) = find_link_at_position(&links.borrow(), pos) {
                        let link_dest = link.destination.clone();

                        // Resolve the link path relative to current file
                        let target_path = resolve_link_path(&current_dir, &link_dest);

                        // Try to load the linked file
                        match fs::read_to_string(&target_path) {
                            Ok(new_contents) => {
                                // Update buffer with new content
                                buffer.borrow_mut().set_text(&new_contents);

                                // Update links
                                let new_links = extract_links(&new_contents);
                                let styled_text = style_markdown_with_hover(&new_contents, &new_links, None);
                                style_buffer.borrow_mut().set_text(&styled_text);
                                *links.borrow_mut() = new_links;
                                *hovered_link.borrow_mut() = None;

                                // Update window title
                                wind_clone.set_label(&format!("ViewMD - {}", target_path.display()));

                                // Update current directory
                                // Note: We can't update current_dir here due to borrow rules,
                                // but for a simple viewer this is acceptable

                                app::redraw();
                            }
                            Err(e) => {
                                eprintln!("Error loading file '{}': {}", target_path.display(), e);
                                // Could show a dialog here in a more complete application
                            }
                        }
                        return true;
                    }
                }
                false
            }
            _ => false,
        }
    });

    // Handle window resize
    wind.handle({
        let mut text_widget_handle = text_widget.clone();
        move |w, event| match event {
            enums::Event::Resize => {
                // Resize the text widget (which will trigger its resize callback)
                let new_w = w.w() - 10;
                let new_h = w.h() - 10;
                text_widget_handle.resize(5, 5, new_w, new_h);
                true
            }
            _ => false,
        }
    });

    wind.make_resizable(true);
    wind.end();
    wind.show();

    text_widget.take_focus().ok();

    app.run().unwrap();
}

/// Resolve a link path relative to the current file's directory
fn resolve_link_path(current_dir: &Path, link_dest: &str) -> PathBuf {
    let mut path = current_dir.join(link_dest);

    // If no extension, try adding .md
    if path.extension().is_none() {
        path.set_extension("md");
    }

    path
}

/// Style markdown text using the same logic as the main application
fn style_markdown(content: &str, links: &[Link]) -> String {
    style_markdown_with_hover(content, links, None)
}

/// Style markdown text with optional hover highlighting
fn style_markdown_with_hover(content: &str, links: &[Link], hovered: Option<(usize, usize)>) -> String {
    let len = content.len();
    let mut styles = vec![STYLE_PLAIN as u8; len];

    // Apply line-by-line styling
    for (line_idx, line) in content.lines().enumerate() {
        let line_start = content
            .lines()
            .take(line_idx)
            .map(|l| l.len() + 1) // +1 for newline
            .sum::<usize>();

        style_line(line, line_start, &mut styles);
    }

    // Apply link styling (overrides other styles)
    for link in links {
        let style = if let Some((hover_start, hover_end)) = hovered {
            // Use hover style if this is the hovered link
            if link.start == hover_start && link.end == hover_end {
                STYLE_LINK_HOVER as u8
            } else {
                STYLE_LINK as u8
            }
        } else {
            STYLE_LINK as u8
        };

        for i in link.start..link.end.min(len) {
            styles[i] = style;
        }
    }

    // Convert to string
    styles.iter().map(|&b| b as char).collect()
}

/// Style a single line based on Markdown syntax
fn style_line(line: &str, line_start: usize, styles: &mut [u8]) {
    let line_end = line_start + line.len();

    // Headers
    if line.starts_with("# ") {
        for i in line_start..line_end {
            styles[i] = STYLE_HEADER1 as u8;
        }
        return;
    } else if line.starts_with("## ") {
        for i in line_start..line_end {
            styles[i] = STYLE_HEADER2 as u8;
        }
        return;
    } else if line.starts_with("### ") {
        for i in line_start..line_end {
            styles[i] = STYLE_HEADER3 as u8;
        }
        return;
    }

    // Blockquotes
    if line.starts_with("> ") {
        for i in line_start..line_end {
            styles[i] = STYLE_QUOTE as u8;
        }
        return;
    }

    // Code blocks (indented with 4 spaces or tab)
    if line.starts_with("    ") || line.starts_with("\t") {
        for i in line_start..line_end {
            styles[i] = STYLE_CODE as u8;
        }
        return;
    }

    // Inline styles (bold, italic, code)
    apply_inline_styles(line, line_start, styles);
}

/// Apply inline styles like **bold**, *italic*, `code`
fn apply_inline_styles(line: &str, line_start: usize, styles: &mut [u8]) {
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Code spans `code`
        if chars[i] == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '`') {
                let end_idx = i + 1 + end;
                for j in i..=end_idx {
                    if line_start + j < styles.len() {
                        styles[line_start + j] = STYLE_CODE as u8;
                    }
                }
                i = end_idx + 1;
                continue;
            }
        }

        // Bold **text**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_delimiter(&chars[i + 2..], "**") {
                let end_idx = i + 2 + end;
                for j in i..=end_idx + 1 {
                    if line_start + j < styles.len() {
                        styles[line_start + j] = STYLE_BOLD as u8;
                    }
                }
                i = end_idx + 2;
                continue;
            }
        }

        // Italic *text*
        if chars[i] == '*' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '*') {
                let end_idx = i + 1 + end;
                for j in i..=end_idx {
                    if line_start + j < styles.len() {
                        styles[line_start + j] = STYLE_ITALIC as u8;
                    }
                }
                i = end_idx + 1;
                continue;
            }
        }

        i += 1;
    }
}

/// Helper function to find a delimiter in a character slice
fn find_delimiter(chars: &[char], delim: &str) -> Option<usize> {
    let delim_chars: Vec<char> = delim.chars().collect();
    let delim_len = delim_chars.len();

    for i in 0..chars.len() {
        if i + delim_len <= chars.len() {
            if chars[i..i + delim_len] == delim_chars[..] {
                return Some(i);
            }
        }
    }
    None
}
