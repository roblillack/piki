//! In-memory scroll-position memory for recently visited notes.
//!
//! Remembers the scroll offset of the last few notes the user left, so
//! navigating back to one — via a link or the picker, not just the back/forward
//! history — resumes where they were instead of jumping to the top. This is
//! deliberately not persisted: it only needs to survive within a session.

/// How many notes' scroll positions are retained.
const CAPACITY: usize = 10;

#[derive(Default)]
pub struct ScrollMemory {
    /// (page name, scroll position), most-recently-remembered first.
    entries: Vec<(String, i32)>,
}

impl ScrollMemory {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record `pos` for `page`, promoting it to most-recent and evicting the
    /// least-recently-remembered note once more than [`CAPACITY`] are tracked.
    pub fn remember(&mut self, page: &str, pos: i32) {
        self.entries.retain(|(name, _)| name != page);
        self.entries.insert(0, (page.to_string(), pos));
        self.entries.truncate(CAPACITY);
    }

    /// The remembered scroll position for `page`, if it is still tracked.
    pub fn get(&self, page: &str) -> Option<i32> {
        self.entries
            .iter()
            .find(|(name, _)| name == page)
            .map(|(_, pos)| *pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remembers_and_returns_position() {
        let mut m = ScrollMemory::new();
        assert_eq!(m.get("a"), None);
        m.remember("a", 42);
        assert_eq!(m.get("a"), Some(42));
    }

    #[test]
    fn updates_existing_position() {
        let mut m = ScrollMemory::new();
        m.remember("a", 10);
        m.remember("a", 99);
        assert_eq!(m.get("a"), Some(99));
    }

    #[test]
    fn evicts_least_recently_remembered_beyond_capacity() {
        let mut m = ScrollMemory::new();
        for i in 0..CAPACITY {
            m.remember(&format!("p{i}"), i as i32);
        }
        // All CAPACITY notes are still tracked.
        assert_eq!(m.get("p0"), Some(0));

        // One more evicts the oldest ("p0"); the newest survives.
        m.remember("new", 123);
        assert_eq!(m.get("p0"), None);
        assert_eq!(m.get("new"), Some(123));
        assert_eq!(m.get("p1"), Some(1));
    }

    #[test]
    fn re_remembering_refreshes_recency() {
        let mut m = ScrollMemory::new();
        for i in 0..CAPACITY {
            m.remember(&format!("p{i}"), i as i32);
        }
        // Touch the oldest so it is no longer the eviction candidate.
        m.remember("p0", 7);
        m.remember("new", 1);
        assert_eq!(m.get("p0"), Some(7)); // survived
        assert_eq!(m.get("p1"), None); // evicted instead
    }
}
