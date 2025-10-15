// Drawing backend trait - abstracts over FLTK's drawing primitives
pub trait DrawContext {
    fn set_color(&mut self, color: u32);
    fn set_font(&mut self, font: u8, size: u8);
    fn draw_text(&mut self, text: &str, x: i32, y: i32);
    fn draw_rect_filled(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32);
    fn text_width(&mut self, text: &str, font: u8, size: u8) -> f64;
    fn text_height(&self, font: u8, size: u8) -> i32;
    fn text_descent(&self, font: u8, size: u8) -> i32;
    fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32);
    fn pop_clip(&mut self);
    fn color_average(&self, c1: u32, c2: u32, weight: f32) -> u32;
    fn color_contrast(&self, fg: u32, bg: u32) -> u32;
    fn color_inactive(&self, c: u32) -> u32;
    fn has_focus(&self) -> bool;
    fn is_active(&self) -> bool;
}
