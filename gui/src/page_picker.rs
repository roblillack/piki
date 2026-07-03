use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::SystemTime;

use fltk::{self, draw, enums::Font, prelude::*, window};
use piki_gui::page_ui::PageUI;

use crate::autosave::AutoSaveState;

thread_local! {
    /// Guards against more than one picker being open at a time. Repeatedly
    /// triggering the shortcut would otherwise stack pickers, because on macOS
    /// the native system menu fires the Cmd-O key equivalent before FLTK's
    /// modal window can intercept it.
    static PICKER_OPEN: Cell<bool> = const { Cell::new(false) };
}

/// Text size (points) used for the browser rows. Kept in sync with the font we
/// measure against so ellipsis truncation lines up with what FLTK draws.
const ROW_TEXT_SIZE: i32 = 14;

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
/// system menu dispatches key equivalents (e.g. Cmd-O) before FLTK's modal grab
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

/// A shared, mutable callback taking a single string slice — used both for the
/// "filter by query" and "open note by name" actions.
type StrCallback = Rc<RefCell<dyn FnMut(&str)>>;

/// One entry in the picker list.
struct Row {
    /// Page name / path used to open the note.
    name: String,
    /// Short plaintext preview parsed from the first paragraphs of the note.
    abbrev: String,
    /// Preformatted last-modification timestamp (right-hand column).
    date: String,
    /// Last-opened time (ms since epoch), used to order notes by recency.
    last_open: Option<i64>,
    /// Last-modification time (ms since epoch), used as a secondary sort key.
    modified: Option<i64>,
}

/// Parse the first few paragraphs of a markdown note into a one-line plaintext
/// preview: markdown syntax is stripped and whitespace collapsed. The result is
/// capped at `max_chars`; the picker adds an ellipsis when it still overflows
/// the available column width.
fn abbreviate(markdown: &str, max_chars: usize) -> String {
    use pulldown_cmark::{Event, Options, Parser};

    let mut out = String::new();
    for event in Parser::new_ext(markdown, Options::empty()) {
        match event {
            Event::Text(t) | Event::Code(t) => out.push_str(&t),
            Event::SoftBreak | Event::HardBreak => out.push(' '),
            // A closing tag ends a block/inline run; keep words from adjacent
            // blocks (e.g. a heading and the paragraph below it) separated.
            Event::End(_) => out.push(' '),
            _ => {}
        }
        // Stop once we clearly have more than enough text for a preview.
        if out.len() >= max_chars * 4 {
            break;
        }
    }

    let collapsed = out.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > max_chars {
        collapsed.chars().take(max_chars).collect()
    } else {
        collapsed
    }
}

/// Format a modification time the way the mockup shows it: "Today 1:08 PM",
/// "Yesterday 9:30 AM", "Jul 3" within the current year, else "2026-07-03".
fn format_timestamp(time: SystemTime) -> String {
    use chrono::{DateTime, Datelike, Local};

    let dt: DateTime<Local> = time.into();
    let now = Local::now();
    let day = dt.date_naive();
    let today = now.date_naive();

    if day == today {
        dt.format("Today %-I:%M %p").to_string()
    } else if Some(day) == today.pred_opt() {
        dt.format("Yesterday %-I:%M %p").to_string()
    } else if dt.year() == now.year() {
        dt.format("%b %-d").to_string()
    } else {
        dt.format("%Y-%m-%d").to_string()
    }
}

fn millis_since_epoch(time: SystemTime) -> Option<i64> {
    time.duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as i64)
}

/// Width in pixels of `s` in the browser's font. Callers must have set the
/// measuring font via [`draw::set_font`] first.
fn text_width(s: &str) -> f64 {
    draw::width(s)
}

/// `@` starts a format code in FLTK browsers, and `\t` separates columns.
/// Double any `@` so page names / previews containing it (e.g. email
/// addresses) render literally, and drop stray tabs.
fn escape(s: &str) -> String {
    s.replace('@', "@@").replace('\t', " ")
}

