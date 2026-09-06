//! Git support: automatic commits of note changes and syncing with remotes.
//!
//! The notes directory itself must be the root of a Git working copy for any
//! of this to apply; a notes directory that merely sits *inside* some other
//! repository is treated as "not a repository" so Piki never commits files it
//! does not own.
//!
//! Syncing is deliberately conservative. Everything is committed first, so the
//! working tree is clean; each remote is then fetched, merged (only when the
//! merge is conflict-free, otherwise nothing is touched and an error is
//! reported), and pushed. Any surprise — unrelated histories, a conflicting
//! edit, a rejected push, an unreachable machine — is an error the caller
//! shows, never something resolved by guessing.

pub mod lock;
pub mod message;
pub mod ssh;

pub use lock::SyncLock;
pub use message::{Change, commit_message, describe_change};

use crate::config::GitConfig;
use git2::{
    BranchType, Delta, ErrorCode, FetchOptions, IndexAddOption, MergeOptions, Oid, PushOptions,
    RemoteCallbacks, Repository, RepositoryState, Signature, Status, StatusOptions, StatusShow,
    build::CheckoutBuilder,
};
use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Multi-valued git config key under which `piki remote add` records the
/// remotes it set up, so `piki sync` knows to use them without further
/// configuration (`[git] remotes` in `~/.pikirc` takes precedence).
pub const SYNC_CONFIG_KEY: &str = "piki.sync";

/// Makes a non-bare repository accept pushes to its checked-out branch by
/// updating the working tree — as long as it is clean, otherwise the push is
/// refused. This is what lets two Piki working copies push to each other.
const DENY_CURRENT_BRANCH_KEY: &str = "receive.denyCurrentBranch";
const DENY_CURRENT_BRANCH_VALUE: &str = "updateInstead";

/// A notes directory that is a Git working copy.
pub struct Repo {
    repo: Repository,
    workdir: PathBuf,
}

/// What an automatic commit recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitSummary {
    pub id: String,
    /// First line of the commit message.
    pub title: String,
    /// Number of changed paths.
    pub changes: usize,
}

/// What happened with one remote during a sync.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoteOutcome {
    pub remote: String,
    /// Commits were fetched and integrated into the local branch.
    pub pulled: bool,
    /// Integrating them required a merge commit (as opposed to a fast-forward).
    pub merged: bool,
    /// Local commits were pushed.
    pub pushed: bool,
}

impl RemoteOutcome {
    /// One-line human-readable description.
    pub fn describe(&self) -> String {
        let what = match (self.pulled, self.merged, self.pushed) {
            (false, _, false) => "already up to date".to_string(),
            (true, true, true) => "merged remote changes and pushed".to_string(),
            (true, true, false) => "merged remote changes".to_string(),
            (true, false, true) => "pulled and pushed changes".to_string(),
            (true, false, false) => "pulled changes".to_string(),
            (false, _, true) => "pushed changes".to_string(),
        };
        format!("{}: {what}", self.remote)
    }
}

/// The result of [`Repo::sync`].
#[derive(Debug, Clone, Default)]
pub struct SyncReport {
    /// The commit created for local changes before syncing, if any.
    pub committed: Option<CommitSummary>,
    /// Per remote, in the order they were tried.
    pub outcomes: Vec<Result<RemoteOutcome, String>>,
    /// Paths (relative to the notes dir) whose content changed on disk as a
    /// result of the sync. Lets a UI reload what it is showing.
    pub changed_paths: Vec<String>,
}

impl SyncReport {
    pub fn has_errors(&self) -> bool {
        self.outcomes.iter().any(|o| o.is_err())
    }

    /// True when anything was fetched into the local branch.
    pub fn pulled(&self) -> bool {
        self.outcomes
            .iter()
            .any(|o| matches!(o, Ok(out) if out.pulled))
    }

    /// The error messages, if any.
    pub fn errors(&self) -> Vec<&str> {
        self.outcomes
            .iter()
            .filter_map(|o| o.as_ref().err().map(String::as_str))
            .collect()
    }

    /// A short summary line suitable for a status bar.
    pub fn summary(&self) -> String {
        if self.outcomes.is_empty() {
            return "Nothing to sync with.".to_string();
        }
        let errors = self.errors();
        if !errors.is_empty() {
            return match errors.len() {
                1 => format!("Sync failed: {}", errors[0]),
                n => format!("Sync failed for {n} remotes: {}", errors[0]),
            };
        }
        let parts: Vec<String> = self
            .outcomes
            .iter()
            .filter_map(|o| o.as_ref().ok().map(RemoteOutcome::describe))
            .collect();
        format!("Synced. {}", parts.join("; "))
    }
}

