# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While pre-1.0, the minor version is bumped for breaking changes.

## [Unreleased]

### Added

- Editing now offers **two caret positions at the edges of styled text** (bold,
  italic, code, links, …), even with Reveal Codes off: arrowing to the border of a
  bold word stops once "before" and once "inside" the style, letting you choose
  whether text you type there picks up the style. The caret shows which side it
  will apply by leaning that way, with small angled tails at its top and bottom.
  (via rutle)
- _Rename Note_ (`Cmd-S`/`Ctrl-S`) renames the currently open note: it moves the
  note's file on disk and carries the note's navigation history, picker recency,
  and remembered scroll position over to the new name. For a freshly created,
  still-unnamed note the dialog opens blank; for a note that already has a name it
  is pre-filled, so the command doubles as a plain rename. Renaming to a name that
  is already taken is refused, and read-only plugin views (e.g. `!index`) cannot
  be renamed. (#32)
- Returning to a recently visited note now resumes at the scroll position you
  left it at, rather than jumping to the top — for the last 10 notes, and via any
  navigation (links, the picker, back/forward). This memory is in-memory only and
  is not persisted across restarts.
- The note picker now remembers when each note was last opened and persists this
  per wiki (in the application data directory, keyed by a hash of the wiki's
  path). The recency ordering therefore survives restarts, and separate wikis
  opened with `--directory` keep independent histories.

### Changed

- _New Note_ (`Cmd-N`/`Ctrl-N`) no longer asks for a name up front. It now
  immediately creates and opens an auto-named note (e.g.
  `untitled_2026-07-04_153412.md`) so a quick thought can be captured without
  stopping to name it first; give it a real name afterwards with _Rename Note_
  (`Cmd-S`/`Ctrl-S`). Because an untitled note is only written to disk once you
  type into it, pressing _New Note_ and navigating away leaves no stray files
  behind. (#32)
- The note picker has been reworked and now opens with `Cmd-O`/`Ctrl-O`
  (previously "Go to Page" on `Cmd-P`/`Ctrl-P`). With an empty query it lists
  notes by last-opened date; every row shows a one-line plaintext preview of the
  note's first paragraphs (Markdown stripped, ellipsized to fit) alongside its
  last-modification time. Keyboard interaction mirrors VS Code's quick-open: type
  to filter, move with the arrow keys, or keep the modifier held after opening
  and tap `O` again to step the selection down (`Shift` to go up) — releasing the
  modifier opens the highlighted note. The currently open note starts selected.
- User-facing wording now says "note" instead of "page": the _Page_ menu is now
  _Note_ (with _New Note …_ and _Open Note …_), and the new-note dialog and the
  status bar ("Note: …") follow suit.
- The text rendering and editing engine has been carved out of Piki into a new
  shared crate, `rutle` (`rutle = "0.2.0"` on crates.io), and the `gui` crate
  now builds on it instead of its own homegrown implementation. This removes
  roughly 10,600 lines — the entire `richtext` module (the structured document
  model, the editor, the rich-text display, the tdoc bridge, and the Markdown
  converter), the old theme and draw-context modules, and their SVG snapshot
  tests and bundled NotoSans fonts (all now maintained in rutle, so Piki also
  drops its `insta` and `rusttype` dev-dependencies). The sibling editor Pure
  builds on the same crate, and both resolve the identical crates.io
  `tdoc 0.11.0`, so `tdoc::Document` values are shared across the crate
  boundary unchanged. rutle renamed several items in the move (e.g.
  `StructuredRichDisplay` → `Renderer`, `StructuredEditor` → `Editor`,
  `DrawContext` → `RenderContext`) and flattened the `richtext` module to its
  crate root; `gui` now uses those names and module paths from `rutle`
  directly. The FLTK integration layer — `fltk_structured_rich_display.rs` and
  the FLTK draw context — implements rutle's `RenderContext` trait and drives
  its `Renderer` straight from the crate, and the small Markdown/HTML `tdoc`
  conversion wrappers Piki still needs for the clipboard and page load/save now
  live in `gui`'s own `markdown_converter` module. Rendering, selection, reveal
  codes, styled links, and table display are unchanged. As part of the shared
  core, rutle's layout cache no longer invalidates on an unchanged
  resize/padding update, which speeds up redraws in Piki as well. (#26)

### Removed

- The _View → Markdown editor_ mode has been removed. This was a separate
  plain-text editor that showed a note's raw Markdown source with rudimentary
  syntax highlighting, toggled from the View menu. It had fallen out of sync
  with the structured rich-text editor and no longer worked correctly, so the
  structured editor is now the only editor.

### Fixed

- Dragging the scrollbar thumb now tracks the mouse correctly. On long notes the
  thumb was far larger than the target FLTK actually let you grab, so a drag
  registered as a click in the trough and the view jumped the wrong way; the thumb
  also stopped short of the very top and bottom because FLTK reserved space for its
  (invisible) arrow buttons. Piki now draws and drives the thumb itself over the
  full track, so what you see is what you grab. Clicking in the track above or
  below the thumb pages by one screen and auto-repeats while held. (#30)
- Alt-Up/Down now reorders the block at the cursor's current nesting level, not
  just top-level paragraphs: list items, checklist items, and quote children can
  be resorted among their siblings, and a nested sub-item stays within its
  sublist (via `rutle 0.2.1`). (#27)
- Pasting rich text via RTF (e.g. from Word or Outlook) no longer inserts inert
  boxes where curly quotes, apostrophes, en/em dashes, or ellipses should be.
  RTF encodes these as Windows-1252 codepage bytes, but the RTF parser decoded
  the C1 range (`0x80`–`0x9F`) as raw Unicode scalars — turning `\'92` into the
  control character U+0092 instead of `'`. Piki now remaps that block to the
  characters the bytes actually stand for on import. (#26)

[Unreleased]: https://github.com/roblillack/piki/compare/piki-v0.3.0...HEAD
