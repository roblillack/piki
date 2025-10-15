use crate::document::DocumentStore;
use fliki_rs::content::ContentProvider;
use std::time::SystemTime;

/// State management for auto-save functionality
pub struct AutoSaveState {
    /// When the content was last changed
    pub last_change_time: Option<SystemTime>,
    /// When the content was last successfully saved
    pub last_save_time: Option<SystemTime>,
    /// Whether a save operation is currently in progress
    pub is_saving: bool,
    /// Whether a save is pending (for debounce)
    pub pending_save: bool,
    /// Original content to detect changes
    pub original_content: String,
    /// Current page being edited
    pub current_page: String,
}

impl AutoSaveState {
    pub fn new() -> Self {
        AutoSaveState {
            last_change_time: None,
            last_save_time: None,
            is_saving: false,
            pending_save: false,
            original_content: String::new(),
            current_page: String::new(),
        }
    }

    /// Mark that content has changed
    pub fn mark_changed(&mut self) {
        self.last_change_time = Some(SystemTime::now());
        self.pending_save = true;
    }

    /// Reset state when loading a new page
    pub fn reset_for_page(&mut self, page_name: &str, content: &str) {
        self.current_page = page_name.to_string();
        self.original_content = content.to_string();
        self.last_change_time = None;
        self.last_save_time = None;
        self.is_saving = false;
        self.pending_save = false;
    }

    /// Check if the current page should be saved (not a plugin page)
    pub fn should_save(&self) -> bool {
        !self.current_page.starts_with('!')
    }

    /// Get the status text for display
    pub fn get_status_text(&self) -> String {
        if self.is_saving {
            return "Saving...".to_string();
        }

        if let Some(save_time) = self.last_save_time {
            format_time_since(save_time)
        } else if self.last_change_time.is_some() {
            "not saved".to_string()
        } else {
            String::new()
        }
    }

    /// Trigger a save operation
    pub fn trigger_save<T: ContentProvider + ?Sized>(
        &mut self,
        editor: &T,
        store: &DocumentStore,
    ) -> Result<(), String> {
        // Don't save plugin pages
        if !self.should_save() {
            self.pending_save = false;
            return Ok(());
        }

        // Don't save if already saving
        if self.is_saving {
            return Ok(());
        }

        // Get current content
        let current_content = editor.get_content();

        // Check if content actually changed
        if current_content == self.original_content {
            self.pending_save = false;
            return Ok(());
        }

        // Mark as saving
        self.is_saving = true;
        self.pending_save = false;

        // Load the document to get the correct path
        let doc_result = store.load(&self.current_page);

        let result = match doc_result {
            Ok(mut doc) => {
                // Update content and save
                doc.content = current_content.clone();
                store.save(&doc)
            }
            Err(e) => Err(e),
        };

        // Update state based on result
        match result {
            Ok(()) => {
                self.last_save_time = Some(SystemTime::now());
                self.original_content = current_content;
                self.is_saving = false;
                Ok(())
            }
            Err(e) => {
                self.is_saving = false;
                Err(e)
            }
        }
    }
}

impl Default for AutoSaveState {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a time duration as a human-readable string
pub fn format_time_since(time: SystemTime) -> String {
    let now = SystemTime::now();

    match now.duration_since(time) {
        Ok(duration) => {
            let secs = duration.as_secs();

            if secs < 60 {
                "saved just now".to_string()
            } else if secs < 3600 {
                // Less than an hour
                let mins = secs / 60;
                if mins == 1 {
                    "saved 1 min ago".to_string()
                } else {
                    format!("saved {} min ago", mins)
                }
            } else if secs < 86400 {
                // Less than a day
                let hours = secs / 3600;
                if hours == 1 {
                    "saved 1 hour ago".to_string()
                } else {
                    format!("saved {} hours ago", hours)
                }
            } else if secs < 604800 {
                // Less than a week
                let days = secs / 86400;
                if days == 1 {
                    "saved 1 day ago".to_string()
                } else {
                    format!("saved {} days ago", days)
                }
            } else {
                // A week or more - show date
                format_absolute_date(time)
            }
        }
        Err(_) => "saved (time error)".to_string(),
    }
}

/// Format a time as an absolute date (YYYY-MM-DD)
fn format_absolute_date(time: SystemTime) -> String {
    use std::time::UNIX_EPOCH;

    match time.duration_since(UNIX_EPOCH) {
        Ok(duration) => {
            let secs = duration.as_secs();
            // Simple date calculation (not accounting for leap years perfectly, but close enough)
            let days = secs / 86400;
            let years_since_epoch = days / 365;
            let year = 1970 + years_since_epoch;

            // This is a simplified version - for production you'd use chrono
            // For now, just show year and approximate date
            format!("saved {}-xx-xx", year)
        }
        Err(_) => "saved (unknown date)".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autosave_state_new() {
        let state = AutoSaveState::new();
        assert!(state.last_change_time.is_none());
        assert!(state.last_save_time.is_none());
        assert!(!state.is_saving);
        assert!(!state.pending_save);
    }

    #[test]
    fn test_mark_changed() {
        let mut state = AutoSaveState::new();
        state.mark_changed();
        assert!(state.last_change_time.is_some());
        assert!(state.pending_save);
    }

    #[test]
    fn test_should_save_plugin_page() {
        let mut state = AutoSaveState::new();
        state.reset_for_page("!index", "");
        assert!(!state.should_save());
    }

    #[test]
    fn test_should_save_normal_page() {
        let mut state = AutoSaveState::new();
        state.reset_for_page("frontpage", "");
        assert!(state.should_save());
    }

    #[test]
    fn test_format_time_just_now() {
        let time = SystemTime::now();
        let formatted = format_time_since(time);
        assert_eq!(formatted, "saved just now");
    }

    #[test]
    fn test_format_time_minutes() {
        use std::time::Duration;
        let time = SystemTime::now() - Duration::from_secs(150);
        let formatted = format_time_since(time);
        assert_eq!(formatted, "saved 2 min ago");
    }

    #[test]
    fn test_format_time_hours() {
        use std::time::Duration;
        let time = SystemTime::now() - Duration::from_secs(7200);
        let formatted = format_time_since(time);
        assert_eq!(formatted, "saved 2 hours ago");
    }
}
