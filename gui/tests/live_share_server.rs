//! End-to-end tests for the Live Note Sharing webserver: start a real
//! [`LiveShare`] on a loopback port and drive it over TCP, exercising routing,
//! rendering, link rewriting, path-traversal rejection, and live updates.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{SystemTime, UNIX_EPOCH};

use piki_gui::live_share::LiveShare;

/// Minimal HTTP/1.0 GET. Returns `(head, body)` split on the blank line. Using
/// HTTP/1.0 with `Connection: close` makes tiny_http close the socket after
/// responding, so reading to EOF yields the whole response.
fn http_get(port: u16, path: &str) -> (String, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    let request = format!("GET {path} HTTP/1.0\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).expect("write");
    let mut buf = String::new();
    stream.read_to_string(&mut buf).expect("read");
    match buf.split_once("\r\n\r\n") {
        Some((head, body)) => (head.to_string(), body.to_string()),
        None => (buf, String::new()),
    }
}

fn unique_dir(tag: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("piki-live-share-{tag}-{nanos}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn serves_current_note_with_rewritten_links_and_reload_script() {
    let dir = unique_dir("current");
    fs::write(dir.join("other.md"), "# Other\n\nBody.\n").unwrap();

    let markdown = "# Frontpage\n\nSee [other](other) and [site](https://example.com).\n";
    let share = LiveShare::start(dir.clone(), "frontpage".into(), markdown.into()).unwrap();
    let port = share.port();

    let (head, body) = http_get(port, "/frontpage");
    assert!(
        head.starts_with("HTTP/1.1 200") || head.starts_with("HTTP/1.0 200"),
        "{head}"
    );
    // Heading anchor injected.
    assert!(body.contains("<h1 id=\"frontpage\">"), "{body}");
    // Internal link rewritten to a server-absolute path; external one untouched.
    assert!(body.contains("href=\"/other\""), "{body}");
    assert!(body.contains("href=\"https://example.com\""), "{body}");
    // Live-reload plumbing present, seeded with the current generation token.
    assert!(body.contains("id=\"piki-doc\""), "{body}");
    assert!(
        body.contains("window.__pikiInitialVersion = \"g1\""),
        "{body}"
    );

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn root_redirects_to_current_note() {
    let dir = unique_dir("root");
    let share = LiveShare::start(dir.clone(), "my/shared/note".into(), "hi".into()).unwrap();
    let (head, _) = http_get(share.port(), "/");
    assert!(head.contains(" 302"), "{head}");
    assert!(head.contains("Location: /my/shared/note"), "{head}");
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn serves_other_notes_from_disk_and_404s_unknown() {
    let dir = unique_dir("disk");
    fs::write(dir.join("other.md"), "# Other note\n").unwrap();
    let share = LiveShare::start(dir.clone(), "frontpage".into(), "hi".into()).unwrap();
    let port = share.port();

    let (head, body) = http_get(port, "/other");
    assert!(head.contains(" 200"), "{head}");
    assert!(body.contains("Other note"), "{body}");

    let (head, _) = http_get(port, "/does-not-exist");
    assert!(head.contains(" 404"), "{head}");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn rejects_path_traversal() {
    let dir = unique_dir("traversal");
    // A secret sibling file the server must never expose.
    let parent = dir.parent().unwrap();
    let secret = parent.join("piki-secret.md");
    fs::write(&secret, "TOP SECRET").unwrap();
    let share = LiveShare::start(dir.clone(), "frontpage".into(), "hi".into()).unwrap();

    let (head, body) = http_get(share.port(), "/..%2Fpiki-secret");
    assert!(head.contains(" 404"), "{head}");
    assert!(!body.contains("TOP SECRET"), "{body}");

    fs::remove_file(&secret).ok();
    fs::remove_dir_all(&dir).ok();
}

#[test]
fn version_endpoint_tracks_live_updates() {
    let dir = unique_dir("version");
    let share = LiveShare::start(dir.clone(), "frontpage".into(), "v1".into()).unwrap();
    let port = share.port();

    let (_, body) = http_get(port, "/__piki/version?note=frontpage");
    assert_eq!(body, "g1");

    // An edit to the current note bumps the generation and the served content.
    share.set_current("frontpage", "# v2 heading\n");
    let (_, body) = http_get(port, "/__piki/version?note=frontpage");
    assert_eq!(body, "g2");

    let (_, raw) = http_get(port, "/frontpage?raw=1");
    assert!(raw.contains("v2 heading"), "{raw}");

    // A note that is not current reports a disk-mtime token, not the generation.
    let (_, other) = http_get(port, "/__piki/version?note=other");
    assert!(other.starts_with('m'), "{other}");

    fs::remove_dir_all(&dir).ok();
}

#[test]
fn selection_highlight_is_served_and_cleared() {
    use piki_gui::live_share::HighlightTarget;

    let dir = unique_dir("highlight");
    // The other note exists on disk so we can check it is never highlighted.
    fs::write(dir.join("other.md"), "para 0\n\npara 1\n").unwrap();
    let share = LiveShare::start(
        dir.clone(),
        "frontpage".into(),
        "para 0\n\npara 1\n\npara 2\n".into(),
    )
    .unwrap();
    let port = share.port();

    // No selection: nothing is spotlighted.
    let (_, raw) = http_get(port, "/frontpage?raw=1");
    assert!(!raw.contains("piki-active"), "{raw}");

    // Selecting inside the second paragraph spotlights it (and bumps the version
    // so the browser reloads), leaving the others untouched.
    share.set_highlight(vec![HighlightTarget { block: 1, li: None }]);
    let (_, ver) = http_get(port, "/__piki/version?note=frontpage");
    assert_eq!(ver, "g2");
    let (_, raw) = http_get(port, "/frontpage?raw=1");
    assert!(
        raw.contains("<p class=\"piki-active piki-lead\">para 1</p>"),
        "{raw}"
    );
    assert!(raw.contains("<p>para 0</p>"), "{raw}");

    // A multi-paragraph selection spotlights every block; only the first leads.
    share.set_highlight(vec![
        HighlightTarget { block: 1, li: None },
        HighlightTarget { block: 2, li: None },
    ]);
    let (_, raw) = http_get(port, "/frontpage?raw=1");
    assert!(
        raw.contains("<p class=\"piki-active piki-lead\">para 1</p>"),
        "{raw}"
    );
    assert!(raw.contains("<p class=\"piki-active\">para 2</p>"), "{raw}");
    assert_eq!(raw.matches("piki-active").count(), 2, "{raw}");
    assert_eq!(raw.matches("piki-lead").count(), 1, "{raw}");

    // The highlight belongs to the current note only: another note never carries it.
    let (_, raw_other) = http_get(port, "/other?raw=1");
    assert!(!raw_other.contains("piki-active"), "{raw_other}");

    // Clearing the selection removes the spotlight.
    share.set_highlight(vec![]);
    let (_, raw) = http_get(port, "/frontpage?raw=1");
    assert!(!raw.contains("piki-active"), "{raw}");

    fs::remove_dir_all(&dir).ok();
}
