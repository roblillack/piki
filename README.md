# fliki-rs

A Rust reimplementation of fliki - a lightweight Markdown wiki browser with clickable links and **syntax highlighting**.

## Features

✅ **Live Syntax Highlighting**
   - Headers (H1, H2, H3) in different sizes
   - **Bold** and *italic* text
   - `Inline code` styling
   - Code blocks (indented)
   - > Blockquotes
   - Links shown in blue
   - **Updates instantly as you type!**

✅ **Interactive Navigation**
   - Click on links to navigate between pages
   - Cursor changes to hand icon when over links
   - Keyboard shortcuts for quick access

✅ **Link Support**
   - Standard Markdown: `[text](page.md)`
   - Wiki-style: `[[PageName]]`

✅ **Simple FLTK-based GUI**
   - Clean text editor interface
   - Menu bar for navigation
   - Status bar showing current page

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

- `Ctrl+F` - Go to frontpage
- `Ctrl+I` - Go to INDEX

## Syntax Highlighting Examples

The editor supports:

```markdown
# Header 1
## Header 2
### Header 3

**Bold text** and *italic text*

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
- `document.rs` - File system operations for loading markdown files
- `editor.rs` - Custom text editor with syntax highlighting using FLTK style buffers
- `link_handler.rs` - Markdown and wiki-link parsing using `pulldown-cmark`

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
- ❌ No page locking/unlocking (filesystem-only)
- ❌ No server push/pull operations
- ❌ No multi-user editing

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
- Save functionality
- Search across pages
- Link preview on hover
- Backlinks
- Full-text search
- Recent pages history

## License

Same as the original fliki project.
