//! A cross-process lock so the GUI and the CLI never commit or sync the same
//! repository at the same time.
//!
//! Git itself only guards the index (`.git/index.lock`), which would turn a
//! concurrent run into a cryptic "index is locked" failure half-way through a
//! sync. This lock is taken for the whole operation instead and produces a
//! clear message. It is an operating-system file lock on a file in the
//! repository's `.git` directory, so it goes away with the process holding it:
//! a crash, or the GUI exiting while a sync is still running, never leaves a
//! stale lock behind. The file itself stays; only the lock on it matters.

use std::fs::{self, TryLockError};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Name of the lock file inside `.git/`.
pub const LOCK_FILE_NAME: &str = "piki.lock";

/// How long to wait for the lock before giving up. Long enough to ride out a
/// commit by the other program, and the moment a process spawned elsewhere
/// holds an inherited copy of the lock before it starts its own program.
pub const WAIT_FOR_LOCK: Duration = Duration::from_secs(2);

/// Held for the duration of a commit or sync; released on drop.
#[derive(Debug)]
pub struct SyncLock {
    _file: fs::File,
}

impl SyncLock {
    /// Take the lock for the repository whose `.git` directory is `git_dir`,
    /// waiting up to [`WAIT_FOR_LOCK`] for another process to release it.
    pub fn acquire(git_dir: &Path) -> Result<SyncLock, String> {
        Self::acquire_path(git_dir.join(LOCK_FILE_NAME), WAIT_FOR_LOCK)
    }

    fn acquire_path(path: PathBuf, wait: Duration) -> Result<SyncLock, String> {
        // Never truncate or delete the file: another process may be holding a
        // lock on it, and replacing the file would let two processes "hold"
        // locks on two different files.
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .map_err(|e| format!("Failed to open lock file {}: {e}", path.display()))?;
        let deadline = Instant::now() + wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(SyncLock { _file: file }),
                Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(TryLockError::WouldBlock) => {
                    return Err("Another Piki process is committing or syncing this notes \
                         directory right now; please try again in a moment."
                        .to_string());
                }
                Err(TryLockError::Error(e)) => {
                    return Err(format!("Failed to lock {}: {e}", path.display()));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

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
        let path = dir.join(LOCK_FILE_NAME);
        let first = SyncLock::acquire(&dir).unwrap();
        let err = SyncLock::acquire_path(path.clone(), Duration::ZERO).unwrap_err();
        assert!(err.contains("Another Piki process"), "{err}");
        drop(first);
        assert!(SyncLock::acquire_path(path, Duration::ZERO).is_ok());
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn waits_for_a_lock_released_meanwhile() {
        let dir = temp_dir("wait");
        let first = SyncLock::acquire(&dir).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(200));
            drop(first);
        });
        assert!(SyncLock::acquire(&dir).is_ok());
        releaser.join().unwrap();
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn leftover_lock_file_does_not_block() {
        // What a crashed process leaves behind: the file, but no lock on it.
        let dir = temp_dir("leftover");
        fs::write(dir.join(LOCK_FILE_NAME), "12345\n").unwrap();
        assert!(SyncLock::acquire(&dir).is_ok());
        fs::remove_dir_all(&dir).ok();
    }
}
