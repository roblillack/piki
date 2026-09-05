use std::io::Cursor;

use tdoc::paragraph::Paragraph;
use tdoc::{Document, Span, html, markdown};

use crate::markdown_converter::{document_to_html, document_to_markdown};
use crate::rtf;

#[derive(Debug)]
pub enum ClipboardDocumentError {
    Empty,
    ClipboardUnavailable(String),
    Parse(String),
}

/// Read the system clipboard and convert it into a `tdoc::Document`.
/// Accepts an optional plain-text fallback (typically provided by FLTK on platforms
/// where arboard isn't available) along with additional format notes supplied by the caller.
pub fn read_document_from_system(
    fallback_plain: Option<&str>,
    platform_formats: &[String],
    platform_rtf: Option<&[u8]>,
) -> Result<Document, ClipboardDocumentError> {
    let mut diagnostics = platform_formats.to_vec();

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        let result = match read_with_arboard(&mut diagnostics, platform_rtf) {
            Ok(doc) => Ok(doc),
            Err(err) => {
                if let Some(text) = fallback_plain {
                    diagnostics.push(format!(
                        "fallback:text/plain ({} bytes from FLTK)",
                        text.len()
                    ));
                    match document_from_plaintext(text) {
                        Ok(doc) => Ok(doc),
                        Err(parse_err) => Err(parse_err),
                    }
                } else {
                    Err(err)
                }
            }
        };
        log_formats(&diagnostics);
        result
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let result = fallback_plain
            .map(|text| {
                diagnostics.push(format!(
                    "fallback:text/plain ({} bytes from FLTK)",
                    text.len()
                ));
                document_from_plaintext(text)
            })
            .unwrap_or(Err(ClipboardDocumentError::Empty));
        log_formats(&diagnostics);
        return result;
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn read_with_arboard(
    diagnostics: &mut Vec<String>,
    platform_rtf: Option<&[u8]>,
) -> Result<Document, ClipboardDocumentError> {
    use arboard::Clipboard;

    let mut clipboard = Clipboard::new()
        .map_err(|err| ClipboardDocumentError::ClipboardUnavailable(err.to_string()))?;

    match clipboard.get().html() {
        Ok(html) if !html.trim().is_empty() => {
            diagnostics.push(format!("arboard:text/html ({} bytes)", html.len()));
            if let Ok(doc) = document_from_html(&html) {
                return Ok(doc);
            } else {
                diagnostics.push("arboard:text/html parse failed".to_string());
            }
        }
        Ok(_) => {
            diagnostics.push("arboard:text/html (empty payload)".to_string());
        }
        Err(arboard::Error::ContentNotAvailable) => {
            diagnostics.push("arboard:text/html unavailable".to_string());
        }
        Err(err) => {
            diagnostics.push(format!("arboard:text/html error ({err})"));
        }
    }

    if let Some(rtf_bytes) = platform_rtf {
        diagnostics.push(format!("platform:public.rtf ({} bytes)", rtf_bytes.len()));
        match rtf::parse_rtf_document(rtf_bytes) {
            Ok(doc) => return Ok(paragraphs_per_line(doc)),
            Err(err) => diagnostics.push(format!("platform:public.rtf parse failed ({err})")),
        }
    }

    let text = clipboard.get_text().map_err(|err| match err {
        arboard::Error::ContentNotAvailable => ClipboardDocumentError::Empty,
        other => ClipboardDocumentError::ClipboardUnavailable(other.to_string()),
    })?;

    diagnostics.push(format!("arboard:text/plain ({} bytes)", text.len()));

    document_from_plaintext(&text)
}

/// Turn pasted plain text into a document.
///
/// Text on the clipboard is read one of two ways, and the difference is what a newline means.
/// Markdown source treats a single newline inside a paragraph as a soft wrap and joins the lines;
/// literal text has no soft wrapping at all — a newline is where the author ended the line — so
/// every line becomes a paragraph of its own, blank lines included, and each one can be styled,
/// bulleted, quoted or moved by itself.
///
/// [`looks_like_markdown`] picks between them, and it has to be right in the literal direction:
/// misreading a letter copied out of a PDF as Markdown collapses it into a couple of huge blocks.
/// Either way, explicit hard breaks end up as paragraph breaks (see [`paragraphs_per_line`]).
fn document_from_plaintext(text: &str) -> Result<Document, ClipboardDocumentError> {
    if text.trim().is_empty() {
        return Err(ClipboardDocumentError::Empty);
    }

    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    // A trailing newline ends the last line rather than starting an empty one.
    let normalized = normalized.strip_suffix('\n').unwrap_or(&normalized);

    if looks_like_markdown(normalized) {
        return markdown::parse(Cursor::new(normalized.as_bytes()))
            .map(paragraphs_per_line)
            .map_err(|err| ClipboardDocumentError::Parse(err.to_string()));
    }

    let paragraphs = normalized
        .split('\n')
        .map(|line| Paragraph::new_text().with_content(vec![Span::new_text(line)]))
        .collect();
    Ok(Document::new().with_paragraphs(paragraphs))
}

/// Whether pasted text should be read as Markdown *source* rather than as literal lines.
///
/// Prose picked up from a document, a mail or a PDF is full of things that look vaguely like
/// Markdown — a dash starting a line, an asterisk, an underscore inside an identifier — so a
/// single hint is never enough: reading prose as Markdown joins its lines and destroys the
/// paragraph structure the user pasted. Evidence comes in two grades:
///
/// * syntax that essentially cannot occur by accident — a fenced code block, a table delimiter
///   row, a checklist marker, an inline link, a link reference, an ATX heading;
/// * otherwise a *pattern* of block markers: text made mostly of list/quote lines (a pasted list
///   or outline), or several different Markdown constructs used together.
///
/// A lone `- …` line in a page of prose therefore stays literal, while `- a`/`- b`/`- c` becomes
/// a real bullet list.
fn looks_like_markdown(text: &str) -> bool {
    let lines: Vec<&str> = text.lines().collect();

    let mut fences = 0usize;
    let mut pipe_rows = 0usize;
    let mut delimiter_row = false;
    for raw in &lines {
        let line = raw.trim();
        if line.starts_with("```") || line.starts_with("~~~") {
            fences += 1;
        }
        if line.contains('|') {
            pipe_rows += 1;
            delimiter_row |= is_table_delimiter_row(line);
        }
        if is_atx_heading(line)
            || is_task_item(line)
            || is_link_reference(line)
            || contains_inline_link(line)
        {
            return true;
        }
    }
    // A pair of fences is a code block; a delimiter row plus a header row is a table.
    if fences >= 2 || (delimiter_row && pipe_rows >= 2) {
        return true;
    }

    let mut bullets = 0usize;
    let mut ordered = 0usize;
    let mut quotes = 0usize;
    let mut inline = 0usize;
    for raw in &lines {
        let line = raw.trim_start();
        if is_bullet_item(line) {
            bullets += 1;
        } else if is_ordered_item(line) {
            ordered += 1;
        } else if line.starts_with('>') {
            quotes += 1;
        }
        inline += paired_inline_markers(line);
    }
    let non_blank = lines.iter().filter(|l| !l.trim().is_empty()).count();
    let marker_lines = bullets + ordered + quotes;
    let kinds = [bullets > 0, ordered > 0, quotes > 0, inline > 0]
        .iter()
        .filter(|present| **present)
        .count();

    // Nothing can be glued together in a single line, so inline markup alone settles it there.
    if !text.contains('\n') && inline > 0 {
        return true;
    }
    // Mostly block markers: a pasted list, outline or quoted mail.
    if marker_lines >= 2 && marker_lines * 2 >= non_blank {
        return true;
    }
    // Or several different constructs at once — one stray marker is not enough.
    kinds >= 2 && marker_lines + inline >= 3
}

/// `# ` … `###### ` with text after it.
fn is_atx_heading(line: &str) -> bool {
    let hashes = line.chars().take_while(|c| *c == '#').count();
    (1..=6).contains(&hashes)
        && line[hashes..].starts_with(' ')
        && !line[hashes..].trim().is_empty()
}

/// A table's delimiter row: only pipes, dashes, colons and spaces, e.g. `|---|:--:|`.
fn is_table_delimiter_row(line: &str) -> bool {
    line.contains('-')
        && line.contains('|')
        && line.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '))
}

/// The text after a `-`/`*`/`+` bullet marker, if the line starts with one.
fn bullet_body(line: &str) -> Option<&str> {
    ["- ", "* ", "+ "]
        .iter()
        .find_map(|marker| line.strip_prefix(marker))
}

fn is_bullet_item(line: &str) -> bool {
    bullet_body(line).is_some_and(|body| !body.trim().is_empty())
}

/// `- [ ] ` / `- [x] `, the checklist marker.
fn is_task_item(line: &str) -> bool {
    bullet_body(line).map(str::trim_start).is_some_and(|body| {
        body.starts_with("[ ]") || body.starts_with("[x]") || body.starts_with("[X]")
    })
}

/// `1. ` or `1) ` with text after it.
fn is_ordered_item(line: &str) -> bool {
    let digits = line.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return false;
    }
    let rest = &line[digits..];
    (rest.starts_with(". ") || rest.starts_with(") ")) && !rest[2..].trim().is_empty()
}

