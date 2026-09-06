//! End-to-end tests for automatic commits and syncing between two Piki
//! working copies, over local paths and over the `ssh://` transport (driven
//! through a fake `ssh` script that runs the remote command locally).

use piki_core::GitConfig;
use piki_core::git::ssh::{self, RemoteCheck, SshUrl};
use piki_core::git::{Repo, SyncLock};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_dir(tag: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("piki-git-{tag}-{nanos}-{n}"));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(dir: &Path, rel: &str, content: &str) {
    let path = dir.join(rel);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, content).unwrap();
}

fn read(dir: &Path, rel: &str) -> String {
    fs::read_to_string(dir.join(rel)).unwrap_or_default()
}

/// A fresh working copy with one committed note, so histories can be shared
/// by cloning from it. The tag goes into the note so two seeded repositories
/// never end up with byte-identical (and thus shared) root commits.
fn seeded_repo(tag: &str) -> (PathBuf, Repo) {
    let dir = unique_dir(tag);
    let repo = Repo::init(&dir).unwrap();
    write(&dir, "frontpage.md", &format!("# Home\n\n<!-- {tag} -->\n"));
    repo.commit_changes().unwrap().expect("initial commit");
    (dir, repo)
}

fn frontpage(tag: &str) -> String {
    format!("# Home\n\n<!-- {tag} -->\n")
}

fn path_url(dir: &Path) -> String {
    dir.to_str().unwrap().to_string()
}

#[test]
fn open_reports_non_repositories_as_none() {
    let dir = unique_dir("plain");
    assert!(Repo::open(&dir).unwrap().is_none());
    // A notes dir nested inside a repository is still "not a repository":
    // Piki must never commit into a parent project.
    let _outer = Repo::init(&dir).unwrap();
    let inner = dir.join("notes");
    fs::create_dir_all(&inner).unwrap();
    assert!(Repo::open(&inner).unwrap().is_none());
    assert!(Repo::open(&dir).unwrap().is_some());
}

#[test]
fn commits_describe_what_changed() {
    let dir = unique_dir("commit");
    let repo = Repo::init(&dir).unwrap();
    assert_eq!(repo.commit_changes().unwrap(), None);

    write(&dir, "recipes.md", "# Recipes\n");
    let c = repo.commit_changes().unwrap().unwrap();
    assert_eq!(c.title, "New note: recipes");
    assert_eq!(c.changes, 1);

    write(&dir, "recipes.md", "# Recipes\n\nPasta.\n");
    assert_eq!(
        repo.commit_changes().unwrap().unwrap().title,
        "Note recipes edited"
    );

    fs::rename(dir.join("recipes.md"), dir.join("cooking.md")).unwrap();
    assert_eq!(
        repo.commit_changes().unwrap().unwrap().title,
        "Note recipes renamed to cooking"
    );

    fs::remove_file(dir.join("cooking.md")).unwrap();
    write(&dir, "a.md", "a\n");
    write(&dir, "sub/b.md", "b\n");
    let c = repo.commit_changes().unwrap().unwrap();
    assert_eq!(c.changes, 3);
    assert!(c.title.ends_with("(+2 more)"), "{}", c.title);

    // Nothing left to commit, and the tree is clean.
    assert_eq!(repo.commit_changes().unwrap(), None);
    assert!(repo.is_clean().unwrap());
}

#[test]
fn ignored_files_are_not_committed() {
    let dir = unique_dir("ignore");
    let repo = Repo::init(&dir).unwrap();
    write(&dir, ".gitignore", "*.tmp\n");
    write(&dir, "scratch.tmp", "x");
    write(&dir, "note.md", "n");
    let c = repo.commit_changes().unwrap().unwrap();
    assert_eq!(c.changes, 2, "gitignore + note, not the .tmp file");
    assert!(repo.is_clean().unwrap());
}

#[test]
fn lock_blocks_concurrent_commits() {
    let dir = unique_dir("lock");
    let repo = Repo::init(&dir).unwrap();
    write(&dir, "note.md", "n");
    let held = SyncLock::acquire(&dir.join(".git")).unwrap();
    let err = repo.commit_changes().unwrap_err();
    assert!(err.contains("Another Piki process"), "{err}");
    drop(held);
    assert!(repo.commit_changes().unwrap().is_some());
}

