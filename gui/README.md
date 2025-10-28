# ✜ Piki – GUI app

**FLTK-based graphical interface for Piki personal wiki**

> [!IMPORTANT]  
> This is the crate-level README for the `piki-gui` application only. For overall Piki documentation, see the [main repo](https://github.com/roblillack/piki).

A lightweight, cross-platform GUI for managing your personal wiki with a custom rich-text Markdown editor. Features live rendering, keyboard shortcuts, and auto-save.

## Installation

```bash
cargo install piki-gui
```

### System Requirements

- **macOS**: No additional dependencies
- **Linux/BSD**: Wayland/X11 development libraries
- **Windows**: No additional dependencies

## Quick Start

```bash
# Open to frontpage
piki-gui

# Open with custom wiki path
piki-gui -d /path/to/wiki
```

## Features

### Rich-Text Editing

- **Live Markdown rendering** as you type
- **Visual hierarchy** for headers (H1, H2, H3)
- **Inline styles**: Bold, italic, code, strikethrough, underline, highlighting
- **Block elements**: Code blocks, blockquotes, lists
- **Clickable links** for easy navigation

### Keyboard Shortcuts

| Shortcut              | Action            |
| --------------------- | ----------------- |
| **Navigation**        |                   |
| `Cmd+N`               | New page          |
| `Cmd+P`               | Open page picker  |
| `Cmd+[`               | Back              |
| `Cmd+]`               | Forward           |
| `Cmd+Option+F`        | Jump to frontpage |
| `Cmd+Option+I`        | Open page index   |
| **Inline Styling**    |                   |
| `Cmd+B`               | Bold              |
| `Cmd+I`               | Italic            |
| `Cmd+U`               | Underline         |
| `Cmd+Shift+C`         | Inline code       |
| `Cmd+Shift+H`         | Highlight text    |
| `Cmd+Shift+X`         | Strikethrough     |
| `Cmd+K`               | Insert/Edit link  |
| `Cmd+\`               | Clear formatting  |
| **Paragraph Styling** |                   |
| `Cmd+Option+0`        | Text paragraph    |
| `Cmd+Option+1`        | Header 1          |
| `Cmd+Option+2`        | Header 2          |
| `Cmd+Option+3`        | Header 3          |
| `Cmd+Shift+5`         | Blockquote        |
| `Cmd+Shift+6`         | Code block        |
| `Cmd+Shift+7`         | Numbered list     |
| `Cmd+Shift+8`         | Bulleted list     |
| `Cmd+Shift+9`         | Checklist         |

Note: On Linux/Windows, use `Ctrl` instead of `Cmd`.

### Auto-Save

- Changes are saved automatically
- Status bar shows save status and last save time
- Creates parent directories as needed

### Link Formats

The editor supports multiple link formats:

- Standard Markdown: `[text](page.md)`
- Wiki-style: `[[PageName]]`
- Nested paths: `[[folder/page]]`

All links are clickable for quick navigation between pages.

### Plugin System

- **Dynamic pages** with `!` prefix
- Built-in **`!index`** plugin lists all pages
- Plugin pages are read-only
- Extensible for custom dynamic content

## Platform Support

| Platform    | Status | Notes                   |
| ----------- | ------ | ----------------------- |
| **macOS**   | ✅     | Native menu bar support |
| **Linux**   | ✅     | X11 or Wayland required |
| **Windows** | ✅     | Fully supported         |
| **BSD**     | ✅     | FreeBSD, OpenBSD, etc.  |

## Architecture

Built with:

- **FLTK**: Lightweight, fast, cross-platform GUI toolkit
- **pulldown-cmark**: Markdown parsing
- **Custom rich-text editor**: Live rendering with visual feedback
- **piki-core**: Shared document management library

## Why FLTK?

FLTK provides:

- Fast startup and low memory footprint
- Native performance without heavy frameworks
- Cross-platform consistency
- Easy keyboard shortcut handling
- Minimal dependencies

## License

MIT License
