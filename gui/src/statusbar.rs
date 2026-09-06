#![allow(dead_code)]

use fltk::{prelude::*, *};
use std::cell::RefCell;
use std::rc::Rc;

/// Side of the square sync indicator at the right end of the bar.
const SYNC_INDICATOR_SIZE: i32 = 14;
/// Gap between the save status text and the sync indicator.
const SYNC_GAP: i32 = 6;
/// Horizontal padding at both ends of the bar.
const PADDING: i32 = 5;
/// Milliseconds for one full turn of the syncing spinner.
const SPINNER_PERIOD_MS: u64 = 1200;

/// What the sync indicator shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncIndicator {
    /// Nothing drawn.
    Idle,
    /// A rotating arc: a sync is in progress.
    Syncing,
    /// A warning badge: the last sync failed (details in the tooltip).
    Error,
}

/// State shared with the indicator's draw callback.
struct SyncVisual {
    mode: SyncIndicator,
    /// Start angle of the spinner arc in degrees.
    angle: i32,
    bg: enums::Color,
    fg: enums::Color,
}

/// Positions of the three children for a bar at `(x, y, w, h)`.
struct Layout {
    /// Note button (left half): x and width; it spans the bar's full height.
    note: (i32, i32),
    /// Save status (right half minus the indicator): x and width.
    save: (i32, i32),
    /// Sync indicator (right end): x, y and side length.
    indicator: (i32, i32, i32),
}

fn layout(x: i32, y: i32, w: i32, h: i32) -> Layout {
    let ind_x = x + w - PADDING - SYNC_INDICATOR_SIZE;
    let ind_y = y + (h - SYNC_INDICATOR_SIZE) / 2;
    let save_x = x + PADDING + w / 2;
    let save_w = (ind_x - SYNC_GAP - save_x).max(10);
    Layout {
        note: (x + PADDING, w / 2 - 2 * PADDING),
        save: (save_x, save_w),
        indicator: (ind_x, ind_y, SYNC_INDICATOR_SIZE),
    }
}

/// Helper function to create a brighter version of a color
/// Increases each RGB component by a factor (clamped to 255)
fn brighten_color(color: enums::Color, factor: f32) -> enums::Color {
    let (r, g, b) = color.to_rgb();
    let new_r = ((r as f32 * factor).min(255.0)) as u8;
    let new_g = ((g as f32 * factor).min(255.0)) as u8;
    let new_b = ((b as f32 * factor).min(255.0)) as u8;
    enums::Color::from_rgb(new_r, new_g, new_b)
}

/// What the left half of the status bar shows: the note we are on, or — while a
/// link is hovered (by the mouse or by the caret sitting inside it) — that
/// link's destination.
///
/// Both halves are kept as data instead of being read back off the widget
/// label, because the two are updated by independent event sources: a link
/// click navigates (new note name) while the hover that started the click is
/// still active. Snapshotting the label at hover start and restoring it at
/// hover end would put the *previous* note's name back on screen.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct NoteLabel {
    /// Name of the note currently on screen (already formatted for display).
    note: String,
    /// Destination of the hovered link, if any; takes precedence while set.
    hover: Option<String>,
}

impl NoteLabel {
    /// Navigation is authoritative: it sets the new note name and drops any
    /// hover, which belongs to the content that was just replaced.
    fn set_note(&mut self, note: &str) {
        self.note = note.to_string();
        self.hover = None;
    }

    fn set_hover(&mut self, target: Option<&str>) {
        self.hover = target.map(str::to_string);
    }

    fn displayed(&self) -> &str {
        self.hover.as_deref().unwrap_or(&self.note)
    }
}

/// Custom status bar widget that manages two child widgets (note status and save status)
/// and automatically handles layout and rendering
pub struct StatusBar {
    // Background frame
    background: frame::Frame,
    // Left side: note status (button for clicking)
    note_status: button::Button,
    // Right side: save status (frame for display)
    save_status: frame::Frame,
    // Far right: the sync spinner / error badge
    sync_indicator: frame::Frame,
    sync_visual: Rc<RefCell<SyncVisual>>,
    // Current note name plus any transient link-hover destination
    note_label: NoteLabel,
    // Colors
    bg_color: enums::Color,
    text_color: enums::Color,
    hover_color: enums::Color,
}

