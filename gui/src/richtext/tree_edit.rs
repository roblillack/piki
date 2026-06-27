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

fn list_ordered(p: &Paragraph) -> Option<bool> {
    match p {
        Paragraph::OrderedList { .. } => Some(true),
        Paragraph::UnorderedList { .. } => Some(false),
        _ => None,
    }
}

fn new_list(ordered: bool, entries: Vec<Vec<Paragraph>>) -> Paragraph {
    if ordered {
        Paragraph::new_ordered_list().with_entries(entries)
    } else {
        Paragraph::new_unordered_list().with_entries(entries)
    }
}

/// The path of the `idx`-th sibling in the container that holds the node at `node_path`.
fn container_child_path(node_path: &TreePath, idx: usize) -> TreePath {
    match node_path.0.last() {
        Some(PathSegment::QuoteChild(_)) => {
            parent_path(node_path).child(PathSegment::QuoteChild(idx))
        }
        _ => TreePath::root(idx),
    }
}

/// Replace the single node at `node_path` with `replacement` in its container (top-level
/// paragraphs or a quote's children). Returns the base index of the replacement.
fn container_splice(
    doc: &mut Document,
    node_path: &TreePath,
    replacement: Vec<Paragraph>,
) -> Option<usize> {
    let last = node_path.0.last()?.clone();
    match last {
        PathSegment::Paragraph(i) => {
            if i >= doc.paragraphs.len() {
                return None;
            }
            doc.paragraphs.splice(i..=i, replacement);
            Some(i)
        }
        PathSegment::QuoteChild(c) => match node_at_mut(doc, &parent_path(node_path))? {
            NodeMut::Para(Paragraph::Quote { children }) => {
                if c >= children.len() {
                    return None;
                }
                children.splice(c..=c, replacement);
                Some(c)
            }
            _ => None,
        },
        _ => None,
    }
}

/// Indent the list/checklist item at `path` beneath its previous sibling (nesting it in a
/// same-kind sublist). Returns the item's new path, or `None` for the first item / a
/// non-list-item leaf.
pub fn indent_list_item(doc: &mut Document, path: &TreePath) -> Option<TreePath> {
    let last = path.0.last()?.clone();
    let pp = parent_path(path);
    match last {
        PathSegment::ListEntry { entry, para } => {
            if entry == 0 {
                return None;
            }
            let ordered = match node_at_mut(doc, &pp)? {
                NodeMut::Para(p) => list_ordered(p)?,
                _ => return None,
            };
            let moved = match node_at_mut(doc, &pp)? {
                NodeMut::Para(
                    Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                ) => {
                    if entry >= entries.len() {
                        return None;
                    }
                    entries.remove(entry)
                }
                _ => return None,
            };
            let prev = entry - 1;
            let (sub_para, sub_entry) = match node_at_mut(doc, &pp)? {
                NodeMut::Para(
                    Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                ) => {
                    let prev_entry = entries.get_mut(prev)?;
                    let reuse =
                        matches!(prev_entry.last(), Some(p) if list_ordered(p) == Some(ordered));
                    if reuse {
                        let pi = prev_entry.len() - 1;
                        match prev_entry.get_mut(pi)? {
                            Paragraph::OrderedList { entries: se }
                            | Paragraph::UnorderedList { entries: se } => {
                                se.push(moved);
                                (pi, se.len() - 1)
                            }
                            _ => return None,
                        }
                    } else {
                        prev_entry.push(new_list(ordered, vec![moved]));
                        (prev_entry.len() - 1, 0)
                    }
                }
                _ => return None,
            };
            Some(
                pp.child(PathSegment::ListEntry {
                    entry: prev,
                    para: sub_para,
                })
                .child(PathSegment::ListEntry {
                    entry: sub_entry,
                    para,
                }),
            )
        }
        PathSegment::ChecklistItem(c) => {
            if c == 0 {
                return None;
            }
            let moved = match node_at_mut(doc, &pp)? {
                NodeMut::Para(Paragraph::Checklist { items }) => {
                    if c >= items.len() {
                        return None;
                    }
                    items.remove(c)
                }
                NodeMut::Check(item) => {
                    if c >= item.children.len() {
                        return None;
                    }
                    item.children.remove(c)
                }
                _ => return None,
            };
            let child_idx = match node_at_mut(doc, &pp)? {
                NodeMut::Para(Paragraph::Checklist { items }) => {
                    let prev = items.get_mut(c - 1)?;
                    prev.children.push(moved);
                    prev.children.len() - 1
                }
                NodeMut::Check(item) => {
                    let prev = item.children.get_mut(c - 1)?;
                    prev.children.push(moved);
                    prev.children.len() - 1
                }
                _ => return None,
            };
            Some(
                pp.child(PathSegment::ChecklistItem(c - 1))
                    .child(PathSegment::ChecklistItem(child_idx)),
            )
        }
        _ => None,
    }
}

