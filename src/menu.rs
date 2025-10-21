use super::{
    AppState, AutoSaveState, editor::MarkdownEditor, load_page_helper, navigate_back,
    navigate_forward, page_picker, statusbar::StatusBar, wire_editor_callbacks,
};
use fliki_rs::link_editor::{self, LinkEditOptions};
use fliki_rs::page_ui::PageUI;
use fliki_rs::richtext::structured_document::{BlockType, InlineContent};
use fliki_rs::ui_adapters::StructuredRichUI;
use fltk::{
    app, button,
    enums::{self, Key, Shortcut},
    frame, input, menu,
    prelude::*,
    window,
};
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Copy, Clone, PartialEq, Eq)]
enum EditorKind {
    Structured,
    Markdown,
}

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
pub fn setup_menu(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: Rc<RefCell<bool>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
    editor_x: i32,
    editor_y: i32,
    editor_w: i32,
    editor_h: i32,
) {
    let mut menu_bar = menu::SysMenuBar::default();
    populate_menu(
        &mut menu_bar,
        app_state,
        autosave_state,
        active_editor,
        is_structured,
        statusbar,
        wind_ref,
        editor_x,
        editor_y,
        editor_w,
        editor_h,
    );
}

#[cfg(not(target_os = "macos"))]
pub fn setup_menu(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: Rc<RefCell<bool>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
    editor_x: i32,
    editor_y: i32,
    editor_w: i32,
    editor_h: i32,
) -> menu::MenuBar {
    let mut menu_bar = menu::MenuBar::new(0, 0, 660, 25, None);
    populate_menu(
        &mut menu_bar,
        app_state,
        autosave_state,
        active_editor,
        is_structured,
        statusbar,
        wind_ref,
        editor_x,
        editor_y,
        editor_w,
        editor_h,
    );
    menu_bar
}

