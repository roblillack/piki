//! Shared SVG `DrawContext` test backend for layout snapshot tests.
//!
//! The rendering-agnostic layout engine ([`StructuredRichDisplay`]) is driven
//! through the abstract [`DrawContext`] trait, so a snapshot test only needs an
//! implementation of that trait that records what was drawn. This one renders
//! to deterministic SVG (no rasterization) under two font modes:
//!
//! * [`FontMode::Proportional`] — NotoSans faces with realistic kerned advances
//!   (measured via `rusttype`). Mirrors Piki's FLTK (proportional, pixel)
//!   backend.
//! * [`FontMode::Monospace`] — a synthetic integer character-cell grid. A
//!   monospace grid is fully defined by a cell width and row height, so this
//!   needs no font at all: metrics come from constants and the SVG references
//!   the generic `monospace` family. Mirrors Pure's ratatui (terminal cell)
//!   backend, where every advance is a whole cell.
//!
//! Both modes feed the *same* engine, so the two snapshot sets exercise the
//! shared wrap / cursor / selection / table math under the two metric regimes
//! the real backends impose. The proportional SVG embeds `@font-face` rules
//! pointing at the bundled `tests/*.ttf` (relative to `tests/snapshots/`) so the
//! `.snap.svg` files preview in a browser; the monospace SVG previews with
//! whatever monospace font the viewer has, pinning each run to its exact grid
//! width via SVG `textLength` (like Pure's harness) so glyphs and selection
//! rects stay aligned regardless of that font's natural advance.
//!
//! NOTE: these tests verify the *layout engine* given plausible, deterministic
//! metrics — they do NOT assert that FLTK or ratatui produce identical metrics
//! at runtime. Their value is catching regressions in the shared layout logic.

#![allow(dead_code)]

use std::collections::HashMap;
use std::fmt::Write;
use std::fs;

use rusttype::{Font, Scale, point};
use unicode_segmentation::UnicodeSegmentation;

use tdoc_editor::draw_context::{DrawContext, FontStyle, FontType};
use tdoc_editor::richtext::markdown_converter::markdown_to_document;
use tdoc_editor::richtext::structured_rich_display::StructuredRichDisplay;
use tdoc_editor::richtext::tree_path::DocumentPosition;

/// Browser SVG text renders larger than rusttype's raw advances; the original
/// Piki backend scaled proportional widths by this factor so manual snapshot
/// review lines up visually with the embedded fonts.
const BROWSER_SCALE: f64 = 1.4;

/// Which font family / metric regime a context renders with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontMode {
    /// Proportional NotoSans — kerned, variable-width advances (Piki/FLTK-like).
    Proportional,
    /// Synthetic monospace cell grid, generic `monospace` family (Pure/ratatui-like).
    Monospace,
}

// Synthetic monospace cell metrics. A terminal cell grid is fully defined by a
// cell width and row height, so the monospace backend derives everything from
// the font size — no font file required. The vertical ratios mirror Pure's SVG
// harness, whose cell is 20px tall with a 15px baseline (and thus a 5px descent)
// for a 16px font: row height ≈ 1.25×, descent ≈ 0.3125× the size, putting the
// baseline ~0.75 down each row.
fn mono_cell_width(size: u8) -> f64 {
    (size as f64 * 0.6).round()
}
fn mono_line_height(size: u8) -> i32 {
    (size as f64 * 1.25).round() as i32
}
fn mono_descent(size: u8) -> i32 {
    (size as f64 * 0.3125).round() as i32
}

/// SVG-based drawing context that records the engine's draw calls as SVG markup.
pub struct SvgDrawContext {
    svg_content: String,
    current_color: u32,
    current_font: FontType,
    current_style: FontStyle,
    current_size: u8,
    has_focus: bool,
    is_active: bool,
    clip_stack: Vec<(i32, i32, i32, i32)>,
    mode: FontMode,
    /// Proportional faces, measured via `rusttype`. `None` in monospace mode,
    /// which uses synthetic cell metrics and the generic `monospace` family.
    fonts: Option<FontSet>,
}

