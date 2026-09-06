//! The user's `~/.pikirc` configuration file, shared by the CLI and the GUI.
//!
//! ```toml
//! [aliases]
//! today = "vim daily/$(date +'%Y-%m-%d').md"
//!
//! [git]
//! enabled = true          # set to false to turn Git support off completely
//! remotes = ["laptop"]    # remotes `piki sync` and the GUI sync with
//! ```
//!
//! Only the CLI uses `[aliases]`. The `[git]` section is honored by both
//! programs; see [`GitConfig`] for the exact defaults.

use crate::home_dir;
use serde::Deserialize;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Deserialize, Debug, Default, Clone, PartialEq, Eq)]
pub struct Config {
    /// CLI aliases: `piki <alias>` runs the shell command in the notes dir.
    #[serde(default)]
    pub aliases: HashMap<String, String>,

    /// Git support settings.
    #[serde(default)]
    pub git: GitConfig,
}

/// The `[git]` section of the configuration file.
#[derive(Deserialize, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct GitConfig {
    /// Master switch. When `false`, Piki never touches Git: no commits, no
    /// syncing, no warnings about the notes dir not being a repository.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Remotes to sync with.
    ///
    /// * Not set (the default): remotes registered with `piki remote add`, or
    ///   `origin` if that exists. Syncing with neither configured is an error.
    /// * An empty list: never sync. Local commits are still created.
    /// * A list of names: exactly these remotes, all of which must exist.
    #[serde(default)]
    pub remotes: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

impl Default for GitConfig {
    fn default() -> Self {
        GitConfig {
            enabled: true,
            remotes: None,
        }
    }
}

impl Config {
    /// Location of the configuration file: `~/.pikirc`, next to the notes
    /// directory it configures — see [`home_dir`] for how that is found.
    pub fn path() -> Option<PathBuf> {
        home_dir().map(|home| home.join(".pikirc"))
    }

    /// Load the configuration from its default location. A missing file yields
    /// the defaults; an unreadable or malformed file is an error so the caller
    /// can warn instead of silently running with the wrong settings.
    pub fn load() -> Result<Config, String> {
        match Self::path() {
            Some(path) if path.exists() => Self::load_from(&path),
            _ => Ok(Config::default()),
        }
    }

    /// Load and parse the configuration file at `path`.
    pub fn load_from(path: &std::path::Path) -> Result<Config, String> {
        let contents = fs::read_to_string(path)
            .map_err(|e| format!("Failed to read {}: {e}", path.display()))?;
        Self::parse(&contents).map_err(|e| format!("Invalid config file {}: {e}", path.display()))
    }

    /// Parse configuration file contents.
    pub fn parse(contents: &str) -> Result<Config, String> {
        toml::from_str::<Config>(contents).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_has_git_enabled_and_no_remote_list() {
        let cfg = Config::parse("").unwrap();
        assert!(cfg.aliases.is_empty());
        assert!(cfg.git.enabled);
        assert_eq!(cfg.git.remotes, None);
    }

    #[test]
    fn aliases_only_config_keeps_git_defaults() {
        let cfg = Config::parse("[aliases]\ng = \"piki-gui\"\n").unwrap();
        assert_eq!(cfg.aliases["g"], "piki-gui");
        assert_eq!(cfg.git, GitConfig::default());
    }

    #[test]
    fn git_section_is_parsed() {
        let cfg =
            Config::parse("[git]\nenabled = false\nremotes = [\"laptop\", \"origin\"]\n").unwrap();
        assert!(!cfg.git.enabled);
        assert_eq!(
            cfg.git.remotes,
            Some(vec!["laptop".to_string(), "origin".to_string()])
        );
    }

    #[test]
    fn empty_remote_list_is_distinct_from_unset() {
        let cfg = Config::parse("[git]\nremotes = []\n").unwrap();
        assert_eq!(cfg.git.remotes, Some(vec![]));
    }

    #[test]
    fn unknown_keys_are_reported() {
        // A typo like `remote = ...` must not silently fall back to defaults.
        assert!(Config::parse("[git]\nremote = [\"laptop\"]\n").is_err());
    }
}
