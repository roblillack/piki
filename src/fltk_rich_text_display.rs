// FLTK integration for RichTextDisplay widget
use crate::fltk_text_display::FltkDrawContext;
use crate::rich_text_display::RichTextDisplay;
use fltk::{enums::*, prelude::*, valuator::Scrollbar};
use std::cell::RefCell;
use std::rc::Rc;

/// Create a Rich Text Display widget with scrollbar
pub fn create_rich_text_display_widget(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> (fltk::group::Group, Rc<RefCell<RichTextDisplay>>) {
    let mut widget = fltk::group::Group::new(x, y, w, h, None);

    let scrollbar_size = 15;

    // Create rich text display
    let rich_display = Rc::new(RefCell::new(RichTextDisplay::new(x, y, w - scrollbar_size, h)));

    let bg_color = Color::White;

    // Create vertical scrollbar
    let mut vscroll = Scrollbar::default()
        .with_pos(x + w - scrollbar_size, y)
        .with_size(scrollbar_size, h);
    vscroll.set_type(fltk::valuator::ScrollbarType::Vertical);
    vscroll.set_callback({
        let display = rich_display.clone();
        let mut widget_clone = widget.clone();
        move |s| {
            let value = s.value() as i32;
            display.borrow_mut().set_scroll(value);
            widget_clone.redraw();
        }
    });
    vscroll.show();

    // Initialize scrollbar
    vscroll.set_bounds(0.0, 1000.0); // Will be updated during draw
    vscroll.set_slider_size(0.5);
    vscroll.set_step(1.0, 10);
    vscroll.set_value(0.0);

    widget.draw({
        let display = rich_display.clone();
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
        let display = rich_display.clone();
        let mut vscroll_handle = vscroll.clone();
        move |w, event| {
            match event {
                Event::Push => {
                    w.take_focus().ok();
                    true
                }
                Event::MouseWheel => {
                    // Handle scroll wheel
                    let dy = fltk::app::event_dy();
                    let scroll_amount = dy as i32 * 20;
                    let mut disp = display.borrow_mut();
                    let scroll = disp.scroll_offset();
                    let new_scroll = (scroll - scroll_amount).max(0); // Negative dy means scroll up
                    disp.set_scroll(new_scroll);
                    vscroll_handle.set_value(new_scroll as f64);
                    w.redraw();
                    true
                }
                Event::KeyDown => {
                    let key = fltk::app::event_key();
                    let mut disp = display.borrow_mut();
                    let scroll = disp.scroll_offset();
                    let visible = disp.h();

                    let new_scroll = match key {
                        Key::Up => (scroll - 20).max(0),
                        Key::Down => scroll + 20,
                        Key::PageUp => (scroll - visible).max(0),
                        Key::PageDown => scroll + visible,
                        Key::Home => 0,
                        Key::End => {
                            let content_height = disp.content_height();
                            (content_height - visible).max(0)
                        }
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

    // Handle widget resize
    widget.resize_callback({
        let display = rich_display.clone();
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

    (widget, rich_display)
}
