//! Native macOS NSTextField overlay for sidebar agent task label editing.
//! Provides 100% native macOS IME (Bopomofo/Pinyin), Cmd+A/C/V/X/Z, mouse text selection,
//! and native Enter/Escape/focus-loss handling.

use crossbeam_channel::{unbounded, Receiver, Sender};
use objc2::encode::{Encode, Encoding, RefEncode};
use objc2::runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel};
use objc2::{class, msg_send, sel};
use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::sync::{Mutex, Once, OnceLock};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::{Window, WindowId};

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
struct NSSize {
    width: f64,
    height: f64,
}

#[repr(C)]
#[derive(Copy, Clone, Debug)]
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

#[derive(Debug, Clone)]
pub enum TextFieldEvent {
    Committed {
        target: (WindowId, u32, u32),
        text: String,
    },
    Cancelled,
}

struct DelegateState {
    target: (WindowId, u32, u32),
    sender: Sender<TextFieldEvent>,
    ns_view: *mut AnyObject,
    text_field: *mut AnyObject,
    completed: bool,
}

fn delegate_states() -> &'static Mutex<HashMap<usize, usize>> {
    static STATES: OnceLock<Mutex<HashMap<usize, usize>>> = OnceLock::new();
    STATES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup_delegate_state(this: &AnyObject) -> *mut DelegateState {
    let key = this as *const AnyObject as usize;
    delegate_states()
        .lock()
        .ok()
        .and_then(|map| map.get(&key).copied())
        .unwrap_or(0) as *mut DelegateState
}

static REGISTER_DELEGATE_CLASS: Once = Once::new();

fn get_delegate_class() -> &'static AnyClass {
    REGISTER_DELEGATE_CLASS.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassBuilder::new(c"SkTermSidebarTextFieldDelegate", superclass)
            .expect("Failed to declare SkTermSidebarTextFieldDelegate");

        // - (BOOL)control:(NSControl *)control textView:(NSTextView *)textView doCommandBySelector:(SEL)commandSelector
        extern "C" fn do_command_by_selector(
            this: &AnyObject,
            _cmd: Sel,
            control: *mut AnyObject,
            _text_view: *mut AnyObject,
            command_selector: Sel,
        ) -> Bool {
            unsafe {
                let state_ptr = lookup_delegate_state(this);
                if state_ptr.is_null() {
                    return Bool::NO;
                }
                let state = &mut *state_ptr;

                if command_selector == sel!(insertNewline:) {
                    if !state.completed {
                        state.completed = true;
                        let text = nsstring_to_string(msg_send![control, stringValue]);
                        let _ = state.sender.send(TextFieldEvent::Committed {
                            target: state.target,
                            text,
                        });
                        close_and_cleanup(state);
                    }
                    return Bool::YES;
                } else if command_selector == sel!(cancelOperation:) {
                    if !state.completed {
                        state.completed = true;
                        let _ = state.sender.send(TextFieldEvent::Cancelled);
                        close_and_cleanup(state);
                    }
                    return Bool::YES;
                }
            }
            Bool::NO
        }

        // - (void)controlTextDidEndEditing:(NSNotification *)notification
        extern "C" fn text_did_end_editing(
            this: &AnyObject,
            _cmd: Sel,
            _notification: *mut AnyObject,
        ) {
            unsafe {
                let state_ptr = lookup_delegate_state(this);
                if state_ptr.is_null() {
                    return;
                }
                let state = &mut *state_ptr;
                if !state.completed {
                    state.completed = true;
                    let text = nsstring_to_string(msg_send![state.text_field, stringValue]);
                    let _ = state.sender.send(TextFieldEvent::Committed {
                        target: state.target,
                        text,
                    });
                    close_and_cleanup(state);
                }
            }
        }

        unsafe {
            decl.add_method(
                sel!(control:textView:doCommandBySelector:),
                do_command_by_selector
                    as extern "C" fn(_, Sel, *mut AnyObject, *mut AnyObject, Sel) -> Bool,
            );
            decl.add_method(
                sel!(controlTextDidEndEditing:),
                text_did_end_editing as extern "C" fn(_, Sel, *mut AnyObject),
            );
        }

        decl.register();
    });

    AnyClass::get(c"SkTermSidebarTextFieldDelegate").unwrap()
}

unsafe fn close_and_cleanup(state: &mut DelegateState) {
    if !state.text_field.is_null() {
        let _: () = msg_send![state.text_field, removeFromSuperview];
        let win: *mut AnyObject = msg_send![state.ns_view, window];
        if !win.is_null() {
            let _: Bool = msg_send![win, makeFirstResponder: state.ns_view];
        }
    }
}

unsafe fn string_to_nsstring(s: &str) -> *mut AnyObject {
    let cstr = CString::new(s).unwrap_or_default();
    msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()]
}

unsafe fn nsstring_to_string(ns_str: *mut AnyObject) -> String {
    if ns_str.is_null() {
        return String::new();
    }
    let utf8: *const i8 = msg_send![ns_str, UTF8String];
    if utf8.is_null() {
        return String::new();
    }
    CStr::from_ptr(utf8).to_string_lossy().into_owned()
}

