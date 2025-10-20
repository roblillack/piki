use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, ErrorKind},
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
}

pub fn state_file_path() -> Option<PathBuf> {
    ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
        .map(|dirs| dirs.data_local_dir().join(STATE_FILE_NAME))
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

    let toml = toml::to_string_pretty(geometry).map_err(|err| {
        io::Error::new(ErrorKind::Other, format!("toml serialization error: {err}"))
    })?;

    fs::write(path, toml)
}
