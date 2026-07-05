//! Persisted user preferences, stored as `settings.toml` in the application's
//! local data directory (alongside `window_state.toml` and the recency stores).
//!
//! Kept deliberately small: load returns defaults on any error (missing file,
//! parse failure) so a corrupt or absent file never blocks startup, and unknown
//! or missing fields fall back to their defaults via `#[serde(default)]`.

use crate::window_state::data_file;
use serde::{Deserialize, Serialize};
use std::fs;

const SETTINGS_FILE_NAME: &str = "settings.toml";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Whether the editor pauses for two caret positions at inline-style
    /// boundaries (the "insert before or after the style" behavior). On by
    /// default; mirrors `rutle::Editor::set_style_boundary_stops`.
    pub style_boundary_stops: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            style_boundary_stops: true,
        }
    }
}

impl Settings {
    /// Load the settings, falling back to defaults on any error.
    pub fn load() -> Self {
        let Some(path) = data_file(SETTINGS_FILE_NAME) else {
            return Self::default();
        };
        let Ok(contents) = fs::read_to_string(&path) else {
            return Self::default();
        };
        match toml::from_str::<Settings>(&contents) {
            Ok(settings) => settings,
            Err(err) => {
                eprintln!("Failed to parse settings file {}: {err}", path.display());
                Self::default()
            }
        }
    }

    /// Persist the settings, logging (but not propagating) any I/O error.
    pub fn save(&self) {
        let Some(path) = data_file(SETTINGS_FILE_NAME) else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(err) = fs::create_dir_all(parent)
        {
            eprintln!("Failed to create settings directory: {err}");
            return;
        }
        match toml::to_string_pretty(self) {
            Ok(toml) => {
                if let Err(err) = fs::write(&path, toml) {
                    eprintln!("Failed to write settings file {}: {err}", path.display());
                }
            }
            Err(err) => eprintln!("Failed to serialize settings: {err}"),
        }
    }
}
