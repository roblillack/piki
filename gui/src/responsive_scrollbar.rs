// Responsive scrollbar with three visibility states: asleep, awake, and hovered
// Based on FLTK's scrollbar with custom drawing

use fltk::{draw as fltk_draw, enums::*, prelude::*, valuator::Scrollbar};
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// Minimum height (in pixels) of the slider thumb.
///
/// FLTK derives the *draggable* thumb from `slider_size()`; for a very long note
/// that fraction is tiny. We floor `slider_size()` (see `min_slider_size`) so the
/// thumb never drops below this, keeping it comfortably grabbable.
pub const MIN_THUMB_HEIGHT: i32 = 20;

/// Compute the thumb rectangle (top `y`, height) for the given scroll state.
///
/// We drive dragging ourselves (see the `handle` closure), so — unlike FLTK's
/// built-in `Fl_Scrollbar` — the thumb spans the *full* height with no space
/// reserved for arrow buttons. Both the drawing and the hit-testing go through
/// this one function, so what you see is exactly what you grab.
///
/// Returns `None` when there is nothing to drag (content fits, or degenerate size).
fn thumb_geometry(
    y: i32,
    h: i32,
    min: f64,
    max: f64,
    val: f64,
    slider_size: f32,
) -> Option<(i32, i32)> {
    if !(max > min) || slider_size <= 0.0 || h <= 0 {
        return None;
    }
    let s = ((slider_size as f64 * h as f64).round() as i32).clamp(MIN_THUMB_HEIGHT, h);
    if s >= h {
        // Content fits within the view; no thumb to draw or drag.
        return None;
    }
    let val_frac = ((val - min) / (max - min)).clamp(0.0, 1.0);
    let track = h - s;
    Some((y + (val_frac * track as f64).round() as i32, s))
}

/// Inverse of `thumb_geometry`'s position mapping: given the desired thumb *top*
/// (window y), return the scroll value that places it there.
fn value_for_thumb_top(
    thumb_top: i32,
    y: i32,
    h: i32,
    thumb_height: i32,
    min: f64,
    max: f64,
) -> f64 {
    let track = h - thumb_height;
    if track <= 0 {
        return min;
    }
    let frac = ((thumb_top - y) as f64 / track as f64).clamp(0.0, 1.0);
    min + frac * (max - min)
}

/// Value delta for one "page" — a full visible screen — matching the PageUp/PageDown
/// keys and FLTK's own trough-click step. Since `slider_size` is `visible/content`,
/// this works out to exactly the visible height in scroll-value units.
fn page_step(min: f64, max: f64, slider_size: f32) -> f64 {
    let ss = slider_size as f64;
    if ss <= 0.0 || ss >= 1.0 {
        return 0.0;
    }
    (max - min) * ss / (1.0 - ss)
}

/// A page-scroll gesture in progress (mouse held down in the track, above or below
/// the thumb). We page toward `target_y` (the click position) and stop once the
/// thumb reaches it, repeating on a timer while the button stays down.
#[derive(Debug, Clone, Copy)]
struct Paging {
    /// `true` = page down (clicked below the thumb), `false` = page up.
    down: bool,
    /// Window-y of the click; paging stops once the thumb covers this point.
    target_y: i32,
}

