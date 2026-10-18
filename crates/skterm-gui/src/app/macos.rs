//! macOS-only AppKit/Objective-C interop for native window titles and tab bars.

use winit::window::Window;

#[cfg(target_os = "macos")]
pub(crate) fn get_nswindow(window: &Window) -> *mut objc2::runtime::AnyObject {
    use objc2::msg_send;
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
            let view_ptr = appkit.ns_view.as_ptr();
            let win_ptr: *mut objc2::runtime::AnyObject =
                unsafe { msg_send![view_ptr as *mut objc2::runtime::AnyObject, window] };
            return win_ptr;
        }
    }
    std::ptr::null_mut()
}

/// Label a macOS native tab independently of its window title.
///
/// A native tab normally borrows the window title. Ours starts with `skTerm — `,
/// and because tab labels are narrow, several tabs all truncate to `skTerm — …`
/// and become indistinguishable. `NSWindow.tab` lets us keep the app name in the
/// window title (where Mission Control and the Window menu want it) while the tab
/// itself shows only what identifies the session.
///
/// `tooltip` is shown on hover, which is where the full path can live.
///
/// # Safety
/// `win_ptr` must be null or a valid pointer to a live `NSWindow`, and this must
/// run on the main thread, as it messages AppKit objects.
#[cfg(target_os = "macos")]
pub unsafe fn set_macos_tab_label(
    win_ptr: *mut objc2::runtime::AnyObject,
    label: &str,
    tooltip: &str,
) {
    use objc2::{msg_send, sel};
    if win_ptr.is_null() {
        return;
    }
    // `-[NSWindow tab]` is 10.13+. Guard rather than assume, so an older system
    // degrades to the plain window title instead of crashing.
    let responds: bool = msg_send![win_ptr, respondsToSelector: sel!(tab)];
    if !responds {
        return;
    }
    let tab: *mut objc2::runtime::AnyObject = msg_send![win_ptr, tab];
    if tab.is_null() {
        return;
    }

    unsafe fn nsstring(s: &str) -> *mut objc2::runtime::AnyObject {
        use objc2::{class, msg_send};
        let bytes = s.as_bytes();
        let data: *mut objc2::runtime::AnyObject = msg_send![class!(NSString), alloc];
        // Length-explicit constructor: tab labels are user data and may contain
        // interior NULs or invalid UTF-8 boundaries that a C-string API would cut.
        let s: *mut objc2::runtime::AnyObject = msg_send![data,
            initWithBytes: bytes.as_ptr() as *const std::os::raw::c_void,
            length: bytes.len(),
            encoding: 4usize]; // NSUTF8StringEncoding
        s
    }

    let ns_label = nsstring(label);
    let _: () = msg_send![tab, setTitle: ns_label];
    let _: () = msg_send![ns_label, release];

    let ns_tip = nsstring(tooltip);
    let _: () = msg_send![tab, setToolTip: ns_tip];
    let _: () = msg_send![ns_tip, release];
}

