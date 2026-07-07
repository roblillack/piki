mod app_icon;
mod app_url;
mod autosave;
pub mod fltk_draw_context;
mod history;
mod link_handler;
mod menu;
mod note_picker;
mod recency;
pub mod responsive_scrollbar;
mod scroll_memory;
mod search_bar;
mod statusbar;
mod window_state;

use autosave::AutoSaveState;
use clap::Parser;
use fltk::{prelude::*, *};
use history::History;
use piki_core::{DocumentStore, IndexPlugin, PluginRegistry, TodoPlugin};
use piki_gui::live_share::LiveShare;
use piki_gui::note_ui::NoteUI;
use piki_gui::on_air_bar::OnAirBar;
use piki_gui::section_link;
use piki_gui::ui_adapters::StructuredRichUI;
use recency::RecentNotes;
use scroll_memory::ScrollMemory;
use search_bar::SearchBar;
use statusbar::StatusBar;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use window_state::WindowGeometry;

/// Top of the content region, below the platform menu bar (0 on macOS, which
/// uses the system menu bar; 25 elsewhere for the in-window menu bar). The ON
/// AIR bar, search bar, and editor stack downward from here.
#[cfg(target_os = "macos")]
const CONTENT_TOP: i32 = 0;
#[cfg(not(target_os = "macos"))]
const CONTENT_TOP: i32 = 25;

/// Callback invoked with `(note, markdown)` whenever the current note changes.
type ShareHook = Box<dyn Fn(&str, &str)>;

thread_local! {
    /// Invoked after the currently open note (or its content) changes, so an
    /// active Live Note Sharing session can update what it serves and the URL
    /// shown in the ON AIR bar. Installed once in `main` and only ever touched
    /// on the FLTK main thread. Doing this via a hook avoids threading the
    /// share handles through `load_note_helper` and its many call sites.
    static SHARE_HOOK: RefCell<Option<ShareHook>> = const { RefCell::new(None) };
}

/// Notify an active sharing session that `note` is now the current note, with
/// the given live `markdown`. A no-op when sharing is off.
fn notify_share_view(note: &str, markdown: &str) {
    SHARE_HOOK.with(|hook| {
        if let Some(cb) = hook.borrow().as_ref() {
            cb(note, markdown);
        }
    });
}

// Timeout to save window state after resize/move
const WINDOW_STATE_SAVE_TIMEOUT_SECS: f64 = 3.0;
// Interval to autosave changes
const AUTOSAVE_INTERVAL_SECS: f64 = 10.0;
// Interval to update "X ago" display in save status
const SAVE_STATUS_UPDATE_INTERVAL_SECS: f64 = 30.0;

#[derive(Parser, Debug)]
#[command(name = "piki-gui")]
#[command(about = "Piki - a simple personal wiki", long_about = None)]
struct Args {
    /// Directory containing markdown files (default: ~/.piki)
    #[arg(short = 'd', long = "directory", value_name = "DIRECTORY")]
    directory: Option<PathBuf>,

    /// Initial note to load (default: frontpage)
    #[arg(short, long, default_value = "frontpage")]
    note: String,
}

struct AppState {
    store: DocumentStore,
    plugin_registry: PluginRegistry,
    current_note: String,
    history: History,
    /// When each note was last opened, used by the note picker to order notes
    /// and to resolve the "previous note" for a double Cmd-O/Ctrl-O.
    recent_notes: RecentNotes,
    /// Where `recent_notes` is persisted (None if no data dir is available).
    recent_notes_path: Option<PathBuf>,
    /// In-memory scroll positions for recently visited notes, so returning to a
    /// note resumes where the user left off.
    scroll_positions: ScrollMemory,
}

impl AppState {
    fn new(
        store: DocumentStore,
        plugin_registry: PluginRegistry,
        initial_note: String,
        recent_notes_path: Option<PathBuf>,
    ) -> Self {
        let recent_notes = recent_notes_path
            .as_deref()
            .map(RecentNotes::load)
            .unwrap_or_default();
        AppState {
            store,
            plugin_registry,
            current_note: initial_note,
            history: History::new(),
            recent_notes,
            recent_notes_path,
            scroll_positions: ScrollMemory::new(),
        }
    }

    /// Record that `note` was just opened and persist the updated recency store.
    fn mark_note_opened(&mut self, note: &str) {
        self.recent_notes.mark_opened(note);
        if let Some(path) = &self.recent_notes_path
            && let Err(e) = self.recent_notes.save(path)
        {
            eprintln!("Failed to save recent notes: {e}");
        }
    }

    /// Update all in-session state that refers to `old` to point at `new` after
    /// a note has been renamed: the current-note pointer, back/forward history,
    /// the picker's recency ordering, and remembered scroll positions. The
    /// on-disk file move is handled by `rename_current_note`.
    fn rename_note(&mut self, old: &str, new: &str) {
        if self.current_note == old {
            self.current_note = new.to_string();
        }
        self.history.rename_note(old, new);
        self.recent_notes.rename(old, new);
        self.scroll_positions.rename(old, new);
        if let Some(path) = &self.recent_notes_path
            && let Err(e) = self.recent_notes.save(path)
        {
            eprintln!("Failed to save recent notes: {e}");
        }
    }

