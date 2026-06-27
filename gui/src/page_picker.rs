use std::cell::{Cell, RefCell};
use std::rc::Rc;

use fltk::{self, prelude::*, window};
use piki_gui::page_ui::PageUI;

use crate::autosave::AutoSaveState;

thread_local! {
    /// Guards against more than one picker being open at a time. Repeatedly
    /// triggering the shortcut would otherwise stack pickers, because on macOS
    /// the native system menu fires the Cmd-P key equivalent before FLTK's
    /// modal window can intercept it.
    static PICKER_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// The application menu saved while the picker is open, so it can be restored
/// verbatim on close. On macOS this is the previous `NSMenu`; elsewhere nothing
/// needs to be tracked.
#[cfg(target_os = "macos")]
type SavedAppMenu = Option<objc2::rc::Retained<objc2_app_kit::NSMenu>>;
#[cfg(not(target_os = "macos"))]
type SavedAppMenu = ();

/// Hide the application's menu bar so its keyboard shortcuts cannot fire while
/// the modal picker is open, returning the previous menu so it can be restored
/// untouched. Marking the FLTK window modal is not enough on macOS: the native
/// system menu dispatches key equivalents (e.g. Cmd-P) before FLTK's modal grab
/// can swallow them, which is what lets pickers stack today.
#[cfg(target_os = "macos")]
fn suspend_app_menu() -> SavedAppMenu {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let mtm = MainThreadMarker::new()?;
    let app = NSApplication::sharedApplication(mtm);
    let previous = app.mainMenu();
    app.setMainMenu(None);
    previous
}

/// Restore the menu captured by [`suspend_app_menu`].
#[cfg(target_os = "macos")]
fn restore_app_menu(saved: &SavedAppMenu) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    NSApplication::sharedApplication(mtm).setMainMenu(saved.as_deref());
}

#[cfg(not(target_os = "macos"))]
fn suspend_app_menu() -> SavedAppMenu {}

#[cfg(not(target_os = "macos"))]
fn restore_app_menu(_saved: &SavedAppMenu) {}

