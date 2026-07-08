//! Live Note Sharing: a localhost webserver that renders the currently open
//! note as a clean, live-reloading HTML page.
//!
//! The GUI ([`crate::on_air_bar`] + `main.rs`) starts a [`LiveShare`] when the
//! user turns sharing on. The server runs on a background thread and reads a
//! small shared snapshot ([`ShareState`]) that the GUI keeps up to date:
//!
//! * the **currently open note** and its **live Markdown** (including edits that
//!   have not been autosaved yet), and
//! * a monotonically increasing **generation** counter that is bumped on every
//!   change, which the embedded browser script polls to drive live reloading.
//!
//! The browser view is deliberately independent of in-app navigation: it only
//! ever shows the note in its own URL. A request for the *current* note is
//! served from the in-memory Markdown (so unsaved edits show up live); any other
//! note is loaded from disk. This lets a presenter keep a "public" note visible
//! in a shared browser tab while taking notes in a "private" one.
//!
//! Binding is localhost-only (`127.0.0.1`) on an OS-assigned ephemeral port: the
//! server is only reachable from the presenter's machine, so remote meeting
//! participants only ever see the screen-shared tab, never the server itself.

use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, UNIX_EPOCH};

use tiny_http::{Header, Request, Response, Server};

use crate::link_handler::is_external_link;
use crate::markdown_converter::{document_to_html, markdown_to_document};
use crate::section_link::{heading_anchors, normalize_link_target, split_target};
use piki_core::ensure_md_extension;
use tdoc::{ChecklistItem, Document, InlineStyle, Paragraph, Span};

/// How long the serve loop blocks waiting for a request before re-checking the
/// shutdown flag. Keeps [`LiveShare::stop`] responsive without busy-looping.
const POLL_TIMEOUT: Duration = Duration::from_millis(250);

/// Snapshot of what the server should serve, kept up to date by the GUI thread.
struct ShareState {
    /// Notes directory, used to load any non-current note from disk.
    dir: PathBuf,
    /// The note currently open in the GUI.
    current_note: String,
    /// Live Markdown of the current note (includes not-yet-saved edits).
    current_markdown: String,
    /// Bumped whenever the current note or its Markdown changes. Drives the
    /// browser's live reload.
    generation: u64,
}

/// A running Live Note Sharing session. Owns the server thread; dropping (or
/// [`stop`](LiveShare::stop)) shuts it down and joins the thread.
pub struct LiveShare {
    state: Arc<Mutex<ShareState>>,
    port: u16,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LiveShare {
    /// Start a sharing session bound to `127.0.0.1` on an OS-assigned port,
    /// serving `note` (with live content `markdown`) from `dir`.
    pub fn start(dir: PathBuf, note: String, markdown: String) -> std::io::Result<LiveShare> {
        let server =
            Server::http("127.0.0.1:0").map_err(|e| std::io::Error::other(e.to_string()))?;
        let port = server
            .server_addr()
            .to_ip()
            .map(|addr| addr.port())
            .unwrap_or(0);

        let state = Arc::new(Mutex::new(ShareState {
            dir,
            current_note: note,
            current_markdown: markdown,
            generation: 1,
        }));
        let stop = Arc::new(AtomicBool::new(false));

        let handle = {
            let state = Arc::clone(&state);
            let stop = Arc::clone(&stop);
            thread::spawn(move || serve_loop(server, state, stop))
        };

        Ok(LiveShare {
            state,
            port,
            stop,
            handle: Some(handle),
        })
    }

    /// The port the server is listening on.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The shareable URL for `note`.
    pub fn url_for(&self, note: &str) -> String {
        format!("http://localhost:{}/{}", self.port, encode_path(note))
    }

    /// Update the note/content the server considers "current". Bumps the
    /// generation (triggering live reload) only when something actually changed.
    pub fn set_current(&self, note: &str, markdown: &str) {
        if let Ok(mut st) = self.state.lock()
            && (st.current_note != note || st.current_markdown != markdown)
        {
            st.current_note = note.to_string();
            st.current_markdown = markdown.to_string();
            st.generation = st.generation.wrapping_add(1);
        }
    }

    /// Stop the server and join its thread. Idempotent.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LiveShare {
    fn drop(&mut self) {
        self.stop();
    }
}

fn serve_loop(server: Server, state: Arc<Mutex<ShareState>>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        match server.recv_timeout(POLL_TIMEOUT) {
            Ok(Some(request)) => handle_request(request, &state),
            Ok(None) => {} // timed out; loop back and re-check the stop flag
            Err(_) => break,
        }
    }
}

