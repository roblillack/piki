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

/// A new paragraph carrying `runs`, used as the trailing half of a split leaf. A split that
/// carries text into the new paragraph keeps the original block type (splitting a heading
/// mid-text yields two headings). An *empty* continuation — Enter pressed at the end of the
/// leaf — instead starts a plain paragraph, so pressing Enter after a heading drops you into
/// normal body text. Non-leaf paragraphs (lists, quotes, tables) have no inline content and
/// also fall back to plain text.
fn same_kind_paragraph(like: &Paragraph, runs: &[InlineContent]) -> Paragraph {
    let spans = inline_to_spans(runs);
    if runs.iter().all(|c| c.text_len() == 0) {
        return Paragraph::new_text().with_content(spans);
    }
    match like {
        Paragraph::Header1 { .. } => Paragraph::new_header1().with_content(spans),
        Paragraph::Header2 { .. } => Paragraph::new_header2().with_content(spans),
        Paragraph::Header3 { .. } => Paragraph::new_header3().with_content(spans),
        Paragraph::CodeBlock { .. } => Paragraph::new_code_block().with_content(spans),
        _ => Paragraph::new_text().with_content(spans),
    }
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
            // Both halves keep the original block type — splitting a heading (at any
            // offset, including the start) yields two headings, never a demoted paragraph.
            let new = same_kind_paragraph(doc.paragraphs.get(i)?, &right);
            doc.paragraphs.insert(i + 1, new);
            Some(TreePath::root(i + 1))
        }
        PathSegment::QuoteChild(c) => {
            if let NodeMut::Para(Paragraph::Quote { children }) = node_at_mut(doc, &pp)? {
                let new = same_kind_paragraph(children.get(c)?, &right);
                children.insert(c + 1, new);
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
                // the original entry; its first paragraph keeps the split leaf's kind.
                let continuation = if para < entries.get(entry)?.len() {
                    entries[entry].split_off(para + 1)
                } else {
                    Vec::new()
                };
                let new_para = same_kind_paragraph(entries.get(entry)?.get(para)?, &right);
                let mut new_entry = vec![new_para];
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

/// Append list `children` as a nested same-kind sublist of `entry`. If `entry` already ends
/// with a sublist of the matching kind, the children are merged into it instead.
fn append_children(entry: &mut Vec<Paragraph>, ordered: bool, children: Vec<Vec<Paragraph>>) {
    if let Some(last) = entry.last_mut()
        && list_ordered(last) == Some(ordered)
        && let Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries } = last
    {
        entries.extend(children);
        return;
    }
    entry.push(new_list(ordered, children));
}

/// The kind of an editable list container.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ListKind {
    Ordered,
    Unordered,
    Checklist,
}

impl ListKind {
    pub fn from_flags(ordered: bool, checklist: bool) -> ListKind {
        if checklist {
            ListKind::Checklist
        } else if ordered {
            ListKind::Ordered
        } else {
            ListKind::Unordered
        }
    }
}

