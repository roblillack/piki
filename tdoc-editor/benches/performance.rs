//! Performance benchmarks for the shared `tdoc-editor` core.
//!
//! Ported in spirit from Pure's `benches/performance.rs`: it measures the cost of
//! a full layout pass and a full edit cycle across document sizes. Layout is
//! driven through a cheap char-cell [`DrawContext`] stub so the numbers reflect
//! the engine's *algorithm* cost (leaf enumeration, span flattening, wrapping,
//! run building) rather than any real font system.
//!
//! Run with: `cargo bench -p tdoc-editor`
//!
//! NOTE: a real FLTK backend pays additional per-token font-measurement cost that
//! this stub does not, so the layout numbers here are an algorithmic lower bound.

use std::time::Instant;

use tdoc_editor::StructuredRichDisplay;
use tdoc_editor::draw_context::{DrawContext, FontStyle, FontType};
use tdoc_editor::richtext::markdown_converter::markdown_to_document;

/// Char-cell stub: width = char count, height = 1. No real font system.
struct StubCtx;

impl DrawContext for StubCtx {
    fn set_color(&mut self, _c: u32) {}
    fn set_font(&mut self, _f: FontType, _s: FontStyle, _sz: u8) {}
    fn draw_text(&mut self, _t: &str, _x: i32, _y: i32) {}
    fn draw_rect_filled(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}
    fn draw_line(&mut self, _a: i32, _b: i32, _c: i32, _d: i32) {}
    fn text_width(&mut self, text: &str, _f: FontType, _s: FontStyle, size: u8) -> f64 {
        text.chars().count() as f64 * (size.max(1) as f64 * 0.5)
    }
    fn text_height(&self, _f: FontType, _s: FontStyle, size: u8) -> i32 {
        (size.max(1) as f64 * 1.3).ceil() as i32
    }
    fn text_descent(&self, _f: FontType, _s: FontStyle, _sz: u8) -> i32 {
        0
    }
    fn push_clip(&mut self, _x: i32, _y: i32, _w: i32, _h: i32) {}
    fn pop_clip(&mut self) {}
    fn color_average(&self, c1: u32, _c2: u32, _w: f32) -> u32 {
        c1
    }
    fn color_contrast(&self, fg: u32, _bg: u32) -> u32 {
        fg
    }
    fn color_inactive(&self, c: u32) -> u32 {
        c
    }
    fn has_focus(&self) -> bool {
        true
    }
    fn is_active(&self) -> bool {
        true
    }
}

fn make_markdown(num_paras: usize, words: usize) -> String {
    let mut out = String::new();
    for i in 0..num_paras {
        for w in 0..words {
            if w > 0 {
                out.push(' ');
            }
            out.push_str("word");
            out.push_str(&((i + w) % 9).to_string());
        }
        out.push_str("\n\n");
    }
    out
}

/// Average wall-clock ms over `iters` runs (after one warmup).
fn time_ms<F: FnMut()>(iters: usize, mut f: F) -> f64 {
    f(); // warmup
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed().as_secs_f64() * 1000.0 / iters as f64
}

fn new_display(markdown: &str) -> StructuredRichDisplay {
    let doc = markdown_to_document(markdown);
    let mut d = StructuredRichDisplay::new(0, 0, 800, 600);
    d.editor_mut().set_tdoc(doc);
    d
}

fn main() {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║        tdoc-editor performance (char-cell stub ctx)          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
    println!(
        "{:>7} │ {:>12} │ {:>12} │ {:>12} │ {:>12}",
        "paras", "layout(draw)", "edit+layout", "edit-only", "cursor-down"
    );
    println!("{:─<8}┼{:─<14}┼{:─<14}┼{:─<14}┼{:─<14}", "", "", "", "", "");

    for (paras, iters) in [(10usize, 200usize), (100, 200), (1000, 50), (10000, 10)] {
        let md = make_markdown(paras, 10);

        // 1. Full layout per draw (editor_mut() invalidates layout each iter).
        let mut d = new_display(&md);
        let layout_ms = time_ms(iters, || {
            let _ = d.editor_mut(); // force re-layout
            d.draw(&mut StubCtx);
        });

        // 2. Full edit cycle: insert a char + relayout/draw.
        let mut d = new_display(&md);
        let edit_layout_ms = time_ms(iters, || {
            let _ = d.editor_mut().insert_text("x");
            d.draw(&mut StubCtx);
        });

        // 3. Edit-only: the mutation cost without layout.
        let mut d = new_display(&md);
        let edit_only_ms = time_ms(iters, || {
            let _ = d.editor_mut().insert_text("x");
        });

        // 4. Cursor movement (down) cost.
        let mut d = new_display(&md);
        let cursor_ms = time_ms(iters, || {
            d.editor_mut().move_cursor_down();
        });

        println!(
            "{:>7} │ {:>9.4} ms │ {:>9.4} ms │ {:>9.4} ms │ {:>9.4} ms",
            paras, layout_ms, edit_layout_ms, edit_only_ms, cursor_ms
        );
    }
    println!();
}
