use clap::{Parser, Subcommand};
use crossterm::terminal;
use fuzzypicker::FuzzyPicker;
use piki_core::git::ssh::{self, RemoteCheck, SshUrl};
use piki_core::git::{Repo, SyncReport};
use piki_core::{Config, DocumentStore, IndexPlugin, PluginRegistry, TodoPlugin, has_md_extension};
use std::env;
use std::fs;
use std::io::{self, BufRead, Cursor, IsTerminal, Write};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use tdoc::formatter::{Formatter, FormattingStyle};
use tdoc::{Document, LinkPolicy, markdown, pager as tdoc_pager};
use url::Url;

#[derive(Parser, Debug)]
#[command(name = "piki")]
#[command(about = "A simple personal wiki", long_about = None)]
struct Args {
    /// Directory containing markdown files (default: ~/.piki)
    #[arg(short = 'd', long = "directory", value_name = "DIRECTORY")]
    directory: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Note name (for default edit command)
    name: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Edit a note
    Edit {
        /// Name of the note to edit
        name: Option<String>,
    },
    /// Generate an index of all notes
    Index,
    /// Create the notes directory, empty or imported from another machine
    Init {
        /// Import the notes from this machine over SSH (as in `ssh HOST`)
        #[arg(long, value_name = "HOST")]
        from: Option<String>,
        /// Notes directory on that machine (default: ~/.piki)
        #[arg(long, value_name = "PATH", requires = "from")]
        path: Option<String>,
    },
    /// Show the commit log
    Log {
        /// Number of commits to show
        #[arg(short = 'n', default_value = "25")]
        count: usize,
    },
    /// List all notes
    Ls,
    /// Manage the machines (Git remotes) to sync with
    Remote {
        #[command(subcommand)]
        action: RemoteCommands,
    },
    /// Run a shell command inside the notes directory
    Run {
        /// Command to run
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// Full-text search notes (all terms must match)
    Search {
        /// Terms to search for; a note matches only when it contains all of them
        #[arg(required = true, trailing_var_arg = true, allow_hyphen_values = true)]
        terms: Vec<String>,
    },
    /// Commit local changes and sync them with the configured remotes
    Sync,
    /// List all todos from all notes
    Todo,
    /// View a note
    View {
        /// Name of the note to view
        name: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum RemoteCommands {
    /// Add a machine to sync with. NAME is the host to reach over SSH (as in
    /// `ssh NAME`, so `user@host` and ~/.ssh/config aliases work); the notes are
    /// expected in ~/.piki there unless --path or an explicit URL is given.
    Add {
        /// Name for the remote; also the SSH host unless URL is given
        name: String,
        /// Git URL instead of the SSH host shorthand (e.g. ssh://user@host/~/notes)
        url: Option<String>,
        /// Notes directory on the remote machine (default: ~/.piki)
        #[arg(long, value_name = "PATH", conflicts_with = "url")]
        path: Option<String>,
    },
    /// List the configured remotes
    Ls,
    /// Remove a remote
    Rm {
        /// Name of the remote to remove
        name: String,
    },
}

/// Load `~/.pikirc`, warning (rather than failing) about a broken file.
fn load_config() -> Config {
    match Config::load() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Warning: {e}; using defaults.");
            Config::default()
        }
    }
}

fn get_notes_dir(dir_opt: Option<PathBuf>) -> PathBuf {
    dir_opt
        .or_else(piki_core::default_notes_dir)
        .unwrap_or_else(|| {
            eprintln!(
                "Error: could not determine your home directory. \
                 Pass --directory to say where your notes live."
            );
            std::process::exit(1);
        })
}

fn get_editor() -> String {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string())
}

fn interactive_select(store: &DocumentStore) -> Result<Option<String>, String> {
    let mut docs = store.list_all_documents()?;

    if docs.is_empty() {
        return Ok(None);
    }

    // Sort alphabetically
    docs.sort();

    let mut picker = FuzzyPicker::new(&docs);
    return match picker.pick() {
        Ok(res) => Ok(res),
        Err(e) => Err(format!("Failed to run fuzzy picker: {}", e)),
    };

    // DANG, Skim doesn't support Windows ... leaving this here for now

    // Use skim for fuzzy finding
    // let options = SkimOptionsBuilder::default()
    //     .height("50%".to_string())
    //     .multi(false)
    //     .build()
    //     .map_err(|e| format!("Failed to build skim options: {}", e))?;

    // Convert docs to a single string with newlines
    // let input = docs.join("\n");
    // let item_reader = SkimItemReader::default();
    // let items = item_reader.of_bufread(Cursor::new(input));

    // // Run skim
    // let selected = Skim::run_with(&options, Some(items))
    //     .map(|out| {
    //         if out.is_abort {
    //             None
    //         } else {
    //             out.selected_items
    //                 .first()
    //                 .map(|item| item.output().to_string())
    //         }
    //     })
    //     .unwrap_or(None);

    // Ok(selected)
}

