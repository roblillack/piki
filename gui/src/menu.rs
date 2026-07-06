use super::{
    AppState, AutoSaveState, load_note_helper, navigate_back, navigate_forward, note_picker,
    rename_current_note, search_bar::SearchBar, statusbar::StatusBar, window_state::WindowGeometry,
};
// Only the non-macOS in-app Quit item saves explicitly; on macOS the system
// Quit routes through the window Close event, which already saves.
#[cfg(not(target_os = "macos"))]
use super::save_current_note;
use chrono::Local;
use fltk::{
    app, button, dialog,
    enums::{self, Key, Shortcut},
    frame, input, menu,
    prelude::*,
    window,
};
use piki_gui::link_editor::{self, LinkEditOptions};
use piki_gui::note_ui::NoteUI;
use piki_gui::ui_adapters::StructuredRichUI;
use rutle::structured_document::{BlockType, InlineContent};
use std::cell::RefCell;
use std::rc::Rc;

const FORMAT_PARAGRAPH: &str = "Format/Text";
const FORMAT_HEADING1: &str = "Format/Heading 1";
const FORMAT_HEADING2: &str = "Format/Heading 2";
const FORMAT_HEADING3: &str = "Format/Heading 3";
const FORMAT_QUOTE: &str = "Format/Quote";
const FORMAT_CODE_BLOCK: &str = "Format/Code Block";
const FORMAT_NUMBERED_LIST: &str = "Format/Numbered List";
const FORMAT_LIST_ITEM: &str = "Format/List Item";
const FORMAT_CHECKLIST_ITEM: &str = "Format/_Checklist Item";

const FORMAT_INLINE_BOLD: &str = "Format/Bold";
const FORMAT_INLINE_ITALIC: &str = "Format/Italic";
const FORMAT_INLINE_UNDERLINE: &str = "Format/Underline";
const FORMAT_INLINE_CODE: &str = "Format/Code";
const FORMAT_INLINE_HIGHLIGHT: &str = "Format/Highlight";
const FORMAT_INLINE_STRIKE: &str = "Format/_Strikethrough";
const FORMAT_EDIT_LINK: &str = "Format/Edit Link…";

const FORMAT_CLEAR: &str = "Format/Clear formatting";

const VIEW_FULLSCREEN: &str = "View/Fullscreen";

// Default padding for normal mode
const DEFAULT_PADDING: i32 = 25;
// Target text width in characters for fullscreen mode
const FULLSCREEN_TARGET_CHARS: i32 = 90;

const PARAGRAPH_ITEMS: &[&str] = &[
    FORMAT_PARAGRAPH,
    FORMAT_HEADING1,
    FORMAT_HEADING2,
    FORMAT_HEADING3,
    FORMAT_QUOTE,
    FORMAT_CODE_BLOCK,
    FORMAT_NUMBERED_LIST,
    FORMAT_LIST_ITEM,
    FORMAT_CHECKLIST_ITEM,
];

const INLINE_ITEMS: &[&str] = &[
    FORMAT_INLINE_BOLD,
    FORMAT_INLINE_ITALIC,
    FORMAT_INLINE_UNDERLINE,
    FORMAT_INLINE_CODE,
    FORMAT_INLINE_HIGHLIGHT,
    FORMAT_INLINE_STRIKE,
    FORMAT_EDIT_LINK,
];

#[cfg(target_os = "macos")]
#[allow(clippy::too_many_arguments)]
pub fn setup_menu(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
    window_geometry: Rc<RefCell<WindowGeometry>>,
    search_bar: Rc<RefCell<SearchBar>>,
) {
    let mut menu_bar = menu::SysMenuBar::default();
    populate_menu(
        &mut menu_bar,
        app_state,
        autosave_state,
        active_editor,
        statusbar,
        wind_ref,
        window_geometry,
        search_bar,
    );
}

