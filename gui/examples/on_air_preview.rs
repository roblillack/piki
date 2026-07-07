//! Visual preview of the ON AIR bar: blinking light, hyperlink-styled URL, and
//! Stop button. Not shipped; used to eyeball layout and styling.
//!
//! Run with: `cargo run -p piki-gui --example on_air_preview`

use std::time::Instant;

use fltk::{app, enums, frame, prelude::*, window};
use piki_gui::on_air_bar::{self, OnAirBar};

fn main() {
    let app = app::App::default();
    let w = 720;
    let h = 200;
    let mut win = window::Window::new(100, 100, w, h, "ON AIR preview");
    win.begin();

    let mut on_air = OnAirBar::new(0, 0, w);
    on_air.set_url("http://localhost:50334/vizzlo/sprint-planning/2026q3.1");
    on_air.on_link_click(|| println!("link clicked"));
    on_air.on_stop(|| println!("stop clicked"));

    // A stand-in for the editor below the bar.
    let mut editor = frame::Frame::new(
        0,
        on_air_bar::HEIGHT,
        w,
        h - on_air_bar::HEIGHT,
        "editor area",
    );
    editor.set_frame(enums::FrameType::FlatBox);
    editor.set_color(enums::Color::from_rgb(255, 255, 245));

    win.end();
    win.show();

    on_air.show();

    let start = Instant::now();
    app::add_timeout3(0.1, move |handle| {
        on_air.tick(start.elapsed().as_millis() as u64);
        app::repeat_timeout3(0.1, handle);
    });

    app.run().unwrap();
}
