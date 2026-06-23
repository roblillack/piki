#![allow(dead_code)]

use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use regex::Regex;

#[derive(Debug, Clone)]
pub struct Link {
    pub start: usize,
    pub end: usize,
    pub destination: String,
    pub text: String,
}

/// Parse Markdown content and extract links with their positions
pub fn extract_links(content: &str) -> Vec<Link> {
    let mut links = Vec::new();
    let parser = Parser::new(content);

    let mut current_link: Option<(usize, String)> = None;
    let mut link_text = String::new();

    for (event, range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                current_link = Some((range.start, dest_url.to_string()));
                link_text.clear();
            }
            Event::Text(text) if current_link.is_some() => {
                link_text.push_str(&text);
            }
            Event::End(TagEnd::Link) => {
                if let Some((start, dest)) = current_link.take() {
                    links.push(Link {
                        start,
                        end: range.end,
                        destination: dest,
                        text: link_text.clone(),
                    });
                }
            }
            _ => {}
        }
    }

    // Also support wiki-style [[page]] links
    let wiki_link_re = Regex::new(r"\[\[([^\]]+)\]\]").unwrap();
    for cap in wiki_link_re.captures_iter(content) {
        if let Some(matched) = cap.get(0) {
            let page = cap.get(1).unwrap().as_str().to_string();
            links.push(Link {
                start: matched.start(),
                end: matched.end(),
                destination: page.clone(),
                text: page,
            });
        }
    }

    links.sort_by_key(|l| l.start);
    links
}

/// Find link at a specific character position in the text
pub fn find_link_at_position(links: &[Link], pos: usize) -> Option<&Link> {
    links
        .iter()
        .find(|link| pos >= link.start && pos < link.end)
}

/// Returns true if the destination is an external link that should be opened
/// in the system browser/handler rather than loaded as a wiki page.
///
/// Recognises URLs with an explicit authority (e.g. `http://`, `https://`,
/// `ftp://`, `file://`) as well as authority-less schemes like `mailto:` and
/// `tel:`. Plain page names (including ones that happen to contain a colon,
/// such as `Notes: Meeting`) are treated as internal.
pub fn is_external_link(destination: &str) -> bool {
    let dest = destination.trim_start();

    // URLs with an authority component: <scheme>://...
    if let Some(scheme_end) = dest.find("://") {
        let scheme = &dest[..scheme_end];
        if !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        {
            return true;
        }
    }

    // Authority-less schemes that should still be handed off to the system.
    let lower = dest.to_ascii_lowercase();
    lower.starts_with("mailto:") || lower.starts_with("tel:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_markdown_links() {
        let content = "This is a [test link](target.md) in markdown.";
        let links = extract_links(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].destination, "target.md");
        assert_eq!(links[0].text, "test link");
    }

    #[test]
    fn test_wiki_links() {
        let content = "This is a [[WikiPage]] link.";
        let links = extract_links(content);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].destination, "WikiPage");
    }

    #[test]
    fn test_mixed_links() {
        let content = "A [markdown](page1) and [[wiki]] link.";
        let links = extract_links(content);
        assert_eq!(links.len(), 2);
    }

    #[test]
    fn test_is_external_link() {
        // External: schemes with an authority component
        assert!(is_external_link("http://example.com"));
        assert!(is_external_link("https://example.com/path?q=1"));
        assert!(is_external_link("ftp://files.example.com"));
        assert!(is_external_link("file:///etc/hosts"));
        assert!(is_external_link("HTTPS://EXAMPLE.COM"));
        assert!(is_external_link("  https://example.com"));

        // External: authority-less schemes
        assert!(is_external_link("mailto:user@example.com"));
        assert!(is_external_link("tel:+1234567890"));

        // Internal: plain page names, including ones containing a colon
        assert!(!is_external_link("frontpage"));
        assert!(!is_external_link("some/page.md"));
        assert!(!is_external_link("[[WikiPage]]"));
        assert!(!is_external_link("Notes: Meeting"));
        assert!(!is_external_link("C:\\path\\file"));
    }
}
