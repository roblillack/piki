//! The red "ON AIR" bar shown at the top of the window while Live Note Sharing
//! is active.
//!
//! Layout: a red background with a blinking recording light, an `ON AIR` label,
//! a clickable link (rendered as a real hyperlink) showing the shareable URL of
//! the currently visible note, and a `Stop` button. The link text is updated as
//! the user navigates (see `main.rs`), so it always points at the note on
//! screen. [`OnAirBar::tick`] is driven from the app's animation timer to blink
//! the light.

use fltk::enums::{Align, Color, Cursor, Event, Font, FrameType};
use fltk::{button, draw, frame, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Height of the ON AIR bar in pixels.
pub const HEIGHT: i32 = 30;

/// Bar background (GitHub "danger" red).
const BAR_RGB: (u8, u8, u8) = (207, 34, 46);
/// The two colors the recording light alternates between.
const LIGHT_YELLOW: (u8, u8, u8) = (250, 204, 21);
const LIGHT_RED: (u8, u8, u8) = (255, 79, 79);
/// A dark ring around the light so it reads as a lamp in both phases.
const LIGHT_RING: (u8, u8, u8) = (90, 0, 0);

const TEXT_COLOR: Color = Color::White;
const STOP_WIDTH: i32 = 70;
const LABEL_WIDTH: i32 = 60;
const LIGHT_DIAMETER: i32 = 12;
const PADDING: i32 = 8;
const GAP: i32 = 6;

fn color(rgb: (u8, u8, u8)) -> Color {
    Color::from_rgb(rgb.0, rgb.1, rgb.2)
}

type ClickCallback = Rc<RefCell<Option<Box<dyn FnMut()>>>>;

/// A red status bar indicating that the current session is being shared live.
pub struct OnAirBar {
    background: frame::Frame,
    light: frame::Frame,
    label: frame::Frame,
    link: frame::Frame,
    stop_btn: button::Button,
    url: Rc<RefCell<String>>,
    on_link: ClickCallback,
    /// Which color the light is currently showing; used to skip redundant
    /// redraws while blinking.
    light_yellow: bool,
}

impl OnAirBar {
    /// Create the bar at the given position and width. Hidden by default.
    pub fn new(x: i32, y: i32, w: i32) -> Self {
        let bg = color(BAR_RGB);

        let mut background = frame::Frame::new(x, y, w, HEIGHT, None);
        background.set_frame(FrameType::FlatBox);
        background.set_color(bg);

        // Blinking recording light, drawn as a filled circle with a dark ring.
        let mut light = frame::Frame::new(
            x + PADDING,
            y + (HEIGHT - LIGHT_DIAMETER) / 2,
            LIGHT_DIAMETER,
            LIGHT_DIAMETER,
            None,
        );
        light.set_frame(FrameType::FlatBox);
        light.set_color(color(LIGHT_RED));
        light.draw(move |f| {
            draw::set_draw_color(color(BAR_RGB));
            draw::draw_rectf(f.x(), f.y(), f.w(), f.h());
            draw::set_draw_color(f.color());
            draw::draw_pie(f.x(), f.y(), f.w(), f.h(), 0.0, 360.0);
            draw::set_draw_color(color(LIGHT_RING));
            draw::draw_arc(f.x(), f.y(), f.w(), f.h(), 0.0, 360.0);
        });

        let label_x = x + PADDING + LIGHT_DIAMETER + GAP;
        let mut label = frame::Frame::new(label_x, y, LABEL_WIDTH, HEIGHT, Some("ON AIR"));
        label.set_frame(FrameType::FlatBox);
        label.set_align(Align::Inside | Align::Left);
        label.set_color(bg);
        label.set_label_color(TEXT_COLOR);
        label.set_label_font(Font::HelveticaBold);
        label.set_label_size(fltk::app::font_size() - 1);

        // The link is a plain frame (no button chrome, focus ring, or pressed
        // state) drawn as an underlined hyperlink, with a hand cursor on hover.
        let link_x = label_x + LABEL_WIDTH + GAP;
        let link_w = (x + w - STOP_WIDTH - PADDING - GAP - link_x).max(50);
        let mut link = frame::Frame::new(link_x, y, link_w, HEIGHT, None);
        link.set_frame(FrameType::FlatBox);
        link.set_color(bg);
        link.set_label_size(fltk::app::font_size() - 1);
        link.draw(|f| {
            draw::set_draw_color(color(BAR_RGB));
            draw::draw_rectf(f.x(), f.y(), f.w(), f.h());
            let text = f.label();
            if text.is_empty() {
                return;
            }
            draw::set_font(Font::Helvetica, f.label_size());
            draw::set_draw_color(TEXT_COLOR);
            draw::draw_text2(
                &text,
                f.x(),
                f.y(),
                f.w(),
                f.h(),
                Align::Left | Align::Inside,
            );
            // Underline the text (clamped to the frame width).
            let (tw, th) = draw::measure(&text, false);
            let underline_w = tw.min(f.w());
            let underline_y = f.y() + (f.h() + th) / 2 - 1;
            draw::draw_line(f.x(), underline_y, f.x() + underline_w, underline_y);
        });

        let on_link: ClickCallback = Rc::new(RefCell::new(None));
        {
            let on_link = on_link.clone();
            link.handle(move |f, ev| match ev {
                Event::Enter => {
                    if let Some(mut win) = f.window() {
                        win.set_cursor(Cursor::Hand);
                    }
                    true
                }
                Event::Leave => {
                    if let Some(mut win) = f.window() {
                        win.set_cursor(Cursor::Default);
                    }
                    true
                }
                // Accept the push so a matching release is delivered, then fire
                // the click on release (button-like behavior).
                Event::Push => true,
                Event::Released => {
                    if let Some(cb) = on_link.borrow_mut().as_mut() {
                        cb();
                    }
                    true
                }
                _ => false,
            });
        }

        let mut stop_btn = button::Button::new(
            x + w - STOP_WIDTH - PADDING,
            y + 4,
            STOP_WIDTH,
            HEIGHT - 8,
            Some("Stop"),
        );
        stop_btn.set_tooltip("Stop sharing");

        background.hide();
        light.hide();
        label.hide();
        link.hide();
        stop_btn.hide();

        OnAirBar {
            background,
            light,
            label,
            link,
            stop_btn,
            url: Rc::new(RefCell::new(String::new())),
            on_link,
            light_yellow: false,
        }
    }

    /// Advance the blinking light. Driven from the app's animation timer with
    /// milliseconds since start; a no-op while the bar is hidden.
    pub fn tick(&mut self, ms_since_start: u64) {
        if !self.background.visible() {
            return;
        }
        let yellow = (ms_since_start / 500).is_multiple_of(2);
        if yellow != self.light_yellow {
            self.light_yellow = yellow;
            self.light
                .set_color(color(if yellow { LIGHT_YELLOW } else { LIGHT_RED }));
            self.light.redraw();
        }
    }

    /// Update the shown URL (link label + tooltip).
    pub fn set_url(&mut self, url: &str) {
        *self.url.borrow_mut() = url.to_string();
        self.link.set_label(url);
        self.link.set_tooltip(url);
        self.link.redraw();
    }

    /// The URL currently shown in the bar.
    pub fn url(&self) -> String {
        self.url.borrow().clone()
    }

    /// Register a callback for clicking the link (opens the URL in the browser).
    pub fn on_link_click<F: FnMut() + 'static>(&mut self, cb: F) {
        *self.on_link.borrow_mut() = Some(Box::new(cb));
    }

    /// Register a callback for the Stop button.
    pub fn on_stop<F: FnMut() + 'static>(&mut self, mut cb: F) {
        self.stop_btn.set_callback(move |_| cb());
    }

    pub fn show(&mut self) {
        self.background.show();
        self.light.show();
        self.label.show();
        self.link.show();
        self.stop_btn.show();
    }

    pub fn hide(&mut self) {
        self.background.hide();
        self.light.hide();
        self.label.hide();
        self.link.hide();
        self.stop_btn.hide();
    }

    pub fn visible(&self) -> bool {
        self.background.visible()
    }

    pub fn height(&self) -> i32 {
        HEIGHT
    }

    /// Reposition/resize the bar and its children to span `w` at `(x, y)`.
    pub fn resize(&mut self, x: i32, y: i32, w: i32) {
        self.background.resize(x, y, w, HEIGHT);
        self.light.resize(
            x + PADDING,
            y + (HEIGHT - LIGHT_DIAMETER) / 2,
            LIGHT_DIAMETER,
            LIGHT_DIAMETER,
        );

        let label_x = x + PADDING + LIGHT_DIAMETER + GAP;
        self.label.resize(label_x, y, LABEL_WIDTH, HEIGHT);

        let link_x = label_x + LABEL_WIDTH + GAP;
        let link_w = (x + w - STOP_WIDTH - PADDING - GAP - link_x).max(50);
        self.link.resize(link_x, y, link_w, HEIGHT);

        self.stop_btn
            .resize(x + w - STOP_WIDTH - PADDING, y + 4, STOP_WIDTH, HEIGHT - 8);
    }
}
