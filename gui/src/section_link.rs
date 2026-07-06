//! Section links: linking to a specific heading inside a note.
//!
//! A section link has two forms:
//!
//! * the **internal** form stored in Markdown link destinations and understood
//!   by the in-app navigation — `path/to/note#section-slug`; and
//! * the **URL** form that is registered with the OS and works from other apps —
//!   `piki://path/to/note#section-slug`.
//!
//! "Copy Link to Section" (Cmd-Shift-K) puts the URL form on the clipboard, so
//! it is clickable everywhere; pasting such a URL into the link editor strips it
//! back to the internal form via [`normalize_link_target`]. Both forms share the
//! same `#section-slug` fragment, and [`heading_slug`] is the single source of
//! truth for turning a heading's text into that slug — used both when a link is
//! generated and when one is resolved back to a heading, so the two always agree.

/// The custom URL scheme Piki registers with the operating system.
pub const URL_SCHEME: &str = "piki";

/// Turn a heading's plain text into an anchor slug.
///
/// Lower-cases the text, keeps (Unicode) alphanumerics, and collapses any run of
/// whitespace, `-`, or `_` into a single `-`, dropping all other punctuation.
/// Leading and trailing dashes are trimmed. This is deliberately simple and,
/// crucially, *self-consistent*: the same function generates the slug written
/// into a link and resolves it back to a heading, so exact GitHub compatibility
/// is not required — only that generation and resolution agree.
///
/// Duplicate headings are disambiguated by [`heading_anchors`], not here.
pub fn heading_slug(text: &str) -> String {
    let mut slug = String::new();
    let mut pending_dash = false;
    for c in text.chars() {
        if c.is_alphanumeric() {
            if pending_dash && !slug.is_empty() {
                slug.push('-');
            }
            pending_dash = false;
            slug.extend(c.to_lowercase());
        } else if c.is_whitespace() || c == '-' || c == '_' {
            // Defer emitting the separator so trailing separators never make it
            // into the slug and runs collapse to a single dash.
            pending_dash = true;
        }
        // Any other character (punctuation, symbols) is dropped.
    }
    slug
}

/// Compute unique anchor slugs for a document's headings, in document order.
///
/// Headings that slug to the same base get a numeric suffix (`-1`, `-2`, …) in
/// order of appearance, mirroring how GitHub disambiguates repeated headings, so
/// a link to the second "Notes" heading resolves to that heading rather than the
/// first. Callers pair the returned slugs positionally with the headings they
/// passed in.
pub fn heading_anchors<S: AsRef<str>>(heading_texts: &[S]) -> Vec<String> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut anchors = Vec::with_capacity(heading_texts.len());
    for text in heading_texts {
        let base = heading_slug(text.as_ref());
        let seen = counts.entry(base.clone()).or_insert(0);
        let anchor = if *seen == 0 {
            base.clone()
        } else {
            format!("{base}-{seen}")
        };
        *seen += 1;
        anchors.push(anchor);
    }
    anchors
}

/// Split a link destination into its note part and optional `#fragment`.
///
/// Splits on the first `#`; the fragment is returned without the `#`. A trailing
/// `#` with nothing after it yields `Some("")`, which callers treat as "no
/// section".
pub fn split_target(dest: &str) -> (&str, Option<&str>) {
    match dest.find('#') {
        Some(i) => (&dest[..i], Some(&dest[i + 1..])),
        None => (dest, None),
    }
}

/// Build the `piki://` URL form of a link to `note`, optionally at `anchor`.
///
/// The note path and the fragment are percent-encoded so the result is a valid,
/// clickable URL even when the note name contains spaces or other characters
/// that are not URL-safe. [`normalize_link_target`] reverses this.
pub fn build_piki_url(note: &str, anchor: Option<&str>) -> String {
    let mut url = format!("{URL_SCHEME}://{}", encode_path(note));
    if let Some(anchor) = anchor.filter(|a| !a.is_empty()) {
        url.push('#');
        url.push_str(&encode_component(anchor));
    }
    url
}

/// Normalize a link destination for storage in a note.
///
/// If `input` is a `piki:` URL it is stripped back to the internal
/// `note#fragment` form (percent-decoding the note path so `%20` becomes a
/// space). Anything else — a plain note name, a relative section link, or an
/// external URL like `https://…` — is returned unchanged (aside from trimming
/// surrounding whitespace on a recognized `piki:` URL only). This is what the
/// link editor applies when a `piki://…` URL is pasted into the target field.
pub fn normalize_link_target(input: &str) -> String {
    match strip_scheme(input.trim()) {
        Some(rest) => percent_decode(rest),
        None => input.to_string(),
    }
}

