// FLTK integration for TextDisplay widget
// Simple wrapper implementation

use crate::text_display::{DrawContext, TextDisplay};
use fltk::{prelude::*, enums::*, draw as fltk_draw, valuator::Scrollbar};
use std::rc::Rc;
use std::cell::RefCell;

/// FLTK implementation of DrawContext
pub struct FltkDrawContext {
    has_focus: bool,
    is_active: bool,
}

impl FltkDrawContext {
    pub fn new(has_focus: bool, is_active: bool) -> Self {
        FltkDrawContext {
            has_focus,
            is_active,
        }
    }
}

impl DrawContext for FltkDrawContext {
    fn set_color(&mut self, color: u32) {
        let r = ((color >> 24) & 0xFF) as u8;
        let g = ((color >> 16) & 0xFF) as u8;
        let b = ((color >> 8) & 0xFF) as u8;
        fltk_draw::set_draw_color(Color::from_rgb(r, g, b));
    }

    fn set_font(&mut self, font: u8, size: u8) {
        fltk_draw::set_font(Font::by_index(font as usize), size as i32);
    }

    fn draw_text(&mut self, text: &str, x: i32, y: i32) {
        fltk_draw::draw_text2(text, x, y, 0, 0, Align::Left);
    }

    fn draw_rect_filled(&mut self, x: i32, y: i32, w: i32, h: i32) {
        fltk_draw::draw_rectf(x, y, w, h);
    }

    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        fltk_draw::draw_line(x1, y1, x2, y2);
    }

    fn text_width(&mut self, text: &str, font: u8, size: u8) -> f64 {
        fltk_draw::set_font(Font::by_index(font as usize), size as i32);
        fltk_draw::width(text) as f64
    }

    fn text_height(&self, font: u8, size: u8) -> i32 {
        fltk_draw::set_font(Font::by_index(font as usize), size as i32);
        fltk_draw::height()
    }

    fn text_descent(&self, font: u8, size: u8) -> i32 {
        fltk_draw::set_font(Font::by_index(font as usize), size as i32);
        fltk_draw::descent()
    }

    fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        fltk_draw::push_clip(x, y, w, h);
    }

    fn pop_clip(&mut self) {
        fltk_draw::pop_clip();
    }

    fn color_average(&self, c1: u32, c2: u32, weight: f32) -> u32 {
        let r1 = ((c1 >> 24) & 0xFF) as f32;
        let g1 = ((c1 >> 16) & 0xFF) as f32;
        let b1 = ((c1 >> 8) & 0xFF) as f32;

        let r2 = ((c2 >> 24) & 0xFF) as f32;
        let g2 = ((c2 >> 16) & 0xFF) as f32;
        let b2 = ((c2 >> 8) & 0xFF) as f32;

        let r = (r1 * (1.0 - weight) + r2 * weight) as u32;
        let g = (g1 * (1.0 - weight) + g2 * weight) as u32;
        let b = (b1 * (1.0 - weight) + b2 * weight) as u32;

        (r << 24) | (g << 16) | (b << 8) | 0xFF
    }

    fn color_contrast(&self, _fg: u32, bg: u32) -> u32 {
        let r = ((bg >> 24) & 0xFF) as f32;
        let g = ((bg >> 16) & 0xFF) as f32;
        let b = ((bg >> 8) & 0xFF) as f32;

        let brightness = (r * 0.299 + g * 0.587 + b * 0.114) / 255.0;

        if brightness > 0.5 {
            0x000000FF // Black
        } else {
            0xFFFFFFFF // White
        }
    }

    fn color_inactive(&self, c: u32) -> u32 {
        let r = ((c >> 24) & 0xFF) as f32;
        let g = ((c >> 16) & 0xFF) as f32;
        let b = ((c >> 8) & 0xFF) as f32;

        let gray = (r + g + b) / 3.0;
        let r = (r * 0.5 + gray * 0.5) as u32;
        let g = (g * 0.5 + gray * 0.5) as u32;
        let b = (b * 0.5 + gray * 0.5) as u32;

        (r << 24) | (g << 16) | (b << 8) | 0xFF
    }

    fn has_focus(&self) -> bool {
        self.has_focus
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}

