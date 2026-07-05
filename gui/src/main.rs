mod app_icon;
mod autosave;
pub mod fltk_draw_context;
mod history;
mod link_handler;
mod markdown_editor;
mod menu;
mod page_picker;
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
use piki_gui::page_ui::PageUI;
use piki_gui::ui_adapters::StructuredRichUI;
use recency::RecentPages;
use scroll_memory::ScrollMemory;
use search_bar::SearchBar;
use statusbar::StatusBar;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;
use window_state::WindowGeometry;

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

    /// Initial page to load (default: frontpage)
    #[arg(short, long, default_value = "frontpage")]
    page: String,
}

struct AppState {
    store: DocumentStore,
    plugin_registry: PluginRegistry,
    current_page: String,
    history: History,
    /// When each page was last opened, used by the page picker to order notes
    /// and to resolve the "previous note" for a double Cmd-O/Ctrl-O.
    recent_pages: RecentPages,
    /// Where `recent_pages` is persisted (None if no data dir is available).
    recent_pages_path: Option<PathBuf>,
    /// In-memory scroll positions for recently visited notes, so returning to a
    /// note resumes where the user left off.
    scroll_positions: ScrollMemory,
}

impl AppState {
    fn new(
        store: DocumentStore,
        plugin_registry: PluginRegistry,
        initial_page: String,
        recent_pages_path: Option<PathBuf>,
    ) -> Self {
        let recent_pages = recent_pages_path
            .as_deref()
            .map(RecentPages::load)
            .unwrap_or_default();
        AppState {
            store,
            plugin_registry,
            current_page: initial_page,
            history: History::new(),
            recent_pages,
            recent_pages_path,
            scroll_positions: ScrollMemory::new(),
        }
    }

    /// Record that `page` was just opened and persist the updated recency store.
    fn mark_page_opened(&mut self, page: &str) {
        self.recent_pages.mark_opened(page);
        if let Some(path) = &self.recent_pages_path
            && let Err(e) = self.recent_pages.save(path)
        {
            eprintln!("Failed to save recent pages: {e}");
        }
    }

    fn load_page(&mut self, page_name: &str) -> Result<String, String> {
        // Check if this is a plugin page (starts with !)
        if let Some(plugin_name) = page_name.strip_prefix('!') {
            // Generate content using the plugin
            self.current_page = page_name.to_string();
            return self.plugin_registry.generate(plugin_name, &self.store);
        }

        // Normal file loading
        match self.store.load(page_name) {
            Ok(doc) => {
                self.current_page = page_name.to_string();
                Ok(doc.content)
            }
            Err(e) => Err(e),
        }
    }
}
/// Flush any pending changes of the currently open page to disk immediately.
///
/// This is the "save when walking away" safeguard: it runs before navigating to
/// another page (links, history, page picker, new page) and when the window is
/// closing, so edits are never lost to the debounced autosave timer. Saving is a
/// no-op when the content is unchanged or the page is a read-only plugin page
/// (handled inside `AutoSaveState::trigger_save`).
fn save_current_page(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
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

fn load_page_helper(
    page_name: &str,
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    restore_scroll: Option<i32>,
) {
    // Save the page we're leaving before its content is replaced below, so
    // switching pages (or creating a new one) never drops unsaved edits.
    save_current_page(app_state, autosave_state, active_editor, statusbar);

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
        let leaving_page = state.current_page.clone();
        state
            .scroll_positions
            .remember(&leaving_page, leaving_scroll);
    }

    // Check if this is a plugin page
    let is_plugin = page_name.starts_with('!');

    // Load content through AppState::load_page (handles plugins)
    let content_result = app_state.borrow_mut().load_page(page_name);

    match content_result {
        Ok(content) => {
            // For non-plugin pages, get the modification time
            let modified_time = if !is_plugin {
                app_state
                    .borrow()
                    .store
                    .load(page_name)
                    .ok()
                    .and_then(|doc| doc.modified_time)
            } else {
                None
            };

            {
                let active = active_editor.borrow();
                let mut editor_mut = active.borrow_mut();
                editor_mut.set_content_from_markdown(&content);

                // Set read-only mode for plugin pages, editable for regular pages
                editor_mut.set_readonly(is_plugin);
            }

            // Decide where to scroll: an explicit position from back/forward
            // history wins; otherwise resume the remembered position for this
            // note (if it is still one of the recent ones), falling back to top.
            let target_scroll = restore_scroll
                .or_else(|| app_state.borrow().scroll_positions.get(page_name))
                .unwrap_or(0);
            {
                let active = active_editor.borrow();
                let mut ed = (*active).borrow_mut();
                ed.set_scroll_pos(target_scroll);
            }
            let final_scroll_pos = target_scroll;

            // Drop the editor borrow before manipulating history

            // If normal navigation (not history), add new page to history
            if restore_scroll.is_none() {
                app_state
                    .borrow_mut()
                    .history
                    .push(page_name.to_string(), final_scroll_pos);
            }

            // Record the open so the page picker can order notes by recency and
            // resolve the "previous note" for a double Cmd-O. Plugin pages (e.g.
            // !index) are generated views that never appear in the picker list.
            if !is_plugin {
                app_state.borrow_mut().mark_page_opened(page_name);
            }

            // Reset autosave state for the new page
            if let Ok(mut as_state) = autosave_state.try_borrow_mut() {
                as_state.reset_for_page(page_name, &content);

                // Set last_save_time to file's modification time if it exists
                if let Some(mtime) = modified_time {
                    as_state.last_save_time = Some(mtime);
                }
            }

            // Determine page status text based on page type
            let page_text = if let Some(plugin_name) = page_name.strip_prefix('!') {
                format!("Plugin: {}", plugin_name)
            } else if content.is_empty() {
                format!("Note: {} (new)", page_name)
            } else {
                format!("Note: {}", page_name)
            };

            statusbar.borrow_mut().set_page(&page_text);

            // Set initial save status based on modification time
            if let Ok(as_state) = autosave_state.try_borrow() {
                statusbar
                    .borrow_mut()
                    .set_status(&as_state.get_status_text());
            } else {
                statusbar.borrow_mut().set_status("");
            }

            app::redraw();
        }
        Err(e) => {
            statusbar.borrow_mut().set_page(&format!("Error: {}", e));
            statusbar.borrow_mut().set_status("");
            app::redraw();
        }
    }
}