fn handle_request(request: Request, state: &Arc<Mutex<ShareState>>) {
    let raw_url = request.url().to_string();
    let (path_part, query_part) = match raw_url.split_once('?') {
        Some((p, q)) => (p, q),
        None => (raw_url.as_str(), ""),
    };
    let path = percent_decode(path_part);

    // Snapshot the shared state under a short lock, then do all I/O and
    // rendering without holding it (so a slow request never blocks the GUI).
    let (dir, current_note, current_markdown, generation) = match state.lock() {
        Ok(st) => (
            st.dir.clone(),
            st.current_note.clone(),
            st.current_markdown.clone(),
            st.generation,
        ),
        Err(_) => {
            let _ = request.respond(html_response("<p>Internal error.</p>", 500));
            return;
        }
    };

    // Root: send the browser to the currently open note.
    if path == "/" {
        let location = format!("/{}", encode_path(&current_note));
        let response = Response::empty(302).with_header(ascii_header("Location", &location));
        let _ = request.respond(response);
        return;
    }

    if path == "/favicon.ico" {
        let _ = request.respond(Response::empty(204));
        return;
    }

    // Version endpoint polled by the live-reload script.
    if path == "/__piki/version" {
        let note = query_param(query_part, "note").unwrap_or_default();
        let token = version_token(&note, &current_note, generation, &dir);
        let _ = request.respond(text_response(&token));
        return;
    }

    // Anything else is a note path.
    let note = path.trim_start_matches('/');
    let markdown = if note == current_note {
        // The current note is served from memory so unsaved edits show up live.
        // This also covers plugin views ("!index"), which have no file on disk.
        Some(current_markdown)
    } else if is_valid_note_name(note) {
        load_note_markdown(&dir, note)
    } else {
        None
    };

    let Some(markdown) = markdown else {
        let _ = request.respond(html_response(&not_found_page(note), 404));
        return;
    };

    if query_param(query_part, "raw").is_some() {
        let _ = request.respond(html_response(&render_fragment(&markdown), 200));
    } else {
        let token = version_token(note, &current_note, generation, &dir);
        let page = render_page(note, &markdown, &token);
        let _ = request.respond(html_response(&page, 200));
    }
}

/// The opaque version token for `note`, compared by the browser to detect
/// changes. The current note uses the generation counter; any other note uses
/// its file modification time. The distinct `g`/`m` prefixes guarantee that the
/// token changes when a note stops being the current one, forcing one reload
/// onto the on-disk content.
fn version_token(note: &str, current_note: &str, generation: u64, dir: &Path) -> String {
    if note == current_note {
        return format!("g{generation}");
    }
    if !is_valid_note_name(note) {
        return "invalid".to_string();
    }
    let path = dir.join(ensure_md_extension(note));
    let nanos = fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("m{nanos}")
}

/// Whether `name` is a safe relative note path (no traversal, no absolute
/// paths, no control characters). Gates on-disk loads so a crafted URL cannot
/// escape the notes directory.
fn is_valid_note_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.contains('\\') {
        return false;
    }
    if name.chars().any(|c| c.is_control()) {
        return false;
    }
    name.split('/')
        .all(|component| !(component.is_empty() || component == "." || component == ".."))
}

fn load_note_markdown(dir: &Path, note: &str) -> Option<String> {
    // `note` is validated by `is_valid_note_name`, so the joined path cannot
    // escape `dir`.
    fs::read_to_string(dir.join(ensure_md_extension(note))).ok()
}

// --- Rendering -------------------------------------------------------------

/// Render just the note body: the `<div id="piki-doc">` inner HTML. Fetched by
/// the live-reload script to swap content in place without a full reload.
fn render_fragment(markdown: &str) -> String {
    let mut doc = markdown_to_document(markdown);
    rewrite_links_in_document(&mut doc);
    let anchors = collect_heading_anchors(&doc);
    let sectioned = render_sectioned_html(&doc);
    inject_heading_ids(&sectioned, &anchors)
}

/// Render the document with each top-level heading and the blocks that follow it
/// (until the next heading) wrapped in a `<section class="piki-sec">`.
///
/// The wrapper groups a heading with its content semantically and anchors the
/// first-child margin reset in the stylesheet. In two-column mode the sections
/// are deliberately *breakable*: a tall section (a heading followed by a long
/// list) must be allowed to split across the column boundary, otherwise the
/// balancer is forced to drop the whole section into one column and overflow it
/// while the other column has room to spare. Keeping the heading attached to at
/// least the start of its content is handled instead by `break-after: avoid` on
/// the heading (see the stylesheet), which lets the content flow on into the
/// next column without orphaning the heading.
fn render_sectioned_html(doc: &Document) -> String {
    let is_heading = |p: &Paragraph| {
        matches!(
            p,
            Paragraph::Header1 { .. } | Paragraph::Header2 { .. } | Paragraph::Header3 { .. }
        )
    };

    let mut out = String::new();
    let mut section_open = false;
    for paragraph in &doc.paragraphs {
        if is_heading(paragraph) {
            if section_open {
                out.push_str("</section>\n");
            }
            out.push_str("<section class=\"piki-sec\">\n");
            section_open = true;
        }
        let single = Document::new().with_paragraphs(vec![paragraph.clone()]);
        out.push_str(&document_to_html(&single));
        out.push('\n');
    }
    if section_open {
        out.push_str("</section>\n");
    }
    out
}

