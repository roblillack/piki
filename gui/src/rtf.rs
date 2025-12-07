use std::borrow::Cow;

use rtf_parser::paragraph::Paragraph as RtfParagraph;
use rtf_parser::{Painter, RtfDocument};
use tdoc::{Document, InlineStyle, Paragraph, Span};

const PARAGRAPH_BREAK_SENTINEL: char = '\u{001E}';
const PARAGRAPH_BREAK_ESCAPE: &str = "\\'1e";

#[derive(Debug)]
pub enum RtfImportError {
    Decode,
    Parse(String),
}

impl std::fmt::Display for RtfImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtfImportError::Decode => write!(f, "RTF data was not valid UTF-8"),
            RtfImportError::Parse(msg) => write!(f, "RTF parse error: {msg}"),
        }
    }
}

impl std::error::Error for RtfImportError {}

/// Convert raw RTF bytes into a [`tdoc::Document`].
/// Styling support currently covers bold, italic, underline, and strikethrough runs.
pub fn parse_rtf_document(bytes: &[u8]) -> Result<Document, RtfImportError> {
    let cow = String::from_utf8_lossy(bytes);
    let normalized = inject_paragraph_sentinels(cow.as_ref());
    // rtf-parser expects a &str; conversions happen internally.
    let rtf = RtfDocument::try_from(normalized.as_ref())
        .map_err(|err| RtfImportError::Parse(err.to_string()))?;

    let mut paragraphs = Vec::new();
    let mut current_spans: Vec<Span> = Vec::new();
    let mut last_paragraph_state: Option<RtfParagraph> = None;

    for block in rtf.body.iter() {
        if last_paragraph_state
            .as_ref()
            .map(|prev| prev != &block.paragraph)
            .unwrap_or(false)
            && !current_spans.is_empty()
        {
            finalize_paragraph(&mut current_spans, &mut paragraphs, false);
        }

        let appended = append_style_block(
            block.text.as_str(),
            &block.painter,
            &mut current_spans,
            &mut paragraphs,
        );

        if appended {
            last_paragraph_state = Some(block.paragraph);
        }
    }

    finalize_paragraph(&mut current_spans, &mut paragraphs, false);

    Ok(Document::new().with_paragraphs(paragraphs))
}

fn append_style_block(
    text: &str,
    painter: &Painter,
    current_spans: &mut Vec<Span>,
    paragraphs: &mut Vec<Paragraph>,
) -> bool {
    let normalized = if text.contains(PARAGRAPH_BREAK_SENTINEL) {
        Cow::Owned(text.replace(PARAGRAPH_BREAK_SENTINEL, "\n"))
    } else {
        Cow::Borrowed(text)
    };
    let text = normalized.as_ref();
    let mut start = 0usize;
    let mut appended = false;
    for (idx, ch) in text.char_indices() {
        match ch {
            '\r' => {
                if start < idx {
                    appended |= push_span(&text[start..idx], painter, current_spans);
                }
                start = idx + ch.len_utf8();
            }
            '\n' => {
                if start < idx {
                    appended |= push_span(&text[start..idx], painter, current_spans);
                }
                finalize_paragraph(current_spans, paragraphs, true);
                start = idx + ch.len_utf8();
            }
            _ => {}
        }
    }

    if start < text.len() {
        appended |= push_span(&text[start..], painter, current_spans);
    }

    appended
}

fn push_span(text: &str, painter: &Painter, spans: &mut Vec<Span>) -> bool {
    if text.is_empty() {
        return false;
    }

    spans.push(apply_styles(Span::new_text(text), painter));
    true
}

fn apply_styles(mut span: Span, painter: &Painter) -> Span {
    let wrap = |style: InlineStyle, enabled: bool, span: Span| -> Span {
        if !enabled {
            span
        } else {
            Span::new_styled(style).with_children(vec![span])
        }
    };

    span = wrap(InlineStyle::Strike, painter.strike, span);
    span = wrap(InlineStyle::Underline, painter.underline, span);
    span = wrap(InlineStyle::Italic, painter.italic, span);
    span = wrap(InlineStyle::Bold, painter.bold, span);
    span
}

fn finalize_paragraph(
    current_spans: &mut Vec<Span>,
    paragraphs: &mut Vec<Paragraph>,
    allow_empty: bool,
) {
    if current_spans.is_empty() {
        if allow_empty {
            paragraphs.push(Paragraph::new_text());
        }
        return;
    }

    let spans = std::mem::take(current_spans);
    paragraphs.push(Paragraph::new_text().with_content(spans));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_rtf() {
        let rtf = br#"{\rtf1\ansi{\fonttbl{\f0 Arial;}}\f0\pard Simple {\b bold} and {\i italic}.\par Next line.}"#;
        let document = parse_rtf_document(rtf).unwrap();
        assert_eq!(document.paragraphs.len(), 2);

        let first = &document.paragraphs[0];
        if let Paragraph::Text { content } = first {
            assert_eq!(content.len(), 5);
            assert_eq!(content[0].text, "Simple ");
            assert_eq!(content[1].children[0].text, "bold");
            assert_eq!(content[2].text, " and ");
            assert_eq!(content[3].children[0].text, "italic");
            assert_eq!(content[4].text, ".");
        } else {
            panic!("expected text paragraph");
        }

        let second = &document.paragraphs[1];
        if let Paragraph::Text { content } = second {
            assert_eq!(content[0].text, "Next line.");
        }
    }

    #[test]
    fn injects_sentinels_after_par_control() {
        let raw = r"{\rtf1 Foo\par Bar\par
}";
        let normalized = super::inject_paragraph_sentinels(raw);

        assert_eq!(normalized, "{\\rtf1 Foo\\par \\'1eBar\\par\\'1e\n}");
    }
}

fn inject_paragraph_sentinels(input: &str) -> Cow<'_, str> {
    const NEEDLE: &[u8] = b"par";
    let bytes = input.as_bytes();
    let mut i = 0;
    let mut last_copied = 0;
    let mut output: Option<String> = None;

    while i + NEEDLE.len() < bytes.len() {
        if bytes[i] != b'\\' {
            i += 1;
            continue;
        }
        let slice = &bytes[i + 1..];
        if slice.len() < NEEDLE.len() {
            break;
        }
        if slice[..NEEDLE.len()]
            .iter()
            .map(|b| b.to_ascii_lowercase())
            .ne(NEEDLE.iter().copied())
        {
            i += 1;
            continue;
        }
        let after_word = i + 1 + NEEDLE.len();
        if let Some(next) = bytes.get(after_word)
            && next.is_ascii_alphabetic() {
                // Skip \pard, \parshape, etc.
                i += 1;
                continue;
            }
        let mut after_space = after_word;
        if bytes.get(after_space) == Some(&b' ') {
            after_space += 1;
        }
        let sentinel_len = PARAGRAPH_BREAK_ESCAPE.len();
        let already_tagged = bytes
            .get(after_space..after_space + sentinel_len)
            .map(|segment| segment == PARAGRAPH_BREAK_ESCAPE.as_bytes())
            .unwrap_or(false);
        if already_tagged {
            i = after_space;
            continue;
        }

        let out = output
            .get_or_insert_with(|| String::with_capacity(input.len() + 8));
        out.push_str(&input[last_copied..after_space]);
        out.push_str(PARAGRAPH_BREAK_ESCAPE);
        last_copied = after_space;
        i = after_space;
    }

    if let Some(mut out) = output {
        out.push_str(&input[last_copied..]);
        Cow::Owned(out)
    } else {
        Cow::Borrowed(input)
    }
}
