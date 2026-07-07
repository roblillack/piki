#![allow(dead_code)]

use crate::position_memory::NotePosition;

const MAX_HISTORY_SIZE: usize = 100;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub note_name: String,
    /// Where the user was in this note (scroll offset + caret) when they last
    /// left it, restored when back/forward navigates here again.
    pub position: NotePosition,
}

impl HistoryEntry {
    pub fn new(note_name: String, position: NotePosition) -> Self {
        HistoryEntry {
            note_name,
            position,
        }
    }
}

#[derive(Debug)]
pub struct History {
    entries: Vec<HistoryEntry>,
    current_index: Option<usize>,
}

impl History {
    pub fn new() -> Self {
        History {
            entries: Vec::new(),
            current_index: None,
        }
    }

    /// Add a new note to history
    /// This clears any forward history and adds the new entry
    pub fn push(&mut self, note_name: String, position: NotePosition) {
        // If we're in the middle of history, truncate everything after current position
        if let Some(idx) = self.current_index {
            self.entries.truncate(idx + 1);
        }

        // Add new entry
        self.entries.push(HistoryEntry::new(note_name, position));

        // Limit history size
        if self.entries.len() > MAX_HISTORY_SIZE {
            self.entries.remove(0);
        }

        // Update current index to point to the new entry
        self.current_index = Some(self.entries.len() - 1);
    }

    /// Rename every entry that points at `old` to `new`, so back/forward
    /// navigation follows a note that was renamed instead of resurrecting its
    /// former (now non-existent) name as an empty note.
    pub fn rename_note(&mut self, old: &str, new: &str) {
        for entry in &mut self.entries {
            if entry.note_name == old {
                entry.note_name = new.to_string();
            }
        }
    }

    /// Update the remembered position (scroll offset + caret) of the current entry
    pub fn update_position(&mut self, position: NotePosition) {
        if let Some(idx) = self.current_index
            && let Some(entry) = self.entries.get_mut(idx)
        {
            entry.position = position;
        }
    }

    /// Check if we can navigate back
    pub fn can_go_back(&self) -> bool {
        if let Some(idx) = self.current_index {
            idx > 0
        } else {
            false
        }
    }

    /// Check if we can navigate forward
    pub fn can_go_forward(&self) -> bool {
        if let Some(idx) = self.current_index {
            idx < self.entries.len() - 1
        } else {
            false
        }
    }

    /// Navigate back one step
    /// Returns the entry we should navigate to, or None if we can't go back
    pub fn go_back(&mut self) -> Option<&HistoryEntry> {
        if let Some(idx) = self.current_index
            && idx > 0
        {
            self.current_index = Some(idx - 1);
            return self.entries.get(idx - 1);
        }
        None
    }

    /// Navigate forward one step
    /// Returns the entry we should navigate to, or None if we can't go forward
    pub fn go_forward(&mut self) -> Option<&HistoryEntry> {
        if let Some(idx) = self.current_index
            && idx < self.entries.len() - 1
        {
            self.current_index = Some(idx + 1);
            return self.entries.get(idx + 1);
        }
        None
    }

    /// Get the current entry without navigating
    pub fn current(&self) -> Option<&HistoryEntry> {
        if let Some(idx) = self.current_index {
            self.entries.get(idx)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rutle::tree_path::DocumentPosition;

    /// A scroll-only position, for tests that only exercise navigation ordering.
    fn scroll(n: i32) -> NotePosition {
        NotePosition {
            scroll: n,
            cursor: None,
        }
    }

    #[test]
    fn test_push_and_navigate() {
        let mut history = History::new();

        history.push("note1".to_string(), scroll(0));
        history.push("note2".to_string(), scroll(10));
        history.push("note3".to_string(), scroll(20));

        assert_eq!(history.current().unwrap().note_name, "note3");
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        history.go_back();
        assert_eq!(history.current().unwrap().note_name, "note2");
        assert_eq!(history.current().unwrap().position.scroll, 10);
        assert!(history.can_go_back());
        assert!(history.can_go_forward());

        history.go_forward();
        assert_eq!(history.current().unwrap().note_name, "note3");
    }

    #[test]
    fn test_push_clears_forward_history() {
        let mut history = History::new();

        history.push("note1".to_string(), scroll(0));
        history.push("note2".to_string(), scroll(0));
        history.push("note3".to_string(), scroll(0));
        history.go_back();
        history.go_back();

        // Now at note1, with note2 and note3 ahead
        assert_eq!(history.current().unwrap().note_name, "note1");

        // Push new note should clear note2 and note3
        history.push("note4".to_string(), scroll(0));
        assert_eq!(history.current().unwrap().note_name, "note4");
        assert!(!history.can_go_forward());
    }

    #[test]
    fn test_max_size() {
        let mut history = History::new();

        // Add more than MAX_HISTORY_SIZE entries
        for i in 0..150 {
            history.push(format!("note{}", i), scroll(i));
        }

        // Should only keep the last 100
        assert_eq!(history.entries.len(), MAX_HISTORY_SIZE);
        assert_eq!(history.current().unwrap().note_name, "note149");
    }

    #[test]
    fn test_rename_note_updates_all_matching_entries() {
        let mut history = History::new();
        history.push("untitled_x".to_string(), scroll(0));
        history.push("other".to_string(), scroll(0));
        history.push("untitled_x".to_string(), scroll(0));

        history.rename_note("untitled_x", "real-name");

        // Every occurrence of the old name follows the rename; other entries are
        // untouched.
        assert_eq!(history.current().unwrap().note_name, "real-name");
        history.go_back();
        assert_eq!(history.current().unwrap().note_name, "other");
        history.go_back();
        assert_eq!(history.current().unwrap().note_name, "real-name");
    }

    #[test]
    fn test_update_position() {
        let mut history = History::new();

        history.push("note1".to_string(), scroll(0));
        assert_eq!(history.current().unwrap().position.scroll, 0);

        // Updating writes both the scroll offset and the caret onto the entry.
        let updated = NotePosition {
            scroll: 42,
            cursor: Some(DocumentPosition::new(1, 3)),
        };
        history.update_position(updated.clone());
        assert_eq!(history.current().unwrap().position, updated);
    }
}