/// The merge stage of an index entry; 0 for an ordinary, conflict-free one.
fn entry_stage(entry: &git2::IndexEntry) -> u16 {
    const STAGE_MASK: u16 = 0x3000;
    const STAGE_SHIFT: u16 = 12;
    (entry.flags & STAGE_MASK) >> STAGE_SHIFT
}

/// Is there anything at `path`? Unlike `Path::exists` this does not follow
/// symlinks, so a dangling one still counts as present — Git tracks the link,
/// not its target, and a broken link is not a deleted note.
fn exists(path: &Path) -> bool {
    path.symlink_metadata().is_ok()
}

fn gerr(context: &str, e: git2::Error) -> String {
    format!("{context}: {}", e.message())
}

impl Repo {
    /// Open the repository rooted at `dir`. Returns `Ok(None)` when `dir` is not
    /// the root of a Git working copy (so callers can warn and carry on without
    /// Git), an error for anything else that is wrong with it.
    pub fn open(dir: &Path) -> Result<Option<Repo>, String> {
        ssh::register();
        match Repository::open(dir) {
            Ok(repo) => {
                if repo.is_bare() {
                    return Err(format!(
                        "{} is a bare Git repository, not a notes directory",
                        dir.display()
                    ));
                }
                Self::from_repository(repo)
            }
            Err(e) if e.code() == ErrorCode::NotFound => Ok(None),
            Err(e) => Err(gerr(
                &format!("Failed to open Git repository in {}", dir.display()),
                e,
            )),
        }
    }

    /// Create a new repository in `dir`, creating the directory as needed.
    pub fn init(dir: &Path) -> Result<Repo, String> {
        ssh::register();
        let repo = Repository::init(dir).map_err(|e| {
            gerr(
                &format!("Failed to create Git repository in {}", dir.display()),
                e,
            )
        })?;
        let repo = Self::from_repository(repo)?.expect("freshly initialized repo");
        repo.allow_pushes_to_checked_out_branch()?;
        Ok(repo)
    }

    /// Clone `url` (any form [`ssh::normalize_url`] understands) into `dir`.
    /// The source becomes the `origin` remote.
    pub fn clone(url: &str, dir: &Path) -> Result<Repo, String> {
        ssh::register();
        let url = ssh::normalize_url(url);
        let repo = Repository::clone(&url, dir)
            .map_err(|e| gerr(&format!("Failed to import notes from {url}"), e))?;
        let repo = Self::from_repository(repo)?.expect("freshly cloned repo");
        repo.allow_pushes_to_checked_out_branch()?;
        repo.register_sync_remote("origin")?;
        Ok(repo)
    }

    fn from_repository(repo: Repository) -> Result<Option<Repo>, String> {
        let Some(workdir) = repo.workdir().map(Path::to_path_buf) else {
            return Ok(None);
        };
        Ok(Some(Repo { repo, workdir }))
    }

    /// The working directory (the notes directory).
    pub fn workdir(&self) -> &Path {
        &self.workdir
    }

    /// Set `receive.denyCurrentBranch = updateInstead` so another machine can
    /// push into this working copy.
    pub fn allow_pushes_to_checked_out_branch(&self) -> Result<(), String> {
        self.repo
            .config()
            .and_then(|mut c| c.set_str(DENY_CURRENT_BRANCH_KEY, DENY_CURRENT_BRANCH_VALUE))
            .map_err(|e| gerr("Failed to update repository configuration", e))
    }

    // ----- Remotes ---------------------------------------------------------

    /// All remotes as `(name, url)`, sorted by name.
    pub fn remotes(&self) -> Result<Vec<(String, String)>, String> {
        let names = self
            .repo
            .remotes()
            .map_err(|e| gerr("Failed to list remotes", e))?;
        let mut out = Vec::new();
        for name in names.iter().flatten().flatten() {
            let url = self
                .repo
                .find_remote(name)
                .ok()
                .and_then(|r| r.url().ok().map(str::to_string))
                .unwrap_or_default();
            out.push((name.to_string(), url));
        }
        out.sort();
        Ok(out)
    }

    pub fn has_remote(&self, name: &str) -> bool {
        self.repo.find_remote(name).is_ok()
    }

    /// Add a remote. The URL is normalized (`host:path` becomes `ssh://`).
    pub fn add_remote(&self, name: &str, url: &str) -> Result<(), String> {
        let url = ssh::normalize_url(url);
        self.repo
            .remote(name, &url)
            .map(|_| ())
            .map_err(|e| gerr(&format!("Failed to add remote '{name}'"), e))
    }