/// A link reference definition: `[id]: https://…`.
fn is_link_reference(line: &str) -> bool {
    line.strip_prefix('[')
        .and_then(|rest| rest.split_once("]: "))
        .is_some_and(|(id, target)| !id.is_empty() && !target.trim().is_empty())
}

/// An inline link or image, `[text](target)` — `](` is vanishingly rare in prose.
fn contains_inline_link(line: &str) -> bool {
    line.split_once("](").is_some_and(|(before, after)| {
        before.contains('[')
            && after.split_once(')').is_some_and(|(target, _)| {
                let target = target.trim();
                !target.is_empty() && !target.contains(' ')
            })
    })
}

/// Paired strong inline markers on one line: `**bold**`, `` `code` ``, `~~struck~~`. Single `*`
/// and `_` are left out on purpose — footnote asterisks and `snake_case` names are not Markdown.
fn paired_inline_markers(line: &str) -> usize {
    ["**", "`", "~~"]
        .iter()
        .map(|marker| line.matches(marker).count() / 2)
        .sum()
}

fn document_from_html(html_content: &str) -> Result<Document, ClipboardDocumentError> {
    if html_content.trim().is_empty() {
        return Err(ClipboardDocumentError::Empty);
    }

    html::parse(Cursor::new(html_content.as_bytes()))
        .map(paragraphs_per_line)
        .map_err(|err| ClipboardDocumentError::Parse(err.to_string()))
}

