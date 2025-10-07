mod document;
mod editor;
mod link_handler;

use clap::Parser;
use document::DocumentStore;
use editor::MarkdownEditor;
use fltk::{prelude::*, *};
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
    current_page: String,
}

impl AppState {
    fn new(store: DocumentStore, initial_page: String) -> Self {
        AppState {
            store,
            current_page: initial_page,
        }
    }

    fn load_page(&mut self, page_name: &str) -> Result<String, String> {
        match self.store.load(page_name) {
            Ok(doc) => {
                self.current_page = page_name.to_string();
                Ok(doc.content)
            }
            Err(e) => Err(e),
        }
    }
}

fn create_menu(
    app_state: Rc<RefCell<AppState>>,
    editor: Rc<RefCell<MarkdownEditor>>,
    status: Rc<RefCell<frame::Frame>>,
) -> menu::MenuBar {
    let mut menu_bar = menu::MenuBar::new(0, 0, 660, 25, None);

    // Index menu item
    {
        let app_state = app_state.clone();
        let editor = editor.clone();
        let status = status.clone();
        menu_bar.add(
            "&Index",
            enums::Shortcut::Ctrl | 'i',
            menu::MenuFlag::Normal,
            move |_| {
                load_page_helper("INDEX", &app_state, &editor, &status);
            },
        );
    }

    // Frontpage menu item
    {
        let app_state = app_state.clone();
        let editor = editor.clone();
        let status = status.clone();
        menu_bar.add(
            "&Frontpage",
            enums::Shortcut::Ctrl | 'f',
            menu::MenuFlag::Normal,
            move |_| {
                load_page_helper("frontpage", &app_state, &editor, &status);
            },
        );
    }

    menu_bar
}

fn load_page_helper(
    page_name: &str,
    app_state: &Rc<RefCell<AppState>>,
    editor: &Rc<RefCell<MarkdownEditor>>,
    status: &Rc<RefCell<frame::Frame>>,
) {
    match app_state.borrow_mut().load_page(page_name) {
        Ok(content) => {
            editor.borrow_mut().set_content(&content);

            // Check if this is a new document (content is empty and file doesn't exist)
            let is_new = content.is_empty();
            let status_text = if is_new {
                format!("Page: {} (new)", page_name)
            } else {
                format!("Page: {}", page_name)
            };

            status.borrow_mut().set_label(&status_text);
            app::redraw();
        }
        Err(e) => {
            status.borrow_mut().set_label(&format!("Error: {}", e));
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
        eprintln!(
            "Error: '{}' is not a directory",
            args.directory.display()
        );
        std::process::exit(1);
    }

    // Initialize FLTK
    let app = app::App::default();
    let mut wind = window::Window::default()
        .with_size(660, 400)
        .with_label("fliki-rs");

    wind.begin();

    // Create state
    let store = DocumentStore::new(args.directory.clone());
    let app_state = Rc::new(RefCell::new(AppState::new(store, args.page.clone())));
    let editor = Rc::new(RefCell::new(MarkdownEditor::new(0, 25, 660, 350)));
    let status = Rc::new(RefCell::new({
        let mut f = frame::Frame::new(560, 0, 100, 25, None);
        f.set_frame(enums::FrameType::FlatBox);
        f.set_color(enums::Color::Black);
        f.set_label_color(enums::Color::White);
        f.set_label("...");
        f
    }));

    // Create menu
    let _menu_bar = create_menu(app_state.clone(), editor.clone(), status.clone());

    // Get the editor widget and set it up
    let mut ed_widget = editor.borrow().widget();
    ed_widget.set_color(enums::Color::from_rgb(255, 255, 245));

    wind.end();
    wind.resizable(&ed_widget);
    wind.show();

    // Load initial page
    load_page_helper(&args.page, &app_state, &editor, &status);

    // Set up immediate restyling on text changes
    {
        let editor_for_callback = editor.clone();
        let mut editor_widget = editor.borrow_mut();

        // Set up a callback that triggers on text modifications
        let widget = editor_widget.widget_mut();
        widget.set_trigger(enums::CallbackTrigger::Changed);
        widget.set_callback(move |_| {
            // Use awake to defer restyling to next event loop iteration
            // This avoids borrow conflicts while still feeling instant
            let editor_clone = editor_for_callback.clone();
            app::awake_callback(move || {
                if let Ok(mut ed) = editor_clone.try_borrow_mut() {
                    ed.restyle();
                }
            });
        });
    }

    // Set up click handler for links
    {
        let app_state = app_state.clone();
        let editor_ref = editor.clone();
        let status = status.clone();

        ed_widget.handle(move |widget, evt| {
            match evt {
                enums::Event::Push => {
                    // Get the click position
                    let click_pos = widget.insert_position();

                    // Check if we clicked on a link
                    if let Some(link_dest) = editor_ref.borrow().find_link_at_position(click_pos as usize) {
                        // Navigate to the linked page - defer to avoid borrow conflict
                        let app_state = app_state.clone();
                        let editor_ref = editor_ref.clone();
                        let status = status.clone();

                        // Use awake callback to defer the page load until after event handler returns
                        app::awake_callback(move || {
                            load_page_helper(&link_dest, &app_state, &editor_ref, &status);
                        });
                        return true;
                    }
                    false
                }
                enums::Event::Move => {
                    // Could change cursor when over a link
                    let pos = widget.insert_position();
                    if editor_ref.borrow().find_link_at_position(pos as usize).is_some() {
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

    app.run().unwrap();
}