fn cmd_edit(name: Option<String>, notes_dir: &PathBuf, config: &Config) -> Result<(), String> {
    let store = DocumentStore::new(notes_dir.clone());

    let note_name = if let Some(name) = name {
        name
    } else {
        // Interactive selection
        match interactive_select(&store)? {
            Some(name) => name,
            None => return Ok(()),
        }
    };

    let doc = store.load(&note_name)?;
    let editor = get_editor();

    // Get the relative path from the notes directory
    let relative_path = doc.path.strip_prefix(notes_dir).unwrap_or(&doc.path);

    let status = Command::new(&editor)
        .arg(relative_path)
        .current_dir(notes_dir)
        .status()
        .map_err(|e| format!("Failed to open editor '{}': {}", editor, e))?;

    if !status.success() {
        return Err(format!("Editor exited with status: {}", status));
    }

    commit_after_edit(notes_dir, config)
}

/// Record the outcome of an edit as a commit (when Git support applies).
fn commit_after_edit(notes_dir: &Path, config: &Config) -> Result<(), String> {
    let Some(repo) = open_repo_if_enabled(notes_dir, config)? else {
        return Ok(());
    };
    if let Some(commit) = repo.commit_changes()? {
        eprintln!("Committed: {}", commit.title);
    }
    Ok(())
}

/// The notes directory as a repository, or `None` when Git support is off or
/// the directory is not a repository (with a warning in the latter case).
fn open_repo_if_enabled(notes_dir: &Path, config: &Config) -> Result<Option<Repo>, String> {
    if !config.git.enabled {
        return Ok(None);
    }
    match Repo::open(notes_dir)? {
        Some(repo) => Ok(Some(repo)),
        None => {
            eprintln!(
                "Warning: {} is not a Git repository; Git support is disabled. \
                 (Run `git init` there to enable it, or set `enabled = false` under \
                 [git] in ~/.pikirc to silence this.)",
                notes_dir.display()
            );
            Ok(None)
        }
    }
}

/// The repository, required: an error when Git is disabled or missing.
fn require_repo(notes_dir: &Path, config: &Config) -> Result<Repo, String> {
    if !config.git.enabled {
        return Err("Git support is disabled in ~/.pikirc ([git] enabled = false).".to_string());
    }
    Repo::open(notes_dir)?.ok_or_else(|| {
        format!(
            "{} is not a Git repository. Run `git init` there first.",
            notes_dir.display()
        )
    })
}

fn cmd_sync(notes_dir: &Path, config: &Config) -> Result<(), String> {
    let repo = require_repo(notes_dir, config)?;
    let remotes = repo.sync_remotes(&config.git)?;
    let report = repo.sync(&remotes)?;
    print_sync_report(&report);
    if report.has_errors() {
        return Err("Sync did not complete for every remote.".to_string());
    }
    Ok(())
}

fn print_sync_report(report: &SyncReport) {
    if let Some(commit) = &report.committed {
        println!("Committed: {}", commit.title);
    }
    if report.outcomes.is_empty() {
        println!("No remotes configured to sync with; local changes are committed only.");
    }
    for outcome in &report.outcomes {
        match outcome {
            Ok(o) => println!("{}", o.describe()),
            Err(e) => eprintln!("Error: {e}"),
        }
    }
    if !report.changed_paths.is_empty() {
        println!("Updated locally: {}", report.changed_paths.join(", "));
    }
}

fn cmd_remote(action: RemoteCommands, notes_dir: &Path, config: &Config) -> Result<(), String> {
    let repo = require_repo(notes_dir, config)?;
    match action {
        RemoteCommands::Ls => {
            let syncing = repo.sync_remotes(&config.git).unwrap_or_default();
            let remotes = repo.remotes()?;
            if remotes.is_empty() {
                println!("No remotes. Add one with `piki remote add <host>`.");
            }
            for (name, url) in remotes {
                let marker = if syncing.contains(&name) { "*" } else { " " };
                println!("{marker} {name}\t{url}");
            }
            Ok(())
        }
        RemoteCommands::Rm { name } => {
            if !repo.has_remote(&name) {
                return Err(format!("No remote named '{name}'."));
            }
            repo.remove_remote(&name)?;
            println!("Removed remote '{name}'.");
            Ok(())
        }
        RemoteCommands::Add { name, url, path } => cmd_remote_add(&repo, &name, url, path, config),
    }
}

