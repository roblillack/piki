//! Application icon wiring.
//!
//! Sets the window / taskbar icon on Linux and Windows, and the Dock icon on
//! macOS. macOS normally reads the Dock icon from the `.app` bundle, so doing
//! it here means the icon also shows up when the unbundled `piki-gui` binary is
//! run directly.

use fltk::window::Window;

#[cfg(not(target_os = "macos"))]
const ICON_SVG: &str = include_str!("../../assets/icon.svg");

/// Set the window icon (used for the title bar and taskbar on Linux/Windows).
#[cfg(not(target_os = "macos"))]
pub fn set_window_icon(wind: &mut Window) {
    use fltk::{image::SvgImage, prelude::*};
    if let Ok(mut icon) = SvgImage::from_data(ICON_SVG) {
        icon.scale(128, 128, true, true);
        wind.set_icon(Some(icon));
    }
}

/// No-op on macOS: FLTK would otherwise draw the window icon as a title-bar
/// proxy ("document drag") icon, which we don't want. The Dock icon is set
/// separately via [`set_macos_dock_icon`].
#[cfg(target_os = "macos")]
pub fn set_window_icon(_wind: &mut Window) {}

/// Rename the macOS menu-bar application menu.
///
/// For an unbundled binary, macOS shows the executable name (`piki-gui`) as the
/// bold application-menu title and embeds it in the About/Hide/Quit items.
/// AppKit caches that from the executable path at launch, so the only reliable
/// runtime fix is to edit the menu FLTK already built: this retitles the
/// application menu to `name` and rewrites any item label that contains the old
/// name. Must be called *after* the system menu bar is created. Inside
/// `Piki.app` `CFBundleName` already handles this, so it is simply a no-op
/// there (the menu never contains `piki-gui`).
#[cfg(target_os = "macos")]
pub fn set_macos_app_name(name: &str) {
    use objc::rc::autoreleasepool;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::{CStr, CString};
    use std::os::raw::c_char;

    let Ok(c_name) = CString::new(name) else {
        return;
    };
    autoreleasepool(|| unsafe {
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let main_menu: *mut Object = msg_send![app, mainMenu];
        if main_menu.is_null() {
            return;
        }
        let count: i64 = msg_send![main_menu, numberOfItems];
        if count <= 0 {
            return;
        }
        let app_item: *mut Object = msg_send![main_menu, itemAtIndex: 0i64];
        if app_item.is_null() {
            return;
        }
        let app_menu: *mut Object = msg_send![app_item, submenu];
        if app_menu.is_null() {
            return;
        }

        let ns_name: *mut Object =
            msg_send![class!(NSString), stringWithUTF8String: c_name.as_ptr()];
        if !ns_name.is_null() {
            let _: () = msg_send![app_menu, setTitle: ns_name];
        }

        // Rewrite "About piki-gui", "Hide piki-gui", "Quit piki-gui", etc.
        let item_count: i64 = msg_send![app_menu, numberOfItems];
        for i in 0..item_count {
            let item: *mut Object = msg_send![app_menu, itemAtIndex: i];
            if item.is_null() {
                continue;
            }
            let title: *mut Object = msg_send![item, title];
            if title.is_null() {
                continue;
            }
            let utf8: *const c_char = msg_send![title, UTF8String];
            if utf8.is_null() {
                continue;
            }
            let current = CStr::from_ptr(utf8).to_string_lossy();
            if current.contains("piki-gui") {
                let replaced = current.replace("piki-gui", name);
                if let Ok(c_new) = CString::new(replaced) {
                    let new_title: *mut Object =
                        msg_send![class!(NSString), stringWithUTF8String: c_new.as_ptr()];
                    if !new_title.is_null() {
                        let _: () = msg_send![item, setTitle: new_title];
                    }
                }
            }
        }
    });
}

/// No-op on non-macOS platforms.
#[cfg(not(target_os = "macos"))]
pub fn set_macos_app_name(_name: &str) {}

/// Set the macOS Dock icon at runtime via `NSApplication`.
#[cfg(target_os = "macos")]
pub fn set_macos_dock_icon() {
    use objc::rc::autoreleasepool;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::os::raw::c_void;

    const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-512.png");

    autoreleasepool(|| unsafe {
        let data: *mut Object = msg_send![class!(NSData),
            dataWithBytes: ICON_PNG.as_ptr() as *const c_void
            length: ICON_PNG.len()];
        if data.is_null() {
            return;
        }
        let image: *mut Object = msg_send![class!(NSImage), alloc];
        let image: *mut Object = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }
        let _: () = msg_send![app, setApplicationIconImage: image];
    });
}

/// No-op on non-macOS platforms; the window icon set above is sufficient there.
#[cfg(not(target_os = "macos"))]
pub fn set_macos_dock_icon() {}

/// Install a custom handler for the "About Piki" application-menu item on macOS.
///
/// FLTK's default about box shows the raw executable name and a stock "GUI with
/// FLTK …" line with a generic icon. This replaces it with the standard macOS
/// about panel, populated with the real application name, version, icon, a short
/// description and a clickable link to the homepage. Safe to call before or
/// after the system menu bar is created.
#[cfg(target_os = "macos")]
pub fn set_macos_about() {
    fltk::menu::mac_set_about(show_about_panel);
}

