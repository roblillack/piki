// Search Bar Widget for in-note search
// A floating search bar with input, prev/next buttons, and match count display

use fltk::{app, button, enums::*, frame, group, input, prelude::*};
use std::cell::RefCell;
use std::rc::Rc;

type SearchCallback = Rc<RefCell<Option<Box<dyn FnMut(String) + 'static>>>>;
type NavCallback = Rc<RefCell<Option<Box<dyn FnMut() + 'static>>>>;

pub const BAR_HEIGHT: i32 = 36;
const BUTTON_WIDTH: i32 = 30;
const COUNT_WIDTH: i32 = 70;
const INPUT_MIN_WIDTH: i32 = 150;

/// A floating search bar with input field, prev/next buttons, and close button
pub struct SearchBar {
    group: group::Group,
    input: input::Input,
    prev_btn: button::Button,
    next_btn: button::Button,
    close_btn: button::Button,
    count_label: frame::Frame,
    on_search: SearchCallback,
    on_next: NavCallback,
    on_prev: NavCallback,
    on_close: NavCallback,
}

impl SearchBar {
    /// Create a new search bar at the specified position
    /// The bar will be hidden by default
    pub fn new(x: i32, y: i32, w: i32) -> Self {
        // Use a group - the caller is responsible for z-ordering/layout
        let mut group = group::Group::new(x, y, w, BAR_HEIGHT, None);
        // group.set_frame(FrameType::FlatBox);
        // group.set_color(Color::from_rgb(240, 240, 240));

        // Calculate positions - buttons fixed to right, input takes remaining space
        let padding = 4;
        let right_section_width = 3 * BUTTON_WIDTH + COUNT_WIDTH + 4 * padding;

        // Child widgets are laid out in absolute window coordinates, so they must
        // be offset by the group's own (x, y) — the bar is not always at the top
        // left (e.g. when the ON AIR bar sits above it).
        let top = y + 4;

        // Input field (takes remaining space on the left)
        let input_width = (w - right_section_width - padding).max(INPUT_MIN_WIDTH);
        let mut input = input::Input::new(x + padding, top, input_width, BAR_HEIGHT - 8, None);
        // input.set_frame(FrameType::BorderBox);
        input.set_text_size(14);

        // Position elements from the right edge
        let mut right_x = x + w - padding - BUTTON_WIDTH;

        // Close button (rightmost)
        let mut close_btn = button::Button::new(right_x, top, BUTTON_WIDTH, BAR_HEIGHT - 8, "@1+");
        // close_btn.set_frame(FrameType::FlatBox);
        close_btn.set_tooltip("Close (Escape)");
        right_x -= BUTTON_WIDTH + padding;

        // Next button
        let mut next_btn = button::Button::new(right_x, top, BUTTON_WIDTH, BAR_HEIGHT - 8, "@>");
        // next_btn.set_frame(FrameType::FlatBox);
        next_btn.set_tooltip("Next match (Enter)");
        right_x -= BUTTON_WIDTH + padding;

        // Previous button
        let mut prev_btn = button::Button::new(right_x, top, BUTTON_WIDTH, BAR_HEIGHT - 8, "@<");
        // prev_btn.set_frame(FrameType::FlatBox);
        prev_btn.set_tooltip("Previous match (Shift+Enter)");
        right_x -= COUNT_WIDTH + padding;

        // Match count label
        let mut count_label = frame::Frame::new(right_x, top, COUNT_WIDTH, BAR_HEIGHT - 8, None);
        count_label.set_label_size(12);
        count_label.set_align(Align::Inside | Align::Right);

        group.end();
        group.hide();

        // Create callback holders
        let on_search: SearchCallback = Rc::new(RefCell::new(None));
        let on_next: NavCallback = Rc::new(RefCell::new(None));
        let on_prev: NavCallback = Rc::new(RefCell::new(None));
        let on_close: NavCallback = Rc::new(RefCell::new(None));

        // Wire up input callback for live search
        {
            let search_cb = on_search.clone();
            input.set_callback(move |inp| {
                let text = inp.value();
                if let Some(cb) = &mut *search_cb.borrow_mut() {
                    cb(text);
                }
            });
            input.set_trigger(CallbackTrigger::Changed);
        }

        // Wire up input key handler for Enter/Shift+Enter/Escape
        {
            let next_cb = on_next.clone();
            let prev_cb = on_prev.clone();
            let close_cb = on_close.clone();
            input.handle(move |_, ev| {
                if ev == Event::KeyDown {
                    let key = fltk::app::event_key();
                    let state = fltk::app::event_state();

                    if key == Key::Enter {
                        if state.contains(Shortcut::Shift) {
                            // Shift+Enter: previous match
                            if let Some(cb) = &mut *prev_cb.borrow_mut() {
                                cb();
                            }
                        } else {
                            // Enter: next match
                            if let Some(cb) = &mut *next_cb.borrow_mut() {
                                cb();
                            }
                        }
                        return true;
                    } else if key == Key::Escape {
                        // Escape: close
                        if let Some(cb) = &mut *close_cb.borrow_mut() {
                            cb();
                        }
                        return true;
                    }
                }
                false
            });
        }

        // Wire up prev button
        {
            let prev_cb = on_prev.clone();
            prev_btn.set_callback(move |_| {
                if let Some(cb) = &mut *prev_cb.borrow_mut() {
                    cb();
                }
            });
        }

        // Wire up next button
        {
            let next_cb = on_next.clone();
            next_btn.set_callback(move |_| {
                if let Some(cb) = &mut *next_cb.borrow_mut() {
                    cb();
                }
            });
        }

        // Wire up close button
        {
            let close_cb = on_close.clone();
            close_btn.set_callback(move |_| {
                if let Some(cb) = &mut *close_cb.borrow_mut() {
                    cb();
                }
            });
        }

        SearchBar {
            group,
            input,
            prev_btn,
            next_btn,
            close_btn,
            count_label,
            on_search,
            on_next,
            on_prev,
            on_close,
        }
    }