fn cmd_remote_add(
    repo: &Repo,
    name: &str,
    url: Option<String>,
    path: Option<String>,
    config: &Config,
) -> Result<(), String> {
    if repo.has_remote(name) {
        return Err(format!(
            "A remote named '{name}' already exists (see `piki remote ls`)."
        ));
    }
    let url = match url {
        Some(u) => ssh::normalize_url(&u),
        None => ssh::url_for_host(name, path.as_deref())?,
    };

    // For SSH targets, check the machine and its notes directory up front so
    // problems come with a plain explanation instead of a protocol error.
    if ssh::is_ssh_url(&url) {
        let parsed = SshUrl::parse(&url)?;
        eprintln!("Checking {} on {} …", parsed.path, parsed.destination());
        match ssh::check_remote_repository(&parsed)? {
            RemoteCheck::Ok => {}
            RemoteCheck::Unreachable(detail) => {
                return Err(format!(
                    "Cannot reach {} over SSH without a password: {detail}\n\
                     Make sure the host is reachable and that an SSH key (or agent) \
                     lets you log in non-interactively, e.g. `ssh {}` works.",
                    parsed.destination(),
                    parsed.destination()
                ));
            }
            RemoteCheck::NoRepository(detail) => {
                return Err(format!(
                    "No Piki notes directory found at {} on {}: {detail}\n\
                     Piki must be set up there with a Git repository (see the README), \
                     or pass --path to point at the right directory.",
                    parsed.path,
                    parsed.destination()
                ));
            }
        }
    }

    repo.add_remote(name, &url)?;
    // Verify the two directories share their history before keeping the
    // remote around; otherwise syncing could only ever fail.
    let verified = repo
        .fetch(name)
        .and_then(|()| repo.shares_history_with(name));
    match verified {
        Ok(true) => {}
        Ok(false) => {
            let _ = repo.remove_remote(name);
            return Err(format!(
                "The notes at {url} have a different history than the local ones (no common \
                 ancestor), so they cannot be synced. To use them, start from a copy: move the \
                 local notes directory aside and let Piki import them from '{name}'."
            ));
        }
        Err(e) => {
            let _ = repo.remove_remote(name);
            return Err(e);
        }
    }
    repo.register_sync_remote(name)?;
    // Our side must accept pushes into the checked-out branch too, so the other
    // machine can add us in return.
    repo.allow_pushes_to_checked_out_branch()?;
    println!("Added remote '{name}' ({url}).");

    match &config.git.remotes {
        Some(list) if !list.iter().any(|n| n == name) => {
            println!(
                "Note: ~/.pikirc lists the remotes to sync with under [git] and does not \
                 include '{name}'; add it there to sync with it."
            );
        }
        _ => println!("`piki sync` and the GUI will now sync with '{name}'."),
    }
    Ok(())
}

/// `piki init`: create the notes directory (as a Git repository when Git
/// support is on), or import it from another machine.
fn cmd_init(
    from: Option<String>,
    path: Option<String>,
    notes_dir: &Path,
    config: &Config,
) -> Result<(), String> {
    if notes_dir.exists() {
        return Err(format!(
            "{} already exists. Remove or move it aside first to import into it.",
            notes_dir.display()
        ));
    }
    match from {
        Some(host) => {
            let url = ssh::url_for_host(&host, path.as_deref())?;
            import_notes(&url, notes_dir)
        }
        None => create_notes_dir(notes_dir, config),
    }
}

fn create_notes_dir(notes_dir: &Path, config: &Config) -> Result<(), String> {
    fs::create_dir_all(notes_dir)
        .map_err(|e| format!("Failed to create {}: {e}", notes_dir.display()))?;
    if config.git.enabled {
        Repo::init(notes_dir)?;
        eprintln!("Created {} as a Git repository.", notes_dir.display());
    } else {
        eprintln!("Created {}.", notes_dir.display());
    }
    Ok(())
}

/// Ask what to do about a missing notes directory: create it, import it from
/// another machine, or quit. Non-interactive sessions get an explanation.
fn set_up_missing_notes_dir(notes_dir: &Path, config: &Config) -> Result<(), String> {
    let stdin = io::stdin();
    if !stdin.is_terminal() || !io::stderr().is_terminal() {
        return Err(format!(
            "Notes directory {} does not exist. Create it with `piki init`, or import your \
             notes from another machine with `piki init --from HOST`.",
            notes_dir.display()
        ));
    }
    eprintln!("Notes directory {} does not exist.", notes_dir.display());
    eprintln!("  [c] Create it");
    eprintln!("  [i] Import notes from another machine over SSH");
    eprintln!("  [q] Quit");
    let answer = prompt("Your choice [c/i/q]: ")?;
    match answer.trim().to_lowercase().as_str() {
        "c" | "create" => create_notes_dir(notes_dir, config),
        "i" | "import" => {
            let host = prompt("Host name of the machine to import from (as in `ssh HOST`): ")?;
            let host = host.trim();
            let path = prompt(&format!(
                "Notes directory on {host} [{}]: ",
                ssh::DEFAULT_REMOTE_PATH
            ))?;
            let path = path.trim();
            let url = ssh::url_for_host(host, (!path.is_empty()).then_some(path))?;
            import_notes(&url, notes_dir)
        }
        _ => Err("Aborted.".to_string()),
    }
}

