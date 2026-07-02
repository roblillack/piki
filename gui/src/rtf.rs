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

    let text = repair_cp1252_c1(text);
    if text.is_empty() {
        return false;
    }

    spans.push(apply_styles(Span::new_text(text), painter));
    true
}

/// Repair mis-decoded Windows-1252 C1 characters (`U+0080`–`U+009F`) in text
/// produced by `rtf-parser`.
///
/// RTF encodes non-ASCII bytes as `\'xx` hex escapes in the document's codepage
/// (almost always Windows-1252 on macOS/Windows). `rtf-parser` decodes them with
/// `byte as char`, i.e. it treats the codepage byte as a Unicode scalar. For
/// bytes `0xA0`–`0xFF` that happens to match Latin-1/Unicode, but the C1 block
/// `0x80`–`0x9F` does not: `\'92` (a curly apostrophe in Windows-1252) becomes
/// the control character `U+0092` instead of `U+2019`, `\'85` becomes `U+0085`
/// instead of `…`, and so on — these render as inert boxes rather than real
/// glyphs. Because `\'xx` escapes *are* codepage bytes, remapping this block is
/// the correct decoding the parser skipped, not a heuristic. Codepoints the
/// caller already decoded correctly (everything outside the C1 block) pass
/// through untouched, and the borrow is only cloned when a repair is needed.
fn repair_cp1252_c1(text: &str) -> Cow<'_, str> {
    if !text.chars().any(|ch| ('\u{80}'..='\u{9F}').contains(&ch)) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(text.chars().filter_map(cp1252_c1_char).collect())
}

/// Map a single character through the Windows-1252 C1 remap. Characters outside
/// the C1 block are returned unchanged; the five codepoints Windows-1252 leaves
/// undefined (`0x81`, `0x8D`, `0x8F`, `0x90`, `0x9D`) are dropped.
fn cp1252_c1_char(ch: char) -> Option<char> {
    Some(match ch {
        '\u{80}' => '\u{20AC}', // €
        '\u{82}' => '\u{201A}', // ‚
        '\u{83}' => '\u{0192}', // ƒ
        '\u{84}' => '\u{201E}', // „
        '\u{85}' => '\u{2026}', // …
        '\u{86}' => '\u{2020}', // †
        '\u{87}' => '\u{2021}', // ‡
        '\u{88}' => '\u{02C6}', // ˆ
        '\u{89}' => '\u{2030}', // ‰
        '\u{8A}' => '\u{0160}', // Š
        '\u{8B}' => '\u{2039}', // ‹
        '\u{8C}' => '\u{0152}', // Œ
        '\u{8E}' => '\u{017D}', // Ž
        '\u{91}' => '\u{2018}', // '
        '\u{92}' => '\u{2019}', // '
        '\u{93}' => '\u{201C}', // "
        '\u{94}' => '\u{201D}', // "
        '\u{95}' => '\u{2022}', // •
        '\u{96}' => '\u{2013}', // –
        '\u{97}' => '\u{2014}', // —
        '\u{98}' => '\u{02DC}', // ˜
        '\u{99}' => '\u{2122}', // ™
        '\u{9A}' => '\u{0161}', // š
        '\u{9B}' => '\u{203A}', // ›
        '\u{9C}' => '\u{0153}', // œ
        '\u{9E}' => '\u{017E}', // ž
        '\u{9F}' => '\u{0178}', // Ÿ
        '\u{81}' | '\u{8D}' | '\u{8F}' | '\u{90}' | '\u{9D}' => return None,
        other => other,
    })
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
            && next.is_ascii_alphabetic()
        {
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

        let out = output.get_or_insert_with(|| String::with_capacity(input.len() + 8));
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
    fn repairs_windows_1252_c1_punctuation() {
        // In Windows-1252: \'92 = curly apostrophe, \'85 = ellipsis,
        // \'97 = em dash. rtf-parser decodes these to the raw C1 control
        // codepoints; parse_rtf_document must map them back to real glyphs.
        let rtf = br#"{\rtf1\ansi\ansicpg1252 It\'92s done\'85 or\'97maybe.\par}"#;
        let document = parse_rtf_document(rtf).unwrap();

        let Paragraph::Text { content } = &document.paragraphs[0] else {
            panic!("expected text paragraph");
        };
        let combined: String = content.iter().map(|span| span.text.as_str()).collect();
        assert_eq!(combined, "It\u{2019}s done\u{2026} or\u{2014}maybe.");
        assert!(
            !combined
                .chars()
                .any(|ch| ('\u{80}'..='\u{9F}').contains(&ch)),
            "no C1 control characters should survive"
        );
    }

    #[test]
    fn injects_sentinels_after_par_control() {
        let raw = r"{\rtf1 Foo\par Bar\par
}";
        let normalized = super::inject_paragraph_sentinels(raw);

        assert_eq!(normalized, "{\\rtf1 Foo\\par \\'1eBar\\par\\'1e\n}");
    }
}