    /// Show the search bar and focus the input
    /// Selects all existing text so typing replaces it
    /// If there's existing text, triggers the search callback to highlight matches
    pub fn show(&mut self) {
        self.group.show();
        let text = self.input.value();
        let len = text.len() as i32;
        self.input.take_focus().ok();
        if len > 0 {
            // In FLTK, position is cursor, mark is selection anchor
            // Setting position to end first, then mark to 0 selects all
            self.input.set_position(len).ok();
            self.input.set_mark(0).ok();

            // Trigger the search callback with existing text to highlight matches
            if let Some(cb) = &mut *self.on_search.borrow_mut() {
                cb(text);
            }
        }
    }

    /// Hide the search bar and clear the search term
    pub fn hide(&mut self) {
        self.group.hide();
    }

    /// Check if the search bar is visible
    pub fn visible(&self) -> bool {
        self.group.visible()
    }

    /// Update the match count display
    pub fn set_match_count(&mut self, current: Option<usize>, total: usize) {
        if total == 0 {
            self.count_label.set_label("No matches");
        } else if let Some(curr) = current {
            self.count_label
                .set_label(&format!("{}/{}", curr + 1, total));
        } else {
            self.count_label.set_label(&format!("{} matches", total));
        }
    }

    /// Set callback for search text changes
    pub fn on_search(&self, cb: impl FnMut(String) + 'static) {
        *self.on_search.borrow_mut() = Some(Box::new(cb));
    }

    /// Set callback for next match navigation
    pub fn on_next(&self, cb: impl FnMut() + 'static) {
        *self.on_next.borrow_mut() = Some(Box::new(cb));
    }

    /// Set callback for previous match navigation
    pub fn on_prev(&self, cb: impl FnMut() + 'static) {
        *self.on_prev.borrow_mut() = Some(Box::new(cb));
    }

    /// Set callback for close action
    pub fn on_close(&self, cb: impl FnMut() + 'static) {
        *self.on_close.borrow_mut() = Some(Box::new(cb));
    }

    /// Resize the search bar
    pub fn resize(&mut self, x: i32, y: i32, w: i32) {
        self.group.resize(x, y, w, BAR_HEIGHT);

        // Recalculate positions - buttons fixed to right, input takes remaining space
        let padding = 4;
        let right_section_width = 3 * BUTTON_WIDTH + COUNT_WIDTH + 4 * padding;
        // Children live in absolute coordinates, so offset by the group origin.
        let top = y + 4;

        // Input field takes remaining space on the left
        let input_width = (w - right_section_width - padding).max(INPUT_MIN_WIDTH);
        self.input
            .resize(x + padding, top, input_width, BAR_HEIGHT - 8);

        // Position elements from the right edge
        let mut right_x = x + w - padding - BUTTON_WIDTH;

        self.close_btn
            .resize(right_x, top, BUTTON_WIDTH, BAR_HEIGHT - 8);
        right_x -= BUTTON_WIDTH + padding;

        self.next_btn
            .resize(right_x, top, BUTTON_WIDTH, BAR_HEIGHT - 8);
        right_x -= BUTTON_WIDTH + padding;

        self.prev_btn
            .resize(right_x, top, BUTTON_WIDTH, BAR_HEIGHT - 8);
        right_x -= COUNT_WIDTH + padding;

        self.count_label
            .resize(right_x, top, COUNT_WIDTH, BAR_HEIGHT - 8);
    }

    /// Focus the input field and select all text
    pub fn take_focus(&mut self) {
        let mut input_clone = self.input.clone();
        app::awake_callback(move || {
            input_clone.take_focus().ok();
            let len = input_clone.value().len() as i32;
            if len > 0 {
                input_clone.set_position(len).ok();
                input_clone.set_mark(0).ok();
            }
        });
    }
}
