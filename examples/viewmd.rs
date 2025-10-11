// viewmd - Markdown file viewer with clickable links
// Usage: cargo run --example viewmd <filename>

use fliki_rs::fltk_rich_text_display::create_rich_text_display_widget;
use fliki_rs::fltk_text_display::create_text_display_widget;
use fliki_rs::link_handler::{extract_links, find_link_at_position, Link};
use fliki_rs::text_buffer::TextBuffer;
use fliki_rs::text_display::{style_attr, PositionType, StyleTableEntry, WrapMode};
use fltk::app::{event_mouse_button, event_x, event_y, MouseButton};
use fltk::{prelude::*, *};
use pulldown_cmark::{Event, Parser, Tag, TagEnd};
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
        eprintln!("Usage: {} [--ast] [--edit] <filename>", args[0]);
        eprintln!("Example: {} README.md", args[0]);
        eprintln!("         {} --ast README.md  (use AST-based rendering)", args[0]);
        eprintln!("         {} --ast --edit README.md  (enable editing)", args[0]);
        process::exit(1);
    }

    // Check for --ast and --edit flags
    let use_ast = args.contains(&"--ast".to_string());
    let edit_mode = args.contains(&"--edit".to_string());
    let filename_arg = args.iter()
        .skip(1)
        .find(|a| *a != "--ast" && *a != "--edit")
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
            "ViewMD{} - {}",
            if use_ast { " (AST)" } else { "" },
            filename.display()
        ))
        .center_screen();

    if use_ast {
        // Use AST-based rendering
        run_ast_viewer(wind, filename, contents, edit_mode);
    } else {
        // Use original buffer-based rendering
        run_buffer_viewer(wind, filename, contents);
    }

    app.run().unwrap();
}