    fn load_note(&mut self, note_name: &str) -> Result<String, String> {
        // Check if this is a plugin note (starts with !)
        if let Some(plugin_name) = note_name.strip_prefix('!') {
            // Generate content using the plugin
            self.current_note = note_name.to_string();
            return self.plugin_registry.generate(plugin_name, &self.store);
        }

        // Normal file loading
        match self.store.load(note_name) {
            Ok(doc) => {
                self.current_note = note_name.to_string();
                Ok(doc.content)
            }
            Err(e) => Err(e),
        }
    }
}
/// Flush any pending changes of the currently open note to disk immediately.
///
/// This is the "save when walking away" safeguard: it runs before navigating to
/// another note (links, history, note picker, new note) and when the window is
/// closing, so edits are never lost to the debounced autosave timer. Saving is a
/// no-op when the content is unchanged or the note is a read-only plugin note
/// (handled inside `AutoSaveState::trigger_save`).
fn save_current_note(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    if let (Ok(ed_ptr), Ok(mut as_state), Ok(app_st)) = (
        active_editor.try_borrow(),
        autosave_state.try_borrow_mut(),
        app_state.try_borrow(),
    ) {
        let ed_ref = (*ed_ptr).borrow();
        match as_state.trigger_save(&*ed_ref, &app_st.store) {
            Ok(()) => {
                if let Ok(mut sb) = statusbar.try_borrow_mut() {
                    sb.set_status(&as_state.get_status_text());
                }
            }
            Err(e) => {
                if let Ok(mut sb) = statusbar.try_borrow_mut() {
                    sb.set_status(&format!("Error: {}", e));
                }
            }
        }
    }
}

/// Rename the currently open note: move its file on disk and update every piece
/// of in-session state to follow it. Backs the "Rename Note …" menu item, which
/// is how a quick, auto-named `untitled_…` note gets a real name.
///
/// The current content is flushed to the old file first, so an edit that has not
/// yet hit the debounced autosave is not lost and there is a file to move. A
/// brand-new untitled note the user has not typed into has no file yet, so the
/// move is skipped and the next autosave simply writes to the new name. Returns
/// an error (surfaced by the dialog) when the target name is already taken or
/// the move fails; read-only plugin notes ("!…") cannot be renamed.
fn rename_current_note(
    new_name: &str,
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) -> Result<(), String> {
    let new_name = new_name.trim();
    if new_name.is_empty() {
        return Err("Please enter a name.".to_string());
    }

    let old_name = app_state.borrow().current_note.clone();
    if new_name == old_name {
        return Ok(());
    }
    if old_name.starts_with('!') {
        return Err("This note cannot be renamed.".to_string());
    }

    // Flush current content to the old file first, so a not-yet-autosaved edit
    // is not lost and there is a file to move.
    save_current_note(app_state, autosave_state, active_editor, statusbar);

    let (old_path, new_path) = {
        let st = app_state.borrow();
        (st.store.path_for(&old_name), st.store.path_for(new_name))
    };
    if new_path.exists() {
        return Err(format!("A note named '{new_name}' already exists."));
    }
    // A never-typed-into untitled note has no file yet; nothing to move, the new
    // name is picked up by the next autosave.
    if old_path.exists() {
        if let Some(parent) = new_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create folder for '{new_name}': {e}"))?;
        }
        std::fs::rename(&old_path, &new_path).map_err(|e| format!("Failed to rename note: {e}"))?;
    }

    // Point all in-session state at the new name. The editor already holds the
    // content, so we deliberately do not reload it.
    app_state.borrow_mut().rename_note(&old_name, new_name);
    if let Ok(mut as_state) = autosave_state.try_borrow_mut() {
        as_state.current_note = new_name.to_string();
    }
    statusbar
        .borrow_mut()
        .set_note(&format!("Note: {new_name}"));

    // Point any live-sharing session at the new name (and refresh the ON AIR
    // link) so a note shared under its old name keeps working after a rename.
    let content = active_editor.borrow().borrow().get_content();
    notify_share_view(new_name, &content);

    Ok(())
}