fn populate_menu<M>(
    menu_bar: &mut M,
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: Rc<RefCell<bool>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
    editor_x: i32,
    editor_y: i32,
    editor_w: i32,
    editor_h: i32,
) where
    M: MenuExt + Clone + 'static,
{
    let cmd = if cfg!(target_os = "macos") {
        Shortcut::Command
    } else {
        Shortcut::Ctrl
    };
    let new_shortcut = cmd | 'n';
    let gotopage_shortcut = cmd | 'p';

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

    // Page menu
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        let wind_ref = wind_ref.clone();
        menu_bar.add(
            "Page/New Page …",
            new_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                show_new_page_dialog(
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
            "Page/_Go to Page …",
            gotopage_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                if let Ok(w) = wind_ref.try_borrow() {
                    page_picker::show_page_picker(
                        app_state.clone(),
                        autosave_state.clone(),
                        active_editor.clone(),
                        statusbar.clone(),
                        &*w,
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
            "Page/Back",
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
            "Page/_Forward",
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
            "Page/Go to Frontpage",
            frontpage_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                load_page_helper(
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
        let label = "Page/_Go to Index";
        // No separator on macOS for this item,
        // as there's not going to be a Quit item below it.
        #[cfg(target_os = "macos")]
        let label = "Page/Go to Index";
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let statusbar = statusbar.clone();
        menu_bar.add(label, index_shortcut, menu::MenuFlag::Normal, move |_| {
            load_page_helper(
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
    menu_bar.add("Page/Quit", quit_shortcut, menu::MenuFlag::Normal, |_| {
        app::quit();
    });

    // Edit menu
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        menu_bar.add(
            "Edit/Cut",
            cut_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_cut(&active_editor, &is_structured);
            },
        );
    }

    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        menu_bar.add(
            "Edit/Copy",
            copy_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_copy(&active_editor, &is_structured);
            },
        );
    }

    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        menu_bar.add(
            "Edit/Paste",
            paste_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_paste(&active_editor, &is_structured);
            },
        );
    }

    // View menu (toggle Markdown editor)
    let view_label = "View/Markdown editor";
    {
        let app_state = app_state.clone();
        let autosave_state = autosave_state.clone();
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let statusbar = statusbar.clone();
        let wind_ref = wind_ref.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            view_label,
            Shortcut::None,
            menu::MenuFlag::Toggle,
            move |_| {
                let target = if *is_structured.borrow() {
                    EditorKind::Markdown
                } else {
                    EditorKind::Structured
                };
                let switched = switch_editor(
                    target,
                    &app_state,
                    &autosave_state,
                    &active_editor,
                    &is_structured,
                    &statusbar,
                    &wind_ref,
                    editor_x,
                    editor_y,
                    editor_w,
                    editor_h,
                );
                if let Some(mut item) = menu_handle.find_item(view_label) {
                    if !*is_structured.borrow() {
                        item.set();
                    } else {
                        item.clear();
                    }
                    if switched {
                        app::redraw();
                    }
                }
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
                register_paragraph_callback(&menu_handle, &active_editor, &is_structured);
            },
        );
    }

    if let Some(mut item) = menu_bar.find_item(view_label) {
        if !*is_structured.borrow() {
            item.set();
        } else {
            item.clear();
        }
    }

    // Format menu - paragraph styles
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_PARAGRAPH,
            paragraph_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.set_block_type(BlockType::Paragraph)
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_HEADING1,
            heading1_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.set_block_type(BlockType::Heading { level: 1 })
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_HEADING2,
            heading2_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.set_block_type(BlockType::Heading { level: 2 })
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_HEADING3,
            heading3_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.set_block_type(BlockType::Heading { level: 3 })
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_QUOTE,
            quote_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_quote()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_CODE_BLOCK,
            code_block_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_code_block()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_NUMBERED_LIST,
            ordered_list_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_ordered_list()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_LIST_ITEM,
            list_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_list()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_CHECKLIST_ITEM,
            checklist_shortcut,
            menu::MenuFlag::Radio,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_checklist()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }

    // Format menu - inline styles
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_BOLD,
            bold_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_bold()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_ITALIC,
            italic_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_italic()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_UNDERLINE,
            underline_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_underline()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_CODE,
            code_inline_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_code()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_HIGHLIGHT,
            highlight_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_highlight()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_INLINE_STRIKE,
            strike_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                let _ = with_structured_editor(&active_editor, &is_structured, true, |editor| {
                    editor.toggle_strikethrough()
                });
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        menu_bar.add(
            FORMAT_EDIT_LINK,
            edit_link_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_edit_link(&active_editor, &is_structured);
            },
        );
    }

    // Format menu - clear formatting
    {
        let active_editor = active_editor.clone();
        let is_structured = is_structured.clone();
        let menu_handle = menu_bar.clone();
        menu_bar.add(
            FORMAT_CLEAR,
            clear_shortcut,
            menu::MenuFlag::Normal,
            move |_| {
                perform_clear_formatting(&active_editor, &is_structured);
                update_format_menu_state(&menu_handle, &active_editor, &is_structured);
            },
        );
    }

    update_format_menu_state(menu_bar, &active_editor, &is_structured);
    register_paragraph_callback(menu_bar, &active_editor, &is_structured);
}

fn perform_cut(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    if let Some(Some(text)) = with_structured_editor(active_editor, is_structured, true, |editor| {
        editor.cut_selection()
    }) {
        if !text.is_empty() {
            app::copy(&text);
        }
        app::redraw();
        return;
    }

    if let Some(Some(text)) = with_markdown_editor(active_editor, is_structured, true, |editor| {
        editor.cut_selection()
    }) {
        if !text.is_empty() {
            app::copy(&text);
        }
        app::redraw();
    }
}

fn perform_copy(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    if let Some(Some(text)) =
        with_structured_editor(active_editor, is_structured, false, |editor| {
            editor.copy_selection()
        })
    {
        if !text.is_empty() {
            app::copy(&text);
        }
        return;
    }

    if let Some(Some(text)) = with_markdown_editor(active_editor, is_structured, false, |editor| {
        editor.copy_selection()
    }) {
        if !text.is_empty() {
            app::copy(&text);
        }
    }
}

