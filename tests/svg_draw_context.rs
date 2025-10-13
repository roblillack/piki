// SVG-based DrawContext implementation for testing and visualization
// Generates SVG output from text display rendering

use fliki_rs::sourceedit::text_display::DrawContext;
use std::fmt::Write;

/// SVG-based drawing context that generates SVG markup
pub struct SvgDrawContext {
    svg_content: String,
    current_color: u32,
    current_font: u8,
    current_size: u8,
    has_focus: bool,
    is_active: bool,
    clip_stack: Vec<(i32, i32, i32, i32)>,
}

impl SvgDrawContext {
    /// Create a new SVG drawing context
    pub fn new(width: i32, height: i32) -> Self {
        let mut ctx = SvgDrawContext {
            svg_content: String::new(),
            current_color: 0x000000FF,
            current_font: 0,
            current_size: 14,
            has_focus: true,
            is_active: true,
            clip_stack: Vec::new(),
        };

        // Start SVG document
        writeln!(
            &mut ctx.svg_content,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            width, height, width, height
        )
        .unwrap();

        // White background
        writeln!(
            &mut ctx.svg_content,
            r##"  <rect width="{}" height="{}" fill="#ffffff"/>"##,
            width, height
        )
        .unwrap();

        ctx
    }

    /// Set focus state
    pub fn set_focus(&mut self, focus: bool) {
        self.has_focus = focus;
    }

    /// Set active state
    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    /// Get the generated SVG content
    pub fn finish(mut self) -> String {
        writeln!(&mut self.svg_content, "</svg>").unwrap();
        self.svg_content
    }

    /// Convert RGBA color to SVG color string
    fn color_to_svg(&self, color: u32) -> String {
        let r = (color >> 24) & 0xFF;
        let g = (color >> 16) & 0xFF;
        let b = (color >> 8) & 0xFF;
        let a = color & 0xFF;

        if a == 0xFF {
            format!("#{:02x}{:02x}{:02x}", r, g, b)
        } else {
            format!("rgba({}, {}, {}, {:.2})", r, g, b, a as f32 / 255.0)
        }
    }

    /// Get font family for a font ID
    fn font_family(&self, font: u8) -> &str {
        match font {
            0 => "Helvetica, Arial, sans-serif",
            1 => "Helvetica, Arial, sans-serif", // Helvetica Bold
            2 => "Helvetica, Arial, sans-serif", // Helvetica Italic
            3 => "Helvetica, Arial, sans-serif", // Helvetica Bold Italic
            4 => "Courier, 'Courier New', monospace",
            5 => "Courier, 'Courier New', monospace", // Courier Bold
            _ => "monospace",
        }
    }

    /// Get font weight for a font ID
    fn font_weight(&self, font: u8) -> &str {
        match font {
            1 | 3 | 5 => "bold",
            _ => "normal",
        }
    }

    /// Get font style for a font ID
    fn font_style(&self, font: u8) -> &str {
        match font {
            2 | 3 => "italic",
            _ => "normal",
        }
    }

    /// Escape XML text
    fn escape_xml(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }
}

impl DrawContext for SvgDrawContext {
    fn set_color(&mut self, color: u32) {
        self.current_color = color;
    }

    fn set_font(&mut self, font: u8, size: u8) {
        self.current_font = font;
        self.current_size = size;
    }