fn load_note_helper(
    note_name: &str,
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    restore_scroll: Option<i32>,
    fragment: Option<&str>,
) {
    // Save the note we're leaving before its content is replaced below, so
    // switching notes (or creating a new one) never drops unsaved edits.
    save_current_note(app_state, autosave_state, active_editor, statusbar);

    // Record the scroll position of the note we're leaving: into the current
    // back/forward history entry (only for non-history navigation), and always
    // into the recent-notes scroll memory so returning to it later — via a link
    // or the picker — resumes where we were.
    {
        let leaving_scroll = active_editor.borrow().borrow().scroll_pos();
        let mut state = app_state.borrow_mut();
        if restore_scroll.is_none() {
            state.history.update_scroll_position(leaving_scroll);
        }
        let leaving_note = state.current_note.clone();
        state
            .scroll_positions
            .remember(&leaving_note, leaving_scroll);
    }

    // Check if this is a plugin note
    let is_plugin = note_name.starts_with('!');

    // Load content through AppState::load_note (handles plugins)
    let content_result = app_state.borrow_mut().load_note(note_name);

    match content_result {
        Ok(content) => {
            // For non-plugin notes, get the modification time
            let modified_time = if !is_plugin {
                app_state
                    .borrow()
                    .store
                    .load(note_name)
                    .ok()
                    .and_then(|doc| doc.modified_time)
            } else {
                None
            };

            {
                let active = active_editor.borrow();
                let mut editor_mut = active.borrow_mut();
                editor_mut.set_content_from_markdown(&content);

                // Set read-only mode for plugin notes, editable for regular notes
                editor_mut.set_readonly(is_plugin);
            }

            // Decide where to scroll. A section fragment (from a section link)
            // wins and scrolls to the matching heading; otherwise an explicit
            // position from back/forward history wins, then the remembered
            // position for this note (if it is still one of the recent ones),
            // falling back to the top.
            let did_anchor = fragment
                .filter(|f| !f.is_empty())
                .map(|frag| {
                    let active = active_editor.borrow();
                    let mut ed = active.borrow_mut();
                    ed.as_any_mut()
                        .downcast_mut::<StructuredRichUI>()
                        .map(|structured| structured.scroll_to_anchor(frag))
                        .unwrap_or(false)
                })
                .unwrap_or(false);

            let final_scroll_pos = if did_anchor {
                active_editor.borrow().borrow().scroll_pos()
            } else {
                let target_scroll = restore_scroll
                    .or_else(|| app_state.borrow().scroll_positions.get(note_name))
                    .unwrap_or(0);
                let active = active_editor.borrow();
                let mut ed = (*active).borrow_mut();
                ed.set_scroll_pos(target_scroll);
                target_scroll
            };

            // Drop the editor borrow before manipulating history

            // If normal navigation (not history), add new note to history
            if restore_scroll.is_none() {
                app_state
                    .borrow_mut()
                    .history
                    .push(note_name.to_string(), final_scroll_pos);
            }

            // Record the open so the note picker can order notes by recency and
            // resolve the "previous note" for a double Cmd-O. Plugin notes (e.g.
            // !index) are generated views that never appear in the picker list.
            if !is_plugin {
                app_state.borrow_mut().mark_note_opened(note_name);
            }

            // Reset autosave state for the new note
            if let Ok(mut as_state) = autosave_state.try_borrow_mut() {
                as_state.reset_for_note(note_name, &content);

                // Set last_save_time to file's modification time if it exists
                if let Some(mtime) = modified_time {
                    as_state.last_save_time = Some(mtime);
                }
            }

            // Determine note status text based on note type
            let note_text = if let Some(plugin_name) = note_name.strip_prefix('!') {
                format!("Plugin: {}", plugin_name)
            } else if content.is_empty() {
                format!("Note: {} (new)", note_name)
            } else {
                format!("Note: {}", note_name)
            };

            statusbar.borrow_mut().set_note(&note_text);

            // Set initial save status based on modification time
            if let Ok(as_state) = autosave_state.try_borrow() {
                statusbar
                    .borrow_mut()
                    .set_status(&as_state.get_status_text());
            } else {
                statusbar.borrow_mut().set_status("");
            }

            // Keep any live-sharing session pointed at the note now on screen,
            // so the ON AIR link and the served content follow it.
            notify_share_view(note_name, &content);

            app::redraw();
        }
        Err(e) => {
            statusbar.borrow_mut().set_note(&format!("Error: {}", e));
            statusbar.borrow_mut().set_status("");
            app::redraw();
        }
    }
}

fn navigate_back(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    // Update current entry's scroll position before navigating
    let scroll_pos = active_editor.borrow().borrow().scroll_pos();
    app_state
        .borrow_mut()
        .history
        .update_scroll_position(scroll_pos);

    // Try to navigate back and extract values before calling load_note_helper
    let target = {
        let mut state = app_state.borrow_mut();
        state
            .history
            .go_back()
            .map(|entry| (entry.note_name.clone(), entry.scroll_position))
    }; // Borrow is dropped here

    if let Some((note_name, scroll_position)) = target {
        load_note_helper(
            &note_name,
            app_state,
            autosave_state,
            active_editor,
            statusbar,
            Some(scroll_position),
            None,
        );
    }
}

fn navigate_forward(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    // Update current entry's scroll position before navigating
    let scroll_pos = active_editor.borrow().borrow().scroll_pos();
    app_state
        .borrow_mut()
        .history
        .update_scroll_position(scroll_pos);

    // Try to navigate forward and extract values before calling load_note_helper
    let target = {
        let mut state = app_state.borrow_mut();
        state
            .history
            .go_forward()
            .map(|entry| (entry.note_name.clone(), entry.scroll_position))
    }; // Borrow is dropped here

    if let Some((note_name, scroll_position)) = target {
        load_note_helper(
            &note_name,
            app_state,
            autosave_state,
            active_editor,
            statusbar,
            Some(scroll_position),
            None,
        );
    }
}

/// Lay out the stacked content widgets for a normal (non-fullscreen) window:
/// the ON AIR bar (if sharing), the search bar (if open) below it, then the
/// editor filling the rest above the status bar. Fullscreen has its own layout
/// in `menu::toggle_fullscreen`.
fn relayout_content(
    win_w: i32,
    win_h: i32,
    on_air: &Rc<RefCell<OnAirBar>>,
    search_bar: &Rc<RefCell<SearchBar>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    let on_air_h = {
        let bar = on_air.borrow();
        if bar.visible() { bar.height() } else { 0 }
    };
    let search_h = if search_bar.borrow().visible() {
        search_bar::BAR_HEIGHT
    } else {
        0
    };
    let statusbar_h = {
        let sb = statusbar.borrow();
        if sb.visible() { sb.height() } else { 0 }
    };

    if on_air_h > 0 {
        on_air.borrow_mut().resize(0, CONTENT_TOP, win_w);
    }
    let search_top = CONTENT_TOP + on_air_h;
    if search_h > 0 {
        search_bar.borrow_mut().resize(0, search_top, win_w);
    }

    let editor_top = search_top + search_h;
    let editor_h = (win_h - editor_top - statusbar_h).max(0);
    if let Ok(ed_ptr) = active_editor.try_borrow()
        && let Ok(mut ed) = ed_ptr.try_borrow_mut()
        && let Some(structured) = ed.as_any_mut().downcast_mut::<StructuredRichUI>()
    {
        structured.resize(0, editor_top, win_w, editor_h);
    }
}

