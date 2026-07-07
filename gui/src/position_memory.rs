//! In-memory position memory for recently visited notes.
//!
//! Remembers where the user was — both the scroll offset and the caret
//! position — in the last few notes they left, so navigating back to one — via
//! a link or the picker, not just the back/forward history — resumes where they
//! were instead of jumping to the top with the caret reset. This is
//! deliberately not persisted: it only needs to survive within a session.

use rutle::tree_path::DocumentPosition;

/// How many notes' positions are retained.
const CAPACITY: usize = 10;

/// Where the user was in a note: the scroll offset and, when known, the caret
/// position. `cursor` is `None` for a position captured from an editor without a
/// caret (e.g. a read-only plugin view); restoring it then leaves the caret at
/// the document start.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NotePosition {
    pub scroll: i32,
    pub cursor: Option<DocumentPosition>,
}

#[derive(Default)]
pub struct PositionMemory {
    /// (note name, position), most-recently-remembered first.
    entries: Vec<(String, NotePosition)>,
}

impl PositionMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `pos` for `note`, promoting it to most-recent and evicting the
    /// least-recently-remembered note once more than [`CAPACITY`] are tracked.
    pub fn remember(&mut self, note: &str, pos: NotePosition) {
        self.entries.retain(|(name, _)| name != note);
        self.entries.insert(0, (note.to_string(), pos));
        self.entries.truncate(CAPACITY);
    }

    /// The remembered position for `note`, if it is still tracked.
    pub fn get(&self, note: &str) -> Option<NotePosition> {
        self.entries
            .iter()
            .find(|(name, _)| name == note)
            .map(|(_, pos)| pos.clone())
    }

    /// Rename a tracked note in place (used when a note is renamed), preserving
    /// its remembered position and recency. No-op if `old` is not tracked.
    pub fn rename(&mut self, old: &str, new: &str) {
        if let Some((name, _)) = self.entries.iter_mut().find(|(name, _)| name == old) {
            *name = new.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scroll-only position, for the tests that only exercise recency/eviction.
    fn at(scroll: i32) -> NotePosition {
        NotePosition {
            scroll,
            cursor: None,
        }
    }

    #[test]
    fn remembers_and_returns_position() {
        let mut m = PositionMemory::new();
        assert_eq!(m.get("a"), None);
        m.remember("a", at(42));
        assert_eq!(m.get("a"), Some(at(42)));
    }

    #[test]
    fn remembers_cursor_alongside_scroll() {
        let mut m = PositionMemory::new();
        let pos = NotePosition {
            scroll: 10,
            cursor: Some(DocumentPosition::new(2, 5)),
        };
        m.remember("a", pos.clone());
        assert_eq!(m.get("a"), Some(pos));
    }

    #[test]
    fn updates_existing_position() {
        let mut m = PositionMemory::new();
        m.remember("a", at(10));
        m.remember("a", at(99));
        assert_eq!(m.get("a"), Some(at(99)));
    }

    #[test]
    fn evicts_least_recently_remembered_beyond_capacity() {
        let mut m = PositionMemory::new();
        for i in 0..CAPACITY {
            m.remember(&format!("p{i}"), at(i as i32));
        }
        // All CAPACITY notes are still tracked.
        assert_eq!(m.get("p0"), Some(at(0)));

        // One more evicts the oldest ("p0"); the newest survives.
        m.remember("new", at(123));
        assert_eq!(m.get("p0"), None);
        assert_eq!(m.get("new"), Some(at(123)));
        assert_eq!(m.get("p1"), Some(at(1)));
    }

    #[test]
    fn rename_preserves_position() {
        let mut m = PositionMemory::new();
        m.remember("old", at(42));
        m.rename("old", "new");
        assert_eq!(m.get("old"), None);
        assert_eq!(m.get("new"), Some(at(42)));
    }

    #[test]
    fn rename_unknown_note_is_noop() {
        let mut m = PositionMemory::new();
        m.remember("a", at(1));
        m.rename("missing", "new");
        assert_eq!(m.get("new"), None);
        assert_eq!(m.get("a"), Some(at(1)));
    }

    #[test]
    fn re_remembering_refreshes_recency() {
        let mut m = PositionMemory::new();
        for i in 0..CAPACITY {
            m.remember(&format!("p{i}"), at(i as i32));
        }
        // Touch the oldest so it is no longer the eviction candidate.
        m.remember("p0", at(7));
        m.remember("new", at(1));
        assert_eq!(m.get("p0"), Some(at(7))); // survived
        assert_eq!(m.get("p1"), None); // evicted instead
    }
}
