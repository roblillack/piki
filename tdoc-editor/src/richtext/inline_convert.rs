// Lossless conversion between a leaf's inline content and tdoc spans.
//
// tdoc represents inline content as a tree of `Span`s where each node carries a single
// `InlineStyle` and styling combinations are expressed by nesting. The editor/display
// work with a flat `Vec<InlineContent>` of styled `TextRun`s (a style flag-set per run),
// which is convenient for cursor math, wrapping, and hit-testing.
//
// `spans_to_inline` flattens the span tree (accumulating styles down each branch) and
// `inline_to_spans` rebuilds it in a canonical nesting order. The two are inverses up to
// that normalization, so an edit→serialize cycle is stable. Line breaks are carried as a
// literal `"\n"` in span text (matching tdoc) and as `InlineContent::HardBreak` in runs.

use super::structured_document::{InlineContent, Link, TextRun, TextStyle};
use tdoc::inline::{InlineStyle, Span};

/// Flatten a tdoc span tree into the editor's flat inline representation.
pub(crate) fn spans_to_inline(spans: &[Span]) -> Vec<InlineContent> {
    let mut content = Vec::new();
    for span in spans {
        span_to_inline_internal(span, TextStyle::plain(), &mut content);
    }
    content
}

/// Rebuild a tdoc span tree from the editor's flat inline representation.
pub(crate) fn inline_to_spans(content: &[InlineContent]) -> Vec<Span> {
    let mut spans = Vec::new();
    for item in content {
        match item {
            InlineContent::Text(run) => {
                spans.extend(text_run_to_spans(run));
            }
            InlineContent::Link { link, content } => {
                let mut span =
                    Span::new_styled(InlineStyle::Link).with_children(inline_to_spans(content));
                if !link.destination.is_empty() {
                    span = span.with_link_target(link.destination.clone());
                }
                spans.push(span);
            }
            InlineContent::HardBreak => {
                spans.push(Span::new_text("\n"));
            }
        }
    }
    spans
}

fn text_run_to_spans(run: &TextRun) -> Vec<Span> {
    if run.style.code {
        return vec![Span::new_styled(InlineStyle::Code).with_text(&run.text)];
    }

    let mut spans = Vec::new();
    let mut buffer = String::new();

    for ch in run.text.chars() {
        if ch == '\n' {
            if !buffer.is_empty() {
                spans.push(apply_style_to_text(&buffer, run.style));
                buffer.clear();
            }
            spans.push(Span::new_text("\n"));
        } else {
            buffer.push(ch);
        }
    }

    if !buffer.is_empty() {
        spans.push(apply_style_to_text(&buffer, run.style));
    }

    spans
}

fn apply_style_to_text(text: &str, style: TextStyle) -> Span {
    if style.code {
        return Span::new_styled(InlineStyle::Code).with_text(text);
    }

    let mut span = Span::new_text(text);

    if style.italic {
        span = Span::new_styled(InlineStyle::Italic).with_children(vec![span]);
    }
    if style.bold {
        span = Span::new_styled(InlineStyle::Bold).with_children(vec![span]);
    }
    if style.strikethrough {
        span = Span::new_styled(InlineStyle::Strike).with_children(vec![span]);
    }
    if style.underline {
        span = Span::new_styled(InlineStyle::Underline).with_children(vec![span]);
    }
    if style.highlight {
        span = Span::new_styled(InlineStyle::Highlight).with_children(vec![span]);
    }

    span
}

fn span_to_inline_internal(span: &Span, active: TextStyle, out: &mut Vec<InlineContent>) {
    let mut style = active;
    match span.style {
        InlineStyle::Bold => style.bold = true,
        InlineStyle::Italic => style.italic = true,
        InlineStyle::Underline => style.underline = true,
        InlineStyle::Strike => style.strikethrough = true,
        InlineStyle::Highlight => style.highlight = true,
        InlineStyle::Code => style.code = true,
        InlineStyle::Link | InlineStyle::None => {}
    }

    if span.style == InlineStyle::Link {
        let mut inner = Vec::new();
        if !span.text.is_empty() {
            push_text(&mut inner, &span.text, style);
        }
        for child in &span.children {
            span_to_inline_internal(child, style, &mut inner);
        }
        // A link nested inside a link is degenerate — markdown's
        // `[![alt](img)](url)` (an image inside a link) parses to exactly this,
        // since tdoc has no image type. Flatten the inner link(s) so the outer
        // link wraps plain styled runs: the content stays editable/stylable and
        // renders (and reveals) as a single link instead of an opaque, unstylable
        // blob. The outer link's destination (the clickable target) is kept.
        let inner = flatten_nested_links(inner);
        let link = Link {
            destination: span.link_target.clone().unwrap_or_default(),
            title: None,
        };
        out.push(InlineContent::Link {
            link,
            content: inner,
        });
        return;
    }

    if !span.text.is_empty() {
        push_text(out, &span.text, style);
    }
    for child in &span.children {
        span_to_inline_internal(child, style, out);
    }
}

/// Replace any link nested within `items` with its content (recursively), so the
/// flat inline model never contains a link inside a link.
fn flatten_nested_links(items: Vec<InlineContent>) -> Vec<InlineContent> {
    let mut out = Vec::new();
    for item in items {
        match item {
            InlineContent::Link { content, .. } => out.extend(flatten_nested_links(content)),
            other => out.push(other),
        }
    }
    out
}

fn push_text(out: &mut Vec<InlineContent>, text: &str, style: TextStyle) {
    if text.is_empty() {
        return;
    }

    if style.code {
        append_text_run(out, text.to_string(), style);
        return;
    }

    let mut start = 0;
    let chars: Vec<char> = text.chars().collect();
    for (idx, ch) in chars.iter().enumerate() {
        if *ch == '\n' {
            if idx > start {
                let segment: String = chars[start..idx].iter().collect();
                append_text_run(out, segment, style);
            }
            out.push(InlineContent::HardBreak);
            start = idx + 1;
        }
    }

    if start < chars.len() {
        let segment: String = chars[start..].iter().collect();
        append_text_run(out, segment, style);
    }
}

fn append_text_run(out: &mut Vec<InlineContent>, text: String, style: TextStyle) {
    if text.is_empty() {
        return;
    }

    if let Some(InlineContent::Text(run)) = out.last_mut()
        && run.style == style
    {
        run.text.push_str(&text);
        return;
    }

    out.push(InlineContent::Text(TextRun::new(text, style)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bold_italic() -> TextStyle {
        TextStyle {
            bold: true,
            italic: true,
            ..Default::default()
        }
    }

    #[test]
    fn combined_style_round_trips() {
        let content = vec![InlineContent::Text(TextRun::new("hi", bold_italic()))];
        let spans = inline_to_spans(&content);
        let back = spans_to_inline(&spans);
        assert_eq!(back, content);
    }

    #[test]
    fn styled_link_round_trips() {
        let content = vec![InlineContent::Link {
            link: Link {
                destination: "https://example.test".to_string(),
                title: None,
            },
            content: vec![InlineContent::Text(TextRun::new(
                "click",
                TextStyle::bold(),
            ))],
        }];
        let spans = inline_to_spans(&content);
        let back = spans_to_inline(&spans);
        assert_eq!(back, content);
    }

    #[test]
    fn hardbreak_round_trips() {
        let content = vec![
            InlineContent::Text(TextRun::plain("a")),
            InlineContent::HardBreak,
            InlineContent::Text(TextRun::plain("b")),
        ];
        let spans = inline_to_spans(&content);
        let back = spans_to_inline(&spans);
        assert_eq!(back, content);
    }
}