/// Simple FLTK widget wrapper for TextDisplay
/// Use fltk::group::Group::new() and add custom draw/handle callbacks
pub fn create_text_display_widget(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> (fltk::group::Group, Rc<RefCell<TextDisplay>>) {
    let mut widget = fltk::group::Group::new(x, y, w, h, None);

    // Calculate scrollbar size
    let scrollbar_size = 15; // Standard scrollbar width

    // Create text display with room for scrollbars
    let text_display = Rc::new(RefCell::new(TextDisplay::new(x, y, w - scrollbar_size, h - scrollbar_size)));

    // Create vertical scrollbar
    let mut vscroll = Scrollbar::default()
        .with_pos(x + w - scrollbar_size, y)
        .with_size(scrollbar_size, h - scrollbar_size);
    vscroll.set_type(fltk::valuator::ScrollbarType::Vertical);
    vscroll.set_callback({
        let text_display = text_display.clone();
        let mut widget_clone = widget.clone();
        move |s| {
            let value = s.value() as usize;
            let mut disp = text_display.borrow_mut();
            let horiz = disp.horiz_offset();
            disp.scroll(value, horiz);
            widget_clone.redraw();
        }
    });
    vscroll.show(); // Explicitly show the scrollbar

    // Create horizontal scrollbar
    let mut hscroll = Scrollbar::default()
        .with_pos(x, y + h - scrollbar_size)
        .with_size(w - scrollbar_size, scrollbar_size);
    hscroll.set_type(fltk::valuator::ScrollbarType::Horizontal);
    hscroll.set_callback({
        let text_display = text_display.clone();
        let mut widget_clone = widget.clone();
        move |s| {
            let value = s.value() as i32;
            let mut disp = text_display.borrow_mut();
            let top_line = disp.top_line_num();
            disp.scroll(top_line, value);
            widget_clone.redraw();
        }
    });
    hscroll.show(); // Explicitly show the scrollbar

    // Initialize scrollbar values before draw
    if let Some(buffer) = text_display.borrow().buffer() {
        let buf = buffer.borrow();
        let n_buffer_lines = buf.count_lines(0, buf.length());
        let n_visible_lines = ((h - scrollbar_size) / 14).max(1) as usize;

        // Set up vertical scrollbar
        // IMPORTANT: set_slider_size() expects a FRACTION (0.0-1.0)!
        let max_lines = (n_buffer_lines + 1) as f64;
        let slider_fraction = if max_lines > 0.0 {
            (n_visible_lines as f64 / max_lines).min(1.0) as f32
        } else {
            1.0
        };

        vscroll.set_bounds(0.0, max_lines);
        vscroll.set_slider_size(slider_fraction);
        vscroll.set_step(1.0, n_visible_lines as i32);
        vscroll.set_value(0.0);

        // Set up horizontal scrollbar
        let max_offset = 1000.0;
        let visible_width = (w - scrollbar_size) as f64;
        let h_slider_fraction = if max_offset > 0.0 {
            (visible_width / max_offset).min(1.0) as f32
        } else {
            1.0
        };

        hscroll.set_bounds(0.0, max_offset);
        hscroll.set_slider_size(h_slider_fraction);
        hscroll.set_step(1.0, 10);
        hscroll.set_value(0.0);
    }

    widget.draw({
        let text_display = text_display.clone();
        let mut vscroll_draw = vscroll.clone();
        let mut hscroll_draw = hscroll.clone();
        move |w| {
            let disp = text_display.borrow();
            let has_focus = fltk::app::focus().map(|f| f.as_base_widget()).as_ref() == Some(&w.as_base_widget());
            let is_active = w.active();
            let mut ctx = FltkDrawContext::new(has_focus, is_active);

            // Update scrollbar values based on current buffer and display size
            if let Some(buffer) = disp.buffer() {
                let buf = buffer.borrow();
                let n_buffer_lines = buf.count_lines(0, buf.length());
                let n_visible_lines = (disp.h() / 14).max(1) as usize;

                // Update vertical scrollbar
                // IMPORTANT: set_slider_size() expects a FRACTION (0.0-1.0), not an absolute value!
                // For 100 lines with 50 visible: slider_size should be 50/100 = 0.5
                let max_lines = (n_buffer_lines + 1) as f64;
                let slider_fraction = if max_lines > 0.0 {
                    (n_visible_lines as f64 / max_lines).min(1.0) as f32
                } else {
                    1.0
                };

                vscroll_draw.set_bounds(0.0, max_lines);
                vscroll_draw.set_slider_size(slider_fraction);
                vscroll_draw.set_step(1.0, n_visible_lines as i32);
                vscroll_draw.set_value(disp.top_line_num() as f64);

                // Update horizontal scrollbar based on longest line (simplified for now)
                let max_offset = 1000.0; // Estimate - could calculate from longest line
                let visible_width = disp.w() as f64;
                let h_slider_fraction = if max_offset > 0.0 {
                    (visible_width / max_offset).min(1.0) as f32
                } else {
                    1.0
                };

                hscroll_draw.set_bounds(0.0, max_offset);
                hscroll_draw.set_slider_size(h_slider_fraction);
                hscroll_draw.set_step(1.0, 10);
                hscroll_draw.set_value(disp.horiz_offset() as f64);
            }

            // Draw background only for the text area, not the scrollbars
            fltk_draw::set_draw_color(w.color());
            fltk_draw::draw_rectf(disp.x(), disp.y(), disp.w(), disp.h());

            // Draw the text display
            disp.draw(&mut ctx);

            // Let FLTK draw the children (scrollbars)
            w.draw_children();
        }
    });

    widget.handle({
        let text_display = text_display.clone();
        let mut vscroll_handle = vscroll.clone();
        let mut hscroll_handle = hscroll.clone();
        move |w, event| {
            match event {
                Event::Push => {
                    w.take_focus().ok();
                    w.redraw();
                    true
                }
                Event::Focus | Event::Unfocus => {
                    w.redraw();
                    true
                }
                Event::KeyDown => {
                    let key = fltk::app::event_key();
                    let mut disp = text_display.borrow_mut();

                    let handled = match key {
                        Key::Left => {
                            disp.move_left();
                            disp.show_insert_position();
                            true
                        }
                        Key::Right => {
                            disp.move_right();
                            disp.show_insert_position();
                            true
                        }
                        Key::Up => {
                            disp.move_up();
                            disp.show_insert_position();
                            true
                        }
                        Key::Down => {
                            disp.move_down();
                            disp.show_insert_position();
                            true
                        }
                        Key::Home => {
                            let line_start = disp.line_start(disp.insert_position());
                            disp.set_insert_position(line_start);
                            disp.show_insert_position();
                            true
                        }
                        Key::End => {
                            let line_end = disp.line_end(disp.insert_position());
                            disp.set_insert_position(line_end);
                            disp.show_insert_position();
                            true
                        }
                        Key::PageUp => {
                            let n_visible_lines = (disp.h() / 14).max(1) as usize;
                            let current_line = disp.count_lines(0, disp.insert_position());
                            let new_line = current_line.saturating_sub(n_visible_lines);
                            let new_pos = disp.skip_lines(0, new_line);
                            disp.set_insert_position(new_pos);
                            disp.show_insert_position();
                            true
                        }
                        Key::PageDown => {
                            let n_visible_lines = (disp.h() / 14).max(1) as usize;
                            let current_line = disp.count_lines(0, disp.insert_position());
                            let new_line = current_line + n_visible_lines;
                            let new_pos = disp.skip_lines(0, new_line);
                            disp.set_insert_position(new_pos);
                            disp.show_insert_position();
                            true
                        }
                        _ => {
                            // Handle text input
                            if let Some(text) = fltk::app::event_text().chars().next() {
                                if !text.is_control() {
                                    disp.insert(&text.to_string());
                                    disp.show_insert_position();
                                    true
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        }
                    };

                    if handled {
                        // Update scrollbar positions after keyboard navigation
                        vscroll_handle.set_value(disp.top_line_num() as f64);
                        hscroll_handle.set_value(disp.horiz_offset() as f64);
                        w.redraw();
                    }
                    handled
                }
                _ => false,
            }
        }
    });

    widget.end();
    widget.resizable(&widget);

    // Handle widget resize to update scrollbar positions
    widget.resize_callback({
        let text_display = text_display.clone();
        let mut vscroll_resize = vscroll.clone();
        let mut hscroll_resize = hscroll.clone();
        let sb_size = scrollbar_size;
        move |_w, x, y, width, height| {
            // Update text display size (leaving room for scrollbars)
            text_display.borrow_mut().resize(x, y, width - sb_size, height - sb_size);

            // Reposition and resize scrollbars
            vscroll_resize.resize(x + width - sb_size, y, sb_size, height - sb_size);
            hscroll_resize.resize(x, y + height - sb_size, width - sb_size, sb_size);
        }
    });

    (widget, text_display)
}
