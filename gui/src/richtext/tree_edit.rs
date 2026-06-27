// Structural mutations of the `tdoc::Document` tree.
//
// `tree_walk` reads/navigates the tree; this module performs the surgery that editing
// requires: splitting a leaf on Enter, merging a leaf into its predecessor on Backspace,
// removing a node (pruning emptied containers), and toggling a checklist item. All take a
// leaf `TreePath` and operate relative to it, returning the resulting cursor location.

use tdoc::Document;
use tdoc::inline::Span;
use tdoc::paragraph::{ChecklistItem, Paragraph};

use super::inline_convert::inline_to_spans;
use super::structured_document::{Block, InlineContent};
use super::tree_path::{PathSegment, TreePath};
use super::tree_walk;

/// A mutable reference to a resolved tree node.
enum NodeMut<'a> {
    Para(&'a mut Paragraph),
    Check(&'a mut ChecklistItem),
}

/// Descend to the node at `path` (a `Paragraph` or a `ChecklistItem`).
fn node_at_mut<'a>(doc: &'a mut Document, path: &TreePath) -> Option<NodeMut<'a>> {
    let mut segs = path.0.iter();
    let PathSegment::Paragraph(i) = segs.next()? else {
        return None;
    };
    let mut cur = NodeMut::Para(doc.paragraphs.get_mut(*i)?);
    for seg in segs {
        cur = match (cur, seg) {
            (NodeMut::Para(Paragraph::Quote { children }), PathSegment::QuoteChild(c)) => {
                NodeMut::Para(children.get_mut(*c)?)
            }
            (
                NodeMut::Para(
                    Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                ),
                PathSegment::ListEntry { entry, para },
            ) => NodeMut::Para(entries.get_mut(*entry)?.get_mut(*para)?),
            (NodeMut::Para(Paragraph::Checklist { items }), PathSegment::ChecklistItem(c)) => {
                NodeMut::Check(items.get_mut(*c)?)
            }
            (NodeMut::Check(item), PathSegment::ChecklistItem(c)) => {
                NodeMut::Check(item.children.get_mut(*c)?)
            }
            _ => return None,
        };
    }
    Some(cur)
}

fn parent_path(path: &TreePath) -> TreePath {
    TreePath(path.0[..path.0.len().saturating_sub(1)].to_vec())
}

fn new_text_paragraph(runs: &[InlineContent]) -> Paragraph {
    Paragraph::new_text().with_content(inline_to_spans(runs))
}

/// Split `content` at flattened byte `offset`, returning `(left, right)`.
fn split_runs(
    content: &[InlineContent],
    offset: usize,
) -> (Vec<InlineContent>, Vec<InlineContent>) {
    let mut block = Block::paragraph();
    block.content = content.to_vec();
    let right = block.split_content_at(offset);
    (block.content, right)
}

/// Replace the leaf paragraph at `path` with `make(spans)`, preserving its inline spans
/// and its position in the tree (in-place variant change, e.g. Text → Header). Returns
/// `false` for checklist items / invalid paths.
pub fn replace_leaf_variant(
    doc: &mut Document,
    path: &TreePath,
    make: impl FnOnce(Vec<Span>) -> Paragraph,
) -> bool {
    let spans = match node_at_mut(doc, path) {
        Some(NodeMut::Para(p)) => p.content().to_vec(),
        _ => return false,
    };
    match node_at_mut(doc, path) {
        Some(NodeMut::Para(p)) => {
            *p = make(spans);
            true
        }
        _ => false,
    }
}

/// Toggle the checked state of the checklist item at `path`. Returns the new state, or
/// `None` if the path is not a checklist item.
pub fn toggle_checkmark(doc: &mut Document, path: &TreePath) -> Option<bool> {
    match node_at_mut(doc, path)? {
        NodeMut::Check(item) => {
            item.checked = !item.checked;
            Some(item.checked)
        }
        NodeMut::Para(_) => None,
    }
}

/// Split the leaf at `path` at byte `offset` into a sibling that follows it (a new
/// paragraph, list entry, quote child, or checklist item depending on context). Returns
/// the path of the new (right-hand) leaf, or `None` for tables / invalid paths.
pub fn split_leaf(doc: &mut Document, path: &TreePath, offset: usize) -> Option<TreePath> {
    // Only editable leaves (not tables) can be split.
    let runs = tree_walk::leaf_spans(doc, path).map(|_| tree_walk::leaf_inline(doc, path))?;
    let (left, right) = split_runs(&runs, offset);
    tree_walk::set_leaf_inline(doc, path, &left);

    let last = path.0.last()?.clone();
    let pp = parent_path(path);
    match last {
        PathSegment::Paragraph(i) => {
            doc.paragraphs.insert(i + 1, new_text_paragraph(&right));
            Some(TreePath::root(i + 1))
        }
        PathSegment::QuoteChild(c) => {
            if let NodeMut::Para(Paragraph::Quote { children }) = node_at_mut(doc, &pp)? {
                children.insert(c + 1, new_text_paragraph(&right));
                Some(pp.child(PathSegment::QuoteChild(c + 1)))
            } else {
                None
            }
        }
        PathSegment::ListEntry { entry, para } => {
            if let NodeMut::Para(
                Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
            ) = node_at_mut(doc, &pp)?
            {
                // The new entry takes the right half plus any continuation paragraphs of
                // the original entry.
                let continuation = if para < entries.get(entry)?.len() {
                    entries[entry].split_off(para + 1)
                } else {
                    Vec::new()
                };
                let mut new_entry = vec![new_text_paragraph(&right)];
                new_entry.extend(continuation);
                entries.insert(entry + 1, new_entry);
                Some(pp.child(PathSegment::ListEntry {
                    entry: entry + 1,
                    para: 0,
                }))
            } else {
                None
            }
        }
        PathSegment::ChecklistItem(c) => {
            let new_item = ChecklistItem::new(false).with_content(inline_to_spans(&right));
            match node_at_mut(doc, &pp)? {
                NodeMut::Para(Paragraph::Checklist { items }) => {
                    items.insert(c + 1, new_item);
                }
                NodeMut::Check(item) => {
                    item.children.insert(c + 1, new_item);
                }
                _ => return None,
            }
            Some(pp.child(PathSegment::ChecklistItem(c + 1)))
        }
    }
}

