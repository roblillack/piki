// Traversal of a `tdoc::Document` as a sequence of leaves in document order.
//
// The tdoc tree is authoritative; this module is the read/navigate layer over it. It
// enumerates leaves (paragraphs, headings, code blocks, list-item paragraphs, checklist
// items, and read-only tables) in the order they render, computing for each its
// `TreePath`, intrinsic kind, list marker, and nesting depths. It also resolves a path
// back to the leaf's inline spans (immutably and mutably) and provides
// previous/next/first/last navigation that replaces the old flat `block_index ± 1`.

use tdoc::Document;
use tdoc::inline::Span;
use tdoc::paragraph::{ChecklistItem, Paragraph};
use unicode_segmentation::UnicodeSegmentation;

use super::inline_convert::{inline_to_spans, spans_to_inline};
use super::structured_document::{BlockType, InlineContent, TableCell, TableRow};
use super::tree_path::{DocumentPosition, PathSegment, TreePath};

/// The intrinsic block kind of a leaf (independent of any list marker around it).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParaKind {
    Paragraph,
    Heading(u8),
    CodeBlock,
    /// A read-only table leaf; carries no editable spans.
    Table,
}

/// Marker shown at the start of a leaf that begins a list/checklist entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListMarker {
    pub ordered: bool,
    /// 1-based ordinal for ordered lists (per nesting level); `None` for unordered.
    pub ordinal: Option<u64>,
    /// `Some` for checklist items; the checked state.
    pub checkbox: Option<bool>,
}

/// Everything the display/editor need about one leaf, plus where it lives in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct LeafInfo {
    pub path: TreePath,
    pub kind: ParaKind,
    /// `Some` when this leaf is the first paragraph of a list/checklist entry.
    pub marker: Option<ListMarker>,
    /// List/checklist indentation depth (0 = top-level item or not in a list).
    pub depth: usize,
    /// Number of enclosing block quotes (0 = not quoted).
    pub quote_depth: usize,
}

/// Enumerate every leaf in document order.
pub fn enumerate_leaves(doc: &Document) -> Vec<LeafInfo> {
    let mut out = Vec::new();
    for (i, para) in doc.paragraphs.iter().enumerate() {
        walk_para(para, TreePath::root(i), 0, 0, None, &mut out);
    }
    out
}

/// Just the leaf paths, in document order.
pub fn leaf_paths(doc: &Document) -> Vec<TreePath> {
    enumerate_leaves(doc).into_iter().map(|l| l.path).collect()
}

fn walk_para(
    para: &Paragraph,
    path: TreePath,
    list_depth: usize,
    quote_depth: usize,
    marker: Option<ListMarker>,
    out: &mut Vec<LeafInfo>,
) {
    match para {
        Paragraph::Text { .. } => push_leaf(
            out,
            path,
            ParaKind::Paragraph,
            marker,
            list_depth,
            quote_depth,
        ),
        Paragraph::Header1 { .. } => push_leaf(
            out,
            path,
            ParaKind::Heading(1),
            marker,
            list_depth,
            quote_depth,
        ),
        Paragraph::Header2 { .. } => push_leaf(
            out,
            path,
            ParaKind::Heading(2),
            marker,
            list_depth,
            quote_depth,
        ),
        Paragraph::Header3 { .. } => push_leaf(
            out,
            path,
            ParaKind::Heading(3),
            marker,
            list_depth,
            quote_depth,
        ),
        Paragraph::CodeBlock { .. } => push_leaf(
            out,
            path,
            ParaKind::CodeBlock,
            marker,
            list_depth,
            quote_depth,
        ),
        Paragraph::Table { .. } => {
            push_leaf(out, path, ParaKind::Table, marker, list_depth, quote_depth)
        }
        Paragraph::Quote { children } => {
            for (c, child) in children.iter().enumerate() {
                walk_para(
                    child,
                    path.child(PathSegment::QuoteChild(c)),
                    list_depth,
                    quote_depth + 1,
                    None,
                    out,
                );
            }
        }
        Paragraph::OrderedList { entries } => {
            walk_list(entries, &path, true, list_depth, quote_depth, out)
        }
        Paragraph::UnorderedList { entries } => {
            walk_list(entries, &path, false, list_depth, quote_depth, out)
        }
        Paragraph::Checklist { items } => {
            walk_checklist(items, &path, list_depth, quote_depth, out)
        }
    }
}