/// Clone `url` into `notes_dir`, which becomes a working copy with `origin`
/// set up for syncing.
fn import_notes(url: &str, notes_dir: &Path) -> Result<(), String> {
    if ssh::is_ssh_url(url) {
        let parsed = SshUrl::parse(url)?;
        eprintln!("Checking {} on {} …", parsed.path, parsed.destination());
        match ssh::check_remote_repository(&parsed)? {
            RemoteCheck::Ok => {}
            RemoteCheck::Unreachable(detail) => {
                return Err(format!(
                    "Cannot reach {} over SSH without a password: {detail}",
                    parsed.destination()
                ));
            }
            RemoteCheck::NoRepository(detail) => {
                return Err(format!(
                    "No Piki notes directory found at {} on {}: {detail}",
                    parsed.path,
                    parsed.destination()
                ));
            }
        }
    }
    eprintln!("Importing notes from {url} …");
    Repo::clone(url, notes_dir)?;
    eprintln!(
        "Imported notes into {}; '{url}' is set up as the 'origin' remote to sync with.",
        notes_dir.display()
    );
    Ok(())
}

fn prompt(text: &str) -> Result<String, String> {
    eprint!("{text}");
    io::stderr().flush().ok();
    let mut line = String::new();
    io::stdin()
        .lock()
        .read_line(&mut line)
        .map_err(|e| format!("Failed to read input: {e}"))?;
    if line.is_empty() {
        return Err("Aborted.".to_string());
    }
    Ok(line)
}

fn cmd_view(name: Option<String>, notes_dir: &Path) -> Result<(), String> {
    let notes_dir_buf = notes_dir.to_path_buf();
    let canonical_notes_dir = normalize_base_path(notes_dir);
    let store = Arc::new(DocumentStore::new(notes_dir_buf.clone()));

    let mut plugin_registry = PluginRegistry::new();
    plugin_registry.register("index", Box::new(IndexPlugin));
    plugin_registry.register("todo", Box::new(TodoPlugin));
    let plugin_registry = Arc::new(plugin_registry);

    let note_name = if let Some(name) = name {
        name
    } else {
        // Interactive selection
        match interactive_select(store.as_ref())? {
            Some(name) => name,
            None => return Ok(()),
        }
    };

    let initial_content = if let Some(plugin_name) = note_name.strip_prefix('!') {
        let generated = plugin_registry
            .generate(plugin_name, store.as_ref())
            .map_err(|err| format!("Error generating plugin '{plugin_name}': {err}"))?;
        let document = markdown::parse(Cursor::new(generated.into_bytes()))
            .map_err(|e| format!("Error parsing FTML: {}", e))?;
        LoadedContent {
            document,
            location: ContentLocation::Plugin,
        }
    } else {
        let doc = store.load(&note_name)?;
        if doc.content.is_empty() {
            println!("(empty)");
            return Ok(());
        }
        let document_path = fs::canonicalize(&doc.path).unwrap_or_else(|_| doc.path.clone());
        let document = markdown::parse(Cursor::new(doc.content.into_bytes()))
            .map_err(|e| format!("Error parsing FTML: {}", e))?;
        LoadedContent {
            document,
            location: ContentLocation::File(document_path),
        }
    };

    let stdout_is_tty = io::stdout().is_terminal();
    let use_ansi = stdout_is_tty;
    let use_pager = use_ansi;

    if !use_pager {
        let mut formatter = if use_ansi {
            let mut style = FormattingStyle::ansi();
            configure_style_for_terminal(&mut style);
            Formatter::new(io::stdout(), style)
        } else {
            Formatter::new_ascii(io::stdout())
        };

        return formatter
            .write_document(&initial_content.document)
            .map_err(|err| format!("Error rendering FTML: {err}"));
    }

    let shared_state = Arc::new(Mutex::new(LinkEnvironment {
        document: initial_content.document.clone(),
        location: initial_content.location.clone(),
    }));

    let initial = render_document_for_terminal(&initial_content.document)?;
    let regen_state = shared_state.clone();
    let regenerator = move |new_width: u16, _new_height: u16| -> Result<String, String> {
        let guard = regen_state
            .lock()
            .map_err(|_| "Failed to access document for resize".to_string())?;
        render_document_for_width(&guard.document, new_width as usize)
    };

    let link_policy = build_link_policy(
        &notes_dir_buf,
        &canonical_notes_dir,
        &initial_content.location,
        &plugin_registry,
    );
    let link_callback: Arc<dyn tdoc_pager::LinkCallback> = Arc::new(LinkCallbackState::new(
        shared_state.clone(),
        notes_dir_buf.clone(),
        canonical_notes_dir.clone(),
        store.clone(),
        plugin_registry.clone(),
    ));

    let options = tdoc_pager::PagerOptions {
        link_policy,
        link_callback: Some(link_callback),
        ..tdoc_pager::PagerOptions::default()
    };

    tdoc_pager::page_output_with_options_and_regenerator(&initial, Some(regenerator), options)
}