    /// Remove a remote and forget it as a sync target.
    pub fn remove_remote(&self, name: &str) -> Result<(), String> {
        self.repo
            .remote_delete(name)
            .map_err(|e| gerr(&format!("Failed to remove remote '{name}'"), e))?;
        self.unregister_sync_remote(name)
    }

    /// Record `name` as a remote to sync with by default (see
    /// [`SYNC_CONFIG_KEY`]).
    pub fn register_sync_remote(&self, name: &str) -> Result<(), String> {
        if self.registered_sync_remotes()?.iter().any(|n| n == name) {
            return Ok(());
        }
        let mut config = self
            .repo
            .config()
            .map_err(|e| gerr("Failed to open repository configuration", e))?;
        // A regexp matching no existing value appends a new one.
        config
            .set_multivar(SYNC_CONFIG_KEY, "^$", name)
            .map_err(|e| gerr("Failed to update repository configuration", e))
    }

    pub fn unregister_sync_remote(&self, name: &str) -> Result<(), String> {
        let mut config = self
            .repo
            .config()
            .map_err(|e| gerr("Failed to open repository configuration", e))?;
        let pattern = format!("^{}$", regex_escape(name));
        match config.remove_multivar(SYNC_CONFIG_KEY, &pattern) {
            Ok(()) => Ok(()),
            Err(e) if e.code() == ErrorCode::NotFound => Ok(()),
            Err(e) => Err(gerr("Failed to update repository configuration", e)),
        }
    }

    /// Remotes recorded by [`Repo::register_sync_remote`], in config order.
    pub fn registered_sync_remotes(&self) -> Result<Vec<String>, String> {
        let config = self
            .repo
            .config()
            .and_then(|mut c| c.snapshot())
            .map_err(|e| gerr("Failed to read repository configuration", e))?;
        let mut names = Vec::new();
        match config.multivar(SYNC_CONFIG_KEY, None) {
            Ok(entries) => {
                entries
                    .for_each(|entry| {
                        if let Ok(v) = entry.value() {
                            names.push(v.to_string());
                        }
                    })
                    .map_err(|e| gerr("Failed to read repository configuration", e))?;
            }
            Err(e) if e.code() == ErrorCode::NotFound => {}
            Err(e) => return Err(gerr("Failed to read repository configuration", e)),
        }
        Ok(names)
    }

    /// Resolve which remotes a sync should use, applying the rules documented
    /// on [`GitConfig::remotes`]. An empty result means "commit only".
    pub fn sync_remotes(&self, config: &GitConfig) -> Result<Vec<String>, String> {
        if let Some(list) = &config.remotes {
            for name in list {
                if !self.has_remote(name) {
                    return Err(format!(
                        "Remote '{name}' from the [git] remotes setting in ~/.pikirc does not \
                         exist. Add it with `piki remote add {name}` or remove it from the list."
                    ));
                }
            }
            return Ok(list.clone());
        }
        let registered: Vec<String> = self
            .registered_sync_remotes()?
            .into_iter()
            .filter(|n| self.has_remote(n))
            .collect();
        if !registered.is_empty() {
            return Ok(registered);
        }
        if self.has_remote("origin") {
            return Ok(vec!["origin".to_string()]);
        }
        Err(
            "No remote to sync with: there is no 'origin' remote and none was added with \
             `piki remote add <host>`. To sync with nothing (commits only), set \
             `remotes = []` under [git] in ~/.pikirc."
                .to_string(),
        )
    }

    // ----- Local state -----------------------------------------------------

    /// Name of the checked-out branch (also for an unborn one).
    pub fn current_branch(&self) -> Result<String, String> {
        match self.repo.head() {
            Ok(head) => head
                .shorthand()
                .map(str::to_string)
                .map_err(|_| "HEAD is not a branch".to_string()),
            Err(e) if e.code() == ErrorCode::UnbornBranch => {
                let head = self
                    .repo
                    .find_reference("HEAD")
                    .map_err(|e| gerr("Failed to read HEAD", e))?;
                head.symbolic_target()
                    .ok()
                    .flatten()
                    .and_then(|t| t.strip_prefix("refs/heads/"))
                    .map(str::to_string)
                    .ok_or_else(|| "HEAD is not a branch".to_string())
            }
            Err(e) => Err(gerr("Failed to read HEAD", e)),
        }
    }

