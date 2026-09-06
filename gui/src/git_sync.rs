//! Git support for the GUI: a background worker that commits and syncs, plus
//! the main-thread bookkeeping that decides *when* to do so.
//!
//! All repository access happens on one worker thread, so the UI never blocks
//! on the network and the GUI never runs two Git operations at once (the
//! cross-process lock in `piki_core::git` guards against the CLI). The main
//! thread talks to the worker through channels and polls for results from the
//! app's animation timer.
//!
//! Commits are debounced: after the last successful save of an edit we wait
//! [`COMMIT_DELAY`] before committing, so a burst of autosaves while typing
//! becomes one commit. Leaving a note or closing the window commits at once.

use piki_core::GitConfig;
use piki_core::git::{CommitSummary, Repo, SyncReport};
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long after the last save an edit waits before it is committed.
pub const COMMIT_DELAY: Duration = Duration::from_secs(60);

/// How often the GUI syncs with the configured remotes on its own.
pub const SYNC_INTERVAL: Duration = Duration::from_secs(15 * 60);

/// Delay before the first automatic sync after launch.
pub const FIRST_SYNC_DELAY: Duration = Duration::from_secs(3);

/// How long closing the window waits for the worker to finish its queue (the
/// final commit and, at worst, a sync that is mid-flight).
pub const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

enum Job {
    Commit,
    Sync,
    Shutdown,
}

/// What the worker reports back.
pub enum Event {
    /// A sync has begun (after its preceding commit).
    SyncStarted,
    Committed(Result<Option<CommitSummary>, String>),
    Synced(Result<SyncReport, String>),
}

/// Handle to the background thread owning the repository.
pub struct GitWorker {
    jobs: Sender<Job>,
    events: Receiver<Event>,
    thread: Option<JoinHandle<()>>,
}

impl GitWorker {
    /// Start a worker for the repository at `dir`. `Ok(None)` when `dir` is not
    /// a Git repository.
    pub fn spawn(dir: PathBuf, config: GitConfig) -> Result<Option<GitWorker>, String> {
        let Some(repo) = Repo::open(&dir)? else {
            return Ok(None);
        };
        let (jobs, job_rx) = channel::<Job>();
        let (event_tx, events) = channel::<Event>();
        let thread = std::thread::Builder::new()
            .name("piki-git".into())
            .spawn(move || worker_loop(repo, config, job_rx, event_tx))
            .map_err(|e| format!("Failed to start Git worker thread: {e}"))?;
        Ok(Some(GitWorker {
            jobs,
            events,
            thread: Some(thread),
        }))
    }

    pub fn request_commit(&self) {
        let _ = self.jobs.send(Job::Commit);
    }

    pub fn request_sync(&self) {
        let _ = self.jobs.send(Job::Sync);
    }

    /// Drain all events the worker has produced so far.
    pub fn poll(&self) -> Vec<Event> {
        let mut out = Vec::new();
        // Stops on Empty as well as Disconnected.
        while let Ok(ev) = self.events.try_recv() {
            out.push(ev);
        }
        out
    }

    /// Queue a final commit, tell the worker to stop and wait (up to `timeout`)
    /// for it to work through its queue. Returns whether it finished in time.
    pub fn shutdown(mut self, timeout: Duration) -> bool {
        let _ = self.jobs.send(Job::Commit);
        let _ = self.jobs.send(Job::Shutdown);
        let Some(thread) = self.thread.take() else {
            return true;
        };
        let deadline = Instant::now() + timeout;
        while !thread.is_finished() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        if thread.is_finished() {
            let _ = thread.join();
            true
        } else {
            false
        }
    }
}

fn worker_loop(repo: Repo, config: GitConfig, jobs: Receiver<Job>, events: Sender<Event>) {
    while let Ok(job) = jobs.recv() {
        match job {
            Job::Commit => {
                let result = repo.commit_changes();
                if events.send(Event::Committed(result)).is_err() {
                    break;
                }
            }
            Job::Sync => {
                if events.send(Event::SyncStarted).is_err() {
                    break;
                }
                // Remotes are resolved per run so ones added with the CLI while
                // the GUI is open are picked up.
                let result = repo
                    .sync_remotes(&config)
                    .and_then(|remotes| repo.sync(&remotes));
                if events.send(Event::Synced(result)).is_err() {
                    break;
                }
            }
            Job::Shutdown => break,
        }
        fltk::app::awake();
    }
}

