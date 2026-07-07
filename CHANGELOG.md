# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While pre-1.0, the minor version is bumped for breaking changes.

<!-- next-header -->

## [Unreleased] - ReleaseDate

### Added

- **Live Note Sharing.** _View → Live Note Sharing_
  (`Cmd-Shift-L`/`Ctrl-Shift-L`) starts a local, loopback-only webserver that
  renders the currently visible note as a clean, live-reloading HTML page and
  opens it in your browser — handy for showing a note in a screen-shared video
  call. While active, a red **ON AIR** bar at the top shows the shareable link
  (which follows whichever note is on screen) and a _Stop_ button. The browser
  view is independent of in-app navigation: it only ever shows the note in its
  own URL, so you can keep a "public" note open in the shared tab while taking
  notes in a private one. Links in the web view are followable, and edits appear
  in the browser within about a second. The server binds `127.0.0.1` on an
  OS-assigned port, so only your machine can reach it. A subtle footer shows the
  Piki version and toggles the page between one and two columns, the latter
  making better use of a shared widescreen (the choice is remembered). The page
  follows the viewer's system light/dark appearance automatically. (#39)
- **Delete a note.** _Note → Delete Note …_ removes the current note's file from
  disk after a confirmation dialog (Cancel is the default, so the destructive
  action is never the accidental choice), then returns to the frontpage. It has
  no keyboard shortcut on purpose. Read-only plugin views (e.g. the index) cannot
  be deleted. (#42)
- **Moving a paragraph now crosses block boundaries** (`Alt-Up`/`Alt-Down`,
  _Option-Up_/_Option-Down_ on macOS) instead of stopping at a list's edge. A
  list/checklist item at the edge of its list now leaves the list — keeping its
  bullet, number, or checkbox — steps past an intervening block such as a heading
  or paragraph, and merges into the next same-kind list it reaches, so two
  adjacent same-kind lists never end up side by side and ordered lists stay
  continuously numbered. A plain paragraph that meets a list or quote is drawn
  into it, and a quote's child at the quote's edge is lifted back out. In effect a
  block can now be reordered across a whole note — e.g. moved from one checklist
  to another past a heading — rather than only within its own list. (#40)
- **Moving now works on a multi-block selection.** When the selection spans
  several sibling blocks (paragraphs, list or checklist items, or quote children)
  `Alt-Up`/`Alt-Down` reorders them together and, at their container's edge,
  carries the whole run out of it the same way a single block does; the moved run
  stays selected. (via `rutle 0.4.0`) (#40)

### Fixed

- Turning a plain paragraph into a list or checklist item now merges it into an
  adjacent same-kind list instead of leaving a second, separate list beside it —
  so prepending a new entry by turning the paragraph above an existing checklist
  into a checklist item yields one checklist rather than two, and ordered lists
  renumber correctly. (via `rutle 0.4.0`) (#40)
- Pasting several copied list items (or other multi-paragraph rich content) into
  a list item now keeps links and text styling instead of dropping in the raw
  Markdown as literal text; each pasted paragraph becomes its own styled list
  item. (via `rutle 0.4.1`) (#41)
- Returning to a recently visited note now restores the caret position as well as
  the scroll offset, so navigating away and back — via a link, the note picker,
  or back/forward — resumes exactly where you were editing instead of dropping
  the caret at the top. (#43)

## [0.5.0] - 2026-07-07

### Added

- **Link to a section heading.** With the caret in a heading, _Copy Link to
  Section_ (`Cmd-Shift-K`/`Ctrl-Shift-K`) copies a link to that heading; clicking
  such a link opens the note and scrolls straight to the heading. The link is
  copied as a `piki://…#section` URL — a scheme Piki now registers with the OS —
  so it is clickable from other apps too and reopens Piki at the right section.
  Pasting such a URL into the link editor normalizes it back to a plain
  `note#section` wiki link. Repeated heading titles are disambiguated so a link
  always resolves to the intended one. (#36)

### Fixed

- Editing and saving a note no longer corrupts its Markdown in several cases
  where the text round-tripped incorrectly (via `rutle 0.3.2` and `tdoc 0.11.2`):
  a styled span that fully contains a shorter differently-styled one
  (`**bold and <u>underlined</u>**`) is no longer split into pieces, and neither
  are two adjacent runs that share an outer style (`~~**durch**gestrichen~~`);
  intra-word italics (`un*frigging*believable`) and adjacent same-style spans are
  parsed and re-emitted faithfully; code blocks and code spans no longer gain
  stray blank lines or lose their content; and empty paragraphs (blank lines
  between blocks) are preserved instead of collapsing. (#37)

## [0.4.0] - 2026-07-06

### Added

- Editing now offers **two caret positions at the edges of styled text** (bold,
  italic, code, links, …), even with Reveal Codes off: arrowing to the border of a
  bold word stops once "before" and once "inside" the style, letting you choose
  whether text you type there picks up the style. The caret shows which side it
  will apply by leaning that way, with small angled tails at its top and bottom.
  (via rutle) (#26)
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
  opened with `--directory` keep independent histories. (#31)

### Changed

- User-facing wording now says "note" instead of "page": the _Page_ menu is now
  _Note_ (with _New Note …_ and _Open Note …_), and the new-note dialog and the
  status bar ("Note: …") follow suit. The rename now carries through the rest of
  the app as well — the `!index` and `!todo` plugin views and the `piki` CLI help
  read "notes", and the `piki-gui` startup flag is now `--note`/`-n` (was
  `--page`/`-p`). (#35)
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
  modifier opens the highlighted note. The currently open note starts selected. (#31)
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
  conversion wrappers Piki still needs for the clipboard and note load/save now
  live in `gui`'s own `markdown_converter` module. Rendering, selection, reveal
  codes, styled links, and table display are unchanged. As part of the shared
  core, rutle's layout cache no longer invalidates on an unchanged
  resize/padding update, which speeds up redraws in Piki as well. (#26)

### Removed

- The _View → Markdown editor_ mode has been removed. This was a separate
  plain-text editor that showed a note's raw Markdown source with rudimentary
  syntax highlighting, toggled from the View menu. It had fallen out of sync
  with the structured rich-text editor and no longer worked correctly, so the
  structured editor is now the only editor. (#34)

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

<!-- next-url -->
[Unreleased]: https://github.com/roblillack/piki/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/roblillack/piki/compare/piki-v0.4.0...v0.5.0
[0.4.0]: https://github.com/roblillack/piki/compare/piki-v0.3.0...piki-v0.4.0