impl StatusBar {
    /// Create a new StatusBar widget
    ///
    /// # Arguments
    /// * `x` - X position
    /// * `y` - Y position
    /// * `w` - Width
    /// * `h` - Height
    pub fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        let bg_color = enums::Color::from_rgb(136, 167, 246); // Default blue
        let text_color = enums::Color::White;
        let hover_color = brighten_color(bg_color, 1.2); // 20% brighter

        // Create background frame
        let mut background = frame::Frame::new(x, y, w, h, None);
        background.set_frame(enums::FrameType::FlatBox);
        background.set_color(bg_color);

        let Layout {
            note: (note_x, note_w),
            save: (save_x, save_w),
            indicator: (ind_x, ind_y, ind_size),
        } = layout(x, y, w, h);

        // Create note status button (left side)
        let mut note_status = button::Button::new(note_x, y, note_w, h, None);
        note_status.set_frame(enums::FrameType::FlatBox);
        note_status.set_align(enums::Align::Left | enums::Align::Inside);
        note_status.set_label_size(app::font_size() - 1);
        note_status.set_color(bg_color);
        note_status.set_label_color(text_color);

        // Add hover effect for note status
        let mut but2 = note_status.clone();
        let hover_bg = hover_color;
        note_status.handle(move |_, evt| match evt {
            enums::Event::Enter => {
                but2.set_color(hover_bg);
                but2.redraw();
                true
            }
            enums::Event::Leave => {
                but2.set_color(bg_color);
                but2.redraw();
                true
            }
            _ => false,
        });

        // Create save status frame (right side)
        let mut save_status = frame::Frame::new(save_x, y, save_w, h, None);
        save_status.set_frame(enums::FrameType::FlatBox);
        save_status.set_align(enums::Align::Right | enums::Align::Inside);
        save_status.set_label_size(app::font_size() - 1);
        save_status.set_color(bg_color);
        save_status.set_label_color(text_color);

        // Sync indicator: custom-drawn from shared state so the animation
        // timer only has to bump an angle and ask for a redraw.
        let sync_visual = Rc::new(RefCell::new(SyncVisual {
            mode: SyncIndicator::Idle,
            angle: 0,
            bg: bg_color,
            fg: text_color,
        }));
        let mut sync_indicator = frame::Frame::new(ind_x, ind_y, ind_size, ind_size, None);
        sync_indicator.set_frame(enums::FrameType::FlatBox);
        sync_indicator.set_color(bg_color);
        {
            let visual = sync_visual.clone();
            sync_indicator.draw(move |f| {
                let v = visual.borrow();
                draw::set_draw_color(v.bg);
                draw::draw_rectf(f.x(), f.y(), f.w(), f.h());
                match v.mode {
                    SyncIndicator::Idle => {}
                    SyncIndicator::Syncing => {
                        // A three-quarter arc that turns: the classic spinner.
                        draw::set_draw_color(v.fg);
                        draw::set_line_style(draw::LineStyle::Solid, 2);
                        draw::draw_arc(
                            f.x() + 2,
                            f.y() + 2,
                            f.w() - 4,
                            f.h() - 4,
                            v.angle as f64,
                            v.angle as f64 + 270.0,
                        );
                        draw::set_line_style(draw::LineStyle::Solid, 0);
                    }
                    SyncIndicator::Error => {
                        // A filled warning disc with an exclamation mark.
                        draw::set_draw_color(enums::Color::from_rgb(250, 204, 21));
                        draw::draw_pie(f.x(), f.y(), f.w(), f.h(), 0.0, 360.0);
                        draw::set_draw_color(enums::Color::from_rgb(60, 40, 0));
                        draw::set_font(enums::Font::HelveticaBold, f.h() - 3);
                        draw::draw_text2("!", f.x(), f.y(), f.w(), f.h(), enums::Align::Center);
                    }
                }
            });
        }

