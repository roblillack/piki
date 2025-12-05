use std::io::Cursor;

use tdoc::{Document, html, markdown};

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
        return result;
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
            Ok(doc) => return Ok(doc),
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

fn document_from_plaintext(text: &str) -> Result<Document, ClipboardDocumentError> {
    if text.trim().is_empty() {
        return Err(ClipboardDocumentError::Empty);
    }

    markdown::parse(Cursor::new(text.as_bytes()))
        .map_err(|err| ClipboardDocumentError::Parse(err.to_string()))
}

fn document_from_html(html_content: &str) -> Result<Document, ClipboardDocumentError> {
    if html_content.trim().is_empty() {
        return Err(ClipboardDocumentError::Empty);
    }

    html::parse(Cursor::new(html_content.as_bytes()))
        .map_err(|err| ClipboardDocumentError::Parse(err.to_string()))
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
