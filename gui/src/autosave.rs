use chrono::{DateTime, Local};
use piki_core::DocumentStore;
use piki_core::git::merge_text;
use piki_gui::content::ContentProvider;
use std::time::SystemTime;

/// What [`AutoSaveState::trigger_save`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The editor's content as it is (or nothing, when nothing had changed).
    Saved,
    /// The note had also changed on disk since it was loaded or last saved
    /// here — typically a sync bringing in an edit from another machine. Both
    /// changes were merged and the result saved; the editor should show it.
    Merged(String),
    /// The note had also changed on disk, in the same lines as the unsaved
    /// edits, so the editor's version was saved over it. With Git support the
    /// other version is still in the history.
    Overwrote,
}

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
    /// Content as last loaded from or saved to disk, to detect changes in the
    /// editor and on disk
    pub original_content: String,
    /// Current note being edited
    pub current_note: String,
    /// Bumped every time note content actually reached disk (saves, renames,
    /// deletions). Git support watches this to know when there is something
    /// new to commit; it is never reset when switching notes.
    pub save_generation: u64,
}

impl AutoSaveState {
    pub fn new() -> Self {
        AutoSaveState {
            last_change_time: None,
            last_save_time: None,
            is_saving: false,
            pending_save: false,
            original_content: String::new(),
            current_note: String::new(),
            save_generation: 0,
        }
    }

    /// Mark that content has changed
    pub fn mark_changed(&mut self) {
        self.last_change_time = Some(SystemTime::now());
        self.pending_save = true;
    }

    /// Reset state when loading a new note
    pub fn reset_for_note(&mut self, note_name: &str, content: &str) {
        self.current_note = note_name.to_string();
        self.original_content = content.to_string();
        self.last_change_time = None;
        self.last_save_time = None;
        self.is_saving = false;
        self.pending_save = false;
    }

    /// Check if the current note should be saved (not a plugin note)
    pub fn should_save(&self) -> bool {
        !self.current_note.starts_with('!')
    }

    /// Get the status text for display
    pub fn get_status_text(&self) -> String {
        if self.is_saving {
            return "Saving …".to_string();
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
    ) -> Result<SaveOutcome, String> {
        // Don't save plugin notes
        if !self.should_save() {
            self.pending_save = false;
            return Ok(SaveOutcome::Saved);
        }

        // Don't save if already saving
        if self.is_saving {
            return Ok(SaveOutcome::Saved);
        }

        // Get current content
        let current_content = editor.get_content();

        // Check if content actually changed
        if current_content == self.original_content {
            self.pending_save = false;
            return Ok(SaveOutcome::Saved);
        }

        // Mark as saving
        self.is_saving = true;
        self.pending_save = false;

        // Load the document to get the correct path
        let doc_result = store.load(&self.current_note);

        let result = match doc_result {
            Ok(mut doc) => {
                let (content, outcome) =
                    if doc.path.exists() && doc.content != self.original_content {
                        // Changed on disk behind our back: keep that change too.
                        merge_with_disk(&self.original_content, current_content, &doc.content)
                    } else {
                        (current_content, SaveOutcome::Saved)
                    };
                doc.content = content;
                store.save(&doc).map(|()| (doc.content, outcome))
            }
            Err(e) => Err(e),
        };

        // Update state based on result
        match result {
            Ok((saved, outcome)) => {
                self.last_save_time = Some(SystemTime::now());
                self.original_content = saved;
                self.is_saving = false;
                self.save_generation += 1;
                Ok(outcome)
            }
            Err(e) => {
                self.is_saving = false;
                Err(e)
            }
        }
    }
}

/// Combine the editor's unsaved edits (`ours`, made on top of `base`) with a
/// change made to the file meanwhile (`theirs`): what to save, and how.
fn merge_with_disk(base: &str, ours: String, theirs: &str) -> (String, SaveOutcome) {
    match merge_text(base, &ours, theirs) {
        Some(merged) if merged == ours => (ours, SaveOutcome::Saved),
        Some(merged) => (merged.clone(), SaveOutcome::Merged(merged)),
        None => (ours, SaveOutcome::Overwrote),
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
    let datetime: DateTime<Local> = time.into();
    format!("saved {}", datetime.format("%Y-%m-%d"))
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
    fn test_should_save_plugin_note() {
        let mut state = AutoSaveState::new();
        state.reset_for_note("!index", "");
        assert!(!state.should_save());
    }

    #[test]
    fn test_should_save_normal_note() {
        let mut state = AutoSaveState::new();
        state.reset_for_note("frontpage", "");
        assert!(state.should_save());
    }

    struct Text(String);

    impl ContentProvider for Text {
        fn get_content(&self) -> String {
            self.0.clone()
        }
    }

    /// A store with `note.md` holding `on_disk`, and autosave state for it as
    /// if it had been loaded with `loaded`.
    fn note_on_disk(tag: &str, loaded: &str, on_disk: &str) -> (DocumentStore, AutoSaveState) {
        let dir = std::env::temp_dir().join(format!("piki-autosave-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("note.md"), on_disk).unwrap();
        let mut state = AutoSaveState::new();
        state.reset_for_note("note", loaded);
        (DocumentStore::new(dir), state)
    }

    fn disk(store: &DocumentStore) -> String {
        store.load("note").unwrap().content
    }

    #[test]
    fn saving_keeps_a_change_made_on_disk_meanwhile() {
        let base = "# Todo\n\nMilk\n\nEggs\n";
        let (store, mut state) = note_on_disk("merge", base, "# Shopping\n\nMilk\n\nEggs\n");
        let edited = Text("# Todo\n\nMilk\n\nEggs\n\nButter\n".into());
        let merged = "# Shopping\n\nMilk\n\nEggs\n\nButter\n";
        assert_eq!(
            state.trigger_save(&edited, &store),
            Ok(SaveOutcome::Merged(merged.into()))
        );
        assert_eq!(disk(&store), merged);
        assert_eq!(state.original_content, merged);
        assert_eq!(state.save_generation, 1);
    }

    #[test]
    fn overlapping_edits_keep_the_editors_version() {
        let base = "# Todo\n\nMilk\n";
        let (store, mut state) = note_on_disk("overlap", base, "# Todo\n\nCheese\n");
        let edited = Text("# Todo\n\nButter\n".into());
        assert_eq!(
            state.trigger_save(&edited, &store),
            Ok(SaveOutcome::Overwrote)
        );
        assert_eq!(disk(&store), "# Todo\n\nButter\n");
    }

    #[test]
    fn unchanged_disk_is_simply_overwritten() {
        let base = "# Todo\n\nMilk\n";
        let (store, mut state) = note_on_disk("plain", base, base);
        let edited = Text("# Todo\n\nMilk and honey\n".into());
        assert_eq!(state.trigger_save(&edited, &store), Ok(SaveOutcome::Saved));
        assert_eq!(disk(&store), "# Todo\n\nMilk and honey\n");
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