#[test]
fn sync_pushes_into_other_working_copy_and_pulls_back() {
    let (b_dir, b) = seeded_repo("b");
    let a_dir = unique_dir("a");
    let a = Repo::clone(&path_url(&b_dir), &a_dir).unwrap();
    assert_eq!(read(&a_dir, "frontpage.md"), frontpage("b"));
    // Cloning turns the path into a file:// URL and registers origin as a
    // sync target.
    assert_eq!(
        a.remotes().unwrap()[0].1,
        format!("file://{}", b_dir.display())
    );
    assert_eq!(
        a.sync_remotes(&GitConfig::default()).unwrap(),
        vec!["origin".to_string()]
    );

    // A writes a note and syncs: committed and pushed, B's tree updated.
    write(&a_dir, "todo.md", "- [ ] x\n");
    let report = a.sync(&["origin".to_string()]).unwrap();
    assert_eq!(report.committed.unwrap().title, "New note: todo");
    let out = report.outcomes[0].clone().unwrap();
    assert!(out.pushed && !out.pulled && !out.merged, "{out:?}");
    assert_eq!(read(&b_dir, "todo.md"), "- [ ] x\n");
    assert!(b.is_clean().unwrap());
    // Nothing changed locally as a result of the sync itself.
    assert!(report.changed_paths.is_empty());

    // B edits and commits; A syncs: fast-forward, file changed on A.
    write(&b_dir, "todo.md", "- [x] x\n");
    b.commit_changes().unwrap().unwrap();
    let report = a.sync(&["origin".to_string()]).unwrap();
    assert!(report.committed.is_none());
    let out = report.outcomes[0].clone().unwrap();
    assert!(out.pulled && !out.merged && !out.pushed, "{out:?}");
    assert_eq!(read(&a_dir, "todo.md"), "- [x] x\n");
    assert_eq!(report.changed_paths, vec!["todo.md".to_string()]);
    assert!(a.is_clean().unwrap());

    // Up to date now.
    let report = a.sync(&["origin".to_string()]).unwrap();
    let out = report.outcomes[0].clone().unwrap();
    assert_eq!(
        out,
        piki_core::git::RemoteOutcome {
            remote: "origin".into(),
            ..Default::default()
        }
    );
    assert_eq!(report.summary(), "Synced. origin: already up to date");
}

#[test]
fn sync_merges_independent_changes_on_both_sides() {
    let (b_dir, b) = seeded_repo("merge-b");
    let a_dir = unique_dir("merge-a");
    let a = Repo::clone(&path_url(&b_dir), &a_dir).unwrap();

    write(&a_dir, "from-a.md", "A\n");
    write(&b_dir, "from-b.md", "B\n");
    b.commit_changes().unwrap().unwrap();

    let report = a.sync(&["origin".to_string()]).unwrap();
    let out = report.outcomes[0].clone().unwrap();
    assert!(out.pulled && out.merged && out.pushed, "{out:?}");
    assert_eq!(out.describe(), "origin: merged remote changes and pushed");
    assert_eq!(read(&a_dir, "from-b.md"), "B\n");
    assert_eq!(read(&b_dir, "from-a.md"), "A\n");
    assert!(a.is_clean().unwrap());
    assert!(b.is_clean().unwrap());
    assert_eq!(report.changed_paths, vec!["from-b.md".to_string()]);
}

#[test]
fn conflicting_edits_are_refused_and_nothing_changes() {
    let (b_dir, b) = seeded_repo("conflict-b");
    let a_dir = unique_dir("conflict-a");
    let a = Repo::clone(&path_url(&b_dir), &a_dir).unwrap();

    write(&a_dir, "frontpage.md", "# Home (A)\n");
    write(&b_dir, "frontpage.md", "# Home (B)\n");
    b.commit_changes().unwrap().unwrap();

    let report = a.sync(&["origin".to_string()]).unwrap();
    assert!(report.has_errors());
    let err = report.outcomes[0].clone().unwrap_err();
    assert!(err.contains("conflict"), "{err}");
    assert!(err.contains("frontpage.md"), "{err}");
    // Local commit exists, but the tree is untouched and clean.
    assert_eq!(read(&a_dir, "frontpage.md"), "# Home (A)\n");
    assert_eq!(read(&b_dir, "frontpage.md"), "# Home (B)\n");
    assert!(a.is_clean().unwrap());
    assert!(report.changed_paths.is_empty());
    assert!(report.summary().starts_with("Sync failed: "));
}