/// Render a complete, styled HTML page for `note`, embedding the current version
/// token so the reload script starts in sync.
fn render_page(note: &str, markdown: &str, version: &str) -> String {
    let body = render_fragment(markdown);
    let mut page = String::with_capacity(
        body.len() + STYLESHEET.len() + RELOAD_SCRIPT.len() + COLUMN_SCRIPT.len() + 512,
    );
    page.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\" />\n");
    page.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />\n");
    // Advertise dark-mode support to the browser before CSS parses, so a viewer
    // on a dark system gets the dark canvas and native controls immediately
    // (no white flash on load). The stylesheet's `prefers-color-scheme` block
    // then themes the content itself.
    page.push_str("<meta name=\"color-scheme\" content=\"light dark\" />\n");
    page.push_str("<title>");
    page.push_str(&html_escape_text(note));
    page.push_str("</title>\n<style>");
    page.push_str(STYLESHEET);
    page.push_str("</style>\n</head>\n<body>\n");
    page.push_str("<div id=\"piki-doc\">\n");
    page.push_str(&body);
    page.push_str("\n</div>\n");
    page.push_str("<div id=\"piki-status\" hidden>Live sharing has ended.</div>\n");
    // Subtle footer with attribution and two reading-mode toggles: line spacing
    // (wide/compact) and layout (1-col / 2-col). It lives outside #piki-doc so it
    // (and the chosen modes) survive live-reload content swaps; each choice is
    // persisted in localStorage across notes.
    page.push_str("<footer id=\"piki-footer\">Shared by Piki v");
    page.push_str(env!("CARGO_PKG_VERSION"));
    page.push_str(FOOTER_CONTROLS);
    page.push_str("</footer>\n");
    page.push_str("<script>window.__pikiInitialVersion = ");
    page.push_str(&json_string(version));
    page.push_str(";</script>\n<script>");
    page.push_str(RELOAD_SCRIPT);
    page.push_str("</script>\n<script>");
    page.push_str(COLUMN_SCRIPT);
    page.push_str("</script>\n<script>");
    page.push_str(SPACING_SCRIPT);
    page.push_str("</script>\n<script>");
    page.push_str(FOOTER_FADE_SCRIPT);
    page.push_str("</script>\n</body>\n</html>\n");
    page
}

fn not_found_page(note: &str) -> String {
    format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\" />\
         <meta name=\"color-scheme\" content=\"light dark\" />\
         <title>Not found</title><style>{STYLESHEET}</style></head>\
         <body><h1>Note not available</h1><p>The note <code>{}</code> is not \
         being shared.</p></body></html>",
        html_escape_text(note)
    )
}

// --- Link rewriting --------------------------------------------------------

/// Rewrite internal link targets throughout `doc` so they resolve against the
/// server root when clicked in the browser.
fn rewrite_links_in_document(doc: &mut Document) {
    for paragraph in &mut doc.paragraphs {
        rewrite_links_in_paragraph(paragraph);
    }
}

fn rewrite_links_in_paragraph(paragraph: &mut Paragraph) {
    match paragraph {
        Paragraph::Text { content }
        | Paragraph::Header1 { content }
        | Paragraph::Header2 { content }
        | Paragraph::Header3 { content }
        | Paragraph::CodeBlock { content } => {
            for span in content.iter_mut() {
                rewrite_links_in_span(span);
            }
        }
        Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries } => {
            for entry in entries.iter_mut() {
                for child in entry.iter_mut() {
                    rewrite_links_in_paragraph(child);
                }
            }
        }
        Paragraph::Checklist { items } => {
            for item in items.iter_mut() {
                rewrite_links_in_checklist_item(item);
            }
        }
        Paragraph::Quote { children } => {
            for child in children.iter_mut() {
                rewrite_links_in_paragraph(child);
            }
        }
        Paragraph::Table { rows } => {
            for row in rows.iter_mut() {
                for cell in row.cells.iter_mut() {
                    for span in cell.content.iter_mut() {
                        rewrite_links_in_span(span);
                    }
                }
            }
        }
    }
}

fn rewrite_links_in_checklist_item(item: &mut ChecklistItem) {
    for span in item.content.iter_mut() {
        rewrite_links_in_span(span);
    }
    for child in item.children.iter_mut() {
        rewrite_links_in_checklist_item(child);
    }
}

fn rewrite_links_in_span(span: &mut Span) {
    if span.style == InlineStyle::Link
        && let Some(target) = &span.link_target
        && let Some(rewritten) = rewrite_link_target(target)
    {
        span.link_target = Some(rewritten);
    }
    for child in span.children.iter_mut() {
        rewrite_links_in_span(child);
    }
}