/// Immutably descend to the `Paragraph` at `path` (through quotes and lists). Returns
/// `None` if the path leaves the paragraph tree (e.g. into a checklist item).
fn para_at<'a>(doc: &'a Document, path: &TreePath) -> Option<&'a Paragraph> {
    let mut segs = path.0.iter();
    let PathSegment::Paragraph(i) = segs.next()? else {
        return None;
    };
    let mut cur = doc.paragraphs.get(*i)?;
    for seg in segs {
        cur = match (cur, seg) {
            (Paragraph::Quote { children }, PathSegment::QuoteChild(c)) => children.get(*c)?,
            (
                Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                PathSegment::ListEntry { entry, para },
            ) => entries.get(*entry)?.get(*para)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// The kind of the list/checklist that *directly* contains the leaf at `path`, or `None`
/// if the leaf is not a list/checklist item.
pub fn containing_list_kind(doc: &Document, path: &TreePath) -> Option<ListKind> {
    match path.0.last()? {
        // A checklist item nests via `ChecklistItem.children`, so its container is always a
        // checklist regardless of whether the parent node is the checklist or another item.
        PathSegment::ChecklistItem(_) => Some(ListKind::Checklist),
        PathSegment::ListEntry { .. } => match para_at(doc, &parent_path(path))? {
            Paragraph::OrderedList { .. } => Some(ListKind::Ordered),
            Paragraph::UnorderedList { .. } => Some(ListKind::Unordered),
            _ => None,
        },
        _ => None,
    }
}

/// Convert one ordered/unordered list entry into a checklist item: the entry's first
/// paragraph supplies the item text, and any continuation paragraphs / nested sublists
/// become nested checklist children.
fn entry_to_checklist_item(entry: Vec<Paragraph>) -> ChecklistItem {
    let mut paras = entry.into_iter();
    let content = paras
        .next()
        .map(|p| p.content().to_vec())
        .unwrap_or_default();
    let mut item = ChecklistItem::new(false).with_content(content);
    for p in paras {
        match p {
            Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries } => {
                item.children
                    .extend(entries.into_iter().map(entry_to_checklist_item));
            }
            Paragraph::Checklist { items } => item.children.extend(items),
            other => item
                .children
                .push(ChecklistItem::new(false).with_content(other.content().to_vec())),
        }
    }
    item
}

/// Convert a checklist item into an ordered/unordered list entry: the item text becomes
/// the entry's paragraph and any nested children become a nested list of the same kind.
fn checklist_item_to_entry(item: ChecklistItem, ordered: bool) -> Vec<Paragraph> {
    let mut entry = vec![Paragraph::new_text().with_content(item.content)];
    if !item.children.is_empty() {
        let sub = item
            .children
            .into_iter()
            .map(|c| checklist_item_to_entry(c, ordered))
            .collect();
        entry.push(new_list(ordered, sub));
    }
    entry
}

/// Change the kind of the list that *directly* contains the leaf at `path` to `target`,
/// preserving the surrounding nesting. Ordered↔unordered is an in-place variant swap;
/// to/from a checklist re-shapes the entries. Returns the leaf's new path, or `None` if
/// the leaf is not a list item (or the conversion has no representation — e.g. a checklist
/// nested inside another checklist item cannot become an ordered list).
pub fn change_list_kind(doc: &mut Document, path: &TreePath, target: ListKind) -> Option<TreePath> {
    let last = path.0.last()?.clone();
    let list_path = parent_path(path);
    match last {
        PathSegment::ListEntry { entry, .. } => match target {
            ListKind::Ordered | ListKind::Unordered => {
                let want_ordered = target == ListKind::Ordered;
                let entries = take_list_entries(doc, &list_path)?;
                set_node(doc, &list_path, new_list(want_ordered, entries))?;
                Some(path.clone())
            }
            ListKind::Checklist => {
                let entries = take_list_entries(doc, &list_path)?;
                let items = entries.into_iter().map(entry_to_checklist_item).collect();
                set_node(
                    doc,
                    &list_path,
                    Paragraph::new_checklist().with_checklist_items(items),
                )?;
                Some(list_path.child(PathSegment::ChecklistItem(entry)))
            }
        },
        PathSegment::ChecklistItem(c) => match target {
            ListKind::Checklist => Some(path.clone()),
            ListKind::Ordered | ListKind::Unordered => {
                let want_ordered = target == ListKind::Ordered;
                // Only a checklist held by a `Paragraph` node (top-level or nested as a
                // sublist) can become an ordered/unordered list; one nested inside another
                // checklist item lives in a `Vec<ChecklistItem>` with no list node to swap.
                let items = match node_at_mut(doc, &list_path)? {
                    NodeMut::Para(Paragraph::Checklist { items }) => std::mem::take(items),
                    _ => return None,
                };
                let entries = items
                    .into_iter()
                    .map(|it| checklist_item_to_entry(it, want_ordered))
                    .collect();
                set_node(doc, &list_path, new_list(want_ordered, entries))?;
                Some(list_path.child(PathSegment::ListEntry { entry: c, para: 0 }))
            }
        },
        _ => None,
    }
}

/// Take (leaving empty) the entries of the ordered/unordered list at `path`.
fn take_list_entries(doc: &mut Document, path: &TreePath) -> Option<Vec<Vec<Paragraph>>> {
    match node_at_mut(doc, path)? {
        NodeMut::Para(
            Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
        ) => Some(std::mem::take(entries)),
        _ => None,
    }
}

/// Replace the whole `Paragraph` node at `path` with `replacement`.
fn set_node(doc: &mut Document, path: &TreePath, replacement: Paragraph) -> Option<()> {
    match node_at_mut(doc, path)? {
        NodeMut::Para(p) => {
            *p = replacement;
            Some(())
        }
        _ => None,
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
                let ordered = match node_at_mut(doc, &pp)? {
                    NodeMut::Para(p) => list_ordered(p)?,
                    _ => return None,
                };
                // Detach the outdented entry together with the siblings that follow it in
                // the inner list; those followers become the outdented item's children
                // (they were visually nested under it and stay grouped with it).
                let (mut moved, following) = match node_at_mut(doc, &pp)? {
                    NodeMut::Para(
                        Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                    ) => {
                        if entry >= entries.len() {
                            return None;
                        }
                        let following = entries.split_off(entry + 1);
                        (entries.remove(entry), following)
                    }
                    _ => return None,
                };
                let inner_empty = match node_at_mut(doc, &pp)? {
                    NodeMut::Para(
                        Paragraph::OrderedList { entries } | Paragraph::UnorderedList { entries },
                    ) => entries.is_empty(),
                    _ => false,
                };
                if !following.is_empty() {
                    append_children(&mut moved, ordered, following);
                }
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
                // Detach the outdented item with its following siblings; the followers
                // become its own children (appended after any children it already has).
                let (mut moved, following) = match node_at_mut(doc, &pp)? {
                    NodeMut::Check(item) => {
                        if c >= item.children.len() {
                            return None;
                        }
                        let following = item.children.split_off(c + 1);
                        (item.children.remove(c), following)
                    }
                    _ => return None,
                };
                moved.children.extend(following);
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
    fn split_heading_keeps_block_type_on_both_halves() {
        // Splitting in the middle of a heading yields two headings.
        let mut doc = parse("# HeaderText");
        let new = split_leaf(&mut doc, &TreePath::root(0), 6).unwrap();
        assert_eq!(new, TreePath::root(1));
        assert!(matches!(doc.paragraphs[0], Paragraph::Header1 { .. }));
        assert!(matches!(doc.paragraphs[1], Paragraph::Header1 { .. }));
        assert_eq!(md(&doc).trim(), "# Header\n\n# Text");
    }

    #[test]
    fn split_heading_at_end_starts_plain_paragraph() {
        // Enter at the end of a heading drops into a plain body paragraph.
        let mut doc = parse("# Title");
        let new = split_leaf(&mut doc, &TreePath::root(0), 5).unwrap();
        assert_eq!(new, TreePath::root(1));
        assert!(matches!(doc.paragraphs[0], Paragraph::Header1 { .. }));
        assert!(matches!(doc.paragraphs[1], Paragraph::Text { .. }));
    }

    #[test]
    fn split_heading_at_start_keeps_both_headings() {
        // At the start, the (empty) leaf above and the content below are both headings —
        // the moved content is never demoted to a plain paragraph.
        let mut doc = parse("# Title");
        let new = split_leaf(&mut doc, &TreePath::root(0), 0).unwrap();
        assert_eq!(new, TreePath::root(1));
        assert!(matches!(doc.paragraphs[0], Paragraph::Header1 { .. }));
        assert!(doc.paragraphs[0].content().is_empty());
        assert!(matches!(doc.paragraphs[1], Paragraph::Header1 { .. }));
        assert_eq!(doc.paragraphs[1].content()[0].text, "Title");
    }

    #[test]
    fn change_nested_list_kind_preserves_outer_level() {
        // Outer ordered list with a nested ordered list under its first entry.
        let mut doc = parse("1. one\n   1. two");
        let nested = TreePath::root(0)
            .child(PathSegment::ListEntry { entry: 0, para: 1 })
            .child(PathSegment::ListEntry { entry: 0, para: 0 });
        let new = change_list_kind(&mut doc, &nested, ListKind::Unordered).unwrap();
        assert_eq!(new, nested);
        // Outer list stays ordered; only the nested list became unordered.
        let Paragraph::OrderedList { entries } = &doc.paragraphs[0] else {
            panic!("outer list should stay ordered");
        };
        assert!(matches!(entries[0][1], Paragraph::UnorderedList { .. }));
    }

    #[test]
    fn outdent_adopts_following_siblings_as_children() {
        // a > {x, y, z}. Outdenting x lifts it beside a and makes y, z its children.
        let mut doc = parse("- a\n  - x\n  - y\n  - z");
        let x = TreePath::root(0)
            .child(PathSegment::ListEntry { entry: 0, para: 1 })
            .child(PathSegment::ListEntry { entry: 0, para: 0 });
        let new = outdent_list_item(&mut doc, &x).unwrap();
        assert_eq!(
            new,
            TreePath::root(0).child(PathSegment::ListEntry { entry: 1, para: 0 })
        );
        let Paragraph::UnorderedList { entries } = &doc.paragraphs[0] else {
            panic!("expected an unordered list");
        };
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].len(), 1); // "a" no longer has a sublist
        let Paragraph::UnorderedList { entries: adopted } = &entries[1][1] else {
            panic!("x should have adopted a sublist of its followers");
        };
        assert_eq!(adopted.len(), 2); // y and z
    }

    #[test]
    fn outdent_middle_item_keeps_preceding_siblings() {
        // a > {x, y, z}. Outdenting y leaves x under a, and z becomes y's child.
        let mut doc = parse("- a\n  - x\n  - y\n  - z");
        let y = TreePath::root(0)
            .child(PathSegment::ListEntry { entry: 0, para: 1 })
            .child(PathSegment::ListEntry { entry: 1, para: 0 });
        outdent_list_item(&mut doc, &y).unwrap();
        let Paragraph::UnorderedList { entries } = &doc.paragraphs[0] else {
            panic!("expected an unordered list");
        };
        assert_eq!(entries.len(), 2);
        // "a" keeps a sublist containing just x.
        let Paragraph::UnorderedList {
            entries: a_children,
        } = &entries[0][1]
        else {
            panic!("a should still have a sublist with x");
        };
        assert_eq!(a_children.len(), 1);
        // y adopted z.
        let Paragraph::UnorderedList {
            entries: y_children,
        } = &entries[1][1]
        else {
            panic!("y should have adopted z");
        };
        assert_eq!(y_children.len(), 1);
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
