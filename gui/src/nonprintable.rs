//! Handling of characters that have no printable form.
//!
//! A control character is the worst kind of stowaway in a note: Quartz paints
//! nothing for it, yet `fl_width()` still measures a glyph advance — so it
//! surfaces as a phantom gap in front of the *next* word, in a place the
//! character isn't, with no way to see it, click it, or aim a Backspace at it.
//! The same character then travels into the saved Markdown as a raw byte and
//! into the live-share HTML, where the browser paints its own idea of nothing.
//!
//! Two defenses, both here so the notion of "not printable" is defined once:
//! [`sanitize_key_text`] keeps such a character from entering the document by
//! keystroke in the first place, and [`printable`] makes any that is already in
//! one (typed by an older build, pasted, or written by another editor) visible
//! wherever the document is displayed.

use std::borrow::Cow;

/// A visible stand-in for a character that has no printable form, or `None` if
/// the character should be shown as-is.
///
/// `\n` and `\t` are deliberately left alone: those are real layout whitespace
/// that the renderer positions itself.
pub fn control_picture(ch: char) -> Option<char> {
    match ch {
        '\n' | '\t' => None,
        // Unicode Control Pictures: U+2400 + code point, i.e. ␀..␟ (ESC -> ␛).
        '\u{0}'..='\u{1f}' => char::from_u32(0x2400 + ch as u32),
        '\u{7f}' => Some('\u{2421}'), // ␡
        // The C1 controls have no picture of their own; flag them generically.
        '\u{80}'..='\u{9f}' => Some('\u{fffd}'), // <?>
        _ => None,
    }
}

/// Maps `text` to what should actually be shown, substituting the
/// [`control_picture`] of every non-printable character. Borrows unchanged in the
/// overwhelmingly common case that there is nothing to substitute.
///
/// The mapping is one char in, one char out, which is what lets a caller apply it
/// to a substring: measuring `printable(prefix)` still lands where the same
/// prefix is painted, so line wrapping, caret positions and selection rectangles
/// all stay consistent with what the user sees.
pub fn printable(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|ch| control_picture(ch).is_some()) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.chars()
            .map(|ch| control_picture(ch).unwrap_or(ch))
            .collect(),
    )
}

/// Strips the characters that must never reach the document from a key event's
/// text.
///
/// FLTK fills `Fl::e_text` for *every* key that AppKit routes through
/// `doCommandBySelector:` — including keys with no editing meaning here — using
/// the raw character of the event. Escape therefore arrives as `"\u{1b}"` and the
/// function keys as private-use codepoints (`NSF1FunctionKey` and friends,
/// `U+F704`..), and inserting either verbatim puts a stowaway in the text.
///
/// Line and tab characters are dropped too: Enter and Tab are handled as
/// structural edits by their own key branches, so anything left here is noise.
pub fn sanitize_key_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() && !matches!(*ch, '\u{f700}'..='\u{f8ff}'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{printable, sanitize_key_text};
    use std::borrow::Cow;

    #[test]
    fn printable_leaves_ordinary_text_borrowed() {
        assert!(matches!(printable("Deployment [RL]"), Cow::Borrowed(_)));
        assert_eq!(printable("Ünïcödé — 😀\tok\n"), "Ünïcödé — 😀\tok\n");
    }

    #[test]
    fn printable_substitutes_control_pictures() {
        // The bug that started this: Escape typed an ESC into a note.
        assert_eq!(printable("Depl\u{1b}oyment"), "Depl␛oyment");
        assert_eq!(printable("\u{0}\u{7}\u{1f}"), "␀␇␟");
        assert_eq!(printable("\u{7f}"), "␡");
        assert_eq!(printable("\u{85}"), "\u{fffd}");
    }

    #[test]
    fn printable_preserves_char_count_so_measurements_stay_aligned() {
        let text = "a\u{1b}b\u{7f}c";
        assert_eq!(printable(text).chars().count(), text.chars().count());
    }

    #[test]
    fn sanitize_key_text_keeps_ordinary_typed_text() {
        assert_eq!(sanitize_key_text("a"), "a");
        assert_eq!(sanitize_key_text("Ünïcödé — ok 😀"), "Ünïcödé — ok 😀");
        assert_eq!(sanitize_key_text(" "), " ");
    }

    #[test]
    fn sanitize_key_text_drops_control_and_function_key_codepoints() {
        // Escape: FLTK reports the raw character of the event as e_text.
        assert_eq!(sanitize_key_text("\u{1b}"), "");
        // Tab/Enter have their own key branches; anything left is noise.
        assert_eq!(sanitize_key_text("\t"), "");
        assert_eq!(sanitize_key_text("\r"), "");
        // NSF1FunctionKey and the rest of AppKit's function-key block.
        assert_eq!(sanitize_key_text("\u{f704}"), "");
        assert_eq!(sanitize_key_text("a\u{1b}b"), "ab");
    }
}
