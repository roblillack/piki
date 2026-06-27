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
