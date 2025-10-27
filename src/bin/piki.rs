use clap::{Parser, Subcommand};
use piki::document::DocumentStore;
use serde::Deserialize;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Parser, Debug)]
#[command(name = "piki")]
#[command(about = "A simple personal wiki", long_about = None)]
struct Args {
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
        if let Some(path) = config_path {
            if path.exists() {
                if let Ok(contents) = fs::read_to_string(&path) {
                    if let Ok(config) = toml::from_str::<Config>(&contents) {
                        return config;
                    }
                }
            }
        }
        Config::default()
    }

    fn config_path() -> Option<PathBuf> {
        env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".pikirc"))
    }
}

fn get_notes_dir() -> PathBuf {
    env::var("HOME")
        .ok()
        .map(|home| PathBuf::from(home).join(".piki"))
        .unwrap_or_else(|| PathBuf::from(".piki"))
}

fn get_editor() -> String {
    env::var("VISUAL")
        .or_else(|_| env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string())
}

fn interactive_select(store: &DocumentStore) -> Result<Option<String>, String> {
    let docs = store.list_all_documents()?;

    if docs.is_empty() {
        return Ok(None);
    }

    // For now, use a simple numbered list selection
    // TODO: Replace with a proper fuzzy matcher like skim
    println!("Select a note:");
    for (i, doc) in docs.iter().enumerate() {
        println!("  {}: {}", i + 1, doc);
    }

    print!("\nEnter number (or 'q' to quit): ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();

    if input == "q" || input.is_empty() {
        return Ok(None);
    }

    if let Ok(num) = input.parse::<usize>() {
        if num > 0 && num <= docs.len() {
            return Ok(Some(docs[num - 1].clone()));
        }
    }

    Err("Invalid selection".to_string())
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

    let status = Command::new(&editor)
        .arg(&doc.path)
        .current_dir(notes_dir)
        .status()
        .map_err(|e| format!("Failed to open editor '{}': {}", editor, e))?;

    if !status.success() {
        return Err(format!("Editor exited with status: {}", status));
    }

    Ok(())
}

fn cmd_view(name: Option<String>, notes_dir: &PathBuf) -> Result<(), String> {
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

    if doc.content.is_empty() {
        println!("(empty)");
    } else {
        print!("{}", doc.content);
    }

    Ok(())
}

fn cmd_ls(notes_dir: &PathBuf) -> Result<(), String> {
    let store = DocumentStore::new(notes_dir.clone());
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

fn print_help_with_aliases(config: &Config) {
    println!("piki - a simple personal wiki");
    println!();
    println!("Usage: piki [COMMAND]");
    println!();
    println!("If no command is given the note to edit can be selected interactively.");
    println!();
    println!("Commands:");
    println!("  edit [name] - edit a note");
    println!("  help        - show this help");
    println!("  log         - show the commit log");
    println!("  ls          - list notes");
    println!("  run [cmd]   - run a shell command inside the notes directory");
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
    let notes_dir = get_notes_dir();

    // Ensure notes directory exists
    if !notes_dir.exists() {
        if let Err(e) = fs::create_dir_all(&notes_dir) {
            eprintln!(
                "Error: Failed to create notes directory '{}': {}",
                notes_dir.display(),
                e
            );
            std::process::exit(1);
        }
    }

    // Load config and check for aliases
    let config = Config::load();
    let args: Vec<String> = env::args().collect();

    // Check if user is asking for help
    if args.len() > 1 {
        let first_arg = &args[1];
        if first_arg == "help" || first_arg == "--help" || first_arg == "-h" {
            print_help_with_aliases(&config);
            std::process::exit(0);
        }
    }

    // Check if first argument (after program name) is an alias
    if args.len() > 1 {
        let potential_alias = &args[1];
        if let Some(alias_cmd) = config.aliases.get(potential_alias) {
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
    }

    // Parse arguments normally
    let args = Args::parse();

    let result = match args.command {
        Some(Commands::Edit { name }) => cmd_edit(name, &notes_dir),
        Some(Commands::View { name }) => cmd_view(name, &notes_dir),
        Some(Commands::Ls) => cmd_ls(&notes_dir),
        Some(Commands::Log { count }) => cmd_log(count, &notes_dir),
        Some(Commands::Run { command }) => cmd_run(command, &notes_dir),
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
