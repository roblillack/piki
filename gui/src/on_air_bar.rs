//! The red "ON AIR" bar shown at the top of the window while Live Note Sharing
//! is active.
//!
//! Layout: a red background with an `ON AIR` label, a clickable link showing the
//! shareable URL of the currently visible note, and a `Stop` button. The link
//! text is updated as the user navigates (see `main.rs`), so it always points at
//! the note on screen.

use fltk::{button, enums, frame, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

/// Height of the ON AIR bar in pixels.
pub const HEIGHT: i32 = 30;

const BG_COLOR: (u8, u8, u8) = (207, 34, 46); // GitHub "danger" red
const TEXT_COLOR: enums::Color = enums::Color::White;
const STOP_WIDTH: i32 = 70;
const LABEL_WIDTH: i32 = 72;
const PADDING: i32 = 8;

/// A red status bar indicating that the current session is being shared live.
pub struct OnAirBar {
    background: frame::Frame,
    label: frame::Frame,
    link: button::Button,
    stop_btn: button::Button,
    url: Rc<RefCell<String>>,
}

impl OnAirBar {
    /// Create the bar at the given position and width. Hidden by default.
    pub fn new(x: i32, y: i32, w: i32) -> Self {
        let bg = enums::Color::from_rgb(BG_COLOR.0, BG_COLOR.1, BG_COLOR.2);

        let mut background = frame::Frame::new(x, y, w, HEIGHT, None);
        background.set_frame(enums::FrameType::FlatBox);
        background.set_color(bg);

        let mut label = frame::Frame::new(x + PADDING, y, LABEL_WIDTH, HEIGHT, Some("🔴 ON AIR"));
        label.set_frame(enums::FrameType::FlatBox);
        label.set_align(enums::Align::Inside | enums::Align::Left);
        label.set_color(bg);
        label.set_label_color(TEXT_COLOR);
        label.set_label_size(fltk::app::font_size() - 1);

        // Clickable link showing the shareable URL. A flat button styled as text.
        let link_x = x + PADDING + LABEL_WIDTH;
        let link_w = (w - LABEL_WIDTH - STOP_WIDTH - 3 * PADDING).max(50);
        let mut link = button::Button::new(link_x, y, link_w, HEIGHT, None);
        link.set_frame(enums::FrameType::FlatBox);
        link.set_align(enums::Align::Inside | enums::Align::Left);
        link.set_color(bg);
        link.set_label_color(TEXT_COLOR);
        link.set_label_size(fltk::app::font_size() - 1);
        link.set_tooltip("Open the shared note in your browser");

        // Stop button (right side).
        let mut stop_btn = button::Button::new(
            x + w - STOP_WIDTH - PADDING,
            y + 4,
            STOP_WIDTH,
            HEIGHT - 8,
            Some("Stop"),
        );
        stop_btn.set_tooltip("Stop sharing");

        background.hide();
        label.hide();
        link.hide();
        stop_btn.hide();

        OnAirBar {
            background,
            label,
            link,
            stop_btn,
            url: Rc::new(RefCell::new(String::new())),
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
    pub fn on_link_click<F: FnMut() + 'static>(&mut self, mut cb: F) {
        self.link.set_callback(move |_| cb());
    }

    /// Register a callback for the Stop button.
    pub fn on_stop<F: FnMut() + 'static>(&mut self, mut cb: F) {
        self.stop_btn.set_callback(move |_| cb());
    }

    pub fn show(&mut self) {
        self.background.show();
        self.label.show();
        self.link.show();
        self.stop_btn.show();
    }

    pub fn hide(&mut self) {
        self.background.hide();
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
        self.label.resize(x + PADDING, y, LABEL_WIDTH, HEIGHT);

        let link_x = x + PADDING + LABEL_WIDTH;
        let link_w = (w - LABEL_WIDTH - STOP_WIDTH - 3 * PADDING).max(50);
        self.link.resize(link_x, y, link_w, HEIGHT);

        self.stop_btn
            .resize(x + w - STOP_WIDTH - PADDING, y + 4, STOP_WIDTH, HEIGHT - 8);
    }
}
