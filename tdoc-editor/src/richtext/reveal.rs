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

    /// The currently-open styles, outermost first.
    fn stack(&self) -> &[RevealStyle] {
        &self.stack
    }
}

/// The reveal-tag model for a leaf: the tag boundaries (in render order) and the
/// active styles over each text run. It walks the content **the same way the
/// display does** — recursing into links so their `[Link>` scope and their inner
/// runs' style tags (`[Bold>`…) are modeled too — so cursor stops, tag counts and
/// tag removal stay consistent with what's drawn.
struct RevealModel {
    /// `(offset, closes, opens)` at each boundary, in order. An offset can repeat
    /// (e.g. a link's `[Link>` and its first inner `[Bold>` are both at the link
    /// start); the tags there are the boundaries concatenated in this order.
    boundaries: Vec<(usize, Vec<RevealStyle>, Vec<RevealStyle>)>,
    /// `(start, end, active styles)` per non-empty text run.
    runs: Vec<(usize, usize, Vec<RevealStyle>)>,
}

fn build_reveal_model(content: &[InlineContent]) -> RevealModel {
    let mut recon = RevealReconciler::new();
    let mut offset = 0usize;
    let mut boundaries = Vec::new();
    let mut runs = Vec::new();
    walk_reveal(content, &[], &mut recon, &mut offset, &mut boundaries, &mut runs);
    let closes = recon.finish();
    if !closes.is_empty() {
        boundaries.push((offset, closes, Vec::new()));
    }
    RevealModel { boundaries, runs }
}

fn walk_reveal(
    content: &[InlineContent],
    base: &[RevealStyle],
    recon: &mut RevealReconciler,
    offset: &mut usize,
    boundaries: &mut Vec<(usize, Vec<RevealStyle>, Vec<RevealStyle>)>,
    runs: &mut Vec<(usize, usize, Vec<RevealStyle>)>,
) {
    for item in content {
        match item {
            InlineContent::Text(run) => {
                let mut target = base.to_vec();
                target.extend(reveal_styles(&run.style));
                let (closes, opens) = recon.reconcile(&target);
                if !closes.is_empty() || !opens.is_empty() {
                    boundaries.push((*offset, closes, opens));
                }
                let len = run.text.len();
                if len > 0 {
                    runs.push((*offset, *offset + len, recon.stack().to_vec()));
                }
                *offset += len;
            }
            InlineContent::Link { content: inner, .. } => {
                // Open the link as its own scope (closing any prior styles), then
                // recurse so inner runs carry `Link` + their own styles.
                let mut link_base = base.to_vec();
                link_base.push(RevealStyle::Link);
                let (closes, opens) = recon.reconcile(&link_base);
                if !closes.is_empty() || !opens.is_empty() {
                    boundaries.push((*offset, closes, opens));
                }
                walk_reveal(inner, &link_base, recon, offset, boundaries, runs);
            }
            InlineContent::HardBreak => {
                *offset += 1;
            }
        }
    }
}

/// Forward extent `[offset, end)` of the contiguous runs (starting at `offset`)
/// that all carry `style`.
fn extent_forward(
    runs: &[(usize, usize, Vec<RevealStyle>)],
    offset: usize,
    style: RevealStyle,
) -> (usize, usize) {
    let mut end = offset;
    for (start, run_end, styles) in runs {
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
    runs: &[(usize, usize, Vec<RevealStyle>)],
    offset: usize,
    style: RevealStyle,
) -> (usize, usize) {
    let mut start = offset;
    for (run_start, run_end, styles) in runs.iter().rev() {
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
/// cursor stops there. Zero when `offset` is not a style boundary, so the caret
/// steps a plain character.
pub(crate) fn reveal_tag_count_at(content: &[InlineContent], offset: usize) -> usize {
    build_reveal_model(content)
        .boundaries
        .iter()
        .filter(|(o, _, _)| *o == offset)
        .map(|(_, closes, opens)| closes.len() + opens.len())
        .sum()
}

/// Every byte offset that carries reveal tags (a style boundary), in order.
fn tag_boundaries(content: &[InlineContent]) -> Vec<usize> {
    let mut out: Vec<usize> = Vec::new();
    for (offset, closes, opens) in &build_reveal_model(content).boundaries {
        if (!closes.is_empty() || !opens.is_empty()) && out.last() != Some(offset) {
            out.push(*offset);
        }
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

/// Replace each link in `items` with its inner content — used to delete a link
/// by removing its reveal tag (a link is a structural element, not a style flag,
/// so it's unwrapped rather than cleared).
pub(crate) fn unwrap_links(items: Vec<InlineContent>) -> Vec<InlineContent> {
    let mut out = Vec::new();
    for item in items {
        match item {
            InlineContent::Link { content, .. } => out.extend(content),
            other => out.push(other),
        }
    }
    out
}

/// The reveal tag the caret sits *immediately beside*, given its reveal-stop, plus
/// the `[start, end)` byte range of the span the tag represents.
///
/// The tags rendered at `offset` are, in print order, the closing tags followed by
/// the opening tags (concatenated across every boundary at that offset — a link's
/// `[Link>` and its inner `[Bold>` share the link-start offset). The caret at
/// reveal-stop `s` sits just after the `s`-th of them. `backward` (backspace)
/// targets the tag just left — the `s`-th tag, so a stop of 0 targets nothing;
/// `forward` (delete) targets the one just right — the `(s+1)`-th. Returns `None`
/// when there is no such tag.
pub(crate) fn reveal_tag_to_remove(
    content: &[InlineContent],
    offset: usize,
    reveal_stop: usize,
    backward: bool,
) -> Option<(RevealStyle, usize, usize)> {
    let model = build_reveal_model(content);

    let mut tags: Vec<(RevealStyle, bool)> = Vec::new();
    for (o, closes, opens) in &model.boundaries {
        if *o == offset {
            tags.extend(closes.iter().map(|s| (*s, false)));
            tags.extend(opens.iter().map(|s| (*s, true)));
        }
    }

    let idx = if backward {
        reveal_stop.checked_sub(1)?
    } else {
        reveal_stop
    };
    let (style, is_open) = *tags.get(idx)?;

    let (start, end) = if is_open {
        extent_forward(&model.runs, offset, style)
    } else {
        extent_backward(&model.runs, offset, style)
    };
    Some((style, start, end))
}