pub struct NativeSidebarEditor {
    pub receiver: Receiver<TextFieldEvent>,
    sender: Sender<TextFieldEvent>,
    active_state_ptr: Option<(usize, *mut DelegateState)>,
}

impl Default for NativeSidebarEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeSidebarEditor {
    pub fn new() -> Self {
        let (sender, receiver) = unbounded();
        Self {
            receiver,
            sender,
            active_state_ptr: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.active_state_ptr.is_some()
    }

    pub fn close_active(&mut self, commit: bool) {
        if let Some((delegate_key, state_ptr)) = self.active_state_ptr.take() {
            unsafe {
                let state = &mut *state_ptr;
                if !state.completed {
                    state.completed = true;
                    if commit {
                        let text = nsstring_to_string(msg_send![state.text_field, stringValue]);
                        let _ = state.sender.send(TextFieldEvent::Committed {
                            target: state.target,
                            text,
                        });
                    } else {
                        let _ = state.sender.send(TextFieldEvent::Cancelled);
                    }
                    close_and_cleanup(state);
                }
                if let Ok(mut map) = delegate_states().lock() {
                    map.remove(&delegate_key);
                }
                let _ = Box::from_raw(state_ptr);
            }
        }
    }

    /// Spawn a native macOS NSTextField overlay directly over the sidebar card.
    // The trailing four arguments are the logical rect handed straight to AppKit;
    // keeping them flat mirrors the NSRect the caller already has on hand.
    #[allow(clippy::too_many_arguments)]
    pub fn open_for_card(
        &mut self,
        window: &Window,
        target: (WindowId, u32, u32),
        initial_text: &str,
        logical_x: f64,
        logical_y: f64,
        logical_w: f64,
        logical_h: f64,
    ) {
        self.close_active(true);

        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return;
        };
        let ns_view = appkit.ns_view.as_ptr() as *mut AnyObject;
        if ns_view.is_null() {
            return;
        }

        unsafe {
            let is_flipped: Bool = msg_send![ns_view, isFlipped];
            let view_bounds: NSRect = msg_send![ns_view, bounds];
            let cocoa_y = if is_flipped.as_bool() {
                logical_y
            } else {
                view_bounds.size.height - logical_y - logical_h
            };

            let frame = NSRect {
                origin: NSPoint {
                    x: logical_x,
                    y: cocoa_y,
                },
                size: NSSize {
                    width: logical_w,
                    height: logical_h,
                },
            };

            let text_field: *mut AnyObject = msg_send![class!(NSTextField), alloc];
            let text_field: *mut AnyObject = msg_send![text_field, initWithFrame: frame];

            // Configure sleek sidebar styling
            let ns_text = string_to_nsstring(initial_text);
            let _: () = msg_send![text_field, setStringValue: ns_text];
            let _: () = msg_send![text_field, setEditable: Bool::YES];
            let _: () = msg_send![text_field, setSelectable: Bool::YES];
            let _: () = msg_send![text_field, setBezeled: Bool::YES];
            let _: () = msg_send![text_field, setDrawsBackground: Bool::YES];

            // Dark background matching sidebar card (#17243A)
            let bg_color: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.09_f64,
                green: 0.14_f64,
                blue: 0.23_f64,
                alpha: 1.0_f64
            ];
            let _: () = msg_send![text_field, setBackgroundColor: bg_color];

            // Crisp white text color
            let text_color: *mut AnyObject = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.98_f64,
                green: 0.98_f64,
                blue: 1.0_f64,
                alpha: 1.0_f64
            ];
            let _: () = msg_send![text_field, setTextColor: text_color];

            // Font: 13pt bold system font
            let font: *mut AnyObject = msg_send![class!(NSFont), boldSystemFontOfSize: 13.0_f64];
            if !font.is_null() {
                let _: () = msg_send![text_field, setFont: font];
            }

            // Create Delegate
            let delegate_cls = get_delegate_class();
            let delegate: *mut AnyObject = msg_send![delegate_cls, new];
            let delegate_key = delegate as usize;

            let state = Box::new(DelegateState {
                target,
                sender: self.sender.clone(),
                ns_view,
                text_field,
                completed: false,
            });
            let state_ptr = Box::into_raw(state);
            if let Ok(mut map) = delegate_states().lock() {
                map.insert(delegate_key, state_ptr as usize);
            }

            let _: () = msg_send![text_field, setDelegate: delegate];

            // Add as subview and focus
            let _: () = msg_send![ns_view, addSubview: text_field];
            let ns_window: *mut AnyObject = msg_send![ns_view, window];
            if !ns_window.is_null() {
                let _: Bool = msg_send![ns_window, makeFirstResponder: text_field];
            }
            let _: () = msg_send![text_field, selectText: std::ptr::null::<AnyObject>()];

            self.active_state_ptr = Some((delegate_key, state_ptr));
        }
    }
}