fn navigate_back(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    // Update current entry's scroll position before navigating
    let scroll_pos = active_editor.borrow().borrow().scroll_pos();
    app_state
        .borrow_mut()
        .history
        .update_scroll_position(scroll_pos);

    // Try to navigate back and extract values before calling load_page_helper
    let target = {
        let mut state = app_state.borrow_mut();
        state
            .history
            .go_back()
            .map(|entry| (entry.page_name.clone(), entry.scroll_position))
    }; // Borrow is dropped here

    if let Some((page_name, scroll_position)) = target {
        load_page_helper(
            &page_name,
            app_state,
            autosave_state,
            active_editor,
            statusbar,
            Some(scroll_position),
        );
    }
}

fn navigate_forward(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    // Update current entry's scroll position before navigating
    let scroll_pos = active_editor.borrow().borrow().scroll_pos();
    app_state
        .borrow_mut()
        .history
        .update_scroll_position(scroll_pos);

    // Try to navigate forward and extract values before calling load_page_helper
    let target = {
        let mut state = app_state.borrow_mut();
        state
            .history
            .go_forward()
            .map(|entry| (entry.page_name.clone(), entry.scroll_position))
    }; // Borrow is dropped here

    if let Some((page_name, scroll_position)) = target {
        load_page_helper(
            &page_name,
            app_state,
            autosave_state,
            active_editor,
            statusbar,
            Some(scroll_position),
        );
    }
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

    let recent_pages_path = window_state::recent_pages_file(&directory);

    let app_state = Rc::new(RefCell::new(AppState::new(
        store,
        plugin_registry,
        args.page.clone(),
        recent_pages_path,
    )));
    let autosave_state = Rc::new(RefCell::new(AutoSaveState::new()));

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
    let rich_editor: Rc<RefCell<dyn PageUI>> = Rc::new(RefCell::new(StructuredRichUI::new(
        editor_x, editor_y, editor_w, editor_h, true,
    )));
    let active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>> = Rc::new(RefCell::new(rich_editor));
    let is_structured: Rc<RefCell<bool>> = Rc::new(RefCell::new(true));

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

    // Create menu (system menu bar on macOS, window menu bar on other platforms)
    #[cfg(target_os = "macos")]
    menu::setup_menu(
        app_state.clone(),
        autosave_state.clone(),
        active_editor.clone(),
        is_structured.clone(),
        statusbar.clone(),
        wind_ref.clone(),
        window_geometry.clone(),
        search_bar.clone(),
        editor_x,
        editor_y,
        editor_w,
        editor_h,
    );

    #[cfg(not(target_os = "macos"))]
    let _menu_bar = menu::setup_menu(
        app_state.clone(),
        autosave_state.clone(),
        active_editor.clone(),
        is_structured.clone(),
        statusbar.clone(),
        wind_ref.clone(),
        window_geometry.clone(),
        search_bar.clone(),
        editor_x,
        editor_y,
        editor_w,
        editor_h,
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
        let active_editor_for_resize = active_editor.clone();
        let statusbar_for_resize = statusbar.clone();
        let app_state_for_close = app_state.clone();
        let autosave_for_close = autosave_state.clone();

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
                    // Check if search bar is visible
                    let search_bar_visible = search_bar_for_resize
                        .try_borrow()
                        .map(|sb| sb.visible())
                        .unwrap_or(false);

                    // Only resize search bar when visible to avoid FLTK resize side effects
                    if search_bar_visible && let Ok(mut sb) = search_bar_for_resize.try_borrow_mut()
                    {
                        sb.resize(0, editor_y, win.width());
                    }

                    // Resize editor based on whether search bar is visible
                    let statusbar_h = statusbar_for_resize
                        .try_borrow()
                        .map(|s| if s.visible() { s.height() } else { 0 })
                        .unwrap_or(0);

                    if let Ok(ed_ptr) = active_editor_for_resize.try_borrow()
                        && let Ok(mut ed) = ed_ptr.try_borrow_mut()
                        && let Some(structured) = ed.as_any_mut().downcast_mut::<StructuredRichUI>()
                    {
                        if search_bar_visible {
                            let bar_h = search_bar::BAR_HEIGHT;
                            let editor_top = editor_y + bar_h;
                            let editor_h = win.height() - editor_top - statusbar_h;
                            structured.resize(0, editor_top, win.width(), editor_h);
                        } else {
                            // Search bar hidden - editor fills full space
                            let editor_h = win.height() - editor_y - statusbar_h;
                            structured.resize(0, editor_y, win.width(), editor_h);
                        }
                    }
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
                save_current_page(
                    &app_state_for_close,
                    &autosave_for_close,
                    &active_editor_for_resize,
                    &statusbar_for_resize,
                );
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

    // Clicking the page status opens the page picker
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar_for_click = statusbar.clone();
        let wind_for_click = wind.clone();
        statusbar.borrow_mut().on_page_click(move |_| {
            page_picker::show_page_picker(
                app_state.clone(),
                autosave_state.clone(),
                active_editor.clone(),
                statusbar_for_click.clone(),
                &wind_for_click,
            );
        });
    }

    // Load initial page
    load_page_helper(
        &args.page,
        &app_state,
        &autosave_state,
        &active_editor,
        &statusbar,
        None,
    );

    // Wire callbacks for active editor
    wire_editor_callbacks(&active_editor, &autosave_state, &app_state, &statusbar);

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

    app.run().unwrap();
}

