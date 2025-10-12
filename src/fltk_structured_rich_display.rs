// FLTK integration for StructuredRichDisplay widget

use crate::fltk_text_display::FltkDrawContext;
use crate::structured_document::InlineContent;
use crate::structured_rich_display::StructuredRichDisplay;
use fltk::{enums::*, prelude::*, valuator::Scrollbar};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// Create a Structured Rich Text Display widget with scrollbar
pub fn create_structured_rich_display_widget(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    edit_mode: bool,
) -> (fltk::group::Group, Rc<RefCell<StructuredRichDisplay>>) {
    let mut widget = fltk::group::Group::new(x, y, w, h, None);

    let scrollbar_size = 15;

    // Create structured rich display
    let display = Rc::new(RefCell::new(StructuredRichDisplay::new(
        x,
        y,
        w - scrollbar_size,
        h,
    )));

    // Track click count for triple-click detection
    let last_click_time = Rc::new(RefCell::new(Instant::now()));
    let last_click_count = Rc::new(RefCell::new(0));

    // Set cursor visibility based on edit mode
    display.borrow_mut().set_cursor_visible(edit_mode);

    // Create vertical scrollbar
    let mut vscroll = Scrollbar::default()
        .with_pos(x + w - scrollbar_size, y)
        .with_size(scrollbar_size, h);
    vscroll.set_type(fltk::valuator::ScrollbarType::Vertical);
    vscroll.set_callback({
        let display = display.clone();
        let mut widget_clone = widget.clone();
        move |s| {
            let value = s.value() as i32;
            display.borrow_mut().set_scroll(value);
            widget_clone.redraw();
        }
    });
    vscroll.show();

    // Initialize scrollbar
    vscroll.set_bounds(0.0, 1000.0);
    vscroll.set_slider_size(0.5);
    vscroll.set_step(1.0, 10);
    vscroll.set_value(0.0);

    widget.draw({
        let display = display.clone();
        let mut vscroll_draw = vscroll.clone();
        move |w| {
            let mut disp = display.borrow_mut();
            let has_focus = fltk::app::focus()
                .map(|f| f.as_base_widget())
                .as_ref()
                == Some(&w.as_base_widget());
            let is_active = w.active();
            let mut ctx = FltkDrawContext::new(has_focus, is_active);

            // Update scrollbar based on content
            let content_height = disp.content_height();
            let visible_height = disp.h();

            if content_height > 0 {
                let max_scroll = (content_height - visible_height).max(0) as f64;
                let slider_fraction = if content_height > 0 {
                    (visible_height as f64 / content_height as f64).min(1.0) as f32
                } else {
                    1.0
                };

                vscroll_draw.set_bounds(0.0, max_scroll);
                vscroll_draw.set_slider_size(slider_fraction);
                vscroll_draw.set_value(disp.scroll_offset() as f64);
            }

            // Draw the display
            disp.draw(&mut ctx);

            // Draw children (scrollbar)
            w.draw_children();
        }
    });

    widget.handle({
        let display = display.clone();
        let mut vscroll_handle = vscroll.clone();
        let click_time = last_click_time.clone();
        let click_count = last_click_count.clone();
        move |w, event| {
            // Handle hover checking for Push, Drag, Move, and Enter
            let check_hover = matches!(event, Event::Push | Event::Drag | Event::Move | Event::Enter);

            if check_hover {
                let x = fltk::app::event_x();
                let y = fltk::app::event_y();
                let mut d = display.borrow_mut();

                let result = d.find_link_at(x - w.x(), y - w.y());

                if let Some((link_id, _dest)) = result {
                    d.set_hovered_link(Some(link_id));
                    if let Some(mut win) = w.window() {
                        win.set_cursor(Cursor::Hand);
                    }
                    w.redraw();
                } else {
                    if d.hovered_link().is_some() {
                        d.set_hovered_link(None);
                        if let Some(mut win) = w.window() {
                            win.set_cursor(Cursor::Default);
                        }
                        w.redraw();
                    }
                }
            }

            match event {
            Event::Push => {
                let x = fltk::app::event_x();
                let y = fltk::app::event_y();

                // Detect click count (FLTK event_clicks() returns true for multi-click)
                let is_multi_click = fltk::app::event_clicks();

                // Track triple-click using time-based detection
                let current_time = Instant::now();
                let mut last_time = click_time.borrow_mut();
                let mut last_count = click_count.borrow_mut();

                let time_diff = current_time.duration_since(*last_time);
                let effective_clicks = if time_diff.as_millis() < 500 {
                    // Within multi-click time window (500ms)
                    if is_multi_click {
                        // This is a multi-click event - increment from last count
                        if *last_count == 0 || *last_count == 1 {
                            // First multi-click in sequence = double-click
                            *last_count = 2;
                            2
                        } else {
                            // Already had a double-click, increment to triple or more
                            *last_count += 1;
                            *last_count
                        }
                    } else {
                        // Single click within time window
                        *last_count = 1;
                        1
                    }
                } else {
                    // Too much time passed, reset
                    *last_count = if is_multi_click { 2 } else { 1 };
                    *last_count
                };

                *last_time = current_time;
                drop(last_time);
                drop(last_count);

                // Handle link clicks
                let d = display.borrow();

                if let Some((b, i)) = d.hovered_link() {
                    let doc = d.editor().document();
                    if b < doc.block_count() {
                        let block = &doc.blocks()[b];
                        if i < block.content.len() {
                            if let InlineContent::Link { link, .. } = &block.content[i] {
                                let destination = link.destination.clone();
                                drop(d); // Release borrow before processing

                                // Check if this is a local markdown file
                                use std::path::Path;
                                let path = Path::new(&destination);

                                // Check if it's a relative or absolute path to a markdown file
                                let is_markdown = path.extension()
                                    .and_then(|e| e.to_str())
                                    .map(|e| e.eq_ignore_ascii_case("md") || e.eq_ignore_ascii_case("markdown"))
                                    .unwrap_or(false);

                                if is_markdown && (path.is_relative() || path.exists()) {
                                    // It's a local markdown file - send a custom event to load it
                                    // We'll use app::handle to send a custom message
                                    fltk::app::handle_main(fltk::enums::Event::from_i32(40)).ok(); // Custom event

                                    // Store the path in a global or pass via callback
                                    // For now, try to read and load it directly
                                    if let Ok(content) = std::fs::read_to_string(&destination) {
                                        use crate::markdown_converter::markdown_to_document;
                                        let new_doc = markdown_to_document(&content);
                                        let mut d = display.borrow_mut();
                                        *d.editor_mut().document_mut() = new_doc;
                                        d.editor_mut().set_cursor(crate::structured_document::DocumentPosition::start());
                                        w.redraw();

                                        // Update window title if possible
                                        if let Some(mut win) = w.window() {
                                            win.set_label(&format!("ViewMD (Structured) - {}", destination));
                                        }

                                        return true;
                                    }
                                }

                                // Not a local markdown file - open in browser
                                #[cfg(target_os = "macos")]
                                std::process::Command::new("open")
                                    .arg(&destination)
                                    .spawn()
                                    .ok();

                                #[cfg(target_os = "linux")]
                                std::process::Command::new("xdg-open")
                                    .arg(&destination)
                                    .spawn()
                                    .ok();

                                #[cfg(target_os = "windows")]
                                std::process::Command::new("cmd")
                                    .args(&["/C", "start", &destination])
                                    .spawn()
                                    .ok();

                                return true;
                            }
                        }
                    }
                } else if edit_mode {
                    // Not on a link - handle cursor positioning and selection in edit mode
                    let pos = d.xy_to_position(x - w.x(), y - w.y());
                    drop(d); // Release borrow

                    match effective_clicks {
                        1 => {
                            // Single click: position cursor
                            display.borrow_mut().editor_mut().set_cursor(pos);
                        }
                        2 => {
                            // Double click: select word
                            display.borrow_mut().editor_mut().select_word_at(pos);
                        }
                        _ => {
                            // Triple click (or more): select line
                            display.borrow_mut().editor_mut().select_line_at(pos);
                        }
                    }
                    w.redraw();
                }

                w.take_focus().ok();
                true
            }
            Event::Drag => {
                // In edit mode, handle drag selection
                if edit_mode {
                    let x = fltk::app::event_x();
                    let y = fltk::app::event_y();
                    let pos = display.borrow().xy_to_position(x - w.x(), y - w.y());
                    display.borrow_mut().editor_mut().extend_selection_to(pos);
                    w.redraw();
                }
                // Hover handled above
                true
            }
            Event::Move | Event::Enter => {
                // Hover handled above
                true
            }
            Event::MouseWheel => {
                // Handle scroll wheel
                let dy = fltk::app::event_dy();
                let scroll_amount = dy as i32 * 20;
                let mut disp = display.borrow_mut();
                let scroll = disp.scroll_offset();
                let new_scroll = (scroll - scroll_amount).max(0);
                disp.set_scroll(new_scroll);
                vscroll_handle.set_value(new_scroll as f64);
                w.redraw();
                true
            }
            Event::KeyDown => {
                let key = fltk::app::event_key();
                let text_input = fltk::app::event_text();
                let state = fltk::app::event_state();

                // Handle editing keys if in edit mode
                if edit_mode {
                    let mut handled = false;

                    // Check for Cmd/Ctrl modifier (without Shift)
                    #[cfg(target_os = "macos")]
                    let cmd_modifier = state.contains(Shortcut::Command) && !state.contains(Shortcut::Shift);
                    #[cfg(not(target_os = "macos"))]
                    let cmd_modifier = state.contains(Shortcut::Ctrl) && !state.contains(Shortcut::Shift);

                    // Check for Cmd/Ctrl-Shift modifier
                    #[cfg(target_os = "macos")]
                    let cmd_shift_modifier = state.contains(Shortcut::Command | Shortcut::Shift);
                    #[cfg(not(target_os = "macos"))]
                    let cmd_shift_modifier = state.contains(Shortcut::Ctrl | Shortcut::Shift);

                    // Cmd/Ctrl-B (toggle bold)
                    if cmd_modifier && key == Key::from_char('b') {
                        let mut disp = display.borrow_mut();
                        disp.editor_mut().toggle_bold().ok();
                        handled = true;
                    }
                    // Cmd/Ctrl-I (toggle italic)
                    else if cmd_modifier && key == Key::from_char('i') {
                        let mut disp = display.borrow_mut();
                        disp.editor_mut().toggle_italic().ok();
                        handled = true;
                    }
                    // Check for Cmd/Ctrl-Shift-H (toggle heading)
                    else if cmd_shift_modifier && key == Key::from_char('h') {
                        let mut disp = display.borrow_mut();
                        disp.editor_mut().toggle_heading().ok();
                        handled = true;
                    }
                    // Check for Cmd/Ctrl-Shift-8 (toggle list)
                    // On US keyboards, Shift-8 produces '*'
                    else if cmd_shift_modifier && (key == Key::from_char('8') || key == Key::from_char('*')) {
                        let mut disp = display.borrow_mut();
                        disp.editor_mut().toggle_list().ok();
                        handled = true;
                    }
                    else {
                        let mut disp = display.borrow_mut();
                        let editor = disp.editor_mut();

                        // Check if Shift is held for selection extension
                        let shift_held = state.contains(Shortcut::Shift);

                        match key {
                            Key::BackSpace => {
                                editor.delete_backward().ok();
                                handled = true;
                            }
                            Key::Delete => {
                                editor.delete_forward().ok();
                                handled = true;
                            }
                            Key::Left => {
                                if shift_held {
                                    editor.move_cursor_left_extend();
                                } else {
                                    editor.move_cursor_left();
                                }
                                handled = true;
                            }
                            Key::Right => {
                                if shift_held {
                                    editor.move_cursor_right_extend();
                                } else {
                                    editor.move_cursor_right();
                                }
                                handled = true;
                            }
                            Key::Up => {
                                if shift_held {
                                    editor.move_cursor_up_extend();
                                } else {
                                    editor.move_cursor_up();
                                }
                                handled = true;
                            }
                            Key::Down => {
                                if shift_held {
                                    editor.move_cursor_down_extend();
                                } else {
                                    editor.move_cursor_down();
                                }
                                handled = true;
                            }
                            Key::Home => {
                                if shift_held {
                                    editor.move_cursor_to_line_start_extend();
                                } else {
                                    editor.move_cursor_to_line_start();
                                }
                                handled = true;
                            }
                            Key::End => {
                                if shift_held {
                                    editor.move_cursor_to_line_end_extend();
                                } else {
                                    editor.move_cursor_to_line_end();
                                }
                                handled = true;
                            }
                            Key::Enter => {
                                editor.insert_newline().ok();
                                handled = true;
                            }
                            Key::PageUp | Key::PageDown => {
                                // Handle scrolling
                                let scroll = disp.scroll_offset();
                                let visible = disp.h();
                                let new_scroll = match key {
                                    Key::PageUp => (scroll - visible).max(0),
                                    Key::PageDown => scroll + visible,
                                    _ => scroll,
                                };
                                if new_scroll != scroll {
                                    disp.set_scroll(new_scroll);
                                    vscroll_handle.set_value(new_scroll as f64);
                                    handled = true;
                                }
                            }
                            _ => {
                                // Handle text input only if no Cmd/Ctrl modifier is pressed
                                // This prevents Cmd-Q, Cmd-W, etc. from inserting characters
                                #[cfg(target_os = "macos")]
                                let has_cmd_modifier = state.contains(Shortcut::Command);
                                #[cfg(not(target_os = "macos"))]
                                let has_cmd_modifier = state.contains(Shortcut::Ctrl);

                                if !text_input.is_empty() && !has_cmd_modifier {
                                    editor.insert_text(&text_input).ok();
                                    handled = true;
                                }
                            }
                        }
                    }

                    if handled {
                        w.redraw();
                    }
                    handled
                } else {
                    // Non-edit mode: only handle scrolling keys
                    let is_scroll_key = matches!(key, Key::PageUp | Key::PageDown);

                    if is_scroll_key {
                        let mut disp = display.borrow_mut();
                        let scroll = disp.scroll_offset();
                        let visible = disp.h();

                        let new_scroll = match key {
                            Key::PageUp => (scroll - visible).max(0),
                            Key::PageDown => scroll + visible,
                            _ => scroll,
                        };

                        if new_scroll != scroll {
                            disp.set_scroll(new_scroll);
                            vscroll_handle.set_value(new_scroll as f64);
                            w.redraw();
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                }
            }
            Event::Focus | Event::Unfocus => {
                w.redraw();
                true
            }
            _ => false,
            }
        }
    });

    widget.end();
    widget.resizable(&widget);

    // Enable mouse tracking for hover events
    widget.set_trigger(fltk::enums::CallbackTrigger::Changed);

    // Handle widget resize
    widget.resize_callback({
        let display = display.clone();
        let mut vscroll_resize = vscroll.clone();
        let mut widget_resize = widget.clone();
        let sb_size = scrollbar_size;
        move |_w, x, y, width, height| {
            // Update display size
            display
                .borrow_mut()
                .resize(x, y, width - sb_size, height);

            // Reposition scrollbar
            vscroll_resize.resize(x + width - sb_size, y, sb_size, height);

            // Trigger redraw
            widget_resize.redraw();
        }
    });

    (widget, display)
}
