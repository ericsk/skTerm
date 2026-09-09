//! Native macOS NSTextField overlay for sidebar agent task label editing.
//! Provides 100% native macOS IME (Bopomofo/Pinyin), Cmd+A/C/V/X/Z, mouse text selection,
//! and native Enter/Escape/focus-loss handling.

#![cfg(target_os = "macos")]

use std::ffi::{c_void, CStr, CString};
use std::sync::Once;
use crossbeam_channel::{unbounded, Receiver, Sender};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, BOOL, NO, YES};
use objc::{class, msg_send, sel, sel_impl};
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
    ns_view: *mut Object,
    text_field: *mut Object,
    completed: bool,
}

static REGISTER_DELEGATE_CLASS: Once = Once::new();

fn get_delegate_class() -> &'static Class {
    REGISTER_DELEGATE_CLASS.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("SkTermSidebarTextFieldDelegate", superclass)
            .expect("Failed to declare SkTermSidebarTextFieldDelegate");

        decl.add_ivar::<*mut c_void>("_rust_state");

        // - (BOOL)control:(NSControl *)control textView:(NSTextView *)textView doCommandBySelector:(SEL)commandSelector
        extern "C" fn do_command_by_selector(
            this: &Object,
            _cmd: Sel,
            control: *mut Object,
            _text_view: *mut Object,
            command_selector: Sel,
        ) -> BOOL {
            unsafe {
                let state_ptr: *mut c_void = *this.get_ivar("_rust_state");
                if state_ptr.is_null() {
                    return NO;
                }
                let state = &mut *(state_ptr as *mut DelegateState);

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
                    return YES;
                } else if command_selector == sel!(cancelOperation:) {
                    if !state.completed {
                        state.completed = true;
                        let _ = state.sender.send(TextFieldEvent::Cancelled);
                        close_and_cleanup(state);
                    }
                    return YES;
                }
            }
            NO
        }

        // - (void)controlTextDidEndEditing:(NSNotification *)notification
        extern "C" fn text_did_end_editing(this: &Object, _cmd: Sel, _notification: *mut Object) {
            unsafe {
                let state_ptr: *mut c_void = *this.get_ivar("_rust_state");
                if state_ptr.is_null() {
                    return;
                }
                let state = &mut *(state_ptr as *mut DelegateState);
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
                    as extern "C" fn(&Object, Sel, *mut Object, *mut Object, Sel) -> BOOL,
            );
            decl.add_method(
                sel!(controlTextDidEndEditing:),
                text_did_end_editing as extern "C" fn(&Object, Sel, *mut Object),
            );
        }

        decl.register();
    });

    Class::get("SkTermSidebarTextFieldDelegate").unwrap()
}

unsafe fn close_and_cleanup(state: &mut DelegateState) {
    if !state.text_field.is_null() {
        let _: () = msg_send![state.text_field, removeFromSuperview];
        let win: *mut Object = msg_send![state.ns_view, window];
        if !win.is_null() {
            let _: BOOL = msg_send![win, makeFirstResponder: state.ns_view];
        }
    }
}

unsafe fn string_to_nsstring(s: &str) -> *mut Object {
    let cstr = CString::new(s).unwrap_or_default();
    msg_send![class!(NSString), stringWithUTF8String: cstr.as_ptr()]
}

unsafe fn nsstring_to_string(ns_str: *mut Object) -> String {
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
    active_state_ptr: Option<*mut DelegateState>,
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
        if let Some(state_ptr) = self.active_state_ptr.take() {
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
                let _ = Box::from_raw(state_ptr);
            }
        }
    }

    /// Spawn a native macOS NSTextField overlay directly over the sidebar card.
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
        let ns_view = appkit.ns_view.as_ptr() as *mut Object;
        if ns_view.is_null() {
            return;
        }

        unsafe {
            let is_flipped: BOOL = msg_send![ns_view, isFlipped];
            let view_bounds: NSRect = msg_send![ns_view, bounds];
            let cocoa_y = if is_flipped == YES {
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

            let text_field: *mut Object = msg_send![class!(NSTextField), alloc];
            let text_field: *mut Object = msg_send![text_field, initWithFrame: frame];

            // Configure sleek sidebar styling
            let ns_text = string_to_nsstring(initial_text);
            let _: () = msg_send![text_field, setStringValue: ns_text];
            let _: () = msg_send![text_field, setEditable: YES];
            let _: () = msg_send![text_field, setSelectable: YES];
            let _: () = msg_send![text_field, setBezeled: YES];
            let _: () = msg_send![text_field, setDrawsBackground: YES];

            // Dark background matching sidebar card (#17243A)
            let bg_color: *mut Object = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.09_f64
                green: 0.14_f64
                blue: 0.23_f64
                alpha: 1.0_f64
            ];
            let _: () = msg_send![text_field, setBackgroundColor: bg_color];

            // Crisp white text color
            let text_color: *mut Object = msg_send![
                class!(NSColor),
                colorWithSRGBRed: 0.98_f64
                green: 0.98_f64
                blue: 1.0_f64
                alpha: 1.0_f64
            ];
            let _: () = msg_send![text_field, setTextColor: text_color];

            // Font: 13pt bold system font
            let font: *mut Object = msg_send![class!(NSFont), boldSystemFontOfSize: 13.0_f64];
            if !font.is_null() {
                let _: () = msg_send![text_field, setFont: font];
            }

            // Create Delegate
            let delegate_cls = get_delegate_class();
            let delegate: *mut Object = msg_send![delegate_cls, new];

            let state = Box::new(DelegateState {
                target,
                sender: self.sender.clone(),
                ns_view,
                text_field,
                completed: false,
            });
            let state_ptr = Box::into_raw(state);
            (*delegate).set_ivar("_rust_state", state_ptr as *mut c_void);

            let _: () = msg_send![text_field, setDelegate: delegate];

            // Add as subview and focus
            let _: () = msg_send![ns_view, addSubview: text_field];
            let ns_window: *mut Object = msg_send![ns_view, window];
            if !ns_window.is_null() {
                let _: BOOL = msg_send![ns_window, makeFirstResponder: text_field];
            }
            let _: () = msg_send![text_field, selectText: std::ptr::null::<Object>()];

            self.active_state_ptr = Some(state_ptr);
        }
    }
}
