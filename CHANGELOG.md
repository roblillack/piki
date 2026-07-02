# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While pre-1.0, the minor version is bumped for breaking changes.

## [Unreleased]

### Changed

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

### Fixed

- Pasting rich text via RTF (e.g. from Word or Outlook) no longer inserts inert
  boxes where curly quotes, apostrophes, en/em dashes, or ellipses should be.
  RTF encodes these as Windows-1252 codepage bytes, but the RTF parser decoded
  the C1 range (`0x80`–`0x9F`) as raw Unicode scalars — turning `\'92` into the
  control character U+0092 instead of `'`. Piki now remaps that block to the
  characters the bytes actually stand for on import. (#26)

[Unreleased]: https://github.com/roblillack/piki/compare/piki-v0.3.0...HEAD