    /// Commit the current branch points at, `None` while it is unborn.
    fn head_oid(&self) -> Result<Option<Oid>, String> {
        match self.repo.head() {
            Ok(head) => Ok(head.target()),
            Err(e) if e.code() == ErrorCode::UnbornBranch => Ok(None),
            Err(e) => Err(gerr("Failed to read HEAD", e)),
        }
    }

    /// Is the working tree free of uncommitted changes (ignored files aside)?
    pub fn is_clean(&self) -> Result<bool, String> {
        let mut opts = StatusOptions::new();
        opts.include_untracked(true)
            .recurse_untracked_dirs(true)
            .include_ignored(false);
        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| gerr("Failed to read working tree status", e))?;
        let tracked = self.index_paths()?;
        Ok(statuses.iter().all(|e| {
            self.real_status(e.path().ok(), e.status(), &tracked)
                .is_empty()
        }))
    }

    /// `status` with libgit2's phantom working-tree bits masked out.
    ///
    /// For a note whose name libgit2 fails to pair up (see [`Self::stage_all`])
    /// it reports two entries: the index entry looks deleted from the working
    /// tree, and the very file it could not match looks untracked. Both are
    /// wrong whenever the path is in the index *and* on disk, and believing
    /// them would keep [`Self::is_clean`] false forever — so a sync would never
    /// dare update the working tree of a wiki holding such a note.
    fn real_status(&self, path: Option<&str>, status: Status, tracked: &HashSet<String>) -> Status {
        let mut status = status;
        if let Some(path) = path
            && status.intersects(Status::WT_DELETED | Status::WT_NEW)
            && tracked.contains(path)
            && exists(&self.workdir.join(path))
        {
            status.remove(Status::WT_DELETED | Status::WT_NEW);
        }
        status
    }

    /// Every conflict-free path in the index.
    fn index_paths(&self) -> Result<HashSet<String>, String> {
        let index = self
            .repo
            .index()
            .map_err(|e| gerr("Failed to open the index", e))?;
        Ok(index
            .iter()
            .filter(|e| entry_stage(e) == 0)
            .filter_map(|e| String::from_utf8(e.path).ok())
            .collect())
    }

    /// Stage every change in the working tree (respecting `.gitignore`).
    fn stage_all(&self) -> Result<(), String> {
        let mut index = self
            .repo
            .index()
            .map_err(|e| gerr("Failed to open the index", e))?;
        index
            .add_all(["*"].iter(), IndexAddOption::DEFAULT, None)
            .map_err(|e| gerr("Failed to stage changes", e))?;
        // Deletions are deliberately *not* staged with `Index::update_all`.
        // That builds on libgit2's index-to-working-tree diff, whose
        // case-insensitive path comparison mis-sorts paths starting with a
        // non-ASCII byte on `core.ignorecase` filesystems (Windows, macOS): a
        // note called "Über.md" is never paired with its own file and so gets
        // staged as deleted on every single commit — dropping it from the
        // repository and, come the next sync, from every other machine too.
        // Asking the filesystem itself sidesteps that comparison entirely.
        let gone: Vec<String> = index
            .iter()
            .filter(|e| entry_stage(e) == 0)
            // A path libgit2 stored as non-UTF-8 is left alone: never stage a
            // deletion we cannot check.
            .filter_map(|e| String::from_utf8(e.path).ok())
            .filter(|path| !exists(&self.workdir.join(path)))
            .collect();
        for path in gone {
            index
                .remove_path(Path::new(&path))
                .map_err(|e| gerr(&format!("Failed to stage the deletion of '{path}'"), e))?;
        }
        index
            .write()
            .map_err(|e| gerr("Failed to write the index", e))
    }

    /// Classify what is staged relative to HEAD.
    fn staged_changes(&self) -> Result<Vec<Change>, String> {
        let mut opts = StatusOptions::new();
        opts.show(StatusShow::Index)
            .renames_head_to_index(true)
            .include_untracked(false);
        let statuses = self
            .repo
            .statuses(Some(&mut opts))
            .map_err(|e| gerr("Failed to read the index status", e))?;
        let mut changes = Vec::new();
        for entry in statuses.iter() {
            let status = entry.status();
            let path = |p: Result<&str, git2::Error>| p.unwrap_or("?").to_string();
            let delta = entry.head_to_index();
            let new_path = delta
                .as_ref()
                .and_then(|d| d.new_file().path().and_then(Path::to_str))
                .map(str::to_string)
                .unwrap_or_else(|| path(entry.path()));
            let old_path = delta
                .as_ref()
                .and_then(|d| d.old_file().path().and_then(Path::to_str))
                .map(str::to_string)
                .unwrap_or_else(|| new_path.clone());
            let change = if status.contains(Status::INDEX_RENAMED) {
                Change::Renamed(old_path, new_path)
            } else if status.contains(Status::INDEX_NEW) {
                Change::Added(new_path)
            } else if status.contains(Status::INDEX_DELETED) {
                Change::Deleted(old_path)
            } else {
                Change::Modified(new_path)
            };
            changes.push(change);
        }
        Ok(changes)
    }

    fn signature(&self) -> Result<Signature<'static>, String> {
        if let Ok(sig) = self.repo.signature() {
            return Ok(sig);
        }
        // No user.name/user.email configured: fall back to something sensible
        // rather than refusing to commit the user's notes.
        let user = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "piki".to_string());
        Signature::now(&user, &format!("{user}@{}", hostname()))
            .map_err(|e| gerr("Failed to create commit signature", e))
    }

    /// Commit everything that changed in the working tree, with a message
    /// describing the change (see [`message`]). Returns `None` when there was
    /// nothing to commit. Takes the cross-process lock.
    pub fn commit_changes(&self) -> Result<Option<CommitSummary>, String> {
        let _lock = SyncLock::acquire(self.repo.path())?;
        self.commit_changes_locked()
    }

    fn commit_changes_locked(&self) -> Result<Option<CommitSummary>, String> {
        self.ensure_no_operation_in_progress()?;
        self.stage_all()?;
        let changes = self.staged_changes()?;
        let Some(message) = commit_message(&changes) else {
            return Ok(None);
        };
        let mut index = self
            .repo
            .index()
            .map_err(|e| gerr("Failed to open the index", e))?;
        let tree_oid = index
            .write_tree()
            .map_err(|e| gerr("Failed to write tree", e))?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|e| gerr("Failed to read tree", e))?;
        let sig = self.signature()?;
        let parent = match self.head_oid()? {
            Some(oid) => Some(
                self.repo
                    .find_commit(oid)
                    .map_err(|e| gerr("Failed to read HEAD commit", e))?,
            ),
            None => None,
        };
        let parents: Vec<&git2::Commit> = parent.iter().collect();
        let id = self
            .repo
            .commit(Some("HEAD"), &sig, &sig, &message, &tree, &parents)
            .map_err(|e| gerr("Failed to commit", e))?;
        Ok(Some(CommitSummary {
            id: id.to_string(),
            title: message.lines().next().unwrap_or_default().to_string(),
            changes: changes.len(),
        }))
    }

    fn ensure_no_operation_in_progress(&self) -> Result<(), String> {
        if self.repo.state() != RepositoryState::Clean {
            return Err(format!(
                "A Git operation (merge, rebase, …) is in progress in {}; finish or abort it \
                 with git first.",
                self.workdir.display()
            ));
        }
        Ok(())
    }

    // ----- Sync ------------------------------------------------------------

    /// Commit local changes, then fetch from, merge with and push to each of
    /// `remotes` in turn. A failing remote does not stop the others; every
    /// outcome is reported. Takes the cross-process lock for the whole run.
    pub fn sync(&self, remotes: &[String]) -> Result<SyncReport, String> {
        let _lock = SyncLock::acquire(self.repo.path())?;
        let mut report = SyncReport {
            committed: self.commit_changes_locked()?,
            ..SyncReport::default()
        };
        let before = self.head_oid()?;
        for remote in remotes {
            report.outcomes.push(self.sync_remote(remote));
        }
        let after = self.head_oid()?;
        report.changed_paths = self.changed_paths(before, after)?;
        Ok(report)
    }

    /// The remote to talk to for `name`, plus the fetch refspecs to use.
    ///
    /// Remotes added with plain `git` often have scp-style (`laptop:.piki`) or
    /// local-path URLs, which libgit2 cannot hand to our transport. Those are
    /// used through an anonymous remote with the normalized URL (and explicit
    /// refspecs, since an anonymous remote has none) so the user's git config
    /// stays untouched.
    fn open_remote(&self, name: &str) -> Result<(git2::Remote<'_>, Vec<String>), String> {
        let configured = self
            .repo
            .find_remote(name)
            .map_err(|e| gerr(&format!("Unknown remote '{name}'"), e))?;
        let url = configured
            .url()
            .map_err(|e| gerr(&format!("Remote '{name}' has an invalid URL"), e))?
            .to_string();
        let normalized = ssh::normalize_url(&url);
        if normalized == url {
            return Ok((configured, Vec::new()));
        }
        let anonymous = self
            .repo
            .remote_anonymous(&normalized)
            .map_err(|e| gerr(&format!("Failed to use remote '{name}'"), e))?;
        Ok((
            anonymous,
            vec![format!("+refs/heads/*:refs/remotes/{name}/*")],
        ))
    }

    fn fetch_from(
        &self,
        name: &str,
        remote: &mut git2::Remote<'_>,
        refspecs: &[String],
    ) -> Result<(), String> {
        let mut fetch_opts = FetchOptions::new();
        remote
            .fetch(refspecs, Some(&mut fetch_opts), None)
            .map_err(|e| gerr(&format!("Fetching from '{name}' failed"), e))
    }

    fn sync_remote(&self, name: &str) -> Result<RemoteOutcome, String> {
        let mut outcome = RemoteOutcome {
            remote: name.to_string(),
            ..RemoteOutcome::default()
        };
        let (mut remote, refspecs) = self.open_remote(name)?;
        let branch = self.current_branch()?;
        self.fetch_from(name, &mut remote, &refspecs)?;

        let (remote_branch, remote_oid) = self.remote_counterpart(name, &branch)?;
        let local_oid = self.head_oid()?;

        let push_needed = match (local_oid, remote_oid) {
            (None, None) => false,
            (None, Some(theirs)) => {
                // Nothing committed here yet: adopt the remote branch wholesale.
                self.fast_forward_to(&branch, theirs, name)?;
                outcome.pulled = true;
                false
            }
            (Some(_), None) => true,
            (Some(ours), Some(theirs)) if ours == theirs => false,
            (Some(ours), Some(theirs)) => {
                if self.is_descendant(ours, theirs)? {
                    true
                } else if self.is_descendant(theirs, ours)? {
                    self.fast_forward_to(&branch, theirs, name)?;
                    outcome.pulled = true;
                    false
                } else {
                    self.merge_into_head(ours, theirs, name)?;
                    outcome.pulled = true;
                    outcome.merged = true;
                    true
                }
            }
        };

        if push_needed {
            self.push(name, &mut remote, &branch, &remote_branch)?;
            outcome.pushed = true;
        }
        Ok(outcome)
    }

    /// The remote-tracking branch to sync `branch` with: the same name when the
    /// remote has it, otherwise the remote's only branch (so a `main` here can
    /// pair with a `master` there). `(name, None)` means the remote is empty.
    fn remote_counterpart(
        &self,
        remote: &str,
        branch: &str,
    ) -> Result<(String, Option<Oid>), String> {
        let same = format!("refs/remotes/{remote}/{branch}");
        if let Ok(r) = self.repo.find_reference(&same) {
            return Ok((branch.to_string(), r.target()));
        }
        let prefix = format!("refs/remotes/{remote}/");
        let refs = self
            .repo
            .references_glob(&format!("{prefix}*"))
            .map_err(|e| gerr("Failed to list remote branches", e))?;
        let mut candidates = Vec::new();
        for r in refs.flatten() {
            let Ok(name) = r.name() else { continue };
            let Some(short) = name.strip_prefix(&prefix) else {
                continue;
            };
            if short == "HEAD" {
                continue;
            }
            if let Some(oid) = r.target() {
                candidates.push((short.to_string(), oid));
            }
        }
        match candidates.len() {
            0 => Ok((branch.to_string(), None)),
            1 => Ok((candidates[0].0.clone(), Some(candidates[0].1))),
            _ => Err(format!(
                "Remote '{remote}' has no branch named '{branch}' but several others ({}); \
                 not sure which one to sync with.",
                candidates
                    .iter()
                    .map(|(n, _)| n.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )),
        }
    }

    fn is_descendant(&self, commit: Oid, ancestor: Oid) -> Result<bool, String> {
        self.repo
            .graph_descendant_of(commit, ancestor)
            .map_err(|e| gerr("Failed to compare histories", e))
    }

    fn ensure_clean_for_checkout(&self, remote: &str) -> Result<(), String> {
        if !self.is_clean()? {
            return Err(format!(
                "Not integrating changes from '{remote}': the notes directory has uncommitted \
                 changes."
            ));
        }
        Ok(())
    }

    /// Move `branch` (and the working tree) to `target`, which must be a
    /// descendant of the current HEAD (or HEAD must be unborn).
    fn fast_forward_to(&self, branch: &str, target: Oid, remote: &str) -> Result<(), String> {
        self.ensure_clean_for_checkout(remote)?;
        let commit = self
            .repo
            .find_commit(target)
            .map_err(|e| gerr("Failed to read remote commit", e))?;
        let tree = commit
            .tree()
            .map_err(|e| gerr("Failed to read remote tree", e))?;
        // Update files first while HEAD still describes what is on disk (the
        // safe-mode baseline), then move the branch.
        self.repo
            .checkout_tree(tree.as_object(), Some(CheckoutBuilder::new().safe()))
            .map_err(|e| gerr(&format!("Failed to update notes from '{remote}'"), e))?;
        self.repo
            .reference(
                &format!("refs/heads/{branch}"),
                target,
                true,
                &format!("piki sync: fast-forward to {remote}"),
            )
            .map_err(|e| gerr("Failed to update branch", e))?;
        Ok(())
    }

    /// Merge `theirs` into `ours` (the current HEAD). Refuses when the histories
    /// are unrelated or the merge has conflicts; in both cases nothing changes.
    fn merge_into_head(&self, ours: Oid, theirs: Oid, remote: &str) -> Result<(), String> {
        self.ensure_clean_for_checkout(remote)?;
        if self.repo.merge_base(ours, theirs).is_err() {
            return Err(format!(
                "The notes on '{remote}' have a different history than the local ones \
                 (no common ancestor); refusing to merge them."
            ));
        }
        let our_commit = self
            .repo
            .find_commit(ours)
            .map_err(|e| gerr("Failed to read local commit", e))?;
        let their_commit = self
            .repo
            .find_commit(theirs)
            .map_err(|e| gerr("Failed to read remote commit", e))?;
        let mut opts = MergeOptions::new();
        opts.find_renames(true);
        let mut index = self
            .repo
            .merge_commits(&our_commit, &their_commit, Some(&opts))
            .map_err(|e| gerr(&format!("Merging changes from '{remote}' failed"), e))?;
        if index.has_conflicts() {
            let mut paths = Vec::new();
            if let Ok(conflicts) = index.conflicts() {
                for c in conflicts.flatten() {
                    let entry = c.our.or(c.their).or(c.ancestor);
                    if let Some(entry) = entry {
                        paths.push(String::from_utf8_lossy(&entry.path).into_owned());
                    }
                }
            }
            paths.sort();
            paths.dedup();
            return Err(format!(
                "Changes from '{remote}' conflict with local changes in: {}. Resolve this with \
                 git in {} (e.g. `git pull {remote}`), then sync again.",
                paths.join(", "),
                self.workdir.display()
            ));
        }
        let tree_oid = index
            .write_tree_to(&self.repo)
            .map_err(|e| gerr("Failed to write merged tree", e))?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(|e| gerr("Failed to read merged tree", e))?;
        // Files first (baseline is still HEAD = ours, and the tree is clean),
        // then the merge commit that moves HEAD.
        self.repo
            .checkout_tree(tree.as_object(), Some(CheckoutBuilder::new().safe()))
            .map_err(|e| gerr(&format!("Failed to update notes from '{remote}'"), e))?;
        let sig = self.signature()?;
        self.repo
            .commit(
                Some("HEAD"),
                &sig,
                &sig,
                &format!("Merge changes from {remote}"),
                &tree,
                &[&our_commit, &their_commit],
            )
            .map_err(|e| gerr("Failed to create merge commit", e))?;
        Ok(())
    }

    fn push(
        &self,
        name: &str,
        remote: &mut git2::Remote<'_>,
        branch: &str,
        remote_branch: &str,
    ) -> Result<(), String> {
        let rejected: RefCell<Option<String>> = RefCell::new(None);
        let mut callbacks = RemoteCallbacks::new();
        callbacks.push_update_reference(|refname, status| {
            if let Some(msg) = status {
                *rejected.borrow_mut() = Some(format!("{refname}: {msg}"));
            }
            Ok(())
        });
        let mut opts = PushOptions::new();
        opts.remote_callbacks(callbacks);
        let refspec = format!("refs/heads/{branch}:refs/heads/{remote_branch}");
        let pushed = remote.push(&[refspec.as_str()], Some(&mut opts));
        drop(opts);
        pushed.map_err(|e| gerr(&format!("Pushing to '{name}' failed"), e))?;
        if let Some(msg) = rejected.into_inner() {
            return Err(format!(
                "Pushing to '{name}' was rejected ({msg}). If the other machine has uncommitted \
                 changes, sync there first."
            ));
        }
        Ok(())
    }

    /// Paths whose content differs between two commits (either may be `None`
    /// for "no commit yet").
    fn changed_paths(&self, from: Option<Oid>, to: Option<Oid>) -> Result<Vec<String>, String> {
        if from == to {
            return Ok(Vec::new());
        }
        let tree = |oid: Option<Oid>| -> Result<Option<git2::Tree<'_>>, String> {
            match oid {
                Some(oid) => self
                    .repo
                    .find_commit(oid)
                    .and_then(|c| c.tree())
                    .map(Some)
                    .map_err(|e| gerr("Failed to read tree", e)),
                None => Ok(None),
            }
        };
        let old = tree(from)?;
        let new = tree(to)?;
        let diff = self
            .repo
            .diff_tree_to_tree(old.as_ref(), new.as_ref(), None)
            .map_err(|e| gerr("Failed to diff trees", e))?;
        let mut paths = Vec::new();
        for delta in diff.deltas() {
            let file = if delta.status() == Delta::Deleted {
                delta.old_file()
            } else {
                delta.new_file()
            };
            if let Some(p) = file.path().and_then(Path::to_str) {
                paths.push(p.to_string());
            }
            if delta.status() == Delta::Renamed
                && let Some(p) = delta.old_file().path().and_then(Path::to_str)
            {
                paths.push(p.to_string());
            }
        }
        paths.sort();
        paths.dedup();
        Ok(paths)
    }

    /// Does the local branch have a `refs/remotes/<remote>/…` counterpart at
    /// all? Used after a first fetch to tell "shares history" from "unrelated".
    pub fn shares_history_with(&self, remote: &str) -> Result<bool, String> {
        let branch = self.current_branch()?;
        let (_, remote_oid) = self.remote_counterpart(remote, &branch)?;
        let Some(theirs) = remote_oid else {
            return Ok(true); // empty remote: nothing to disagree about
        };
        let Some(ours) = self.head_oid()? else {
            return Ok(true); // nothing local yet
        };
        Ok(ours == theirs || self.repo.merge_base(ours, theirs).is_ok())
    }

    /// Fetch from `remote` without integrating anything.
    pub fn fetch(&self, name: &str) -> Result<(), String> {
        let (mut remote, refspecs) = self.open_remote(name)?;
        self.fetch_from(name, &mut remote, &refspecs)
    }

    /// Local branches, for diagnostics.
    pub fn local_branches(&self) -> Result<Vec<String>, String> {
        let branches = self
            .repo
            .branches(Some(BranchType::Local))
            .map_err(|e| gerr("Failed to list branches", e))?;
        Ok(branches
            .flatten()
            .filter_map(|(b, _)| b.name().ok().flatten().map(str::to_string))
            .collect())
    }
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|h| !h.is_empty())
        })
        .unwrap_or_else(|| "localhost".to_string())
}