/// If `s` begins with the `piki` scheme, return the remainder (path + fragment)
/// with the scheme and any `//` authority marker removed. Case-insensitive.
fn strip_scheme(s: &str) -> Option<&str> {
    let lower = s.to_ascii_lowercase();
    if lower.starts_with("piki://") {
        Some(&s["piki://".len()..])
    } else if lower.starts_with("piki:") {
        Some(&s["piki:".len()..])
    } else {
        None
    }
}

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encode a note path, preserving `/` path separators.
fn encode_path(s: &str) -> String {
    encode_with(s, |b| is_unreserved(b) || b == b'/')
}

/// Percent-encode a single URL component (the fragment), encoding `/` too.
fn encode_component(s: &str) -> String {
    encode_with(s, is_unreserved)
}

fn encode_with(s: &str, keep: impl Fn(u8) -> bool) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if keep(b) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push_str(&format!("{b:02X}"));
        }
    }
    out
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let (Some(hi), Some(lo)) = (hex_val(bytes[i + 1]), hex_val(bytes[i + 2]))
        {
            out.push(hi * 16 + lo);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_val(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_basics() {
        assert_eq!(heading_slug("Hello World"), "hello-world");
        assert_eq!(heading_slug("Security Model"), "security-model");
        assert_eq!(heading_slug("  Trailing spaces  "), "trailing-spaces");
        assert_eq!(heading_slug("Notes: Meeting!"), "notes-meeting");
        assert_eq!(heading_slug("Q4 — Budget (2026)"), "q4-budget-2026");
        assert_eq!(heading_slug("under_score and-dash"), "under-score-and-dash");
        assert_eq!(heading_slug("multiple   spaces"), "multiple-spaces");
        assert_eq!(heading_slug("---"), "");
        assert_eq!(heading_slug(""), "");
    }

    #[test]
    fn anchors_disambiguate_duplicates() {
        let headings = ["Notes", "Details", "Notes", "notes"];
        let anchors = heading_anchors(&headings);
        assert_eq!(anchors, vec!["notes", "details", "notes-1", "notes-2"]);
    }

    #[test]
    fn split_target_splits_on_first_hash() {
        assert_eq!(split_target("note"), ("note", None));
        assert_eq!(split_target("note#sec"), ("note", Some("sec")));
        assert_eq!(
            split_target("path/to/note#sec-tion"),
            ("path/to/note", Some("sec-tion"))
        );
        // Only the first '#' delimits the fragment.
        assert_eq!(split_target("note#a#b"), ("note", Some("a#b")));
        assert_eq!(split_target("note#"), ("note", Some("")));
    }

    #[test]
    fn build_url_roundtrips_through_normalize() {
        let url = build_piki_url("work/auth-refactor", Some("security-model"));
        assert_eq!(url, "piki://work/auth-refactor#security-model");
        assert_eq!(
            normalize_link_target(&url),
            "work/auth-refactor#security-model"
        );

        // Without an anchor.
        let url = build_piki_url("frontpage", None);
        assert_eq!(url, "piki://frontpage");
        assert_eq!(normalize_link_target(&url), "frontpage");

        // An empty anchor is treated as no section.
        assert_eq!(build_piki_url("frontpage", Some("")), "piki://frontpage");
    }

    #[test]
    fn build_url_percent_encodes_spaces() {
        let url = build_piki_url("Notes: Meeting", Some("agenda"));
        assert_eq!(url, "piki://Notes%3A%20Meeting#agenda");
        assert_eq!(normalize_link_target(&url), "Notes: Meeting#agenda");
    }

    #[test]
    fn normalize_leaves_non_piki_untouched() {
        assert_eq!(normalize_link_target("note#sec"), "note#sec");
        assert_eq!(normalize_link_target("path/to/note"), "path/to/note");
        assert_eq!(
            normalize_link_target("https://example.com/x"),
            "https://example.com/x"
        );
        // A partially typed value is returned verbatim (no trimming) so it does
        // not fight the user mid-edit.
        assert_eq!(normalize_link_target("  note "), "  note ");
    }

    #[test]
    fn normalize_handles_scheme_case_and_missing_slashes() {
        assert_eq!(normalize_link_target("PIKI://frontpage"), "frontpage");
        assert_eq!(normalize_link_target("piki:frontpage#top"), "frontpage#top");
    }

    #[test]
    fn percent_decode_tolerates_stray_percent() {
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("%41%42"), "AB");
    }
}