/// Perform one page step for an in-progress track-click gesture. Returns `true` if
/// paging should continue (thumb hasn't yet reached the cursor and isn't at a bound).
fn apply_page_step(state: &Rc<RefCell<ResponsiveScrollbarState>>, sb: &mut Scrollbar) -> bool {
    let paging = match state.borrow().paging {
        Some(p) => p,
        None => return false,
    };
    let (y, h) = (sb.y(), sb.h());
    let (min, max) = (sb.minimum(), sb.maximum());
    let slider_size = sb.slider_size();
    let val = sb.value();

    let (thumb_top, thumb_h) = match thumb_geometry(y, h, min, max, val, slider_size) {
        Some(g) => g,
        None => return false,
    };

    // Stop once the thumb has moved far enough to cover the click point.
    let reached = if paging.down {
        paging.target_y < thumb_top + thumb_h
    } else {
        paging.target_y >= thumb_top
    };
    if reached {
        return false;
    }

    let page = page_step(min, max, slider_size);
    if page <= 0.0 {
        return false;
    }
    let new_val = if paging.down {
        (val + page).min(max)
    } else {
        (val - page).max(min)
    };
    if new_val == val {
        return false; // already at a bound
    }
    sb.set_value(new_val);
    sb.do_callback();
    sb.redraw();
    true
}

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
    /// While a thumb drag is in progress, the offset (px) from the thumb's top
    /// to the mouse at grab time. `None` when not dragging.
    drag_offset: Option<i32>,
    /// An in-progress track-click page gesture, if any.
    paging: Option<Paging>,
    /// Whether the auto-repeat timeout for paging is currently scheduled, so a
    /// held button doesn't stack up multiple timer chains.
    paging_timer_active: bool,
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
            drag_offset: None,
            paging: None,
            paging_timer_active: false,
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

                        if let Some((slider_y, slider_height)) =
                            thumb_geometry(y, h, min, max, val, slider_size)
                        {
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

                        if let Some((slider_y, slider_height)) =
                            thumb_geometry(y, h, min, max, val, slider_size)
                        {
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
                    Event::Push => {
                        // We drive dragging ourselves (see `super_handle_first(false)` below)
                        // so the thumb uses the full height with no arrow-button gap.
                        let ey = fltk::app::event_y();
                        let (y, h) = (sb.y(), sb.h());
                        let geom = thumb_geometry(
                            y,
                            h,
                            sb.minimum(),
                            sb.maximum(),
                            sb.value(),
                            sb.slider_size(),
                        );

                        let mut start_paging = false;
                        {
                            let mut st = state.borrow_mut();
                            st.state = ScrollbarState::Hovered;
                            st.drag_offset = None;
                            st.paging = None;
                            if let Some((thumb_top, thumb_h)) = geom {
                                if ey >= thumb_top && ey < thumb_top + thumb_h {
                                    // Grabbed the thumb directly.
                                    st.drag_offset = Some(ey - thumb_top);
                                } else {
                                    // Clicked in the track: page toward the click, repeating
                                    // while the button stays down (classic scrollbar paging).
                                    st.paging = Some(Paging {
                                        down: ey >= thumb_top + thumb_h,
                                        target_y: ey,
                                    });
                                    start_paging = true;
                                }
                            }
                        }
                        if start_paging {
                            // Page once now, then auto-repeat after a short initial delay.
                            let more = apply_page_step(&state, &mut sb);
                            let mut st = state.borrow_mut();
                            if more && !st.paging_timer_active {
                                st.paging_timer_active = true;
                                drop(st);
                                let state_pg = state.clone();
                                let mut sb_pg = sb.clone();
                                fltk::app::add_timeout3(0.3, move |handle| {
                                    let cont = apply_page_step(&state_pg, &mut sb_pg);
                                    if cont && state_pg.borrow().paging.is_some() {
                                        fltk::app::repeat_timeout3(0.05, handle);
                                    } else {
                                        state_pg.borrow_mut().paging_timer_active = false;
                                    }
                                });
                            } else if !more {
                                st.paging = None;
                            }
                        }
                        sb.redraw();
                        true
                    }
                    Event::Drag => {
                        let offset = state.borrow().drag_offset;
                        if let Some(offset) = offset {
                            let ey = fltk::app::event_y();
                            let y = sb.y();
                            let h = sb.h();
                            let (min, max) = (sb.minimum(), sb.maximum());
                            if let Some((_, thumb_h)) =
                                thumb_geometry(y, h, min, max, sb.value(), sb.slider_size())
                            {
                                let v = value_for_thumb_top(ey - offset, y, h, thumb_h, min, max);
                                sb.set_value(v);
                                sb.do_callback();
                            }
                            sb.redraw();
                        }
                        state.borrow_mut().state = ScrollbarState::Hovered;
                        true
                    }
                    Event::Released => {
                        // Clearing `paging` also tells the auto-repeat timer to stop.
                        let mut st = state.borrow_mut();
                        st.drag_offset = None;
                        st.paging = None;
                        st.state = ScrollbarState::Hovered;
                        st.last_wake_time = Instant::now();
                        drop(st);
                        sb.redraw();
                        true
                    }
                    _ => false,
                }
            }
        });

        // Run our handler *before* FLTK's built-in scrollbar logic, and let it
        // consume Push/Drag/Release. This bypasses `Fl_Scrollbar`'s arrow-button
        // track reservation entirely, so the thumb spans the full height.
        scrollbar.super_handle_first(false);

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

    /// Smallest slider-size fraction that still yields a thumb at least
    /// `MIN_THUMB_HEIGHT` pixels tall, given the current scrollbar geometry.
    /// Callers should clamp their computed `slider_size` to at least this.
    pub fn min_slider_size(&self) -> f32 {
        let h = self.scrollbar.h();
        if h > 0 {
            (MIN_THUMB_HEIGHT as f32 / h as f32).min(1.0)
        } else {
            1.0
        }
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
