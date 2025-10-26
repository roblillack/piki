const MAX_HISTORY_SIZE: usize = 100;

#[derive(Debug, Clone)]
pub struct HistoryEntry {
    pub page_name: String,
    pub scroll_position: i32,
}

impl HistoryEntry {
    pub fn new(page_name: String, scroll_position: i32) -> Self {
        HistoryEntry {
            page_name,
            scroll_position,
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

    /// Add a new page to history
    /// This clears any forward history and adds the new entry
    pub fn push(&mut self, page_name: String, scroll_position: i32) {
        // If we're in the middle of history, truncate everything after current position
        if let Some(idx) = self.current_index {
            self.entries.truncate(idx + 1);
        }

        // Add new entry
        self.entries
            .push(HistoryEntry::new(page_name, scroll_position));

        // Limit history size
        if self.entries.len() > MAX_HISTORY_SIZE {
            self.entries.remove(0);
        }

        // Update current index to point to the new entry
        self.current_index = Some(self.entries.len() - 1);
    }

    /// Update the scroll position of the current entry
    pub fn update_scroll_position(&mut self, scroll_position: i32) {
        if let Some(idx) = self.current_index
            && let Some(entry) = self.entries.get_mut(idx) {
                entry.scroll_position = scroll_position;
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
            && idx > 0 {
                self.current_index = Some(idx - 1);
                return self.entries.get(idx - 1);
            }
        None
    }

    /// Navigate forward one step
    /// Returns the entry we should navigate to, or None if we can't go forward
    pub fn go_forward(&mut self) -> Option<&HistoryEntry> {
        if let Some(idx) = self.current_index
            && idx < self.entries.len() - 1 {
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

    #[test]
    fn test_push_and_navigate() {
        let mut history = History::new();

        history.push("page1".to_string(), 0);
        history.push("page2".to_string(), 10);
        history.push("page3".to_string(), 20);

        assert_eq!(history.current().unwrap().page_name, "page3");
        assert!(history.can_go_back());
        assert!(!history.can_go_forward());

        history.go_back();
        assert_eq!(history.current().unwrap().page_name, "page2");
        assert_eq!(history.current().unwrap().scroll_position, 10);
        assert!(history.can_go_back());
        assert!(history.can_go_forward());

        history.go_forward();
        assert_eq!(history.current().unwrap().page_name, "page3");
    }

    #[test]
    fn test_push_clears_forward_history() {
        let mut history = History::new();

        history.push("page1".to_string(), 0);
        history.push("page2".to_string(), 0);
        history.push("page3".to_string(), 0);
        history.go_back();
        history.go_back();

        // Now at page1, with page2 and page3 ahead
        assert_eq!(history.current().unwrap().page_name, "page1");

        // Push new page should clear page2 and page3
        history.push("page4".to_string(), 0);
        assert_eq!(history.current().unwrap().page_name, "page4");
        assert!(!history.can_go_forward());
    }

    #[test]
    fn test_max_size() {
        let mut history = History::new();

        // Add more than MAX_HISTORY_SIZE entries
        for i in 0..150 {
            history.push(format!("page{}", i), i as i32);
        }

        // Should only keep the last 100
        assert_eq!(history.entries.len(), MAX_HISTORY_SIZE);
        assert_eq!(history.current().unwrap().page_name, "page149");
    }

    #[test]
    fn test_update_scroll_position() {
        let mut history = History::new();

        history.push("page1".to_string(), 0);
        assert_eq!(history.current().unwrap().scroll_position, 0);

        history.update_scroll_position(42);
        assert_eq!(history.current().unwrap().scroll_position, 42);
    }
}