/// Map a stored link destination to what it should point at in the web view.
///
/// Returns `None` (leave unchanged) for external links (`https://`, `mailto:`,
/// …) and bare same-page anchors (`#section`). Internal note links — including
/// `piki://` URLs — are rewritten to a server-absolute `/<note>[#section]` so
/// they resolve regardless of how deep the current URL is (piki note names are
/// absolute from the notes directory, not relative to the current note).
fn rewrite_link_target(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    // Strip our own `piki://` scheme first; genuine external URLs are untouched.
    let normalized = normalize_link_target(trimmed);
    if is_external_link(&normalized) {
        return None;
    }
    let (note, fragment) = split_target(&normalized);
    if note.is_empty() {
        return None;
    }
    let mut out = format!("/{}", encode_path(note));
    if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
        out.push('#');
        out.push_str(&encode_fragment(fragment));
    }
    Some(out)
}

// --- Heading anchors -------------------------------------------------------

/// Anchor slugs for every heading in `doc`, in the order the HTML writer emits
/// them (depth-first, document order), so they can be paired positionally with
/// the `<hN>` tags in the serialized output.
fn collect_heading_anchors(doc: &Document) -> Vec<String> {
    let mut texts = Vec::new();
    collect_heading_texts(&doc.paragraphs, &mut texts);
    heading_anchors(&texts)
}

fn collect_heading_texts(paragraphs: &[Paragraph], out: &mut Vec<String>) {
    for paragraph in paragraphs {
        match paragraph {
            Paragraph::Header1 { content }
            | Paragraph::Header2 { content }
            | Paragraph::Header3 { content } => out.push(spans_plain_text(content)),
            Paragraph::Quote { children } => collect_heading_texts(children, out),
            Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries } => {
                for entry in entries {
                    collect_heading_texts(entry, out);
                }
            }
            _ => {}
        }
    }
}

fn spans_plain_text(spans: &[Span]) -> String {
    let mut text = String::new();
    for span in spans {
        collect_span_text(span, &mut text);
    }
    text
}

fn collect_span_text(span: &Span, out: &mut String) {
    out.push_str(&span.text);
    for child in &span.children {
        collect_span_text(child, out);
    }
}

/// Splice `id` attributes into the heading tags of `html`, pairing the i-th
/// `<h1>`/`<h2>`/`<h3>` (in output order) with `anchors[i]`. Matching the bare
/// opening tag is safe because the HTML writer never emits attributes on
/// headings and entity-encodes any literal `<` in text/code.
fn inject_heading_ids(html: &str, anchors: &[String]) -> String {
    if anchors.is_empty() {
        return html.to_string();
    }
    let mut out = String::with_capacity(html.len() + anchors.len() * 20);
    let mut rest = html;
    let mut idx = 0;
    while idx < anchors.len() {
        let next = ["<h1>", "<h2>", "<h3>"]
            .iter()
            .filter_map(|tag| rest.find(tag).map(|pos| (pos, *tag)))
            .min_by_key(|(pos, _)| *pos);
        let Some((pos, tag)) = next else { break };
        out.push_str(&rest[..pos]);
        let level = &tag[1..3]; // "h1" | "h2" | "h3"
        out.push_str(&format!(
            "<{level} id=\"{}\">",
            html_escape_attr(&anchors[idx])
        ));
        rest = &rest[pos + tag.len()..];
        idx += 1;
    }
    out.push_str(rest);
    out
}

// --- HTTP helpers ----------------------------------------------------------

fn html_response(body: &str, code: u16) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body.to_string())
        .with_status_code(code)
        .with_header(ascii_header("Content-Type", "text/html; charset=utf-8"))
}

fn text_response(body: &str) -> Response<Cursor<Vec<u8>>> {
    Response::from_string(body.to_string())
        .with_header(ascii_header("Content-Type", "text/plain; charset=utf-8"))
}

/// Build a header from ASCII name/value. Both are always ASCII here (fixed names
/// and percent-encoded values), so construction cannot fail in practice; on the
/// impossible error we fall back to a benign header rather than panic.
fn ascii_header(name: &str, value: &str) -> Header {
    Header::from_bytes(name.as_bytes(), value.as_bytes())
        .unwrap_or_else(|_| Header::from_bytes(&b"X-Piki"[..], &b"1"[..]).unwrap())
}

fn query_param(query: &str, key: &str) -> Option<String> {
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        (k == key).then(|| percent_decode(&v.replace('+', " ")))
    })
}

// --- Encoding helpers ------------------------------------------------------

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

fn encode_path(s: &str) -> String {
    encode_with(s, |b| is_unreserved(b) || b == b'/')
}