/// Modal quick page picker with fuzzy filtering and keyboard navigation.
pub fn show_page_picker(
    app_state: Rc<RefCell<super::AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    statusbar: Rc<RefCell<super::statusbar::StatusBar>>,
    parent: &window::Window,
) {
    use fltk::{
        browser::HoldBrowser,
        enums::{CallbackTrigger, Event, Key},
        input::Input,
        window::Window,
    };

    // Only one picker may be open at a time. Without this guard, pressing the
    // shortcut again while the picker is up would open another one on top.
    if PICKER_OPEN.with(|open| open.replace(true)) {
        return;
    }

    // Collect all pages once
    let all_pages: Vec<String> = app_state
        .borrow()
        .store
        .list_all_documents()
        .unwrap_or_else(|_| vec![]);

    // Create a modal dialog centered on parent
    let width = 520;
    let height = 420;
    let px = parent.x() + (parent.w() - width) / 2;
    let py = parent.y() + (parent.h() - height) / 2;
    let mut win = Window::new(px.max(0), py.max(0), width, height, Some("Open Page"));
    win.begin();
    win.make_modal(true);

    let mut input = Input::new(10, 10, width - 20, 28, None);
    let mut list = HoldBrowser::new(10, 50, width - 20, height - 60, None);
    list.set_scrollbar_size(12);

    // Disable the application menu (and therefore its keyboard shortcuts) for as
    // long as the picker is open, so it behaves like a real modal: app shortcuts
    // such as Cmd-P no longer reach the window underneath. The menu is restored
    // verbatim when the picker closes.
    // On non-macOS `suspend_app_menu()` returns `()`; wrapping a unit value is
    // intentional here so the type is uniform across platforms.
    #[allow(clippy::unit_arg)]
    let saved_menu: Rc<RefCell<SavedAppMenu>> = Rc::new(RefCell::new(suspend_app_menu()));

    // Single entry point for closing the picker: restore the menu, clear the
    // open guard and hide the window. Idempotent, so it is safe to call from
    // every close path (Escape, Enter, double-click, window close button).
    let close_picker: Rc<RefCell<dyn FnMut()>> = {
        let mut win = win.clone();
        let saved_menu = saved_menu.clone();
        Rc::new(RefCell::new(move || {
            if !PICKER_OPEN.with(|open| open.replace(false)) {
                return; // already closed
            }
            restore_app_menu(&saved_menu.borrow());
            win.hide();
        }))
    };

    // Helper: populate list with provided items and maintain selection
    let mut populate_list = {
        let mut list = list.clone();
        move |items: &Vec<String>, selected_index: Option<usize>| {
            list.clear();
            for s in items {
                list.add(s);
            }
            if let Some(idx) = selected_index {
                if !items.is_empty() {
                    let i = (idx.min(items.len() - 1) + 1) as i32; // 1-based
                    list.select(i);
                    list.top_line(i);
                }
            } else if !items.is_empty() {
                list.select(1);
                list.top_line(1);
            }
        }
    };

    // Simple fuzzy match: subsequence match with light scoring
    fn fuzzy_score(query: &str, candidate: &str) -> Option<i32> {
        if query.is_empty() {
            return Some(0);
        }
        let mut score = 0i32;
        let mut qi = 0usize;
        let q = query.to_lowercase();
        let c = candidate.to_lowercase();
        let qb = q.as_bytes();
        let cb = c.as_bytes();
        for (i, &ch) in cb.iter().enumerate() {
            if qi < qb.len() && ch == qb[qi] {
                // Reward matches earlier and consecutive
                score += 10 - ((i as i32).min(9));
                // Bonus for start of word or after '/'
                if i == 0 || cb.get(i - 1) == Some(&b'/') {
                    score += 5;
                }
                qi += 1;
                if qi == qb.len() {
                    break;
                }
            }
        }
        if qi == qb.len() {
            // Prefer prefix and exact
            if c.starts_with(&q) {
                score += 20;
            }
            if c == q {
                score += 50;
            }
            Some(score)
        } else {
            None
        }
    }

    // Current filtered results and selection index (0-based)
    let results: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(all_pages.clone()));
    let selection: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    // Initial population
    populate_list(&results.borrow(), Some(*selection.borrow()));

    // Filtering callback when input changes
    {
        let results = results.clone();
        let mut list = list.clone();
        input.set_trigger(CallbackTrigger::Changed);
        input.set_callback(move |inp| {
            let q = inp.value();
            let mut items: Vec<(i32, String)> = Vec::new();
            if q.trim().is_empty() {
                for s in &all_pages {
                    items.push((0, s.clone()));
                }
            } else {
                for s in &all_pages {
                    if let Some(sc) = fuzzy_score(&q, s) {
                        items.push((sc, s.clone()));
                    }
                }
            }
            // Sort by score desc then name asc
            items.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
            let filtered: Vec<String> = items.into_iter().map(|(_, s)| s).collect();
            *results.borrow_mut() = filtered;
            // Reset selection to top
            list.clear();
            for s in results.borrow().iter() {
                list.add(s);
            }
            if list.size() > 0 {
                list.select(1);
                list.top_line(1);
            }
        });
    }

    // Accept helper: open current selection and close dialog
    let accept_cb: Rc<RefCell<dyn FnMut()>> = {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let close_picker = close_picker.clone();
        let list_for_accept = list.clone();
        Rc::new(RefCell::new(move || {
            let idx = list_for_accept.value(); // 1-based
            let name_opt = if idx > 0 {
                list_for_accept.text(idx)
            } else {
                None
            };
            if let Some(name) = name_opt {
                (close_picker.borrow_mut())();
                super::load_page_helper(
                    &name,
                    &app_state,
                    &autosave_state,
                    &active_editor,
                    &statusbar,
                    None,
                );
            }
        }))
    };

    // Keyboard handling on input: up/down/select/esc
    {
        let mut list = list.clone();
        let accept_cb = accept_cb.clone();
        let close_picker = close_picker.clone();
        input.handle(move |_, ev| match ev {
            Event::KeyDown => {
                let key = fltk::app::event_key();
                match key {
                    Key::Down => {
                        let sz = list.size();
                        if sz > 0 {
                            let cur = list.value().max(1);
                            let next = (cur + 1).min(sz);
                            list.select(next);
                            list.top_line(next);
                        }
                        true
                    }
                    Key::Up => {
                        let sz = list.size();
                        if sz > 0 {
                            let cur = list.value().max(1);
                            let prev = (cur - 1).max(1);
                            list.select(prev);
                            list.top_line(prev);
                        }
                        true
                    }
                    Key::Enter => {
                        (accept_cb.borrow_mut())();
                        true
                    }
                    Key::Escape => {
                        // Cancel
                        (close_picker.borrow_mut())();
                        true
                    }
                    _ => false,
                }
            }
            _ => false,
        });
    }

    // Double-click or Enter on list accepts
    {
        let accept_cb = accept_cb.clone();
        let close_picker = close_picker.clone();
        list.handle(move |_, ev| match ev {
            Event::Push => {
                if fltk::app::event_clicks() {
                    (accept_cb.borrow_mut())();
                    true
                } else {
                    false
                }
            }
            Event::KeyDown => {
                if fltk::app::event_key() == Key::Enter {
                    (accept_cb.borrow_mut())();
                    true
                } else if fltk::app::event_key() == Key::Escape {
                    // Close on Esc
                    (close_picker.borrow_mut())();
                    true
                } else {
                    false
                }
            }
            _ => false,
        });
    }

    win.end();
    {
        // Closing via the window's close button must also restore the menu and
        // clear the open guard.
        let close_picker = close_picker.clone();
        win.set_callback(move |_| {
            (close_picker.borrow_mut())();
        });
    }
    win.show();
    let _ = input.take_focus();
}
