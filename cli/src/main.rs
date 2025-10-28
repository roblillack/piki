use clap::{Parser, Subcommand};
use fuzzypicker::FuzzyPicker;
use piki_core::{DocumentStore, IndexPlugin, Plugin, TodoPlugin};
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
    let store = DocumentStore::new(notes_dir.to_path_buf());

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

    if doc.content.is_empty() {
        println!("(empty)");
    } else {
        print!("{}", doc.content);
    }

    Ok(())
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
    print!("{}", content);
    Ok(())
}

fn cmd_todo(notes_dir: &Path) -> Result<(), String> {
    let store = DocumentStore::new(notes_dir.to_path_buf());
    let plugin = TodoPlugin;
    let content = plugin.generate_content(&store)?;
    print!("{}", content);
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