/// Split every hard line break in a pasted document into a paragraph break.
///
/// Rich clipboard payloads routinely put line breaks *inside* a paragraph — `<br>` in HTML is the
/// common one — and piki's block operations (list/quote conversion, block styles, moving blocks)
/// all act on a whole paragraph. A pasted letter that arrives as one 60-line paragraph is
/// therefore a single indivisible block: selecting "some paragraphs" and converting them yields
/// one item for the whole thing. Splitting on paste makes every pasted line a real paragraph,
/// matching what the plain-text path and typed input produce.
///
/// Code blocks keep their newlines (there they are content, not layout), and so do checklist
/// items, which can hold spans only.
fn paragraphs_per_line(mut doc: Document) -> Document {
    doc.paragraphs = std::mem::take(&mut doc.paragraphs)
        .into_iter()
        .flat_map(split_paragraph_at_breaks)
        .collect();
    doc
}

/// One paragraph in, one or more out: split at the newlines its spans carry.
fn split_paragraph_at_breaks(paragraph: Paragraph) -> Vec<Paragraph> {
    // A break inside a list item splits it into that item's continuation paragraphs, so the item
    // keeps its bullet and the following lines stay with it.
    fn split_entries(entries: Vec<Vec<Paragraph>>) -> Vec<Vec<Paragraph>> {
        entries
            .into_iter()
            .map(|entry| {
                entry
                    .into_iter()
                    .flat_map(split_paragraph_at_breaks)
                    .collect()
            })
            .collect()
    }

    match paragraph {
        // Newlines are content here (code), or unrepresentable as a split (a table's cells and a
        // checklist's span-only items).
        Paragraph::CodeBlock { .. } | Paragraph::Table { .. } | Paragraph::Checklist { .. } => {
            vec![paragraph]
        }
        Paragraph::Quote { children } => vec![
            Paragraph::new_quote().with_children(
                children
                    .into_iter()
                    .flat_map(split_paragraph_at_breaks)
                    .collect(),
            ),
        ],
        Paragraph::OrderedList { entries } => vec![Paragraph::OrderedList {
            entries: split_entries(entries),
        }],
        Paragraph::UnorderedList { entries } => vec![Paragraph::UnorderedList {
            entries: split_entries(entries),
        }],
        leaf => {
            let lines = split_spans_at_newlines(leaf.content());
            if lines.len() <= 1 {
                return vec![leaf];
            }
            let kind = leaf.paragraph_type();
            lines
                .into_iter()
                .map(|spans| Paragraph::new(kind).with_content(spans))
                .collect()
        }
    }
}

