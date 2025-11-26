use clap::{Parser, Subcommand};
use crossterm::terminal;
use fuzzypicker::FuzzyPicker;
use piki_core::{DocumentStore, IndexPlugin, Plugin, PluginRegistry, TodoPlugin};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Cursor, IsTerminal};
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use tdoc::formatter::{Formatter, FormattingStyle};
use tdoc::{Document, LinkPolicy, markdown, pager as tdoc_pager};
use url::Url;

mod pager;

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
    /// Generate an index of all pages
    Index,
    /// Show the commit log
    Log {
        /// Number of commits to show
        #[arg(short = 'n', default_value = "25")]
        count: usize,
    },
    /// List all notes
    Ls,
    /// Run a shell command inside the notes directory
    Run {
        /// Command to run
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        command: Vec<String>,
    },
    /// List all todos from all pages
    Todo,
    /// View a note
    View {
        /// Name of the note to view
        name: Option<String>,
    },
}

#[derive(Deserialize, Debug, Default)]
struct Config {
    #[serde(default)]
    aliases: HashMap<String, String>,
}

impl Config {
    fn load() -> Self {
        let config_path = Self::config_path();
        if let Some(path) = config_path
            && path.exists()
            && let Ok(contents) = fs::read_to_string(&path)
            && let Ok(config) = toml::from_str::<Config>(&contents)
        {
            return config;
        }
        Config::default()
    }

    fn config_path() -> Option<PathBuf> {
        env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".pikirc"))
    }
}

fn get_notes_dir(dir_opt: Option<PathBuf>) -> PathBuf {
    dir_opt.unwrap_or_else(|| {
        env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".piki"))
            .unwrap_or_else(|| PathBuf::from(".piki"))
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

fn cmd_edit(name: Option<String>, notes_dir: &PathBuf) -> Result<(), String> {
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

    Ok(())
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
        && plugin_registry.has_plugin(plugin_name) {
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

    let mut candidates = Vec::new();
    if raw_path.extension().is_none() {
        candidates.push(resolved_base.with_extension("md"));
    }
    candidates.push(resolved_base);

    for candidate in candidates {
        if !candidate.exists() {
            continue;
        }
        if let Ok(canonical_candidate) = fs::canonicalize(&candidate)
            && canonical_candidate.starts_with(canonical_notes_dir) {
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
    let store = DocumentStore::new(notes_dir.to_path_buf());
    let plugin = IndexPlugin;
    let content = plugin.generate_content(&store)?;
    pager::page_output(&content)?;
    Ok(())
}

fn cmd_todo(notes_dir: &Path) -> Result<(), String> {
    let store = DocumentStore::new(notes_dir.to_path_buf());
    let plugin = TodoPlugin;
    let content = plugin.generate_content(&store)?;
    pager::page_output(&content)?;
    Ok(())
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
    println!("  index       - generate an index of all pages");
    println!("  log         - show the commit log");
    println!("  ls          - list notes");
    println!("  run [cmd]   - run a shell command inside the notes directory");
    println!("  todo        - list all todos from all pages");
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
    let config = Config::load();
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

    // Ensure notes directory exists
    if !notes_dir.exists()
        && let Err(e) = fs::create_dir_all(&notes_dir)
    {
        eprintln!(
            "Error: Failed to create notes directory '{}': {}",
            notes_dir.display(),
            e
        );
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
        Some(Commands::Edit { name }) => cmd_edit(name, &notes_dir),
        Some(Commands::Index) => cmd_index(&notes_dir),
        Some(Commands::View { name }) => cmd_view(name, &notes_dir),
        Some(Commands::Ls) => cmd_ls(&notes_dir),
        Some(Commands::Log { count }) => cmd_log(count, &notes_dir),
        Some(Commands::Run { command }) => cmd_run(command, &notes_dir),
        Some(Commands::Todo) => cmd_todo(&notes_dir),
        None => {
            // Default to edit command, either with provided name or interactive
            cmd_edit(args.name, &notes_dir)
        }
    };

    if let Err(e) = result {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
