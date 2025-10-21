use fltk::{prelude::*, *};

/// Helper function to create a brighter version of a color
/// Increases each RGB component by a factor (clamped to 255)
fn brighten_color(color: enums::Color, factor: f32) -> enums::Color {
    let (r, g, b) = color.to_rgb();
    let new_r = ((r as f32 * factor).min(255.0)) as u8;
    let new_g = ((g as f32 * factor).min(255.0)) as u8;
    let new_b = ((b as f32 * factor).min(255.0)) as u8;
    enums::Color::from_rgb(new_r, new_g, new_b)
}

/// Custom status bar widget that manages two child widgets (page status and save status)
/// and automatically handles layout and rendering
pub struct StatusBar {
    // Background frame
    background: frame::Frame,
    // Left side: page status (button for clicking)
    page_status: button::Button,
    // Right side: save status (frame for display)
    save_status: frame::Frame,
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

        // Create page status button (left side)
        let mut page_status = button::Button::new(x + 5, y, w / 2 - 10, h, None);
        page_status.set_frame(enums::FrameType::FlatBox);
        page_status.set_align(enums::Align::Left | enums::Align::Inside);
        page_status.set_label_size(app::font_size() - 1);
        page_status.set_color(bg_color);
        page_status.set_label_color(text_color);

        // Add hover effect for page status
        let mut but2 = page_status.clone();
        let hover_bg = hover_color;
        page_status.handle(move |_, evt| match evt {
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
        let mut save_status = frame::Frame::new(x + 5 + w / 2, y, w / 2 - 10, h, None);
        save_status.set_frame(enums::FrameType::FlatBox);
        save_status.set_align(enums::Align::Right | enums::Align::Inside);
        save_status.set_label_size(app::font_size() - 1);
        save_status.set_color(bg_color);
        save_status.set_label_color(text_color);

        StatusBar {
            background,
            page_status,
            save_status,
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
        self.page_status.set_color(color);
        self.save_status.set_color(color);

        // Update the hover handler with the new colors
        let mut but2 = self.page_status.clone();
        let bg = color;
        let hover_bg = self.hover_color;
        self.page_status.handle(move |_, evt| match evt {
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
        self.page_status.set_label_color(color);
        self.save_status.set_label_color(color);
    }

    /// Set the page status text (left side)
    pub fn set_page(&mut self, text: &str) {
        self.page_status.set_label(text);
    }

    /// Set the save status text (right side)
    pub fn set_status(&mut self, text: &str) {
        self.save_status.set_label(text);
    }

    /// Set the tooltip for the page status (left side)
    pub fn set_page_tooltip(&mut self, tooltip: &str) {
        self.page_status.set_tooltip(tooltip);
    }

    /// Set the tooltip for the save status (right side)
    pub fn set_status_tooltip(&mut self, tooltip: &str) {
        self.save_status.set_tooltip(tooltip);
    }

    /// Set the hover color for the page status button
    pub fn set_hover_color(&mut self, color: enums::Color) {
        self.hover_color = color;

        // Update the hover handler with the new hover color
        let mut but2 = self.page_status.clone();
        let bg = self.bg_color;
        let hover_bg = color;
        self.page_status.handle(move |_, evt| match evt {
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

    /// Register a callback for when the page status is clicked
    pub fn on_page_click<F: FnMut(&mut button::Button) + 'static>(&mut self, cb: F) {
        self.page_status.set_callback(cb);
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

    /// Get a reference to the page status widget (for external manipulation)
    pub fn page_status_widget(&self) -> button::Button {
        self.page_status.clone()
    }

    /// Get a reference to the save status widget (for external manipulation)
    pub fn save_status_widget(&self) -> frame::Frame {
        self.save_status.clone()
    }

    /// Resize the status bar and update child positions
    pub fn resize(&mut self, x: i32, y: i32, w: i32, h: i32) {
        self.background.resize(x, y, w, h);
        self.page_status.resize(x + 5, y, w / 2 - 10, h);
        self.save_status.resize(x + 5 + w / 2, y, w / 2 - 10, h);
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
}