#[test]
fn unrelated_histories_are_refused() {
    let (b_dir, _b) = seeded_repo("unrelated-b");
    let (c_dir, c) = seeded_repo("unrelated-c");
    c.add_remote("b", &path_url(&b_dir)).unwrap();
    let report = c.sync(&["b".to_string()]).unwrap();
    let err = report.outcomes[0].clone().unwrap_err();
    assert!(err.contains("different history"), "{err}");
    assert_eq!(read(&c_dir, "frontpage.md"), frontpage("unrelated-c"));
    assert!(c.is_clean().unwrap());
    // The diagnostic used by `piki remote add` agrees.
    assert!(!c.shares_history_with("b").unwrap());
}

#[test]
fn push_is_rejected_while_the_other_side_has_uncommitted_changes() {
    let (b_dir, _b) = seeded_repo("dirty-b");
    let a_dir = unique_dir("dirty-a");
    let a = Repo::clone(&path_url(&b_dir), &a_dir).unwrap();

    write(&b_dir, "frontpage.md", "# Home, being edited\n"); // not committed
    write(&a_dir, "new.md", "new\n");
    let report = a.sync(&["origin".to_string()]).unwrap();
    let err = report.outcomes[0].clone().unwrap_err();
    assert!(err.contains("rejected"), "{err}");
    // B's edit survived untouched.
    assert_eq!(read(&b_dir, "frontpage.md"), "# Home, being edited\n");
    assert_eq!(read(&b_dir, "new.md"), "");
}

#[test]
fn empty_repository_adopts_remote_history() {
    let (b_dir, _b) = seeded_repo("adopt-b");
    let d_dir = unique_dir("adopt-d");
    let d = Repo::init(&d_dir).unwrap();
    d.add_remote("b", &path_url(&b_dir)).unwrap();
    assert!(d.shares_history_with("b").unwrap_or(true));
    let report = d.sync(&["b".to_string()]).unwrap();
    let out = report.outcomes[0].clone().unwrap();
    assert!(out.pulled && !out.pushed, "{out:?}");
    assert_eq!(read(&d_dir, "frontpage.md"), frontpage("adopt-b"));
    assert_eq!(report.changed_paths, vec!["frontpage.md".to_string()]);
    assert!(d.is_clean().unwrap());
}

#[test]
fn pushes_into_an_empty_remote() {
    let (a_dir, a) = seeded_repo("fill-a");
    let e_dir = unique_dir("fill-e");
    let _e = Repo::init(&e_dir).unwrap();
    a.add_remote("e", &path_url(&e_dir)).unwrap();
    let report = a.sync(&["e".to_string()]).unwrap();
    let out = report.outcomes[0].clone().unwrap();
    assert!(out.pushed && !out.pulled, "{out:?}");
    assert_eq!(read(&e_dir, "frontpage.md"), frontpage("fill-a"));
    let _ = a_dir;
}

#[test]
fn differently_named_branches_pair_up_when_unambiguous() {
    let (b_dir, _b) = seeded_repo("branch-b");
    // A fresh repo whose (unborn) branch is called differently: it adopts B's
    // branch under its own name, then pushes its next change back to B's.
    let a2_dir = unique_dir("branch-a2");
    {
        let raw = git2::Repository::init(&a2_dir).unwrap();
        raw.set_head("refs/heads/notes").unwrap();
    }
    let a2 = Repo::open(&a2_dir).unwrap().unwrap();
    a2.add_remote("b", &path_url(&b_dir)).unwrap();
    let report = a2.sync(&["b".to_string()]).unwrap();
    assert!(report.outcomes[0].clone().unwrap().pulled);
    assert_eq!(a2.current_branch().unwrap(), "notes");
    write(&a2_dir, "extra.md", "e\n");
    let report = a2.sync(&["b".to_string()]).unwrap();
    assert!(report.outcomes[0].clone().unwrap().pushed);
    assert_eq!(read(&b_dir, "extra.md"), "e\n");
}