fn wire_editor_callbacks(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    app_state: &Rc<RefCell<AppState>>,
    statusbar: &Rc<RefCell<StatusBar>>,
) {
    let editor_for_callback = active_editor.clone();
    let autosave_for_callback = autosave_state.clone();
    let app_state_for_callback = app_state.clone();
    let statusbar_for_callback = statusbar.clone();
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

    // Link click handler via PageUI uses active editor
    let app_state_links = app_state.clone();
    let autosave_links = autosave_state.clone();
    let statusbar_links = statusbar.clone();
    let current_for_links = active_editor.borrow().clone();
    {
        let mut cur = current_for_links.borrow_mut();
        let active_clone = active_editor.clone();
        cur.on_link_click(Box::new(move |link_dest: String| {
            // External links (http(s)://, mailto:, ...) open in the system
            // browser/handler instead of being loaded as a wiki page.
            if link_handler::is_external_link(&link_dest) {
                let statusbar = statusbar_links.clone();
                app::awake_callback(move || {
                    if let Err(e) = webbrowser::open(&link_dest) {
                        statusbar
                            .borrow_mut()
                            .set_status(&format!("Failed to open link: {}", e));
                        app::redraw();
                    }
                });
                return;
            }

            let app_state = app_state_links.clone();
            let autosave_state = autosave_links.clone();
            let editor_ref = active_clone.clone();
            let statusbar = statusbar_links.clone();
            app::awake_callback(move || {
                load_page_helper(
                    &link_dest,
                    &app_state,
                    &autosave_state,
                    &editor_ref,
                    &statusbar,
                    None,
                );
            });
        }));
    }

    // Hover handler to show link destinations in the page status bar
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
                            let current = statusbar_for_cb.borrow().page_status_widget().label();
                            *base_label_for_cb.borrow_mut() = Some(current);
                        }
                        statusbar_for_cb.borrow_mut().set_page(&dest);
                    }
                    None => {
                        if let Some(orig) = base_label_for_cb.borrow_mut().take() {
                            statusbar_for_cb.borrow_mut().set_page(&orig);
                        }
                    }
                }
                app::redraw();
            });
        }));
    }
}
