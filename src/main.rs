mod autosave;
mod document;
pub mod draw_context;
mod editor;
pub mod fltk_draw_context;
pub mod fltk_text_display;
mod history;
mod link_handler;
mod menu;
mod page_picker;
mod plugin;
pub mod responsive_scrollbar;
pub mod sourceedit;
mod window_state;

use autosave::AutoSaveState;
use clap::Parser;
use document::DocumentStore;
use fliki_rs::page_ui::PageUI;
use fliki_rs::ui_adapters::StructuredRichUI;
use fltk::enums::Color;
use fltk::{prelude::*, *};
use history::History;
use plugin::{IndexPlugin, PluginRegistry};
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
#[command(name = "fliki-rs")]
#[command(about = "A Markdown wiki browser with clickable links", long_about = None)]
struct Args {
    /// Directory containing markdown files
    #[arg(value_name = "DIR")]
    directory: PathBuf,

    /// Initial page to load (default: frontpage)
    #[arg(short, long, default_value = "frontpage")]
    page: String,
}

struct AppState {
    store: DocumentStore,
    plugin_registry: PluginRegistry,
    current_page: String,
    history: History,
}

impl AppState {
    fn new(store: DocumentStore, plugin_registry: PluginRegistry, initial_page: String) -> Self {
        AppState {
            store,
            plugin_registry,
            current_page: initial_page,
            history: History::new(),
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
fn load_page_helper<PS: WidgetExt>(
    page_name: &str,
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    page_status: &Rc<RefCell<PS>>,
    save_status: &Rc<RefCell<frame::Frame>>,
    restore_scroll: Option<i32>,
) {
    // If we're not restoring from history, update the scroll position of the current history entry
    if restore_scroll.is_none() {
        let scroll_pos = active_editor.borrow().borrow().scroll_pos();
        app_state
            .borrow_mut()
            .history
            .update_scroll_position(scroll_pos);
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

            // Restore scroll position if provided (from history navigation)
            // Otherwise, scroll to top for normal navigation
            let final_scroll_pos = if let Some(scroll_pos) = restore_scroll {
                let active = active_editor.borrow();
                let mut ed = (&*active).borrow_mut();
                ed.set_scroll_pos(scroll_pos);
                scroll_pos
            } else {
                let active = active_editor.borrow();
                let mut ed = (&*active).borrow_mut();
                ed.set_scroll_pos(0);
                0
            };

            // Drop the editor borrow before manipulating history

            // If normal navigation (not history), add new page to history
            if restore_scroll.is_none() {
                app_state
                    .borrow_mut()
                    .history
                    .push(page_name.to_string(), final_scroll_pos);
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
                format!("Page: {} (new)", page_name)
            } else {
                format!("Page: {}", page_name)
            };

            page_status.borrow_mut().set_label(&page_text);

            // Set initial save status based on modification time
            if let Ok(as_state) = autosave_state.try_borrow() {
                save_status
                    .borrow_mut()
                    .set_label(&as_state.get_status_text());
            } else {
                save_status.borrow_mut().set_label("");
            }

            app::redraw();
        }
        Err(e) => {
            page_status.borrow_mut().set_label(&format!("Error: {}", e));
            save_status.borrow_mut().set_label("");
            app::redraw();
        }
    }
}

fn navigate_back<PS: WidgetExt>(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    page_status: &Rc<RefCell<PS>>,
    save_status: &Rc<RefCell<frame::Frame>>,
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
            page_status,
            save_status,
            Some(scroll_position),
        );
    }
}

fn navigate_forward<PS: WidgetExt>(
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    page_status: &Rc<RefCell<PS>>,
    save_status: &Rc<RefCell<frame::Frame>>,
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
            page_status,
            save_status,
            Some(scroll_position),
        );
    }
}

