//! In-memory full-text search over notes.
//!
//! A personal wiki is tiny — a few hundred notes, well under a megabyte of text
//! — so there is deliberately **no index and no external `ripgrep`**: we simply
//! scan the note text in-process. Reading and scanning the whole corpus is a
//! handful of milliseconds, which keeps live filtering (see the GUI note picker)
//! comfortably interactive without the staleness and complexity an index would
//! add.
//!
//! Matching is case-insensitive and **AND-of-terms**: a note matches when
//! *every* whitespace-separated query term appears somewhere in it. This module
//! only concerns itself with note *content*; matching against note names is left
//! to the caller (the GUI picker still fuzzy-matches names on top of this).

use crate::DocumentStore;

/// Split a query into lowercase, whitespace-separated terms, dropping empties.
///
/// Callers pass the resulting terms to the matching helpers below; keeping the
/// terms pre-lowercased means the per-note hot path never re-lowercases them.
pub fn parse_terms(query: &str) -> Vec<String> {
    query.split_whitespace().map(str::to_lowercase).collect()
}

/// True when `haystack_lower` — which the caller must have already lowercased —
/// contains every term. An empty term list matches everything.
///
/// This is the hot path for live filtering: the GUI lowercases each note's body
/// once when the picker opens and then calls this per keystroke, so it stays a
/// plain substring scan with no per-keypress allocation.
pub fn contains_all_terms(haystack_lower: &str, terms: &[String]) -> bool {
    terms.iter().all(|t| haystack_lower.contains(t.as_str()))
}

/// Every line of `content` that contains at least one term, returned as
/// `(1-based line number, line text)` pairs. Case-insensitive.
///
/// Note the asymmetry with [`contains_all_terms`]: inclusion of a note is
/// AND-of-terms (all terms present *somewhere*), but the lines shown are those
/// matching *any* term — the grep-like behaviour you want when displaying where
/// the matches are.
pub fn matching_lines(content: &str, terms: &[String]) -> Vec<(usize, String)> {
    if terms.is_empty() {
        return Vec::new();
    }
    content
        .lines()
        .enumerate()
        .filter_map(|(i, line)| {
            let lower = line.to_lowercase();
            if terms.iter().any(|t| lower.contains(t.as_str())) {
                Some((i + 1, line.to_string()))
            } else {
                None
            }
        })
        .collect()
}

/// The single best snippet line for `content`: the line matching the most
/// distinct terms, ties broken by appearing earliest. Returns
/// `(1-based line number, trimmed line text)`, or `None` when nothing matches.
///
/// Used by the GUI picker to show *where* a content-only hit matched, in place
/// of the note's generic preview.
pub fn first_snippet(content: &str, terms: &[String]) -> Option<(usize, String)> {
    if terms.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize, String)> = None; // (distinct hits, line no, line)
    for (i, line) in content.lines().enumerate() {
        let lower = line.to_lowercase();
        let hits = terms.iter().filter(|t| lower.contains(t.as_str())).count();
        if hits == 0 {
            continue;
        }
        if best.as_ref().map(|(b, _, _)| hits > *b).unwrap_or(true) {
            best = Some((hits, i + 1, line.to_string()));
        }
    }
    best.map(|(_, no, line)| (no, line.trim().to_string()))
}

/// One note's search result: its name and every line that matched a term.
pub struct NoteSearchResult {
    pub name: String,
    pub lines: Vec<(usize, String)>,
}

/// Search every note in `store` for `query`, returning the notes that contain
/// *all* terms, sorted by name, each with its matching lines.
///
/// This reads every note once; for a personal wiki that is a few milliseconds.
/// An empty (or all-whitespace) query matches nothing.
pub fn search_store(store: &DocumentStore, query: &str) -> Result<Vec<NoteSearchResult>, String> {
    let terms = parse_terms(query);
    if terms.is_empty() {
        return Ok(Vec::new());
    }

    let mut names = store.list_all_documents()?;
    names.sort();

    let mut results = Vec::new();
    for name in names {
        // A note that can't be read (e.g. deleted mid-scan) is simply skipped.
        let Ok(doc) = store.load(&name) else { continue };
        let lower = doc.content.to_lowercase();
        if !contains_all_terms(&lower, &terms) {
            continue;
        }
        let lines = matching_lines(&doc.content, &terms);
        results.push(NoteSearchResult { name, lines });
    }
    Ok(results)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_terms_lowercases_and_splits() {
        assert_eq!(parse_terms("  Hello   World "), vec!["hello", "world"]);
        assert!(parse_terms("   ").is_empty());
    }

    #[test]
    fn contains_all_terms_is_and_semantics() {
        let hay = "the quick brown fox".to_string();
        assert!(contains_all_terms(&hay, &parse_terms("quick fox")));
        assert!(contains_all_terms(&hay, &parse_terms("QUICK FOX"))); // caller lowercases hay; terms lowercased here
        assert!(!contains_all_terms(&hay, &parse_terms("quick cat")));
        // Empty term list matches everything.
        assert!(contains_all_terms(&hay, &[]));
    }

    #[test]
    fn matching_lines_reports_line_numbers_for_any_term() {
        let content = "alpha line\nbeta here\ngamma and beta\n";
        let terms = parse_terms("beta");
        assert_eq!(
            matching_lines(content, &terms),
            vec![
                (2, "beta here".to_string()),
                (3, "gamma and beta".to_string()),
            ]
        );
    }

    #[test]
    fn matching_lines_matches_any_of_multiple_terms() {
        let content = "has alpha\nhas beta\nhas neither\n";
        let terms = parse_terms("alpha beta");
        // A line needs only one of the terms to be shown.
        assert_eq!(
            matching_lines(content, &terms),
            vec![(1, "has alpha".to_string()), (2, "has beta".to_string())]
        );
    }

    #[test]
    fn first_snippet_prefers_the_line_with_most_terms() {
        let content = "just alpha here\nalpha and beta together\nbeta alone\n";
        let terms = parse_terms("alpha beta");
        assert_eq!(
            first_snippet(content, &terms),
            Some((2, "alpha and beta together".to_string()))
        );
    }

    #[test]
    fn first_snippet_trims_and_falls_back_to_none() {
        assert_eq!(
            first_snippet("   padded match  \n", &parse_terms("match")),
            Some((1, "padded match".to_string()))
        );
        assert_eq!(first_snippet("nothing here", &parse_terms("zzz")), None);
    }

    #[test]
    fn search_store_finds_notes_with_all_terms() {
        use std::env;
        use std::fs;

        let dir = env::temp_dir().join("piki-test-search-store");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.md"), "the quick brown fox").unwrap();
        fs::write(dir.join("b.md"), "quick notes only").unwrap();
        fs::create_dir_all(dir.join("sub")).unwrap();
        fs::write(dir.join("sub/c.md"), "a fox is quick and brown").unwrap();

        let store = DocumentStore::new(dir.clone());
        let results = search_store(&store, "quick brown").unwrap();

        // Both a.md and sub/c.md contain "quick" AND "brown"; b.md does not.
        let names: Vec<_> = results.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["a", "sub/c"]);
        assert_eq!(
            results[0].lines,
            vec![(1, "the quick brown fox".to_string())]
        );

        // Empty query matches nothing.
        assert!(search_store(&store, "   ").unwrap().is_empty());

        fs::remove_dir_all(&dir).ok();
    }
}