fn walk_list(
    entries: &[Vec<Paragraph>],
    path: &TreePath,
    ordered: bool,
    list_depth: usize,
    quote_depth: usize,
    out: &mut Vec<LeafInfo>,
) {
    for (e, entry) in entries.iter().enumerate() {
        for (pi, para) in entry.iter().enumerate() {
            let marker = if pi == 0 {
                Some(ListMarker {
                    ordered,
                    ordinal: ordered.then_some((e + 1) as u64),
                    checkbox: None,
                })
            } else {
                None
            };
            walk_para(
                para,
                path.child(PathSegment::ListEntry { entry: e, para: pi }),
                list_depth + 1,
                quote_depth,
                marker,
                out,
            );
        }
    }
}

fn walk_checklist(
    items: &[ChecklistItem],
    path: &TreePath,
    list_depth: usize,
    quote_depth: usize,
    out: &mut Vec<LeafInfo>,
) {
    for (i, item) in items.iter().enumerate() {
        let item_path = path.child(PathSegment::ChecklistItem(i));
        push_leaf(
            out,
            item_path.clone(),
            ParaKind::Paragraph,
            Some(ListMarker {
                ordered: false,
                ordinal: None,
                checkbox: Some(item.checked),
            }),
            list_depth + 1,
            quote_depth,
        );
        if !item.children.is_empty() {
            walk_checklist(&item.children, &item_path, list_depth + 1, quote_depth, out);
        }
    }
}

fn push_leaf(
    out: &mut Vec<LeafInfo>,
    path: TreePath,
    kind: ParaKind,
    marker: Option<ListMarker>,
    list_depth: usize,
    quote_depth: usize,
) {
    out.push(LeafInfo {
        path,
        kind,
        marker,
        depth: list_depth.saturating_sub(1),
        quote_depth,
    });
}

// ---- Path resolution --------------------------------------------------------------

/// A reference to a resolved leaf node — either a `Paragraph` or a `ChecklistItem`.
enum LeafRef<'a> {
    Para(&'a Paragraph),
    Check(&'a ChecklistItem),
}

fn resolve<'a>(doc: &'a Document, path: &TreePath) -> Option<LeafRef<'a>> {
    let mut segs = path.0.iter();
    let PathSegment::Paragraph(i) = segs.next()? else {
        return None;
    };
    enum Cur<'a> {
        Para(&'a Paragraph),
        Check(&'a ChecklistItem),
    }
    let mut cur = Cur::Para(doc.paragraphs.get(*i)?);
    for seg in segs {
        cur = match (cur, seg) {
            (Cur::Para(Paragraph::Quote { children }), PathSegment::QuoteChild(c)) => {
                Cur::Para(children.get(*c)?)
            }
            (
                Cur::Para(
                    Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                ),
                PathSegment::ListEntry { entry, para },
            ) => Cur::Para(entries.get(*entry)?.get(*para)?),
            (Cur::Para(Paragraph::Checklist { items }), PathSegment::ChecklistItem(c)) => {
                Cur::Check(items.get(*c)?)
            }
            (Cur::Check(item), PathSegment::ChecklistItem(c)) => Cur::Check(item.children.get(*c)?),
            _ => return None,
        };
    }
    Some(match cur {
        Cur::Para(p) => LeafRef::Para(p),
        Cur::Check(c) => LeafRef::Check(c),
    })
}

/// The inline spans of the leaf at `path`, or `None` for tables / invalid paths.
pub fn leaf_spans<'a>(doc: &'a Document, path: &TreePath) -> Option<&'a [Span]> {
    match resolve(doc, path)? {
        LeafRef::Para(Paragraph::Table { .. }) => None,
        LeafRef::Para(p) => Some(p.content()),
        LeafRef::Check(item) => Some(&item.content),
    }
}