impl SvgDrawContext {
    /// Create a new SVG drawing context for the given font mode and canvas size.
    pub fn new(mode: FontMode, width: i32, height: i32) -> Self {
        let fonts = match mode {
            FontMode::Proportional => Some(FontSet::load_proportional()),
            FontMode::Monospace => None,
        };

        let mut ctx = SvgDrawContext {
            svg_content: String::new(),
            current_color: 0x000000FF,
            current_font: FontType::Content,
            current_style: FontStyle::Regular,
            current_size: 14,
            has_focus: true,
            is_active: true,
            clip_stack: Vec::new(),
            mode,
            fonts,
        };

        writeln!(
            &mut ctx.svg_content,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">"#,
            width, height, width, height
        )
        .unwrap();

        // Register the bundled proportional faces so the snapshot previews in a
        // browser (monospace previews with the viewer's generic monospace font).
        // Snapshots live in tests/snapshots/, the fonts in tests/, hence "..".
        let css = ctx.font_face_css();
        ctx.svg_content.push_str(css);

        writeln!(
            &mut ctx.svg_content,
            r##"  <rect width="{}" height="{}" fill="#ffffff"/>"##,
            width, height
        )
        .unwrap();

        ctx
    }

    pub fn set_focus(&mut self, focus: bool) {
        self.has_focus = focus;
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }

    /// Finish the SVG document and return its source.
    pub fn finish(mut self) -> String {
        writeln!(&mut self.svg_content, "</svg>").unwrap();
        self.svg_content
    }

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

    fn font_weight(&self, style: FontStyle) -> &str {
        match style {
            FontStyle::Bold | FontStyle::BoldItalic => "bold",
            _ => "normal",
        }
    }

    fn font_style(&self, style: FontStyle) -> &str {
        match style {
            FontStyle::Italic | FontStyle::BoldItalic => "italic",
            _ => "normal",
        }
    }

    fn escape_xml(text: &str) -> String {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&apos;")
    }

    /// Logical family name emitted in the SVG `font-family` attribute.
    fn family(&self) -> &'static str {
        match self.mode {
            FontMode::Proportional => "SnapshotSans",
            // A real monospace stack for nicer preview; alignment comes from
            // `textLength`, so the exact font here is only cosmetic.
            FontMode::Monospace => "'DejaVu Sans Mono', Menlo, Consolas, monospace",
        }
    }

    /// `<style>` block registering the bundled faces for browser preview. The
    /// monospace backend renders with the viewer's generic monospace font, so it
    /// emits nothing.
    fn font_face_css(&self) -> &'static str {
        match self.mode {
            FontMode::Proportional => {
                r#"<style>
            @font-face { font-family: 'SnapshotSans'; font-weight: 500; font-style: normal; src: url('../NotoSans-Medium.ttf') format('truetype'); }
            @font-face { font-family: 'SnapshotSans'; font-weight: 700; font-style: normal; src: url('../NotoSans-Bold.ttf') format('truetype'); }
            @font-face { font-family: 'SnapshotSans'; font-weight: 500; font-style: italic; src: url('../NotoSans-MediumItalic.ttf') format('truetype'); }
            @font-face { font-family: 'SnapshotSans'; font-weight: 700; font-style: italic; src: url('../NotoSans-BoldItalic.ttf') format('truetype'); }
            </style>
"#
            }
            FontMode::Monospace => "",
        }
    }
}

impl DrawContext for SvgDrawContext {
    fn set_color(&mut self, color: u32) {
        self.current_color = color;
    }

    fn set_font(&mut self, font: FontType, style: FontStyle, size: u8) {
        self.current_font = font;
        self.current_style = style;
        self.current_size = size;
    }