        StatusBar {
            background,
            note_status,
            save_status,
            sync_indicator,
            sync_visual,
            note_label: NoteLabel::default(),
            bg_color,
            text_color,
            hover_color,
        }
    }

    /// Set the background color of the status bar
    /// Also automatically updates the hover color to be a brighter version
    pub fn set_color(&mut self, color: enums::Color) {
        self.bg_color = color;
        self.hover_color = brighten_color(color, 1.2); // 20% brighter
        self.background.set_color(color);
        self.note_status.set_color(color);
        self.save_status.set_color(color);
        self.sync_indicator.set_color(color);
        self.sync_visual.borrow_mut().bg = color;

        // Update the hover handler with the new colors
        let mut but2 = self.note_status.clone();
        let bg = color;
        let hover_bg = self.hover_color;
        self.note_status.handle(move |_, evt| match evt {
            enums::Event::Enter => {
                but2.set_color(hover_bg);
                but2.redraw();
                true
            }
            enums::Event::Leave => {
                but2.set_color(bg);
                but2.redraw();
                true
            }
            _ => false,
        });
    }

    /// Set the text color of the status bar
    pub fn set_text_color(&mut self, color: enums::Color) {
        self.text_color = color;
        self.note_status.set_label_color(color);
        self.save_status.set_label_color(color);
        self.sync_visual.borrow_mut().fg = color;
    }

    /// Switch the sync indicator at the right end of the bar. The tooltip
    /// explains the state (e.g. the last sync error) on hover.
    pub fn set_sync_indicator(&mut self, mode: SyncIndicator, tooltip: &str) {
        self.sync_visual.borrow_mut().mode = mode;
        self.sync_indicator.set_tooltip(tooltip);
        self.sync_indicator.redraw();
    }

    pub fn sync_indicator(&self) -> SyncIndicator {
        self.sync_visual.borrow().mode
    }

    /// Advance the spinner. Driven from the app's animation timer with
    /// milliseconds since start; a no-op unless a sync is showing.
    pub fn tick(&mut self, ms_since_start: u64) {
        let mut v = self.sync_visual.borrow_mut();
        if v.mode != SyncIndicator::Syncing {
            return;
        }
        // Clockwise on screen: FLTK angles grow counter-clockwise.
        let angle = 360 - ((ms_since_start % SPINNER_PERIOD_MS) * 360 / SPINNER_PERIOD_MS) as i32;
        if angle != v.angle {
            v.angle = angle;
            drop(v);
            self.sync_indicator.redraw();
        }
    }

    /// Set the note status text (left side).
    ///
    /// This is the authoritative "which note is on screen" label, so it also
    /// drops any link-hover destination currently being shown: navigation
    /// replaces the content under the mouse, making that hover stale.
    pub fn set_note(&mut self, text: &str) {
        self.note_label.set_note(text);
        self.refresh_note_label();
    }

    /// Show the destination of the hovered link in place of the note name, or
    /// pass `None` when the hover ends to fall back to the note on screen.
    pub fn set_link_hover(&mut self, target: Option<&str>) {
        self.note_label.set_hover(target);
        self.refresh_note_label();
    }

    fn refresh_note_label(&mut self) {
        let label = self.note_label.displayed().to_string();
        self.note_status.set_label(&label);
    }

    /// Set the save status text (right side)
    pub fn set_status(&mut self, text: &str) {
        self.save_status.set_label(text);
    }

    /// Set the tooltip for the note status (left side)
    pub fn set_note_tooltip(&mut self, tooltip: &str) {
        self.note_status.set_tooltip(tooltip);
    }

    /// Set the tooltip for the save status (right side)
    pub fn set_status_tooltip(&mut self, tooltip: &str) {
        self.save_status.set_tooltip(tooltip);
    }

    /// Set the hover color for the note status button
    pub fn set_hover_color(&mut self, color: enums::Color) {
        self.hover_color = color;

        // Update the hover handler with the new hover color
        let mut but2 = self.note_status.clone();
        let bg = self.bg_color;
        let hover_bg = color;
        self.note_status.handle(move |_, evt| match evt {
            enums::Event::Enter => {
                but2.set_color(hover_bg);
                but2.redraw();
                true
            }
            enums::Event::Leave => {
                but2.set_color(bg);
                but2.redraw();
                true
            }
            _ => false,
        });
    }

    /// Register a callback for when the note status is clicked
    pub fn on_note_click<F: FnMut(&mut button::Button) + 'static>(&mut self, cb: F) {
        self.note_status.set_callback(cb);
    }

    /// Register a callback for when the save status is clicked
    /// Note: This converts the frame to a button if needed for click handling
    pub fn on_status_click<F: FnMut() + 'static>(&mut self, mut cb: F) {
        // For now, we handle this via a manual event handler
        // since save_status is a Frame, not a Button
        self.save_status.handle(move |_, evt| {
            if evt == enums::Event::Push {
                cb();
                true
            } else {
                false
            }
        });
    }

    /// Get a reference to the note status widget (for external manipulation)
    pub fn note_status_widget(&self) -> button::Button {
        self.note_status.clone()
    }

    /// Get a reference to the save status widget (for external manipulation)
    pub fn save_status_widget(&self) -> frame::Frame {
        self.save_status.clone()
    }

    /// Resize the status bar and update child positions
    pub fn resize(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let Layout {
            note: (note_x, note_w),
            save: (save_x, save_w),
            indicator: (ind_x, ind_y, ind_size),
        } = layout(x, y, w, h);
        self.background.resize(x, y, w, h);
        self.note_status.resize(note_x, y, note_w, h);
        self.save_status.resize(save_x, y, save_w, h);
        self.sync_indicator.resize(ind_x, ind_y, ind_size, ind_size);
    }

    /// Get the height of the status bar
    pub fn height(&self) -> i32 {
        self.background.height()
    }

    /// Get the width of the status bar
    pub fn width(&self) -> i32 {
        self.background.width()
    }

    /// Get the x position of the status bar
    pub fn x(&self) -> i32 {
        self.background.x()
    }

    /// Get the y position of the status bar
    pub fn y(&self) -> i32 {
        self.background.y()
    }

    /// Hide the status bar
    pub fn hide(&mut self) {
        self.background.hide();
        self.note_status.hide();
        self.save_status.hide();
        self.sync_indicator.hide();
    }

    /// Show the status bar
    pub fn show(&mut self) {
        self.background.show();
        self.note_status.show();
        self.save_status.show();
        self.sync_indicator.show();
    }

    /// Check if the status bar is visible
    pub fn visible(&self) -> bool {
        self.background.visible()
    }
}

