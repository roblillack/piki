// viewmd_structured - Markdown file viewer with structured editor
// Usage: cargo run --example viewmd_structured [--edit] <filename>

use fliki_rs::fltk_structured_rich_display::FltkStructuredRichDisplay;
use fliki_rs::richtext::markdown_converter::{document_to_markdown, markdown_to_document};
use fliki_rs::richtext::structured_document::DocumentPosition;
use fliki_rs::sourceedit::text_display::{style_attr, StyleTableEntry};
use fltk::{prelude::*, *};
use std::time::Instant;
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

const DEFAULT_FONT_SIZE: u8 = 14;
const HIGHLIGHT_COLOR: u32 = 0xFFFF00FF; // Yellow highlight

/// Build a complete style table including all text decoration combinations
fn build_style_table() -> Vec<StyleTableEntry> {
    let mut styles = Vec::new();

    // Styles 0-10: Base styles (existing)
    styles.extend_from_slice(&[
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
    ]);

    // Styles 11-42: Computed decorated styles
    // Formula: 11 + (base * 8) + decoration_flags
    // where base = 0 (plain), 1 (bold), 2 (italic), 3 (bold+italic)
    // and decoration_flags = (underline ? 1 : 0) | (strikethrough ? 2 : 0) | (highlight ? 4 : 0)

    let base_fonts = [0, 1, 2, 3]; // plain, bold, italic, bold+italic

    for base in 0..4 {
        for decoration in 1..8 {
            // Skip 0 (no decorations)
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

    styles
}

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

    // Determine menu height
    let menu_height = if edit_mode { 25 } else { 0 };

    // Create structured rich display widget
    let structured = FltkStructuredRichDisplay::new(
        5,                 // x
        5 + menu_height,   // y
        790,               // width
        590 - menu_height, // height
        edit_mode,         // edit mode
    );
    let mut display_widget = structured.group.clone();
    let display = structured.display.clone();

    // Create menu bar if in edit mode (after display is created)
    let mut format_menu: Option<menu::MenuBar> = None;

    if edit_mode {
        let mut menu_bar = menu::MenuBar::new(0, 0, 800, 25, None);

        #[cfg(target_os = "macos")]
        let bold_shortcut = enums::Shortcut::Command | 'b';
        #[cfg(not(target_os = "macos"))]
        let bold_shortcut = enums::Shortcut::Ctrl | 'b';

        #[cfg(target_os = "macos")]
        let italic_shortcut = enums::Shortcut::Command | 'i';
        #[cfg(not(target_os = "macos"))]
        let italic_shortcut = enums::Shortcut::Ctrl | 'i';

        #[cfg(target_os = "macos")]
        let underline_shortcut = enums::Shortcut::Command | 'u';
        #[cfg(not(target_os = "macos"))]
        let underline_shortcut = enums::Shortcut::Ctrl | 'u';

        let display_for_menu = display.clone();
        menu_bar.add("Format/Bold\t", bold_shortcut, menu::MenuFlag::Normal, {
            let display = display_for_menu.clone();
            move |_| {
                display.borrow_mut().editor_mut().toggle_bold().ok();
            }
        });

        menu_bar.add(
            "Format/Italic\t",
            italic_shortcut,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display.borrow_mut().editor_mut().toggle_italic().ok();
                }
            },
        );

        menu_bar.add(
            "Format/Underline\t",
            underline_shortcut,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display.borrow_mut().editor_mut().toggle_underline().ok();
                }
            },
        );

        menu_bar.add(
            "Format/Strikethrough",
            enums::Shortcut::None,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display
                        .borrow_mut()
                        .editor_mut()
                        .toggle_strikethrough()
                        .ok();
                }
            },
        );

        menu_bar.add(
            "Format/Highlight",
            enums::Shortcut::None,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display.borrow_mut().editor_mut().toggle_highlight().ok();
                }
            },
        );

        menu_bar.add(
            "Format/Code",
            enums::Shortcut::None,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display.borrow_mut().editor_mut().toggle_code().ok();
                }
            },
        );

        // Clear Formatting (Cmd/Ctrl-\)
        #[cfg(target_os = "macos")]
        let clear_shortcut = enums::Shortcut::Command | '\\';
        #[cfg(not(target_os = "macos"))]
        let clear_shortcut = enums::Shortcut::Ctrl | '\\';

        menu_bar.add(
            "Format/Clear Formatting\t",
            clear_shortcut,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display.borrow_mut().editor_mut().clear_formatting().ok();
                }
            },
        );

        format_menu = Some(menu_bar);
    }

    // Convert markdown to structured document
    let doc = markdown_to_document(&contents);
    {
        let mut d = display.borrow_mut();
        *d.editor_mut().document_mut() = doc;
        d.editor_mut().set_cursor(DocumentPosition::start());
    }

    // Set up style table with all text decoration combinations
    let style_table = build_style_table();
    display.borrow_mut().set_style_table(style_table);
    display.borrow_mut().set_padding(10, 10, 25, 25);

    // Set widget color
    display_widget.set_color(enums::Color::from_rgb(255, 255, 245));
    display_widget.set_frame(enums::FrameType::FlatBox);

    // Handle window resize (focus-based cursor handled internally)
    wind.handle({
        let mut widget_handle = display_widget.clone();
        let menu_h = menu_height;
        move |w, event| match event {
            enums::Event::Resize => {
                let new_w = w.w() - 10;
                let new_h = w.h() - 10 - menu_h;
                widget_handle.resize(5, 5 + menu_h, new_w, new_h);
                true
            }
            _ => false,
        }
    });

    wind.make_resizable(true);
    wind.end();
    wind.show();

    display_widget.take_focus().ok();
    structured.set_change_callback(Some(Box::new(|| println!("change!"))));

    // Register link click callback to load markdown files
    {
        let display_ref = display.clone();
        let mut win_handle = wind.clone();
        structured.set_link_callback(Some(Box::new(move |destination: String| {
            use std::path::Path;
            let path = Path::new(&destination);
            let is_markdown = path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
                .unwrap_or(false);

            if is_markdown && (path.is_relative() || path.exists()) {
                if let Ok(content) = fs::read_to_string(&destination) {
                    let new_doc = markdown_to_document(&content);
                    let mut d = display_ref.borrow_mut();
                    *d.editor_mut().document_mut() = new_doc;
                    d.editor_mut().set_cursor(DocumentPosition::start());
                    if let Some(mut win) = win_handle.as_base_widget().window() {
                        win.set_label(&format!(
                            "ViewMD (Structured{}) - {}",
                            if edit_mode { " Edit" } else { "" },
                            destination
                        ));
                    }
                    app::redraw();
                }
            } else {
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&destination).spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open")
                    .arg(&destination)
                    .spawn();
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("cmd")
                    .args(&["/C", "start", &destination])
                    .spawn();
            }
        })));
    }

    // Blink/tick timer for cursor
    {
        let start = Instant::now();
        let display_for_tick = display.clone();
        let mut widget_for_tick = display_widget.clone();
        app::add_timeout3(0.1, move |handle| {
            let ms = start.elapsed().as_millis() as u64;
            let changed = display_for_tick.borrow_mut().tick(ms);
            if changed {
                widget_for_tick.redraw();
            }
            app::repeat_timeout3(0.1, handle);
        });
    }

    app.run().unwrap();
}