/// Mutable inline spans of the leaf at `path`, or `None` for tables / invalid paths.
pub fn leaf_spans_mut<'a>(doc: &'a mut Document, path: &TreePath) -> Option<&'a mut Vec<Span>> {
    let mut segs = path.0.iter();
    let PathSegment::Paragraph(i) = segs.next()? else {
        return None;
    };
    enum Cur<'a> {
        Para(&'a mut Paragraph),
        Check(&'a mut ChecklistItem),
    }
    let mut cur = Cur::Para(doc.paragraphs.get_mut(*i)?);
    for seg in segs {
        cur = match (cur, seg) {
            (Cur::Para(Paragraph::Quote { children }), PathSegment::QuoteChild(c)) => {
                Cur::Para(children.get_mut(*c)?)
            }
            (
                Cur::Para(
                    Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                ),
                PathSegment::ListEntry { entry, para },
            ) => Cur::Para(entries.get_mut(*entry)?.get_mut(*para)?),
            (Cur::Para(Paragraph::Checklist { items }), PathSegment::ChecklistItem(c)) => {
                Cur::Check(items.get_mut(*c)?)
            }
            (Cur::Check(item), PathSegment::ChecklistItem(c)) => {
                Cur::Check(item.children.get_mut(*c)?)
            }
            _ => return None,
        };
    }
    match cur {
        // Only leaf paragraph types own inline content; `content_mut` panics otherwise.
        Cur::Para(p) => match p {
            Paragraph::Text { content }
            | Paragraph::Header1 { content }
            | Paragraph::Header2 { content }
            | Paragraph::Header3 { content }
            | Paragraph::CodeBlock { content } => Some(content),
            _ => None,
        },
        Cur::Check(item) => Some(&mut item.content),
    }
}

// ---- Navigation -------------------------------------------------------------------

pub fn first_leaf_path(doc: &Document) -> Option<TreePath> {
    leaf_paths(doc).into_iter().next()
}

pub fn last_leaf_path(doc: &Document) -> Option<TreePath> {
    leaf_paths(doc).into_iter().last()
}

pub fn next_leaf_path(doc: &Document, path: &TreePath) -> Option<TreePath> {
    let paths = leaf_paths(doc);
    let idx = paths.iter().position(|p| p == path)?;
    paths.into_iter().nth(idx + 1)
}

pub fn prev_leaf_path(doc: &Document, path: &TreePath) -> Option<TreePath> {
    let paths = leaf_paths(doc);
    let idx = paths.iter().position(|p| p == path)?;
    if idx == 0 {
        None
    } else {
        paths.into_iter().nth(idx - 1)
    }
}

/// Number of leaves in the document (the path-model analogue of block count).
pub fn leaf_count(doc: &Document) -> usize {
    enumerate_leaves(doc).len()
}

/// The leaf's inline content as flat runs (empty for tables / invalid paths).
pub fn leaf_inline(doc: &Document, path: &TreePath) -> Vec<InlineContent> {
    leaf_spans(doc, path)
        .map(spans_to_inline)
        .unwrap_or_default()
}

/// Replace the leaf's inline content (converting runs back to spans). Returns `false`
/// for tables / invalid paths (which own no editable spans), leaving the tree unchanged.
pub fn set_leaf_inline(doc: &mut Document, path: &TreePath, content: &[InlineContent]) -> bool {
    if let Some(spans) = leaf_spans_mut(doc, path) {
        *spans = inline_to_spans(content);
        true
    } else {
        false
    }
}

/// Build the presentation `BlockType` for a leaf (the transient descriptor the display
/// and menus consume). Resolves table rows when the leaf is a table.
pub fn leaf_block_type(doc: &Document, info: &LeafInfo) -> BlockType {
    if let Some(marker) = &info.marker {
        return BlockType::ListItem {
            ordered: marker.ordered,
            number: marker.ordinal,
            checkbox: marker.checkbox,
            depth: info.depth,
        };
    }
    match &info.kind {
        ParaKind::Paragraph => {
            if info.quote_depth > 0 {
                BlockType::BlockQuote
            } else {
                BlockType::Paragraph
            }
        }
        ParaKind::Heading(level) => BlockType::Heading { level: *level },
        ParaKind::CodeBlock => BlockType::CodeBlock { language: None },
        ParaKind::Table => BlockType::Table {
            rows: table_rows_at(doc, &info.path),
        },
    }
}

fn table_rows_at(doc: &Document, path: &TreePath) -> Vec<TableRow> {
    let Some(LeafRef::Para(Paragraph::Table { rows })) = resolve(doc, path) else {
        return Vec::new();
    };
    rows.iter()
        .map(|row| {
            TableRow::new(
                row.cells
                    .iter()
                    .map(|cell| TableCell::new(cell.is_header, spans_to_inline(&cell.content)))
                    .collect(),
            )
        })
        .collect()
}

// ---- Plain text & offsets ---------------------------------------------------------

fn span_plain_text(span: &Span, out: &mut String) {
    out.push_str(&span.text);
    for child in &span.children {
        span_plain_text(child, out);
    }
}