fn encode_fragment(s: &str) -> String {
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

fn html_escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn html_escape_attr(s: &str) -> String {
    html_escape_text(s).replace('"', "&quot;")
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// --- Embedded assets -------------------------------------------------------

/// Polls the version endpoint and swaps in fresh content when it changes,
/// preserving scroll position. Surfaces a banner when the server goes away.
const RELOAD_SCRIPT: &str = r#"(function () {
  var pathNote = decodeURIComponent(location.pathname.replace(/^\//, ""));
  var version = window.__pikiInitialVersion;
  var statusEl = document.getElementById("piki-status");
  function poll() {
    fetch("/__piki/version?note=" + encodeURIComponent(pathNote))
      .then(function (r) { if (!r.ok) throw new Error("bad"); return r.text(); })
      .then(function (v) {
        if (statusEl) statusEl.hidden = true;
        if (v === version) return;
        version = v;
        return fetch(location.pathname + "?raw=1")
          .then(function (r) { return r.text(); })
          .then(function (html) {
            var doc = document.getElementById("piki-doc");
            if (doc) doc.innerHTML = html;
          });
      })
      .catch(function () { if (statusEl) statusEl.hidden = false; });
  }
  setInterval(poll, 1000);
  poll();
})();"#;

/// Wires the footer's 1-col / 2-col toggle: applies the saved preference on
/// load, reflects the active choice, and persists changes. The layout itself is
/// driven by the `cols-2` class on `<body>` (see the stylesheet), which survives
/// live-reload content swaps because only `#piki-doc` is replaced.
const COLUMN_SCRIPT: &str = r#"(function () {
  function apply(n) {
    document.body.classList.toggle("cols-2", n === 2);
    try { localStorage.setItem("pikiCols", String(n)); } catch (e) {}
    var links = document.querySelectorAll(".piki-col");
    for (var i = 0; i < links.length; i++) {
      links[i].classList.toggle("active", links[i].getAttribute("data-cols") === String(n));
    }
  }
  var saved = 1;
  try { if (localStorage.getItem("pikiCols") === "2") saved = 2; } catch (e) {}
  apply(saved);
  var links = document.querySelectorAll(".piki-col");
  for (var i = 0; i < links.length; i++) {
    links[i].addEventListener("click", function (e) {
      e.preventDefault();
      apply(parseInt(this.getAttribute("data-cols"), 10));
    });
  }
})();"#;

/// The footer's interactive controls: a wide/compact line-spacing toggle (two
/// stacked-line icons) followed by a one/two column toggle. The two mirror each
/// other — each is a set of `<a>` links carrying a `data-*` value, with the
/// current choice marked `active`; the scripts below drive them and persist the
/// choice. Kept as one raw string literal so the inline SVG icons need no
/// escaping; the newlines between elements collapse to a single space in HTML.
const FOOTER_CONTROLS: &str = r##" &bull;
<a href="#" class="piki-spacing active" data-spacing="wide" title="Wide line spacing" aria-label="Wide line spacing"><svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><line x1="2.5" y1="4" x2="13.5" y2="4"/><line x1="2.5" y1="12" x2="13.5" y2="12"/></svg></a>
<a href="#" class="piki-spacing" data-spacing="compact" title="Compact line spacing" aria-label="Compact line spacing"><svg viewBox="0 0 16 16" width="15" height="15" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><line x1="2.5" y1="6" x2="13.5" y2="6"/><line x1="2.5" y1="10" x2="13.5" y2="10"/></svg></a>
&bull;
<a href="#" class="piki-col active" data-cols="1">1 col</a>
<a href="#" class="piki-col" data-cols="2">2 cols</a>"##;

/// Wires the footer's wide/compact line-spacing toggle, mirroring
/// `COLUMN_SCRIPT`: applies the saved preference on load, reflects the active
/// choice, and persists changes. The density is driven by the `compact` class on
/// `<body>` (see the stylesheet), which survives live-reload content swaps
/// because only `#piki-doc` is replaced.
const SPACING_SCRIPT: &str = r#"(function () {
  function apply(mode) {
    document.body.classList.toggle("compact", mode === "compact");
    try { localStorage.setItem("pikiSpacing", mode); } catch (e) {}
    var links = document.querySelectorAll(".piki-spacing");
    for (var i = 0; i < links.length; i++) {
      links[i].classList.toggle("active", links[i].getAttribute("data-spacing") === mode);
    }
  }
  var saved = "wide";
  try { if (localStorage.getItem("pikiSpacing") === "compact") saved = "compact"; } catch (e) {}
  apply(saved);
  var links = document.querySelectorAll(".piki-spacing");
  for (var i = 0; i < links.length; i++) {
    links[i].addEventListener("click", function (e) {
      e.preventDefault();
      apply(this.getAttribute("data-spacing"));
    });
  }
})();"#;