    fn draw_text(&mut self, text: &str, x: i32, y: i32) {
        if text.is_empty() {
            return;
        }

        let color = self.color_to_svg(self.current_color);
        let family = self.family().to_string();
        let weight = self.font_weight(self.current_style).to_string();
        let style = self.font_style(self.current_style).to_string();
        let size = self.current_size;
        let escaped_text = Self::escape_xml(text);

        let clip_attr = if let Some(&(cx, cy, cw, ch)) = self.clip_stack.last() {
            format!(r#" clip-path="url(#clip-{}-{}-{}-{})""#, cx, cy, cw, ch)
        } else {
            String::new()
        };

        // Monospace: pin the run to the exact cell-grid width the engine used in
        // `text_width`, so glyphs (and the selection/background rects drawn at
        // those same coords) line up regardless of the viewer's monospace font.
        // `spacingAndGlyphs` lets the browser redistribute advances to fit.
        let fit_attr = match self.mode {
            FontMode::Monospace => {
                let len = (text.graphemes(true).count() as f64 * mono_cell_width(size)).round();
                format!(
                    r#" textLength="{len}" lengthAdjust="spacingAndGlyphs" xml:space="preserve""#
                )
            }
            FontMode::Proportional => String::new(),
        };

        writeln!(
            &mut self.svg_content,
            r#"  <text x="{}" y="{}" fill="{}" font-family="{}" font-size="{}" font-weight="{}" font-style="{}"{}{}>{}</text>"#,
            x, y, color, family, size, weight, style, fit_attr, clip_attr, escaped_text
        )
        .unwrap();
    }

    fn draw_rect_filled(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let color = self.color_to_svg(self.current_color);

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

    fn text_width(&mut self, text: &str, font: FontType, style: FontStyle, size: u8) -> f64 {
        match self.mode {
            FontMode::Proportional => {
                let font_ref = self.fonts.as_ref().unwrap().load_font(font, style);
                let scale = Scale::uniform(size as f32);
                // Lay out glyphs (including kerning) and take the rightmost edge.
                let mut max_x: f32 = 0.0;
                for g in font_ref.layout(text, scale, point(0.0, 0.0)) {
                    let adv = g.unpositioned().h_metrics().advance_width;
                    let x = g.position().x + adv;
                    if x > max_x {
                        max_x = x;
                    }
                }
                max_x as f64 * BROWSER_SCALE
            }
            // One grapheme cluster == one cell on a fixed-width grid.
            FontMode::Monospace => text.graphemes(true).count() as f64 * mono_cell_width(size),
        }
    }

    fn text_height(&self, font: FontType, style: FontStyle, size: u8) -> i32 {
        match self.mode {
            FontMode::Proportional => {
                let font_ref = self.fonts.as_ref().unwrap().load_font(font, style);
                let v = font_ref.v_metrics(Scale::uniform(size as f32));
                (v.ascent - v.descent + v.line_gap).ceil() as i32
            }
            FontMode::Monospace => mono_line_height(size),
        }
    }

    fn text_descent(&self, font: FontType, style: FontStyle, size: u8) -> i32 {
        match self.mode {
            FontMode::Proportional => {
                let font_ref = self.fonts.as_ref().unwrap().load_font(font, style);
                let v = font_ref.v_metrics(Scale::uniform(size as f32));
                (-v.descent).ceil() as i32
            }
            FontMode::Monospace => mono_descent(size),
        }
    }

    fn push_clip(&mut self, x: i32, y: i32, w: i32, h: i32) {
        let clip_id = format!("clip-{}-{}-{}-{}", x, y, w, h);

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
        let bg_r = ((bg >> 24) & 0xFF) as f32 / 255.0;
        let bg_g = ((bg >> 16) & 0xFF) as f32 / 255.0;
        let bg_b = ((bg >> 8) & 0xFF) as f32 / 255.0;

        let bg_lum = 0.299 * bg_r + 0.587 * bg_g + 0.114 * bg_b;

        if bg_lum < 0.5 {
            0xFFFFFFFF
        } else {
            let fg_r = ((fg >> 24) & 0xFF) as f32 / 255.0;
            let fg_g = ((fg >> 16) & 0xFF) as f32 / 255.0;
            let fg_b = ((fg >> 8) & 0xFF) as f32 / 255.0;
            let fg_lum = 0.299 * fg_r + 0.587 * fg_g + 0.114 * fg_b;

            if (bg_lum - fg_lum).abs() < 0.3 {
                0x000000FF
            } else {
                fg
            }
        }
    }

    fn color_inactive(&self, c: u32) -> u32 {
        let r = ((c >> 24) & 0xFF) as f32;
        let g = ((c >> 16) & 0xFF) as f32;
        let b = ((c >> 8) & 0xFF) as f32;
        let a = (c & 0xFF) as f32;

        let gray = (r + g + b) / 3.0;

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

/// The bundled proportional faces (NotoSans), measured via `rusttype`. Only the
/// proportional backend needs real glyph metrics; monospace is synthetic.
struct FontSet {
    faces: HashMap<FontStyle, Font<'static>>,
    fallback_key: FontStyle,
}

impl FontSet {
    fn load_proportional() -> Self {
        let files = [
            (FontStyle::Regular, "tests/NotoSans-Medium.ttf"),
            (FontStyle::Bold, "tests/NotoSans-Bold.ttf"),
            (FontStyle::Italic, "tests/NotoSans-MediumItalic.ttf"),
            (FontStyle::BoldItalic, "tests/NotoSans-BoldItalic.ttf"),
        ];

        let mut faces: HashMap<FontStyle, Font<'static>> = HashMap::new();
        for (style, path) in files {
            if let Ok(bytes) = fs::read(path)
                && let Some(font) = Font::try_from_vec(bytes)
            {
                faces.insert(style, font);
            }
        }

        let fallback_key = [
            FontStyle::Regular,
            FontStyle::Bold,
            FontStyle::Italic,
            FontStyle::BoldItalic,
        ]
        .into_iter()
        .find(|s| faces.contains_key(s))
        .expect("No NotoSans test fonts found under tests/");

        FontSet {
            faces,
            fallback_key,
        }
    }

    fn load_font(&self, _font: FontType, style: FontStyle) -> &Font<'static> {
        self.faces
            .get(&style)
            .unwrap_or_else(|| &self.faces[&self.fallback_key])
    }
}

// ---------------------------------------------------------------------------
// Scenarios — shared between the proportional and monospace snapshot suites.
// Each returns the rendered SVG bytes for one document/interaction so the two
// suites differ only by the `FontMode` they pass in.
// ---------------------------------------------------------------------------

fn display_for(md: &str, w: i32, h: i32) -> StructuredRichDisplay {
    let mut display = StructuredRichDisplay::new(0, 0, w, h);
    display.editor_mut().set_tdoc(markdown_to_document(md));
    display
}

fn render(mut display: StructuredRichDisplay, mode: FontMode, w: i32, h: i32) -> Vec<u8> {
    let mut ctx = SvgDrawContext::new(mode, w, h);
    display.draw(&mut ctx);
    ctx.finish().into_bytes()
}

pub fn basic_render(mode: FontMode) -> Vec<u8> {
    let md = "# Heading 1\n\nThis is a paragraph with a [link](https://example.com).";
    let (w, h) = (600, 220);
    render(display_for(md, w, h), mode, w, h)
}

pub fn complex_rendering(mode: FontMode) -> Vec<u8> {
    let md = fs::read_to_string("tests/data/example.md").expect("read tests/data/example.md");
    let (w, h) = (400, 700);
    render(display_for(&md, w, h), mode, w, h)
}

pub fn selection_single_block(mode: FontMode) -> Vec<u8> {
    let md = "This is selection test.";
    let (w, h) = (500, 140);
    let mut display = display_for(md, w, h);
    let start = md.find("selection").unwrap();
    let end = start + "selection".len();
    display.editor_mut().set_selection(
        DocumentPosition::new(0, start),
        DocumentPosition::new(0, end),
    );
    render(display, mode, w, h)
}

pub fn cursor_positioning_middle_of_line(mode: FontMode) -> Vec<u8> {
    let md = "Paragraph with a cursor.";
    let (w, h) = (520, 140);
    let mut display = display_for(md, w, h);
    let pos = md.find("with").unwrap() + "with".len();
    display
        .editor_mut()
        .set_cursor(DocumentPosition::new(0, pos));
    render(display, mode, w, h)
}

pub fn table_render(mode: FontMode) -> Vec<u8> {
    let md = "# Shopping\n\n\
        | Item | Qty | Notes |\n\
        | --- | --- | --- |\n\
        | Apples | 3 | Granny Smith |\n\
        | Whole-grain bread | 1 | from the corner bakery |\n\
        | Coffee beans | 2 | dark roast |";
    let (w, h) = (520, 260);
    render(display_for(md, w, h), mode, w, h)
}

pub fn table_force_wrap_long_tokens(mode: FontMode) -> Vec<u8> {
    // Narrow columns with long unbreakable tokens (inline code / link syntax)
    // must force-wrap at character boundaries so a cell can't bleed into the
    // next column. Reproduces the embed-syntax comparison table.
    let md = "\
        | Tool | Default embed syntax | Where files go | How the path resolves |\n\
        | --- | --- | --- | --- |\n\
        | Obsidian | `![[image.png]]` (wikilink-embed) | Configurable: vault root or a named folder | \"Shortest path\" by default |\n\
        | Logseq | `![alt](../assets/image.png)` standard Markdown | `assets/` folder at graph root | Relative path |";
    let (w, h) = (520, 360);
    render(display_for(md, w, h), mode, w, h)
}

pub fn selection_across_blocks(mode: FontMode) -> Vec<u8> {
    let md = "# Title\n\nParagraph one with content.\n\nParagraph two continues here.";
    let (w, h) = (640, 260);
    let mut display = display_for(md, w, h);

    // Blocks: 0=Heading, 1=Para1, 2=Para2
    let para1 = "Paragraph one with content.";
    let para2 = "Paragraph two continues here.";
    let start = para1.find("with").unwrap() + 2; // mid-word for variety
    let end = para2.find("continues").unwrap() + "continues".len() - 2;
    display.editor_mut().set_selection(
        DocumentPosition::new(1, start),
        DocumentPosition::new(2, end),
    );
    render(display, mode, w, h)
}
