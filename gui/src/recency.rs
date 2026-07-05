//! Tracks when each page was last opened.
//!
//! The page picker lists notes most-recently-opened first and lets a double
//! `Cmd-O`/`Ctrl-O` jump straight back to the previous note. Both need to know
//! *when* pages were last opened, which the in-session navigation [`History`]
//! does not record. This is persisted as TOML next to the window-state file so
//! the ordering survives restarts — otherwise "sort by last open date" would be
//! empty on every launch. The file is scoped per wiki directory (see
//! [`crate::window_state::recent_pages_file`]) so different wikis keep separate
//! histories.
//!
//! [`History`]: crate::history::History

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecentPages {
    /// Page name -> last-opened time in milliseconds since the Unix epoch.
    #[serde(default)]
    opened: HashMap<String, i64>,
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl RecentPages {
    /// Load from `path`, returning an empty store if it is missing or corrupt.
    pub fn load(path: &Path) -> Self {
        fs::read_to_string(path)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// Persist to `path`, creating parent directories as needed.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let toml = toml::to_string_pretty(self)
            .map_err(|e| io::Error::other(format!("toml serialization error: {e}")))?;
        fs::write(path, toml)
    }

    /// Record that `page` was opened just now.
    pub fn mark_opened(&mut self, page: &str) {
        self.opened.insert(page.to_string(), now_millis());
    }

    /// The last-opened time (ms since epoch) for `page`, if it has been opened.
    pub fn last_opened(&self, page: &str) -> Option<i64> {
        self.opened.get(page).copied()
    }

    /// Move `old`'s recency entry to `new` (used when a note is renamed) so the
    /// renamed note keeps its place in the picker's ordering and no stale entry
    /// for the vanished name lingers. No-op if `old` was never opened.
    pub fn rename(&mut self, old: &str, new: &str) {
        if let Some(time) = self.opened.remove(old) {
            self.opened.insert(new.to_string(), time);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mark_opened_then_last_opened_is_set() {
        let mut r = RecentPages::default();
        assert_eq!(r.last_opened("x"), None);
        r.mark_opened("x");
        assert!(r.last_opened("x").is_some());
    }

    #[test]
    fn rename_moves_entry_preserving_time() {
        let mut r = RecentPages::default();
        r.mark_opened("old");
        let t = r.last_opened("old").unwrap();

        r.rename("old", "new");

        assert_eq!(r.last_opened("old"), None);
        assert_eq!(r.last_opened("new"), Some(t));
    }

    #[test]
    fn rename_unknown_page_is_noop() {
        let mut r = RecentPages::default();
        r.rename("missing", "new");
        assert_eq!(r.last_opened("new"), None);
    }

    #[test]
    fn roundtrips_names_with_slashes() {
        let mut r = RecentPages::default();
        r.opened.insert("project-a/standup".into(), 42);
        let toml = toml::to_string_pretty(&r).unwrap();
        let back: RecentPages = toml::from_str(&toml).unwrap();
        assert_eq!(back.last_opened("project-a/standup"), Some(42));
    }
}