/// Outdent the list/checklist item at `path` one level: into the parent list/checklist if
/// nested, otherwise out of the list into its container (as a paragraph). Returns the new
/// path, or `None` for a non-list-item leaf.
pub fn outdent_list_item(doc: &mut Document, path: &TreePath) -> Option<TreePath> {
    let last = path.0.last()?.clone();
    let pp = parent_path(path);
    match last {
        PathSegment::ListEntry { entry, para } => match pp.0.last() {
            Some(PathSegment::ListEntry {
                entry: outer_e,
                para: outer_para,
            }) => {
                let (outer_e, outer_para) = (*outer_e, *outer_para);
                let ppp = parent_path(&pp);
                let moved = match node_at_mut(doc, &pp)? {
                    NodeMut::Para(
                        Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                    ) => {
                        if entry >= entries.len() {
                            return None;
                        }
                        entries.remove(entry)
                    }
                    _ => return None,
                };
                let inner_empty = match node_at_mut(doc, &pp)? {
                    NodeMut::Para(
                        Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                    ) => entries.is_empty(),
                    _ => false,
                };
                match node_at_mut(doc, &ppp)? {
                    NodeMut::Para(
                        Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                    ) => {
                        if inner_empty
                            && let Some(outer_entry) = entries.get_mut(outer_e)
                            && outer_para < outer_entry.len()
                        {
                            outer_entry.remove(outer_para);
                        }
                        if outer_e < entries.len() {
                            entries.insert(outer_e + 1, moved);
                        }
                    }
                    _ => return None,
                }
                Some(ppp.child(PathSegment::ListEntry {
                    entry: outer_e + 1,
                    para,
                }))
            }
            _ => exit_list_to_container(doc, &pp, entry, para),
        },
        PathSegment::ChecklistItem(c) => match pp.0.last() {
            Some(PathSegment::ChecklistItem(parent_c)) => {
                let parent_c = *parent_c;
                let ppp = parent_path(&pp);
                let moved = match node_at_mut(doc, &pp)? {
                    NodeMut::Check(item) => {
                        if c >= item.children.len() {
                            return None;
                        }
                        item.children.remove(c)
                    }
                    _ => return None,
                };
                match node_at_mut(doc, &ppp)? {
                    NodeMut::Para(Paragraph::Checklist { items }) => {
                        items.insert(parent_c + 1, moved);
                    }
                    NodeMut::Check(item) => {
                        item.children.insert(parent_c + 1, moved);
                    }
                    _ => return None,
                }
                Some(ppp.child(PathSegment::ChecklistItem(parent_c + 1)))
            }
            _ => exit_checklist_to_container(doc, &pp, c),
        },
        _ => None,
    }
}

/// Split a top-level (or quote-child) list at `list_path`, lifting entry `entry` out as
/// plain paragraph(s) in the list's container. Returns the path of the lifted leaf.
fn exit_list_to_container(
    doc: &mut Document,
    list_path: &TreePath,
    entry: usize,
    para: usize,
) -> Option<TreePath> {
    let ordered = match node_at_mut(doc, list_path)? {
        NodeMut::Para(p) => list_ordered(p)?,
        _ => return None,
    };
    let entries = match node_at_mut(doc, list_path)? {
        NodeMut::Para(
            Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
        ) => {
            if entry >= entries.len() {
                return None;
            }
            std::mem::take(entries)
        }
        _ => return None,
    };
    let mut before = entries;
    let after = before.split_off(entry + 1);
    let moved = before.pop()?; // the entry's paragraphs

    let mut replacement: Vec<Paragraph> = Vec::new();
    if !before.is_empty() {
        replacement.push(new_list(ordered, before));
    }
    let moved_start = replacement.len();
    replacement.extend(moved);
    if !after.is_empty() {
        replacement.push(new_list(ordered, after));
    }

    let base = container_splice(doc, list_path, replacement)?;
    Some(container_child_path(list_path, base + moved_start + para))
}

/// Lift a top-level checklist item out of the checklist as a plain paragraph.
fn exit_checklist_to_container(
    doc: &mut Document,
    list_path: &TreePath,
    c: usize,
) -> Option<TreePath> {
    let items = match node_at_mut(doc, list_path)? {
        NodeMut::Para(Paragraph::Checklist { items }) => {
            if c >= items.len() {
                return None;
            }
            std::mem::take(items)
        }
        _ => return None,
    };
    let mut before = items;
    let after = before.split_off(c + 1);
    let moved = before.pop()?;

    let mut replacement: Vec<Paragraph> = Vec::new();
    if !before.is_empty() {
        replacement.push(Paragraph::new_checklist().with_checklist_items(before));
    }
    let moved_start = replacement.len();
    replacement.push(Paragraph::new_text().with_content(moved.content));
    if !moved.children.is_empty() {
        replacement.push(Paragraph::new_checklist().with_checklist_items(moved.children));
    }
    if !after.is_empty() {
        replacement.push(Paragraph::new_checklist().with_checklist_items(after));
    }

    let base = container_splice(doc, list_path, replacement)?;
    Some(container_child_path(list_path, base + moved_start))
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