/// Start a Live Note Sharing session for the currently open note: spin up the
/// localhost server, show the ON AIR bar, reflow the layout, and open the note
/// in the browser. No-op if already sharing.
#[allow(clippy::too_many_arguments)]
fn start_sharing(
    app_state: &Rc<RefCell<AppState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    live_share: &Rc<RefCell<Option<LiveShare>>>,
    on_air: &Rc<RefCell<OnAirBar>>,
    search_bar: &Rc<RefCell<SearchBar>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    wind_ref: &Rc<RefCell<window::Window>>,
) {
    if live_share.borrow().is_some() {
        return;
    }

    let (dir, note) = {
        let st = app_state.borrow();
        (st.store.base_path().to_path_buf(), st.current_note.clone())
    };
    let markdown = active_editor.borrow().borrow().get_content();

    match LiveShare::start(dir, note.clone(), markdown) {
        Ok(session) => {
            let url = session.url_for(&note);
            {
                let mut bar = on_air.borrow_mut();
                bar.set_url(&url);
                bar.show();
            }
            *live_share.borrow_mut() = Some(session);

            let (w, h) = {
                let win = wind_ref.borrow();
                (win.width(), win.height())
            };
            relayout_content(w, h, on_air, search_bar, active_editor, statusbar);
            statusbar
                .borrow_mut()
                .set_status(&format!("Sharing live at {url}"));
            app::redraw();
            let _ = webbrowser::open(&url);
        }
        Err(e) => {
            statusbar
                .borrow_mut()
                .set_status(&format!("Could not start sharing: {e}"));
        }
    }
}

/// Stop the active Live Note Sharing session: shut down the server (joining its
/// thread), hide the ON AIR bar, and reflow the layout. No-op if not sharing.
fn stop_sharing(
    live_share: &Rc<RefCell<Option<LiveShare>>>,
    on_air: &Rc<RefCell<OnAirBar>>,
    search_bar: &Rc<RefCell<SearchBar>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    wind_ref: &Rc<RefCell<window::Window>>,
) {
    // Move the session out (releasing the RefCell borrow) before dropping it, so
    // joining the server thread happens with no borrow held.
    let session = live_share.borrow_mut().take();
    if session.is_none() {
        return;
    }
    drop(session);

    on_air.borrow_mut().hide();
    let (w, h) = {
        let win = wind_ref.borrow();
        (win.width(), win.height())
    };
    relayout_content(w, h, on_air, search_bar, active_editor, statusbar);
    statusbar.borrow_mut().set_status("Live sharing stopped.");
    app::redraw();
}

fn get_directory(dir_opt: Option<PathBuf>) -> PathBuf {
    dir_opt.unwrap_or_else(|| {
        std::env::var("HOME")
            .ok()
            .map(|home| PathBuf::from(home).join(".piki"))
            .unwrap_or_else(|| PathBuf::from(".piki"))
    })
}