/// Main-thread state: decides when to commit, tracks whether a sync is running.
pub struct GitState {
    worker: Option<GitWorker>,
    /// Save generation (see `AutoSaveState::save_generation`) last observed.
    seen_generation: u64,
    /// Set when a save happened that has not been committed yet; the commit
    /// fires once `COMMIT_DELAY` has passed without another save.
    pending_since: Option<Instant>,
    /// Whether a sync is in flight on the worker.
    pub syncing: bool,
    /// The last sync error, until a sync succeeds.
    pub last_error: Option<String>,
    /// Why Git support is unavailable (disabled, or not a repository).
    pub unavailable_reason: Option<String>,
}

impl GitState {
    pub fn new(worker: Option<GitWorker>, unavailable_reason: Option<String>) -> Self {
        GitState {
            worker,
            seen_generation: 0,
            pending_since: None,
            syncing: false,
            last_error: None,
            unavailable_reason,
        }
    }

    pub fn is_available(&self) -> bool {
        self.worker.is_some()
    }

    /// Called from the animation timer with the autosave's save generation:
    /// notices new saves and fires the debounced commit.
    pub fn observe(&mut self, generation: u64, now: Instant) {
        if generation != self.seen_generation {
            self.seen_generation = generation;
            self.pending_since = Some(now);
        }
        if let Some(since) = self.pending_since
            && now.duration_since(since) >= COMMIT_DELAY
        {
            self.commit_now(generation);
        }
    }

    /// Commit right away if anything was saved since the last commit request.
    /// Used when leaving a note and when closing the window.
    pub fn commit_now(&mut self, generation: u64) {
        let changed = generation != self.seen_generation || self.pending_since.is_some();
        self.seen_generation = generation;
        self.pending_since = None;
        if changed && let Some(worker) = &self.worker {
            worker.request_commit();
        }
    }

    /// Queue a sync (which commits first). Returns false when Git support is
    /// unavailable or a sync is already running.
    pub fn request_sync(&mut self) -> bool {
        if self.syncing {
            return false;
        }
        let Some(worker) = &self.worker else {
            return false;
        };
        self.pending_since = None;
        worker.request_sync();
        true
    }

    /// Events from the worker, with the sync flag kept in step.
    pub fn poll(&mut self) -> Vec<Event> {
        let Some(worker) = &self.worker else {
            return Vec::new();
        };
        let events = worker.poll();
        for ev in &events {
            match ev {
                Event::SyncStarted => self.syncing = true,
                Event::Synced(result) => {
                    self.syncing = false;
                    self.last_error = match result {
                        Ok(report) if report.has_errors() => Some(report.summary()),
                        Ok(_) => None,
                        Err(e) => Some(e.clone()),
                    };
                }
                Event::Committed(_) => {}
            }
        }
        events
    }

    /// Commit pending changes and stop the worker; blocks up to
    /// [`SHUTDOWN_TIMEOUT`].
    pub fn shutdown(&mut self, generation: u64) {
        self.seen_generation = generation;
        self.pending_since = None;
        if let Some(worker) = self.worker.take()
            && !worker.shutdown(SHUTDOWN_TIMEOUT)
        {
            eprintln!("Warning: Git sync did not finish before exit.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commit_is_debounced_after_saves() {
        // Without a worker the requests go nowhere, but the timing logic is
        // still observable through `pending_since`.
        let mut st = GitState::new(None, None);
        let t0 = Instant::now();
        st.observe(0, t0);
        assert!(st.pending_since.is_none(), "nothing saved yet");

        st.observe(1, t0);
        assert!(st.pending_since.is_some(), "a save starts the timer");
        st.observe(1, t0 + COMMIT_DELAY / 2);
        assert!(st.pending_since.is_some(), "not yet due");
        // Another save restarts the wait.
        st.observe(2, t0 + COMMIT_DELAY / 2);
        st.observe(2, t0 + COMMIT_DELAY + Duration::from_millis(1));
        assert!(st.pending_since.is_some(), "restarted by the second save");
        st.observe(2, t0 + COMMIT_DELAY / 2 + COMMIT_DELAY);
        assert!(st.pending_since.is_none(), "committed once due");
    }

    #[test]
    fn commit_now_clears_pending_and_tracks_generation() {
        let mut st = GitState::new(None, None);
        st.observe(3, Instant::now());
        st.commit_now(3);
        assert!(st.pending_since.is_none());
        // A later save is noticed again.
        st.observe(4, Instant::now());
        assert!(st.pending_since.is_some());
    }

    #[test]
    fn sync_requires_a_worker() {
        let mut st = GitState::new(None, Some("disabled".into()));
        assert!(!st.is_available());
        assert!(!st.request_sync());
        assert!(st.poll().is_empty());
    }
}