#[cfg(not(target_os = "macos"))]
#[allow(clippy::too_many_arguments)]
pub fn setup_menu(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
    window_geometry: Rc<RefCell<WindowGeometry>>,
    search_bar: Rc<RefCell<SearchBar>>,
) -> menu::MenuBar {
    let mut menu_bar = menu::MenuBar::new(0, 0, 660, 25, None);
    populate_menu(
        &mut menu_bar,
        app_state,
        autosave_state,
        active_editor,
        statusbar,
        wind_ref,
        window_geometry,
        search_bar,
    );
    menu_bar
}

#[allow(clippy::too_many_arguments)]
fn populate_menu<M>(
    menu_bar: &mut M,
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
    window_geometry: Rc<RefCell<WindowGeometry>>,
    search_bar: Rc<RefCell<SearchBar>>,
) where
    M: MenuExt + Clone + 'static,
{
    let cmd = if cfg!(target_os = "macos") {
        Shortcut::Command
    } else {
        Shortcut::Ctrl
    };
    let new_shortcut = cmd | 'n';
    let rename_shortcut = cmd | 's';
    let goto_note_shortcut = cmd | 'o';

    let back_shortcut = if cfg!(target_os = "macos") {
        Shortcut::Command | '['
    } else {
        Shortcut::Alt | Key::Left
    };

    let forward_shortcut = if cfg!(target_os = "macos") {
        Shortcut::Command | ']'
    } else {
        Shortcut::Alt | Key::Right
    };

    let frontpage_shortcut = cmd | Shortcut::Alt | 'f';
    let index_shortcut = cmd | Shortcut::Alt | 'i';
    #[cfg(not(target_os = "macos"))]
    let quit_shortcut = cmd | 'q';
    let cut_shortcut = cmd | 'x';
    let copy_shortcut = cmd | 'c';
    let paste_shortcut = cmd | 'v';
    let paragraph_shortcut = cmd | Shortcut::Alt | '0';
    let heading1_shortcut = cmd | Shortcut::Alt | '1';
    let heading2_shortcut = cmd | Shortcut::Alt | '2';
    let heading3_shortcut = cmd | Shortcut::Alt | '3';
    let quote_shortcut = cmd | Shortcut::Shift | '5';
    let code_block_shortcut = cmd | Shortcut::Shift | '6';
    let ordered_list_shortcut = cmd | Shortcut::Shift | '7';
    let list_shortcut = cmd | Shortcut::Shift | '8';
    let checklist_shortcut = cmd | Shortcut::Shift | '9';
    let bold_shortcut = cmd | 'b';
    let italic_shortcut = cmd | 'i';
    let underline_shortcut = cmd | 'u';
    let code_inline_shortcut = cmd | Shortcut::Shift | 'c';
    let highlight_shortcut = cmd | Shortcut::Shift | 'h';
    let strike_shortcut = cmd | Shortcut::Shift | 'x';
    let edit_link_shortcut = cmd | 'k';
    let clear_shortcut = cmd | '\\';
    let undo_shortcut = cmd | 'z';
    let redo_shortcut = cmd | Shortcut::Shift | 'z';

    // Write room shortcut: Ctrl/Cmd-Shift-F
    let fullscreen_shortcut = cmd | Shortcut::Shift | 'f';

    // Note menu
    // New Note creates an auto-named `untitled_…` note and opens it immediately,
    // so a quick thought can be captured without first inventing a name; the note
    // is given a real name later with Rename Note (Cmd-S).
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(
            "Note/New Note",
            new_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                load_note_helper(
                    &default_new_note_name(),
                    &app_state,
                    &autosave_state,
                    &active_editor,
                    &statusbar,
                    None,
                );
            },
        );
    }

    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let wind_ref = wind_ref.clone();
        menu_bar.add(
            "Note/Rename Note …",
            rename_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                show_rename_dialog(
                    app_state.clone(),
                    autosave_state.clone(),
                    active_editor.clone(),
                    statusbar.clone(),
                    wind_ref.clone(),
                );
            },
        );
    }

    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let wind_ref = wind_ref.clone();
        menu_bar.add(
            "Note/_Open Note …",
            goto_note_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                if let Ok(w) = wind_ref.try_borrow() {
                    note_picker::show_note_picker(
                        app_state.clone(),
                        autosave_state.clone(),
                        active_editor.clone(),
                        statusbar.clone(),
                        &w,
                    );
                }
            },
        );
    }

    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(
            "Note/Back",
            back_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                navigate_back(&app_state, &autosave_state, &active_editor, &statusbar);
            },
        );
    }

    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(
            "Note/_Forward",
            forward_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                navigate_forward(&app_state, &autosave_state, &active_editor, &statusbar);
            },
        );
    }

    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(
            "Note/Go to Frontpage",
            frontpage_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                load_note_helper(
                    "frontpage",
                    &app_state,
                    &autosave_state,
                    &active_editor,
                    &statusbar,
                    None,
                );
            },
        );
    }

    {
        #[cfg(not(target_os = "macos"))]
        let label = "Note/_Go to Index";
        // No separator on macOS for this item,
        // as there's not going to be a Quit item below it.
        #[cfg(target_os = "macos")]
        let label = "Note/Go to Index";
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(label, index_shortcut, menu::MenuFlag::Normal, move |_| {
            load_note_helper(
                "!index",
                &app_state,
                &autosave_state,
                &active_editor,
                &statusbar,
                None,
            );
        });
    }

    #[cfg(not(target_os = "macos"))]
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(
            "Note/Quit",
            quit_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                // Save the open note before leaving.
                save_current_note(&app_state, &autosave_state, &active_editor, &statusbar);
                app::quit();
            },
        );
    }

    // Edit menu
    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            "Edit/Undo",
            undo_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_undo(&active_editor);
            },
        );
    }

    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            "Edit/_Redo",
            redo_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_redo(&active_editor);
            },
        );
    }

    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            "Edit/Cut",
            cut_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_cut(&active_editor);
            },
        );
    }

    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            "Edit/Copy",
            copy_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_copy(&active_editor);
            },
        );
    }

    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            "Edit/Paste",
            paste_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_paste(&active_editor);
            },
        );
    }

    // Find (Cmd/Ctrl+F)
    {
        let search_bar = search_bar.clone();
        let active_editor = active_editor.clone();
        menu_bar.add(
            "Edit/Find…",
            cmd | Key::from_char('f'),
            menu::MenuFlag::Normal,
            move |_| {
                if let Ok(mut sb) = search_bar.try_borrow_mut() {
                    if sb.visible() {
                        // If already visible, just focus the input
                        sb.take_focus();
                    } else {
                        // Move editor down to make room for search bar
                        if let Ok(ed_ptr) = active_editor.try_borrow()
                            && let Ok(mut ed) = ed_ptr.try_borrow_mut()
                            && let Some(structured) =
                                ed.as_any_mut().downcast_mut::<StructuredRichUI>()
                        {
                            let bar_h = crate::search_bar::BAR_HEIGHT;
                            let x = structured.x();
                            let y = structured.y();
                            let w = structured.width();
                            let h = structured.height();
                            // Resize search bar to match editor width and position
                            sb.resize(x, y, w);
                            structured.resize(x, y + bar_h, w, h - bar_h);
                        }
                        sb.show();
                    }
                    app::redraw();
                }
            },
        );
    }

    // Reveal Codes (Cmd/Ctrl-R): surface rutle's inline-style tags (`[Bold>`…)
    // inline. A plain action rather than a checkmarked toggle, because it can
    // also be flipped from the keyboard (Cmd/Ctrl-R / F9, handled in the editor)
    // while the editor has focus — keeping a menu checkmark in sync would give
    // it a chance to go stale. The tags appearing in the document are the
    // feedback that the mode is on.
    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            "View/Reveal Codes",
            cmd | 'r',
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, false, |editor| {
                    editor.toggle_reveal_codes()
                });
                app::redraw();
            },
        );
    }

    // Write Room mode (fullscreen with centered text)
    {
        let wind_ref = wind_ref.clone();
        let window_geometry = window_geometry.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let search_bar = search_bar.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            VIEW_FULLSCREEN,
            fullscreen_shortcut,
            menu::MenuFlag::Toggle,
            move |_| {
                toggle_fullscreen(
                    &wind_ref,
                    &window_geometry,
                    &active_editor,
                    &statusbar,
                    &search_bar,
                    &menu_handle,
                );
            },
        );
    }

    // Initialize write room menu state based on saved state
    if let Some(mut item) = menu_bar.find_item(VIEW_FULLSCREEN) {
        if window_geometry.borrow().fullscreen {
            item.set();
        } else {
            item.clear();
        }
    }

    // Format menu - paragraph styles
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_PARAGRAPH,
            paragraph_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.set_block_type(BlockType::Paragraph)
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_HEADING1,
            heading1_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.set_block_type(BlockType::Heading { level: 1 })
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_HEADING2,
            heading2_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.set_block_type(BlockType::Heading { level: 2 })
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_HEADING3,
            heading3_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.set_block_type(BlockType::Heading { level: 3 })
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_QUOTE,
            quote_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ =
                    with_structured_editor(&active_editor, true, |editor| editor.toggle_quote());
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_CODE_BLOCK,
            code_block_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.toggle_code_block()
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_NUMBERED_LIST,
            ordered_list_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.toggle_ordered_list()
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_LIST_ITEM,
            list_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| editor.toggle_list());
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_CHECKLIST_ITEM,
            checklist_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.toggle_checklist()
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }

    // Format menu - inline styles
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_BOLD,
            bold_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| editor.toggle_bold());
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_ITALIC,
            italic_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ =
                    with_structured_editor(&active_editor, true, |editor| editor.toggle_italic());
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_UNDERLINE,
            underline_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.toggle_underline()
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_CODE,
            code_inline_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| editor.toggle_code());
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_HIGHLIGHT,
            highlight_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.toggle_highlight()
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_STRIKE,
            strike_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, true, |editor| {
                    editor.toggle_strikethrough()
                });
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        menu_bar.add(
            FORMAT_EDIT_LINK,
            edit_link_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_edit_link(&active_editor);
            },
        );
    }

    // Format menu - clear formatting
    {
        let active_editor = active_editor.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_CLEAR,
            clear_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_clear_formatting(&active_editor);
                update_format_menu_state(&menu_handle, &active_editor);
            },
        );
    }

    update_format_menu_state(menu_bar, &active_editor);
    register_paragraph_callback(menu_bar, &active_editor);
}