/// Split a span list at every newline: one span list per line, empty for a blank line. Styled
/// wrappers are re-created around each piece so a break inside bold text keeps both halves bold.
fn split_spans_at_newlines(spans: &[Span]) -> Vec<Vec<Span>> {
    let mut lines: Vec<Vec<Span>> = vec![Vec::new()];
    for span in spans {
        let pieces: Vec<Vec<Span>> = if span.children.is_empty() {
            span.text
                .split('\n')
                .map(|piece| {
                    if piece.is_empty() {
                        Vec::new()
                    } else {
                        let mut clone = span.clone();
                        clone.text = piece.to_string();
                        vec![clone]
                    }
                })
                .collect()
        } else {
            split_spans_at_newlines(&span.children)
                .into_iter()
                .map(|children| {
                    if children.is_empty() {
                        Vec::new()
                    } else {
                        let mut clone = span.clone();
                        clone.children = children;
                        vec![clone]
                    }
                })
                .collect()
        };
        for (i, piece) in pieces.into_iter().enumerate() {
            if i > 0 {
                lines.push(Vec::new());
            }
            if let Some(last) = lines.last_mut() {
                last.extend(piece);
            }
        }
    }
    lines
}

/// Copy plain text (e.g. a section link URL) to the system clipboard.
///
/// Prefers arboard so the text lands on the real system pasteboard, falling back
/// to FLTK's clipboard when arboard is unavailable.
pub fn copy_text_to_system(text: &str) {
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    {
        use arboard::Clipboard;
        match Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text.to_string())) {
            Ok(()) => return,
            Err(err) => eprintln!("[piki] Failed to copy text to clipboard: {err}"),
        }
    }
    fltk::app::copy(text);
}

/// Copy a structured selection to the system clipboard.
///
/// Places HTML on the clipboard for rich-text-aware targets, with the Markdown
/// serialization as the plain-text alternative so plain-text (and
/// Markdown-aware) targets get a useful representation too. Falls back to a
/// plain-text Markdown copy via FLTK when the system clipboard is unavailable.
pub fn copy_structured_to_system(doc: &Document) {
    let markdown = document_to_markdown(doc);
    let html = document_to_html(doc);
    place_on_clipboard(&markdown, &html);
}