fn main() {
    let args = Args::parse();

    // Validate directory
    if !args.directory.exists() {
        eprintln!(
            "Error: Directory '{}' does not exist",
            args.directory.display()
        );
        std::process::exit(1);
    }

    if !args.directory.is_dir() {
        eprintln!("Error: '{}' is not a directory", args.directory.display());
        std::process::exit(1);
    }

    // Initialize FLTK
    let app = app::App::default();
    let window_state_path = window_state::state_file_path().map(Rc::new);
    let mut wind = window::Window::default()
        .with_size(400, 650) // Golden ratio 1:1.618 approx
        .with_label("Piki");

    if let Some(path) = window_state_path.as_ref() {
        if let Some(saved_state) = window_state::load_state(path.as_path()) {
            if saved_state.width > 0 && saved_state.height > 0 {
                wind.resize(
                    saved_state.x,
                    saved_state.y,
                    saved_state.width,
                    saved_state.height,
                );
            }
        }
    }

    // #[cfg(target_os = "macos")]
    // wind.set_color(Color::White);

    wind.begin();

    // Create state and register plugins
    let store = DocumentStore::new(args.directory.clone());
    let mut plugin_registry = PluginRegistry::new();
    plugin_registry.register("index", Box::new(IndexPlugin));

    let app_state = Rc::new(RefCell::new(AppState::new(
        store,
        plugin_registry,
        args.page.clone(),
    )));
    let autosave_state = Rc::new(RefCell::new(AutoSaveState::new()));

    #[cfg(target_os = "macos")]
    let editor_padding = 0;
    #[cfg(not(target_os = "macos"))]
    let editor_padding = 5;

    let statusbar_size = 25;
    let statusbar_bgcolor = Color::from_rgb(136, 167, 246); // "rgba(136, 167, 246, 1)"
    let statusbar_fgcolor = Color::White;

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

    // Create two status frames at the bottom: page status and save status
    let mut statusbar =
        frame::Frame::new(0, wind.h() - statusbar_size, wind.w(), statusbar_size, None);
    statusbar.set_frame(enums::FrameType::FlatBox);
    statusbar.set_color(statusbar_bgcolor);

    let page_status = Rc::new(RefCell::new({
        let mut f = button::Button::new(
            5,
            wind.h() - statusbar_size,
            wind.w() / 2 - 10,
            statusbar_size,
            None,
        );
        // let mut f = frame::Frame::new(5, wind.h() - 25, wind.w() / 2 - 10, 25, None);
        f.set_frame(enums::FrameType::FlatBox);
        f.set_label("...");
        f.set_tooltip("Click to open page picker");
        f.set_align(enums::Align::Left | enums::Align::Inside);
        f.set_label_size(fltk::app::font_size() - 1);
        f.set_color(statusbar_bgcolor);
        f.set_label_color(statusbar_fgcolor);
        // f.handle_event(fltk::enums::Event::Enter);
        let mut but2 = f.clone();
        f.handle(move |_, evt| match evt {
            enums::Event::Enter => {
                // f.(enums::Cursor::Hand);
                but2.set_color(Color::Black);
                true
            }
            enums::Event::Leave => {
                // app::set_cursor(enums::Cursor::Default);
                but2.set_color(statusbar_bgcolor);
                true
            }
            _ => false,
        });

        f
    }));

    let save_status = Rc::new(RefCell::new({
        let mut f = frame::Frame::new(
            5 + wind.w() / 2,
            wind.h() - statusbar_size,
            wind.w() / 2 - 10,
            statusbar_size,
            None,
        );
        f.set_frame(enums::FrameType::FlatBox);
        f.set_label("");
        f.set_align(enums::Align::Right | enums::Align::Inside);
        f.set_label_size(fltk::app::font_size() - 1);
        f.set_color(statusbar_bgcolor);
        f.set_label_color(statusbar_fgcolor);
        f
    }));

    // Create a clone handle to the window for callbacks
    let wind_ref = Rc::new(RefCell::new(wind.clone()));

    // Create menu (system menu bar on macOS, window menu bar on other platforms)
    #[cfg(target_os = "macos")]
    menu::setup_menu(
        app_state.clone(),
        autosave_state.clone(),
        active_editor.clone(),
        is_structured.clone(),
        page_status.clone(),
        save_status.clone(),
        wind_ref.clone(),
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
        page_status.clone(),
        save_status.clone(),
        wind_ref.clone(),
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

    wind.end();

    let window_geometry = Rc::new(RefCell::new(WindowGeometry {
        x: wind.x(),
        y: wind.y(),
        width: wind.width(),
        height: wind.height(),
    }));
    let pending_save_handle = Rc::new(RefCell::new(None::<app::TimeoutHandle>));

    {
        let geometry = window_geometry.clone();
        let pending = pending_save_handle.clone();
        let state_path_for_handler = window_state_path.clone();

        wind.handle(move |win, event| match event {
            enums::Event::Move | enums::Event::Resize => {
                if (win.x() == geometry.borrow().x)
                    && (win.y() == geometry.borrow().y)
                    && (win.width() == geometry.borrow().width)
                    && (win.height() == geometry.borrow().height)
                {
                    return false;
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

    // Clicking the page status opens the page picker
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let page_status_for_click = page_status.clone();
        let save_status_for_click = save_status.clone();
        let wind_for_click = wind.clone();
        page_status.borrow_mut().set_callback(move |_| {
            page_picker::show_page_picker(
                app_state.clone(),
                autosave_state.clone(),
                active_editor.clone(),
                page_status_for_click.clone(),
                save_status_for_click.clone(),
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
        &page_status,
        &save_status,
        None,
    );

    // Wire callbacks for active editor
    wire_editor_callbacks(
        &active_editor,
        &autosave_state,
        &app_state,
        &page_status,
        &save_status,
    );

    // Set up periodic timer to update "X ago" display
    {
        let autosave_ref = autosave_state.clone();
        let save_status_ref = save_status.clone();

        app::add_timeout3(SAVE_STATUS_UPDATE_INTERVAL_SECS, move |handle| {
            // Update the status text
            if let (Ok(as_state), Ok(mut status)) =
                (autosave_ref.try_borrow(), save_status_ref.try_borrow_mut())
            {
                if !as_state.is_saving && as_state.last_save_time.is_some() {
                    status.set_label(&as_state.get_status_text());
                    app::redraw();
                }
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
            if let Ok(ed_ptr) = editor_ref.try_borrow() {
                if let Ok(mut ed) = (&*ed_ptr).try_borrow_mut() {
                    ed.tick(ms);
                }
            }
            app::repeat_timeout3(0.1, handle);
        });
    }

    // No window activation forwarding needed; cursor shows when widget has focus

    app.run().unwrap();
}

fn wire_editor_callbacks<PS: WidgetExt + 'static>(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    app_state: &Rc<RefCell<AppState>>,
    page_status: &Rc<RefCell<PS>>,
    save_status: &Rc<RefCell<frame::Frame>>,
) {
    let editor_for_callback = active_editor.clone();
    let autosave_for_callback = autosave_state.clone();
    let app_state_for_callback = app_state.clone();
    let save_status_for_callback = save_status.clone();
    let current_for_change = active_editor.borrow().clone();
    current_for_change.borrow_mut().on_change(Box::new(move || {
        // Restyle if supported
        let editor_clone = editor_for_callback.clone();
        app::awake_callback(move || {
            if let Ok(ed_ptr) = editor_clone.try_borrow() {
                let mut ed_ref = (&*ed_ptr).borrow_mut();
                ed_ref.restyle();
            }
        });

        if let Ok(mut as_state) = autosave_for_callback.try_borrow_mut() {
            as_state.mark_changed();
        }

        let editor_clone = editor_for_callback.clone();
        let autosave_clone = autosave_for_callback.clone();
        let app_state_clone = app_state_for_callback.clone();
        let save_status_clone = save_status_for_callback.clone();

        app::add_timeout3(AUTOSAVE_INTERVAL_SECS, move |_| {
            let should_save = autosave_clone
                .try_borrow()
                .map(|s| s.pending_save)
                .unwrap_or(false);

            if should_save {
                if let Ok(mut status) = save_status_clone.try_borrow_mut() {
                    status.set_label("Saving...");
                    app::redraw();
                }

                if let (Ok(ed_ptr), Ok(mut as_state), Ok(app_st)) = (
                    editor_clone.try_borrow(),
                    autosave_clone.try_borrow_mut(),
                    app_state_clone.try_borrow(),
                ) {
                    let ed_ref = (&*ed_ptr).borrow();
                    match as_state.trigger_save(&*ed_ref, &app_st.store) {
                        Ok(()) => {
                            if let Ok(mut status) = save_status_clone.try_borrow_mut() {
                                status.set_label(&as_state.get_status_text());
                                app::redraw();
                            }
                        }
                        Err(e) => {
                            if let Ok(mut status) = save_status_clone.try_borrow_mut() {
                                status.set_label(&format!("Error: {}", e));
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
    let page_status_links = save_status.clone(); // not used here
    let current_for_links = active_editor.borrow().clone();
    {
        let mut cur = current_for_links.borrow_mut();
        let active_clone = active_editor.clone();
        cur.on_link_click(Box::new(move |link_dest: String| {
            let app_state = app_state_links.clone();
            let autosave_state = autosave_links.clone();
            let editor_ref = active_clone.clone();
            let save_status = page_status_links.clone();
            app::awake_callback(move || {
                // We cannot refresh page_status from here easily; keep it unchanged
                let dummy = Rc::new(RefCell::new(frame::Frame::new(0, 0, 0, 0, None)));
                load_page_helper(
                    &link_dest,
                    &app_state,
                    &autosave_state,
                    &editor_ref,
                    &dummy,
                    &save_status,
                    None,
                );
            });
        }));
    }

    // Hover handler to show link destinations in the page status bar
    let current_for_hover = active_editor.borrow().clone();
    {
        let mut cur = current_for_hover.borrow_mut();
        let page_status_clone = page_status.clone();
        let base_label: Rc<RefCell<Option<String>>> = Rc::new(RefCell::new(None));
        cur.on_link_hover(Box::new(move |target: Option<String>| {
            let page_status_for_cb = page_status_clone.clone();
            let base_label_for_cb = base_label.clone();
            let tgt = target.clone();
            app::awake_callback(move || {
                match &tgt {
                    Some(dest) => {
                        let dest = dest.clone();
                        if base_label_for_cb.borrow().is_none() {
                            let current = page_status_for_cb.borrow().label();
                            *base_label_for_cb.borrow_mut() = Some(current);
                        }
                        page_status_for_cb.borrow_mut().set_label(&dest);
                    }
                    None => {
                        if let Some(orig) = base_label_for_cb.borrow_mut().take() {
                            page_status_for_cb.borrow_mut().set_label(&orig);
                        }
                    }
                }
                app::redraw();
            });
        }));
    }
}