fn perform_undo(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    let _ = with_structured_editor(active_editor, true, |editor| {
        editor.undo();
    });
}

fn perform_redo(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    let _ = with_structured_editor(active_editor, true, |editor| {
        editor.redo();
    });
}

fn perform_cut(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    if with_structured_editor(active_editor, true, |editor| editor.cut_selection()).is_some() {
        app::redraw();
    }
}

fn perform_copy(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    let _ = with_structured_editor(active_editor, false, |editor| editor.copy_selection());
}

fn perform_paste(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    let _ = with_structured_editor(active_editor, true, |editor| {
        editor.paste_from_clipboard();
    });
}

fn perform_clear_formatting(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    if let Some(changed) =
        with_structured_editor(active_editor, true, |editor| editor.clear_formatting())
        && changed
    {
        app::redraw();
    }
}

fn perform_edit_link(active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>) {
    let init_data = with_structured_editor_ref(active_editor, |editor| {
        if editor.is_readonly() {
            return None;
        }

        let display = editor.0.display.borrow();
        let (init_target, init_text, mode_existing_link, selection_mode, link_pos) =
            if let Some((block_idx, inline_idx)) = display.hovered_link() {
                let content =
                    rutle::tree_walk::leaf_inline(display.editor().document(), &block_idx);
                if let Some(InlineContent::Link {
                    link,
                    content: inner,
                }) = content.get(inline_idx)
                {
                    let text = inner.iter().map(|c| c.to_plain_text()).collect::<String>();
                    (
                        link.destination.clone(),
                        text,
                        true,
                        false,
                        Some((block_idx, inline_idx)),
                    )
                } else {
                    (String::new(), String::new(), false, false, None)
                }
            } else if let Some((start, end)) = display.editor().selection() {
                let text = display.editor().text_in_range(start, end);
                (String::new(), text, false, true, None)
            } else {
                (String::new(), String::new(), false, false, None)
            };

        drop(display);

        let group = editor.0.group.clone();
        let parent = group.parent().unwrap_or_else(|| group.clone());
        let center_rect = Some((parent.x(), parent.y(), parent.w(), parent.h()));

        Some((
            init_target,
            init_text,
            mode_existing_link,
            selection_mode,
            link_pos,
            center_rect,
        ))
    });

    let Some((init_target, init_text, mode_existing_link, selection_mode, link_pos, center_rect)) =
        init_data.flatten()
    else {
        return;
    };

    let opts = LinkEditOptions {
        init_target,
        init_text,
        mode_existing_link,
        selection_mode,
        center_rect,
    };

    let active_editor_save = Rc::clone(active_editor);
    let link_pos_for_save = link_pos.clone();
    let remove_cb = if link_pos.is_some() {
        let active_editor_remove = Rc::clone(active_editor);
        let link_pos_remove = link_pos.clone();
        Some(move || {
            if let Some((block_idx, inline_idx)) = link_pos_remove.clone() {
                let _ = with_structured_editor(&active_editor_remove, true, |editor| {
                    let changed = {
                        let mut disp = editor.0.display.borrow_mut();
                        let editor_mut = disp.editor_mut();
                        editor_mut
                            .remove_link_at(block_idx.clone(), inline_idx)
                            .is_ok()
                    };
                    if changed {
                        editor.0.notify_change();
                        editor.0.emit_paragraph_state();
                    }
                });
            }
        })
    } else {
        None
    };

    link_editor::show_link_editor(
        opts,
        move |dest: String, txt: String| {
            let _ = with_structured_editor(&active_editor_save, true, |editor| {
                let changed = {
                    let mut disp = editor.0.display.borrow_mut();
                    let editor_mut = disp.editor_mut();

                    if let Some((block_idx, inline_idx)) = link_pos_for_save.clone() {
                        editor_mut
                            .edit_link_at(block_idx.clone(), inline_idx, &dest, &txt)
                            .is_ok()
                    } else if !txt.is_empty() {
                        if editor_mut.selection().is_some() {
                            editor_mut.replace_selection_with_link(&dest, &txt).is_ok()
                        } else {
                            editor_mut.insert_link_at_cursor(&dest, &txt).is_ok()
                        }
                    } else {
                        false
                    }
                };

                if changed {
                    editor.0.notify_change();
                    editor.0.emit_paragraph_state();
                }
            });
        },
        remove_cb,
    );
}

