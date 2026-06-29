#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontType {
    Heading,
    Content,
    Code,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

// Drawing backend trait - abstracts over FLTK's drawing primitives
pub trait DrawContext {
    fn set_color(&mut self, color: u32);
    fn set_font(&mut self, font: FontType, style: FontStyle, size: u8);
    fn draw_text(&mut self, text: &str, x: i32, y: i32);
    fn draw_rect_filled(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32);
    fn text_width(&mut self, text: &str, font: FontType, style: FontStyle, size: u8) -> f64;
    fn text_height(&self, font: FontType, style: FontStyle, size: u8) -> i32;
    fn text_descent(&self, font: FontType, style: FontStyle, size: u8) -> i32;
    fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn pop_clip(&mut self);
    fn color_average(&self, c1: u32, c2: u32, weight: f32) -> u32;
    fn color_contrast(&self, fg: u32, bg: u32) -> u32;
    fn color_inactive(&self, c: u32) -> u32;
    fn has_focus(&self) -> bool;
    fn is_active(&self) -> bool;

    /// Pen state for text decorations, consulted by the next [`draw_text`]. The
    /// default ignores them — pixel backends draw underline/strikethrough as
    /// separate lines. A cell backend overrides these to fold the decoration
    /// into the glyph's attributes (see [`crate::theme::Theme::text_decoration_lines`]).
    fn set_underline(&mut self, _on: bool) {}
    fn set_strikethrough(&mut self, _on: bool) {}

    /// Draw a checklist checkbox of `size` at (x, y) in the active color.
    ///
    /// The default draws a square outline (and a check mark when `checked`) from
    /// line primitives — right for a pixel canvas. A character-cell backend can
    /// override this to stamp a single glyph instead, since a multi-line square
    /// collapses badly into one cell.
    fn draw_checkbox(&mut self, x: i32, y: i32, size: i32, checked: bool) {
        let box_right = x + size;
        let box_bottom = y + size;
        self.draw_line(x, y, box_right, y);
        self.draw_line(x, y, x, box_bottom);
        self.draw_line(x, box_bottom, box_right, box_bottom);
        self.draw_line(box_right, y, box_right, box_bottom);
        if checked {
            let mut inset = ((size as f32) * 0.2).round() as i32;
            if inset < 2 {
                inset = 2;
            }
            if inset * 2 >= size {
                inset = size / 2;
            }
            let x1 = x + inset;
            let y1 = y + inset;
            let x2 = box_right - inset;
            let y2 = box_bottom - inset;
            self.draw_line(x1, y1, x2, y2);
            self.draw_line(x1, y2, x2, y1);
        }
    }
}