#[test]
fn sync_remote_resolution_rules() {
    let dir = unique_dir("resolve");
    let repo = Repo::init(&dir).unwrap();
    let other = unique_dir("resolve-other");

    // Nothing configured at all: error pointing at `piki remote add`.
    let err = repo.sync_remotes(&GitConfig::default()).unwrap_err();
    assert!(err.contains("piki remote add"), "{err}");

    // Explicit empty list: commit only.
    let cfg = GitConfig {
        enabled: true,
        remotes: Some(vec![]),
    };
    assert!(repo.sync_remotes(&cfg).unwrap().is_empty());

    // Explicit list naming a missing remote: error.
    let cfg = GitConfig {
        enabled: true,
        remotes: Some(vec!["nope".into()]),
    };
    assert!(repo.sync_remotes(&cfg).unwrap_err().contains("'nope'"));

    // origin exists and nothing registered: origin.
    repo.add_remote("origin", &path_url(&other)).unwrap();
    assert_eq!(
        repo.sync_remotes(&GitConfig::default()).unwrap(),
        vec!["origin".to_string()]
    );

    // Registered remotes win over the origin default, in order; duplicates
    // are not recorded twice; removing a remote unregisters it.
    repo.add_remote("laptop", &path_url(&other)).unwrap();
    repo.add_remote("desk", &path_url(&other)).unwrap();
    repo.register_sync_remote("laptop").unwrap();
    repo.register_sync_remote("desk").unwrap();
    repo.register_sync_remote("laptop").unwrap();
    assert_eq!(
        repo.sync_remotes(&GitConfig::default()).unwrap(),
        vec!["laptop".to_string(), "desk".to_string()]
    );
    repo.remove_remote("laptop").unwrap();
    assert_eq!(
        repo.sync_remotes(&GitConfig::default()).unwrap(),
        vec!["desk".to_string()]
    );
    assert_eq!(
        repo.remotes()
            .unwrap()
            .into_iter()
            .map(|(n, _)| n)
            .collect::<Vec<_>>(),
        vec!["desk".to_string(), "origin".to_string()]
    );

    // An explicit list still overrides everything.
    let cfg = GitConfig {
        enabled: true,
        remotes: Some(vec!["origin".into()]),
    };
    assert_eq!(repo.sync_remotes(&cfg).unwrap(), vec!["origin".to_string()]);
}

#[test]
fn remotes_added_with_plain_git_paths_work() {
    // `git remote add other /path` leaves a plain path in the config, which
    // libgit2 alone could not push to (non-bare); syncing normalizes it.
    let (b_dir, _b) = seeded_repo("rawpath-b");
    let (a_dir, a) = seeded_repo("rawpath-a");
    {
        let raw = git2::Repository::open(&a_dir).unwrap();
        raw.remote("b", b_dir.to_str().unwrap()).unwrap();
    }
    assert_eq!(a.remotes().unwrap()[0].1, b_dir.to_str().unwrap());
    // Unrelated histories are still refused …
    assert!(a.sync(&["b".to_string()]).unwrap().has_errors());
    assert!(!a.shares_history_with("b").unwrap());
    // … but a related one syncs fine, and the stored URL is left alone.
    let c_dir = unique_dir("rawpath-c");
    let c = Repo::init(&c_dir).unwrap();
    {
        let raw = git2::Repository::open(&c_dir).unwrap();
        raw.remote("b", b_dir.to_str().unwrap()).unwrap();
    }
    let report = c.sync(&["b".to_string()]).unwrap();
    assert!(report.outcomes[0].clone().unwrap().pulled, "{report:?}");
    write(&c_dir, "c.md", "c\n");
    let report = c.sync(&["b".to_string()]).unwrap();
    assert!(report.outcomes[0].clone().unwrap().pushed, "{report:?}");
    assert_eq!(read(&b_dir, "c.md"), "c\n");
    assert_eq!(c.remotes().unwrap()[0].1, b_dir.to_str().unwrap());
}

// ----- SSH transport ---------------------------------------------------------

#[cfg(unix)]
mod over_ssh {
    use super::*;
    use std::sync::OnceLock;

