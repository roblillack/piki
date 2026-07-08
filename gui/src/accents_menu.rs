//! Enables the macOS "press and hold" accented-character popup for a custom widget.
//!
//! On macOS, holding a key such as `e` is meant to open a small popup offering
//! accented variants (é, ë, è, ê, …). FLTK only shows that popup for widgets that
//! carry the `MAC_USE_ACCENTS_MENU` flag: its `-[FLView selectedRange]` returns
//! `NSNotFound` — which disables the feature — unless the focused widget's
//! `use_accents_menu()` is true. FLTK sets the flag on its own `Fl_Input_` and
//! `Fl_Text_Editor`, but Piki's editor is a custom `Fl_Group`, so the flag is never
//! set and the popup never appears. (Once the flag is set, picking a variant from
//! the popup reaches us as ordinary backspace + text key events, which the editor
//! already handles.)
//!
//! Neither fltk-rs nor the underlying cfltk C layer exposes a setter for this flag —
//! `Fl_Widget::set_flag` is `protected` — so we set the bit directly on the C++
//! `Fl_Widget`'s private `flags_` field via the raw widget pointer.
//!
//! To avoid hard-coding a struct offset that could silently corrupt memory if
//! FLTK's layout ever changes, we *locate* `flags_` at runtime rather than assume
//! where it lives: `set_visible_focus()` / `clear_visible_focus()` (which fltk-rs
//! does expose) toggle the `VISIBLE_FOCUS` bit of that same `flags_` word, so we
//! find the one machine word in the widget that flips accordingly and OR the
//! `MAC_USE_ACCENTS_MENU` bit into it. If the field cannot be located unambiguously
//! the call is a safe no-op (the popup simply stays disabled).

/// Turn on the macOS press-and-hold accent popup for `widget`, which must be the
/// widget that receives keyboard focus while editing. No-op on non-macOS targets.
#[cfg(target_os = "macos")]
pub fn enable<W: fltk::prelude::WidgetExt>(widget: &mut W) {
    // Bit positions within `Fl_Widget::flags_` (see FLTK's `FL/Fl_Widget.H`).
    const VISIBLE_FOCUS: u32 = 1 << 9; // probe bit toggled to locate `flags_`
    const MAC_USE_ACCENTS_MENU: u32 = 1 << 19; // the bit we want to set

    // `flags_` sits ~96 bytes into `Fl_Widget`; scanning the leading 128 bytes
    // covers it with margin while staying well inside the (much larger) allocated
    // widget object, so the reads below cannot run past it.
    const SCAN_WORDS: usize = 32;

    let base = widget.as_widget_ptr() as *mut u32;
    if base.is_null() {
        return;
    }

    // Preserve the widget's current VISIBLE_FOCUS state so probing is invisible.
    let had_visible_focus = widget.has_visible_focus();

    let snapshot = |base: *const u32| -> [u32; SCAN_WORDS] {
        let mut words = [0u32; SCAN_WORDS];
        // SAFETY: `base` is the live C++ `Fl_Widget`; reading its first
        // `SCAN_WORDS` 4-byte words stays within the allocated object. The
        // pointer is 8-byte aligned, so every `u32` read is aligned. Reads only.
        for (i, slot) in words.iter_mut().enumerate() {
            *slot = unsafe { base.add(i).read() };
        }
        words
    };

    widget.clear_visible_focus();
    let cleared = snapshot(base);
    widget.set_visible_focus();
    let set = snapshot(base);

    // `flags_` is the unique word whose only difference between the two snapshots
    // is the probe bit. Anything else (zero matches, or more than one) means the
    // layout isn't what we expect, so we decline to touch memory.
    let mut flags_index = None;
    for i in 0..SCAN_WORDS {
        if set[i] ^ cleared[i] == VISIBLE_FOCUS {
            if flags_index.is_some() {
                flags_index = None; // ambiguous — bail out
                break;
            }
            flags_index = Some(i);
        }
    }

    // Restore the original VISIBLE_FOCUS state.
    if had_visible_focus {
        widget.set_visible_focus();
    } else {
        widget.clear_visible_focus();
    }

    if let Some(i) = flags_index {
        // SAFETY: `flags_ptr` was just confirmed to be the widget's `flags_` field
        // (it tracked the VISIBLE_FOCUS toggle), so OR-ing in another flag bit is
        // exactly what `Fl_Widget::set_flag` does internally.
        let flags_ptr = unsafe { base.add(i) };
        unsafe { flags_ptr.write(flags_ptr.read() | MAC_USE_ACCENTS_MENU) };
    }
}

/// No-op off macOS: the press-and-hold accent popup is a macOS feature.
#[cfg(not(target_os = "macos"))]
pub fn enable<W: fltk::prelude::WidgetExt>(_widget: &mut W) {}

/// Tell macOS where the text caret is so the accent popup (and any IME window)
/// appears next to it instead of at the window origin. `x`/`y` are in the focused
/// widget's window coordinates with `y` at the *bottom* of the caret; `height` is
/// the caret height. Call whenever the caret may have moved (e.g. after drawing).
///
/// FLTK positions the popup from this value in `-[FLView firstRectForCharacterRange:]`;
/// its built-in input widgets report it via `fl_set_spot`, but a custom widget must
/// do so itself. No-op off macOS.
#[cfg(target_os = "macos")]
pub fn report_caret(x: i32, y: i32, height: i32) {
    // `Fl::insertion_point_location(int, int, int)` — a stable (if deprecated)
    // FLTK entry point with no fltk-rs/cfltk binding, so we link the C++ symbol
    // directly. A wrong name fails loudly at link time, never silently.
    unsafe extern "C" {
        #[link_name = "_ZN2Fl24insertion_point_locationEiii"]
        fn fl_insertion_point_location(x: i32, y: i32, height: i32);
    }
    unsafe { fl_insertion_point_location(x, y, height) };
}

/// No-op off macOS.
#[cfg(not(target_os = "macos"))]
pub fn report_caret(_x: i32, _y: i32, _height: i32) {}
