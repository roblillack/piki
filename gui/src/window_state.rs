use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self},
    path::{Path, PathBuf},
};

const QUALIFIER: &str = "net.roblillack";
const ORGANIZATION: &str = "Piki";
const APPLICATION: &str = "piki-gui";
const STATE_FILE_NAME: &str = "window_state.toml";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WindowGeometry {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    /// Whether fullscreen mode is active
    #[serde(default)]
    pub fullscreen: bool,
}

/// Path to a file named `name` inside the application's local data directory.
/// Used for the window-state file and the note-picker recency store so they
/// live side by side.
pub fn data_file(name: &str) -> Option<PathBuf> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.data_local_dir().join(name))
}

pub fn state_file_path() -> Option<PathBuf> {
    data_file(STATE_FILE_NAME)
}

/// Path to the note-picker recency store for a specific wiki directory.
///
/// Recency is scoped per wiki: the filename embeds a hash of the (canonical)
/// wiki path so opening notes in one wiki never reorders another wiki's picker.
pub fn recent_notes_file(wiki_dir: &Path) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};

    let canonical = wiki_dir
        .canonicalize()
        .unwrap_or_else(|_| wiki_dir.to_path_buf());
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    canonical.hash(&mut hasher);
    data_file(&format!("recent_notes_{:016x}.toml", hasher.finish()))
}

pub fn load_state(path: &Path) -> Option<WindowGeometry> {
    let contents = fs::read_to_string(path).ok()?;
    match toml::from_str::<WindowGeometry>(&contents) {
        Ok(state) => Some(state),
        Err(err) => {
            eprintln!(
                "Failed to parse window state file {}: {err}",
                path.display()
            );
            None
        }
    }
}

pub fn save_state(path: &Path, geometry: &WindowGeometry) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let toml = toml::to_string_pretty(geometry)
        .map_err(|err| io::Error::other(format!("toml serialization error: {err}")))?;

    fs::write(path, toml)
}