    /// A stand-in for `ssh` that ignores the connection options and host and
    /// runs the remote command locally — except for the host "unreachable",
    /// which fails the way ssh does when it cannot connect.
    fn fake_ssh() -> &'static Path {
        static SCRIPT: OnceLock<PathBuf> = OnceLock::new();
        SCRIPT.get_or_init(|| {
            let dir = unique_dir("fake-ssh");
            let path = dir.join("ssh");
            fs::write(
                &path,
                "#!/bin/sh\n\
                 while [ $# -gt 0 ]; do\n\
                   case \"$1\" in\n\
                     -o|-p) shift 2 ;;\n\
                     --) shift; break ;;\n\
                     -*) shift ;;\n\
                     *) break ;;\n\
                   esac\n\
                 done\n\
                 host=\"$1\"; shift\n\
                 if [ \"$host\" = unreachable ]; then\n\
                   echo \"ssh: connect to host unreachable port 22: No route to host\" >&2\n\
                   exit 255\n\
                 fi\n\
                 exec sh -c \"$*\"\n",
            )
            .unwrap();
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            path
        })
    }

    fn use_fake_ssh() {
        ssh::set_ssh_program(Some(fake_ssh().to_str().unwrap()));
    }

    #[test]
    fn fetch_merge_and_push_over_the_ssh_transport() {
        use_fake_ssh();
        let (b_dir, b) = seeded_repo("ssh-b");
        let (a_dir, a) = {
            let a_dir = unique_dir("ssh-a");
            let url = format!("ssh://laptop{}", b_dir.display());
            let a = Repo::clone(&url, &a_dir).unwrap();
            (a_dir, a)
        };
        assert_eq!(read(&a_dir, "frontpage.md"), frontpage("ssh-b"));

        write(&a_dir, "from-a.md", "A\n");
        write(&b_dir, "from-b.md", "B\n");
        b.commit_changes().unwrap().unwrap();
        let report = a.sync(&["origin".to_string()]).unwrap();
        let out = report.outcomes[0].clone().unwrap();
        assert!(out.pulled && out.merged && out.pushed, "{out:?}");
        assert_eq!(read(&a_dir, "from-b.md"), "B\n");
        assert_eq!(read(&b_dir, "from-a.md"), "A\n");
        assert!(b.is_clean().unwrap());
    }

    #[test]
    fn scp_style_remote_urls_work() {
        use_fake_ssh();
        let (b_dir, _b) = seeded_repo("scp-b");
        let a_dir = unique_dir("scp-a");
        let a = Repo::init(&a_dir).unwrap();
        {
            // As left behind by `git remote add laptop laptop:/abs/path`.
            let raw = git2::Repository::open(&a_dir).unwrap();
            raw.remote("laptop", &format!("laptop:{}", b_dir.display()))
                .unwrap();
        }
        let report = a.sync(&["laptop".to_string()]).unwrap();
        assert!(report.outcomes[0].clone().unwrap().pulled, "{report:?}");
        assert_eq!(read(&a_dir, "frontpage.md"), frontpage("scp-b"));
    }

    #[test]
    fn connection_failures_are_reported_clearly() {
        use_fake_ssh();
        let (a_dir, a) = seeded_repo("ssh-fail");
        a.add_remote("far", "ssh://unreachable/~/.piki").unwrap();
        let report = a.sync(&["far".to_string()]).unwrap();
        let err = report.outcomes[0].clone().unwrap_err();
        assert!(err.contains("unreachable"), "{err}");
        assert!(err.contains("No route to host"), "{err}");
        let _ = a_dir;
    }

    #[test]
    fn remote_repository_check() {
        use_fake_ssh();
        let (b_dir, _b) = seeded_repo("ssh-check");
        let plain = unique_dir("ssh-check-plain");

        let ok = SshUrl::parse(&format!("ssh://laptop{}", b_dir.display())).unwrap();
        assert_eq!(check(&ok), RemoteCheck::Ok);
        // The check also prepares the remote for incoming pushes.
        let cfg = git2::Repository::open(&b_dir)
            .unwrap()
            .config()
            .unwrap()
            .get_string("receive.denyCurrentBranch")
            .unwrap();
        assert_eq!(cfg, "updateInstead");

        let no_repo = SshUrl::parse(&format!("ssh://laptop{}", plain.display())).unwrap();
        assert!(matches!(check(&no_repo), RemoteCheck::NoRepository(_)));

        let missing = SshUrl::parse("ssh://laptop/nonexistent/dir").unwrap();
        assert!(matches!(check(&missing), RemoteCheck::NoRepository(_)));

        let down = SshUrl::parse("ssh://unreachable/~/.piki").unwrap();
        assert!(matches!(check(&down), RemoteCheck::Unreachable(m) if m.contains("No route")));
    }

    fn check(url: &SshUrl) -> RemoteCheck {
        ssh::check_remote_repository(url).unwrap()
    }
}