fn main() {
    let args = Args::parse();
    let directory = get_directory(args.directory);

    // Ensure directory exists
    if !directory.exists()
        && let Err(e) = std::fs::create_dir_all(&directory)
    {
        eprintln!(
            "Error: Failed to create directory '{}': {}",
            directory.display(),
            e
        );
        std::process::exit(1);
    }

    // Validate directory
    if !directory.is_dir() {
        eprintln!("Error: '{}' is not a directory", directory.display());
        std::process::exit(1);
    }

    // Initialize FLTK
    let app = app::App::default();
    // Set the Dock icon on macOS (works even for the unbundled binary).
    app_icon::set_macos_dock_icon();
    let window_state_path = window_state::state_file_path().map(Rc::new);
    let mut wind = window::Window::default()
        .with_size(400, 650) // Golden ratio 1:1.618 approx
        .with_label("Piki");

    if let Some(path) = window_state_path.as_ref()
        && let Some(saved_state) = window_state::load_state(path.as_path())
        && saved_state.width > 0
        && saved_state.height > 0
    {
        wind.resize(
            saved_state.x,
            saved_state.y,
            saved_state.width,
            saved_state.height,
        );
    }

    app_icon::set_window_icon(&mut wind);

    // #[cfg(target_os = "macos")]
    // wind.set_color(Color::White);

    wind.begin();

    // Create state and register plugins
    let store = DocumentStore::new(directory.clone());
    let mut plugin_registry = PluginRegistry::new();
    plugin_registry.register("index", Box::new(IndexPlugin));
    plugin_registry.register("todo", Box::new(TodoPlugin));

    let recent_notes_path = window_state::recent_notes_file(&directory);

    let app_state = Rc::new(RefCell::new(AppState::new(
        store,
        plugin_registry,
        args.note.clone(),
        recent_notes_path,
    )));
    let autosave_state = Rc::new(RefCell::new(AutoSaveState::new()));
    // Holds the active Live Note Sharing session, if any.
    let live_share: Rc<RefCell<Option<LiveShare>>> = Rc::new(RefCell::new(None));

    #[cfg(target_os = "macos")]
    let editor_padding = 0;
    #[cfg(not(target_os = "macos"))]
    let editor_padding = 0;

    let statusbar_size = 25;

    // macOS uses system menu bar (no space needed), other platforms use window menu bar (25px)
    #[cfg(target_os = "macos")]
    let (editor_y, editor_height) = (editor_padding, wind.h() - statusbar_size - editor_padding);
    #[cfg(not(target_os = "macos"))]
    let (editor_y, editor_height) = (
        25 + editor_padding,
        wind.h() - statusbar_size - editor_padding - 25,
    );

    // Create only the initially active editor (structured rich editor)
    let editor_x = editor_padding;
    let editor_w = wind.w() - 2 * editor_padding;
    let editor_h = editor_height;
    let rich_editor: Rc<RefCell<dyn NoteUI>> = Rc::new(RefCell::new(StructuredRichUI::new(
        editor_x, editor_y, editor_w, editor_h, true,
    )));
    let active_editor: Rc<RefCell<Rc<RefCell<dyn NoteUI>>>> = Rc::new(RefCell::new(rich_editor));

    // Create status bar at the bottom using the custom StatusBar widget
    let statusbar = Rc::new(RefCell::new(StatusBar::new(
        0,
        wind.h() - statusbar_size,
        wind.w(),
        statusbar_size,
    )));

    // Create a clone handle to the window for callbacks
    let wind_ref = Rc::new(RefCell::new(wind.clone()));

    // Initialize window geometry state (with fullscreen from saved state if available)
    let saved_fullscreen = window_state_path
        .as_ref()
        .and_then(|path| window_state::load_state(path.as_path()))
        .map(|state| state.fullscreen)
        .unwrap_or(false);
    let window_geometry = Rc::new(RefCell::new(WindowGeometry {
        x: wind.x(),
        y: wind.y(),
        width: wind.width(),
        height: wind.height(),
        fullscreen: saved_fullscreen,
    }));

    // Create search bar (uses a sub-window so it floats on top)
    let search_bar = Rc::new(RefCell::new(SearchBar::new(editor_x, editor_y, editor_w)));

    // Create the ON AIR bar (hidden until Live Note Sharing is enabled).
    let on_air = Rc::new(RefCell::new(OnAirBar::new(editor_x, editor_y, editor_w)));

    // Wire the ON AIR bar: Stop ends sharing; clicking the link opens it.
    {
        let live_share = live_share.clone();
        let on_air_for_stop = on_air.clone();
        let search_bar = search_bar.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let wind_ref = wind_ref.clone();
        on_air.borrow_mut().on_stop(move || {
            stop_sharing(
                &live_share,
                &on_air_for_stop,
                &search_bar,
                &active_editor,
                &statusbar,
                &wind_ref,
            );
        });
    }
    {
        let on_air_for_link = on_air.clone();
        let statusbar = statusbar.clone();
        on_air.borrow_mut().on_link_click(move || {
            let url = on_air_for_link.borrow().url();
            if !url.is_empty()
                && let Err(e) = webbrowser::open(&url)
            {
                statusbar
                    .borrow_mut()
                    .set_status(&format!("Failed to open link: {e}"));
            }
        });
    }

    // Install the hook that keeps an active sharing session pointed at the
    // currently visible note (updating served content and the ON AIR link).
    {
        let live_share = live_share.clone();
        let on_air = on_air.clone();
        SHARE_HOOK.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move |note: &str, markdown: &str| {
                if let Some(session) = live_share.borrow().as_ref() {
                    session.set_current(note, markdown);
                    if let Ok(mut bar) = on_air.try_borrow_mut() {
                        bar.set_url(&session.url_for(note));
                    }
                }
            }));
        });
    }

    // Create menu (system menu bar on macOS, window menu bar on other platforms)
    #[cfg(target_os = "macos")]
    menu::setup_menu(
        app_state.clone(),
        autosave_state.clone(),
        active_editor.clone(),
        statusbar.clone(),
        wind_ref.clone(),
        window_geometry.clone(),
        search_bar.clone(),
        live_share.clone(),
        on_air.clone(),
    );

    #[cfg(not(target_os = "macos"))]
    let _menu_bar = menu::setup_menu(
        app_state.clone(),
        autosave_state.clone(),
        active_editor.clone(),
        statusbar.clone(),
        wind_ref.clone(),
        window_geometry.clone(),
        search_bar.clone(),
        live_share.clone(),
        on_air.clone(),
    );

    // Configure editor UI
    active_editor
        .borrow()
        .borrow_mut()
        .set_bg_color(enums::Color::from_rgb(255, 255, 245));

    // Wire up search bar callbacks
    {
        let search_bar_for_search = search_bar.clone();
        let editor_for_search = active_editor.clone();

        // On search text change
        search_bar.borrow().on_search(move |term| {
            if let Ok(ed_ptr) = editor_for_search.try_borrow()
                && let Ok(mut ed) = ed_ptr.try_borrow_mut()
                && let Some(structured) = ed.as_any_mut().downcast_mut::<StructuredRichUI>()
            {
                let count = structured.search(&term);
                if let Ok(mut sb) = search_bar_for_search.try_borrow_mut() {
                    sb.set_match_count(if count > 0 { Some(0) } else { None }, count);
                }
                // Scroll to first match if any
                if count > 0 {
                    structured.scroll_to_current_match();
                }
                app::redraw();
            }
        });
    }

    {
        let search_bar_for_next = search_bar.clone();
        let editor_for_next = active_editor.clone();

        // On next match
        search_bar.borrow().on_next(move || {
            if let Ok(ed_ptr) = editor_for_next.try_borrow()
                && let Ok(mut ed) = ed_ptr.try_borrow_mut()
                && let Some(structured) = ed.as_any_mut().downcast_mut::<StructuredRichUI>()
                && structured.next_match()
            {
                let total = structured.search_matches().len();
                let current = structured.search_current_index();
                if let Ok(mut sb) = search_bar_for_next.try_borrow_mut() {
                    sb.set_match_count(current, total);
                }
                structured.scroll_to_current_match();
                app::redraw();
            }
        });
    }

    {
        let search_bar_for_prev = search_bar.clone();
        let editor_for_prev = active_editor.clone();

        // On previous match
        search_bar.borrow().on_prev(move || {
            if let Ok(ed_ptr) = editor_for_prev.try_borrow()
                && let Ok(mut ed) = ed_ptr.try_borrow_mut()
                && let Some(structured) = ed.as_any_mut().downcast_mut::<StructuredRichUI>()
                && structured.prev_match()
            {
                let total = structured.search_matches().len();
                let current = structured.search_current_index();
                if let Ok(mut sb) = search_bar_for_prev.try_borrow_mut() {
                    sb.set_match_count(current, total);
                }
                structured.scroll_to_current_match();
                app::redraw();
            }
        });
    }

    {
        let search_bar_for_close = search_bar.clone();
        let editor_for_close = active_editor.clone();

        // On close
        search_bar.borrow().on_close(move || {
            // Restore editor position (move up to fill the space)
            if let Ok(ed_ptr) = editor_for_close.try_borrow()
                && let Ok(mut ed) = ed_ptr.try_borrow_mut()
                && let Some(structured) = ed.as_any_mut().downcast_mut::<StructuredRichUI>()
            {
                let bar_h = search_bar::BAR_HEIGHT;
                let x = structured.x();
                let y = structured.y();
                let w = structured.width();
                let h = structured.height();
                structured.resize(x, y - bar_h, w, h + bar_h);
                structured.clear_search();
            }

            if let Ok(mut sb) = search_bar_for_close.try_borrow_mut() {
                sb.hide();
            }

            // Return focus to editor
            if let Ok(ed_ptr) = editor_for_close.try_borrow()
                && let Ok(mut ed) = ed_ptr.try_borrow_mut()
            {
                ed.take_focus();
            }
            app::redraw();
        });
    }

    wind.end();
    let pending_save_handle = Rc::new(RefCell::new(None::<app::TimeoutHandle>));

    {
        let geometry = window_geometry.clone();
        let pending = pending_save_handle.clone();
        let state_path_for_handler = window_state_path.clone();
        let search_bar_for_resize = search_bar.clone();
        let on_air_for_resize = on_air.clone();
        let active_editor_for_resize = active_editor.clone();
        let statusbar_for_resize = statusbar.clone();
        let app_state_for_close = app_state.clone();
        let autosave_for_close = autosave_state.clone();
        let live_share_for_close = live_share.clone();

        wind.handle(move |win, event| match event {
            enums::Event::Move | enums::Event::Resize => {
                // Don't update geometry while in fullscreen mode - preserve the
                // pre-fullscreen window position for when we exit fullscreen
                if geometry.borrow().fullscreen {
                    return false;
                }

                if (win.x() == geometry.borrow().x)
                    && (win.y() == geometry.borrow().y)
                    && (win.width() == geometry.borrow().width)
                    && (win.height() == geometry.borrow().height)
                {
                    return false;
                }

                // Skip custom resize logic when in fullscreen mode
                // (fullscreen has its own layout with padding)
                let is_fullscreen = geometry.borrow().fullscreen;

                if !is_fullscreen {
                    // Reflow the stacked ON AIR bar / search bar / editor for the
                    // new window size.
                    relayout_content(
                        win.width(),
                        win.height(),
                        &on_air_for_resize,
                        &search_bar_for_resize,
                        &active_editor_for_resize,
                        &statusbar_for_resize,
                    );
                }

                {
                    let mut geom = geometry.borrow_mut();
                    geom.x = win.x();
                    geom.y = win.y();
                    geom.width = win.width();
                    geom.height = win.height();
                }

                if let Some(handle) = {
                    let mut slot = pending.borrow_mut();
                    slot.take()
                } {
                    app::remove_timeout3(handle);
                }

                if let Some(path) = state_path_for_handler.as_ref() {
                    let geometry_for_timer = geometry.clone();
                    let pending_for_timer = pending.clone();
                    let path_for_timer = path.clone();
                    let new_handle = app::add_timeout3(WINDOW_STATE_SAVE_TIMEOUT_SECS, move |_| {
                        let snapshot = geometry_for_timer.borrow().clone();
                        if let Err(err) =
                            window_state::save_state(path_for_timer.as_path(), &snapshot)
                        {
                            eprintln!("Failed to save window state: {err}");
                        }
                        pending_for_timer.borrow_mut().take();
                    });
                    pending.borrow_mut().replace(new_handle);
                }
                false
            }
            enums::Event::Close => {
                // Flush the open note before the window goes away.
                save_current_note(
                    &app_state_for_close,
                    &autosave_for_close,
                    &active_editor_for_resize,
                    &statusbar_for_resize,
                );
                // Shut the sharing server down cleanly (joins its thread).
                let session = live_share_for_close.borrow_mut().take();
                drop(session);
                if let Some(handle) = {
                    let mut slot = pending.borrow_mut();
                    slot.take()
                } {
                    app::remove_timeout3(handle);
                }
                if let Some(path) = state_path_for_handler.as_ref() {
                    let snapshot = geometry.borrow().clone();
                    if let Err(err) = window_state::save_state(path.as_path(), &snapshot) {
                        eprintln!("Failed to save window state on close: {err}");
                    }
                }
                false
            }
            _ => false,
        });
    }

    active_editor.borrow().borrow().set_resizable(&mut wind);
    wind.show();

    // Restore fullscreen mode if it was previously enabled
    if saved_fullscreen {
        // Determine which screen the window is on using its center point
        let win_center_x = wind.x() + wind.width() / 2;
        let win_center_y = wind.y() + wind.height() / 2;
        let screen_num = app::screen_num(win_center_x, win_center_y);

        // Enter fullscreen mode
        wind.fullscreen(true);

        // Calculate and apply padding using the correct screen dimensions
        let (_, _, screen_w, screen_h) = app::screen_xywh(screen_num);
        let font_size = 14; // Default body text font size from theme
        let char_width = (font_size as f32 * 0.55) as i32;
        let target_text_width = char_width * 90; // ~90 chars
        let scrollbar_width = 15;
        let available_width = screen_w - scrollbar_width;
        let padding = ((available_width - target_text_width) / 2).max(25);

        // Apply padding and resize the editor to take full height
        if let Ok(active_ptr) = active_editor.try_borrow()
            && let Ok(mut editor) = active_ptr.try_borrow_mut()
            && let Some(structured) = editor.as_any_mut().downcast_mut::<StructuredRichUI>()
        {
            structured.set_horizontal_padding(padding);
            // Expand editor to full screen height (no statusbar)
            let y = structured.y();
            structured.resize(0, y, screen_w, screen_h - y);
        }

        // Hide status bar
        statusbar.borrow_mut().hide();
    }

    // Clicking the note status opens the note picker
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar_for_click = statusbar.clone();
        let wind_for_click = wind.clone();
        statusbar.borrow_mut().on_note_click(move |_| {
            note_picker::show_note_picker(
                app_state.clone(),
                autosave_state.clone(),
                active_editor.clone(),
                statusbar_for_click.clone(),
                &wind_for_click,
            );
        });
    }

    // Load initial note
    load_note_helper(
        &args.note,
        &app_state,
        &autosave_state,
        &active_editor,
        &statusbar,
        None,
        None,
    );

    // Wire callbacks for active editor
    wire_editor_callbacks(
        &active_editor,
        &autosave_state,
        &app_state,
        &statusbar,
        &live_share,
    );

    // Set up periodic timer to update "X ago" display
    {
        let autosave_ref = autosave_state.clone();
        let statusbar_ref = statusbar.clone();

        app::add_timeout3(SAVE_STATUS_UPDATE_INTERVAL_SECS, move |handle| {
            // Update the status text
            if let (Ok(as_state), Ok(mut sb)) =
                (autosave_ref.try_borrow(), statusbar_ref.try_borrow_mut())
                && !as_state.is_saving
                && as_state.last_save_time.is_some()
            {
                sb.set_status(&as_state.get_status_text());
                app::redraw();
            }

            // Repeat every second
            app::repeat_timeout3(SAVE_STATUS_UPDATE_INTERVAL_SECS, handle);
        });
    }

    // Set up a lightweight tick for blinking cursor and animations
    {
        let start = Instant::now();
        let editor_ref = active_editor.clone();
        app::add_timeout3(0.1, move |handle| {
            let ms = start.elapsed().as_millis() as u64;
            if let Ok(ed_ptr) = editor_ref.try_borrow()
                && let Ok(mut ed) = (*ed_ptr).try_borrow_mut()
            {
                ed.tick(ms);
            }
            app::repeat_timeout3(0.1, handle);
        });
    }

    // No window activation forwarding needed; cursor shows when widget has focus

    // Rename the macOS application menu now that the system menu bar exists, so
    // an unbundled binary shows "Piki" instead of "piki-gui".
    app_icon::set_macos_app_name("Piki");

    // Replace FLTK's default about box with a proper macOS about panel (real
    // name, version, icon, description and homepage link).
    app_icon::set_macos_about();

    // Handle `piki://note#section` URLs opened from other apps / the OS: strip
    // the scheme, split off the section, and navigate (scrolling to the heading).
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        app_url::set_open_url_handler(move |url: String| {
            let target = section_link::normalize_link_target(&url);
            let (note, fragment) = section_link::split_target(&target);
            let note = note.to_string();
            let fragment = fragment.map(str::to_string);
            let app_state = app_state.clone();
            let autosave_state = autosave_state.clone();
            let active_editor = active_editor.clone();
            let statusbar = statusbar.clone();
            app::awake_callback(move || {
                load_note_helper(
                    &note,
                    &app_state,
                    &autosave_state,
                    &active_editor,
                    &statusbar,
                    None,
                    fragment.as_deref(),
                );
            });
        });
        app_url::register();
    }

    app.run().unwrap();
}