/// No-op on non-macOS platforms (the system menu / about panel is macOS-only).
#[cfg(not(target_os = "macos"))]
pub fn set_macos_about() {}

/// Open the standard macOS about panel with Piki's metadata.
#[cfg(target_os = "macos")]
fn show_about_panel() {
    use objc::rc::autoreleasepool;
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CString;
    use std::os::raw::c_void;

    const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-512.png");
    const APP_NAME: &str = "Piki";
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const DESCRIPTION: &str = "A personal wiki system for your Markdown files";
    const COPYRIGHT: &str = "© 2025–2026 Robert Lillack";
    const HOMEPAGE: &str = "https://github.com/roblillack/piki";

    autoreleasepool(|| unsafe {
        // The about-panel option keys and NSAttributedString attribute keys are
        // stable string constants; using their literal values avoids having to
        // link the AppKit symbols.
        let nsstr = |s: &str| -> *mut Object {
            match CString::new(s) {
                Ok(c) => msg_send![class!(NSString), stringWithUTF8String: c.as_ptr()],
                Err(_) => std::ptr::null_mut(),
            }
        };

        let app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        if app.is_null() {
            return;
        }

        let options: *mut Object = msg_send![class!(NSMutableDictionary), dictionary];
        let set_opt = |key: &str, val: *mut Object| {
            if !val.is_null() {
                let _: () = msg_send![options, setObject: val forKey: nsstr(key)];
            }
        };

        set_opt("ApplicationName", nsstr(APP_NAME));
        set_opt("ApplicationVersion", nsstr(VERSION));

        // Application icon, decoded from the bundled PNG.
        let data: *mut Object = msg_send![class!(NSData),
            dataWithBytes: ICON_PNG.as_ptr() as *const c_void
            length: ICON_PNG.len()];
        if !data.is_null() {
            let image: *mut Object = msg_send![class!(NSImage), alloc];
            let image: *mut Object = msg_send![image, initWithData: data];
            set_opt("ApplicationIcon", image);
        }

        // Credits area: description, copyright and a clickable homepage link,
        // centered to match the rest of the panel.
        let credits: *mut Object = msg_send![class!(NSMutableAttributedString), alloc];
        let credits: *mut Object = msg_send![credits, init];

        let para: *mut Object = msg_send![class!(NSMutableParagraphStyle), alloc];
        let para: *mut Object = msg_send![para, init];
        let _: () = msg_send![para, setAlignment: 1i64]; // NSTextAlignmentCenter

        let font: *mut Object = msg_send![class!(NSFont), systemFontOfSize: 11.0f64];
        let color: *mut Object = msg_send![class!(NSColor), secondaryLabelColor];

        let append = |text: &str, attrs: *mut Object| {
            let astr: *mut Object = msg_send![class!(NSAttributedString), alloc];
            let astr: *mut Object = msg_send![astr, initWithString: nsstr(text) attributes: attrs];
            let _: () = msg_send![credits, appendAttributedString: astr];
        };

        let base_attrs: *mut Object = msg_send![class!(NSMutableDictionary), dictionary];
        let _: () = msg_send![base_attrs, setObject: font forKey: nsstr("NSFont")];
        let _: () = msg_send![base_attrs, setObject: color forKey: nsstr("NSColor")];
        let _: () = msg_send![base_attrs, setObject: para forKey: nsstr("NSParagraphStyle")];

        append(DESCRIPTION, base_attrs);

        // Only add a copyright line when a bundled Info.plist hasn't already
        // provided one (the standard panel renders NSHumanReadableCopyright in
        // its own slot), so the bundled app doesn't show it twice.
        let bundle: *mut Object = msg_send![class!(NSBundle), mainBundle];
        let bundle_copyright: *mut Object = if bundle.is_null() {
            std::ptr::null_mut()
        } else {
            msg_send![bundle, objectForInfoDictionaryKey: nsstr("NSHumanReadableCopyright")]
        };
        if bundle_copyright.is_null() {
            append("\n\n", base_attrs);
            append(COPYRIGHT, base_attrs);
        }

        append("\n\n", base_attrs);
        let link_attrs: *mut Object = msg_send![class!(NSMutableDictionary), dictionary];
        let _: () = msg_send![link_attrs, setObject: font forKey: nsstr("NSFont")];
        let _: () = msg_send![link_attrs, setObject: para forKey: nsstr("NSParagraphStyle")];
        let url: *mut Object = msg_send![class!(NSURL), URLWithString: nsstr(HOMEPAGE)];
        if !url.is_null() {
            let _: () = msg_send![link_attrs, setObject: url forKey: nsstr("NSLink")];
        }
        append(HOMEPAGE, link_attrs);

        set_opt("Credits", credits);

        let _: () = msg_send![app, orderFrontStandardAboutPanelWithOptions: options];
    });
}