/// Run the original buffer-based viewer
fn run_buffer_viewer(mut wind: window::Window, filename: PathBuf, contents: String) {
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
    text_display
        .borrow_mut()
        .set_style_buffer(style_buffer.clone());

    // Define style table matching main application
    let style_table = vec![
        // Style A - STYLE_PLAIN
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 0,           // Helvetica
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF, // Light yellow background (255, 255, 245)
        },
        // Style B - STYLE_BOLD
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 1,           // HelveticaBold
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style C - STYLE_ITALIC
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 2,           // HelveticaItalic
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style D - STYLE_BOLD_ITALIC
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 3,           // HelveticaBoldItalic
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style E - STYLE_CODE
        StyleTableEntry {
            color: 0x0064C8FF, // Blue (0, 100, 200)
            font: 4,           // Courier
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style F - STYLE_LINK
        StyleTableEntry {
            color: 0x0000FFFF, // Blue
            font: 0,           // Helvetica
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style G - STYLE_HEADER1
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 1,           // HelveticaBold
            size: DEFAULT_FONT_SIZE + 4,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style H - STYLE_HEADER2
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 1,           // HelveticaBold
            size: DEFAULT_FONT_SIZE + 2,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style I - STYLE_HEADER3
        StyleTableEntry {
            color: 0x000000FF, // Black
            font: 1,           // HelveticaBold
            size: DEFAULT_FONT_SIZE + 2,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style J - STYLE_QUOTE
        StyleTableEntry {
            color: 0x640000FF, // Dark red (100, 0, 0)
            font: 10,          // TimesItalic
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::BGCOLOR,
            bgcolor: 0xFFFFF5FF,
        },
        // Style K - STYLE_LINK_HOVER (link with gray background when hovered)
        StyleTableEntry {
            color: 0x0000FFFF, // Blue
            font: 0,           // Helvetica
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: 0xD3D3D3FF, // Light gray background (211, 211, 211)
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
    text_display
        .borrow_mut()
        .set_wrap_mode(WrapMode::AtBounds, 0);

    // Disable line numbers (like main application)
    text_display.borrow_mut().set_linenumber_width(0);

    // Set padding: 10px vertical, 25px horizontal
    text_display.borrow_mut().set_padding_vertical(10);
    text_display.borrow_mut().set_padding_horizontal(25);

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
                let pos = text_display
                    .borrow()
                    .xy_to_position(x, y, PositionType::CursorPos);

                // Check if cursor is over a link
                if let Some(link) = find_link_at_position(&links.borrow(), pos) {
                    wind_clone.set_cursor(enums::Cursor::Hand);

                    // Update hovered link and restyle if it changed
                    let new_hover = Some((link.start, link.end));
                    let should_update = {
                        let mut current_hover = hovered_link.borrow_mut();
                        let needs_update = *current_hover != new_hover;
                        if needs_update {
                            *current_hover = new_hover;
                        }
                        needs_update
                    }; // Borrow is dropped here

                    if should_update {
                        // Now safe to restyle and redraw without holding the borrow
                        let content = buffer.borrow().text();
                        let styled_text = style_markdown_with_hover(
                            &content,
                            &links.borrow(),
                            *hovered_link.borrow(),
                        );
                        style_buffer.borrow_mut().set_text(&styled_text);
                        widget_clone.redraw();
                    }
                } else {
                    wind_clone.set_cursor(enums::Cursor::Arrow);

                    // Clear hovered link if it was set
                    let should_update = {
                        let mut current_hover = hovered_link.borrow_mut();
                        let needs_update = current_hover.is_some();
                        if needs_update {
                            *current_hover = None;
                        }
                        needs_update
                    }; // Borrow is dropped here

                    if should_update {
                        // Now safe to restyle and redraw without holding the borrow
                        let content = buffer.borrow().text();
                        let styled_text =
                            style_markdown_with_hover(&content, &links.borrow(), None);
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
                    let pos = text_display
                        .borrow()
                        .xy_to_position(x, y, PositionType::CursorPos);

                    // Check if we clicked on a link - extract link destination and drop borrow
                    let link_dest = {
                        let links_borrow = links.borrow();
                        find_link_at_position(&links_borrow, pos)
                            .map(|link| link.destination.clone())
                    }; // Borrow is dropped here

                    if let Some(link_dest) = link_dest {
                        // Resolve the link path relative to current file
                        let target_path = resolve_link_path(&current_dir, &link_dest);

                        // Try to load the linked file
                        match fs::read_to_string(&target_path) {
                            Ok(new_contents) => {
                                // Update buffer with new content
                                buffer.borrow_mut().set_text(&new_contents);

                                // Update links
                                let new_links = extract_links(&new_contents);
                                let styled_text =
                                    style_markdown_with_hover(&new_contents, &new_links, None);
                                style_buffer.borrow_mut().set_text(&styled_text);
                                *links.borrow_mut() = new_links;
                                *hovered_link.borrow_mut() = None;

                                // Update window title
                                wind_clone
                                    .set_label(&format!("ViewMD - {}", target_path.display()));

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
}

/// Run the AST-based viewer
fn run_ast_viewer(mut wind: window::Window, filename: PathBuf, contents: String, edit_mode: bool) {
    // Create rich text display widget
    let (mut rich_widget, rich_display) = create_rich_text_display_widget(
        5,   // x
        5,   // y
        790, // width
        590, // height
    );

    // Set markdown content
    rich_display.borrow_mut().set_markdown(&contents);

    // Set up style table (same as buffer-based version)
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
        // Style 10 - STYLE_LINK_HOVER (link with gray background when hovered)
        StyleTableEntry {
            color: 0x0000FFFF,
            font: 0,
            size: DEFAULT_FONT_SIZE,
            attr: style_attr::UNDERLINE | style_attr::BGCOLOR,
            bgcolor: 0xD3D3D3FF, // Light gray background (211, 211, 211)
        },
    ];

    rich_display.borrow_mut().set_style_table(style_table);
    rich_display.borrow_mut().set_padding(10, 10, 25, 25);

    // Configure cursor visibility based on edit mode
    rich_display.borrow_mut().set_cursor_visible(edit_mode);

    // Set widget color
    rich_widget.set_color(enums::Color::from_rgb(255, 255, 245));
    rich_widget.set_frame(enums::FrameType::FlatBox);

    // Store current file directory for resolving relative links
    let current_dir = Rc::new(
        filename
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from(".")),
    );

    // Handle mouse events for hover and clicking
    rich_widget.handle({
        let rich_display = rich_display.clone();
        let current_dir = current_dir.clone();
        let mut wind_clone = wind.clone();
        let mut widget_clone = rich_widget.clone();
        let edit_mode = edit_mode; // Capture edit_mode in closure

        move |widget, evt| {
            // First, handle basic widget events (these were in the built-in handler)
            match evt {
                enums::Event::Push => {
                    widget.take_focus().ok();
                    // Continue to handle other Push logic below
                }
                enums::Event::Focus | enums::Event::Unfocus => {
                    widget.redraw();
                    return true;
                }
                _ => {}
            }

            // Now handle specific events
            match evt {
            enums::Event::Move => {
                let x = event_x() - widget.x();
                let y = event_y() - widget.y();

                // Check if cursor is over a link (extract result before mutable borrow)
                let link_info = rich_display.borrow().find_link_at(x, y);

                if let Some((node_id, _destination)) = link_info {
                    wind_clone.set_cursor(enums::Cursor::Hand);
                    rich_display.borrow_mut().set_hovered_link(Some(node_id));
                    widget_clone.redraw();
                } else {
                    wind_clone.set_cursor(enums::Cursor::Arrow);
                    let has_hover = rich_display.borrow().hovered_link().is_some();
                    if has_hover {
                        rich_display.borrow_mut().set_hovered_link(None);
                        widget_clone.redraw();
                    }
                }
                true
            }
            enums::Event::Push => {
                if event_mouse_button() == MouseButton::Left {
                    let x = event_x() - widget.x();
                    let y = event_y() - widget.y();

                    // Check if we clicked on a link (extract result before mutable borrow)
                    let link_info = rich_display.borrow().find_link_at(x, y);

                    if let Some((_node_id, destination)) = link_info {
                        // Resolve the link path relative to current file
                        let target_path = resolve_link_path(&current_dir, &destination);

                        // Try to load the linked file
                        match fs::read_to_string(&target_path) {
                            Ok(new_contents) => {
                                // Update display with new content
                                rich_display.borrow_mut().set_markdown(&new_contents);
                                rich_display.borrow_mut().set_hovered_link(None);
                                rich_display.borrow_mut().set_cursor_pos(0);

                                // Update window title
                                wind_clone.set_label(&format!("ViewMD (AST) - {}", target_path.display()));

                                app::redraw();
                            }
                            Err(e) => {
                                eprintln!("Error loading file '{}': {}", target_path.display(), e);
                            }
                        }
                        return true;
                    } else if edit_mode {
                        // No link clicked - position cursor at click location (only in edit mode)
                        let pos = rich_display.borrow().xy_to_position(x, y);
                        rich_display.borrow_mut().set_cursor_pos(pos);
                        widget_clone.redraw();
                        return true;
                    }
                }
                false
            }
            enums::Event::KeyDown if edit_mode => {
                // Handle keyboard input for editing (only in edit mode)
                let key = app::event_key();
                let text_input = app::event_text();

                match key {
                    enums::Key::BackSpace => {
                        // Delete character before cursor
                        let result = rich_display.borrow().delete_before_cursor();
                        if let Some((new_text, new_pos)) = result {
                            let mut display = rich_display.borrow_mut();
                            display.set_markdown(&new_text);
                            display.set_cursor_pos(new_pos);
                            widget_clone.redraw();
                        }
                        true
                    }
                    enums::Key::Delete => {
                        // Delete character at cursor
                        let result = rich_display.borrow().delete_at_cursor();
                        if let Some((new_text, new_pos)) = result {
                            let mut display = rich_display.borrow_mut();
                            display.set_markdown(&new_text);
                            display.set_cursor_pos(new_pos);
                            widget_clone.redraw();
                        }
                        true
                    }
                    enums::Key::Left => {
                        // Move cursor left
                        let mut display = rich_display.borrow_mut();
                        let pos = display.cursor_pos();
                        if pos > 0 {
                            display.set_cursor_pos(pos - 1);
                            widget_clone.redraw();
                        }
                        true
                    }
                    enums::Key::Right => {
                        // Move cursor right
                        let mut display = rich_display.borrow_mut();
                        let pos = display.cursor_pos();
                        if let Some(doc) = display.document() {
                            if pos < doc.source.len() {
                                display.set_cursor_pos(pos + 1);
                                widget_clone.redraw();
                            }
                        }
                        true
                    }
                    enums::Key::Up => {
                        // Move cursor up one line
                        // TODO: Implement line-based cursor movement
                        false
                    }
                    enums::Key::Down => {
                        // Move cursor down one line
                        // TODO: Implement line-based cursor movement
                        false
                    }
                    enums::Key::Home => {
                        // Move cursor to start of line
                        // TODO: Implement start-of-line positioning
                        false
                    }
                    enums::Key::End => {
                        // Move cursor to end of line
                        // TODO: Implement end-of-line positioning
                        false
                    }
                    enums::Key::Enter => {
                        // Insert newline with smart list handling
                        let result = rich_display.borrow().insert_text_at_cursor("\n");
                        if let Some((new_text, new_pos)) = result {
                            let mut display = rich_display.borrow_mut();
                            display.set_markdown(&new_text);
                            display.set_cursor_pos(new_pos);
                            widget_clone.redraw();
                        }
                        true
                    }
                    _ => {
                        // Handle regular text input
                        if !text_input.is_empty() {
                            let result = rich_display.borrow().insert_text_at_cursor(&text_input);
                            if let Some((new_text, new_pos)) = result {
                                let mut display = rich_display.borrow_mut();
                                display.set_markdown(&new_text);
                                display.set_cursor_pos(new_pos);
                                widget_clone.redraw();
                            }
                            true
                        } else {
                            false
                        }
                    }
                }
            }
            _ => false,
            }
        }
    });

    // Handle window resize
    wind.handle({
        let mut widget_handle = rich_widget.clone();
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

    rich_widget.take_focus().ok();
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

/// Style markdown text with optional hover highlighting using pulldown-cmark parser
fn style_markdown_with_hover(
    content: &str,
    links: &[Link],
    hovered: Option<(usize, usize)>,
) -> String {
    let len = content.len();
    let mut styles = vec![STYLE_PLAIN as u8; len];

    // Use pulldown-cmark parser to apply styles
    let parser = Parser::new(content);

    // Track style state stack for nested elements
    let mut style_stack: Vec<u8> = Vec::new();

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(tag) => {
                let style = match tag {
                    Tag::Heading { level, .. } => match level {
                        pulldown_cmark::HeadingLevel::H1 => STYLE_HEADER1 as u8,
                        pulldown_cmark::HeadingLevel::H2 => STYLE_HEADER2 as u8,
                        pulldown_cmark::HeadingLevel::H3 => STYLE_HEADER3 as u8,
                        _ => STYLE_HEADER3 as u8, // H4+ use H3 style
                    },
                    Tag::BlockQuote(_) => STYLE_QUOTE as u8,
                    Tag::CodeBlock(_) => STYLE_CODE as u8,
                    Tag::Emphasis => STYLE_ITALIC as u8,
                    Tag::Strong => STYLE_BOLD as u8,
                    Tag::Link { .. } => STYLE_LINK as u8,
                    _ => STYLE_PLAIN as u8,
                };
                style_stack.push(style);
            }
            Event::End(tag_end) => {
                match tag_end {
                    TagEnd::Heading(_) | TagEnd::BlockQuote(_) | TagEnd::CodeBlock
                    | TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link => {
                        style_stack.pop();
                    }
                    _ => {}
                }
            }
            Event::Text(_) | Event::Code(_) => {
                // Apply the current style from the stack
                let current_style = if matches!(event, Event::Code(_)) {
                    STYLE_CODE as u8
                } else {
                    style_stack.last().copied().unwrap_or(STYLE_PLAIN as u8)
                };

                for i in range.start..range.end.min(len) {
                    styles[i] = current_style;
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                // Line breaks keep plain style
                for i in range.start..range.end.min(len) {
                    styles[i] = STYLE_PLAIN as u8;
                }
            }
            _ => {}
        }
    }

    // Apply link styling with hover support (overrides other styles)
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