/// Truncate `text` so it fits within `avail` pixels, appending an ellipsis when
/// anything is dropped. Assumes the measuring font is already set.
fn truncate_to_width(text: &str, avail: f64) -> String {
    if text_width(text) <= avail {
        return text.to_string();
    }
    let ellipsis = "…";
    let target = (avail - text_width(ellipsis)).max(0.0);
    let mut acc = String::new();
    let mut used = 0.0;
    let mut buf = [0u8; 4];
    for ch in text.chars() {
        let cw = text_width(ch.encode_utf8(&mut buf));
        if used + cw > target {
            break;
        }
        acc.push(ch);
        used += cw;
    }
    format!("{}{ellipsis}", acc.trim_end())
}

/// The left column text: "name — preview", with the preview ellipsized to fit
/// `avail` pixels while the name is kept intact whenever possible.
fn left_column(row: &Row, avail: f64) -> String {
    if row.abbrev.is_empty() {
        return truncate_to_width(&row.name, avail);
    }
    let prefix = format!("{} — ", row.name);
    let prefix_w = text_width(&prefix);
    if prefix_w + text_width(&row.abbrev) <= avail {
        format!("{prefix}{}", row.abbrev)
    } else if prefix_w >= avail {
        // Even the name barely fits; truncate it and drop the preview.
        truncate_to_width(&row.name, avail)
    } else {
        let preview = truncate_to_width(&row.abbrev, avail - prefix_w);
        format!("{prefix}{preview}")
    }
}

/// Build the full browser line (both columns) for a row. The measuring font
/// must already be set.
fn browser_line(row: &Row, left_avail: f64) -> String {
    let left = escape(&left_column(row, left_avail));
    if row.date.is_empty() {
        left
    } else {
        // Second column, right-aligned (`@r`), holding the timestamp.
        format!("{left}\t@r{}", escape(&row.date))
    }
}

/// Order all rows most-recently-opened first (never-opened notes sink to the
/// bottom, ordered by last modification), used when the query box is empty.
fn recency_order(rows: &[Row]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    order.sort_by(|&a, &b| {
        let ra = &rows[a];
        let rb = &rows[b];
        rb.last_open
            .cmp(&ra.last_open)
            .then(rb.modified.cmp(&ra.modified))
            .then_with(|| ra.name.to_lowercase().cmp(&rb.name.to_lowercase()))
    });
    order
}

// Simple fuzzy match: subsequence match with light scoring.
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

/// Next 1-based selection when stepping the quick-open cycle with the modifier
/// held. `cur` and `sz` are 1-based; the selection wraps around both ends.
/// Returns 0 for an empty list.
fn cycle_index(cur: i32, sz: i32, up: bool) -> i32 {
    if sz <= 0 {
        return 0;
    }
    let cur = cur.clamp(1, sz);
    if up {
        if cur <= 1 { sz } else { cur - 1 }
    } else if cur >= sz {
        1
    } else {
        cur + 1
    }
}

/// Order rows matching `query` by fuzzy score (best first), dropping non-matches.
fn fuzzy_order(rows: &[Row], query: &str) -> Vec<usize> {
    let mut scored: Vec<(i32, usize)> = rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| fuzzy_score(query, &r.name).map(|s| (s, i)))
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            rows[a.1]
                .name
                .to_lowercase()
                .cmp(&rows[b.1].name.to_lowercase())
        })
    });
    scored.into_iter().map(|(_, i)| i).collect()
}

