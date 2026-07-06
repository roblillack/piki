//! Handling of incoming `piki://` URLs.
//!
//! Piki registers the `piki` URL scheme (see `assets/macos/Info.plist`), so a
//! section link copied with Cmd-Shift-K is a real, OS-recognized URL that opens
//! Piki — even from another app — at the right note and heading.
//!
//! On macOS, LaunchServices delivers an opened URL to the running (or
//! just-launched) instance via the `kInternetEventClass`/`kAEGetURL` Apple
//! Event rather than on the command line, so we install a handler for it with
//! `NSAppleEventManager`. [`set_open_url_handler`] registers the app-level
//! closure that actually navigates; [`register`] wires up the OS handler. On
//! other platforms both are no-ops for now (the scheme is macOS-registered).

#[cfg(target_os = "macos")]
mod imp {
    use std::cell::RefCell;

    use objc2::rc::Retained;
    use objc2::runtime::{NSObject, NSObjectProtocol};
    use objc2::{AnyThread, MainThreadMarker, define_class, msg_send, sel};
    use objc2_foundation::{NSAppleEventDescriptor, NSAppleEventManager};

    thread_local! {
        /// App-level navigation closure, invoked with each opened `piki://` URL.
        static URL_HANDLER: RefCell<Option<Box<dyn FnMut(String)>>> =
            const { RefCell::new(None) };
        /// Keeps the Objective-C handler object alive: `NSAppleEventManager` does
        /// not retain its handler, so it must outlive `register`.
        static HANDLER: RefCell<Option<Retained<PikiUrlHandler>>> = const { RefCell::new(None) };
    }

    pub fn set_open_url_handler<F: FnMut(String) + 'static>(handler: F) {
        URL_HANDLER.with(|h| *h.borrow_mut() = Some(Box::new(handler)));
    }

    /// Deliver an opened URL to the registered closure. Runs on the main thread
    /// (the Apple Event is dispatched from the run loop); `try_borrow_mut`
    /// guards against the pathological case of a re-entrant open.
    fn dispatch(url: String) {
        URL_HANDLER.with(|h| {
            if let Ok(mut slot) = h.try_borrow_mut()
                && let Some(cb) = slot.as_mut()
            {
                cb(url);
            }
        });
    }

    define_class!(
        // A plain NSObject subclass with no instance variables, used solely as
        // the Apple Event handler target. No `Drop`, no ivars, no main-thread
        // restriction — the default kind is fine.
        #[unsafe(super(NSObject))]
        #[name = "PikiUrlHandler"]
        struct PikiUrlHandler;

        impl PikiUrlHandler {
            // - (void)handleGetURLEvent:(NSAppleEventDescriptor *)event
            //           withReplyEvent:(NSAppleEventDescriptor *)reply;
            #[unsafe(method(handleGetURLEvent:withReplyEvent:))]
            fn handle_get_url_event(
                &self,
                event: &NSAppleEventDescriptor,
                _reply: &NSAppleEventDescriptor,
            ) {
                // The URL lives in the direct object parameter (keyDirectObject,
                // four-char code '----').
                let key_direct_object = u32::from_be_bytes(*b"----");
                // SAFETY: `paramDescriptorForKeyword:` takes an AEKeyword (u32)
                // and returns a nullable NSAppleEventDescriptor.
                let desc: Option<Retained<NSAppleEventDescriptor>> =
                    unsafe { msg_send![event, paramDescriptorForKeyword: key_direct_object] };
                if let Some(desc) = desc
                    && let Some(url) = desc.stringValue()
                {
                    dispatch(url.to_string());
                }
            }
        }

        unsafe impl NSObjectProtocol for PikiUrlHandler {}
    );

    impl PikiUrlHandler {
        fn new() -> Retained<Self> {
            // No ivars and no overridden `init`, so `init` dispatches straight to
            // NSObject.
            unsafe { msg_send![Self::alloc(), init] }
        }
    }

    pub fn register() {
        // The shared manager must be touched on the main thread.
        if MainThreadMarker::new().is_none() {
            return;
        }

        let manager = NSAppleEventManager::sharedAppleEventManager();
        let handler = PikiUrlHandler::new();

        // kInternetEventClass == kAEGetURL == 'GURL'.
        let event_class = u32::from_be_bytes(*b"GURL");
        let event_id = u32::from_be_bytes(*b"GURL");

        // SAFETY: `handler` responds to `handleGetURLEvent:withReplyEvent:`; the
        // four-char codes are the documented GetURL class/id; the arg types
        // (object, Sel, AEEventClass=u32, AEEventID=u32) match the method. The
        // handler is kept alive in the `HANDLER` thread-local below.
        unsafe {
            let _: () = msg_send![
                &*manager,
                setEventHandler: &*handler,
                andSelector: sel!(handleGetURLEvent:withReplyEvent:),
                forEventClass: event_class,
                andEventID: event_id,
            ];
        }

        HANDLER.with(|h| *h.borrow_mut() = Some(handler));
    }
}

#[cfg(target_os = "macos")]
pub use imp::{register, set_open_url_handler};

/// Register the closure invoked when a `piki://` URL is opened. The closure runs
/// on the main thread; defer any UI work with `fltk::app::awake_callback`.
#[cfg(not(target_os = "macos"))]
pub fn set_open_url_handler<F: FnMut(String) + 'static>(_handler: F) {}

/// Install the OS-level handler for the `piki` URL scheme. No-op off macOS.
#[cfg(not(target_os = "macos"))]
pub fn register() {}
