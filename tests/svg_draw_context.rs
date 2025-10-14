// SVG-based DrawContext implementation for testing and visualization
// Generates SVG output from text display rendering with accurate font metrics

use fliki_rs::sourceedit::text_display::DrawContext;
use rusttype::{point, Font, Scale};
use std::collections::HashMap;
use std::fmt::Write;
use std::fs;

/// SVG-based drawing context that generates SVG markup
pub struct SvgDrawContext {
    svg_content: String,
    current_color: u32,
    current_font: u8,
    current_size: u8,
    has_focus: bool,
    is_active: bool,
    clip_stack: Vec<(i32, i32, i32, i32)>,
    fonts: FontSet,
}

impl SvgDrawContext {
    /// Create a new SVG drawing context
    pub fn new(width: i32, height: i32) -> Self {
        let fonts = FontSet::load_default();

        let mut ctx = SvgDrawContext {
            svg_content: String::new(),
            current_color: 0x000000FF,
            current_font: 0,
            current_size: 14,
            has_focus: true,
            is_active: true,
            clip_stack: Vec::new(),
            fonts,
        };

        // Start SVG document
        writeln!(
            &mut ctx.svg_content,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            width, height, width, height
        )
        .unwrap();

        // Inject a style tag that registers the local test fonts for browser
        // preview of snapshots. The snapshot files live in tests/snapshots,
        // and the fonts are in tests/, thus "..".
        writeln!(
            &mut ctx.svg_content,
            r#"<style>
            @font-face {{ font-family: 'SnapshotSans'; font-weight: 500; font-style: normal; src: url('../NotoSans-Medium.ttf') format('truetype'); }}
            @font-face {{ font-family: 'SnapshotSans'; font-weight: 700; font-style: normal; src: url('../NotoSans-Bold.ttf') format('truetype'); }}
            @font-face {{ font-family: 'SnapshotSans'; font-weight: 500; font-style: italic; src: url('../NotoSans-MediumItalic.ttf') format('truetype'); }}
            @font-face {{ font-family: 'SnapshotSans'; font-weight: 700; font-style: italic; src: url('../NotoSans-BoldItalic.ttf') format('truetype'); }}
            </style>"#
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

    /// Get font family name for SVG text; we register these via the style tag.
    fn font_family(&self, _font: u8) -> &str {
        // Use a single logical family name with different weight/style faces provided.
        "SnapshotSans"
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

    fn text_width(&mut self, text: &str, font: u8, size: u8) -> f64 {
        let font_ref = self.fonts.font_for_id(font);
        let scale = Scale::uniform(size as f32);
        // Determine width using laid-out glyph positions including kerning
        let mut max_x: f32 = 0.0;
        for g in font_ref.layout(text, scale, point(0.0, 0.0)) {
            let adv = g.unpositioned().h_metrics().advance_width;
            let x = g.position().x + adv;
            if x > max_x {
                max_x = x;
            }
        }
        // TODO: Don't know where this is coming from, but browser SVG
        // text rendering is larger -- we scale it up to match visually
        // with browser so we can manually verify snapshots.
        max_x as f64 * 1.4
    }

    fn text_height(&self, font: u8, size: u8) -> i32 {
        let font_ref = self.fonts.font_for_id(font);
        let scale = Scale::uniform(size as f32);
        let v = font_ref.v_metrics(scale);
        // Total recommended line height
        (v.ascent - v.descent + v.line_gap).ceil() as i32
    }

    fn text_descent(&self, font: u8, size: u8) -> i32 {
        let font_ref = self.fonts.font_for_id(font);
        let scale = Scale::uniform(size as f32);
        let v = font_ref.v_metrics(scale);
        (-v.descent).ceil() as i32
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

/// Holds loaded fonts and maps font IDs used by the style tables to the correct face.
struct FontSet {
    // Key: (weight, italic)
    faces: HashMap<(u16, bool), Font<'static>>,
    // Fallback face key present in `faces`
    fallback_key: (u16, bool),
}

impl FontSet {
    fn load_default() -> Self {
        // Attempt to load the four NotoSans variants placed under tests/
        // If any load fails, fall back to Medium for all.
        let mut faces: HashMap<(u16, bool), Font<'static>> = HashMap::new();

        let load_font = |path: &str| -> Option<Font<'static>> {
            match fs::read(path) {
                Ok(bytes) => Font::try_from_vec(bytes),
                Err(_) => None,
            }
        };

        // Medium (weight 500, normal)
        let medium = load_font("tests/NotoSans-Medium.ttf");
        let bold = load_font("tests/NotoSans-Bold.ttf");
        let medium_it = load_font("tests/NotoSans-MediumItalic.ttf");
        let bold_it = load_font("tests/NotoSans-BoldItalic.ttf");

        if let Some(f) = medium {
            faces.insert((500, false), f);
        }
        if let Some(f) = bold {
            faces.insert((700, false), f);
        }
        if let Some(f) = medium_it {
            faces.insert((500, true), f);
        }
        if let Some(f) = bold_it {
            faces.insert((700, true), f);
        }

        // Determine fallback key based on what we actually loaded
        let fallback_key = if faces.contains_key(&(500, false)) {
            (500, false)
        } else if faces.contains_key(&(700, false)) {
            (700, false)
        } else if faces.contains_key(&(500, true)) {
            (500, true)
        } else if faces.contains_key(&(700, true)) {
            (700, true)
        } else {
            panic!("No test fonts found under tests/ (NotoSans-*.ttf)");
        };

        FontSet {
            faces,
            fallback_key,
        }
    }

    /// Map style-table font IDs to weight/italic and return a font face.
    fn font_for_id(&self, font_id: u8) -> &Font<'static> {
        let (weight, italic) = match font_id {
            1 => (700, false), // Bold
            2 => (500, true),  // Italic
            3 => (700, true),  // Bold Italic
            5 => (700, false), // Bold (legacy mapping from sourceedit tests)
            // 4 (code) and everything else -> Medium normal
            _ => (500, false),
        };
        if let Some(f) = self.faces.get(&(weight, italic)) {
            f
        } else {
            &self.faces[&self.fallback_key]
        }
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