#[cfg(test)]
mod tests {
    use super::{NoteLabel, SYNC_GAP, SYNC_INDICATOR_SIZE, layout};

    #[test]
    fn layout_keeps_indicator_at_the_right_end() {
        let l = layout(0, 100, 400, 25);
        let ((note_x, note_w), (save_x, save_w), (ind_x, ind_y, size)) =
            (l.note, l.save, l.indicator);
        assert_eq!(size, SYNC_INDICATOR_SIZE);
        assert_eq!(ind_x + size, 400 - 5, "indicator ends at the right padding");
        assert!(
            ind_y > 100 && ind_y + size < 125,
            "vertically inside the bar"
        );
        assert_eq!(
            save_x + save_w + SYNC_GAP,
            ind_x,
            "save text stops before it"
        );
        assert!(note_x + note_w <= save_x, "halves do not overlap");
    }

    #[test]
    fn hover_overlays_the_note_name_and_falls_back_to_it() {
        let mut label = NoteLabel::default();
        label.set_note("Note: frontpage");
        assert_eq!(label.displayed(), "Note: frontpage");

        label.set_hover(Some("recipes"));
        assert_eq!(label.displayed(), "recipes");

        label.set_hover(None);
        assert_eq!(label.displayed(), "Note: frontpage");
    }

    #[test]
    fn navigating_while_hovering_leaves_no_stale_note_name() {
        let mut label = NoteLabel::default();
        label.set_note("Note: frontpage");

        // Hover the "recipes" link, then click it: the load sets the new note
        // name while the hover that started the click is still in effect.
        label.set_hover(Some("recipes"));
        label.set_note("Note: recipes");
        assert_eq!(label.displayed(), "Note: recipes");

        // The hover-end event arriving afterwards must not resurrect the note
        // we came from.
        label.set_hover(None);
        assert_eq!(label.displayed(), "Note: recipes");
    }

    #[test]
    fn hover_end_after_back_navigation_keeps_the_note_we_went_back_to() {
        let mut label = NoteLabel::default();
        label.set_note("Note: frontpage");
        label.set_hover(Some("recipes"));
        label.set_note("Note: recipes");

        // Back to the frontpage with the caret landing next to one of its
        // links, then away from it again.
        label.set_note("Note: frontpage");
        label.set_hover(Some("notes"));
        assert_eq!(label.displayed(), "notes");
        label.set_hover(None);
        assert_eq!(label.displayed(), "Note: frontpage");
    }
}