/// The flattened plain text of the leaf at `path` (empty for tables / invalid paths).
/// Byte offsets in a `DocumentPosition` index into this string.
pub fn leaf_plain_text(doc: &Document, path: &TreePath) -> String {
    let Some(spans) = leaf_spans(doc, path) else {
        return String::new();
    };
    let mut text = String::new();
    for span in spans {
        span_plain_text(span, &mut text);
    }
    text
}

/// Byte length of the leaf's flattened plain text.
pub fn leaf_text_len(doc: &Document, path: &TreePath) -> usize {
    leaf_plain_text(doc, path).len()
}

// ---- Position clamping & grapheme navigation --------------------------------------

/// Resolve a (possibly stale) path to a valid leaf path, snapping to the nearest
/// existing leaf in document order. Returns `None` only for an empty document.
fn nearest_leaf_path(doc: &Document, path: &TreePath) -> Option<TreePath> {
    let paths = leaf_paths(doc);
    if paths.iter().any(|p| p == path) {
        return Some(path.clone());
    }
    // Snap to the last leaf whose path is <= the target, else the first leaf.
    paths
        .iter()
        .rev()
        .find(|p| *p <= path)
        .or_else(|| paths.first())
        .cloned()
}

/// Clamp a position to a valid leaf and a grapheme boundary at or before its offset.
pub fn clamp_position(doc: &Document, pos: &DocumentPosition) -> DocumentPosition {
    let Some(path) = nearest_leaf_path(doc, &pos.path) else {
        return DocumentPosition::start();
    };
    let text = leaf_plain_text(doc, &path);
    let offset = grapheme_offset_at_or_before(&text, pos.offset);
    DocumentPosition::at(path, offset)
}

/// Clamp a position to a valid leaf and a grapheme boundary at or after its offset.
pub fn clamp_position_forward(doc: &Document, pos: &DocumentPosition) -> DocumentPosition {
    let Some(path) = nearest_leaf_path(doc, &pos.path) else {
        return DocumentPosition::start();
    };
    let text = leaf_plain_text(doc, &path);
    let offset = grapheme_offset_at_or_after(&text, pos.offset);
    DocumentPosition::at(path, offset)
}

/// Previous grapheme boundary within the same leaf (does not cross leaves).
pub fn previous_grapheme_position(doc: &Document, pos: &DocumentPosition) -> DocumentPosition {
    let Some(path) = nearest_leaf_path(doc, &pos.path) else {
        return DocumentPosition::start();
    };
    let text = leaf_plain_text(doc, &path);
    let offset = grapheme_offset_before(&text, pos.offset);
    DocumentPosition::at(path, offset)
}

/// Next grapheme boundary within the same leaf (does not cross leaves).
pub fn next_grapheme_position(doc: &Document, pos: &DocumentPosition) -> DocumentPosition {
    let Some(path) = nearest_leaf_path(doc, &pos.path) else {
        return DocumentPosition::start();
    };
    let text = leaf_plain_text(doc, &path);
    let offset = grapheme_offset_after(&text, pos.offset);
    DocumentPosition::at(path, offset)
}

fn grapheme_boundaries(text: &str) -> Vec<usize> {
    let mut boundaries: Vec<usize> = text.grapheme_indices(true).map(|(idx, _)| idx).collect();
    if boundaries.is_empty() {
        boundaries.push(0);
        return boundaries;
    }
    if boundaries[0] != 0 {
        boundaries.insert(0, 0);
    }
    if *boundaries.last().unwrap() != text.len() {
        boundaries.push(text.len());
    }
    boundaries
}

fn grapheme_offset_at_or_before(text: &str, offset: usize) -> usize {
    let boundaries = grapheme_boundaries(text);
    let mut result = 0usize;
    let max_offset = offset.min(text.len());
    for boundary in boundaries {
        if boundary > max_offset {
            break;
        }
        result = boundary;
    }
    result
}

fn grapheme_offset_at_or_after(text: &str, offset: usize) -> usize {
    let boundaries = grapheme_boundaries(text);
    let max_offset = offset.min(text.len());
    for boundary in boundaries {
        if boundary >= max_offset {
            return boundary;
        }
    }
    text.len()
}

fn grapheme_offset_before(text: &str, offset: usize) -> usize {
    if offset == 0 {
        return 0;
    }
    let boundaries = grapheme_boundaries(text);
    let mut previous = 0usize;
    let max_offset = offset.min(text.len());
    for boundary in boundaries {
        if boundary >= max_offset {
            if boundary == max_offset {
                return previous;
            }
            break;
        }
        previous = boundary;
    }
    previous
}

