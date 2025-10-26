use crate::richtext::structured_document::BlockType;
use fltk::{
    enums::Shortcut,
    menu::{MenuButton, MenuFlag},
    prelude::{MenuExt, WidgetExt},
};

/// Actions to be wired to context menu entries.
pub struct MenuActions {
    pub has_selection: bool,
    /// Current block type at cursor, for radio selection state
    pub current_block: BlockType,

    // Block styles
    pub set_paragraph: Box<dyn FnMut()>,
    pub set_heading1: Box<dyn FnMut()>,
    pub set_heading2: Box<dyn FnMut()>,
    pub set_heading3: Box<dyn FnMut()>,
    pub toggle_quote: Box<dyn FnMut()>,
    pub toggle_code_block: Box<dyn FnMut()>,
    pub toggle_list: Box<dyn FnMut()>,
    pub toggle_checklist: Box<dyn FnMut()>,
    pub toggle_ordered_list: Box<dyn FnMut()>,

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

    // Numbered list (Cmd/Ctrl + Shift + 7)
    #[cfg(target_os = "macos")]
    let ordered_list_shortcut = Shortcut::Command | Shortcut::Shift | '7';
    #[cfg(not(target_os = "macos"))]
    let ordered_list_shortcut = Shortcut::Ctrl | Shortcut::Shift | '7';

    // Code paragraph (Cmd/Ctrl + Shift + 6)
    #[cfg(target_os = "macos")]
    let code_block_shortcut = Shortcut::Command | Shortcut::Shift | '6';
    #[cfg(not(target_os = "macos"))]
    let code_block_shortcut = Shortcut::Ctrl | Shortcut::Shift | '6';

    // Quote (Cmd/Ctrl + Shift + 9)
    #[cfg(target_os = "macos")]
    let quote_shortcut = Shortcut::Command | Shortcut::Shift | '5';
    #[cfg(not(target_os = "macos"))]
    let quote_shortcut = Shortcut::Ctrl | Shortcut::Shift | '5';

    #[cfg(target_os = "macos")]
    let checklist_shortcut = Shortcut::Command | Shortcut::Shift | '9';
    #[cfg(not(target_os = "macos"))]
    let checklist_shortcut = Shortcut::Ctrl | Shortcut::Shift | '9';

    // Paragraph style items as a radio group
    menu.add(
        "Paragraph Style/Paragraph\t",
        para_shortcut,
        MenuFlag::Radio,
        move |_| (actions.set_paragraph)(),
    );
    menu.add(
        "Paragraph Style/Heading 1\t",
        h1_shortcut,
        MenuFlag::Radio,
        move |_| (actions.set_heading1)(),
    );
    menu.add(
        "Paragraph Style/Heading 2\t",
        h2_shortcut,
        MenuFlag::Radio,
        move |_| (actions.set_heading2)(),
    );
    menu.add(
        "Paragraph Style/Heading 3\t",
        h3_shortcut,
        MenuFlag::Radio,
        move |_| (actions.set_heading3)(),
    );
    menu.add(
        "Paragraph Style/Quote\t",
        quote_shortcut,
        MenuFlag::Radio,
        move |_| (actions.toggle_quote)(),
    );
    menu.add(
        "Paragraph Style/Code\t",
        code_block_shortcut,
        MenuFlag::Radio,
        move |_| (actions.toggle_code_block)(),
    );
    menu.add(
        "Paragraph Style/Numbered List\t",
        ordered_list_shortcut,
        MenuFlag::Radio,
        move |_| (actions.toggle_ordered_list)(),
    );
    menu.add(
        "Paragraph Style/List Item\t",
        list_shortcut,
        MenuFlag::Radio,
        move |_| (actions.toggle_list)(),
    );
    menu.add(
        "Paragraph Style/Checklist Item\t",
        checklist_shortcut,
        MenuFlag::Radio,
        move |_| (actions.toggle_checklist)(),
    );

    // Reflect current block selection in the radio group
    let labels = [
        "Paragraph Style/Paragraph\t",
        "Paragraph Style/Heading 1\t",
        "Paragraph Style/Heading 2\t",
        "Paragraph Style/Heading 3\t",
        "Paragraph Style/Quote\t",
        "Paragraph Style/Code\t",
        "Paragraph Style/Numbered List\t",
        "Paragraph Style/List Item\t",
        "Paragraph Style/Checklist Item\t",
    ];
    // Ensure radio flag is set on all items and clear Value by default
    for &label in &labels {
        let idx = menu.find_index(label);
        if idx >= 0 {
            menu.set_mode(idx, MenuFlag::Radio);
        }
    }
    // Set the selected item based on current block
    if let Some(lbl) = match actions.current_block {
        BlockType::Paragraph => Some("Paragraph Style/Paragraph\t"),
        BlockType::Heading { level } => match level {
            1 => Some("Paragraph Style/Heading 1\t"),
            2 => Some("Paragraph Style/Heading 2\t"),
            3 => Some("Paragraph Style/Heading 3\t"),
            _ => None,
        },
        BlockType::CodeBlock { .. } => Some("Paragraph Style/Code\t"),
        BlockType::BlockQuote => Some("Paragraph Style/Quote\t"),
        BlockType::ListItem {
            ordered, checkbox, ..
        } => Some(if ordered {
            "Paragraph Style/Numbered List\t"
        } else if checkbox.is_some() {
            "Paragraph Style/Checklist Item\t"
        } else {
            "Paragraph Style/List Item\t"
        }),
    }
        && let Some(mut item) = menu.find_item(lbl) {
            item.set();
        }

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

    menu.add(
        "Toggle Bold\t",
        bold_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_bold)(),
    );
    menu.add(
        "Toggle Italic\t",
        italic_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_italic)(),
    );
    menu.add(
        "Toggle Underline\t",
        underline_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_underline)(),
    );
    menu.add(
        "Toggle Code\t",
        code_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_code)(),
    );
    menu.add(
        "Toggle Highlight\t",
        highlight_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_highlight)(),
    );
    menu.add(
        "Toggle Strikethrough\t",
        strike_shortcut,
        MenuFlag::Normal,
        move |_| (actions.toggle_strike)(),
    );
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

    menu.add(
        "_Clear Formatting\t",
        clear_shortcut,
        MenuFlag::Normal,
        move |_| (actions.clear_formatting)(),
    );

    // Clipboard
    #[cfg(target_os = "macos")]
    let cut_shortcut = Shortcut::Command | 'x';
    #[cfg(not(target_os = "macos"))]
    let cut_shortcut = Shortcut::Ctrl | 'x';
    menu.add("Cut\t", cut_shortcut, MenuFlag::Normal, move |_| {
        (actions.cut)()
    });

    #[cfg(target_os = "macos")]
    let copy_shortcut = Shortcut::Command | 'c';
    #[cfg(not(target_os = "macos"))]
    let copy_shortcut = Shortcut::Ctrl | 'c';
    menu.add("Copy\t", copy_shortcut, MenuFlag::Normal, move |_| {
        (actions.copy)()
    });

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