fn wire_editor_callbacks(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    app_state: &Rc<RefCell<AppState>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    live_share: &Rc<RefCell<Option<LiveShare>>>,
) {
    let editor_for_callback = active_editor.clone();
    let autosave_for_callback = autosave_state.clone();
    let app_state_for_callback = app_state.clone();
    let statusbar_for_callback = statusbar.clone();
    let live_share_for_change = live_share.clone();
    let current_for_change = active_editor.borrow().clone();
    current_for_change.borrow_mut().on_change(Box::new(move || {
        // Restyle if supported
        let editor_clone = editor_for_callback.clone();
        app::awake_callback(move || {
            if let Ok(ed_ptr) = editor_clone.try_borrow() {
                let mut ed_ref = (*ed_ptr).borrow_mut();
                ed_ref.restyle();
            }
        });

        // While sharing, push the edited content to the browser (deferred: the
        // editor is borrowed while this change callback fires). Guarded so the
        // Markdown serialization cost is only paid when ON AIR.
        if live_share_for_change.borrow().is_some() {
            let live = live_share_for_change.clone();
            let editor = editor_for_callback.clone();
            let app_state = app_state_for_callback.clone();
            app::awake_callback(move || {
                if let (Ok(ed_ptr), Ok(app_st)) = (editor.try_borrow(), app_state.try_borrow())
                    && let Ok(inner) = ed_ptr.try_borrow()
                {
                    let markdown = inner.get_content();
                    let note = app_st.current_note.clone();
                    if let Some(session) = live.borrow().as_ref() {
                        session.set_current(&note, &markdown);
                    }
                }
            });
        }

        if let Ok(mut as_state) = autosave_for_callback.try_borrow_mut() {
            as_state.mark_changed();
        }

        let editor_clone = editor_for_callback.clone();
        let autosave_clone = autosave_for_callback.clone();
        let app_state_clone = app_state_for_callback.clone();
        let statusbar_clone = statusbar_for_callback.clone();

        app::add_timeout3(AUTOSAVE_INTERVAL_SECS, move |_| {
            let should_save = autosave_clone
                .try_borrow()
                .map(|s| s.pending_save)
                .unwrap_or(false);

            if should_save {
                if let Ok(mut sb) = statusbar_clone.try_borrow_mut() {
                    sb.set_status("Saving …");
                    app::redraw();
                }

                if let (Ok(ed_ptr), Ok(mut as_state), Ok(app_st)) = (
                    editor_clone.try_borrow(),
                    autosave_clone.try_borrow_mut(),
                    app_state_clone.try_borrow(),
                ) {
                    let ed_ref = (*ed_ptr).borrow();
                    match as_state.trigger_save(&*ed_ref, &app_st.store) {
                        Ok(()) => {
                            if let Ok(mut sb) = statusbar_clone.try_borrow_mut() {
                                sb.set_status(&as_state.get_status_text());
                                app::redraw();
                            }
                        }
                        Err(e) => {
                            if let Ok(mut sb) = statusbar_clone.try_borrow_mut() {
                                sb.set_status(&format!("Error: {}", e));
                                app::redraw();
                            }
                        }
                    }
                }
            }
        });
    }));

    // Link click handler via NoteUI uses active editor
    let app_state_links = app_state.clone();
    let autosave_links = autosave_state.clone();
    let statusbar_links = statusbar.clone();
    let current_for_links = active_editor.borrow().clone();
    {
        let mut cur = current_for_links.borrow_mut();
        let active_clone = active_editor.clone();
        cur.on_link_click(Box::new(move |link_dest: String| {
            // A `piki:` URL is our own scheme (e.g. a section link pasted in as-is
            // or arriving from another app): normalize it to the internal
            // `note#section` form and navigate in-app instead of handing it to
            // the browser. Non-`piki:` destinations are returned unchanged.
            let normalized = section_link::normalize_link_target(&link_dest);

            // Genuine external links (http(s)://, mailto:, ...) open in the system
            // browser/handler. Normalization only strips the `piki:` scheme, so a
            // real external URL is untouched here and still detected as external.
            if link_handler::is_external_link(&normalized) {
                let statusbar = statusbar_links.clone();
                app::awake_callback(move || {
                    if let Err(e) = webbrowser::open(&normalized) {
                        statusbar
                            .borrow_mut()
                            .set_status(&format!("Failed to open link: {}", e));
                        app::redraw();
                    }
                });
                return;
            }

            // Internal link: split off an optional `#section` fragment so we can
            // scroll to that heading after the note loads.
            let (note, fragment) = section_link::split_target(&normalized);
            let note = note.to_string();
            let fragment = fragment.map(str::to_string);

            let app_state = app_state_links.clone();
            let autosave_state = autosave_links.clone();
            let editor_ref = active_clone.clone();
            let statusbar = statusbar_links.clone();
            app::awake_callback(move || {
                load_note_helper(
                    &note,
                    &app_state,
                    &autosave_state,
                    &editor_ref,
                    &statusbar,
                    None,
                    fragment.as_deref(),
                );
            });
        }));
    }

    // Hover handler to show link destinations in the note status bar
    let current_for_hover = active_editor.borrow().clone();
    {
        let mut cur = current_for_hover.borrow_mut();
        let statusbar_clone = statusbar.clone();
        let base_label: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        cur.on_link_hover(Box::new(move |target: Option<String>| {
            let statusbar_for_cb = statusbar_clone.clone();
            let base_label_for_cb = base_label.clone();
            let tgt = target.clone();
            app::awake_callback(move || {
                match &tgt {
                    Some(dest) => {
                        let dest = dest.clone();
                        if base_label_for_cb.borrow().is_none() {
                            let current = statusbar_for_cb.borrow().note_status_widget().label();
                            *base_label_for_cb.borrow_mut() = Some(current);
                        }
                        statusbar_for_cb.borrow_mut().set_note(&dest);
                    }
                    None => {
                        if let Some(orig) = base_label_for_cb.borrow_mut().take() {
                            statusbar_for_cb.borrow_mut().set_note(&orig);
                        }
                    }
                }
                app::redraw();
            });
        }));
    }
}