fn with_structured_editor<F, R>(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    require_writable: bool,
    mut f: F,
) -> Option<R>
where
    F: FnMut(&mut StructuredRichUI) -> R,
{
    if let Ok(active_ptr) = active_editor.try_borrow() {
        let editor_rc = active_ptr.clone();
        drop(active_ptr);
        if let Ok(mut editor) = editor_rc.try_borrow_mut() {
            if require_writable && editor.is_readonly() {
                return None;
            }
            if let Some(structured) = editor.as_any_mut().downcast_mut::<StructuredRichUI>() {
                return Some(f(structured));
            }
        }
    }
    None
}

fn with_structured_editor_ref<F, R>(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    f: F,
) -> Option<R>
where
    F: FnOnce(&StructuredRichUI) -> R,
{
    if let Ok(active_ptr) = active_editor.try_borrow() {
        let editor_rc = active_ptr.clone();
        drop(active_ptr);
        if let Ok(editor) = editor_rc.try_borrow()
            && let Some(structured) = editor.as_any().downcast_ref::<StructuredRichUI>()
        {
            return Some(f(structured));
        }
    }
    None
}

fn paragraph_label_for_block(block: &BlockType) -> Option<&'static str> {
    match block {
        BlockType::Paragraph => Some(FORMAT_PARAGRAPH),
        BlockType::Heading { level } => match level {
            1 => Some(FORMAT_HEADING1),
            2 => Some(FORMAT_HEADING2),
            3 => Some(FORMAT_HEADING3),
            _ => None,
        },
        BlockType::CodeBlock { .. } => Some(FORMAT_CODE_BLOCK),
        BlockType::BlockQuote => Some(FORMAT_QUOTE),
        BlockType::ListItem {
            ordered, checkbox, ..
        } => {
            if *ordered {
                Some(FORMAT_NUMBERED_LIST)
            } else if checkbox.is_some() {
                Some(FORMAT_CHECKLIST_ITEM)
            } else {
                Some(FORMAT_LIST_ITEM)
            }
        }
        // Tables have no paragraph-style menu entry.
        BlockType::Table { .. } => None,
    }
}

