// FLTK integration for StructuredRichDisplay widget

use crate::fltk_draw_context::FltkDrawContext;
use crate::responsive_scrollbar::ResponsiveScrollbar;
use crate::richtext::structured_document::{BlockType, InlineContent};
use crate::richtext::structured_rich_display::StructuredRichDisplay;
use fltk::{app::MouseWheel, enums::*, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Instant;

type Callback<T> = Rc<RefCell<Option<Box<dyn Fn(T) + 'static>>>>;
type MutCallback<T> = Rc<RefCell<Option<Box<dyn FnMut(T) + 'static>>>>;
type MutCallback0 = Rc<RefCell<Option<Box<dyn FnMut() + 'static>>>>;

/// FLTK wrapper for StructuredRichDisplay with scrollbar and event handling
pub struct FltkStructuredRichDisplay {
    pub group: fltk::group::Group,
    pub display: Rc<RefCell<StructuredRichDisplay>>,
    link_cb: Callback<String>,
    hover_cb: Callback<Option<String>>,
    change_cb: MutCallback0,
    paragraph_cb: MutCallback<BlockType>,
}

const SCROLLBAR_WIDTH: i32 = 15;

impl FltkStructuredRichDisplay {
    pub fn new(x: i32, y: i32, w: i32, h: i32, edit_mode: bool) -> Self {
        let mut widget = fltk::group::Group::new(x, y, w, h, None);

        // Create structured rich display
        let display = Rc::new(RefCell::new(StructuredRichDisplay::new(
            x,
            y,
            w - SCROLLBAR_WIDTH,
            h,
        )));

        // Track click count for triple-click detection
        let last_click_time = Rc::new(RefCell::new(Instant::now()));
        let last_click_count = Rc::new(RefCell::new(0));

        // Track when a link click is in progress to prevent cursor repositioning
        let link_click_in_progress = Rc::new(RefCell::new(false));

        // Set cursor visibility based on edit mode
        display.borrow_mut().set_cursor_visible(edit_mode);

        // Callbacks holders
        let link_callback: Callback<String> = Rc::new(RefCell::new(None));
        let change_callback: MutCallback0 = Rc::new(RefCell::new(None));
        let hover_callback: Callback<Option<String>> = Rc::new(RefCell::new(None));
        let paragraph_callback: MutCallback<BlockType> = Rc::new(RefCell::new(None));

        // Create vertical responsive scrollbar
        let mut vscroll = ResponsiveScrollbar::new(
            x + w - SCROLLBAR_WIDTH,
            y,
            SCROLLBAR_WIDTH,
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
                disp.draw(&mut FltkDrawContext::from_widget_ptr(w));

                // Draw children (scrollbar)
                w.draw_children();
            }
        });

        widget.handle({
            let display = display.clone();
            let mut vscroll_handle = vscroll.clone();
            let click_time = last_click_time.clone();
            let click_count = last_click_count.clone();
            let link_click_flag = link_click_in_progress.clone();
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
                    let (desired_hover, desired_target, mouse_over) = {
                        let d = display.borrow_mut();
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
                        // Toggle checklist markers on left-click in edit mode
                        if edit_mode && fltk::app::event_button() == 1 {
                            let local_x = fltk::app::event_x() - w.x();
                            let local_y = fltk::app::event_y() - w.y();
                            let toggled = {
                                let mut disp = display.borrow_mut();
                                if let Some(block_idx) = disp.checklist_marker_hit(local_x, local_y)
                                {
                                    disp.editor_mut()
                                        .toggle_checkmark_at(block_idx)
                                        .unwrap_or_default()
                                } else {
                                    false
                                }
                            };
                            if toggled {
                                if let Some(cb) = &mut *change_cb.borrow_mut() {
                                    (cb)();
                                }
                                w.redraw();
                                return true;
                            }
                        }

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
                                if !blocks.is_empty() && cur.block_index < blocks.len() {
                                    blocks[cur.block_index].block_type.clone()
                                } else {
                                    BlockType::Paragraph
                                }
                            };
                            let w_for_actions = w.clone();
                            let actions = crate::context_menu::MenuActions {
                                has_selection,
                                current_block,
                                set_paragraph: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Paragraph)
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                set_heading1: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Heading { level: 1 })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                set_heading2: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Heading { level: 2 })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                set_heading3: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .set_block_type(BlockType::Heading { level: 3 })
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_code_block: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_code_block().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_quote: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        // Toggle: if current is quote -> paragraph, else -> quote
                                        let set_to_quote = {
                                            let d = display.borrow();
                                            let ed = d.editor();
                                            let cur = ed.cursor();
                                            let doc = ed.document();
                                            let blocks = doc.blocks();
                                            if !blocks.is_empty() && cur.block_index < blocks.len()
                                            {
                                                !matches!(
                                                    blocks[cur.block_index].block_type,
                                                    BlockType::BlockQuote
                                                )
                                            } else {
                                                true
                                            }
                                        };
                                        let mut ed = display.borrow_mut();
                                        if set_to_quote {
                                            ed.editor_mut()
                                                .set_block_type(BlockType::BlockQuote)
                                                .ok();
                                        } else {
                                            ed.editor_mut()
                                                .set_block_type(BlockType::Paragraph)
                                                .ok();
                                        }
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_list: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_list().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_checklist: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_checklist().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_ordered_list: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .toggle_ordered_list()
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_bold: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_bold().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_italic: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_italic().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_code: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_code().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_strike: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display
                                            .borrow_mut()
                                            .editor_mut()
                                            .toggle_strikethrough()
                                            .ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_underline: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_underline().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                toggle_highlight: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().toggle_highlight().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                clear_formatting: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        display.borrow_mut().editor_mut().clear_formatting().ok();
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                cut: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let mut w_r = w_for_actions.clone();
                                    move || {
                                        if let Ok(text) = display.borrow_mut().editor_mut().cut() {
                                            fltk::app::copy(&text);
                                        }
                                        if let Some(cb) = &mut *change_cb.borrow_mut() {
                                            (cb)();
                                        }
                                        w_r.redraw();
                                    }
                                }),
                                copy: Box::new({
                                    let display = display.clone();
                                    move || {
                                        let text = display.borrow().editor().copy();
                                        if !text.is_empty() {
                                            fltk::app::copy(&text);
                                        }
                                    }
                                }),
                                paste: Box::new({
                                    let w_r = w_for_actions.clone();
                                    move || {
                                        fltk::app::paste(&w_r);
                                    }
                                }),
                                edit_link: Box::new({
                                    let display = display.clone();
                                    let change_cb = change_cb.clone();
                                    let w_for_dialog = w.clone();
                                    move || {
                                        // Determine initial state: hovered link, selection, or empty
                                        let (
                                            init_target,
                                            init_text,
                                            mode_existing_link,
                                            selection_mode,
                                            link_pos,
                                        ) = {
                                            let disp = display.borrow_mut();
                                            if let Some((b, i)) = disp.hovered_link() {
                                                let doc = disp.editor().document();
                                                let block = &doc.blocks()[b];
                                                if let InlineContent::Link { link, content } =
                                                    &block.content[i]
                                                {
                                                    let text = content
                                                        .iter()
                                                        .map(|c| c.to_plain_text())
                                                        .collect::<String>();
                                                    (
                                                        link.destination.clone(),
                                                        text,
                                                        true,
                                                        false,
                                                        Some((b, i)),
                                                    )
                                                } else {
                                                    (
                                                        String::new(),
                                                        String::new(),
                                                        false,
                                                        false,
                                                        None,
                                                    )
                                                }
                                            } else if let Some((a, b)) = disp.editor().selection() {
                                                let text = disp.editor().text_in_range(a, b);
                                                (String::new(), text, false, true, None)
                                            } else {
                                                (String::new(), String::new(), false, false, None)
                                            }
                                        };

                                        let center_rect = w_for_dialog.window().map(|parent| {
                                            (parent.x(), parent.y(), parent.w(), parent.h())
                                        });

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
                                                    if editor.selection().is_some() {
                                                        editor
                                                            .replace_selection_with_link(
                                                                &dest, &txt,
                                                            )
                                                            .ok();
                                                    } else {
                                                        editor
                                                            .insert_link_at_cursor(&dest, &txt)
                                                            .ok();
                                                    }
                                                }
                                                drop(disp);
                                                if let Some(cb) = &mut *change_cb_ref.borrow_mut() {
                                                    (cb)();
                                                }
                                            },
                                            Some({
                                                let display_rm = display.clone();
                                                let change_cb_rm = change_cb.clone();
                                                move || {
                                                    if let Some((b, i)) = link_pos {
                                                        let mut disp = display_rm.borrow_mut();
                                                        disp.editor_mut().remove_link_at(b, i).ok();
                                                        drop(disp);
                                                        if let Some(cb) =
                                                            &mut *change_cb_rm.borrow_mut()
                                                        {
                                                            (cb)();
                                                        }
                                                    }
                                                }
                                            }),
                                        );
                                    }
                                }),
                            };

                            crate::context_menu::show_context_menu(x, y, actions);
                            return true;
                        }

                        let x = fltk::app::event_x();
                        let y = fltk::app::event_y();

                        // Don't process clicks on the scrollbar area
                        if x >= w.x() + w.w() - SCROLLBAR_WIDTH {
                            // Click is on scrollbar, let it handle the event
                            return false;
                        }

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
                            // Set flag to prevent drag events during link navigation
                            *link_click_flag.borrow_mut() = true;
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
                                    let mut d = display.borrow_mut();
                                    d.editor_mut().set_cursor(pos);
                                    d.record_preferred_pos(pos);
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
                        // Skip drag events if a link click is in progress
                        if *link_click_flag.borrow() {
                            return true;
                        }

                        // In edit mode, handle drag selection and auto-scroll
                        if edit_mode {
                            let x = fltk::app::event_x();
                            let y = fltk::app::event_y();

                            // Don't process drags on the scrollbar area
                            if x >= w.x() + w.w() - SCROLLBAR_WIDTH {
                                // Drag is on scrollbar, let it handle the event
                                return false;
                            }

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
                                new_scroll += delta;
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
                            display.borrow_mut().record_preferred_pos(pos);
                            let final_scroll = {
                                let mut d = display.borrow_mut();
                                d.ensure_cursor_visible(&mut FltkDrawContext::from_widget_ptr(w));
                                d.scroll_offset()
                            };
                            vscroll_handle.set_value(final_scroll as f64);
                            vscroll_handle.wake();

                            w.redraw();
                        }
                        // Hover handled above
                        true
                    }
                    Event::Released => {
                        // Clear link click flag on mouse release
                        *link_click_flag.borrow_mut() = false;
                        true
                    }
                    Event::Move | Event::Enter | Event::Leave => {
                        // Hover handled above
                        let x = fltk::app::event_x();
                        // Wake up the scrollbar if we're getting near it
                        if x >= w.x() + w.w() - 3 * SCROLLBAR_WIDTH {
                            vscroll_handle.wake();
                        }
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
                        let mut text_input = fltk::app::event_text();
                        let state = fltk::app::event_state();

                        // Handle editing keys if in edit mode
                        if edit_mode {
                            let mut handled = false;
                            let mut did_horizontal = false;

                            // Ctrl/Cmd+K: Open link editor dialog
                            #[cfg(target_os = "macos")]
                            let cmd_modifier = state.contains(Shortcut::Command);
                            #[cfg(not(target_os = "macos"))]
                            let cmd_modifier = state.contains(Shortcut::Ctrl);

                            if cmd_modifier
                                && (key == Key::from_char('k') || key == Key::from_char('K'))
                            {
                                // Gather current context: hovered link or selection
                                let disp = display.borrow_mut();
                                let hovered = disp.hovered_link();
                                let (
                                    init_target,
                                    init_text,
                                    mode_existing_link,
                                    selection_mode,
                                    link_pos,
                                ) = if let Some((b, i)) = hovered {
                                    // Prefill from hovered link
                                    let doc = disp.editor().document();
                                    let block = &doc.blocks()[b];
                                    if let InlineContent::Link { link, content } = &block.content[i]
                                    {
                                        let text = content
                                            .iter()
                                            .map(|c| c.to_plain_text())
                                            .collect::<String>();
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
                                let center_rect = w
                                    .window()
                                    .map(|parent| (parent.x(), parent.y(), parent.w(), parent.h()));

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
                                                editor
                                                    .replace_selection_with_link(&dest, &txt)
                                                    .ok();
                                            } else {
                                                editor.insert_link_at_cursor(&dest, &txt).ok();
                                            }
                                        }
                                        drop(disp);
                                        if let Some(cb) = &mut *change_cb_ref.borrow_mut() {
                                            (cb)();
                                        }
                                    },
                                    Some({
                                        let display_rm = display.clone();
                                        let change_cb_rm = change_cb.clone();
                                        move || {
                                            if let Some((b, i)) = link_pos {
                                                let mut disp = display_rm.borrow_mut();
                                                disp.editor_mut().remove_link_at(b, i).ok();
                                                drop(disp);
                                                if let Some(cb) = &mut *change_cb_rm.borrow_mut() {
                                                    (cb)();
                                                }
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
                                        && key == Key::F10;
                                    is_menu_key || shift_f10
                                };

                                if open_context_menu {
                                    let x = fltk::app::event_x();
                                    let y = fltk::app::event_y();

                                    let has_selection =
                                        display.borrow().editor().selection().is_some();
                                    let w_for_actions = w.clone();
                                    let actions = crate::context_menu::MenuActions {
                                        has_selection,
                                        current_block: {
                                            let d = display.borrow();
                                            let ed = d.editor();
                                            let cur = ed.cursor();
                                            let doc = ed.document();
                                            let blocks = doc.blocks();
                                            if !blocks.is_empty() && cur.block_index < blocks.len()
                                            {
                                                blocks[cur.block_index].block_type.clone()
                                            } else {
                                                BlockType::Paragraph
                                            }
                                        },
                                        set_paragraph: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .set_block_type(BlockType::Paragraph)
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_quote: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_quote()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        set_heading1: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .set_block_type(BlockType::Heading { level: 1 })
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        set_heading2: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .set_block_type(BlockType::Heading { level: 2 })
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        set_heading3: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .set_block_type(BlockType::Heading { level: 3 })
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_code_block: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_code_block()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_list: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_list()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_checklist: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_checklist()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_ordered_list: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_ordered_list()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_bold: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_bold()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_italic: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_italic()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_code: Box::new({
                                            let display = display.clone();

                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_code()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_strike: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_strikethrough()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_underline: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_underline()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        toggle_highlight: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .toggle_highlight()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        clear_formatting: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                display
                                                    .borrow_mut()
                                                    .editor_mut()
                                                    .clear_formatting()
                                                    .ok();
                                                w_r.redraw();
                                            }
                                        }),
                                        cut: Box::new({
                                            let display = display.clone();
                                            let mut w_r = w_for_actions.clone();
                                            move || {
                                                if let Ok(text) =
                                                    display.borrow_mut().editor_mut().cut()
                                                {
                                                    fltk::app::copy(&text);
                                                }
                                                w_r.redraw();
                                            }
                                        }),
                                        copy: Box::new({
                                            let display = display.clone();
                                            move || {
                                                let text = display.borrow().editor().copy();
                                                if !text.is_empty() {
                                                    fltk::app::copy(&text);
                                                }
                                            }
                                        }),
                                        paste: Box::new({
                                            let w_r = w_for_actions.clone();
                                            move || {
                                                fltk::app::paste(&w_r);
                                            }
                                        }),
                                        edit_link: Box::new({
                                            let display = display.clone();
                                            let w_for_dialog = w.clone();
                                            move || {
                                                // Determine initial state: hovered link, selection, or empty
                                                let (
                                                    init_target,
                                                    init_text,
                                                    mode_existing_link,
                                                    selection_mode,
                                                    link_pos,
                                                ) = {
                                                    let disp = display.borrow_mut();
                                                    if let Some((b, i)) = disp.hovered_link() {
                                                        let doc = disp.editor().document();
                                                        let block = &doc.blocks()[b];
                                                        if let InlineContent::Link {
                                                            link,
                                                            content,
                                                        } = &block.content[i]
                                                        {
                                                            let text = content
                                                                .iter()
                                                                .map(|c| c.to_plain_text())
                                                                .collect::<String>();
                                                            (
                                                                link.destination.clone(),
                                                                text,
                                                                true,
                                                                false,
                                                                Some((b, i)),
                                                            )
                                                        } else {
                                                            (
                                                                String::new(),
                                                                String::new(),
                                                                false,
                                                                false,
                                                                None,
                                                            )
                                                        }
                                                    } else if let Some((a, b)) =
                                                        disp.editor().selection()
                                                    {
                                                        let text =
                                                            disp.editor().text_in_range(a, b);
                                                        (String::new(), text, false, true, None)
                                                    } else {
                                                        (
                                                            String::new(),
                                                            String::new(),
                                                            false,
                                                            false,
                                                            None,
                                                        )
                                                    }
                                                };

                                                let center_rect =
                                                    w_for_dialog.window().map(|parent| {
                                                        (
                                                            parent.x(),
                                                            parent.y(),
                                                            parent.w(),
                                                            parent.h(),
                                                        )
                                                    });

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
                                                            editor
                                                                .edit_link_at(b, i, &dest, &txt)
                                                                .ok();
                                                        } else if !txt.is_empty() {
                                                            if editor.selection().is_some() {
                                                                editor
                                                                    .replace_selection_with_link(
                                                                        &dest, &txt,
                                                                    )
                                                                    .ok();
                                                            } else {
                                                                editor
                                                                    .insert_link_at_cursor(
                                                                        &dest, &txt,
                                                                    )
                                                                    .ok();
                                                            }
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
                                let cmd_alt_modifier = state
                                    .contains(Shortcut::Command | Shortcut::Alt)
                                    && !state.contains(Shortcut::Shift);
                                #[cfg(not(target_os = "macos"))]
                                let cmd_alt_modifier = state
                                    .contains(Shortcut::Ctrl | Shortcut::Alt)
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
                                // Cmd/Ctrl-J (insert hard line break)
                                else if cmd_modifier && key == Key::from_char('j') {
                                    let mut disp = display.borrow_mut();
                                    disp.editor_mut().insert_hard_break().ok();
                                    if let Some(cb) = &mut *change_cb.borrow_mut() {
                                        (cb)();
                                    }
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
                                // Cmd/Ctrl-Shift-6 (toggle code paragraph)
                                else if cmd_shift_modifier
                                    && (key == Key::from_char('6') || key == Key::from_char('^'))
                                {
                                    let mut disp = display.borrow_mut();
                                    disp.editor_mut().toggle_code_block().ok();
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
                                // Check for Cmd/Ctrl-Shift-8 (toggle bullet list)
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
                                // Check for Cmd/Ctrl-Shift-7 (toggle numbered list)
                                // On US keyboards, Shift-7 produces '&'
                                else if cmd_shift_modifier
                                    && (key == Key::from_char('7') || key == Key::from_char('&'))
                                {
                                    let mut disp = display.borrow_mut();
                                    disp.editor_mut().toggle_ordered_list().ok();
                                    if let Some(cb) = &mut *change_cb.borrow_mut() {
                                        (cb)();
                                    }
                                    handled = true;
                                }
                                // Cmd/Ctrl-Shift-9: toggle checklist
                                else if cmd_shift_modifier
                                    && (key == Key::from_char('9') || key == Key::from_char('('))
                                {
                                    let mut disp = display.borrow_mut();
                                    disp.editor_mut().toggle_checklist().ok();
                                    if let Some(cb) = &mut *change_cb.borrow_mut() {
                                        (cb)();
                                    }
                                    handled = true;
                                }
                                // Cmd/Ctrl-Shift-5: toggle quote paragraph
                                else if cmd_shift_modifier
                                    && (key == Key::from_char('5') || key == Key::from_char('%'))
                                {
                                    let mut disp = display.borrow_mut();
                                    disp.editor_mut().toggle_quote().ok();
                                    if let Some(cb) = &mut *change_cb.borrow_mut() {
                                        (cb)();
                                    }
                                    handled = true;
                                }
                                // Cmd/Ctrl-Alt-Enter: toggle current checklist state
                                else if cmd_alt_modifier && key == Key::Enter {
                                    let mut disp = display.borrow_mut();
                                    let changed = disp
                                        .editor_mut()
                                        .toggle_current_checkmark()
                                        .unwrap_or(false);
                                    if changed && let Some(cb) = &mut *change_cb.borrow_mut() {
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

                                    // Check if Shift is held for selection extension
                                    let shift_held = state.contains(Shortcut::Shift);
                                    // Check for word navigation modifier (Alt on macOS, Ctrl elsewhere)
                                    #[cfg(target_os = "macos")]
                                    let word_mod = state.contains(Shortcut::Alt)
                                        && !state.contains(Shortcut::Command);
                                    #[cfg(not(target_os = "macos"))]
                                    let word_mod = state.contains(Shortcut::Ctrl)
                                        && !state.contains(Shortcut::Shift);
                                    // Check for line navigation modifier (Cmd on macOS)
                                    #[cfg(target_os = "macos")]
                                    let line_mod = state.contains(Shortcut::Command);
                                    #[cfg(not(target_os = "macos"))]
                                    let line_mod = false;

                                    match key {
                                        Key::BackSpace => {
                                            {
                                                let editor = disp.editor_mut();
                                                if word_mod {
                                                    editor.delete_word_backward().ok();
                                                } else {
                                                    editor.delete_backward().ok();
                                                }
                                            }
                                            // non-vertical action
                                            did_horizontal = true;
                                            if let Some(cb) = &mut *change_cb.borrow_mut() {
                                                (cb)();
                                            }
                                            handled = true;
                                        }
                                        Key::Delete => {
                                            {
                                                let editor = disp.editor_mut();
                                                if word_mod {
                                                    editor.delete_word_forward().ok();
                                                } else {
                                                    editor.delete_forward().ok();
                                                }
                                            }
                                            // non-vertical action
                                            did_horizontal = true;
                                            if let Some(cb) = &mut *change_cb.borrow_mut() {
                                                (cb)();
                                            }
                                            handled = true;
                                        }
                                        Key::Left => {
                                            if line_mod {
                                                // Cmd-Left on macOS: Jump to line start
                                                disp.move_cursor_visual_line_start(
                                                    shift_held,
                                                    &mut FltkDrawContext::from_widget_ptr(w),
                                                );
                                                // non-vertical action
                                                did_horizontal = true;
                                                handled = true;
                                            } else {
                                                {
                                                    let editor = disp.editor_mut();
                                                    if word_mod {
                                                        if shift_held {
                                                            editor.move_word_left_extend();
                                                        } else {
                                                            editor.move_word_left();
                                                        }
                                                    } else if shift_held {
                                                        editor.move_cursor_left_extend();
                                                    } else {
                                                        editor.move_cursor_left();
                                                    }
                                                }
                                                // non-vertical action
                                                did_horizontal = true;
                                                handled = true;
                                            }
                                        }
                                        Key::Right => {
                                            if line_mod {
                                                // Cmd-Right on macOS: Jump to line end
                                                disp.move_cursor_visual_line_end_precise(
                                                    shift_held,
                                                    &mut FltkDrawContext::from_widget_ptr(w),
                                                );
                                                // non-vertical action
                                                did_horizontal = true;
                                                handled = true;
                                            } else {
                                                {
                                                    let editor = disp.editor_mut();
                                                    if word_mod {
                                                        if shift_held {
                                                            editor.move_word_right_extend();
                                                        } else {
                                                            editor.move_word_right();
                                                        }
                                                    } else if shift_held {
                                                        editor.move_cursor_right_extend();
                                                    } else {
                                                        editor.move_cursor_right();
                                                    }
                                                }
                                                // non-vertical action
                                                did_horizontal = true;
                                                handled = true;
                                            }
                                        }
                                        Key::Up => {
                                            // Visual line-aware up movement using precise font metrics
                                            disp.move_cursor_visual_up(
                                                shift_held,
                                                &mut FltkDrawContext::from_widget_ptr(w),
                                            );
                                            handled = true;
                                        }
                                        Key::Down => {
                                            // Visual line-aware down movement using precise font metrics
                                            disp.move_cursor_visual_down(
                                                shift_held,
                                                &mut FltkDrawContext::from_widget_ptr(w),
                                            );
                                            handled = true;
                                        }
                                        Key::Home => {
                                            disp.move_cursor_visual_line_start(
                                                shift_held,
                                                &mut FltkDrawContext::from_widget_ptr(w),
                                            );
                                            // non-vertical action
                                            did_horizontal = true;
                                            handled = true;
                                        }
                                        Key::End => {
                                            disp.move_cursor_visual_line_end_precise(
                                                shift_held,
                                                &mut FltkDrawContext::from_widget_ptr(w),
                                            );
                                            // non-vertical action
                                            did_horizontal = true;
                                            handled = true;
                                        }
                                        Key::Enter => {
                                            let alt_pressed = state.contains(Shortcut::Alt);
                                            let ctrl_pressed = state.contains(Shortcut::Ctrl);
                                            let cmd_pressed = state.contains(Shortcut::Command);
                                            let force_hard_break = !cmd_pressed
                                                && !ctrl_pressed
                                                && (shift_held || alt_pressed);

                                            if force_hard_break {
                                                disp.editor_mut().insert_hard_break().ok();
                                            } else {
                                                disp.editor_mut().insert_newline().ok();
                                            }
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
                                            let has_cmd_modifier =
                                                state.contains(Shortcut::Command);
                                            #[cfg(not(target_os = "macos"))]
                                            let has_cmd_modifier = state.contains(Shortcut::Ctrl);

                                            if !has_cmd_modifier {
                                                let compose_result = fltk::app::compose();
                                                if compose_result.is_some() {
                                                    text_input = fltk::app::event_text();
                                                }

                                                let mut text_changed = false;
                                                {
                                                    let editor = disp.editor_mut();

                                                    if let Some(del) = compose_result {
                                                        let delete_bytes = del.max(0) as usize;
                                                        if delete_bytes > 0
                                                            && matches!(
                                                                editor.delete_backward_bytes(
                                                                    delete_bytes
                                                                ),
                                                                Ok(true)
                                                            )
                                                        {
                                                            text_changed = true;
                                                            did_horizontal = true;
                                                        }
                                                    }

                                                    if !text_input.is_empty()
                                                        && editor.insert_text(&text_input).is_ok()
                                                    {
                                                        text_changed = true;
                                                        did_horizontal = true;
                                                    }
                                                }

                                                if text_changed {
                                                    if let Some(cb) = &mut *change_cb.borrow_mut() {
                                                        (cb)();
                                                    }
                                                    handled = true;
                                                } else if compose_result.is_some() {
                                                    handled = true;
                                                }
                                            }
                                        }
                                    }
                                }
                            }

                            if handled {
                                // After handling an edit/cursor move, ensure cursor is visible
                                let new_scroll = {
                                    let mut disp = display.borrow_mut();
                                    if did_horizontal {
                                        let cursor = disp.editor().cursor();
                                        disp.record_preferred_pos(cursor);
                                    }
                                    disp.ensure_cursor_visible(
                                        &mut FltkDrawContext::from_widget_ptr(w),
                                    );
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
                            let d = display.borrow_mut();
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
            move |_w, x, y, width, height| {
                // Update display size
                display
                    .borrow_mut()
                    .resize(x, y, width - SCROLLBAR_WIDTH, height);

                // Reposition scrollbar
                vscroll_resize.resize(x + width - SCROLLBAR_WIDTH, y, SCROLLBAR_WIDTH, height);

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
            paragraph_cb: paragraph_callback,
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

    pub fn notify_change(&self) {
        if let Ok(mut cb_ref) = self.change_cb.try_borrow_mut()
            && let Some(cb) = &mut *cb_ref
        {
            (cb)();
        }
        let mut group = self.group.clone();
        group.redraw();
    }

    pub fn set_paragraph_callback(&self, cb: Option<Box<dyn FnMut(BlockType) + 'static>>) {
        // *self.paragraph_cb.borrow_mut() = cb.clone();
        self.display
            .borrow_mut()
            .editor_mut()
            .set_paragraph_change_callback(cb);
        // self.emit_paragraph_state();
    }

    pub fn emit_paragraph_state(&self) {
        if let Some(block_type) = self.current_block_type() {
            println!("Emitting paragraph type: {:?}", block_type);
            if let Ok(mut cb_ref) = self.paragraph_cb.try_borrow_mut()
                && let Some(cb) = &mut *cb_ref
            {
                (cb)(block_type);
            }
        }
    }

    pub fn current_block_type(&self) -> Option<BlockType> {
        let disp = self.display.borrow();
        let editor = disp.editor();
        let blocks = editor.document().blocks();
        let idx = editor.cursor().block_index;
        blocks.get(idx).map(|b| b.block_type.clone())
    }
}
