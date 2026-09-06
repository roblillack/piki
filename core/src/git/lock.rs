//! A cross-process lock so the GUI and the CLI never commit or sync the same
//! repository at the same time.
//!
//! Git itself only guards the index (`.git/index.lock`), which would turn a
//! concurrent run into a cryptic "index is locked" failure half-way through a
//! sync. This lock is taken for the whole operation instead and produces a
//! clear message. It is a plain lock file created with `O_EXCL` semantics in
//! the repository's `.git` directory; a stale file left behind by a crashed
//! process is reclaimed after [`STALE_AFTER`].

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Name of the lock file inside `.git/`.
pub const LOCK_FILE_NAME: &str = "piki.lock";

/// A lock older than this is assumed to be left over from a crashed process.
pub const STALE_AFTER: Duration = Duration::from_secs(10 * 60);

/// Held for the duration of a commit or sync; removes the lock file on drop.
#[derive(Debug)]
pub struct SyncLock {
    path: PathBuf,
}

impl SyncLock {
    /// Take the lock for the repository whose `.git` directory is `git_dir`.
    pub fn acquire(git_dir: &Path) -> Result<SyncLock, String> {
        Self::acquire_path(git_dir.join(LOCK_FILE_NAME), STALE_AFTER)
    }

    fn acquire_path(path: PathBuf, stale_after: Duration) -> Result<SyncLock, String> {
        for attempt in 0..2 {
            match fs::File::create_new(&path) {
                Ok(mut file) => {
                    use std::io::Write;
                    let _ = writeln!(file, "{}", std::process::id());
                    return Ok(SyncLock { path });
                }
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    if attempt == 0 && Self::is_stale(&path, stale_after) {
                        // Reclaim a lock left behind by a crashed process; if the
                        // removal races with another reclaimer that is fine, the
                        // second `create_new` decides who wins.
                        let _ = fs::remove_file(&path);
                        continue;
                    }
                    return Err("Another Piki process is committing or syncing this notes \
                         directory right now; please try again in a moment."
                        .to_string());
                }
                Err(e) => {
                    return Err(format!(
                        "Failed to create lock file {}: {e}",
                        path.display()
                    ));
                }
            }
        }
        Err("Another Piki process is committing or syncing this notes directory.".to_string())
    }

    fn is_stale(path: &Path, stale_after: Duration) -> bool {
        fs::metadata(path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
            .map(|age| age > stale_after)
            .unwrap_or(false)
    }
}

impl Drop for SyncLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("piki-lock-{tag}-{nanos}"));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lock_is_exclusive_and_released_on_drop() {
        let dir = temp_dir("exclusive");
        let first = SyncLock::acquire(&dir).unwrap();
        let err = SyncLock::acquire(&dir).unwrap_err();
        assert!(err.contains("Another Piki process"), "{err}");
        drop(first);
        assert!(SyncLock::acquire(&dir).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stale_lock_is_reclaimed() {
        let dir = temp_dir("stale");
        let path = dir.join(LOCK_FILE_NAME);
        fs::write(&path, "12345\n").unwrap();
        // With a zero staleness threshold the just-written file already counts
        // as abandoned and gets replaced.
        let lock = SyncLock::acquire_path(path.clone(), Duration::ZERO).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap().trim(),
            std::process::id().to_string()
        );
        drop(lock);
        assert!(!path.exists());
        fs::remove_dir_all(&dir).ok();
    }
}
