//! Native macOS titlebar accessory hosting the global AI agents toggle.
//!
//! The agents panel is application-global, so its toggle must not live in the
//! tab strip. On macOS the tab strip is the system `NSTabBar`, which sits inside
//! the titlebar and therefore outside our wgpu surface -- nothing we draw can
//! reach it. A `NSTitlebarAccessoryViewController` is the supported way to put a
//! real control up there, alongside the tabs rather than inside the content.
//!
//! The module is gated on macOS by its declaration in `lib.rs`.

use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use std::collections::HashMap;
use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, Once, OnceLock};

/// Bumped by the button's action. The event loop compares it against the count
/// it last observed, which keeps the AppKit callback allocation-free and lets
/// several windows share one counter.
static CLICK_COUNT: AtomicU64 = AtomicU64::new(0);

/// NSWindow pointer -> our NSButton, so the button can mirror the panel state.
fn buttons() -> &'static Mutex<HashMap<usize, usize>> {
    static BUTTONS: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();
    BUTTONS.get_or_init(|| Mutex::new(HashMap::new()))
}

static REGISTER_TARGET_CLASS: Once = Once::new();

fn target_class() -> &'static AnyClass {
    REGISTER_TARGET_CLASS.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassBuilder::new(c"SkTermAgentToggleTarget", superclass)
            .expect("Failed to declare SkTermAgentToggleTarget");

        extern "C" fn toggle_clicked(_this: &AnyObject, _cmd: Sel, _sender: *mut AnyObject) {
            CLICK_COUNT.fetch_add(1, Ordering::SeqCst);
        }

        unsafe {
            decl.add_method(
                sel!(skTermToggleAgents:),
                toggle_clicked as extern "C" fn(_, Sel, *mut AnyObject),
            );
        }
        decl.register();
    });
    AnyClass::get(c"SkTermAgentToggleTarget").unwrap()
}

/// The shared action target. AppKit does not retain targets, so this one is
/// created once and deliberately leaked for the lifetime of the process.
fn shared_target() -> *mut AnyObject {
    static TARGET: OnceLock<usize> = OnceLock::new();
    *TARGET.get_or_init(|| unsafe {
        let obj: *mut AnyObject = msg_send![target_class(), new];
        obj as usize
    }) as *mut AnyObject
}

unsafe fn nsstring(s: &str) -> *mut AnyObject {
    let c = CString::new(s).unwrap_or_else(|_| CString::new("").unwrap());
    let cls = class!(NSString);
    msg_send![cls, stringWithUTF8String: c.as_ptr()]
}

/// Adds the "AI" toggle to `win_ptr`'s titlebar, to the right of the tabs.
///
/// Safe to call repeatedly for the same window; the second call is a no-op.
///
/// # Safety
/// `win_ptr` must be null or a valid pointer to a live `NSWindow` on the main
/// thread.
pub unsafe fn install_agent_toggle(win_ptr: *mut AnyObject) {
    if win_ptr.is_null() {
        return;
    }
    {
        let map = buttons().lock().unwrap();
        if map.contains_key(&(win_ptr as usize)) {
            return;
        }
    }

    let title = nsstring("AI");
    let button: *mut AnyObject = msg_send![
        class!(NSButton),
        buttonWithTitle: title,
        target: shared_target(),
        action: sel!(skTermToggleAgents:)
    ];
    if button.is_null() {
        return;
    }
    // NSButtonTypePushOnPushOff: the control shows whether the panel is open.
    let _: () = msg_send![button, setButtonType: 1usize];
    // NSBezelStyleRounded
    let _: () = msg_send![button, setBezelStyle: 1usize];
    let _: () = msg_send![button, setToolTip: nsstring("All AI agents, across every tab")];
    let _: () = msg_send![button, setFrame: NSRect {
        origin: NSPoint { x: 0.0, y: 0.0 },
        size: NSSize { width: 44.0, height: 22.0 },
    }];

    let vc: *mut AnyObject = msg_send![class!(NSTitlebarAccessoryViewController), alloc];
    let vc: *mut AnyObject = msg_send![vc, init];
    if vc.is_null() {
        return;
    }
    let _: () = msg_send![vc, setView: button];
    // NSLayoutAttributeRight = 2: park it at the trailing end of the titlebar so
    // it never fights the tab bar for space.
    let _: () = msg_send![vc, setLayoutAttribute: 2isize];
    let _: () = msg_send![win_ptr, addTitlebarAccessoryViewController: vc];

    buttons()
        .lock()
        .unwrap()
        .insert(win_ptr as usize, button as usize);
}

/// Mirrors the panel's open/closed state onto the window's toggle.
///
/// # Safety
/// Must run on the main thread, with `win_ptr` a live `NSWindow` or null.
pub unsafe fn set_agent_toggle_state(win_ptr: *mut AnyObject, open: bool) {
    if win_ptr.is_null() {
        return;
    }
    let button = {
        let map = buttons().lock().unwrap();
        match map.get(&(win_ptr as usize)) {
            Some(&b) => b as *mut AnyObject,
            None => return,
        }
    };
    // NSControlStateValueOn = 1, Off = 0
    let _: () = msg_send![button, setState: if open { 1isize } else { 0isize }];
}

/// Forgets a closed window's button so the map does not grow without bound.
pub fn forget_window(win_ptr: *mut AnyObject) {
    buttons().lock().unwrap().remove(&(win_ptr as usize));
}

/// Number of times the toggle has been clicked since the process started.
///
/// The event loop keeps the previously seen value and acts on any increase,
/// which avoids losing clicks that land between two polls.
pub fn click_count() -> u64 {
    CLICK_COUNT.load(Ordering::SeqCst)
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Copy, Clone)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

unsafe impl Encode for NSPoint {
    const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl RefEncode for NSPoint {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

unsafe impl Encode for NSSize {
    const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
}
unsafe impl RefEncode for NSSize {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}

unsafe impl Encode for NSRect {
    const ENCODING: Encoding = Encoding::Struct("CGRect", &[NSPoint::ENCODING, NSSize::ENCODING]);
}
unsafe impl RefEncode for NSRect {
    const ENCODING_REF: Encoding = Encoding::Pointer(&Self::ENCODING);
}
