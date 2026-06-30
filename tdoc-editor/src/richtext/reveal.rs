// Reveal-codes support (a Pure/terminal feature, off by default).
//
// "Reveal codes" shows a document's inline-style structure inline, the way
// classic Pure (and WordPerfect before it) did: a styled span renders with a
// `[Bold>` tag where it opens and a `<Bold]` tag where it closes, nesting for
// stacked styles (`[Bold>styles [Highlight>gets messy<Highlight]<Bold]`).
//
// The tags are derived purely from the leaf's flattened inline runs by a small
// stack reconciler: walking the runs left to right, a style that is present in a
// run but not the running stack opens (push + start tag), and styles that fall
// off the stack close (pop + end tags, innermost first). This reproduces classic
// Pure's nesting — a style stays open across an inner style's span rather than
// closing and reopening — without needing the raw tdoc span tree.
//
// The same reconciler powers both the display (which lays the tags out as
// zero-width, cursor-skipped runs) and the editor (where backspacing into a tag
// removes that style from its span instead of deleting text).

use super::structured_document::{InlineContent, TextStyle};

/// An inline style that reveal codes surfaces as a `[Name>` / `<Name]` tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevealStyle {
    Highlight,
    Underline,
    Strikethrough,
    Bold,
    Italic,
    Code,
    Link,
}

impl RevealStyle {
    /// The tag label (`Bold`, `Highlight`, …); matches classic Pure's names.
    pub(crate) fn label(self) -> &'static str {
        match self {
            RevealStyle::Highlight => "Highlight",
            RevealStyle::Underline => "Underline",
            RevealStyle::Strikethrough => "Strikethrough",
            RevealStyle::Bold => "Bold",
            RevealStyle::Italic => "Italic",
            RevealStyle::Code => "Code",
            RevealStyle::Link => "Link",
        }
    }
}

/// The reveal styles active on a text run, outermost-first — the order classic
/// Pure nested them (highlight outermost, italic/code innermost). Only used to
/// order styles that open at the *same* boundary; styles inherited from an
/// earlier run keep their existing stack order.
pub(crate) fn reveal_styles(style: &TextStyle) -> Vec<RevealStyle> {
    let mut v = Vec::new();
    if style.highlight {
        v.push(RevealStyle::Highlight);
    }
    if style.underline {
        v.push(RevealStyle::Underline);
    }
    if style.strikethrough {
        v.push(RevealStyle::Strikethrough);
    }
    if style.bold {
        v.push(RevealStyle::Bold);
    }
    if style.italic {
        v.push(RevealStyle::Italic);
    }
    if style.code {
        v.push(RevealStyle::Code);
    }
    v
}

/// The reveal styles of one inline run, in outer→inner order. A link is a single
/// `Link` scope (its inner styling is not separately tagged); a hard break never
/// changes the style stack, so it yields `None`.
pub(crate) fn item_reveal_styles(item: &InlineContent) -> Option<Vec<RevealStyle>> {
    match item {
        InlineContent::Text(run) => Some(reveal_styles(&run.style)),
        InlineContent::Link { .. } => Some(vec![RevealStyle::Link]),
        InlineContent::HardBreak => None,
    }
}

/// Clear one reveal style flag from a run style (used when a tag is deleted).
pub(crate) fn clear_reveal_style(style: &mut TextStyle, which: RevealStyle) {
    match which {
        RevealStyle::Highlight => style.highlight = false,
        RevealStyle::Underline => style.underline = false,
        RevealStyle::Strikethrough => style.strikethrough = false,
        RevealStyle::Bold => style.bold = false,
        RevealStyle::Italic => style.italic = false,
        RevealStyle::Code => style.code = false,
        // A link is a structural inline element, not a style flag, so there is
        // nothing to clear on the text runs.
        RevealStyle::Link => {}
    }
}

/// Tracks the open reveal-style tags as runs are walked left to right.
#[derive(Default)]
pub(crate) struct RevealReconciler {
    stack: Vec<RevealStyle>,
}

impl RevealReconciler {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reconcile the open stack to `target`, returning the tags to render at this
    /// boundary: `closes` (the styles that end here, innermost-first) followed by
    /// `opens` (the styles that begin here, outermost-first). Mutates the stack to
    /// match `target`.
    pub(crate) fn reconcile(
        &mut self,
        target: &[RevealStyle],
    ) -> (Vec<RevealStyle>, Vec<RevealStyle>) {
        // Keep the longest prefix of the stack that is still wanted; everything
        // above it closes (top-down → innermost first).
        let mut keep = 0;
        while keep < self.stack.len() && target.contains(&self.stack[keep]) {
            keep += 1;
        }
        let mut closes = Vec::new();
        while self.stack.len() > keep {
            closes.push(self.stack.pop().unwrap());
        }
        // Open everything in `target` not already on the stack, in target order
        // (outermost first) so the new tags nest the classic way.
        let mut opens = Vec::new();
        for &style in target {
            if !self.stack.contains(&style) {
                self.stack.push(style);
                opens.push(style);
            }
        }
        (closes, opens)
    }

    /// Close every still-open tag (innermost first) — call after the last run.
    pub(crate) fn finish(&mut self) -> Vec<RevealStyle> {
        let (closes, _) = self.reconcile(&[]);
        closes
    }
}

