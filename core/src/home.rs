//! Where Piki keeps its things: `~/.piki` for the notes, `~/.pikirc` for the
//! configuration. Both hang off the user's home directory, which is not found
//! the same way on every platform.

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;

/// The user's home directory.
///
/// Windows has no `HOME`: the profile directory lives in `USERPROFILE`, so
/// that is what we look at first there. A POSIX shell on Windows (Git Bash,
/// MSYS) does export `HOME`, but as a path only that shell understands
/// (`/c/Users/...`), which would put the notes somewhere native binaries
/// cannot follow. Everywhere else `HOME` is the standard and comes first.
pub fn home_dir() -> Option<PathBuf> {
    // Not `home_dir_from(env::var_os)`: the closure pins the generic
    // `var_os` to a single `&str` signature that can take any lifetime.
    home_dir_from(|name: &str| env::var_os(name))
}

/// Where the notes live unless the user says otherwise: `~/.piki`.
///
/// `None` only when there is no home directory to be found. Callers must then
/// ask the user for a directory rather than fall back to a relative path,
/// which would scatter a notes dir over every working directory Piki is
/// started from.
pub fn default_notes_dir() -> Option<PathBuf> {
    home_dir().map(|home| home.join(".piki"))
}

/// [`home_dir`] against an arbitrary environment, so it can be tested without
/// mutating the process-wide one.
fn home_dir_from(lookup: impl Fn(&str) -> Option<OsString>) -> Option<PathBuf> {
    let names: &[&str] = if cfg!(windows) {
        &["USERPROFILE", "HOME"]
    } else {
        &["HOME", "USERPROFILE"]
    };

    names
        .iter()
        .filter_map(|name| lookup(name))
        .find(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An environment with exactly the given variables set.
    fn env<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<OsString> + 'a {
        move |name| {
            vars.iter()
                .find(|(k, _)| *k == name)
                .map(|(_, v)| OsString::from(*v))
        }
    }

    #[test]
    fn home_is_used_when_set() {
        let home = home_dir_from(env(&[("HOME", "/home/rob")]));
        assert_eq!(home, Some(PathBuf::from("/home/rob")));
    }

    #[test]
    fn userprofile_is_used_when_home_is_unset() {
        // The Windows default: no HOME anywhere in the environment.
        let home = home_dir_from(env(&[("USERPROFILE", r"C:\Users\rob")]));
        assert_eq!(home, Some(PathBuf::from(r"C:\Users\rob")));
    }

    #[test]
    fn empty_values_are_skipped() {
        let home = home_dir_from(env(&[("HOME", ""), ("USERPROFILE", r"C:\Users\rob")]));
        assert_eq!(home, Some(PathBuf::from(r"C:\Users\rob")));
    }

    #[test]
    fn windows_prefers_userprofile_over_a_posix_shell_home() {
        // Git Bash sets HOME to a path native binaries cannot resolve.
        let home = home_dir_from(env(&[
            ("HOME", "/c/Users/rob"),
            ("USERPROFILE", r"C:\Users\rob"),
        ]));
        let expected = if cfg!(windows) {
            r"C:\Users\rob"
        } else {
            "/c/Users/rob"
        };
        assert_eq!(home, Some(PathBuf::from(expected)));
    }

    #[test]
    fn no_home_yields_none_rather_than_a_relative_path() {
        assert_eq!(home_dir_from(env(&[])), None);
    }

    #[test]
    fn the_notes_dir_is_dot_piki_under_the_home_dir() {
        let notes = home_dir().map(|h| h.join(".piki"));
        assert_eq!(notes, default_notes_dir());
    }
}
