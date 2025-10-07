mod autosave;
mod document;
mod editor;
mod link_handler;
mod plugin;

use autosave::AutoSaveState;
use clap::Parser;
use document::DocumentStore;
use editor::MarkdownEditor;
#[cfg(target_os = "macos")]
use fltk::enums::Color;
use fltk::{prelude::*, *};
use plugin::{IndexPlugin, PluginRegistry};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

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
}

impl AppState {
    fn new(store: DocumentStore, plugin_registry: PluginRegistry, initial_page: String) -> Self {
        AppState {
            store,
            plugin_registry,
            current_page: initial_page,
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

#[cfg(target_os = "macos")]
fn create_menu(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    editor: Rc<RefCell<MarkdownEditor>>,
    page_status: Rc<RefCell<frame::Frame>>,
    save_status: Rc<RefCell<frame::Frame>>,
) {
    // Use system menu bar on macOS
    let mut menu_bar = menu::SysMenuBar::default();

    // Navigate menu
    menu_bar.add(
        "Navigate/Index\t",
        enums::Shortcut::Ctrl | 'i',
        menu::MenuFlag::Normal,
        {
            let app_state = app_state.clone();
            let autosave_state = autosave_state.clone();
            let editor = editor.clone();
            let page_status = page_status.clone();
            let save_status = save_status.clone();
            move |_| {
                load_page_helper(
                    "!index",
                    &app_state,
                    &autosave_state,
                    &editor,
                    &page_status,
                    &save_status,
                );
            }
        },
    );

    menu_bar.add(
        "Navigate/Frontpage\t",
        enums::Shortcut::Ctrl | 'f',
        menu::MenuFlag::Normal,
        {
            let app_state = app_state.clone();
            let autosave_state = autosave_state.clone();
            let editor = editor.clone();
            let page_status = page_status.clone();
            let save_status = save_status.clone();
            move |_| {
                load_page_helper(
                    "frontpage",
                    &app_state,
                    &autosave_state,
                    &editor,
                    &page_status,
                    &save_status,
                );
            }
        },
    );
}

#[cfg(not(target_os = "macos"))]
fn create_menu(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    editor: Rc<RefCell<MarkdownEditor>>,
    page_status: Rc<RefCell<frame::Frame>>,
    save_status: Rc<RefCell<frame::Frame>>,
) -> menu::MenuBar {
    // Use regular menu bar on other platforms
    let mut menu_bar = menu::MenuBar::new(0, 0, 660, 25, None);

    // Index menu item
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let editor = editor.clone();
        let page_status = page_status.clone();
        let save_status = save_status.clone();
        menu_bar.add(
            "&Index",
            enums::Shortcut::Ctrl | 'i',
            menu::MenuFlag::Normal,
            move |_| {
                load_page_helper(
                    "!index",
                    &app_state,
                    &autosave_state,
                    &editor,
                    &page_status,
                    &save_status,
                );
            },
        );
    }

    // Frontpage menu item
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let editor = editor.clone();
        let page_status = page_status.clone();
        let save_status = save_status.clone();
        menu_bar.add(
            "&Frontpage",
            enums::Shortcut::Ctrl | 'f',
            menu::MenuFlag::Normal,
            move |_| {
                load_page_helper(
                    "frontpage",
                    &app_state,
                    &autosave_state,
                    &editor,
                    &page_status,
                    &save_status,
                );
            },
        );
    }

    menu_bar
}

fn load_page_helper(
    page_name: &str,
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    editor: &Rc<RefCell<MarkdownEditor>>,
    page_status: &Rc<RefCell<frame::Frame>>,
    save_status: &Rc<RefCell<frame::Frame>>,
) {
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

            let mut editor_mut = editor.borrow_mut();
            editor_mut.set_content(&content);

            // Set read-only mode for plugin pages, editable for regular pages
            editor_mut.set_readonly(is_plugin);

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
                format!("Page: {} (plugin: {})", page_name, plugin_name)
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
    let mut wind = window::Window::default()
        .with_size(660, 400)
        .with_label("fliki-rs");

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

    // macOS uses system menu bar (no space needed), other platforms use window menu bar (25px)
    #[cfg(target_os = "macos")]
    let (editor_y, editor_height) = (5, 370);
    #[cfg(not(target_os = "macos"))]
    let (editor_y, editor_height) = (30, 345);

    let editor = Rc::new(RefCell::new(MarkdownEditor::new(
        5,
        editor_y,
        650,
        editor_height,
    )));

    // Create two status frames at the bottom: page status and save status
    let page_status = Rc::new(RefCell::new({
        let mut f = frame::Frame::new(5, 375, 400, 25, None);
        f.set_frame(enums::FrameType::FlatBox);
        f.set_label("...");
        f.set_align(enums::Align::Left | enums::Align::Inside);
        f
    }));

    let save_status = Rc::new(RefCell::new({
        let mut f = frame::Frame::new(400, 375, 255, 25, None);
        f.set_frame(enums::FrameType::FlatBox);
        f.set_label("");
        f.set_align(enums::Align::Right | enums::Align::Inside);
        f
    }));

    // Create menu (system menu bar on macOS, window menu bar on other platforms)
    #[cfg(target_os = "macos")]
    create_menu(
        app_state.clone(),
        autosave_state.clone(),
        editor.clone(),
        page_status.clone(),
        save_status.clone(),
    );

    #[cfg(not(target_os = "macos"))]
    let _menu_bar = create_menu(
        app_state.clone(),
        autosave_state.clone(),
        editor.clone(),
        page_status.clone(),
        save_status.clone(),
    );

    // Get the editor widget and set it up
    let mut ed_widget = editor.borrow().widget();
    ed_widget.set_color(enums::Color::from_rgb(255, 255, 245));

    wind.end();
    wind.resizable(&ed_widget);
    wind.show();

    // Load initial page
    load_page_helper(
        &args.page,
        &app_state,
        &autosave_state,
        &editor,
        &page_status,
        &save_status,
    );

    // Set up immediate restyling on text changes and auto-save
    {
        let editor_for_callback = editor.clone();
        let autosave_for_callback = autosave_state.clone();
        let app_state_for_callback = app_state.clone();
        let save_status_for_callback = save_status.clone();
        let mut editor_widget = editor.borrow_mut();

        // Set up a callback that triggers on text modifications
        let widget = editor_widget.widget_mut();
        widget.set_trigger(enums::CallbackTrigger::Changed);
        widget.set_callback(move |_| {
            // Use awake to defer restyling to next event loop iteration
            let editor_clone = editor_for_callback.clone();
            app::awake_callback(move || {
                if let Ok(mut ed) = editor_clone.try_borrow_mut() {
                    ed.restyle();
                }
            });

            // Mark content as changed in autosave state
            if let Ok(mut as_state) = autosave_for_callback.try_borrow_mut() {
                as_state.mark_changed();
            }

            // Schedule debounced save (1 second delay)
            let editor_clone = editor_for_callback.clone();
            let autosave_clone = autosave_for_callback.clone();
            let app_state_clone = app_state_for_callback.clone();
            let save_status_clone = save_status_for_callback.clone();

            app::add_timeout3(1.0, move |_| {
                // Check if save is still pending
                let should_save = autosave_clone
                    .try_borrow()
                    .map(|s| s.pending_save)
                    .unwrap_or(false);

                if should_save {
                    // Update status to "Saving..."
                    if let Ok(mut status) = save_status_clone.try_borrow_mut() {
                        status.set_label("Saving...");
                        app::redraw();
                    }

                    // Perform the save
                    if let (Ok(ed), Ok(mut as_state), Ok(app_st)) = (
                        editor_clone.try_borrow(),
                        autosave_clone.try_borrow_mut(),
                        app_state_clone.try_borrow(),
                    ) {
                        match as_state.trigger_save(&ed, &app_st.store) {
                            Ok(()) => {
                                // Update status with new save time
                                if let Ok(mut status) = save_status_clone.try_borrow_mut() {
                                    status.set_label(&as_state.get_status_text());
                                    app::redraw();
                                }
                            }
                            Err(e) => {
                                // Show error
                                if let Ok(mut status) = save_status_clone.try_borrow_mut() {
                                    status.set_label(&format!("Error: {}", e));
                                    app::redraw();
                                }
                            }
                        }
                    }
                }
            });
        });
    }

    // Set up click handler for links
    {
        let app_state = app_state.clone();
        let autosave_state_for_links = autosave_state.clone();
        let editor_ref = editor.clone();
        let page_status_ref = page_status.clone();
        let save_status_ref = save_status.clone();

        ed_widget.handle(move |widget, evt| {
            // Block keyboard input if in read-only mode
            if let Ok(ed) = editor_ref.try_borrow() {
                if ed.is_readonly() {
                    match evt {
                        enums::Event::KeyDown | enums::Event::KeyUp => {
                            // Allow arrow keys, page up/down, home/end for navigation
                            let key = app::event_key();
                            match key {
                                enums::Key::Left
                                | enums::Key::Right
                                | enums::Key::Up
                                | enums::Key::Down
                                | enums::Key::Home
                                | enums::Key::End
                                | enums::Key::PageUp
                                | enums::Key::PageDown => {
                                    // Allow navigation keys
                                    return false;
                                }
                                _ => {
                                    // Block all other keys (typing, backspace, delete, etc.)
                                    return true;
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            match evt {
                enums::Event::Push => {
                    // Get the click position
                    let click_pos = widget.insert_position();

                    // Check if we clicked on a link
                    if let Some(link_dest) = editor_ref
                        .borrow()
                        .find_link_at_position(click_pos as usize)
                    {
                        // Navigate to the linked page - defer to avoid borrow conflict
                        let app_state = app_state.clone();
                        let autosave_state = autosave_state_for_links.clone();
                        let editor_ref = editor_ref.clone();
                        let page_status = page_status_ref.clone();
                        let save_status = save_status_ref.clone();

                        // Use awake callback to defer the page load until after event handler returns
                        app::awake_callback(move || {
                            load_page_helper(
                                &link_dest,
                                &app_state,
                                &autosave_state,
                                &editor_ref,
                                &page_status,
                                &save_status,
                            );
                        });
                        return true;
                    }
                    false
                }
                enums::Event::Move => {
                    // Could change cursor when over a link
                    let pos = widget.insert_position();
                    if editor_ref
                        .borrow()
                        .find_link_at_position(pos as usize)
                        .is_some()
                    {
                        widget.window().unwrap().set_cursor(enums::Cursor::Hand);
                    } else {
                        widget.window().unwrap().set_cursor(enums::Cursor::Default);
                    }
                    false
                }
                _ => false,
            }
        });
    }

    // Set up periodic timer to update "X ago" display
    {
        let autosave_ref = autosave_state.clone();
        let save_status_ref = save_status.clone();

        app::add_timeout3(1.0, move |handle| {
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
            app::repeat_timeout3(1.0, handle);
        });
    }

    app.run().unwrap();
}
