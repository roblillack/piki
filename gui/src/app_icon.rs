//! Application icon wiring.
//!
//! Sets the window / taskbar icon on Linux and Windows, and the Dock icon on
//! macOS. macOS normally reads the Dock icon from the `.app` bundle, so doing
//! it here means the icon also shows up when the unbundled `piki-gui` binary is
//! run directly.

use fltk::window::Window;

#[cfg(not(target_os = "macos"))]
const ICON_SVG: &str = include_str!("../assets/icon.svg");

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
    use objc2::MainThreadMarker;
    use objc2::rc::autoreleasepool;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    autoreleasepool(|_| {
        let app = NSApplication::sharedApplication(mtm);
        let Some(main_menu) = app.mainMenu() else {
            return;
        };
        if main_menu.numberOfItems() <= 0 {
            return;
        }
        let Some(app_item) = main_menu.itemAtIndex(0) else {
            return;
        };
        let Some(app_menu) = app_item.submenu() else {
            return;
        };

        app_menu.setTitle(&NSString::from_str(name));

        // Rewrite "About piki-gui", "Hide piki-gui", "Quit piki-gui", etc.
        for i in 0..app_menu.numberOfItems() {
            let Some(item) = app_menu.itemAtIndex(i) else {
                continue;
            };
            let current = item.title().to_string();
            if current.contains("piki-gui") {
                let replaced = current.replace("piki-gui", name);
                item.setTitle(&NSString::from_str(&replaced));
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
    use objc2::rc::autoreleasepool;
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const ICON_PNG: &[u8] = include_bytes!("../assets/icon-512.png");

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    autoreleasepool(|_| {
        let data = NSData::with_bytes(ICON_PNG);
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            return;
        };
        let app = NSApplication::sharedApplication(mtm);
        // SAFETY: setting the Dock icon image is a standard AppKit operation.
        unsafe {
            app.setApplicationIconImage(Some(&image));
        }
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
    use objc2::rc::autoreleasepool;
    use objc2::runtime::AnyObject;
    use objc2::{AnyThread, MainThreadMarker, Message, msg_send};
    use objc2_app_kit::{
        NSApplication, NSColor, NSFont, NSImage, NSMutableParagraphStyle, NSTextAlignment,
    };
    use objc2_foundation::{
        NSAttributedString, NSBundle, NSData, NSDictionary, NSMutableAttributedString,
        NSMutableDictionary, NSString, NSURL,
    };

    const ICON_PNG: &[u8] = include_bytes!("../assets/icon-512.png");
    const APP_NAME: &str = "Piki";
    const VERSION: &str = env!("CARGO_PKG_VERSION");
    const DESCRIPTION: &str = "A personal wiki system for your Markdown files";
    const COPYRIGHT: &str = "© 2025–2026 Robert Lillack";
    const HOMEPAGE: &str = "https://github.com/roblillack/piki";

    // Store a value under a string key in a heterogeneous options/attributes
    // dictionary. The about-panel option keys and NSAttributedString attribute
    // keys are stable string constants; using their literal values avoids having
    // to link the AppKit symbols.
    fn put<T: Message>(dict: &NSMutableDictionary<NSString, AnyObject>, key: &str, value: &T) {
        let key = NSString::from_str(key);
        // SAFETY: the dictionary holds arbitrary objects keyed by NSString,
        // matching `-setObject:forKey:`.
        unsafe {
            let _: () = msg_send![dict, setObject: value, forKey: &*key];
        }
    }

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    autoreleasepool(|_| {
        let app = NSApplication::sharedApplication(mtm);

        let options = NSMutableDictionary::<NSString, AnyObject>::new();
        put(&options, "ApplicationName", &*NSString::from_str(APP_NAME));
        put(
            &options,
            "ApplicationVersion",
            &*NSString::from_str(VERSION),
        );

        // Application icon, decoded from the bundled PNG.
        let data = NSData::with_bytes(ICON_PNG);
        if let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) {
            put(&options, "ApplicationIcon", &*image);
        }

        // Credits area: description, copyright and a clickable homepage link,
        // centered to match the rest of the panel.
        let credits = NSMutableAttributedString::new();

        let para = NSMutableParagraphStyle::new();
        para.setAlignment(NSTextAlignment::Center);
        let font = NSFont::systemFontOfSize(11.0);
        let color = NSColor::secondaryLabelColor();

        let append = |text: &str, attrs: &NSDictionary<NSString, AnyObject>| {
            // SAFETY: `attrs` maps attribute-name strings to valid attribute
            // values, as required by `-initWithString:attributes:`.
            let astr = unsafe {
                NSAttributedString::initWithString_attributes(
                    NSAttributedString::alloc(),
                    &NSString::from_str(text),
                    Some(attrs),
                )
            };
            credits.appendAttributedString(&astr);
        };

        let base_attrs = NSMutableDictionary::<NSString, AnyObject>::new();
        put(&base_attrs, "NSFont", &*font);
        put(&base_attrs, "NSColor", &*color);
        put(&base_attrs, "NSParagraphStyle", &*para);

        append(DESCRIPTION, &base_attrs);

        // Only add a copyright line when a bundled Info.plist hasn't already
        // provided one (the standard panel renders NSHumanReadableCopyright in
        // its own slot), so the bundled app doesn't show it twice.
        let has_bundle_copyright = NSBundle::mainBundle()
            .objectForInfoDictionaryKey(&NSString::from_str("NSHumanReadableCopyright"))
            .is_some();
        if !has_bundle_copyright {
            append("\n\n", &base_attrs);
            append(COPYRIGHT, &base_attrs);
        }

        append("\n\n", &base_attrs);
        let link_attrs = NSMutableDictionary::<NSString, AnyObject>::new();
        put(&link_attrs, "NSFont", &*font);
        put(&link_attrs, "NSParagraphStyle", &*para);
        if let Some(url) = NSURL::URLWithString(&NSString::from_str(HOMEPAGE)) {
            put(&link_attrs, "NSLink", &*url);
        }
        append(HOMEPAGE, &link_attrs);

        put(&options, "Credits", &*credits);

        // SAFETY: `options` only contains the documented about-panel keys.
        unsafe {
            app.orderFrontStandardAboutPanelWithOptions(&options);
        }
    });
}
