// viewmd_structured - Markdown file viewer with structured editor
// Usage: cargo run --example viewmd_structured [--edit] <filename>

use fliki_rs::fltk_structured_rich_display::FltkStructuredRichDisplay;
use fliki_rs::richtext::markdown_converter::markdown_to_document;
use fliki_rs::richtext::structured_document::DocumentPosition;
use fltk::{prelude::*, *};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::time::Instant;

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
        let widget_for_menu = display_widget.clone();
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

        // Strikethrough (Cmd/Ctrl-Shift-X)
        #[cfg(target_os = "macos")]
        let strike_shortcut = enums::Shortcut::Command | enums::Shortcut::Shift | 'x';
        #[cfg(not(target_os = "macos"))]
        let strike_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Shift | 'x';
        menu_bar.add(
            "Format/Strikethrough\t",
            strike_shortcut,
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

        // Highlight (Cmd/Ctrl-Shift-H)
        #[cfg(target_os = "macos")]
        let highlight_shortcut = enums::Shortcut::Command | enums::Shortcut::Shift | 'h';
        #[cfg(not(target_os = "macos"))]
        let highlight_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Shift | 'h';
        menu_bar.add(
            "Format/Highlight\t",
            highlight_shortcut,
            menu::MenuFlag::Normal,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display.borrow_mut().editor_mut().toggle_highlight().ok();
                }
            },
        );

        // Code (Cmd/Ctrl-Shift-C)
        #[cfg(target_os = "macos")]
        let code_shortcut = enums::Shortcut::Command | enums::Shortcut::Shift | 'c';
        #[cfg(not(target_os = "macos"))]
        let code_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Shift | 'c';
        menu_bar.add("Format/Code\t", code_shortcut, menu::MenuFlag::Normal, {
            let display = display_for_menu.clone();
            move |_| {
                display.borrow_mut().editor_mut().toggle_code().ok();
            }
        });

        // Edit Link (Cmd/Ctrl-K)
        #[cfg(target_os = "macos")]
        let edit_link_shortcut = enums::Shortcut::Command | 'k';
        #[cfg(not(target_os = "macos"))]
        let edit_link_shortcut = enums::Shortcut::Ctrl | 'k';
        let display_for_menu = display.clone();
        menu_bar.add(
            "Format/Edit Link…\t",
            edit_link_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                // Determine initial values
                let (init_target, init_text, mode_existing_link, selection_mode, link_pos) = {
                    let d = display_for_menu.borrow_mut();
                    if let Some((b, i)) = d.hovered_link() {
                        let doc = d.editor().document();
                        let block = &doc.blocks()[b];
                        if let fliki_rs::richtext::structured_document::InlineContent::Link {
                            link,
                            content,
                        } = &block.content[i]
                        {
                            let text = content
                                .iter()
                                .map(|c| c.to_plain_text())
                                .collect::<String>();
                            (link.destination.clone(), text, true, false, Some((b, i)))
                        } else {
                            (String::new(), String::new(), false, false, None)
                        }
                    } else if let Some((a, b)) = d.editor().selection() {
                        let text = d.editor().text_in_range(a, b);
                        (String::new(), text, false, true, None)
                    } else {
                        (String::new(), String::new(), false, false, None)
                    }
                };

                // Determine center rectangle from parent widget (fallback to screen handled by helper)
                let dw_for_center = widget_for_menu.clone();
                let parent = dw_for_center.parent().unwrap_or(dw_for_center.clone());
                let center_rect = Some((parent.x(), parent.y(), parent.w(), parent.h()));

                let opts = fliki_rs::link_editor::LinkEditOptions {
                    init_target,
                    init_text: init_text.clone(),
                    mode_existing_link,
                    selection_mode,
                    center_rect,
                };

                // Invoke shared link editor dialog
                let display_cb = display_for_menu.clone();
                let redraw_handle = widget_for_menu.clone();
                fliki_rs::link_editor::show_link_editor(
                    opts,
                    move |dest: String, txt: String| {
                        let mut d = display_cb.borrow_mut();
                        let editor = d.editor_mut();
                        if let Some((b, i)) = link_pos {
                            editor.edit_link_at(b, i, &dest, &txt).ok();
                        } else if !txt.is_empty() {
                            if editor.selection().is_some() {
                                editor.replace_selection_with_link(&dest, &txt).ok();
                            } else {
                                editor.insert_link_at_cursor(&dest, &txt).ok();
                            }
                        }
                        drop(d);
                        let mut w_local = redraw_handle.clone();
                        w_local.redraw();
                    },
                    Some({
                        let display_rm = display_for_menu.clone();
                        let redraw_rm = widget_for_menu.clone();
                        move || {
                            if let Some((b, i)) = link_pos {
                                let mut d = display_rm.borrow_mut();
                                d.editor_mut().remove_link_at(b, i).ok();
                                drop(d);
                            }
                            let mut w_local = redraw_rm.clone();
                            w_local.redraw();
                        }
                    }),
                );
            },
        );

        // Bullet list toggle (Cmd/Ctrl-Shift-8) under Paragraph Style
        #[cfg(target_os = "macos")]
        let list_shortcut = enums::Shortcut::Command | enums::Shortcut::Shift | '8';
        #[cfg(not(target_os = "macos"))]
        let list_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Shift | '8';
        menu_bar.add(
            "Paragraph Style/List Item\t",
            list_shortcut,
            menu::MenuFlag::Radio,
            {
                let display = display.clone();
                move |_| {
                    display
                        .borrow_mut()
                        .editor_mut()
                        .set_block_type(
                            fliki_rs::richtext::structured_document::BlockType::ListItem {
                                ordered: false,
                                number: None,
                                checkbox: None,
                            },
                        )
                        .ok();
                }
            },
        );

        // Paragraph Style (Cmd/Ctrl-Alt-0..3)
        #[cfg(target_os = "macos")]
        let para_shortcut = enums::Shortcut::Command | enums::Shortcut::Alt | '0';
        #[cfg(not(target_os = "macos"))]
        let para_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Alt | '0';

        #[cfg(target_os = "macos")]
        let h1_shortcut = enums::Shortcut::Command | enums::Shortcut::Alt | '1';
        #[cfg(not(target_os = "macos"))]
        let h1_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Alt | '1';

        #[cfg(target_os = "macos")]
        let h2_shortcut = enums::Shortcut::Command | enums::Shortcut::Alt | '2';
        #[cfg(not(target_os = "macos"))]
        let h2_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Alt | '2';

        #[cfg(target_os = "macos")]
        let h3_shortcut = enums::Shortcut::Command | enums::Shortcut::Alt | '3';
        #[cfg(not(target_os = "macos"))]
        let h3_shortcut = enums::Shortcut::Ctrl | enums::Shortcut::Alt | '3';

        let display_for_menu = display.clone();
        menu_bar.add(
            "Paragraph Style/Paragraph\t",
            para_shortcut,
            menu::MenuFlag::Radio,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display
                        .borrow_mut()
                        .editor_mut()
                        .set_block_type(
                            fliki_rs::richtext::structured_document::BlockType::Paragraph,
                        )
                        .ok();
                }
            },
        );
        let display_for_menu = display.clone();
        menu_bar.add(
            "Paragraph Style/Heading 1\t",
            h1_shortcut,
            menu::MenuFlag::Radio,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display
                        .borrow_mut()
                        .editor_mut()
                        .set_block_type(
                            fliki_rs::richtext::structured_document::BlockType::Heading {
                                level: 1,
                            },
                        )
                        .ok();
                }
            },
        );
        let display_for_menu = display.clone();
        menu_bar.add(
            "Paragraph Style/Heading 2\t",
            h2_shortcut,
            menu::MenuFlag::Radio,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display
                        .borrow_mut()
                        .editor_mut()
                        .set_block_type(
                            fliki_rs::richtext::structured_document::BlockType::Heading {
                                level: 2,
                            },
                        )
                        .ok();
                }
            },
        );
        let display_for_menu = display.clone();
        menu_bar.add(
            "Paragraph Style/Heading 3\t",
            h3_shortcut,
            menu::MenuFlag::Radio,
            {
                let display = display_for_menu.clone();
                move |_| {
                    display
                        .borrow_mut()
                        .editor_mut()
                        .set_block_type(
                            fliki_rs::richtext::structured_document::BlockType::Heading {
                                level: 3,
                            },
                        )
                        .ok();
                }
            },
        );

        // Keep paragraph style radio selection in sync with cursor
        {
            use fliki_rs::richtext::structured_document::BlockType;
            let mb = menu_bar.clone();
            let disp = display.clone();
            app::add_timeout3(0.25, move |h| {
                let current = {
                    let d = disp.borrow();
                    let ed = d.editor();
                    let cur = ed.cursor();
                    let doc = ed.document();
                    let blocks = doc.blocks();
                    if !blocks.is_empty() && (cur.block_index as usize) < blocks.len() {
                        blocks[cur.block_index as usize].block_type.clone()
                    } else {
                        BlockType::Paragraph
                    }
                };

                if let Some(selected) = match current {
                    BlockType::Paragraph => Some("Paragraph Style/Paragraph\t"),
                    BlockType::Heading { level } => match level {
                        1 => Some("Paragraph Style/Heading 1\t"),
                        2 => Some("Paragraph Style/Heading 2\t"),
                        3 => Some("Paragraph Style/Heading 3\t"),
                        _ => None,
                    },
                    BlockType::ListItem {
                        ordered, checkbox, ..
                    } => Some(if ordered {
                        "Paragraph Style/Numbered List\t"
                    } else if checkbox.is_some() {
                        "Paragraph Style/List Item\t"
                    } else {
                        "Paragraph Style/List Item\t"
                    }),
                    _ => None,
                } {
                    if let Some(mut item) = mb.find_item(selected) {
                        item.set();
                    }
                }

                app::repeat_timeout3(0.25, h);
            });
        }

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
    }

    // Convert markdown to structured document
    let doc = markdown_to_document(&contents);
    {
        let mut d = display.borrow_mut();
        *d.editor_mut().document_mut() = doc;
        d.editor_mut().set_cursor(DocumentPosition::start());
    }

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
