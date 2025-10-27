use fltk::{
    button,
    enums::{Align, CallbackTrigger, Event, Key},
    input,
    prelude::{GroupExt, InputExt, WidgetBase, WidgetExt},
    window,
};

/// Options to configure the link editor dialog.
#[derive(Default)]
pub struct LinkEditOptions {
    /// Initial link target (URL/destination) to show in the dialog.
    pub init_target: String,
    /// Initial link text to show in the dialog.
    pub init_text: String,
    /// When editing an existing link. Enables the Remove button; text may be left empty to keep existing.
    pub mode_existing_link: bool,
    /// When a text selection exists. Text input may be left empty to keep selected text.
    pub selection_mode: bool,
    /// Optional rectangle (x, y, w, h) to center the dialog over. If None, center on primary screen.
    pub center_rect: Option<(i32, i32, i32, i32)>,
}

/// Show a link editor dialog and wire Save/Remove actions.
/// - `on_save(dest, text)` is invoked when Save is pressed and inputs validate.
/// - `on_remove()` is invoked when Remove is pressed (only enabled in `mode_existing_link`).
pub fn show_link_editor<FS, FR>(opts: LinkEditOptions, on_save: FS, on_remove: Option<FR>)
where
    FS: Fn(String, String) + 'static,
    FR: FnMut() + 'static,
{
    // Build dialog window
    let mut win = window::Window::new(0, 0, 420, 160, Some("Edit Link"));

    // Target row
    let mut target_label = fltk::frame::Frame::new(10, 10, 120, 24, Some("Link target:"));
    target_label.set_align(Align::Inside | Align::Left);
    let mut target_input = input::Input::new(130, 10, 280, 24, None);
    target_input.set_value(&opts.init_target);

    // Text row
    let mut text_label = fltk::frame::Frame::new(10, 44, 120, 24, Some("Link text:"));
    text_label.set_align(Align::Inside | Align::Left);
    let mut text_input_w = input::Input::new(130, 44, 280, 24, None);
    text_input_w.set_value(&opts.init_text);

    // Buttons
    let mut remove_btn = button::Button::new(130, 110, 80, 30, Some("Remove"));
    let mut cancel_btn = button::Button::new(220, 110, 80, 30, Some("Cancel"));
    let mut save_btn = button::ReturnButton::new(310, 110, 80, 30, Some("Save"));

    if !opts.mode_existing_link {
        remove_btn.deactivate();
    }

    // Initial validation state
    let initial_text_required = !(opts.mode_existing_link || opts.selection_mode);
    let target_ok = !target_input.value().trim().is_empty();
    let text_ok = if initial_text_required {
        !text_input_w.value().trim().is_empty()
    } else {
        true
    };
    if target_ok && text_ok {
        save_btn.activate();
    } else {
        save_btn.deactivate();
    }

    // Live validation callbacks
    {
        let mut save_btn_v = save_btn.clone();
        let tgt_v = target_input.clone();
        let txt_v = text_input_w.clone();
        let require_text = initial_text_required;
        let validate_cb = move |_i: &mut input::Input| {
            let target_ok = !tgt_v.value().trim().is_empty();
            let text_ok = if require_text {
                !txt_v.value().trim().is_empty()
            } else {
                true
            };
            if target_ok && text_ok {
                save_btn_v.activate();
            } else {
                save_btn_v.deactivate();
            }
        };
        target_input.set_trigger(CallbackTrigger::Changed);
        target_input.set_callback(validate_cb.clone());
        text_input_w.set_trigger(CallbackTrigger::Changed);
        text_input_w.set_callback(validate_cb);
    }

    // Wire Save/Remove/Cancel
    let mut win_for_save = win.clone();
    let mut win_for_remove = win.clone();
    let mut win_for_cancel = win.clone();
    let target_input_s = target_input.clone();
    let text_input_s = text_input_w.clone();
    let init_text_s = opts.init_text.clone();

    save_btn.set_callback(move |_| {
        let dest = target_input_s.value();
        let txt = if opts.selection_mode || opts.mode_existing_link {
            let val = text_input_s.value();
            if !val.is_empty() {
                val
            } else {
                init_text_s.clone()
            }
        } else {
            text_input_s.value()
        };
        on_save(dest, txt);
        win_for_save.hide();
    });

    if let Some(on_remove_cb) = on_remove {
        let mut on_remove_cb = on_remove_cb;
        remove_btn.set_callback(move |_| {
            on_remove_cb();
            win_for_remove.hide();
        });
    } else {
        // If no remove handler provided, just hide window on click (should be disabled anyway).
        remove_btn.set_callback(move |_| {
            win_for_remove.hide();
        });
    }

    cancel_btn.set_callback(move |_| {
        win_for_cancel.hide();
    });

    win.end();

    // Position the dialog: center over provided rect or screen
    win.make_resizable(false);
    let dlg_w = 420;
    let dlg_h = 160;
    if let Some((px, py, pw, ph)) = opts.center_rect {
        let cx = px + (pw - dlg_w) / 2;
        let cy = py + (ph - dlg_h) / 2;
        win.set_pos(cx.max(0), cy.max(0));
    } else {
        let (sx, sy, sw, sh) = fltk::app::screen_xywh(0);
        let cx = sx + (sw - dlg_w) / 2;
        let cy = sy + (sh - dlg_h) / 2;
        win.set_pos(cx.max(0), cy.max(0));
    }

    win.show();
    let _ = target_input.take_focus();

    // Wire Escape to Cancel using window handler
    let mut cancel_btn_h = cancel_btn.clone();
    win.handle(move |_, e| {
        if e == Event::KeyDown {
            let k = fltk::app::event_key();
            if k == Key::Escape {
                if cancel_btn_h.active() {
                    cancel_btn_h.do_callback();
                }
                return true;
            }
        }
        false
    });
}
