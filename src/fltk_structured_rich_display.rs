// FLTK integration for StructuredRichDisplay widget

use crate::fltk_text_display::FltkDrawContext;
use crate::responsive_scrollbar::ResponsiveScrollbar;
use crate::richtext::markdown_converter;
use crate::richtext::structured_document::{BlockType, DocumentPosition, InlineContent};
use crate::richtext::structured_rich_display::StructuredRichDisplay;
use fltk::{app::MouseWheel, enums::*, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

/// FLTK wrapper for StructuredRichDisplay with scrollbar and event handling
pub struct FltkStructuredRichDisplay {
    pub group: fltk::group::Group,
    pub display: Rc<RefCell<StructuredRichDisplay>>,
    link_cb: Rc<RefCell<Option<Box<dyn Fn(String) + 'static>>>>,
    hover_cb: Rc<RefCell<Option<Box<dyn Fn(Option<String>) + 'static>>>>,
    change_cb: Rc<RefCell<Option<Box<dyn FnMut() + 'static>>>>,
}

impl FltkStructuredRichDisplay {
    pub fn new(x: i32, y: i32, w: i32, h: i32, edit_mode: bool) -> Self {
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

        // Callbacks holders
        let link_callback: Rc<RefCell<Option<Box<dyn Fn(String) + 'static>>>> =
            Rc::new(RefCell::new(None));
        let change_callback: Rc<RefCell<Option<Box<dyn FnMut() + 'static>>>> =
            Rc::new(RefCell::new(None));
        let hover_callback: Rc<RefCell<Option<Box<dyn Fn(Option<String>) + 'static>>>> =
            Rc::new(RefCell::new(None));

        // Create vertical responsive scrollbar
        let mut vscroll = ResponsiveScrollbar::new(
            x + w - scrollbar_size,
            y,
            scrollbar_size,
            h,
            Color::from_rgb(255, 255, 245), // Match widget background
        );
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
                let has_focus = fltk::app::focus().map(|f| f.as_base_widget()).as_ref()
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
            let link_cb = link_callback.clone();
            let hover_cb = hover_callback.clone();
            let change_cb = change_callback.clone();
            move |w, event| {
                // Handle hover checking for Push, Drag, Move, and Enter
                let check_hover = matches!(
                    event,
                    Event::Push | Event::Drag | Event::Move | Event::Enter
                );

                if check_hover {
                    let x = fltk::app::event_x();
                    let y = fltk::app::event_y();
                    // Determine desired hover: prefer mouse link, otherwise cursor-adjacent link
                    let (desired_hover, desired_target, mouse_over)
                        = {
                            let mut d = display.borrow_mut();
                            let mouse_hit = d.find_link_at(x - w.x(), y - w.y());
                            if let Some((id, dest)) = mouse_hit {
                                (Some(id), Some(dest), true)
                            } else if let Some((id, dest)) = d.find_link_near_cursor() {
                                (Some(id), Some(dest), false)
                            } else {
                                (None, None, false)
                            }
                        };

                    // Update cursor icon based on mouse hit only
                    if let Some(mut win) = w.window() {
                        if mouse_over {
                            win.set_cursor(Cursor::Hand);
                        } else {
                            win.set_cursor(Cursor::Default);
                        }
                    }

                    // Update hover state and notify if changed
                    let mut d = display.borrow_mut();
                    let prev = d.hovered_link();
                    if prev != desired_hover {
                        d.set_hovered_link(desired_hover);
                        drop(d);
                        if let Some(cb) = &*hover_cb.borrow() {
                            (cb)(desired_target);
                        }
                        w.redraw();
                    }
                }

                match event {
                    Event::Push => {
                        // Check for right-click context menu in edit mode (button 3 is right-click)
                        if edit_mode && fltk::app::event_button() == 3 {
                            let x = fltk::app::event_x();
                            let y = fltk::app::event_y();

                            // Unified context menu via context_menu module
                            // If no selection, move caret to click location first
                            let clicked_pos = {
                                let d = display.borrow();
                                d.xy_to_position(x - w.x(), y - w.y())
                            };
                            let has_selection = display.borrow().editor().selection().is_some();
                            if !has_selection {
                                display.borrow_mut().editor_mut().set_cursor(clicked_pos);
                            }
                            // Determine current block type based on caret position
                            let current_block = {
                                let d = display.borrow();
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
                            let mut w_for_actions = w.clone();
                            let actions = crate::context_menu::MenuActions {
                                has_selection,
                                current_block,
                                set_paragraph: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().set_block_type(BlockType::Paragraph).ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                set_heading1: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().set_block_type(BlockType::Heading { level: 1 }).ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                set_heading2: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().set_block_type(BlockType::Heading { level: 2 }).ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                set_heading3: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().set_block_type(BlockType::Heading { level: 3 }).ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_list: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_list().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_bold: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_bold().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_italic: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_italic().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_code: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_code().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_strike: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_strikethrough().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_underline: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_underline().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_highlight: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_highlight().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                clear_formatting: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().clear_formatting().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                cut: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        if let Ok(text) = display.borrow_mut().editor_mut().cut() { fltk::app::copy(&text); }
                                        if let Some(cb) = &mut *change_cb.borrow_mut() { (cb)(); }
                                        w_r.redraw();
                                    }
                                }),
                                copy: Box::new({
                                    let display = display.clone();
                                    move || {
                                        let text = display.borrow().editor().copy();
                                        if !text.is_empty() { fltk::app::copy(&text); }
                                    }
                                }),
                                paste: Box::new({
                                    let mut w_r = w_for_actions.clone();
                                    move || { fltk::app::paste(&mut w_r); }
                                }),
                                edit_link: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let w_for_dialog = w.clone();
                                    move || {
                                        // Determine initial state: hovered link, selection, or empty
                                        let (init_target, init_text, mode_existing_link, selection_mode, link_pos) = {
                                            let mut disp = display.borrow_mut();
                                            if let Some((b, i)) = disp.hovered_link() {
                                                let doc = disp.editor().document();
                                                let block = &doc.blocks()[b];
                                                if let InlineContent::Link { link, content } = &block.content[i] {
                                                    let text = content.iter().map(|c| c.to_plain_text()).collect::<String>();
                                                    (link.destination.clone(), text, true, false, Some((b, i)))
                                                } else { (String::new(), String::new(), false, false, None) }
                                            } else if let Some((a, b)) = disp.editor().selection() {
                                                let text = disp.editor().text_in_range(a, b);
                                                (String::new(), text, false, true, None)
                                            } else {
                                                (String::new(), String::new(), false, false, None)
                                            }
                                        };

                                        let center_rect = if let Some(parent) = w_for_dialog.window() {
                                            Some((parent.x(), parent.y(), parent.w(), parent.h()))
                                        } else { None };

                                        let opts = crate::link_editor::LinkEditOptions {
                                            init_target,
                                            init_text: init_text.clone(),
                                            mode_existing_link,
                                            selection_mode,
                                            center_rect,
                                        };

                                        let display_cb = display.clone();
                                        let change_cb_ref = change_cb.clone();
                                        crate::link_editor::show_link_editor(
                                            opts,
                                            move |dest: String, txt: String| {
                                                let mut disp = display_cb.borrow_mut();
                                                let editor = disp.editor_mut();
                                                if let Some((b, i)) = link_pos {
                                                    editor.edit_link_at(b, i, &dest, &txt).ok();
                                                } else if !txt.is_empty() {
                                                    if editor.selection().is_some() { editor.replace_selection_with_link(&dest, &txt).ok(); }
                                                    else { editor.insert_link_at_cursor(&dest, &txt).ok(); }
                                                }
                                                drop(disp);
                                                if let Some(cb) = &mut *change_cb_ref.borrow_mut() { (cb)(); }
                                            },
                                            Some({
                                                let display_rm = display.clone();
                                                let change_cb_rm = change_cb.clone();
                                                move || {
                                                    if let Some((b, i)) = link_pos {
                                                        let mut disp = display_rm.borrow_mut();
                                                        disp.editor_mut().remove_link_at(b, i).ok();
                                                        drop(disp);
                                                        if let Some(cb) = &mut *change_cb_rm.borrow_mut() { (cb)(); }
                                                    }
                                                }
                                            }),
                                        );
                                    }
                                }),
                                };

                            crate::context_menu::show_context_menu(x, y, actions);
                            return true;

                            // Paragraph Style submenu
                            // Platform-specific shortcuts for paragraph and headings
                            #[cfg(target_os = "macos")]
                            let para_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Alt
                                | '0';
                            #[cfg(not(target_os = "macos"))]
                            let para_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Alt
                                | '0';

                            #[cfg(target_os = "macos")]
                            let h1_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Alt
                                | '1';
                            #[cfg(not(target_os = "macos"))]
                            let h1_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Alt
                                | '1';

                            #[cfg(target_os = "macos")]
                            let h2_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Alt
                                | '2';
                            #[cfg(not(target_os = "macos"))]
                            let h2_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Alt
                                | '2';

                            #[cfg(target_os = "macos")]
                            let h3_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Alt
                                | '3';
                            #[cfg(not(target_os = "macos"))]
                            let h3_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Alt
                                | '3';

                            // Dummy menu to satisfy subsequent unreachable menu.add calls
                            let mut menu = fltk::menu::MenuButton::default();

                            menu.add(
                                "Paragraph Style/Paragraph\t",
                                para_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Paragraph)
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            menu.add(
                                "Paragraph Style/Heading 1\t",
                                h1_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Heading { level: 1 })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            menu.add(
                                "Paragraph Style/Heading 2\t",
                                h2_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Heading { level: 2 })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            menu.add(
                                "Paragraph Style/Heading 3\t",
                                h3_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Heading { level: 3 })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            #[cfg(target_os = "macos")]
                            let list_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Shift
                                | '8';
                            #[cfg(not(target_os = "macos"))]
                            let list_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Shift
                                | '8';

                            menu.add(
                                "Paragraph Style/List Item\t",
                                list_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::ListItem {
                                                ordered: false,
                                                number: None,
                                            })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            // Inline styles
                            #[cfg(target_os = "macos")]
                            let bold_shortcut = fltk::enums::Shortcut::Command | 'b';
                            #[cfg(not(target_os = "macos"))]
                            let bold_shortcut = fltk::enums::Shortcut::Ctrl | 'b';

                            menu.add(
                                "Toggle Bold\t",
                                bold_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display.borrow_mut().editor_mut().toggle_bold().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            #[cfg(target_os = "macos")]
                            let italic_shortcut = fltk::enums::Shortcut::Command | 'i';
                            #[cfg(not(target_os = "macos"))]
                            let italic_shortcut = fltk::enums::Shortcut::Ctrl | 'i';

                            menu.add(
                                "Toggle Italic\t",
                                italic_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display.borrow_mut().editor_mut().toggle_italic().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            #[cfg(target_os = "macos")]
                            let code_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Shift
                                | 'c';
                            #[cfg(not(target_os = "macos"))]
                            let code_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Shift
                                | 'c';

                            menu.add(
                                "Toggle Code\t",
                                code_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display.borrow_mut().editor_mut().toggle_code().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            #[cfg(target_os = "macos")]
                            let strike_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Shift
                                | 'x';
                            #[cfg(not(target_os = "macos"))]
                            let strike_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Shift
                                | 'x';

                            menu.add(
                                "Toggle Strikethrough\t",
                                strike_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .toggle_strikethrough()
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            #[cfg(target_os = "macos")]
                            let underline_shortcut = fltk::enums::Shortcut::Command | 'u';
                            #[cfg(not(target_os = "macos"))]
                            let underline_shortcut = fltk::enums::Shortcut::Ctrl | 'u';

                            menu.add(
                                "Toggle Underline\t",
                                underline_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display.borrow_mut().editor_mut().toggle_underline().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            #[cfg(target_os = "macos")]
                            let highlight_shortcut = fltk::enums::Shortcut::Command
                                | fltk::enums::Shortcut::Shift
                                | 'h';
                            #[cfg(not(target_os = "macos"))]
                            let highlight_shortcut = fltk::enums::Shortcut::Ctrl
                                | fltk::enums::Shortcut::Shift
                                | 'h';

                            menu.add(
                                "Toggle Highlight\t",
                                highlight_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display.borrow_mut().editor_mut().toggle_highlight().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            // Separator
                            menu.add(
                                "Clear Formatting\t",
                                {
                                    #[cfg(target_os = "macos")]
                                    {
                                        fltk::enums::Shortcut::Command | '\\'
                                    }
                                    #[cfg(not(target_os = "macos"))]
                                    {
                                        fltk::enums::Shortcut::Ctrl | '\\'
                                    }
                                },
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let mut w_clone = w.clone();
                                    let change_cb = change_cb.clone();
                                    move |_| {
                                        display.borrow_mut().editor_mut().clear_formatting().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_clone.redraw();
                                    }
                                },
                            );

                            menu.add(
                                "_",
                                fltk::enums::Shortcut::None,
                                fltk::menu::MenuFlag::MenuDivider,
                                |_| {},
                            );

                            // Edit Link (Cmd/Ctrl-K)
                            #[cfg(target_os = "macos")]
                            let edit_link_shortcut = fltk::enums::Shortcut::Command | 'k';
                            #[cfg(not(target_os = "macos"))]
                            let edit_link_shortcut = fltk::enums::Shortcut::Ctrl | 'k';

                            menu.add(
                                "Edit Link…\t",
                                edit_link_shortcut,
                                fltk::menu::MenuFlag::Normal,
                                {
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_for_dialog = w.clone();
                                    move |_| {
                                        // Determine initial state: hovered link, selection, or empty
                                        let (init_target, init_text, mode_existing_link, selection_mode, link_pos) = {
                                            let mut disp = display.borrow_mut();
                                            if let Some((b, i)) = disp.hovered_link() {
                                                let doc = disp.editor().document();
                                                let block = &doc.blocks()[b];
                                                if let InlineContent::Link { link, content } = &block.content[i] {
                                                    let text = content.iter().map(|c| c.to_plain_text()).collect::<String>();
                                                    (link.destination.clone(), text, true, false, Some((b, i)))
                                                } else {
                                                    (String::new(), String::new(), false, false, None)
                                                }
                                            } else if let Some((a, b)) = disp.editor().selection() {
                                                let text = disp.editor().text_in_range(a, b);
                                                (String::new(), text, false, true, None)
                                            } else {
                                                (String::new(), String::new(), false, false, None)
                                            }
                                        };

                                        let center_rect = if let Some(parent) = w_for_dialog.window() {
                                            Some((parent.x(), parent.y(), parent.w(), parent.h()))
                                        } else {
                                            None
                                        };

                                        let opts = crate::link_editor::LinkEditOptions {
                                            init_target,
                                            init_text: init_text.clone(),
                                            mode_existing_link,
                                            selection_mode,
                                            center_rect,
                                        };

                                        // Use shared link editor dialog
                                let display_cb = display.clone();
                                let change_cb_ref = change_cb.clone();
                                let w_for_redraw = w_for_dialog.clone();
                                        crate::link_editor::show_link_editor(
                                            opts,
                                            move |dest: String, txt: String| {
                                                let mut disp = display_cb.borrow_mut();
                                                let editor = disp.editor_mut();
                                                if let Some((b, i)) = link_pos {
                                                    editor.edit_link_at(b, i, &dest, &txt).ok();
                                                } else if !txt.is_empty() {
                                                    if editor.selection().is_some() {
                                                        editor.replace_selection_with_link(&dest, &txt).ok();
                                                    } else {
                                                        editor.insert_link_at_cursor(&dest, &txt).ok();
                                                    }
                                                }
                                                drop(disp);
                                                if let Some(cb) = &mut *change_cb_ref.borrow_mut() { (cb)(); }
                                                let mut w_local = w_for_redraw.clone();
                                                w_local.redraw();
                                            },
                                            Some({
                                                let display_rm = display.clone();
                                                let change_cb_rm = change_cb.clone();
                                                let w_for_rm = w_for_dialog.clone();
                                                move || {
                                                    if let Some((b, i)) = link_pos {
                                                        let mut disp = display_rm.borrow_mut();
                                                        disp.editor_mut().remove_link_at(b, i).ok();
                                                        drop(disp);
                                                        if let Some(cb) = &mut *change_cb_rm.borrow_mut() { (cb)(); }
                                                    }
                                                    let mut w_local = w_for_rm.clone();
                                                    w_local.redraw();
                                                }
                                            }),
                                        );
                                    }
                                },
                            );

                            // Edit operations
                            let has_selection = display.borrow().editor().selection().is_some();

                            #[cfg(target_os = "macos")]
                            let cut_shortcut = fltk::enums::Shortcut::Command | 'x';
                            #[cfg(not(target_os = "macos"))]
                            let cut_shortcut = fltk::enums::Shortcut::Ctrl | 'x';

                            menu.add("Cut\t", cut_shortcut, fltk::menu::MenuFlag::Normal, {
                                let display = display.clone();
                                let mut w_clone = w.clone();
                                let change_cb = change_cb.clone();
                                move |_| {
                                    if let Ok(text) = display.borrow_mut().editor_mut().cut() {
                                        fltk::app::copy(&text);
                                    }
                                    if let Some(cb) = &mut *change_cb.borrow_mut() {
                                        (cb)();
                                    }
                                    w_clone.redraw();
                                }
                            });

                            if !has_selection {
                                let idx = menu.find_index("Cut\t");
                                if idx >= 0 {
                                    menu.set_mode(idx, fltk::menu::MenuFlag::Inactive);
                                }
                            }

                            #[cfg(target_os = "macos")]
                            let copy_shortcut = fltk::enums::Shortcut::Command | 'c';
                            #[cfg(not(target_os = "macos"))]
                            let copy_shortcut = fltk::enums::Shortcut::Ctrl | 'c';

                            menu.add("Copy\t", copy_shortcut, fltk::menu::MenuFlag::Normal, {
                                let display = display.clone();
                                move |_| {
                                    let text = display.borrow().editor().copy();
                                    if !text.is_empty() {
                                        fltk::app::copy(&text);
                                    }
                                }
                            });

                            if !has_selection {
                                let idx = menu.find_index("Copy\t");
                                if idx >= 0 {
                                    menu.set_mode(idx, fltk::menu::MenuFlag::Inactive);
                                }
                            }

                            #[cfg(target_os = "macos")]
                            let paste_shortcut = fltk::enums::Shortcut::Command | 'v';
                            #[cfg(not(target_os = "macos"))]
                            let paste_shortcut = fltk::enums::Shortcut::Ctrl | 'v';

                            // Paste via FLTK paste event so we can read text in Event::Paste
                            menu.add("Paste\t", paste_shortcut, fltk::menu::MenuFlag::Normal, {
                                let mut w_clone = w.clone();
                                move |_m: &mut fltk::menu::MenuButton| {
                                    fltk::app::paste(&mut w_clone);
                                }
                            });

                            // Show the menu
                            menu.popup();
                            return true;
                        }

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

                        // Handle link clicks: only when the mouse is actually over a link
                        let x_local = x - w.x();
                        let y_local = y - w.y();
                        let mouse_link = {
                            let d = display.borrow();
                            d.find_link_at(x_local, y_local)
                        };
                        if let Some((_, destination)) = mouse_link {
                            if let Some(cb) = &*link_cb.borrow() {
                                cb(destination);
                                return true;
                            }
                            return false;
                        } else if edit_mode {
                            // Not on a link - handle cursor positioning and selection in edit mode
                            let pos = {
                                let d = display.borrow();
                                d.xy_to_position(x_local, y_local)
                            };

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
                        // In edit mode, handle drag selection and auto-scroll
                        if edit_mode {
                            let x = fltk::app::event_x();
                            let y = fltk::app::event_y();

                            // Auto-scroll when dragging near top/bottom edges
                            let mut disp = display.borrow_mut();
                            let mut new_scroll = disp.scroll_offset();
                            let top_edge = w.y() + 12;
                            let bottom_edge = w.y() + w.h() - 12;

                            if y < top_edge {
                                let delta = (top_edge - y).max(4);
                                new_scroll = (new_scroll - delta).max(0);
                            } else if y > bottom_edge {
                                let delta = (y - bottom_edge).max(4);
                                new_scroll = new_scroll + delta;
                            }

                            if new_scroll != disp.scroll_offset() {
                                disp.set_scroll(new_scroll);
                                drop(disp); // release before UI updates
                                vscroll_handle.set_value(new_scroll as f64);
                                vscroll_handle.wake();
                            } else {
                                drop(disp);
                            }

                            // Update selection end to the current pointer position
                            let pos = {
                                let d = display.borrow();
                                d.xy_to_position(x - w.x(), y - w.y())
                            };
                            display.borrow_mut().editor_mut().extend_selection_to(pos);

                            // Ensure the cursor (selection end) is visible after update
                            let has_focus = fltk::app::focus().map(|f| f.as_base_widget()).as_ref()
                                == Some(&w.as_base_widget());
                            let is_active = w.active();
                            let mut ctx = FltkDrawContext::new(has_focus, is_active);
                            let final_scroll = {
                                let mut d = display.borrow_mut();
                                d.ensure_cursor_visible(&mut ctx);
                                d.scroll_offset()
                            };
                            vscroll_handle.set_value(final_scroll as f64);
                            vscroll_handle.wake();

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
                        // Handle vertical scroll wheel only
                        let dy = fltk::app::event_dy();
                        let dx = fltk::app::event_dx();

                        // Only handle vertical scrolling, ignore horizontal
                        if dy != MouseWheel::None && dx == MouseWheel::None {
                            let scroll_amount = match dy {
                                MouseWheel::Up => -20,
                                MouseWheel::Down => 20,
                                _ => 0,
                            };

                            let mut disp = display.borrow_mut();
                            let scroll = disp.scroll_offset();
                            let new_scroll = (scroll + scroll_amount).max(0);
                            disp.set_scroll(new_scroll);
                            drop(disp); // Release borrow before calling wake
                            vscroll_handle.set_value(new_scroll as f64);
                            vscroll_handle.wake(); // Wake the scrollbar
                            w.redraw();
                            true
                        } else {
                            // Don't handle horizontal scrolling or mixed scrolling
                            false
                        }
                    }
                    Event::KeyDown => {
                        let key = fltk::app::event_key();
                        let text_input = fltk::app::event_text();
                        let state = fltk::app::event_state();

                        // Handle editing keys if in edit mode
                        if edit_mode {
                            let mut handled = false;

                            // Ctrl/Cmd+K: Open link editor dialog
                            #[cfg(target_os = "macos")]
                            let cmd_modifier = state.contains(Shortcut::Command);
                            #[cfg(not(target_os = "macos"))]
                            let cmd_modifier = state.contains(Shortcut::Ctrl);

                            if cmd_modifier && (key == Key::from_char('k') || key == Key::from_char('K')) {
                                // Gather current context: hovered link or selection
                                let mut disp = display.borrow_mut();
                                let hovered = disp.hovered_link();
                                let (init_target, init_text, mode_existing_link, selection_mode, link_pos) = if let Some((b, i)) = hovered {
                                    // Prefill from hovered link
                                    let doc = disp.editor().document();
                                    let block = &doc.blocks()[b];
                                    if let InlineContent::Link { link, content } = &block.content[i] {
                                        let text = content.iter().map(|c| c.to_plain_text()).collect::<String>();
                                        (link.destination.clone(), text, true, false, Some((b, i)))
                                    } else {
                                        (String::new(), String::new(), false, false, None)
                                    }
                                } else if let Some((a, b)) = disp.editor().selection() {
                                    let text = disp.editor().text_in_range(a, b);
                                    (String::new(), text, false, true, None)
                                } else {
                                    (String::new(), String::new(), false, false, None)
                                };

                                drop(disp); // release borrow before creating UI

                                // Center rectangle from the current window (fallback handled in helper)
                                let center_rect = if let Some(parent) = w.window() {
                                    Some((parent.x(), parent.y(), parent.w(), parent.h()))
                                } else {
                                    None
                                };

                                let opts = crate::link_editor::LinkEditOptions {
                                    init_target,
                                    init_text: init_text.clone(),
                                    mode_existing_link,
                                    selection_mode,
                                    center_rect,
                                };

                                // Invoke shared dialog
                                let display_cb = display.clone();
                                let change_cb_ref = change_cb.clone();
                                crate::link_editor::show_link_editor(
                                    opts,
                                    move |dest: String, txt: String| {
                                        let mut disp = display_cb.borrow_mut();
                                        let editor = disp.editor_mut();
                                        if let Some((b, i)) = link_pos {
                                            editor.edit_link_at(b, i, &dest, &txt).ok();
                                        } else if !txt.is_empty() {
                                            if editor.selection().is_some() {
                                                editor.replace_selection_with_link(&dest, &txt).ok();
                                            } else {
                                                editor.insert_link_at_cursor(&dest, &txt).ok();
                                            }
                                        }
                                        drop(disp);
                                        if let Some(cb) = &mut *change_cb_ref.borrow_mut() { (cb)(); }
                                    },
                                    Some({
                                        let display_rm = display.clone();
                                        let change_cb_rm = change_cb.clone();
                                        move || {
                                            if let Some((b, i)) = link_pos {
                                                let mut disp = display_rm.borrow_mut();
                                                disp.editor_mut().remove_link_at(b, i).ok();
                                                drop(disp);
                                                if let Some(cb) = &mut *change_cb_rm.borrow_mut() { (cb)(); }
                                            }
                                        }
                                    }),
                                );
                                handled = true;
                            } else {

                            // Open context menu on Menu key or Shift+F10
                            let open_context_menu = {
                                let is_menu_key = key == Key::Menu;
                                // Detect Shift+F10
                                let shift_f10 = state.contains(Shortcut::Shift)
                                    && (key == Key::F10 || key == Key::from_char('0'))
                                    && key == Key::F10;
                                is_menu_key || shift_f10
                            };

                            if open_context_menu {
                                let x = fltk::app::event_x();
                                let y = fltk::app::event_y();

                                let has_selection = display.borrow().editor().selection().is_some();
                                let mut w_for_actions = w.clone();
                                let actions = crate::context_menu::MenuActions {
                                    has_selection,
                                    current_block: {
                                        let d = display.borrow();
                                        let ed = d.editor();
                                        let cur = ed.cursor();
                                        let doc = ed.document();
                                        let blocks = doc.blocks();
                                        if !blocks.is_empty() && (cur.block_index as usize) < blocks.len() {
                                            blocks[cur.block_index as usize].block_type.clone()
                                        } else {
                                            BlockType::Paragraph
                                        }
                                    },
                                    set_paragraph: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().set_block_type(BlockType::Paragraph).ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    set_heading1: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().set_block_type(BlockType::Heading { level: 1 }).ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    set_heading2: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().set_block_type(BlockType::Heading { level: 2 }).ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    set_heading3: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().set_block_type(BlockType::Heading { level: 3 }).ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_list: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_list().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_bold: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_bold().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_italic: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_italic().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_code: Box::new({
                                        let display = display.clone();
        
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_code().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_strike: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_strikethrough().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_underline: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_underline().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    toggle_highlight: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().toggle_highlight().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    clear_formatting: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            display.borrow_mut().editor_mut().clear_formatting().ok();
                                            w_r.redraw();
                                        }
                                    }),
                                    cut: Box::new({
                                        let display = display.clone();
                                        let mut w_r = w_for_actions.clone();
                                        move || {
                                            if let Ok(text) = display.borrow_mut().editor_mut().cut() { fltk::app::copy(&text); }
                                            w_r.redraw();
                                        }
                                    }),
                                    copy: Box::new({
                                        let display = display.clone();
                                        move || {
                                            let text = display.borrow().editor().copy();
                                            if !text.is_empty() { fltk::app::copy(&text); }
                                        }
                                    }),
                                    paste: Box::new({
                                        let mut w_r = w_for_actions.clone();
                                        move || { fltk::app::paste(&mut w_r); }
                                    }),
                                    edit_link: Box::new({
                                        let display = display.clone();
                                        let w_for_dialog = w.clone();
                                        move || {
                                            // Determine initial state: hovered link, selection, or empty
                                            let (init_target, init_text, mode_existing_link, selection_mode, link_pos) = {
                                                let mut disp = display.borrow_mut();
                                                if let Some((b, i)) = disp.hovered_link() {
                                                    let doc = disp.editor().document();
                                                    let block = &doc.blocks()[b];
                                                    if let InlineContent::Link { link, content } = &block.content[i] {
                                                        let text = content.iter().map(|c| c.to_plain_text()).collect::<String>();
                                                        (link.destination.clone(), text, true, false, Some((b, i)))
                                                    } else { (String::new(), String::new(), false, false, None) }
                                                } else if let Some((a, b)) = disp.editor().selection() {
                                                    let text = disp.editor().text_in_range(a, b);
                                                    (String::new(), text, false, true, None)
                                                } else { (String::new(), String::new(), false, false, None) }
                                            };

                                            let center_rect = if let Some(parent) = w_for_dialog.window() {
                                                Some((parent.x(), parent.y(), parent.w(), parent.h()))
                                            } else { None };

                                            let opts = crate::link_editor::LinkEditOptions {
                                                init_target,
                                                init_text: init_text.clone(),
                                                mode_existing_link,
                                                selection_mode,
                                                center_rect,
                                            };

                                            let display_cb = display.clone();
                                            crate::link_editor::show_link_editor(
                                                opts,
                                                move |dest: String, txt: String| {
                                                    let mut disp = display_cb.borrow_mut();
                                                    let editor = disp.editor_mut();
                                                    if let Some((b, i)) = link_pos {
                                                        editor.edit_link_at(b, i, &dest, &txt).ok();
                                                    } else if !txt.is_empty() {
                                                        if editor.selection().is_some() { editor.replace_selection_with_link(&dest, &txt).ok(); }
                                                        else { editor.insert_link_at_cursor(&dest, &txt).ok(); }
                                                    }
                                                },
                                                Option::<fn()>::None,
                                            );
                                        }
                                    }),
                                };

                                crate::context_menu::show_context_menu(x, y, actions);
                                return true;
                            }

                            // Check for Cmd/Ctrl modifier (without Shift)
                            #[cfg(target_os = "macos")]
                            let cmd_modifier = state.contains(Shortcut::Command)
                                && !state.contains(Shortcut::Shift)
                                && !state.contains(Shortcut::Alt);
                            #[cfg(not(target_os = "macos"))]
                            let cmd_modifier = state.contains(Shortcut::Ctrl)
                                && !state.contains(Shortcut::Shift)
                                && !state.contains(Shortcut::Alt);

                            // Check for Cmd/Ctrl-Shift modifier
                            #[cfg(target_os = "macos")]
                            let cmd_shift_modifier = state.contains(Shortcut::Command)
                                && state.contains(Shortcut::Shift)
                                && !state.contains(Shortcut::Alt);
                            #[cfg(not(target_os = "macos"))]
                            let cmd_shift_modifier = state.contains(Shortcut::Ctrl)
                                && state.contains(Shortcut::Shift)
                                && !state.contains(Shortcut::Alt);

                            // Check for Cmd/Ctrl-Alt modifier (for headings/paragraph)
                            #[cfg(target_os = "macos")]
                            let cmd_alt_modifier =
                                state.contains(Shortcut::Command | Shortcut::Alt)
                                    && !state.contains(Shortcut::Shift);
                            #[cfg(not(target_os = "macos"))]
                            let cmd_alt_modifier =
                                state.contains(Shortcut::Ctrl | Shortcut::Alt)
                                    && !state.contains(Shortcut::Shift);

                            // Cmd/Ctrl-A (Select All) - no content change
                            if cmd_modifier && key == Key::from_char('a') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().select_all();
                                handled = true;
                            }
                            // Cmd/Ctrl-B (toggle bold)
                            else if cmd_modifier && key == Key::from_char('b') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_bold().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-I (toggle italic)
                            else if cmd_modifier && key == Key::from_char('i') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_italic().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-U (toggle underline)
                            else if cmd_modifier && key == Key::from_char('u') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_underline().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-\ (clear formatting)
                            else if cmd_modifier && key == Key::from_char('\\') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().clear_formatting().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-C (copy)
                            else if cmd_modifier && key == Key::from_char('c') {
                                let text = display.borrow().editor().copy();
                                if !text.is_empty() {
                                    fltk::app::copy(&text);
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-X (cut)
                            else if cmd_modifier && key == Key::from_char('x') {
                                if let Ok(text) = display.borrow_mut().editor_mut().cut() {
                                    fltk::app::copy(&text);
                                }
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-V (paste)
                            else if cmd_modifier && key == Key::from_char('v') {
                                // Ask FLTK to deliver a paste event containing clipboard text
                                fltk::app::paste(w);
                                handled = true;
                            }
                            // Cmd/Ctrl-Shift-H (toggle highlight)
                            else if cmd_shift_modifier && key == Key::from_char('h') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_highlight().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-Shift-C (toggle code)
                            else if cmd_shift_modifier && key == Key::from_char('c') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_code().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-Shift-X (toggle strikethrough)
                            else if cmd_shift_modifier && key == Key::from_char('x') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_strikethrough().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Check for Cmd/Ctrl-Shift-8 (toggle list)
                            // On US keyboards, Shift-8 produces '*'
                            else if cmd_shift_modifier
                                && (key == Key::from_char('8') || key == Key::from_char('*'))
                            {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().toggle_list().ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-Alt-1..3: set heading level 1..3
                            else if cmd_alt_modifier && key == Key::from_char('1') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut()
                                    .set_block_type(BlockType::Heading { level: 1 })
                                    .ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            } else if cmd_alt_modifier && key == Key::from_char('2') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut()
                                    .set_block_type(BlockType::Heading { level: 2 })
                                    .ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            } else if cmd_alt_modifier && key == Key::from_char('3') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut()
                                    .set_block_type(BlockType::Heading { level: 3 })
                                    .ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            }
                            // Cmd/Ctrl-Alt-0: set paragraph
                            else if cmd_alt_modifier && key == Key::from_char('0') {
                                let mut disp = display.borrow_mut();
                                disp.editor_mut().set_block_type(BlockType::Paragraph).ok();
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                handled = true;
                            } else {
                                let mut disp = display.borrow_mut();
                                let editor = disp.editor_mut();

                                // Check if Shift is held for selection extension
                                let shift_held = state.contains(Shortcut::Shift);
                                // Check for word navigation modifier (Alt on macOS, Ctrl elsewhere)
                                #[cfg(target_os = "macos")]
                                let word_mod = state.contains(Shortcut::Alt)
                                    && !state.contains(Shortcut::Command);
                                #[cfg(not(target_os = "macos"))]
                                let word_mod = state.contains(Shortcut::Ctrl)
                                    && !state.contains(Shortcut::Shift);

                                match key {
                                    Key::BackSpace => {
                                        if word_mod {
                                            editor.delete_word_backward().ok();
                                        } else {
                                            editor.delete_backward().ok();
                                        }
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        handled = true;
                                    }
                                    Key::Delete => {
                                        if word_mod {
                                            editor.delete_word_forward().ok();
                                        } else {
                                            editor.delete_forward().ok();
                                        }
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        handled = true;
                                    }
                                    Key::Left => {
                                        if word_mod {
                                            if shift_held {
                                                editor.move_word_left_extend();
                                            } else {
                                                editor.move_word_left();
                                            }
                                        } else {
                                            if shift_held {
                                                editor.move_cursor_left_extend();
                                            } else {
                                                editor.move_cursor_left();
                                            }
                                        }
                                        handled = true;
                                    }
                                    Key::Right => {
                                        if word_mod {
                                            if shift_held {
                                                editor.move_word_right_extend();
                                            } else {
                                                editor.move_word_right();
                                            }
                                        } else {
                                            if shift_held {
                                                editor.move_cursor_right_extend();
                                            } else {
                                                editor.move_cursor_right();
                                            }
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
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
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
                                            drop(disp); // Release borrow before calling wake
                                            vscroll_handle.set_value(new_scroll as f64);
                                            vscroll_handle.wake(); // Wake the scrollbar
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
                                            if let Some(cb) = &mut *change_cb.borrow_mut() {
                                                (cb)();
                                            }
                                            handled = true;
                                        }
                                    }
                                }
                            }

                            }

                            if handled {
                                // After handling an edit/cursor move, ensure cursor is visible
                                // Create a draw context for measurements used by ensure_cursor_visible
                                let has_focus =
                                    fltk::app::focus().map(|f| f.as_base_widget()).as_ref()
                                        == Some(&w.as_base_widget());
                                let is_active = w.active();
                                let mut ctx = FltkDrawContext::new(has_focus, is_active);

                                // Adjust scroll to bring cursor into view
                                let new_scroll = {
                                    let mut disp = display.borrow_mut();
                                    disp.ensure_cursor_visible(&mut ctx);
                                    disp.scroll_offset()
                                };

                                // Update link hover state based on the new cursor position
                                {
                                    let mut disp = display.borrow_mut();
                                    if let Some(((b, i), dest)) = disp.find_link_near_cursor() {
                                        let prev = disp.hovered_link();
                                        if prev != Some((b, i)) {
                                            disp.set_hovered_link(Some((b, i)));
                                            if let Some(cb) = &*hover_cb.borrow() {
                                                (cb)(Some(dest));
                                            }
                                        }
                                    } else if disp.hovered_link().is_some() {
                                        disp.set_hovered_link(None);
                                        if let Some(cb) = &*hover_cb.borrow() {
                                            (cb)(None);
                                        }
                                    }
                                }

                                // Sync scrollbar position and redraw
                                vscroll_handle.set_value(new_scroll as f64);
                                vscroll_handle.wake();
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
                                    drop(disp); // Release borrow before calling wake
                                    vscroll_handle.set_value(new_scroll as f64);
                                    vscroll_handle.wake(); // Wake the scrollbar
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
                    Event::Paste => {
                        if edit_mode {
                            let pasted = fltk::app::event_text();
                            if !pasted.is_empty() {
                                let mut disp = display.borrow_mut();
                                let _ = disp.editor_mut().paste(&pasted);
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                w.redraw();
                            }
                            true
                        } else {
                            false
                        }
                    }
                    Event::Focus => {
                        // On focus, re-evaluate hover from cursor position
                        let (desired_hover, desired_target) = {
                            let mut d = display.borrow_mut();
                            if let Some((id, dest)) = d.find_link_near_cursor() {
                                (Some(id), Some(dest))
                            } else {
                                (None, None)
                            }
                        };
                        let mut d = display.borrow_mut();
                        let prev = d.hovered_link();
                        if prev != desired_hover {
                            d.set_hovered_link(desired_hover);
                            drop(d);
                            if let Some(cb) = &*hover_cb.borrow() {
                                (cb)(desired_target);
                            }
                        }
                        w.redraw();
                        true
                    }
                    Event::Unfocus => {
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
                display.borrow_mut().resize(x, y, width - sb_size, height);

                // Reposition scrollbar
                vscroll_resize.resize(x + width - sb_size, y, sb_size, height);

                // Trigger redraw
                widget_resize.redraw();
            }
        });

        FltkStructuredRichDisplay {
            group: widget,
            display,
            link_cb: link_callback,
            hover_cb: hover_callback,
            change_cb: change_callback,
        }
    }

    pub fn set_link_callback(&self, cb: Option<Box<dyn Fn(String) + 'static>>) {
        *self.link_cb.borrow_mut() = cb;
    }

    pub fn set_link_hover_callback(&self, cb: Option<Box<dyn Fn(Option<String>) + 'static>>) {
        *self.hover_cb.borrow_mut() = cb;
    }

    pub fn set_change_callback(&self, cb: Option<Box<dyn FnMut() + 'static>>) {
        *self.change_cb.borrow_mut() = cb;
    }

    /// Periodic tick to update cursor blinking; triggers redraw if needed
    pub fn tick(&mut self, ms_since_start: u64) {
        let changed = self.display.borrow_mut().tick(ms_since_start);
        if changed {
            self.group.redraw();
        }
    }
}