fn perform_paste(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    if with_structured_editor(active_editor, is_structured, true, |editor| {
        editor.paste_from_clipboard();
    })
    .is_some()
    {
        return;
    }

    if with_markdown_editor(active_editor, is_structured, true, |editor| {
        editor.paste_from_clipboard();
    })
    .is_some()
    {
        return;
    }
}

fn perform_clear_formatting(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    if let Some(changed) = with_structured_editor(active_editor, is_structured, true, |editor| {
        editor.clear_formatting()
    }) {
        if changed {
            app::redraw();
        }
    }
}

fn perform_edit_link(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    let init_data = with_structured_editor_ref(active_editor, is_structured, |editor| {
        if editor.is_readonly() {
            return None;
        }

        let display = editor.0.display.borrow();
        let (init_target, init_text, mode_existing_link, selection_mode, link_pos) =
            if let Some((block_idx, inline_idx)) = display.hovered_link() {
                let doc = display.editor().document();
                let block = doc.blocks().get(block_idx);
                if let Some(block) = block {
                    if let Some(InlineContent::Link { link, content }) =
                        block.content.get(inline_idx)
                    {
                        let text = content
                            .iter()
                            .map(|c| c.to_plain_text())
                            .collect::<String>();
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
    let is_structured_save = Rc::clone(is_structured);
    let link_pos_for_save = link_pos;
    let remove_cb = if link_pos.is_some() {
        let active_editor_remove = Rc::clone(active_editor);
        let is_structured_remove = Rc::clone(is_structured);
        Some(move || {
            if let Some((block_idx, inline_idx)) = link_pos {
                let _ = with_structured_editor(
                    &active_editor_remove,
                    &is_structured_remove,
                    true,
                    |editor| {
                        let changed = {
                            let mut disp = editor.0.display.borrow_mut();
                            let editor_mut = disp.editor_mut();
                            editor_mut.remove_link_at(block_idx, inline_idx).is_ok()
                        };
                        if changed {
                            editor.0.notify_change();
                            editor.0.emit_paragraph_state();
                        }
                    },
                );
            }
        })
    } else {
        None
    };

    link_editor::show_link_editor(
        opts,
        move |dest: String, txt: String| {
            let _ =
                with_structured_editor(&active_editor_save, &is_structured_save, true, |editor| {
                    let changed = {
                        let mut disp = editor.0.display.borrow_mut();
                        let editor_mut = disp.editor_mut();

                        if let Some((block_idx, inline_idx)) = link_pos_for_save {
                            editor_mut
                                .edit_link_at(block_idx, inline_idx, &dest, &txt)
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
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
    require_writable: bool,
    mut f: F,
) -> Option<R>
where
    F: FnMut(&mut StructuredRichUI) -> R,
{
    if !*is_structured.borrow() {
        return None;
    }
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
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
    f: F,
) -> Option<R>
where
    F: FnOnce(&StructuredRichUI) -> R,
{
    if !*is_structured.borrow() {
        return None;
    }
    if let Ok(active_ptr) = active_editor.try_borrow() {
        let editor_rc = active_ptr.clone();
        drop(active_ptr);
        if let Ok(editor) = editor_rc.try_borrow() {
            if let Some(structured) = editor.as_any().downcast_ref::<StructuredRichUI>() {
                return Some(f(structured));
            }
        }
    }
    None
}

fn with_markdown_editor<F, R>(
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
    require_writable: bool,
    mut f: F,
) -> Option<R>
where
    F: FnMut(&mut MarkdownEditor) -> R,
{
    if *is_structured.borrow() {
        return None;
    }
    if let Ok(active_ptr) = active_editor.try_borrow() {
        let editor_rc = active_ptr.clone();
        drop(active_ptr);
        if let Ok(mut editor) = editor_rc.try_borrow_mut() {
            if require_writable && editor.is_readonly() {
                return None;
            }
            if let Some(markdown) = editor.as_any_mut().downcast_mut::<MarkdownEditor>() {
                return Some(f(markdown));
            }
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
    }
}

fn update_format_menu_state<M: MenuExt>(
    menu: &M,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    let structured_active = *is_structured.borrow();
    let mut readonly = true;
    let mut current_label: Option<&'static str> = None;

    if structured_active {
        if let Some((block, ro)) =
            with_structured_editor_ref(active_editor, is_structured, |editor| {
                (editor.current_block_type(), editor.is_readonly())
            })
        {
            readonly = ro;
            if let Some(block_type) = block {
                current_label = paragraph_label_for_block(&block_type);
            }
        }
    }

    for &label in PARAGRAPH_ITEMS {
        if let Some(mut item) = menu.find_item(label) {
            if structured_active && !readonly {
                item.activate();
            } else {
                item.deactivate();
            }
            item.clear();
        }
    }

    if structured_active {
        if let Some(label) = current_label {
            if let Some(mut item) = menu.find_item(label) {
                item.set();
            }
        }
    }

    for &label in INLINE_ITEMS {
        if let Some(mut item) = menu.find_item(label) {
            if structured_active && !readonly {
                item.activate();
            } else {
                item.deactivate();
            }
        }
    }

    if let Some(mut item) = menu.find_item(FORMAT_CLEAR) {
        if structured_active && !readonly {
            item.activate();
        } else {
            item.deactivate();
        }
    }
}

fn register_paragraph_callback<M: MenuExt + Clone + 'static>(
    menu: &M,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
) {
    let menu_rc = Rc::new(menu.clone());
    let active_editor_rc = active_editor.clone();
    let is_structured_rc = is_structured.clone();
    let _ = with_structured_editor(active_editor, is_structured, false, |editor| {
        let menu_for_cb = menu_rc.clone();
        let active_for_cb = active_editor_rc.clone();
        let structured_for_cb = is_structured_rc.clone();
        editor.on_paragraph_style_change(Box::new(move |_block_type| {
            println!("Paragraph style changed callback triggered");
            let menu_clone = menu_for_cb.clone();
            let active_clone = active_for_cb.clone();
            let structured_clone = structured_for_cb.clone();
            app::awake_callback(move || {
                update_format_menu_state(&*menu_clone, &active_clone, &structured_clone);
            });
        }));
    });

    let menu_for_init = menu_rc.clone();
    let active_for_init = active_editor_rc.clone();
    let structured_for_init = is_structured_rc.clone();
    app::awake_callback(move || {
        update_format_menu_state(&*menu_for_init, &active_for_init, &structured_for_init);
    });
}

fn instantiate_editor(
    kind: EditorKind,
    wind_ref: &Rc<RefCell<window::Window>>,
    editor_x: i32,
    editor_y: i32,
    editor_w: i32,
    editor_h: i32,
) -> Rc<RefCell<dyn PageUI>> {
    if let Ok(mut win) = wind_ref.try_borrow_mut() {
        let cur_w = win.w();
        let cur_h = win.h();
        let nx = editor_x;
        let bottom_status_h = 25;
        let nh = (cur_h - (editor_y + bottom_status_h)).max(1);
        let nw = (cur_w - (editor_x * 2)).max(1);

        win.begin();
        let editor: Rc<RefCell<dyn PageUI>> = match kind {
            EditorKind::Structured => Rc::new(RefCell::new(StructuredRichUI::new(
                nx, editor_y, nw, nh, true,
            ))),
            EditorKind::Markdown => {
                Rc::new(RefCell::new(MarkdownEditor::new(nx, editor_y, nw, nh)))
            }
        };
        editor
            .borrow_mut()
            .set_bg_color(enums::Color::from_rgb(255, 255, 245));
        editor.borrow().set_resizable(&mut *win);
        win.end();
        editor
    } else {
        let editor: Rc<RefCell<dyn PageUI>> = match kind {
            EditorKind::Structured => Rc::new(RefCell::new(StructuredRichUI::new(
                editor_x, editor_y, editor_w, editor_h, true,
            ))),
            EditorKind::Markdown => Rc::new(RefCell::new(MarkdownEditor::new(
                editor_x, editor_y, editor_w, editor_h,
            ))),
        };
        editor
            .borrow_mut()
            .set_bg_color(enums::Color::from_rgb(255, 255, 245));
        editor
    }
}

#[allow(clippy::too_many_arguments)]
fn switch_editor(
    target: EditorKind,
    app_state: &Rc<RefCell<AppState>>,
    autosave_state: &Rc<RefCell<AutoSaveState>>,
    active_editor: &Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    is_structured: &Rc<RefCell<bool>>,
    statusbar: &Rc<RefCell<StatusBar>>,
    wind_ref: &Rc<RefCell<window::Window>>,
    editor_x: i32,
    editor_y: i32,
    editor_w: i32,
    editor_h: i32,
) -> bool {
    let want_structured = matches!(target, EditorKind::Structured);
    if want_structured == *is_structured.borrow() {
        return false;
    }

    let scroll_pos = if let Ok(active_ptr) = active_editor.try_borrow() {
        let editor_rc = active_ptr.clone();
        drop(active_ptr);
        if let Ok(editor) = editor_rc.try_borrow() {
            editor.scroll_pos()
        } else {
            0
        }
    } else {
        0
    };

    // Hide the old editor before creating the new one
    if let Ok(active_ptr) = active_editor.try_borrow() {
        let editor_rc = active_ptr.clone();
        drop(active_ptr);
        if let Ok(mut editor) = editor_rc.try_borrow_mut() {
            editor.hide();
        }
    }

    let new_editor = instantiate_editor(target, wind_ref, editor_x, editor_y, editor_w, editor_h);

    {
        if let Ok(mut active_mut) = active_editor.try_borrow_mut() {
            *active_mut = new_editor.clone();
        }
    }
    *is_structured.borrow_mut() = want_structured;

    wire_editor_callbacks(active_editor, autosave_state, app_state, statusbar);

    if let Ok(state) = app_state.try_borrow() {
        let current_page = state.current_page.clone();
        drop(state);
        load_page_helper(
            &current_page,
            app_state,
            autosave_state,
            active_editor,
            statusbar,
            Some(scroll_pos),
        );
    }

    true
}

fn show_new_page_dialog(
    app_state: Rc<RefCell<AppState>>,
    autosave_state: Rc<RefCell<AutoSaveState>>,
    active_editor: Rc<RefCell<Rc<RefCell<dyn PageUI>>>>,
    statusbar: Rc<RefCell<StatusBar>>,
    wind_ref: Rc<RefCell<window::Window>>,
) {
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

    let mut win = window::Window::new(pos_x.max(0), pos_y.max(0), width, height, Some("New Page"));
    win.make_modal(true);
    win.begin();

    let mut label = frame::Frame::new(10, 10, width - 20, 24, Some("Enter new page name:"));
    label.set_align(enums::Align::Inside | enums::Align::Left);

    let mut input = input::Input::new(10, 40, width - 20, 28, None);

    let mut cancel_btn = button::Button::new(width - 180, height - 40, 80, 30, Some("Cancel"));
    let mut create_btn = button::ReturnButton::new(width - 90, height - 40, 80, 30, Some("Create"));
    create_btn.deactivate();

    {
        let mut create_btn_clone = create_btn.clone();
        input.set_trigger(enums::CallbackTrigger::Changed);
        input.set_callback(move |inp| {
            if inp.value().trim().is_empty() {
                create_btn_clone.deactivate();
            } else {
                create_btn_clone.activate();
            }
        });
    }

    let input_for_create = input.clone();
    {
        let mut win_for_create = win.clone();
        create_btn.set_callback(move |_| {
            let name = input_for_create.value().trim().to_string();
            if name.is_empty() {
                return;
            }

            load_page_helper(
                &name,
                &app_state,
                &autosave_state,
                &active_editor,
                &statusbar,
                None,
            );
            win_for_create.hide();
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
