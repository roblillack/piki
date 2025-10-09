// FLTK integration for TextDisplay widget
// Simple wrapper implementation

use crate::text_buffer::TextBuffer;
use crate::text_display::{DrawContext, TextDisplay, CursorStyle, StyleTableEntry, WrapMode};
use fltk::{prelude::*, enums::*, draw as fltk_draw};
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
    let text_display = Rc::new(RefCell::new(TextDisplay::new(x, y, w, h)));

    widget.draw({
        let text_display = text_display.clone();
        move |w| {
            let disp = text_display.borrow();
            let has_focus = fltk::app::focus().map(|f| f.as_base_widget()).as_ref() == Some(&w.as_base_widget());
            let is_active = w.active();
            let mut ctx = FltkDrawContext::new(has_focus, is_active);

            // Draw background
            fltk_draw::set_draw_color(w.color());
            fltk_draw::draw_rectf(w.x(), w.y(), w.w(), w.h());

            // Draw the text display
            disp.draw(&mut ctx);
        }
    });

    widget.handle({
        let text_display = text_display.clone();
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

                    match key {
                        Key::Left => {
                            disp.move_left();
                            disp.show_insert_position();
                            w.redraw();
                            true
                        }
                        Key::Right => {
                            disp.move_right();
                            disp.show_insert_position();
                            w.redraw();
                            true
                        }
                        Key::Up => {
                            disp.move_up();
                            disp.show_insert_position();
                            w.redraw();
                            true
                        }
                        Key::Down => {
                            disp.move_down();
                            disp.show_insert_position();
                            w.redraw();
                            true
                        }
                        Key::Home => {
                            let line_start = disp.line_start(disp.insert_position());
                            disp.set_insert_position(line_start);
                            disp.show_insert_position();
                            w.redraw();
                            true
                        }
                        Key::End => {
                            let line_end = disp.line_end(disp.insert_position());
                            disp.set_insert_position(line_end);
                            disp.show_insert_position();
                            w.redraw();
                            true
                        }
                        _ => {
                            // Handle text input
                            if let Some(text) = fltk::app::event_text().chars().next() {
                                if !text.is_control() {
                                    disp.insert(&text.to_string());
                                    disp.show_insert_position();
                                    w.redraw();
                                    return true;
                                }
                            }
                            false
                        }
                    }
                }
                _ => false,
            }
        }
    });

    widget.end();

    (widget, text_display)
}
