// Tree paths into a `tdoc::Document`.
//
// The editor keeps the tdoc tree authoritative. A cursor/selection endpoint is a
// `(TreePath, byte_offset)`: the path locates a single leaf (an inline-content-bearing
// node — a paragraph, heading, code block, list-item paragraph, checklist item, or a
// read-only table) within the tree; the offset is a byte offset into that leaf's
// flattened plain text.
//
// A `PathSegment` describes how to descend one level. The variant that is valid at a
// given level is determined by the parent node's type:
//   - `Paragraph(i)`      — only the first segment: `Document.paragraphs[i]`.
//   - `QuoteChild(c)`     — descend into the current `Quote`'s `children[c]`.
//   - `ListEntry{entry, para}` — into the current list's `entries[entry][para]`.
//   - `ChecklistItem(i)`  — into the current `Checklist`'s `items[i]`, or (when the
//                           current node is itself a checklist item) its `children[i]`.

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum PathSegment {
    /// Index into `Document.paragraphs`. Only ever the first segment of a path.
    Paragraph(usize),
    /// Index into a `Quote`'s `children`.
    QuoteChild(usize),
    /// Index into a list's `entries[entry]`, then the `para`-th paragraph of that entry.
    ListEntry { entry: usize, para: usize },
    /// Index into a `Checklist`'s `items`, or a checklist item's `children`.
    ChecklistItem(usize),
}

impl PathSegment {
    /// Sibling ordering key used to compare two paths in document order. Two paths that
    /// share an identical prefix select children of the *same* container at the first
    /// divergence, so the diverging segments are always the same variant and these keys
    /// are directly comparable.
    fn order_key(&self) -> (usize, usize) {
        match *self {
            PathSegment::Paragraph(i) => (i, 0),
            PathSegment::QuoteChild(c) => (c, 0),
            PathSegment::ListEntry { entry, para } => (entry, para),
            PathSegment::ChecklistItem(i) => (i, 0),
        }
    }
}

/// A path from the document root to a single leaf.
#[derive(Debug, Clone, PartialEq, Eq, Default, Hash)]
pub struct TreePath(pub Vec<PathSegment>);

impl TreePath {
    pub fn new() -> Self {
        TreePath(Vec::new())
    }

    pub fn root(index: usize) -> Self {
        TreePath(vec![PathSegment::Paragraph(index)])
    }

    pub fn segments(&self) -> &[PathSegment] {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Return a new path with `segment` appended.
    pub fn child(&self, segment: PathSegment) -> TreePath {
        let mut segs = self.0.clone();
        segs.push(segment);
        TreePath(segs)
    }

    pub fn last(&self) -> Option<&PathSegment> {
        self.0.last()
    }
}

impl Ord for TreePath {
    fn cmp(&self, other: &Self) -> Ordering {
        for (a, b) in self.0.iter().zip(other.0.iter()) {
            match a.order_key().cmp(&b.order_key()) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        // Identical up to the shorter length: the shorter path is an ancestor and renders
        // first (e.g. a checklist item before its nested children).
        self.0.len().cmp(&other.0.len())
    }
}

impl PartialOrd for TreePath {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// A logical cursor/selection position: a leaf (`path`) plus a byte `offset` into that
/// leaf's flattened plain text. Replaces the old flat `{ block_index, offset }`.
///
/// Not `Copy` (the path owns a `Vec`); clone explicitly where a position must outlive a
/// borrow. Ordering is document order: by path, then by offset.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct DocumentPosition {
    pub path: TreePath,
    pub offset: usize,
}

impl DocumentPosition {
    /// Position at `offset` within the leaf located by `path`.
    pub fn at(path: TreePath, offset: usize) -> Self {
        DocumentPosition { path, offset }
    }

    /// Convenience for top-level blocks: `block_index` selects `Document.paragraphs`.
    /// Retained so top-level call sites and tests read naturally.
    pub fn new(block_index: usize, offset: usize) -> Self {
        DocumentPosition {
            path: TreePath::root(block_index),
            offset,
        }
    }

    /// The document start: first top-level paragraph, offset 0.
    pub fn start() -> Self {
        DocumentPosition {
            path: TreePath::root(0),
            offset: 0,
        }
    }
}

impl Ord for DocumentPosition {
    fn cmp(&self, other: &Self) -> Ordering {
        self.path
            .cmp(&other.path)
            .then_with(|| self.offset.cmp(&other.offset))
    }
}

impl PartialOrd for DocumentPosition {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(i: usize) -> PathSegment {
        PathSegment::Paragraph(i)
    }
    fn le(entry: usize, para: usize) -> PathSegment {
        PathSegment::ListEntry { entry, para }
    }

    #[test]
    fn top_level_order() {
        assert!(TreePath(vec![p(0)]) < TreePath(vec![p(1)]));
        assert!(TreePath(vec![p(2)]) > TreePath(vec![p(1)]));
        assert_eq!(TreePath(vec![p(1)]), TreePath(vec![p(1)]));
    }

    #[test]
    fn nested_after_parent_prefix() {
        // A nested list item comes after its containing top-level item's sibling? No:
        // it shares the prefix [p(0), le(0,0)] and is deeper, so it sorts right after.
        let parent = TreePath(vec![p(0), le(0, 0)]);
        let nested = TreePath(vec![p(0), le(0, 1), le(0, 0)]);
        assert!(parent < nested);
    }

    #[test]
    fn prefix_is_less_than_descendant() {
        // A checklist item (leaf) is an ancestor of its nested children and renders first.
        let item = TreePath(vec![p(0), PathSegment::ChecklistItem(0)]);
        let child = TreePath(vec![
            p(0),
            PathSegment::ChecklistItem(0),
            PathSegment::ChecklistItem(0),
        ]);
        assert!(item < child);
    }

    #[test]
    fn entry_then_para_ordering() {
        assert!(TreePath(vec![p(0), le(0, 0)]) < TreePath(vec![p(0), le(0, 1)]));
        assert!(TreePath(vec![p(0), le(0, 5)]) < TreePath(vec![p(0), le(1, 0)]));
    }

    #[test]
    fn child_builder() {
        let base = TreePath::root(3);
        let c = base.child(PathSegment::QuoteChild(2));
        assert_eq!(c.segments(), &[p(3), PathSegment::QuoteChild(2)]);
    }
}