fn grapheme_offset_after(text: &str, offset: usize) -> usize {
    let boundaries = grapheme_boundaries(text);
    let max_offset = offset.min(text.len());
    for boundary in boundaries {
        if boundary > max_offset {
            return boundary;
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(md: &str) -> Document {
        tdoc::markdown::parse(&mut Cursor::new(md.as_bytes())).expect("parse")
    }

    #[test]
    fn enumerates_top_level_paragraphs() {
        let doc = parse("First\n\nSecond\n\n# Heading");
        let leaves = enumerate_leaves(&doc);
        assert_eq!(leaves.len(), 3);
        assert_eq!(leaves[0].kind, ParaKind::Paragraph);
        assert_eq!(leaves[2].kind, ParaKind::Heading(1));
        assert!(leaves.iter().all(|l| l.marker.is_none() && l.depth == 0));
    }

    #[test]
    fn nested_unordered_list_depths_and_paths() {
        let doc = parse("- a\n    - b\n- c");
        let leaves = enumerate_leaves(&doc);
        let texts: Vec<_> = leaves
            .iter()
            .map(|l| (leaf_text(&doc, &l.path), l.depth, l.marker.is_some()))
            .collect();
        assert_eq!(
            texts,
            vec![
                ("a".to_string(), 0, true),
                ("b".to_string(), 1, true),
                ("c".to_string(), 0, true),
            ]
        );
    }

    #[test]
    fn ordered_list_ordinals_per_level() {
        let doc = parse("1. one\n2. two\n    1. nested-one\n    2. nested-two\n3. three");
        let leaves = enumerate_leaves(&doc);
        let ords: Vec<_> = leaves
            .iter()
            .map(|l| l.marker.as_ref().and_then(|m| m.ordinal))
            .collect();
        // Top level 1,2 then nested 1,2 then top 3.
        assert_eq!(ords, vec![Some(1), Some(2), Some(1), Some(2), Some(3)]);
    }

    #[test]
    fn checklist_items_carry_checkbox() {
        let doc = parse("- [ ] todo\n- [x] done");
        let leaves = enumerate_leaves(&doc);
        let checks: Vec<_> = leaves
            .iter()
            .map(|l| l.marker.as_ref().and_then(|m| m.checkbox))
            .collect();
        assert_eq!(checks, vec![Some(false), Some(true)]);
    }

    #[test]
    fn quote_children_carry_quote_depth() {
        let doc = parse("> quoted line");
        let leaves = enumerate_leaves(&doc);
        assert_eq!(leaves.len(), 1);
        assert_eq!(leaves[0].quote_depth, 1);
    }

    #[test]
    fn leaf_paths_are_in_sorted_order() {
        let doc = parse("- a\n    - b\n- c\n\nAfter");
        let paths = leaf_paths(&doc);
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(
            paths, sorted,
            "enumeration must already be in document order"
        );
    }

    #[test]
    fn navigation_round_trips() {
        let doc = parse("- a\n    - b\n- c");
        let paths = leaf_paths(&doc);
        assert_eq!(first_leaf_path(&doc).as_ref(), Some(&paths[0]));
        assert_eq!(last_leaf_path(&doc).as_ref(), Some(&paths[2]));
        assert_eq!(next_leaf_path(&doc, &paths[0]).as_ref(), Some(&paths[1]));
        assert_eq!(prev_leaf_path(&doc, &paths[1]).as_ref(), Some(&paths[0]));
        assert_eq!(prev_leaf_path(&doc, &paths[0]), None);
        assert_eq!(next_leaf_path(&doc, &paths[2]), None);
    }

    #[test]
    fn leaf_spans_mut_edits_tree() {
        let mut doc = parse("hello");
        let path = TreePath::root(0);
        let spans = leaf_spans_mut(&mut doc, &path).expect("spans");
        spans.push(Span::new_text(" world"));
        assert_eq!(leaf_text(&doc, &path), "hello world");
    }

    fn leaf_text(doc: &Document, path: &TreePath) -> String {
        leaf_spans(doc, path)
            .map(|spans| spans.iter().map(span_text).collect::<String>())
            .unwrap_or_default()
    }

    fn span_text(span: &Span) -> String {
        let mut s = span.text.clone();
        for child in &span.children {
            s.push_str(&span_text(child));
        }
        s
    }
}
