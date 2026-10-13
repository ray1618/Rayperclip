//! Thin wrappers around the macOS APIs Rayperclip needs.
//!
//! Everything in here must be called on the main thread unless noted.

use std::ffi::c_void;
use std::time::Duration;

use core_foundation::base::TCFType;
use core_foundation::boolean::CFBoolean;
use core_foundation::date::CFDate;
use core_foundation::dictionary::CFDictionary;
use core_foundation::runloop::{
    CFRunLoop, CFRunLoopTimer, CFRunLoopTimerRef, kCFRunLoopCommonModes,
};
use core_foundation::string::CFString;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationOptions, NSApplicationActivationPolicy, NSPasteboard,
    NSPasteboardTypeHTML, NSPasteboardTypeRTF, NSPasteboardTypeString, NSRunningApplication,
    NSWindow, NSWorkspace,
};
use objc2_foundation::{NSData, NSString};

use rayperclip::capture::{FrontApp, Snapshot};
use rayperclip::clipboard::Clipboard;
use rayperclip::model::Clip;
use rayperclip::paste::{KEYCODE_V, PasteBackend};

/// The general pasteboard.
pub struct MacClipboard {
    pb: Retained<NSPasteboard>,
}

impl MacClipboard {
    pub fn new() -> Self {
        Self {
            pb: NSPasteboard::generalPasteboard(),
        }
    }
}

impl Clipboard for MacClipboard {
    fn change_count(&self) -> i64 {
        self.pb.changeCount() as i64
    }

    fn snapshot(&self) -> Snapshot {
        // SAFETY: the pasteboard type constants are immutable statics.
        let (t_string, t_rtf, t_html) = unsafe {
            (
                NSPasteboardTypeString,
                NSPasteboardTypeRTF,
                NSPasteboardTypeHTML,
            )
        };
        let types = self
            .pb
            .types()
            .map(|a| a.iter().map(|t| t.to_string()).collect())
            .unwrap_or_default();
        Snapshot {
            change_count: self.change_count(),
            types,
            text: self.pb.stringForType(t_string).map(|s| s.to_string()),
            rtf: self.pb.dataForType(t_rtf).map(|d| d.to_vec()),
            html: self.pb.dataForType(t_html).map(|d| d.to_vec()),
        }
    }

    fn write_clip(&mut self, clip: &Clip) {
        // SAFETY: see above.
        let (t_string, t_rtf, t_html) = unsafe {
            (
                NSPasteboardTypeString,
                NSPasteboardTypeRTF,
                NSPasteboardTypeHTML,
            )
        };
        self.pb.clearContents();
        self.pb
            .setString_forType(&NSString::from_str(&clip.contents), t_string);
        if let Some(rtf) = &clip.rtf {
            self.pb
                .setData_forType(Some(&NSData::with_bytes(rtf)), t_rtf);
        }
        if let Some(html) = &clip.html {
            self.pb
                .setData_forType(Some(&NSData::with_bytes(html)), t_html);
        }
    }
}

/// Currently frontmost application.
pub fn frontmost_app() -> Option<FrontApp> {
    frontmost_running_app().map(|app| FrontApp {
        name: app.localizedName().map(|s| s.to_string()),
        bundle_id: app.bundleIdentifier().map(|s| s.to_string()),
        pid: app.processIdentifier(),
    })
}

pub fn frontmost_running_app() -> Option<Retained<NSRunningApplication>> {
    NSWorkspace::sharedWorkspace().frontmostApplication()
}

pub fn own_pid() -> i32 {
    std::process::id() as i32
}

// ---------------------------------------------------------------------------
// Accessibility & synthetic paste
// ---------------------------------------------------------------------------

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
}

/// Whether Rayperclip may post keyboard events.
pub fn accessibility_trusted() -> bool {
    // SAFETY: plain C call without arguments.
    unsafe { AXIsProcessTrusted() }
}

/// Show the system prompt asking for Accessibility permission.
pub fn request_accessibility() {
    let key = CFString::from_static_string("AXTrustedCheckOptionPrompt");
    let dict =
        CFDictionary::from_CFType_pairs(&[(key.as_CFType(), CFBoolean::true_value().as_CFType())]);
    // SAFETY: `dict` is a valid CFDictionary for the duration of the call.
    unsafe {
        AXIsProcessTrustedWithOptions(dict.as_concrete_TypeRef() as *const c_void);
    }
}