fn update_format_menu_state<M: MenuExt>(
    menu: &M,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
) {
    let mut readonly = true;
    let mut current_label: Option<&'static str> = None;

    if let Some((block, ro)) = with_structured_editor_ref(active_editor, |editor| {
        (editor.current_block_type(), editor.is_readonly())
    }) {
        readonly = ro;
        if let Some(block_type) = block {
            current_label = paragraph_label_for_block(&block_type);
        }
    }

    for &label in PARAGRAPH_ITEMS {
        if let Some(mut item) = menu.find_item(label) {
            if !readonly {
                item.activate();
            } else {
                item.deactivate();
            }
            item.clear();
        }
    }

    if let Some(label) = current_label
        && let Some(mut item) = menu.find_item(label)
    {
        item.set();
    }

    for &label in INLINE_ITEMS {
        if let Some(mut item) = menu.find_item(label) {
            if !readonly {
                item.activate();
            } else {
                item.deactivate();
            }
        }
    }

    if let Some(mut item) = menu.find_item(FORMAT_CLEAR) {
        if !readonly {
            item.activate();
        } else {
            item.deactivate();
        }
    }
}

fn register_paragraph_callback<M: MenuExt + Clone + 'static>(
    menu: &M,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
) {
    let menu_rc = Rc::new(menu.clone());
    let active_editor_rc = active_editor.clone();
    let _ = with_structured_editor(active_editor, false, |editor| {
        let menu_for_cb = menu_rc.clone();
        let active_for_cb = active_editor_rc.clone();
        editor.on_paragraph_style_change(Box::new(move |_block_type| {
            println!("Paragraph style changed callback triggered");
            let menu_clone = menu_for_cb.clone();
            let active_clone = active_for_cb.clone();
            app::awake_callback(move || {
                update_format_menu_state(&*menu_clone, &active_clone);
            });
        }));
    });

    let menu_for_init = menu_rc.clone();
    let active_for_init = active_editor_rc.clone();
    app::awake_callback(move || {
        update_format_menu_state(&*menu_for_init, &active_for_init);
    });
}