/// Merge the leaf at `path` into the previous leaf in document order (appending its text).
/// Returns the resulting cursor position (the join point), or `None` when there is no
/// previous leaf or either leaf is a table.
pub fn merge_with_previous(doc: &mut Document, path: &TreePath) -> Option<(TreePath, usize)> {
    let prev = tree_walk::prev_leaf_path(doc, path)?;
    // Both leaves must be editable (tables have no text to merge).
    tree_walk::leaf_spans(doc, &prev)?;
    tree_walk::leaf_spans(doc, path)?;

    let prev_len = tree_walk::leaf_text_len(doc, &prev);
    let cur_runs = tree_walk::leaf_inline(doc, path);

    let mut block = Block::paragraph();
    block.content = tree_walk::leaf_inline(doc, &prev);
    block.content.extend(cur_runs);
    block.normalize_content();
    tree_walk::set_leaf_inline(doc, &prev, &block.content);

    remove_node_at(doc, path);
    Some((prev, prev_len))
}

/// Remove the node at `path` from its parent container, pruning containers that become
/// empty as a result (e.g. removing a list's last entry removes the list).
pub fn remove_node_at(doc: &mut Document, path: &TreePath) {
    let Some(last) = path.0.last().cloned() else {
        return;
    };
    let pp = parent_path(path);
    match last {
        PathSegment::Paragraph(i) => {
            if i < doc.paragraphs.len() {
                doc.paragraphs.remove(i);
            }
        }
        PathSegment::QuoteChild(c) => {
            let mut empty = false;
            if let Some(NodeMut::Para(Paragraph::Quote { children })) = node_at_mut(doc, &pp) {
                if c < children.len() {
                    children.remove(c);
                }
                empty = children.is_empty();
            }
            if empty {
                remove_node_at(doc, &pp);
            }
        }
        PathSegment::ListEntry { entry, para } => {
            let mut empty = false;
            if let Some(NodeMut::Para(
                Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
            )) = node_at_mut(doc, &pp)
            {
                if entry < entries.len() {
                    if para < entries[entry].len() {
                        entries[entry].remove(para);
                    }
                    if entries[entry].is_empty() {
                        entries.remove(entry);
                    }
                }
                empty = entries.is_empty();
            }
            if empty {
                remove_node_at(doc, &pp);
            }
        }
        PathSegment::ChecklistItem(c) => {
            let mut empty = false;
            match node_at_mut(doc, &pp) {
                Some(NodeMut::Para(Paragraph::Checklist { items })) => {
                    if c < items.len() {
                        items.remove(c);
                    }
                    empty = items.is_empty();
                }
                Some(NodeMut::Check(item)) if c < item.children.len() => {
                    item.children.remove(c);
                }
                _ => {}
            }
            if empty {
                remove_node_at(doc, &pp);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn parse(md: &str) -> Document {
        tdoc::markdown::parse(&mut Cursor::new(md.as_bytes())).expect("parse")
    }

    fn md(doc: &Document) -> String {
        let mut buf = Vec::new();
        tdoc::markdown::write(&mut buf, doc).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn split_top_level_paragraph() {
        let mut doc = parse("HelloWorld");
        let new = split_leaf(&mut doc, &TreePath::root(0), 5).unwrap();
        assert_eq!(new, TreePath::root(1));
        assert_eq!(md(&doc).trim(), "Hello\n\nWorld");
    }

    #[test]
    fn split_list_item_creates_new_entry() {
        let mut doc = parse("- onetwo");
        let path = TreePath::root(0).child(PathSegment::ListEntry { entry: 0, para: 0 });
        let new = split_leaf(&mut doc, &path, 3).unwrap();
        assert_eq!(
            new,
            TreePath::root(0).child(PathSegment::ListEntry { entry: 1, para: 0 })
        );
        assert_eq!(md(&doc).trim(), "- one\n- two");
    }

    #[test]
    fn merge_paragraph_into_previous() {
        let mut doc = parse("Hello\n\nWorld");
        let (pos, off) = merge_with_previous(&mut doc, &TreePath::root(1)).unwrap();
        assert_eq!(pos, TreePath::root(0));
        assert_eq!(off, 5);
        assert_eq!(md(&doc).trim(), "HelloWorld");
    }

    #[test]
    fn removing_only_list_item_prunes_list() {
        let mut doc = parse("Before\n\n- only\n\nAfter");
        // The list is paragraphs[1]; its single item is entry 0, para 0.
        let item = TreePath::root(1).child(PathSegment::ListEntry { entry: 0, para: 0 });
        remove_node_at(&mut doc, &item);
        assert_eq!(md(&doc).trim(), "Before\n\nAfter");
    }

    #[test]
    fn toggle_checkmark_flips() {
        let mut doc = parse("- [ ] todo");
        let item = TreePath::root(0).child(PathSegment::ChecklistItem(0));
        assert_eq!(toggle_checkmark(&mut doc, &item), Some(true));
        assert!(md(&doc).contains("[x]"));
    }
}