    fn draw_text(&mut self, text: &str, x: i32, y: i32) {
        if text.is_empty() {
            return;
        }

        let color = self.color_to_svg(self.current_color);
        let family = self.font_family(self.current_font).to_string();
        let weight = self.font_weight(self.current_font).to_string();
        let style = self.font_style(self.current_font).to_string();
        let size = self.current_size;
        let escaped_text = Self::escape_xml(text);

        // Apply clipping if active
        let clip_attr = if let Some(&(cx, cy, cw, ch)) = self.clip_stack.last() {
            format!(r#" clip-path="url(#clip-{}-{}-{}-{})""#, cx, cy, cw, ch)
        } else {
            String::new()
        };

        writeln!(
            &mut self.svg_content,
            r#"  <text x="{}" y="{}" fill="{}" font-family="{}" font-size="{}" font-weight="{}" font-style="{}"{}>{}</text>"#,
            x, y, color, family, size, weight, style, clip_attr, escaped_text
        )
        .unwrap();
    }

    fn draw_rect_filled(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let color = self.color_to_svg(self.current_color);

        // Apply clipping if active
        let clip_attr = if let Some(&(cx, cy, cw, ch)) = self.clip_stack.last() {
            format!(r#" clip-path="url(#clip-{}-{}-{}-{})""#, cx, cy, cw, ch)
        } else {
            String::new()
        };

        writeln!(
            &mut self.svg_content,
            r#"  <rect x="{}" y="{}" width="{}" height="{}" fill="{}"{}/>"#,
            x, y, w, h, color, clip_attr
        )
        .unwrap();
    }

    fn draw_line(&mut self, x1: i32, y1: i32, x2: i32, y2: i32) {
        let color = self.color_to_svg(self.current_color);

        writeln!(
            &mut self.svg_content,
            r#"  <line x1="{}" y1="{}" x2="{}" y2="{}" stroke="{}" stroke-width="1"/>"#,
            x1, y1, x2, y2, color
        )
        .unwrap();
    }

    fn text_width(&mut self, text: &str, _font: u8, size: u8) -> f64 {
        // Approximate monospace character width
        // For monospace fonts: width ≈ 0.6 * font_size per character
        let char_count = text.chars().count();
        (char_count as f64) * (size as f64 * 0.6)
    }

    fn text_height(&self, _font: u8, size: u8) -> i32 {
        // Height includes ascent, descent, and leading
        // Approximately 1.2x the point size
        ((size as f64) * 1.2) as i32
    }

    fn text_descent(&self, _font: u8, size: u8) -> i32 {
        // Descent is approximately 0.2x the point size
        ((size as f64) * 0.2) as i32
    }

    fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        // Create a clip path definition
        let clip_id = format!("clip-{}-{}-{}-{}", x, y, w, h);

        // Only add the clip definition if it doesn't exist yet
        if !self.svg_content.contains(&clip_id) {
            writeln!(
                &mut self.svg_content,
                r#"  <defs>
    <clipPath id="{}">
      <rect x="{}" y="{}" width="{}" height="{}"/>
    </clipPath>
  </defs>"#,
                clip_id, x, y, w, h
            )
            .unwrap();
        }

        self.clip_stack.push((x, y, w, h));
    }

    fn pop_clip(&mut self) {
        self.clip_stack.pop();
    }

    fn color_average(&self, c1: u32, c2: u32, weight: f32) -> u32 {
        let r1 = ((c1 >> 24) & 0xFF) as f32;
        let g1 = ((c1 >> 16) & 0xFF) as f32;
        let b1 = ((c1 >> 8) & 0xFF) as f32;
        let a1 = (c1 & 0xFF) as f32;

        let r2 = ((c2 >> 24) & 0xFF) as f32;
        let g2 = ((c2 >> 16) & 0xFF) as f32;
        let b2 = ((c2 >> 8) & 0xFF) as f32;
        let a2 = (c2 & 0xFF) as f32;

        let r = (r1 * (1.0 - weight) + r2 * weight) as u32;
        let g = (g1 * (1.0 - weight) + g2 * weight) as u32;
        let b = (b1 * (1.0 - weight) + b2 * weight) as u32;
        let a = (a1 * (1.0 - weight) + a2 * weight) as u32;

        (r << 24) | (g << 16) | (b << 8) | a
    }

    fn color_contrast(&self, fg: u32, bg: u32) -> u32 {
        // Calculate relative luminance
        let bg_r = ((bg >> 24) & 0xFF) as f32 / 255.0;
        let bg_g = ((bg >> 16) & 0xFF) as f32 / 255.0;
        let bg_b = ((bg >> 8) & 0xFF) as f32 / 255.0;

        let bg_lum = 0.299 * bg_r + 0.587 * bg_g + 0.114 * bg_b;

        // If background is dark, use white; if light, use black or the original fg
        if bg_lum < 0.5 {
            0xFFFFFFFF // White
        } else {
            // Check if fg has enough contrast
            let fg_r = ((fg >> 24) & 0xFF) as f32 / 255.0;
            let fg_g = ((fg >> 16) & 0xFF) as f32 / 255.0;
            let fg_b = ((fg >> 8) & 0xFF) as f32 / 255.0;
            let fg_lum = 0.299 * fg_r + 0.587 * fg_g + 0.114 * fg_b;

            if (bg_lum - fg_lum).abs() < 0.3 {
                0x000000FF // Black
            } else {
                fg
            }
        }
    }

    fn color_inactive(&self, c: u32) -> u32 {
        // Desaturate and lighten the color for inactive state
        let r = ((c >> 24) & 0xFF) as f32;
        let g = ((c >> 16) & 0xFF) as f32;
        let b = ((c >> 8) & 0xFF) as f32;
        let a = (c & 0xFF) as f32;

        // Convert to grayscale (average method)
        let gray = (r + g + b) / 3.0;

        // Mix with gray (50% desaturation)
        let r_new = ((r + gray) / 2.0) as u32;
        let g_new = ((g + gray) / 2.0) as u32;
        let b_new = ((b + gray) / 2.0) as u32;

        (r_new << 24) | (g_new << 16) | (b_new << 8) | (a as u32)
    }

    fn has_focus(&self) -> bool {
        self.has_focus
    }

    fn is_active(&self) -> bool {
        self.is_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_svg_context_creation() {
        let ctx = SvgDrawContext::new(400, 300);
        let svg = ctx.finish();
        assert!(svg.contains(r#"width="400""#));
        assert!(svg.contains(r#"height="300""#));
    }

    #[test]
    fn test_color_conversion() {
        let ctx = SvgDrawContext::new(100, 100);
        assert_eq!(ctx.color_to_svg(0xFF0000FF), String::from("#ff0000"));
        assert_eq!(ctx.color_to_svg(0x00FF00FF), String::from("#00ff00"));
        assert_eq!(ctx.color_to_svg(0x0000FFFF), String::from("#0000ff"));
    }

    #[test]
    fn test_draw_text() {
        let mut ctx = SvgDrawContext::new(200, 100);
        ctx.set_color(0x000000FF);
        ctx.set_font(4, 14);
        ctx.draw_text("Hello World", 10, 20);
        let svg = ctx.finish();
        assert!(svg.contains("Hello World"));
        assert!(svg.contains(r#"x="10""#));
        assert!(svg.contains(r#"y="20""#));
    }

    #[test]
    fn test_draw_rect() {
        let mut ctx = SvgDrawContext::new(200, 100);
        ctx.set_color(0xFF0000FF);
        ctx.draw_rect_filled(10, 20, 50, 30);
        let svg = ctx.finish();
        assert!(svg.contains(r#"x="10""#));
        assert!(svg.contains(r#"width="50""#));
        assert!(svg.contains(r#"height="30""#));
    }

    #[test]
    fn test_xml_escaping() {
        let mut ctx = SvgDrawContext::new(200, 100);
        ctx.draw_text("<test> & \"quote\"", 0, 10);
        let svg = ctx.finish();
        assert!(svg.contains("&lt;test&gt; &amp; &quot;quote&quot;"));
    }
}