/// Per-run reveal styles with the run's byte range in the leaf's flattened text.
fn run_style_ranges(content: &[InlineContent]) -> Vec<(Vec<RevealStyle>, usize, usize)> {
    let mut out = Vec::new();
    let mut offset = 0usize;
    for item in content {
        let len = item.text_len();
        if let Some(styles) = item_reveal_styles(item) {
            out.push((styles, offset, offset + len));
        }
        offset += len;
    }
    out
}

/// Forward extent `[offset, end)` of the contiguous runs (starting at `offset`)
/// that all carry `style`.
fn extent_forward(
    runs: &[(Vec<RevealStyle>, usize, usize)],
    offset: usize,
    style: RevealStyle,
) -> (usize, usize) {
    let mut end = offset;
    for (styles, start, run_end) in runs {
        if *start < offset {
            continue;
        }
        if styles.contains(&style) {
            end = *run_end;
        } else {
            break;
        }
    }
    (offset, end)
}

/// Backward extent `[start, offset)` of the contiguous runs (ending at `offset`)
/// that all carry `style`.
fn extent_backward(
    runs: &[(Vec<RevealStyle>, usize, usize)],
    offset: usize,
    style: RevealStyle,
) -> (usize, usize) {
    let mut start = offset;
    for (styles, run_start, run_end) in runs.iter().rev() {
        if *run_end > offset {
            continue;
        }
        if styles.contains(&style) {
            start = *run_start;
        } else {
            break;
        }
    }
    (start, offset)
}

/// Number of reveal tags rendered at byte `offset` — the count of intermediate
/// cursor stops there (closes + opens at that boundary). Zero when `offset` is
/// not a style boundary, so the caret steps a plain character.
pub(crate) fn reveal_tag_count_at(content: &[InlineContent], offset: usize) -> usize {
    let runs = run_style_ranges(content);
    let mut recon = RevealReconciler::new();
    for (styles, start, _end) in &runs {
        let (closes, opens) = recon.reconcile(styles);
        if *start == offset {
            return closes.len() + opens.len();
        }
    }
    let total = runs.last().map(|r| r.2).unwrap_or(0);
    if total == offset {
        return recon.finish().len();
    }
    0
}

/// Every byte offset that carries reveal tags (a style boundary), in order.
fn tag_boundaries(content: &[InlineContent]) -> Vec<usize> {
    let runs = run_style_ranges(content);
    let mut recon = RevealReconciler::new();
    let mut out = Vec::new();
    for (styles, start, _end) in &runs {
        let (closes, opens) = recon.reconcile(styles);
        if !closes.is_empty() || !opens.is_empty() {
            out.push(*start);
        }
    }
    let total = runs.last().map(|r| r.2).unwrap_or(0);
    if !recon.finish().is_empty() {
        out.push(total);
    }
    out
}

/// The nearest style boundary (offset carrying reveal tags) strictly after
/// `offset`, so word-wise motion can stop on tags the way classic Pure did
/// ("reveal tags count as one word").
pub(crate) fn next_tag_boundary(content: &[InlineContent], offset: usize) -> Option<usize> {
    tag_boundaries(content).into_iter().find(|&b| b > offset)
}

/// The nearest style boundary strictly before `offset`.
pub(crate) fn prev_tag_boundary(content: &[InlineContent], offset: usize) -> Option<usize> {
    tag_boundaries(content).into_iter().rev().find(|&b| b < offset)
}

/// The reveal tag rendered immediately to one side of `offset`, if any, plus the
/// byte range of the span whose style it represents. `backward` picks the tag to
/// the left of the caret (the last one printed at the boundary); otherwise the
/// tag to the right (the first one printed). Returns the style and the
/// `[start, end)` range over which to clear it.
pub(crate) fn reveal_tag_at(
    content: &[InlineContent],
    offset: usize,
    backward: bool,
) -> Option<(RevealStyle, usize, usize)> {
    let runs = run_style_ranges(content);

    // Find the boundary at `offset` and the tags emitted there.
    let mut recon = RevealReconciler::new();
    let mut boundary: Option<(Vec<RevealStyle>, Vec<RevealStyle>)> = None;
    for (styles, start, _end) in &runs {
        let (closes, opens) = recon.reconcile(styles);
        if *start == offset {
            boundary = Some((closes, opens));
            break;
        }
    }
    if boundary.is_none() {
        let total = runs.last().map(|r| r.2).unwrap_or(0);
        if total == offset {
            boundary = Some((recon.finish(), Vec::new()));
        }
    }
    let (closes, opens) = boundary?;

    // The tag adjacent to the caret: backspace removes the last-printed tag at
    // the boundary (`<Bold]` over `<Highlight]`, or the lone `[Italic>`); delete
    // removes the first-printed one.
    let (style, is_open) = if backward {
        if let Some(&s) = opens.last() {
            (s, true)
        } else if let Some(&s) = closes.last() {
            (s, false)
        } else {
            return None;
        }
    } else if let Some(&s) = closes.first() {
        (s, false)
    } else if let Some(&s) = opens.first() {
        (s, true)
    } else {
        return None;
    };

    let (start, end) = if is_open {
        extent_forward(&runs, offset, style)
    } else {
        extent_backward(&runs, offset, style)
    };
    Some((style, start, end))
}