#[derive(Clone)]
enum ContentLocation {
    File(PathBuf),
    Plugin,
}

struct LoadedContent {
    document: Document,
    location: ContentLocation,
}

enum LinkTarget {
    File(PathBuf),
    Plugin(String),
}

struct LinkEnvironment {
    document: Document,
    location: ContentLocation,
}

struct LinkCallbackState {
    shared: Arc<Mutex<LinkEnvironment>>,
    notes_dir: PathBuf,
    canonical_notes_dir: PathBuf,
    store: Arc<DocumentStore>,
    plugin_registry: Arc<PluginRegistry>,
}

impl LinkCallbackState {
    fn new(
        shared: Arc<Mutex<LinkEnvironment>>,
        notes_dir: PathBuf,
        canonical_notes_dir: PathBuf,
        store: Arc<DocumentStore>,
        plugin_registry: Arc<PluginRegistry>,
    ) -> Self {
        Self {
            shared,
            notes_dir,
            canonical_notes_dir,
            store,
            plugin_registry,
        }
    }
}

impl tdoc_pager::LinkCallback for LinkCallbackState {
    fn on_link(
        &self,
        target: &str,
        context: &mut tdoc_pager::LinkCallbackContext<'_>,
    ) -> Result<(), String> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return Ok(());
        }

        context.set_status(format!("Loading {trimmed} ..."))?;

        let current_location = {
            let guard = self
                .shared
                .lock()
                .map_err(|_| "Unable to read current document state".to_string())?;
            guard.location.clone()
        };

        match load_internal_content(
            self.store.as_ref(),
            self.plugin_registry.as_ref(),
            &self.notes_dir,
            &self.canonical_notes_dir,
            &current_location,
            trimmed,
        ) {
            Ok(Some(loaded)) => {
                let LoadedContent { document, location } = loaded;
                let render_width = context.content_width().max(1);
                let rendered = render_document_for_width(&document, render_width)?;
                context.replace_content(&rendered)?;
                context.set_link_policy(build_link_policy(
                    &self.notes_dir,
                    &self.canonical_notes_dir,
                    &location,
                    &self.plugin_registry,
                ));
                {
                    let mut guard = self
                        .shared
                        .lock()
                        .map_err(|_| "Unable to update current document state".to_string())?;
                    guard.document = document;
                    guard.location = location;
                }
                context.clear_status()?;
            }
            Ok(None) => {
                context.set_status("Unable to open link".to_string())?;
            }
            Err(err) => {
                context.set_status(format!("Error: {err}"))?;
            }
        }

        Ok(())
    }
}

fn build_link_policy(
    notes_dir: &Path,
    canonical_notes_dir: &Path,
    location: &ContentLocation,
    plugin_registry: &Arc<PluginRegistry>,
) -> LinkPolicy {
    let notes_dir_owned = notes_dir.to_path_buf();
    let canonical_owned = canonical_notes_dir.to_path_buf();
    let location_owned = location.clone();
    let plugin_registry = Arc::clone(plugin_registry);

    LinkPolicy::new(
        true,
        Arc::new(move |target: &str| {
            resolve_link_target(
                &notes_dir_owned,
                &canonical_owned,
                &location_owned,
                target,
                plugin_registry.as_ref(),
            )
            .is_some()
        }),
    )
}

fn configure_style_for_terminal(style: &mut FormattingStyle) {
    if let Ok((width, _height)) = terminal::size() {
        configure_style_for_width(style, width as usize);
    }
}

