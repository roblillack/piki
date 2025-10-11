// FLTK integration for StructuredRichDisplay widget

use crate::fltk_text_display::FltkDrawContext;
use crate::structured_document::InlineContent;
use crate::structured_rich_display::StructuredRichDisplay;
use fltk::{enums::*, prelude::*, valuator::Scrollbar};
use std::cell::RefCell;
use std::rc::Rc;

/// Create a Structured Rich Text Display widget with scrollbar
pub fn create_structured_rich_display_widget(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
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
                }

                w.take_focus().ok();
                true
            }
            Event::Drag | Event::Move | Event::Enter => {
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
                // Only handle scrolling keys here
                let key = fltk::app::event_key();

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