/// Centers the macOS window title by re-anchoring the titlebar `NSTextField`.
///
/// # Safety
/// `win_ptr` must be null or a valid pointer to a live `NSWindow`. The caller must
/// ensure this is invoked on the main thread, as it walks and mutates the AppKit
/// view hierarchy.
#[cfg(target_os = "macos")]
pub unsafe fn center_macos_window_title(win_ptr: *mut objc2::runtime::AnyObject) {
    use objc2::{class, msg_send};
    if win_ptr.is_null() {
        return;
    }

    unsafe fn find_titlebar_text_field(
        view: *mut objc2::runtime::AnyObject,
        content_view: *mut objc2::runtime::AnyObject,
    ) -> *mut objc2::runtime::AnyObject {
        if view.is_null() || view == content_view {
            return std::ptr::null_mut();
        }
        // Never descend into NSTabBar: tab buttons also contain NSTextField
        // subviews, and touching those would corrupt the tab bar.
        if let Some(tab_bar_cls) = objc2::runtime::AnyClass::get(c"NSTabBar") {
            let is_tab_bar: bool = msg_send![view, isKindOfClass: tab_bar_cls];
            if is_tab_bar {
                return std::ptr::null_mut();
            }
        }
        // Same reasoning for buttons -- the traffic lights and our titlebar
        // accessory are NSButtons, and the window title is never inside one.
        let is_button: bool = msg_send![view, isKindOfClass: class!(NSButton)];
        if is_button {
            return std::ptr::null_mut();
        }
        let is_tf: bool = msg_send![view, isKindOfClass: class!(NSTextField)];
        if is_tf {
            return view;
        }
        let subviews: *mut objc2::runtime::AnyObject = msg_send![view, subviews];
        if subviews.is_null() {
            return std::ptr::null_mut();
        }
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let sub: *mut objc2::runtime::AnyObject = msg_send![subviews, objectAtIndex: i];
            let found = find_titlebar_text_field(sub, content_view);
            if !found.is_null() {
                return found;
            }
        }
        std::ptr::null_mut()
    }

    let content_view: *mut objc2::runtime::AnyObject = msg_send![win_ptr, contentView];
    if content_view.is_null() {
        return;
    }
    let frame_view: *mut objc2::runtime::AnyObject = msg_send![content_view, superview];
    if frame_view.is_null() {
        return;
    }
    let tf = find_titlebar_text_field(frame_view, content_view);
    if tf.is_null() {
        return;
    }
    let titlebar: *mut objc2::runtime::AnyObject = msg_send![tf, superview];
    if titlebar.is_null() {
        return;
    }

    let _: () = msg_send![tf, setAlignment: 1isize]; // NSTextAlignmentCenter = 1

    // Install Auto Layout constraints at most once per window. Re-adding them on
    // every resize or title update accumulates duplicate constraints.
    let translates: bool = msg_send![tf, translatesAutoresizingMaskIntoConstraints];
    if !translates {
        let _: () = msg_send![titlebar, layoutSubtreeIfNeeded];
        return;
    }

    let _: () = msg_send![tf, setTranslatesAutoresizingMaskIntoConstraints: false];

    let tf_center_x: *mut objc2::runtime::AnyObject = msg_send![tf, centerXAnchor];
    let bar_center_x: *mut objc2::runtime::AnyObject = msg_send![titlebar, centerXAnchor];
    if !tf_center_x.is_null() && !bar_center_x.is_null() {
        let constraint_x: *mut objc2::runtime::AnyObject =
            msg_send![tf_center_x, constraintEqualToAnchor: bar_center_x];
        if !constraint_x.is_null() {
            let _: () = msg_send![constraint_x, setActive: true];
        }
    }

    // Anchor vertical position to the traffic-light close button
    // (NSWindowCloseButton = 0) rather than `titlebar.centerYAnchor`. When native
    // tabs are open, `titlebar` (NSTitlebarView) grows to include the NSTabBar
    // below the titlebar strip, so `titlebar.centerYAnchor` lands right on the
    // boundary between the titlebar and the tab bar and drags the window title
    // over the tabs. The traffic lights always stay in the top titlebar strip.
    let close_btn: *mut objc2::runtime::AnyObject =
        msg_send![win_ptr, standardWindowButton: 0usize];
    let target_center_y: *mut objc2::runtime::AnyObject = if !close_btn.is_null() {
        msg_send![close_btn, centerYAnchor]
    } else {
        msg_send![titlebar, centerYAnchor]
    };
    let tf_center_y: *mut objc2::runtime::AnyObject = msg_send![tf, centerYAnchor];
    if !tf_center_y.is_null() && !target_center_y.is_null() {
        let constraint_y: *mut objc2::runtime::AnyObject =
            msg_send![tf_center_y, constraintEqualToAnchor: target_center_y];
        if !constraint_y.is_null() {
            let _: () = msg_send![constraint_y, setActive: true];
        }
    }

    let _: () = msg_send![titlebar, layoutSubtreeIfNeeded];
}