fn configure_style_for_width(style: &mut FormattingStyle, width: usize) {
    if width < 60 {
        style.wrap_width = width - 1; // for the scrollbar
        style.left_padding = 0;
    } else if width < 100 {
        style.wrap_width = width.saturating_sub(2);
        style.left_padding = 2;
    } else {
        let padding = (width.saturating_sub(100)) / 2 + 4;
        style.wrap_width = width.saturating_sub(padding);
        style.left_padding = padding;
    }
}

fn render_document_for_terminal(document: &Document) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut style = FormattingStyle::ansi();
    configure_style_for_terminal(&mut style);
    {
        let mut formatter = Formatter::new(&mut buf, style);
        formatter
            .write_document(document)
            .map_err(|err| format!("Unable to write document: {err}"))?;
    }
    String::from_utf8(buf).map_err(|err| format!("UTF-8 error: {err}"))
}

fn render_document_for_width(document: &Document, width: usize) -> Result<String, String> {
    let mut buf = Vec::new();
    let mut style = FormattingStyle::ansi();
    configure_style_for_width(&mut style, width);
    {
        let mut formatter = Formatter::new(&mut buf, style);
        formatter
            .write_document(document)
            .map_err(|err| format!("Unable to write document: {err}"))?;
    }
    String::from_utf8(buf).map_err(|err| format!("UTF-8 error: {err}"))
}

fn normalize_base_path(path: &Path) -> PathBuf {
    fs::canonicalize(path)
        .or_else(|_| {
            if path.is_absolute() {
                Ok(path.to_path_buf())
            } else {
                env::current_dir().map(|cwd| cwd.join(path))
            }
        })
        .unwrap_or_else(|_| path.to_path_buf())
}

fn resolve_link_target(
    notes_dir: &Path,
    canonical_notes_dir: &Path,
    current_location: &ContentLocation,
    target: &str,
    plugin_registry: &PluginRegistry,
) -> Option<LinkTarget> {
    let trimmed = target.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') || is_absolute_url(trimmed) {
        return None;
    }

    let path_part = trimmed.split('#').next().unwrap_or(trimmed).trim();
    if path_part.is_empty() {
        return None;
    }

    if let Some(plugin_name) = path_part.strip_prefix('!')
        && plugin_registry.has_plugin(plugin_name)
    {
        return Some(LinkTarget::Plugin(plugin_name.to_string()));
    }

    let raw_path = Path::new(path_part);

    let base_dir = match current_location {
        ContentLocation::File(path) => path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| canonical_notes_dir.to_path_buf()),
        ContentLocation::Plugin => canonical_notes_dir.to_path_buf(),
    };

    let resolved_base = if raw_path.is_absolute() {
        let stripped = raw_path.strip_prefix(Path::new("/")).unwrap_or(raw_path);
        notes_dir.join(stripped)
    } else {
        base_dir.join(raw_path)
    };

    // Prefer the `.md` version of the target, falling back to the raw path
    // (e.g. for links to assets). We append `.md` rather than using
    // `with_extension`, which would mangle dotted note names like
    // "sprint-q2.6" into "sprint-q2.md".
    let mut candidates = Vec::new();
    if !has_md_extension(path_part) {
        let mut with_md = resolved_base.clone().into_os_string();
        with_md.push(".md");
        candidates.push(PathBuf::from(with_md));
    }
    candidates.push(resolved_base);

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        if let Ok(canonical_candidate) = fs::canonicalize(&candidate)
            && canonical_candidate.starts_with(canonical_notes_dir)
        {
            return Some(LinkTarget::File(canonical_candidate));
        }
    }

    None
}

fn load_internal_content(
    store: &DocumentStore,
    plugin_registry: &PluginRegistry,
    notes_dir: &Path,
    canonical_notes_dir: &Path,
    current_location: &ContentLocation,
    target: &str,
) -> Result<Option<LoadedContent>, String> {
    match resolve_link_target(
        notes_dir,
        canonical_notes_dir,
        current_location,
        target,
        plugin_registry,
    ) {
        Some(LinkTarget::File(path)) => {
            let content = fs::read_to_string(&path)
                .map_err(|err| format!("Unable to read {}: {}", path.display(), err))?;
            let document = markdown::parse(Cursor::new(content.into_bytes()))
                .map_err(|err| format!("Error parsing FTML: {}", err))?;
            Ok(Some(LoadedContent {
                document,
                location: ContentLocation::File(path),
            }))
        }
        Some(LinkTarget::Plugin(plugin_name)) => {
            let generated = plugin_registry.generate(&plugin_name, store)?;
            let document = markdown::parse(Cursor::new(generated.into_bytes()))
                .map_err(|err| format!("Error parsing FTML: {}", err))?;
            Ok(Some(LoadedContent {
                document,
                location: ContentLocation::Plugin,
            }))
        }
        None => Ok(None),
    }
}