/// Post Cmd+V to the system after `delay` (runs on a helper thread).
pub fn post_cmd_v_after(delay: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        let Ok(source) = CGEventSource::new(CGEventSourceStateID::HIDSystemState) else {
            eprintln!("Rayperclip: failed to create CGEventSource");
            return;
        };
        for down in [true, false] {
            match CGEvent::new_keyboard_event(source.clone(), KEYCODE_V, down) {
                Ok(ev) => {
                    ev.set_flags(CGEventFlags::CGEventFlagCommand);
                    ev.post(CGEventTapLocation::HID);
                }
                Err(()) => eprintln!("Rayperclip: failed to create keyboard event"),
            }
        }
    });
}

// ---------------------------------------------------------------------------
// Application & window management
// ---------------------------------------------------------------------------

/// Run as a menu-bar-only app (no Dock icon).
pub fn set_accessory(mtm: MainThreadMarker) {
    NSApplication::sharedApplication(mtm)
        .setActivationPolicy(NSApplicationActivationPolicy::Accessory);
}

/// Rayperclip's main window (identified by its title).
pub fn main_window(mtm: MainThreadMarker) -> Option<Retained<NSWindow>> {
    NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .find(|w| w.title().to_string() == rayperclip::APP_NAME)
}

pub fn window_is_visible(mtm: MainThreadMarker) -> bool {
    let app = NSApplication::sharedApplication(mtm);
    app.isActive() && main_window(mtm).is_some_and(|w| w.isVisible())
}

/// Bring the main window to the front and activate Rayperclip.
pub fn show_window(mtm: MainThreadMarker) {
    let app = NSApplication::sharedApplication(mtm);
    app.unhide(None);
    #[allow(deprecated)]
    app.activateIgnoringOtherApps(true);
    if let Some(w) = main_window(mtm) {
        if !w.isVisible() {
            w.center();
        }
        w.makeKeyAndOrderFront(None);
    }
}

/// Hide the main window and give focus back to `previous` (or whichever
/// app macOS chooses when there is none).
pub fn hide_window(mtm: MainThreadMarker, previous: Option<&NSRunningApplication>) {
    let app = NSApplication::sharedApplication(mtm);
    if let Some(w) = main_window(mtm) {
        w.orderOut(None);
    }
    match previous {
        Some(p) if p.processIdentifier() != own_pid() => {
            p.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
        }
        _ => app.hide(None),
    }
}

/// Pointer to the main window's content view, used to anchor popup menus.
pub fn content_view_ptr(mtm: MainThreadMarker) -> Option<*const c_void> {
    main_window(mtm)
        .and_then(|w| w.contentView())
        .map(|v| Retained::as_ptr(&v) as *const c_void)
}

// ---------------------------------------------------------------------------
// Timer
// ---------------------------------------------------------------------------

/// Repeating timer on the main run loop (fires during menu tracking too).
pub fn schedule_repeating(
    interval_secs: f64,
    callback: extern "C" fn(CFRunLoopTimerRef, *mut c_void),
) -> CFRunLoopTimer {
    let start = CFDate::now().abs_time() + interval_secs;
    let timer = CFRunLoopTimer::new(start, interval_secs, 0, 0, callback, std::ptr::null_mut());
    // SAFETY: kCFRunLoopCommonModes is an immutable CF constant.
    CFRunLoop::get_main().add_timer(&timer, unsafe { kCFRunLoopCommonModes });
    timer
}

// ---------------------------------------------------------------------------
// Misc
// ---------------------------------------------------------------------------

/// Open a text file in the default text editor.
pub fn open_in_text_editor(path: &std::path::Path) {
    if let Err(e) = std::process::Command::new("open")
        .arg("-t")
        .arg(path)
        .spawn()
    {
        eprintln!("Rayperclip: could not open {}: {e}", path.display());
    }
}

/// Paste backend that talks to the real system.
pub struct MacPasteBackend {
    pub mtm: MainThreadMarker,
    /// Restore focus to this app (when pasting from the window).
    pub restore_to: Option<Retained<NSRunningApplication>>,
    /// Whether focus must be restored at all (menus never steal focus).
    pub restore: bool,
}

impl PasteBackend for MacPasteBackend {
    fn write_clipboard(&mut self, clip: &Clip) {
        MacClipboard::new().write_clip(clip);
    }

    fn restore_focus(&mut self) {
        if self.restore {
            hide_window(self.mtm, self.restore_to.as_deref());
        }
    }

    fn send_paste_keystroke(&mut self, delay: Duration) {
        post_cmd_v_after(delay);
    }

    fn request_permission(&mut self) {
        request_accessibility();
    }
}
