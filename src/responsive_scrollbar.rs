// Responsive scrollbar with three visibility states: asleep, awake, and hovered
// Based on FLTK's scrollbar with custom drawing

use fltk::{draw as fltk_draw, enums::*, prelude::*, valuator::Scrollbar};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Visibility state of the responsive scrollbar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarState {
    /// Only background is rendered
    Asleep,
    /// Background + light gray rectangle where slider would be
    Awake,
    /// Full scrollbar is rendered (on hover)
    Hovered,
}

/// Shared state for the responsive scrollbar
#[derive(Debug, Clone)]
struct ResponsiveScrollbarState {
    state: ScrollbarState,
    last_wake_time: Instant,
    background_color: Color,
}

/// Responsive scrollbar wrapper
#[derive(Clone)]
pub struct ResponsiveScrollbar {
    scrollbar: Scrollbar,
    state: Rc<RefCell<ResponsiveScrollbarState>>,
}

impl ResponsiveScrollbar {
    /// Create a new responsive scrollbar
    pub fn new(x: i32, y: i32, w: i32, h: i32, background_color: Color) -> Self {
        let mut scrollbar = Scrollbar::default().with_pos(x, y).with_size(w, h);

        let state = Rc::new(RefCell::new(ResponsiveScrollbarState {
            state: ScrollbarState::Asleep,
            last_wake_time: Instant::now() - Duration::from_secs(10),
            background_color,
        }));

        // Set up custom draw callback
        scrollbar.draw({
            let state = state.clone();
            let sb = scrollbar.clone();
            move |_| {
                let st = state.borrow();

                // Only check for auto-sleep transition here
                // Don't check mouse position in draw callback - use handle callback for that

                let x = sb.x();
                let y = sb.y();
                let w = sb.w();
                let h = sb.h();

                let rect_col = Color::from_rgb(204, 204, 204); // Light gray for slider rectangle

                match st.state {
                    ScrollbarState::Asleep | ScrollbarState::Awake => {
                        // Draw background
                        fltk_draw::set_draw_color(st.background_color);
                        fltk_draw::draw_rectf(x, y, w, h);

                        // Draw light gray rectangle where slider would be
                        // Calculate slider position and size based on scrollbar values
                        let min = sb.minimum();
                        let max = sb.maximum();
                        let val = sb.value();
                        let slider_size = sb.slider_size();
                        let sbw = if st.state == ScrollbarState::Awake {
                            w - 4
                        } else {
                            3
                        };
                        let offset = if st.state == ScrollbarState::Awake {
                            3
                        } else {
                            w - sbw - 2
                        };

                        if max > min && slider_size > 0.0 {
                            let range = max - min;
                            let slider_frac = slider_size as f64;

                            // Calculate slider dimensions
                            let slider_height = (h as f64 * slider_frac).max(10.0) as i32;
                            let track_height = h - slider_height;

                            // Calculate slider position
                            let pos_frac = if range > 0.0 {
                                ((val - min) / range).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let slider_y = y + (track_height as f64 * pos_frac) as i32;

                            // Draw light gray slider rectangle
                            fltk_draw::set_draw_color(rect_col);
                            fltk_draw::draw_rounded_rectf(
                                x + offset,
                                slider_y + 1,
                                sbw,
                                slider_height - 2,
                                1,
                            );
                        }
                    }
                    ScrollbarState::Hovered => {
                        // Draw a proper scrollbar
                        // Draw scrollbar background
                        // fltk_draw::draw_box(
                        //     FrameType::FlatBox,
                        //     x,
                        //     y,
                        //     w,
                        //     h,
                        //     Color::from_rgb(240, 240, 240),
                        // );
                        fltk_draw::set_draw_color(st.background_color);
                        fltk_draw::draw_rectf(x, y, w, h);

                        // Calculate slider position and size
                        let min = sb.minimum();
                        let max = sb.maximum();
                        let val = sb.value();
                        let slider_size = sb.slider_size();

                        if max > min && slider_size > 0.0 {
                            let range = max - min;
                            let slider_frac = slider_size as f64;

                            // Calculate slider dimensions
                            let slider_height = (h as f64 * slider_frac).max(20.0) as i32;
                            let track_height = h - slider_height;

                            // Calculate slider position
                            let pos_frac = if range > 0.0 {
                                ((val - min) / range).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            let slider_y = y + (track_height as f64 * pos_frac) as i32;

                            // Draw slider with proper 3D look
                            fltk_draw::draw_box(
                                FrameType::ThinUpBox,
                                x + 1,
                                slider_y + 1,
                                w - 2,
                                slider_height - 2,
                                rect_col,
                            );
                        }
                    }
                }
            }
        });

        // Set up handle callback for proper hover detection
        scrollbar.handle({
            let state = state.clone();
            let mut sb = scrollbar.clone();
            move |_, event| {
                match event {
                    Event::Enter => {
                        // Mouse entered scrollbar area
                        state.borrow_mut().state = ScrollbarState::Hovered;
                        sb.redraw();
                        true // We handled this event
                    }
                    Event::Leave => {
                        // Mouse left scrollbar area
                        let mut st = state.borrow_mut();
                        st.state = ScrollbarState::Awake;
                        st.last_wake_time = Instant::now();
                        sb.redraw();
                        true
                    }
                    Event::Move => {
                        // Keep updating hovered state while mouse is moving over it
                        let mut st = state.borrow_mut();
                        if st.state != ScrollbarState::Hovered {
                            st.state = ScrollbarState::Hovered;
                            sb.redraw();
                        }
                        true
                    }
                    Event::Push | Event::Drag | Event::Released => {
                        // Ensure we're hovered during interaction
                        let mut st = state.borrow_mut();
                        st.state = ScrollbarState::Hovered;
                        sb.redraw();
                        true
                    }
                    _ => false,
                }
            }
        });

        // Set up periodic timer to check for auto-sleep (every 500ms)
        {
            let state_timer = state.clone();
            let mut sb_timer = scrollbar.clone();
            fltk::app::add_timeout3(0.1, move |handle| {
                // Check if we need to transition to asleep
                let needs_redraw = {
                    let mut st = state_timer.borrow_mut();
                    if st.state == ScrollbarState::Awake {
                        let elapsed = Instant::now().duration_since(st.last_wake_time);
                        if elapsed > Duration::from_secs(1) {
                            st.state = ScrollbarState::Asleep;
                            true
                        } else {
                            false
                        }
                    } else {
                        false
                    }
                };

                if needs_redraw {
                    sb_timer.redraw();
                }

                fltk::app::repeat_timeout3(0.1, handle);
            });
        }

        Self { scrollbar, state }
    }

    /// Wake the scrollbar (transition to awake state)
    pub fn wake(&mut self) {
        let mut st = self.state.borrow_mut();
        let old_state = st.state;
        if st.state != ScrollbarState::Hovered {
            st.state = ScrollbarState::Awake;
        }
        st.last_wake_time = Instant::now();
        if old_state != st.state {
            self.scrollbar.redraw();
        }
    }

    /// Set the scrollbar type (vertical or horizontal)
    pub fn set_type(&mut self, typ: fltk::valuator::ScrollbarType) {
        // self.wake();
        self.scrollbar.set_type(typ);
    }

    /// Set the bounds of the scrollbar
    pub fn set_bounds(&mut self, min: f64, max: f64) {
        // self.wake();
        self.scrollbar.set_bounds(min, max);
    }

    /// Set the slider size (as a fraction)
    pub fn set_slider_size(&mut self, size: f32) {
        // self.wake();
        self.scrollbar.set_slider_size(size);
    }

    /// Set the step sizes
    pub fn set_step(&mut self, a: f64, b: i32) {
        // self.wake();
        self.scrollbar.set_step(a, b);
    }

    /// Set the value
    pub fn set_value(&mut self, val: f64) {
        // self.wake();
        self.scrollbar.set_value(val);
    }

    /// Get the value
    pub fn value(&self) -> f64 {
        self.scrollbar.value()
    }

    /// Get the minimum value
    pub fn minimum(&self) -> f64 {
        self.scrollbar.minimum()
    }

    /// Get the maximum value
    pub fn maximum(&self) -> f64 {
        self.scrollbar.maximum()
    }

    /// Get the slider size
    pub fn slider_size(&self) -> f32 {
        self.scrollbar.slider_size()
    }

    /// Set a callback for when the scrollbar value changes
    pub fn set_callback<F: FnMut(&mut Scrollbar) + 'static>(&mut self, cb: F) {
        self.scrollbar.set_callback(cb);
    }

    /// Resize the scrollbar
    pub fn resize(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.scrollbar.resize(x, y, w, h);
    }

    /// Show the scrollbar
    pub fn show(&mut self) {
        self.scrollbar.show();
    }

    /// Redraw the scrollbar
    pub fn redraw(&mut self) {
        self.scrollbar.redraw();
    }

    /// Get the underlying scrollbar widget (for adding to parent)
    pub fn as_base_widget(&self) -> fltk::widget::Widget {
        self.scrollbar.as_base_widget()
    }
}
