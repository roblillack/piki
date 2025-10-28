# ✜ Piki – CLI tool

**Command-line interface for Piki personal wiki**

> [!IMPORTANT]  
> This is the crate-level README for the `piki` CLI only. For overall Piki documentation, see the [main repo](https://github.com/roblillack/piki).

A fast, lightweight CLI for managing your personal wiki with plain Markdown files. Perfect for quick edits, scripting, and terminal-based workflows.

## Installation

```bash
cargo install piki
```

## Quick Start

```bash
# Initialize your wiki
mkdir ~/.piki
cd ~/.piki
echo "# My Wiki" > frontpage.md

# Edit interactively (fuzzy picker)
piki

# Edit a specific note
piki edit frontpage

# List all notes
piki ls

# View a note
piki view frontpage
```

## Usage

```bash
piki [options] [command]

Options:
  -d, --directory DIRECTORY   Directory containing markdown files (default: ~/.piki)

Commands:
  edit [name]   Edit a note (opens in $EDITOR or $VISUAL, defaults to vim)
  view [name]   View a note
  ls            List all notes
  log [-n NUM]  Show git commit log (if using git)
  run [cmd]     Run a shell command inside the notes directory
  help          Show help information
```

## Configuration

Create a `~/.pikirc` file to define custom aliases and shortcuts:

```toml
[aliases]
# Daily notes
today = "code . -g daily/$(date +'%Y-%m-%d').md"
standup = "vim work/standup-$(date +'%Y').md"

# Git shortcuts
status = "git status -u"
sync = "git ci -m 'Auto-sync' && git pull --rebase && git push"
push = "git commit -m 'Auto-sync' && git push"

# Open in your favorite editor/IDE
code = "code ."
cfg = "vim ~/.pikirc"

# Launch GUI from CLI
g = "piki-gui"
```

## Interactive Mode

When no command is specified, Piki opens an interactive fuzzy picker for quickly finding and editing notes:

```bash
piki -d ~/my-wiki
# Type to filter notes, arrow keys to navigate, Enter to edit
```

## Example Workflows

```bash
# Daily note workflow
piki edit "daily/$(date +'%Y-%m-%d')"

# Quick capture
piki edit inbox

# Browse and edit
piki -d ~/my-wiki  # Interactive picker

# View without editing
piki view project-ideas

# Git integration
piki run git status
piki log -n 10
```

## Git Integration

Piki works seamlessly with Git for version control:

```bash
cd ~/.piki
git init
git add .
git commit -m "Initial wiki"

# Use piki's git commands
piki log
piki run git status

# Or use aliases in .pikirc
piki sync    # Commit, pull, push
piki push    # Commit and push
```

## Features

- **Local-first**: Your notes are plain Markdown files
- **Fuzzy Search**: Fast interactive note picker
- **Git Integration**: Built-in git log and run commands
- **Customizable**: Define aliases for your workflow
- **Cross-platform**: Works on Windows, macOS, Linux, and BSD
- **Fast**: Written in Rust for performance

## License

MIT License
