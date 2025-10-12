# fliki-rs

A Rust reimplementation of fliki - a lightweight Markdown wiki browser with clickable links and **syntax highlighting**.

## Features

### Live Syntax Highlighting

- Headers (H1, H2, H3) in different sizes
- **Bold** and _italic_ text
- `Inline code` styling
- ~~Strikethrough~~ text
- <u>Underlined</u> text
- <mark>Highlighted</mark> text
- Code blocks (indented)
- > Blockquotes
- Links shown in blue
- **Updates instantly as you type!**

### Auto-Save

- Saves changes automatically 1 second after you stop typing
- Debounced to prevent excessive disk writes
- Status bar shows save status ("Saving...", "saved 2 min ago", etc.)
- Creates new files and parent directories as needed

### Interactive Navigation

- Click on links to navigate between pages
- Cursor changes to hand icon when over links
- Keyboard shortcuts for quick access
- Support for nested page paths (`[[project-a/standup]]`)

### Link Support

- Standard Markdown: `[text](page.md)`
- Wiki-style: `[[PageName]]`
- Nested paths: `[[folder/page]]`

### Plugin System

- Dynamic page generation with `!` prefix
- Built-in `!index` plugin shows all pages
- Plugin pages are read-only
- Extensible architecture for custom plugins

### Smart Status Bar

- Page status (left): Shows current page and type
- Save status (right): Real-time save feedback with time tracking

## Building

```bash
cd fliki-rs
cargo build --release
```

The binary will be at `target/release/fliki-rs`

## Usage

```bash
# Run with example wiki
cargo run --release -- example-wiki

# Or specify an initial page
cargo run --release -- example-wiki --page features

# After building
./target/release/fliki-rs /path/to/your/wiki
```

## Directory Structure

Your markdown directory should contain `.md` files. The application will:

- Load `frontpage.md` by default (or specify with `--page`)
- Follow links to other markdown files in the same directory
- Support both `file.md` and `file` link formats
- Apply syntax highlighting automatically

## Example Wiki

An example wiki is included in `example-wiki/`:

```bash
cargo run --release -- example-wiki
```

Try clicking on the links to see navigation in action!

## Keyboard Shortcuts

- `Ctrl+F` (or `Cmd+F` on Mac) - Go to frontpage
- `Ctrl+I` (or `Cmd+I` on Mac) - Go to dynamic index (`!index` plugin)

## Platform Integration

- **macOS**: Uses native system menu bar (appears at top of screen)
- **Linux/Windows**: Uses window menu bar (appears in application window)

## Syntax Highlighting Examples

The editor supports:

```markdown
# Header 1

## Header 2

### Header 3

**Bold text** and _italic text_

`inline code` and:

    indented code blocks
    with monospace font

> Blockquotes
> in red italic

[Standard links](page.md) and [[wiki links]]
```

## Implementation Details

### Architecture

- `main.rs` - Application entry point, window management, event handling
- `document.rs` - File system operations for loading/saving markdown files
- `editor.rs` - Custom text editor with syntax highlighting using FLTK style buffers
- `link_handler.rs` - Markdown and wiki-link parsing using `pulldown-cmark`
- `autosave.rs` - Auto-save state management and debouncing
- `plugin.rs` - Plugin system for dynamic content generation

### How Link Following Works

1. When you click in the editor, the click position is captured
2. The link parser identifies all links and their positions in the document
3. If the click position falls within a link's range, that link is followed
4. The target page is loaded and displayed with fresh syntax highlighting

### How Syntax Highlighting Works

1. On every keypress, the widget's `Changed` trigger fires
2. An `awake_callback` schedules immediate restyling on the next event loop
3. Text is analyzed line-by-line for block-level formatting (headers, quotes, code)
4. Inline styles (bold, italic, code) are parsed within each line
5. Links are highlighted after other formatting
6. A parallel style buffer maps each character to a style entry
7. FLTK's TextEditor renders text according to the style table
8. Restyling happens instantly - no delays or timers

## Differences from Original fliki

This Rust version:

- ✅ Uses Markdown instead of custom Liki markup
- ✅ Reads from local filesystem instead of network
- ✅ Has full syntax highlighting for Markdown
- ✅ Clickable links with visual feedback
- ✅ Auto-save functionality
- ✅ Plugin system for dynamic pages
- ✅ Nested directory support
- ❌ No page locking/unlocking (uses auto-save instead)
- ❌ No server push/pull operations (local-only)
- ❌ No multi-user editing (single-user with file locking possible)

## Dependencies

- `fltk` 1.4 - Cross-platform GUI toolkit
- `pulldown-cmark` 0.11 - Markdown parser
- `clap` 4.5 - Command-line argument parsing
- `regex` 1.10 - Pattern matching for wiki links
- `walkdir` 2.5 - Directory traversal

## Contributing

This is a demonstration project showing how to build a wiki-style editor in Rust with:

- FLTK for GUI
- Syntax highlighting using style buffers
- Event-driven navigation
- Markdown parsing

Feel free to extend it with features like:

- Manual save shortcut (Ctrl+S)
- Search across pages
- Link preview on hover
- Backlinks
- Full-text search
- Recent pages history
- Version control integration
- Custom plugins
- Configurable auto-save delay

## Documentation

- [AUTOSAVE.md](AUTOSAVE.md): Auto-save implementation details
- [PLUGIN_SYSTEM.md](PLUGIN_SYSTEM.md): Plugin system guide
- [READONLY_IMPLEMENTATION.md](READONLY_IMPLEMENTATION.md): Read-only mode for plugins

## License

Same as the original fliki project.