/// Write `html` (with `markdown` as the plain-text alternative) to the system
/// clipboard, falling back to a plain-text copy through FLTK when arboard is
/// unavailable or the HTML payload is empty.
fn place_on_clipboard(markdown: &str, html: &str) {
    let wrote_html = !html.trim().is_empty() && write_html_with_alt(markdown, html);
    if !wrote_html {
        fltk::app::copy(markdown);
    }
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn write_html_with_alt(markdown: &str, html: &str) -> bool {
    use arboard::Clipboard;

    match Clipboard::new().and_then(|mut clipboard| clipboard.set().html(html, Some(markdown))) {
        Ok(()) => true,
        Err(err) => {
            eprintln!("[piki] Failed to copy HTML to clipboard: {err}");
            false
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn write_html_with_alt(_markdown: &str, _html: &str) -> bool {
    false
}

fn log_formats(formats: &[String]) {
    if formats.is_empty() {
        eprintln!("[piki] Clipboard formats during paste: (none detected)");
    } else {
        eprintln!(
            "[piki] Clipboard formats during paste: {}",
            formats.join(", ")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown_converter::document_to_markdown;

    fn span_text(span: &Span) -> String {
        let mut out = span.text.clone();
        for child in &span.children {
            out.push_str(&span_text(child));
        }
        out
    }

    fn texts(doc: &Document) -> Vec<String> {
        doc.paragraphs
            .iter()
            .map(|p| p.content().iter().map(span_text).collect())
            .collect()
    }

    /// The reported case: a letter copied out of a PDF. One stray "- " line must not make the
    /// whole thing Markdown — that would glue its lines back into two blocks.
    const LETTER: &str = "Sehr geehrte Eltern, liebe Schülerinnen und Schüler,\n\num unseren Sportunterricht lehrplangerecht durchführen zu können und die Verletzungsgefahren zu minimieren, möchten wir Ihnen/Euch mitteilen, welche Richtlinien gelten.\n\nSportbekleidung Sicherheitsbedingungen: (vgl. Erlass vom 1. Juli 2010)\n· Die Teilnahme am Sportunterricht ist nur in vollständiger Sportbekleidung möglich.\n· Lange Haare müssen zusammengebunden werden.\n\nSportbefreiung\n· Eine Befreiung ist für eine Woche möglich.\n- Verpasste Sportpraktische Inhalte sind selbstständig nachzuholen.\n\nWertsachen\n· Für Wertsachen wird keine Haftung übernommen.";

    #[test]
    fn letter_from_a_pdf_is_not_markdown() {
        assert!(!looks_like_markdown(LETTER));
        let doc = document_from_plaintext(LETTER).expect("document");
        assert_eq!(doc.paragraphs.len(), LETTER.lines().count());
        assert!(
            doc.paragraphs
                .iter()
                .all(|p| matches!(p, Paragraph::Text { .. })),
            "every line stays a plain paragraph: {:?}",
            doc.paragraphs
        );
    }

    #[test]
    fn markdown_source_is_detected_and_parsed() {
        // Decisive syntax on its own is enough.
        assert!(looks_like_markdown("# Überschrift\n\nEtwas Text."));
        assert!(looks_like_markdown(
            "see [the docs](https://example.test) for more"
        ));
        assert!(looks_like_markdown("Setup\n\n```sh\ncargo build\n```\n"));
        assert!(looks_like_markdown("- [ ] offen\n- [x] erledigt"));
        assert!(looks_like_markdown("| a | b |\n|---|---|\n| 1 | 2 |"));
        assert!(looks_like_markdown("[docs]: https://example.test"));

        let doc = document_from_plaintext("# Titel\n\nEin Absatz, der\nüber zwei Zeilen läuft.")
            .expect("document");
        assert!(matches!(doc.paragraphs[0], Paragraph::Header1 { .. }));
        // Markdown soft wrapping: the two source lines are one paragraph again.
        assert_eq!(doc.paragraphs.len(), 2);
        assert_eq!(texts(&doc)[1], "Ein Absatz, der über zwei Zeilen läuft.");
    }

    #[test]
    fn single_line_with_inline_markup_is_markdown() {
        // One line has no soft-wrap ambiguity: reading it as Markdown cannot join anything, so
        // `**bold**` is worth honouring here even though it would be too weak on its own in a
        // longer text.
        assert!(looks_like_markdown("Hello **World**!"));
        let doc = document_from_plaintext("Hello **World**!").expect("document");
        assert_eq!(texts(&doc), vec!["Hello World!"]);
        assert_eq!(document_to_markdown(&doc), "Hello **World**!\n");
        // …but a lone bullet or a date stays literal: those would change the block, not just the
        // inline styling.
        assert!(!looks_like_markdown("- Milch"));
        assert!(!looks_like_markdown("1. Juli 2010 ist der Stichtag"));
    }

    #[test]
    fn piki_own_markdown_is_detected_so_copy_paste_round_trips() {
        // Copying out of piki puts Markdown on the clipboard as the plain-text alternative. When
        // a target hands us that text back, it has to be recognised as Markdown again.
        let note = "# Titel\n\nEin Absatz.\n\n- eins\n- zwei\n";
        let doc = markdown::parse(Cursor::new(note.as_bytes())).expect("parse");
        let round_tripped = document_to_markdown(&doc);
        assert!(
            looks_like_markdown(&round_tripped),
            "not detected: {round_tripped:?}"
        );
    }

    #[test]
    fn a_pasted_list_becomes_a_list() {
        assert!(looks_like_markdown("- Milch\n- Eier\n- Brot"));
        let doc = document_from_plaintext("- Milch\n- Eier\n- Brot").expect("document");
        assert_eq!(doc.paragraphs.len(), 1);
        assert!(matches!(doc.paragraphs[0], Paragraph::UnorderedList { .. }));
    }

    #[test]
    fn prose_with_incidental_markdown_lookalikes_stays_literal() {
        // A single marker, a date at line start, snake_case names, a footnote asterisk: all of
        // these show up in ordinary text and must not flip the decision.
        assert!(!looks_like_markdown(
            "Ein Absatz.\n- ein einzelner Strich\nNoch ein Absatz."
        ));
        assert!(!looks_like_markdown(
            "Termin\n1. Juli 2010 ist der Stichtag\nDanach nicht mehr."
        ));
        assert!(!looks_like_markdown(
            "list_all_documents() liefert alles.\nDas ist die snake_case Variante."
        ));
        assert!(!looks_like_markdown(
            "Preis 5 * 3 Euro\nZahlung* bis Freitag\n* Fußnote"
        ));
    }

    #[test]
    fn markdown_paste_still_splits_explicit_hard_breaks() {
        // Two trailing spaces are Markdown's hard break; a pasted document full of them is what
        // started this, so they become paragraph breaks here too.
        let doc = document_from_plaintext("# Titel\n\nZeile eins  \nZeile zwei").expect("document");
        assert_eq!(texts(&doc), vec!["Titel", "Zeile eins", "Zeile zwei"]);
    }

    #[test]
    fn plaintext_paste_makes_one_paragraph_per_line() {
        let doc = document_from_plaintext("Sehr geehrte Eltern,\n\nZeile eins\nZeile zwei\n")
            .expect("document");
        assert_eq!(
            texts(&doc),
            vec!["Sehr geehrte Eltern,", "", "Zeile eins", "Zeile zwei"]
        );
    }

    #[test]
    fn plaintext_paste_normalizes_crlf_and_cr() {
        let doc = document_from_plaintext("eins\r\nzwei\rdrei").expect("document");
        assert_eq!(texts(&doc), vec!["eins", "zwei", "drei"]);
    }

    #[test]
    fn plaintext_paste_ignores_one_trailing_newline() {
        // A file-style trailing newline ends the last line; it must not add an empty paragraph.
        let doc = document_from_plaintext("eins\nzwei\n").expect("document");
        assert_eq!(texts(&doc), vec!["eins", "zwei"]);
        // A second one is a real blank line and is kept.
        let doc = document_from_plaintext("eins\nzwei\n\n").expect("document");
        assert_eq!(texts(&doc), vec!["eins", "zwei", ""]);
    }

    #[test]
    fn html_paste_splits_br_into_paragraphs() {
        let doc = document_from_html(
            "<p>Absatz eins<br>Zeile zwei<br><br>Zeile drei</p><p>Absatz zwei</p>",
        )
        .expect("document");
        assert_eq!(
            texts(&doc),
            vec!["Absatz eins", "Zeile zwei", "", "Zeile drei", "Absatz zwei"]
        );
    }

    #[test]
    fn html_paste_keeps_styling_across_a_break() {
        let doc = document_from_html("<p><b>fett eins<br>fett zwei</b></p>").expect("document");
        assert_eq!(
            document_to_markdown(&doc),
            "**fett eins**\n\n**fett zwei**\n"
        );
    }

    #[test]
    fn html_paste_keeps_code_block_newlines() {
        let doc = document_from_html("<pre>zeile eins\nzeile zwei</pre>").expect("document");
        assert_eq!(doc.paragraphs.len(), 1);
        assert!(matches!(doc.paragraphs[0], Paragraph::CodeBlock { .. }));
        assert_eq!(texts(&doc), vec!["zeile eins\nzeile zwei"]);
    }

    #[test]
    fn html_paste_break_in_list_item_becomes_a_continuation_paragraph() {
        let doc = document_from_html("<ul><li>eins<br>fortsetzung</li><li>zwei</li></ul>")
            .expect("document");
        assert_eq!(doc.paragraphs.len(), 1);
        let Paragraph::UnorderedList { entries } = &doc.paragraphs[0] else {
            panic!("expected a bullet list, got {:?}", doc.paragraphs[0]);
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].len(),
            2,
            "item keeps both lines: {:?}",
            entries[0]
        );
    }
}