/// Highlights the focused tab in AppKit's native `NSTabBar` so it stands out
/// clearly from inactive tabs.
///
/// In dark mode macOS draws the active tab as a subtle gray pill that is hard to
/// distinguish at a glance. Walking the `NSTabButton` subviews and applying an
/// accent border, tinted pill background, and bold white label makes the active
/// tab unmistakable.
///
/// # Safety
/// `active_win_ptr` must be null or a valid pointer to a live `NSWindow` on the
/// main thread.
#[cfg(target_os = "macos")]
pub unsafe fn style_macos_tab_bar(active_win_ptr: *mut objc2::runtime::AnyObject) {
    use objc2::{class, msg_send, sel};
    if active_win_ptr.is_null() {
        return;
    }

    unsafe fn style_button_subviews(view: *mut objc2::runtime::AnyObject, is_active: bool) {
        if view.is_null() {
            return;
        }
        if let Some(glass_cls) = objc2::runtime::AnyClass::get(c"NSGlassEffectView") {
            let is_glass: bool = msg_send![view, isKindOfClass: glass_cls];
            if is_glass {
                let _: () = msg_send![view, setWantsLayer: true];
                let layer: *mut objc2::runtime::AnyObject = msg_send![view, layer];
                if !layer.is_null() {
                    if is_active {
                        let _: () = msg_send![layer, setCornerRadius: 6.0f64];
                        let ns_bg: *mut objc2::runtime::AnyObject = msg_send![
                            class!(NSColor),
                            colorWithSRGBRed: 0.22f64,
                            green: 0.36f64,
                            blue: 0.58f64,
                            alpha: 0.55f64
                        ];
                        let cg_bg: *mut std::os::raw::c_void = msg_send![ns_bg, CGColor];
                        let _: () = msg_send![layer, setBackgroundColor: cg_bg];
                    } else {
                        let null_cg: *mut std::os::raw::c_void = std::ptr::null_mut();
                        let _: () = msg_send![layer, setBackgroundColor: null_cg];
                    }
                }
            }
        }
        let is_tf: bool = msg_send![view, isKindOfClass: class!(NSTextField)];
        if is_tf {
            let color: *mut objc2::runtime::AnyObject = if is_active {
                msg_send![
                    class!(NSColor),
                    colorWithSRGBRed: 1.0f64,
                    green: 1.0f64,
                    blue: 1.0f64,
                    alpha: 1.0f64
                ]
            } else {
                msg_send![
                    class!(NSColor),
                    colorWithSRGBRed: 0.65f64,
                    green: 0.67f64,
                    blue: 0.70f64,
                    alpha: 1.0f64
                ]
            };
            let _: () = msg_send![view, setTextColor: color];
            let font: *mut objc2::runtime::AnyObject = if is_active {
                msg_send![class!(NSFont), boldSystemFontOfSize: 12.0f64]
            } else {
                msg_send![class!(NSFont), systemFontOfSize: 12.0f64]
            };
            if !font.is_null() {
                let _: () = msg_send![view, setFont: font];
            }
        }
        let subviews: *mut objc2::runtime::AnyObject = msg_send![view, subviews];
        if !subviews.is_null() {
            let count: usize = msg_send![subviews, count];
            for i in 0..count {
                let sub: *mut objc2::runtime::AnyObject = msg_send![subviews, objectAtIndex: i];
                style_button_subviews(sub, is_active);
            }
        }
    }

    unsafe fn walk_for_tab_buttons(
        view: *mut objc2::runtime::AnyObject,
        active_win_ptr: *mut objc2::runtime::AnyObject,
        content_view: *mut objc2::runtime::AnyObject,
    ) {
        if view.is_null() || view == content_view {
            return;
        }
        if let Some(tab_btn_cls) = objc2::runtime::AnyClass::get(c"NSTabButton") {
            let is_btn: bool = msg_send![view, isKindOfClass: tab_btn_cls];
            if is_btn {
                let responds: bool = msg_send![view, respondsToSelector: sel!(associatedWindow)];
                let assoc_win: *mut objc2::runtime::AnyObject = if responds {
                    msg_send![view, associatedWindow]
                } else {
                    std::ptr::null_mut()
                };
                let is_active = assoc_win == active_win_ptr;
                let _: () = msg_send![view, setWantsLayer: true];
                let layer: *mut objc2::runtime::AnyObject = msg_send![view, layer];
                if !layer.is_null() {
                    if is_active {
                        let _: () = msg_send![layer, setCornerRadius: 6.0f64];
                        let _: () = msg_send![layer, setBorderWidth: 1.5f64];
                        let ns_border: *mut objc2::runtime::AnyObject = msg_send![
                            class!(NSColor),
                            colorWithSRGBRed: 0.42f64,
                            green: 0.72f64,
                            blue: 1.0f64,
                            alpha: 0.95f64
                        ];
                        let cg_border: *mut std::os::raw::c_void = msg_send![ns_border, CGColor];
                        let _: () = msg_send![layer, setBorderColor: cg_border];
                        let ns_bg: *mut objc2::runtime::AnyObject = msg_send![
                            class!(NSColor),
                            colorWithSRGBRed: 0.22f64,
                            green: 0.36f64,
                            blue: 0.58f64,
                            alpha: 0.35f64
                        ];
                        let cg_bg: *mut std::os::raw::c_void = msg_send![ns_bg, CGColor];
                        let _: () = msg_send![layer, setBackgroundColor: cg_bg];
                    } else {
                        let _: () = msg_send![layer, setBorderWidth: 0.0f64];
                        let null_cg: *mut std::os::raw::c_void = std::ptr::null_mut();
                        let _: () = msg_send![layer, setBackgroundColor: null_cg];
                    }
                }
                style_button_subviews(view, is_active);
                return;
            }
        }
        let subviews: *mut objc2::runtime::AnyObject = msg_send![view, subviews];
        if !subviews.is_null() {
            let count: usize = msg_send![subviews, count];
            for i in 0..count {
                let sub: *mut objc2::runtime::AnyObject = msg_send![subviews, objectAtIndex: i];
                walk_for_tab_buttons(sub, active_win_ptr, content_view);
            }
        }
    }

    let content_view: *mut objc2::runtime::AnyObject = msg_send![active_win_ptr, contentView];
    if content_view.is_null() {
        return;
    }
    let frame_view: *mut objc2::runtime::AnyObject = msg_send![content_view, superview];
    if frame_view.is_null() {
        return;
    }
    walk_for_tab_buttons(frame_view, active_win_ptr, content_view);
}
