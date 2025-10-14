use fltk::{
    enums::Shortcut,
    menu::{MenuButton, MenuFlag},
    prelude::{MenuExt, WidgetExt},
};

/// Actions to be wired to context menu entries.
pub struct MenuActions {
    pub has_selection: bool,

    // Block styles
    pub set_paragraph: Box<dyn FnMut()>,
    pub set_heading1: Box<dyn FnMut()>,
    pub set_heading2: Box<dyn FnMut()>,
    pub set_heading3: Box<dyn FnMut()>,
    pub toggle_list: Box<dyn FnMut()>,

    // Inline styles
    pub toggle_bold: Box<dyn FnMut()>,
    pub toggle_italic: Box<dyn FnMut()>,
    pub toggle_code: Box<dyn FnMut()>,
    pub toggle_strike: Box<dyn FnMut()>,
    pub toggle_underline: Box<dyn FnMut()>,
    pub toggle_highlight: Box<dyn FnMut()>,
    pub clear_formatting: Box<dyn FnMut()>,

    // Clipboard
    pub cut: Box<dyn FnMut()>,
    pub copy: Box<dyn FnMut()>,
    pub paste: Box<dyn FnMut()>,

    // Links
    pub edit_link: Box<dyn FnMut()>,
}

/// Show a context menu at the given screen position (x, y) with standard entries.
pub fn show_context_menu(x: i32, y: i32, mut actions: MenuActions) {
    let mut menu = MenuButton::default();
    menu.set_pos(x, y);

    // Paragraph Style submenu with accelerators
    #[cfg(target_os = "macos")]
    let para_shortcut = Shortcut::Command | Shortcut::Alt | '0';
    #[cfg(not(target_os = "macos"))]
    let para_shortcut = Shortcut::Ctrl | Shortcut::Alt | '0';

    #[cfg(target_os = "macos")]
    let h1_shortcut = Shortcut::Command | Shortcut::Alt | '1';
    #[cfg(not(target_os = "macos"))]
    let h1_shortcut = Shortcut::Ctrl | Shortcut::Alt | '1';

    #[cfg(target_os = "macos")]
    let h2_shortcut = Shortcut::Command | Shortcut::Alt | '2';
    #[cfg(not(target_os = "macos"))]
    let h2_shortcut = Shortcut::Ctrl | Shortcut::Alt | '2';

    #[cfg(target_os = "macos")]
    let h3_shortcut = Shortcut::Command | Shortcut::Alt | '3';
    #[cfg(not(target_os = "macos"))]
    let h3_shortcut = Shortcut::Ctrl | Shortcut::Alt | '3';

    #[cfg(target_os = "macos")]
    let list_shortcut = Shortcut::Command | Shortcut::Shift | '8';
    #[cfg(not(target_os = "macos"))]
    let list_shortcut = Shortcut::Ctrl | Shortcut::Shift | '8';

    menu.add(
        "Paragraph Style/Paragraph\t",
        para_shortcut,
        MenuFlag::Normal,
        move |_| (actions.set_paragraph)(),
    );
    menu.add(
        "Paragraph Style/Heading 1\t",
        h1_shortcut,
        MenuFlag::Normal,
        move |_| (actions.set_heading1)(),
    );
    menu.add(
        "Paragraph Style/Heading 2\t",
        h2_shortcut,
        MenuFlag::Normal,
        move |_| (actions.set_heading2)(),
    );
    menu.add(
        "Paragraph Style/Heading 3\t",
        h3_shortcut,
        MenuFlag::Normal,
        move |_| (actions.set_heading3)(),
    );
    menu.add(
        "Paragraph Style/List Item\t",
        list_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_list)(),
    );

    // Inline style accelerators
    #[cfg(target_os = "macos")]
    let bold_shortcut = Shortcut::Command | 'b';
    #[cfg(not(target_os = "macos"))]
    let bold_shortcut = Shortcut::Ctrl | 'b';

    #[cfg(target_os = "macos")]
    let italic_shortcut = Shortcut::Command | 'i';
    #[cfg(not(target_os = "macos"))]
    let italic_shortcut = Shortcut::Ctrl | 'i';

    #[cfg(target_os = "macos")]
    let code_shortcut = Shortcut::Command | Shortcut::Shift | 'c';
    #[cfg(not(target_os = "macos"))]
    let code_shortcut = Shortcut::Ctrl | Shortcut::Shift | 'c';

    #[cfg(target_os = "macos")]
    let strike_shortcut = Shortcut::Command | Shortcut::Shift | 'x';
    #[cfg(not(target_os = "macos"))]
    let strike_shortcut = Shortcut::Ctrl | Shortcut::Shift | 'x';

    #[cfg(target_os = "macos")]
    let underline_shortcut = Shortcut::Command | 'u';
    #[cfg(not(target_os = "macos"))]
    let underline_shortcut = Shortcut::Ctrl | 'u';

    #[cfg(target_os = "macos")]
    let highlight_shortcut = Shortcut::Command | Shortcut::Shift | 'h';
    #[cfg(not(target_os = "macos"))]
    let highlight_shortcut = Shortcut::Ctrl | Shortcut::Shift | 'h';

    #[cfg(target_os = "macos")]
    let clear_shortcut = Shortcut::Command | '\\';
    #[cfg(not(target_os = "macos"))]
    let clear_shortcut = Shortcut::Ctrl | '\\';

    menu.add("Toggle Bold\t", bold_shortcut, MenuFlag::Normal, move |_| (actions.toggle_bold)());
    menu.add(
        "Toggle Italic\t",
        italic_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_italic)(),
    );
    menu.add(
        "Toggle Code\t",
        code_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_code)(),
    );
    menu.add(
        "Toggle Strikethrough\t",
        strike_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_strike)(),
    );
    menu.add(
        "Toggle Underline\t",
        underline_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_underline)(),
    );
    menu.add(
        "Toggle Highlight\t",
        highlight_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_highlight)(),
    );

    menu.add("_", Shortcut::None, MenuFlag::MenuDivider, |_| {});

    menu.add(
        "Clear Formatting\t",
        clear_shortcut,
        MenuFlag::Normal,
        move |_| (actions.clear_formatting)(),
    );

    menu.add("_", Shortcut::None, MenuFlag::MenuDivider, |_| {});

    // Edit Link
    #[cfg(target_os = "macos")]
    let edit_link_shortcut = Shortcut::Command | 'k';
    #[cfg(not(target_os = "macos"))]
    let edit_link_shortcut = Shortcut::Ctrl | 'k';
    menu.add(
        "Edit Link…\t",
        edit_link_shortcut,
        MenuFlag::Normal,
        move |_| (actions.edit_link)(),
    );

    menu.add("_", Shortcut::None, MenuFlag::MenuDivider, |_| {});

    // Clipboard
    #[cfg(target_os = "macos")]
    let cut_shortcut = Shortcut::Command | 'x';
    #[cfg(not(target_os = "macos"))]
    let cut_shortcut = Shortcut::Ctrl | 'x';
    menu.add("Cut\t", cut_shortcut, MenuFlag::Normal, move |_| (actions.cut)());

    #[cfg(target_os = "macos")]
    let copy_shortcut = Shortcut::Command | 'c';
    #[cfg(not(target_os = "macos"))]
    let copy_shortcut = Shortcut::Ctrl | 'c';
    menu.add("Copy\t", copy_shortcut, MenuFlag::Normal, move |_| (actions.copy)());

    #[cfg(target_os = "macos")]
    let paste_shortcut = Shortcut::Command | 'v';
    #[cfg(not(target_os = "macos"))]
    let paste_shortcut = Shortcut::Ctrl | 'v';
    menu.add(
        "Paste\t",
        paste_shortcut,
        MenuFlag::Normal,
        move |_m: &mut MenuButton| (actions.paste)(),
    );

    // Disable cut/copy if no selection
    if !actions.has_selection {
        for label in ["Cut\t", "Copy\t"] {
            let idx = menu.find_index(label);
            if idx >= 0 {
                menu.set_mode(idx, MenuFlag::Inactive);
            }
        }
    }

    menu.popup();
}