/// Escape `s` for use inside a regular expression (git config multivar
/// patterns are POSIX extended regexps).
fn regex_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if r"\.^$|()[]{}*+?".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regex_escaping() {
        assert_eq!(regex_escape("laptop"), "laptop");
        assert_eq!(regex_escape("a.b"), r"a\.b");
        assert_eq!(regex_escape("x(1)"), r"x\(1\)");
    }

    #[test]
    fn outcome_descriptions() {
        let mut o = RemoteOutcome {
            remote: "laptop".into(),
            ..Default::default()
        };
        assert_eq!(o.describe(), "laptop: already up to date");
        o.pushed = true;
        assert_eq!(o.describe(), "laptop: pushed changes");
        o.pulled = true;
        assert_eq!(o.describe(), "laptop: pulled and pushed changes");
        o.merged = true;
        assert_eq!(o.describe(), "laptop: merged remote changes and pushed");
        o.pushed = false;
        assert_eq!(o.describe(), "laptop: merged remote changes");
    }

    #[test]
    fn report_summary_prefers_errors() {
        let report = SyncReport {
            committed: None,
            outcomes: vec![
                Ok(RemoteOutcome {
                    remote: "a".into(),
                    pushed: true,
                    ..Default::default()
                }),
                Err("boom".into()),
            ],
            changed_paths: vec![],
        };
        assert!(report.has_errors());
        assert_eq!(report.summary(), "Sync failed: boom");
        let ok = SyncReport {
            outcomes: vec![Ok(RemoteOutcome {
                remote: "a".into(),
                ..Default::default()
            })],
            ..Default::default()
        };
        assert_eq!(ok.summary(), "Synced. a: already up to date");
        assert_eq!(SyncReport::default().summary(), "Nothing to sync with.");
    }
}