/// Modal "Open Note" picker: fuzzy filtering, recency ordering, previews and
/// last-modified timestamps, with keyboard navigation.
pub fn show_page_picker(
    app_state: Rc<RefCell<super::AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    statusbar: Rc<RefCell<super::statusbar::StatusBar>>,
    parent: &window::Window,
) {
    use fltk::{
        browser::HoldBrowser,
        enums::{CallbackTrigger, Event, Key, Shortcut},
        input::Input,
        window::Window,
    };

    // Only one picker may be open at a time. Without this guard, pressing the
    // shortcut again while the picker is up would open another one on top.
    if PICKER_OPEN.with(|open| open.replace(true)) {
        return;
    }

    // Gather every note plus the metadata the list shows. We read each file once
    // here for its content (preview) and modification time; personal wikis are
    // small enough that this is cheap.
    let (rows, current_page) = {
        let state = app_state.borrow();
        let names = state.store.list_all_documents().unwrap_or_default();
        let current = state.current_page.clone();
        let rows: Vec<Row> = names
            .into_iter()
            .map(|name| {
                let doc = state.store.load(&name).ok();
                let content = doc.as_ref().map(|d| d.content.clone()).unwrap_or_default();
                let mtime = doc.as_ref().and_then(|d| d.modified_time);
                Row {
                    abbrev: abbreviate(&content, 200),
                    date: mtime.map(format_timestamp).unwrap_or_default(),
                    last_open: state.recent_pages.last_opened(&name),
                    modified: mtime.and_then(millis_since_epoch),
                    name,
                }
            })
            .collect();
        (rows, current)
    };
    let rows = Rc::new(rows);

    // Create a modal dialog centered on parent
    let width = 600;
    let height = 460;
    let px = parent.x() + (parent.w() - width) / 2;
    let py = parent.y() + (parent.h() - height) / 2;
    let mut win = Window::new(px.max(0), py.max(0), width, height, Some("Open Note"));
    win.begin();
    win.make_modal(true);

    let mut input = Input::new(10, 10, width - 20, 28, None);
    let mut list = HoldBrowser::new(10, 50, width - 20, height - 60, None);
    list.set_scrollbar_size(12);
    list.set_text_size(ROW_TEXT_SIZE);

    // Measure with the same font the browser draws in (default FLTK sans at our
    // row size) so ellipsis truncation matches on screen.
    draw::set_font(Font::Helvetica, ROW_TEXT_SIZE);

    // Split the row into a flexible left column (name + preview) and a fixed
    // right column just wide enough for the widest timestamp. FLTK only applies
    // colour/alignment codes at the start of a column, so the right-aligned date
    // has to live in its own column.
    let date_w = rows
        .iter()
        .map(|r| text_width(&r.date))
        .fold(0.0_f64, f64::max)
        + 28.0;
    // Conservative estimate of the drawable width (widget minus box + scrollbar)
    // so the date column never collides with the scrollbar.
    let inner = (width - 44) as f64;
    let left_w = (inner - date_w).max(140.0);
    list.set_column_char('\t');
    list.set_column_widths(&[left_w as i32]);
    // FLTK insets each field by a few pixels when drawing (see item_draw).
    let left_avail = left_w - 8.0;

    // Disable the application menu (and therefore its keyboard shortcuts) for as
    // long as the picker is open, so it behaves like a real modal: app shortcuts
    // such as Cmd-O no longer reach the window underneath. The menu is restored
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

    // Page names in current display order, parallel to the browser lines. The
    // browser text is formatted (columns + preview), so accepting a selection
    // maps the 1-based line back to a name through this list.
    let results: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));

    // Rebuild the list for a query: recency order when empty, fuzzy otherwise.
    // With an empty query we pre-select the *current* note (the top of the
    // recency list), so a held Cmd-O can then step the selection downwards.
    let refill: StrCallback = {
        let mut list = list.clone();
        let rows = rows.clone();
        let results = results.clone();
        let current_page = current_page.clone();
        Rc::new(RefCell::new(move |query: &str| {
            draw::set_font(Font::Helvetica, ROW_TEXT_SIZE);
            let q = query.trim();
            let order = if q.is_empty() {
                recency_order(&rows)
            } else {
                fuzzy_order(&rows, q)
            };

            list.clear();
            let mut names = Vec::with_capacity(order.len());
            for &i in &order {
                list.add(&browser_line(&rows[i], left_avail));
                names.push(rows[i].name.clone());
            }

            if !names.is_empty() {
                let target = if q.is_empty() {
                    Some(current_page.as_str())
                } else {
                    None
                };
                let line = target
                    .and_then(|t| names.iter().position(|n| n == t))
                    .map(|p| p as i32 + 1)
                    .unwrap_or(1);
                list.select(line);
                list.top_line(1);
            }
            *results.borrow_mut() = names;
        }))
    };

    // Initial population.
    (refill.borrow_mut())("");

    // Filter as the user types.
    {
        let refill = refill.clone();
        input.set_trigger(CallbackTrigger::Changed);
        input.set_callback(move |inp| {
            (refill.borrow_mut())(&inp.value());
        });
    }

    // Accept helper: open the currently selected row. Closes the picker first
    // (restoring the menu), then loads the note.
    let accept_cb: Rc<RefCell<dyn FnMut()>> = {
        let list = list.clone();
        let results = results.clone();
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let close_picker = close_picker.clone();
        Rc::new(RefCell::new(move || {
            let idx = list.value(); // 1-based
            if idx > 0
                && let Some(name) = results.borrow().get((idx - 1) as usize).cloned()
            {
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

    // Keyboard handling on the input. Three interaction styles are supported,
    // mirroring VS Code's quick-open:
    //   1. type to filter, then Enter;
    //   2. arrow keys to move the selection, then Enter;
    //   3. keep the Cmd/Ctrl modifier held after opening and tap the hotkey (O)
    //      again to step the selection downwards (Shift to go up); releasing the
    //      modifier then opens the highlighted note.
    // The app menu is suspended while the picker is open, so repeated Cmd-O
    // presses arrive here as key events instead of re-firing the menu.
    {
        let mut list = list.clone();
        let accept_cb = accept_cb.clone();
        let close_picker = close_picker.clone();
        // Set once the user taps the hotkey again while the modifier is held; a
        // subsequent modifier release then commits the selection. Left false in
        // the type/arrow flows so releasing the modifier does nothing there.
        let mut navigating = false;
        input.handle(move |_, ev| match ev {
            Event::KeyDown => {
                let key = fltk::app::event_key();
                let state = fltk::app::event_state();

                // Hotkey tapped again with the modifier still down: step the
                // selection (down, or up with Shift) and arm commit-on-release.
                if state.contains(Shortcut::Command) && key == Key::from_char('o') {
                    let sz = list.size();
                    if sz > 0 {
                        let next = cycle_index(list.value(), sz, state.contains(Shortcut::Shift));
                        list.select(next);
                        list.make_visible(next);
                        navigating = true;
                    }
                    return true;
                }

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
                        (close_picker.borrow_mut())();
                        true
                    }
                    _ => false,
                }
            }
            Event::KeyUp => {
                // Commit when the held modifier is released after cycling. Detect
                // either the modifier key's own release or the modifier bit
                // having cleared from the event state.
                if !navigating {
                    return false;
                }
                let key = fltk::app::event_key();
                #[cfg(target_os = "macos")]
                let released_modifier = key == Key::MetaL || key == Key::MetaR;
                #[cfg(not(target_os = "macos"))]
                let released_modifier = key == Key::ControlL || key == Key::ControlR;
                if released_modifier || !fltk::app::event_state().contains(Shortcut::Command) {
                    navigating = false;
                    (accept_cb.borrow_mut())();
                    return true;
                }
                false
            }
            _ => false,
        });
    }

    // Double-click or Enter on the list accepts; Escape cancels.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_down_steps_and_wraps() {
        assert_eq!(cycle_index(1, 3, false), 2);
        assert_eq!(cycle_index(2, 3, false), 3);
        assert_eq!(cycle_index(3, 3, false), 1); // wrap to top
    }

    #[test]
    fn cycle_up_steps_and_wraps() {
        assert_eq!(cycle_index(3, 3, true), 2);
        assert_eq!(cycle_index(2, 3, true), 1);
        assert_eq!(cycle_index(1, 3, true), 3); // wrap to bottom
    }

    #[test]
    fn cycle_handles_empty_and_unselected() {
        assert_eq!(cycle_index(0, 0, false), 0);
        // An unselected list (value 0) steps to the first item.
        assert_eq!(cycle_index(0, 3, false), 2); // clamps to 1, then steps down
        assert_eq!(cycle_index(0, 3, true), 3); // clamps to 1, then wraps up
    }

    #[test]
    fn abbreviate_strips_markdown_and_collapses() {
        let md = "# Title\n\nSome **bold** and `code` text.\n\n- item one\n- item two";
        let out = abbreviate(md, 200);
        assert_eq!(out, "Title Some bold and code text. item one item two");
    }

    #[test]
    fn escape_doubles_at_signs_and_strips_tabs() {
        assert_eq!(escape("a@b\tc"), "a@@b c");
    }
}