fn is_absolute_url(value: &str) -> bool {
    if value.starts_with("//") {
        return true;
    }
    Url::parse(value).is_ok()
}

fn cmd_ls(notes_dir: &Path) -> Result<(), String> {
    let store = DocumentStore::new(notes_dir.to_path_buf());
    let mut docs = store.list_all_documents()?;
    docs.sort();

    for doc in docs {
        println!("{}", doc);
    }

    Ok(())
}

/// ANSI escape sequences used when stdout is a TTY. Bold cyan for the note
/// name, green for the line number, bold red for the matched terms — the same
/// visual grammar `grep --color` and `rg` use, so the output reads familiarly.
const C_NAME: &str = "\x1b[1;36m";
const C_LINE: &str = "\x1b[32m";
const C_MATCH: &str = "\x1b[1;31m";
const C_RESET: &str = "\x1b[0m";

/// Wrap every case-insensitive occurrence of any term in `line` with the match
/// colour. Boundary-safe: it only does offset-based highlighting when
/// lowercasing preserved the byte length (i.e. plain ASCII case folding) and the
/// computed offsets fall on `char` boundaries; otherwise it returns the line
/// untouched rather than risk slicing mid-character.
fn highlight_terms(line: &str, terms: &[String], enabled: bool) -> String {
    if !enabled || terms.is_empty() {
        return line.to_string();
    }

    let lower = line.to_lowercase();
    if lower.len() != line.len() {
        // Non-ASCII case folding changed the byte length, so offsets in `lower`
        // no longer map onto `line`. Show the line without highlights.
        return line.to_string();
    }

    // Collect the byte ranges of every term occurrence, then merge overlaps so
    // adjacent/overlapping matches don't produce nested colour codes.
    let mut ranges: Vec<(usize, usize)> = Vec::new();
    for term in terms {
        let mut from = 0;
        while let Some(pos) = lower[from..].find(term.as_str()) {
            let start = from + pos;
            let end = start + term.len();
            ranges.push((start, end));
            from = end.max(start + 1);
        }
    }
    if ranges.is_empty() {
        return line.to_string();
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in ranges {
        match merged.last_mut() {
            Some(last) if start <= last.1 => last.1 = last.1.max(end),
            _ => merged.push((start, end)),
        }
    }

    let mut out = String::with_capacity(line.len() + merged.len() * 12);
    let mut cursor = 0;
    for (start, end) in merged {
        if start < cursor || !line.is_char_boundary(start) || !line.is_char_boundary(end) {
            continue;
        }
        out.push_str(&line[cursor..start]);
        out.push_str(C_MATCH);
        out.push_str(&line[start..end]);
        out.push_str(C_RESET);
        cursor = end;
    }
    out.push_str(&line[cursor..]);
    out
}

fn cmd_search(terms: Vec<String>, notes_dir: &Path) -> Result<(), String> {
    let store = DocumentStore::new(notes_dir.to_path_buf());
    let query = terms.join(" ");
    let parsed = piki_core::search::parse_terms(&query);
    let results = piki_core::search::search_store(&store, &query)?;

    if results.is_empty() {
        eprintln!("No matches for “{}”.", query);
        return Ok(());
    }

    let use_color = io::stdout().is_terminal();
    for note in &results {
        for (line_no, text) in &note.lines {
            let shown = highlight_terms(text.trim(), &parsed, use_color);
            if use_color {
                println!(
                    "{C_NAME}{}{C_RESET}:{C_LINE}{line_no}{C_RESET}: {shown}",
                    note.name
                );
            } else {
                println!("{}:{line_no}: {shown}", note.name);
            }
        }
    }

    Ok(())
}

fn cmd_log(count: usize, notes_dir: &PathBuf) -> Result<(), String> {
    let output = Command::new("git")
        .args([
            "log",
            &format!("-n{}", count),
            "--pretty=format:* %ad %s",
            "--date=short",
        ])
        .current_dir(notes_dir)
        .output()
        .map_err(|e| format!("Failed to run git log: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "git log failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    print!("{}", String::from_utf8_lossy(&output.stdout));
    Ok(())
}

fn cmd_run(command: Vec<String>, notes_dir: &PathBuf) -> Result<(), String> {
    if command.is_empty() {
        return Err("No command specified".to_string());
    }

    let status = Command::new(&command[0])
        .args(&command[1..])
        .current_dir(notes_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("Failed to run command: {}", e))?;

    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }

    Ok(())
}

fn cmd_index(notes_dir: &Path) -> Result<(), String> {
    cmd_view(Some("!index".to_string()), notes_dir)
}

fn cmd_todo(notes_dir: &Path) -> Result<(), String> {
    cmd_view(Some("!todo".to_string()), notes_dir)
}

fn print_help_with_aliases(config: &Config) {
    println!("piki - a simple personal wiki");
    println!();
    println!("Usage: piki [-d DIRECTORY] [COMMAND]");
    println!();
    println!("If no command is given the note to edit can be selected interactively.");
    println!();
    println!("Options:");
    println!(
        "  -d, --directory DIRECTORY - Directory containing markdown files (default: ~/.piki)"
    );
    println!();
    println!("Commands:");
    println!("  edit [name] - edit a note");
    println!("  help        - show this help");
    println!("  index       - generate an index of all notes");
    println!("  init [--from HOST [--path PATH]]");
    println!("              - create the notes directory, or import it from another machine");
    println!("  log         - show the commit log");
    println!("  ls          - list notes");
    println!("  remote add NAME [URL] [--path PATH]");
    println!("              - add a machine to sync with (NAME is its SSH host)");
    println!("  remote ls   - list remotes ('*' marks the ones being synced)");
    println!("  remote rm NAME - remove a remote");
    println!("  run [cmd]   - run a shell command inside the notes directory");
    println!("  search [terms] - full-text search notes (all terms must match)");
    println!("  sync        - commit local changes and sync with the configured remotes");
    println!("  todo        - list all todos from all notes");
    println!("  view [name] - view a note");

    if !config.aliases.is_empty() {
        println!();
        println!("Aliases:");
        let mut aliases: Vec<_> = config.aliases.iter().collect();
        aliases.sort_by_key(|(k, _)| *k);
        for (alias, command) in aliases {
            println!("  {} => {}", alias, command);
        }
    }
}

fn main() {
    // Load config and check for aliases
    let config = load_config();
    let raw_args: Vec<String> = env::args().collect();

    // Check if user is asking for help
    if raw_args.len() > 1 {
        let first_arg = &raw_args[1];
        if first_arg == "help" || first_arg == "--help" || first_arg == "-h" {
            print_help_with_aliases(&config);
            std::process::exit(0);
        }
    }

    // Parse arguments to get the directory option and other args
    let args = Args::parse();
    let notes_dir = get_notes_dir(args.directory.clone());

    // A missing notes directory is set up interactively: created fresh or
    // imported from another machine. `piki init` does the same explicitly.
    if !notes_dir.exists()
        && !matches!(args.command, Some(Commands::Init { .. }))
        && let Err(e) = set_up_missing_notes_dir(&notes_dir, &config)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }

    // Check if first non-option argument is an alias
    // Skip program name and any -d/--directory options
    let mut first_positional = None;
    let mut skip_next = false;
    for arg in raw_args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        if arg == "-d" || arg == "--directory" {
            skip_next = true;
            continue;
        }
        if arg.starts_with("-d=") || arg.starts_with("--directory=") || arg.starts_with("-") {
            continue;
        }
        first_positional = Some(arg.as_str());
        break;
    }

    // Check if first positional argument is an alias
    if let Some(potential_alias) = first_positional
        && let Some(alias_cmd) = config.aliases.get(potential_alias)
    {
        // Execute the alias as a shell command in the notes directory
        let status = Command::new("sh")
            .arg("-c")
            .arg(alias_cmd)
            .current_dir(&notes_dir)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status();

        match status {
            Ok(status) => std::process::exit(status.code().unwrap_or(0)),
            Err(e) => {
                eprintln!("Error: Failed to run alias '{}': {}", potential_alias, e);
                std::process::exit(1);
            }
        }
    }

    let result = match args.command {
        Some(Commands::Edit { name }) => cmd_edit(name, &notes_dir, &config),
        Some(Commands::Index) => cmd_index(&notes_dir),
        Some(Commands::Init { from, path }) => cmd_init(from, path, &notes_dir, &config),
        Some(Commands::View { name }) => cmd_view(name, &notes_dir),
        Some(Commands::Ls) => cmd_ls(&notes_dir),
        Some(Commands::Log { count }) => cmd_log(count, &notes_dir),
        Some(Commands::Remote { action }) => cmd_remote(action, &notes_dir, &config),
        Some(Commands::Run { command }) => cmd_run(command, &notes_dir),
        Some(Commands::Search { terms }) => cmd_search(terms, &notes_dir),
        Some(Commands::Sync) => cmd_sync(&notes_dir, &config),
        Some(Commands::Todo) => cmd_todo(&notes_dir),
        None => {
            // Default to edit command, either with provided name or interactive
            cmd_edit(args.name, &notes_dir, &config)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