/// Auto-hides the footer so it stays out of the way: it fades to fully
/// transparent (and click-through, via the `piki-faded` class — see the
/// stylesheet) about 3s after the page loads and about 3s after the pointer
/// last left its corner. Any pointer movement near that corner — or keyboard
/// focus landing on a toggle — rouses it again and restarts the timer, so it is
/// there whenever you reach for it and gone the rest of the time.
const FOOTER_FADE_SCRIPT: &str = r#"(function () {
  var footer = document.getElementById("piki-footer");
  if (!footer) return;
  var timer = null;
  function fade() { footer.classList.add("piki-faded"); }
  function wake() {
    footer.classList.remove("piki-faded");
    if (timer) clearTimeout(timer);
    timer = setTimeout(fade, 3000);
  }
  function nearCorner(x, y) {
    var r = footer.getBoundingClientRect();
    return x >= r.left - 40 && y >= r.top - 40;
  }
  document.addEventListener("mousemove", function (e) {
    if (nearCorner(e.clientX, e.clientY)) wake();
  });
  document.addEventListener("touchstart", function (e) {
    var t = e.touches[0];
    if (t && nearCorner(t.clientX, t.clientY)) wake();
  }, { passive: true });
  footer.addEventListener("focusin", wake);
  // Visible on load, then fade after ~3s.
  wake();
})();"#;

/// Self-contained stylesheet, modeled on VS Code's Markdown preview (the look
/// of tdoc's own `html::write_document`), with automatic dark mode. The
/// first-child rule is scoped to `#piki-doc` because the content lives in that
/// wrapper rather than directly under `<body>`.
const STYLESHEET: &str = r##"
:root { color-scheme: light dark; }

body {
  font-family: -apple-system, BlinkMacSystemFont, "Segoe WPC", "Segoe UI",
    system-ui, "Ubuntu", "Droid Sans", sans-serif;
  font-size: 14px;
  line-height: 1.6;
  color: #1f2328;
  background-color: #ffffff;
  max-width: 760px;
  margin: 0 auto;
  padding: 24px 26px 48px;
  word-wrap: break-word;
}

