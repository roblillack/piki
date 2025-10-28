# ✜ Piki – core library

**Core library for Piki personal wiki application**

> [!IMPORTANT]  
> This is the crate-level README for the `piki-core` library only. For overall Piki documentation, see the [main repo](https://github.com/roblillack/piki).

This crate provides the shared functionality used by both the CLI and GUI frontends of Piki. It handles document storage, plugin system, and core wiki operations.

## Overview

`piki-core` is a Rust library that provides the foundation for managing a personal wiki using plain Markdown files. It's designed to be backend-agnostic, allowing different frontends (CLI, GUI, etc.) to build on top of it.

## Features

- **Document Store**: Manages Markdown files on the filesystem
- **Plugin System**: Extensible architecture for dynamic pages
- **Git-friendly**: Works seamlessly with version-controlled directories
- **Cross-platform**: Works on Windows, macOS, Linux, and BSD
- **No Dependencies on UI**: Pure Rust library with no GUI dependencies

## Usage

Add this to your `Cargo.toml`:

```toml
[dependencies]
piki-core = "0.1.1"
```

## Architecture

The core library provides:

- **Document Management**: Reading, writing, and organizing Markdown files
- **Plugin System**: Built-in `!index` plugin and extensible plugin API for dynamic content
- **Link Resolution**: Handling both Markdown links (`[text](page.md)`) and wiki-style links (`[[PageName]]`)
- **File Operations**: Safe file I/O with parent directory creation

## Plugin System

The plugin system allows for dynamic pages with a `!` prefix. Built-in plugins include:

- `!index` - Lists all pages in the wiki

Plugins are read-only and generate content dynamically rather than being stored on disk.

## License

MIT License