/// The auto-generated name for a quick new note, e.g.
/// `untitled_2026-07-04_153412`. Seconds are included so two notes created
/// within the same minute do not collide onto the same file.
fn default_new_note_name() -> String {
    format!("untitled_{}", Local::now().format("%Y-%m-%d_%H%M%S"))
}

/// Whether `name` is an auto-generated quick-note name (see
/// [`default_new_note_name`]) that has not been given a real name yet.
fn is_untitled(name: &str) -> bool {
    name.starts_with("untitled_")
}

/// Prompt for a new name for the currently open note and rename it in place
/// (see [`rename_current_note`]). This is how a quick, auto-named note gets a
/// real name, but it works on any note.
fn show_rename_dialog(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
) {
    let current_name = app_state.borrow().current_note.clone();

    // Read-only plugin views (e.g. !index) have no file to rename.
    if current_name.starts_with('!') {
        dialog::alert_default("This note cannot be renamed.");
        return;
    }

    let width = 360;
    let height = 140;

    let (px, py, pw, ph) = if let Ok(win) = wind_ref.try_borrow() {
        (win.x(), win.y(), win.w(), win.h())
    } else {
        let (sx, sy, sw, sh) = app::screen_xywh(0);
        (sx, sy, sw, sh)
    };
    let pos_x = px + (pw - width) / 2;
    let pos_y = py + (ph - height) / 2;

    let mut win = window::Window::new(
        pos_x.max(0),
        pos_y.max(0),
        width,
        height,
        Some("Rename Note"),
    );
    win.make_modal(true);
    win.begin();

    let mut label = frame::Frame::new(10, 10, width - 20, 24, Some("Rename note to:"));
    label.set_align(enums::Align::Inside | enums::Align::Left);

    let mut input = input::Input::new(10, 40, width - 20, 28, None);
    // Start blank for an unnamed quick note so the user just types a name; for a
    // note that already has a real name, pre-fill it so this acts as an edit.
    if !is_untitled(&current_name) {
        input.set_value(&current_name);
    }

    let mut cancel_btn = button::Button::new(width - 180, height - 40, 80, 30, Some("Cancel"));
    let mut rename_btn = button::ReturnButton::new(width - 90, height - 40, 80, 30, Some("Rename"));
    if input.value().trim().is_empty() {
        rename_btn.deactivate();
    }

    {
        let mut rename_btn_clone = rename_btn.clone();
        input.set_trigger(enums::CallbackTrigger::Changed);
        input.set_callback(move |inp| {
            if inp.value().trim().is_empty() {
                rename_btn_clone.deactivate();
            } else {
                rename_btn_clone.activate();
            }
        });
    }

    let input_for_rename = input.clone();
    {
        let mut win_for_rename = win.clone();
        rename_btn.set_callback(move |_| {
            let name = input_for_rename.value().trim().to_string();
            if name.is_empty() {
                return;
            }

            match rename_current_note(
                &name,
                &app_state,
                &autosave_state,
                &active_editor,
                &statusbar,
            ) {
                Ok(()) => {
                    win_for_rename.hide();
                    app::redraw();
                }
                // Keep the dialog open on failure (e.g. the name is taken) so
                // the user can correct the name.
                Err(e) => dialog::alert_default(&e),
            }
        });
    }

    let mut win_for_cancel = win.clone();
    cancel_btn.set_callback(move |_| {
        win_for_cancel.hide();
    });

    {
        let mut cancel_clone = cancel_btn.clone();
        win.handle(move |_, ev| {
            if ev == enums::Event::KeyDown && app::event_key() == Key::Escape {
                cancel_clone.do_callback();
                true
            } else {
                false
            }
        });
    }

    win.end();
    win.show();
    let _ = input.take_focus();
}