a { color: #0969da; text-decoration: none; }
a:hover { text-decoration: underline; }

h1, h2, h3, h4, h5, h6 {
  margin-top: 24px;
  margin-bottom: 16px;
  font-weight: 600;
  line-height: 1.25;
}
h1 { font-size: 2em; padding-bottom: 0.3em; border-bottom: 1px solid #d8dee4; }
h2 { font-size: 1.5em; padding-bottom: 0.3em; border-bottom: 1px solid #d8dee4; }
h3 { font-size: 1.25em; }

#piki-doc > :first-child,
#piki-doc > .piki-sec:first-child > :first-child { margin-top: 0; }

p { margin-top: 0; margin-bottom: 16px; }

ul, ol { margin-top: 0; margin-bottom: 16px; padding-left: 2em; }
li + li { margin-top: 0.25em; }
li > ul, li > ol { margin-top: 0.25em; margin-bottom: 0; }
li:has(> input[type="checkbox"]) { list-style: none; }
li > input[type="checkbox"] { margin: 0 0.4em 0 -1.4em; vertical-align: middle; }

blockquote {
  margin: 0 0 16px 0;
  padding: 0 1em;
  color: #656d76;
  border-left: 0.25em solid #d0d7de;
}
blockquote > :first-child { margin-top: 0; }
blockquote > :last-child { margin-bottom: 0; }

code, tt {
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas,
    "Liberation Mono", monospace;
  font-size: 0.9em;
  padding: 0.2em 0.4em;
  background-color: rgba(175, 184, 193, 0.2);
  border-radius: 6px;
}

pre {
  margin-top: 0;
  margin-bottom: 16px;
  padding: 16px;
  overflow: auto;
  font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas,
    "Liberation Mono", monospace;
  font-size: 0.9em;
  line-height: 1.45;
  background-color: #f6f8fa;
  border-radius: 6px;
}
pre code, pre tt {
  padding: 0;
  font-size: inherit;
  background-color: transparent;
  border-radius: 0;
}

table {
  margin-top: 0;
  margin-bottom: 16px;
  border-collapse: collapse;
  display: block;
  width: max-content;
  max-width: 100%;
  overflow: auto;
}
th, td { padding: 6px 13px; border: 1px solid #d0d7de; }
th { font-weight: 600; }
tr:nth-child(2n) { background-color: #f6f8fa; }

mark { background-color: #fff8c5; color: inherit; }

hr { height: 0.25em; margin: 24px 0; background-color: #d0d7de; border: 0; }

img { max-width: 100%; }

#piki-status {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  padding: 8px 16px;
  font-weight: 600;
  text-align: center;
  color: #ffffff;
  background-color: #cf222e;
}

/* Subtle footer with attribution and the reading-mode toggles, pinned as a
   small rounded pill in the bottom-right corner rather than a full-width bar, so
   it takes up as little of the page as possible. With no `left`/`width` it
   shrinks to fit its (single-line) content; the body's bottom padding keeps the
   last line from hiding behind it. */
#piki-footer {
  position: fixed;
  right: 12px;
  bottom: 12px;
  padding: 5px 12px;
  font-size: 12px;
  white-space: nowrap;
  color: #8b949e;
  background-color: rgba(255, 255, 255, 0.92);
  border: 1px solid #d8dee4;
  border-radius: 8px;
  box-shadow: 0 1px 4px rgba(31, 35, 40, 0.12);
  /* Fade in quickly when roused (see FOOTER_FADE_SCRIPT). */
  transition: opacity 0.18s ease;
}
/* Idle state: faded fully out and click-through, so it never obscures or blocks
   the content beneath it. Fades out slowly, unlike the quick fade-in above. */
#piki-footer.piki-faded {
  opacity: 0;
  pointer-events: none;
  transition: opacity 1s ease;
}
@media (prefers-reduced-motion: reduce) {
  #piki-footer, #piki-footer.piki-faded { transition: none; }
}
#piki-footer a.piki-col { color: #0969da; text-decoration: none; cursor: pointer; }
#piki-footer a.piki-col:hover { text-decoration: underline; }
#piki-footer a.piki-col.active {
  color: inherit;
  text-decoration: underline;
  cursor: default;
}
/* The line-spacing toggle uses icons rather than text; the current choice is
   shown grey (inherit) like the active column link, the other stays a blue
   link. `currentColor` on the SVG strokes makes them follow that color. */
#piki-footer a.piki-spacing {
  color: #0969da;
  cursor: pointer;
  display: inline-block;
  vertical-align: middle;
  margin: 0 1px;
}
#piki-footer a.piki-spacing.active { color: inherit; cursor: default; }
#piki-footer a.piki-spacing svg { display: block; }

/* Compact reading mode: tighter line height and block spacing so more content
   fits on screen. Toggled from the footer; the `compact` class on <body>
   survives live-reload because only #piki-doc is swapped. */
body.compact { line-height: 1.35; }
body.compact p,
body.compact ul,
body.compact ol,
body.compact pre,
body.compact blockquote,
body.compact table { margin-bottom: 10px; }
body.compact h1,
body.compact h2,
body.compact h3,
body.compact h4,
body.compact h5,
body.compact h6 { margin-top: 16px; margin-bottom: 8px; }
body.compact li + li { margin-top: 0.1em; }

/* Two-column reading mode: widen the page and flow the document into two
   balanced columns. Content is allowed to break across the column boundary so
   the columns balance — a long section (e.g. a heading with a big list) splits
   between the two columns instead of being forced whole into one and
   overflowing it. The rules below only forbid the *awkward* breaks: a heading
   stranded at a column foot, or a list item / code block sliced in half. */
body.cols-2 { max-width: 1400px; }
body.cols-2 #piki-doc { column-count: 2; column-gap: 48px; }
/* Keep a heading with the content that follows it, so a column break never
   orphans a heading at the foot of a column, while still letting the content
   itself flow on into the next column. `avoid-column` is the value Firefox
   honors in multicol; the `-webkit-`/`page-break-` forms cover WebKit/Blink
   and older engines. */
#piki-doc h1, #piki-doc h2, #piki-doc h3,
#piki-doc h4, #piki-doc h5, #piki-doc h6 {
  break-after: avoid;
  break-after: avoid-column;
  -webkit-column-break-after: avoid;
  page-break-after: avoid;
  break-inside: avoid;
}
#piki-doc pre, #piki-doc blockquote, #piki-doc table, #piki-doc li,
#piki-doc img { break-inside: avoid; }

@media (prefers-color-scheme: dark) {
  body { color: #e6edf3; background-color: #0d1117; }
  a { color: #4493f8; }
  h1, h2 { border-bottom-color: #30363d; }
  blockquote { color: #9198a1; border-left-color: #30363d; }
  code, tt { background-color: rgba(110, 118, 129, 0.4); }
  pre { background-color: #161b22; }
  th, td { border-color: #30363d; }
  tr:nth-child(2n) { background-color: #161b22; }
  mark { background-color: #bb8009; color: #1f2328; }
  hr { background-color: #30363d; }
  #piki-footer {
    color: #8b949e;
    background-color: rgba(13, 17, 23, 0.92);
    border-color: #30363d;
    box-shadow: 0 1px 4px rgba(1, 4, 9, 0.4);
  }
  #piki-footer a.piki-col { color: #4493f8; }
  #piki-footer a.piki-col.active { color: inherit; }
  #piki-footer a.piki-spacing { color: #4493f8; }
  #piki-footer a.piki-spacing.active { color: inherit; }
}
"##;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_note_names() {
        assert!(is_valid_note_name("frontpage"));
        assert!(is_valid_note_name("project-a/standup"));
        assert!(is_valid_note_name("sprint-q2.6"));

        assert!(!is_valid_note_name(""));
        assert!(!is_valid_note_name("/etc/passwd"));
        assert!(!is_valid_note_name(".."));
        assert!(!is_valid_note_name("a/../b"));
        assert!(!is_valid_note_name("a/./b"));
        assert!(!is_valid_note_name("a//b"));
        assert!(!is_valid_note_name("a\\b"));
        assert!(!is_valid_note_name("a\nb"));
    }

    #[test]
    fn rewrites_internal_links_to_server_paths() {
        assert_eq!(rewrite_link_target("other"), Some("/other".into()));
        assert_eq!(
            rewrite_link_target("path/to/note"),
            Some("/path/to/note".into())
        );
        assert_eq!(
            rewrite_link_target("note#section"),
            Some("/note#section".into())
        );
        // piki:// URLs are normalized then rewritten.
        assert_eq!(
            rewrite_link_target("piki://work/auth#security-model"),
            Some("/work/auth#security-model".into())
        );
        // Reserved characters (space, colon) are percent-encoded, matching
        // piki's own `section_link` path encoding.
        assert_eq!(
            rewrite_link_target("Notes: Meeting"),
            Some("/Notes%3A%20Meeting".into())
        );
    }

    #[test]
    fn leaves_external_and_anchor_links_untouched() {
        assert_eq!(rewrite_link_target("https://example.com/x"), None);
        assert_eq!(rewrite_link_target("http://example.com"), None);
        assert_eq!(rewrite_link_target("mailto:a@b.com"), None);
        assert_eq!(rewrite_link_target("#section"), None);
        assert_eq!(rewrite_link_target("   "), None);
    }

    #[test]
    fn injects_heading_ids_in_order() {
        let html = "<h1>Title</h1>\n<p>x</p>\n<h2>Sub</h2>";
        let out = inject_heading_ids(html, &["title".into(), "sub".into()]);
        assert!(out.contains("<h1 id=\"title\">Title</h1>"), "{out}");
        assert!(out.contains("<h2 id=\"sub\">Sub</h2>"), "{out}");
    }

    #[test]
    fn render_fragment_rewrites_links_and_adds_anchors() {
        let md = "# Hello World\n\nSee [other](other) and [ext](https://example.com).\n";
        let fragment = render_fragment(md);
        assert!(fragment.contains("<h1 id=\"hello-world\">"), "{fragment}");
        // Each heading and its content are grouped into a section (breakable in
        // two-column mode; the heading's own `break-after: avoid` keeps it from
        // being orphaned at a column foot).
        assert!(
            fragment.contains("<section class=\"piki-sec\">"),
            "{fragment}"
        );
        assert!(fragment.contains("href=\"/other\""), "{fragment}");
        assert!(
            fragment.contains("href=\"https://example.com\""),
            "{fragment}"
        );
    }

    #[test]
    fn query_param_decodes_values() {
        assert_eq!(query_param("note=a%2Fb", "note"), Some("a/b".into()));
        assert_eq!(query_param("raw=1", "raw"), Some("1".into()));
        assert_eq!(query_param("note=x&raw=1", "raw"), Some("1".into()));
        assert_eq!(query_param("note=x", "raw"), None);
    }

    #[test]
    fn page_has_footer_with_version_and_column_toggle() {
        let page = render_page("frontpage", "# Hi\n", "g1");
        assert!(page.contains("id=\"piki-footer\""), "{page}");
        assert!(
            page.contains(concat!("Shared by Piki v", env!("CARGO_PKG_VERSION"))),
            "{page}"
        );
        assert!(page.contains("data-cols=\"1\""), "{page}");
        assert!(page.contains("data-cols=\"2\""), "{page}");
        // The wide/compact line-spacing toggle and the density hook it drives.
        assert!(page.contains("data-spacing=\"wide\""), "{page}");
        assert!(page.contains("data-spacing=\"compact\""), "{page}");
        assert!(page.contains("body.compact"), "{page}");
        // The footer auto-hide: both the faded style and the script that drives it.
        assert!(page.contains("#piki-footer.piki-faded"), "{page}");
        assert!(page.contains("piki-faded"), "{page}");
        // The layout hook the toggle drives must be present in the stylesheet,
        // including the rule that keeps headings with their following content.
        assert!(page.contains("body.cols-2"), "{page}");
        assert!(page.contains("avoid-column"), "{page}");
        // The footer is page-level, not part of the swappable fragment.
        assert!(!render_fragment("# Hi\n").contains("piki-footer"));
    }

    #[test]
    fn page_supports_native_dark_mode() {
        let page = render_page("frontpage", "# Hi\n", "g1");
        // Declared to the browser up front, and themed via the media query.
        assert!(
            page.contains("<meta name=\"color-scheme\" content=\"light dark\" />"),
            "{page}"
        );
        assert!(page.contains("color-scheme: light dark;"), "{page}");
        assert!(
            page.contains("@media (prefers-color-scheme: dark)"),
            "{page}"
        );
    }

    #[test]
    fn version_token_distinguishes_current_from_disk() {
        let dir = Path::new("/tmp/does-not-exist-piki");
        assert_eq!(version_token("cur", "cur", 7, dir), "g7");
        assert_eq!(version_token("gone", "cur", 7, dir), "m0");
    }
}