/// Calculate padding for write room mode to achieve target text width
fn calculate_fullscreen_padding(window_width: i32, font_size: i32) -> i32 {
    // Approximate character width as 0.5 * font_size for proportional fonts
    // This is a rough estimate; actual measurement would be more accurate
    let char_width = (font_size as f32 * 0.55) as i32;
    let target_text_width = char_width * FULLSCREEN_TARGET_CHARS;

    // Scrollbar width (must match SCROLLBAR_WIDTH in fltk_structured_rich_display.rs)
    let scrollbar_width = 15;
    let available_width = window_width - scrollbar_width;

    // Calculate padding to center the text
    let padding = (available_width - target_text_width) / 2;

    // Ensure minimum padding
    padding.max(DEFAULT_PADDING)
}

/// Toggle fullscreen mode (fullscreen with centered text)
fn toggle_fullscreen<M: MenuExt>(
    wind_ref: &Rc<RefCell<window::Window>>,
    window_geometry: &Rc<RefCell<WindowGeometry>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn NoteUI>>>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    search_bar: &Rc<RefCell<SearchBar>>,
    menu_handle: &M,
) {
    let entering_fullscreen = !window_geometry.borrow().fullscreen;

    // Update fullscreen state early, before window changes, so that Move/Resize
    // handlers can detect fullscreen mode and avoid overwriting the saved position
    window_geometry.borrow_mut().fullscreen = entering_fullscreen;

    // Get statusbar dimensions before toggling
    let statusbar_height = statusbar.borrow().height();

    // Update fullscreen state BEFORE triggering resize events
    // so that resize handlers know to skip their logic
    window_geometry.borrow_mut().fullscreen = entering_fullscreen;

    // Check if search bar is visible
    let search_bar_visible = search_bar
        .try_borrow()
        .map(|sb| sb.visible())
        .unwrap_or(false);
    let search_bar_height = if search_bar_visible {
        crate::search_bar::BAR_HEIGHT
    } else {
        0
    };

    if let Ok(mut win) = wind_ref.try_borrow_mut() {
        if entering_fullscreen {
            // Determine which screen the window is on using its center point
            let win_center_x = win.x() + win.width() / 2;
            let win_center_y = win.y() + win.height() / 2;
            let screen_num = app::screen_num(win_center_x, win_center_y);

            // Enter fullscreen mode
            win.fullscreen(true);

            // Calculate padding for ~90 char text width
            // Use the screen dimensions where the window is located
            let (_, _, screen_w, screen_h) = app::screen_xywh(screen_num);
            let font_size = 14; // Default body text font size from theme
            let padding = calculate_fullscreen_padding(screen_w, font_size);

            // Resize search bar if visible
            if search_bar_visible && let Ok(mut sb) = search_bar.try_borrow_mut() {
                // On macOS, editor_y is 0; otherwise it's 25 for menu bar
                #[cfg(target_os = "macos")]
                let editor_y = 0;
                #[cfg(not(target_os = "macos"))]
                let editor_y = 25;
                sb.resize(0, editor_y, screen_w);
            }

            // Apply padding and resize the editor to take full height
            if let Ok(active_ptr) = active_editor.try_borrow()
                && let Ok(mut editor) = active_ptr.try_borrow_mut()
                && let Some(structured) = editor.as_any_mut().downcast_mut::<StructuredRichUI>()
            {
                structured.set_horizontal_padding(padding);
                // Expand editor to full screen height (no statusbar)
                // Account for search bar if visible
                #[cfg(target_os = "macos")]
                let editor_y = 0;
                #[cfg(not(target_os = "macos"))]
                let editor_y = 25;
                let editor_top = editor_y + search_bar_height;
                structured.resize(0, editor_top, screen_w, screen_h - editor_top);
            }

            // Hide status bar
            statusbar.borrow_mut().hide();
        } else {
            // Exit fullscreen mode
            win.fullscreen(false);

            // Resize search bar if visible
            if search_bar_visible && let Ok(mut sb) = search_bar.try_borrow_mut() {
                #[cfg(target_os = "macos")]
                let editor_y = 0;
                #[cfg(not(target_os = "macos"))]
                let editor_y = 25;
                sb.resize(0, editor_y, win.width());
            }

            // Restore default padding and resize editor to make room for statusbar
            if let Ok(active_ptr) = active_editor.try_borrow()
                && let Ok(mut editor) = active_ptr.try_borrow_mut()
                && let Some(structured) = editor.as_any_mut().downcast_mut::<StructuredRichUI>()
            {
                structured.set_horizontal_padding(DEFAULT_PADDING);
                // Resize editor to window height minus statusbar
                // Account for search bar if visible
                #[cfg(target_os = "macos")]
                let editor_y = 0;
                #[cfg(not(target_os = "macos"))]
                let editor_y = 25;
                let editor_top = editor_y + search_bar_height;
                structured.resize(
                    0,
                    editor_top,
                    win.width(),
                    win.height() - editor_top - statusbar_height,
                );
            }

            // Show status bar again
            statusbar.borrow_mut().show();
        }
    }

    // Update menu item
    if let Some(mut item) = menu_handle.find_item(VIEW_FULLSCREEN) {
        if entering_fullscreen {
            item.set();
        } else {
            item.clear();
        }
    }

    app::redraw();
}
