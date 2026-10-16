//! Native macOS application menu bar (`NSMenu`) with live i18n localization.
//!
//! Builds and manages the standard macOS menu bar (`skTerm`, `File / 檔案`,
//! `Edit / 編輯`, and `Help / 輔助說明`), routing menu item activations and
//! keyboard shortcuts into the winit event loop via [`AppEvent::MenuAction`].

use crate::app::{AppEvent, NativeMenuAction};
use crate::i18n::{strings, Locale};
use objc::declare::ClassDecl;
use objc::runtime::{Class, Object, Sel, BOOL, YES};
use objc::{class, msg_send, sel, sel_impl};
use std::os::raw::c_void;
use std::sync::{Mutex, Once, OnceLock};
use winit::event_loop::EventLoopProxy;

const NSEVENT_MODIFIER_SHIFT: usize = 1 << 17;
const NSEVENT_MODIFIER_OPTION: usize = 1 << 19;
const NSEVENT_MODIFIER_COMMAND: usize = 1 << 20;

static EVENT_PROXY: OnceLock<Mutex<Option<EventLoopProxy<AppEvent>>>> = OnceLock::new();
static MENU_HANDLES: OnceLock<Mutex<Option<MacMenuHandles>>> = OnceLock::new();
static REGISTER_MENU_TARGET: Once = Once::new();

struct MacMenuHandles {
    main_menu: usize,

    // App menu (skTerm)
    app_about: usize,
    app_preferences: usize,
    app_services_item: usize,
    app_services_menu: usize,
    app_hide: usize,
    app_hide_others: usize,
    app_show_all: usize,
    app_quit: usize,

    // File menu
    file_menu_item: usize,
    file_menu: usize,
    file_new_window: usize,
    file_new_tab: usize,
    file_split_horizontal: usize,
    file_split_vertical: usize,
    file_close_tab: usize,
    file_close_window: usize,

    // Edit menu
    edit_menu_item: usize,
    edit_menu: usize,
    edit_undo: usize,
    edit_redo: usize,
    edit_cut: usize,
    edit_copy: usize,
    edit_paste: usize,
    edit_select_all: usize,
    edit_find: usize,
    edit_clear_scrollback: usize,

    // Help menu
    help_menu_item: usize,
    help_menu: usize,
    help_github: usize,
    help_report_issue: usize,
    help_open_config: usize,
}

/// Register the [`EventLoopProxy`] used by menu callbacks to wake the winit loop.
pub fn set_event_proxy(proxy: EventLoopProxy<AppEvent>) {
    let lock = EVENT_PROXY.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = lock.lock() {
        *guard = Some(proxy);
    }
}

fn dispatch_menu_action(action: NativeMenuAction) {
    if let Some(lock) = EVENT_PROXY.get() {
        if let Ok(guard) = lock.lock() {
            if let Some(proxy) = guard.as_ref() {
                let _ = proxy.send_event(AppEvent::MenuAction(action));
            }
        }
    }
}

fn menu_target_class() -> &'static Class {
    REGISTER_MENU_TARGET.call_once(|| {
        let superclass = class!(NSObject);
        let mut decl = ClassDecl::new("SkTermNativeMenuTarget", superclass)
            .expect("Failed to declare SkTermNativeMenuTarget");

        extern "C" fn perform_menu_action(_this: &Object, _cmd: Sel, sender: *mut Object) {
            if sender.is_null() {
                return;
            }
            unsafe {
                let tag: isize = msg_send![sender, tag];
                let Some(action) = NativeMenuAction::from_tag(tag) else {
                    return;
                };

                // If an AppKit text view/field (such as our NativeSidebarEditor NSTextField)
                // is currently the firstResponder in the key window, forward Copy / Paste /
                // SelectAll down the native responder chain so inline text editing works.
                if matches!(
                    action,
                    NativeMenuAction::Copy | NativeMenuAction::Paste | NativeMenuAction::SelectAll
                ) && is_appkit_text_responder_active()
                {
                    let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
                    let sel = match action {
                        NativeMenuAction::Copy => sel!(copy:),
                        NativeMenuAction::Paste => sel!(paste:),
                        NativeMenuAction::SelectAll => sel!(selectAll:),
                        _ => unreachable!(),
                    };
                    let _: BOOL = msg_send![
                        ns_app,
                        sendAction: sel
                        to: std::ptr::null::<Object>()
                        from: sender
                    ];
                    return;
                }

                dispatch_menu_action(action);
            }
        }

        unsafe {
            decl.add_method(
                sel!(skTermPerformMenuAction:),
                perform_menu_action as extern "C" fn(&Object, Sel, *mut Object),
            );
        }
        decl.register();
    });
    Class::get("SkTermNativeMenuTarget").unwrap()
}

fn shared_menu_target() -> *mut Object {
    static TARGET: OnceLock<usize> = OnceLock::new();
    *TARGET.get_or_init(|| unsafe {
        let obj: *mut Object = msg_send![menu_target_class(), new];
        obj as usize
    }) as *mut Object
}

unsafe fn is_appkit_text_responder_active() -> bool {
    let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
    if ns_app.is_null() {
        return false;
    }
    let key_win: *mut Object = msg_send![ns_app, keyWindow];
    if key_win.is_null() {
        return false;
    }
    let responder: *mut Object = msg_send![key_win, firstResponder];
    if responder.is_null() {
        return false;
    }
    let is_text: BOOL = msg_send![responder, isKindOfClass: class!(NSText)];
    let is_tf: BOOL = msg_send![responder, isKindOfClass: class!(NSTextField)];
    is_text == YES || is_tf == YES
}

unsafe fn nsstring(s: &str) -> *mut Object {
    let bytes = s.as_bytes();
    let alloc: *mut Object = msg_send![class!(NSString), alloc];
    msg_send![
        alloc,
        initWithBytes: bytes.as_ptr() as *const c_void
        length: bytes.len()
        encoding: 4usize // NSUTF8StringEncoding
    ]
}

unsafe fn set_title(obj: *mut Object, title: &str) {
    if obj.is_null() {
        return;
    }
    let ns_str = nsstring(title);
    let _: () = msg_send![obj, setTitle: ns_str];
    let _: () = msg_send![ns_str, release];
}

unsafe fn create_menu(title: &str) -> *mut Object {
    let ns_title = nsstring(title);
    let alloc: *mut Object = msg_send![class!(NSMenu), alloc];
    let menu: *mut Object = msg_send![alloc, initWithTitle: ns_title];
    let _: () = msg_send![ns_title, release];
    menu
}

unsafe fn create_separator() -> *mut Object {
    msg_send![class!(NSMenuItem), separatorItem]
}

unsafe fn create_action_item(
    title: &str,
    action: NativeMenuAction,
    key_equiv: &str,
    modifiers: usize,
) -> *mut Object {
    let ns_title = nsstring(title);
    let ns_key = nsstring(key_equiv);
    let alloc: *mut Object = msg_send![class!(NSMenuItem), alloc];
    let item: *mut Object = msg_send![
        alloc,
        initWithTitle: ns_title
        action: sel!(skTermPerformMenuAction:)
        keyEquivalent: ns_key
    ];
    let _: () = msg_send![ns_title, release];
    let _: () = msg_send![ns_key, release];

    if !item.is_null() {
        let _: () = msg_send![item, setTarget: shared_menu_target()];
        let _: () = msg_send![item, setTag: action as isize];
        if !key_equiv.is_empty() {
            let _: () = msg_send![item, setKeyEquivalentModifierMask: modifiers];
        }
    }
    item
}

unsafe fn create_selector_item(
    title: &str,
    selector: Sel,
    target: *mut Object,
    key_equiv: &str,
    modifiers: usize,
) -> *mut Object {
    let ns_title = nsstring(title);
    let ns_key = nsstring(key_equiv);
    let alloc: *mut Object = msg_send![class!(NSMenuItem), alloc];
    let item: *mut Object = msg_send![
        alloc,
        initWithTitle: ns_title
        action: selector
        keyEquivalent: ns_key
    ];
    let _: () = msg_send![ns_title, release];
    let _: () = msg_send![ns_key, release];

    if !item.is_null() {
        let _: () = msg_send![item, setTarget: target];
        if !key_equiv.is_empty() {
            let _: () = msg_send![item, setKeyEquivalentModifierMask: modifiers];
        }
    }
    item
}

unsafe fn detach_submenu_from_existing_menus(main_menu: *mut Object, target_submenu: *mut Object) {
    if main_menu.is_null() || target_submenu.is_null() {
        return;
    }
    let count: isize = msg_send![main_menu, numberOfItems];
    for i in 0..count {
        let item: *mut Object = msg_send![main_menu, itemAtIndex: i];
        if !item.is_null() {
            let sub: *mut Object = msg_send![item, submenu];
            if sub == target_submenu {
                let _: () = msg_send![item, setSubmenu: std::ptr::null_mut::<Object>()];
            } else if !sub.is_null() {
                detach_submenu_from_existing_menus(sub, target_submenu);
            }
        }
    }
}

/// Installs the native macOS menu bar on first call, or updates all menu titles in-place
/// when the active [`Locale`] changes.
///
/// Must be called on the main thread.
pub fn install_or_update_macos_menu(locale: Locale) {
    let lock = MENU_HANDLES.get_or_init(|| Mutex::new(None));
    let mut guard = match lock.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };

    let tr = strings(locale);

    unsafe {
        if let Some(handles) = guard.as_ref() {
            // Update all titles in-place
            set_title(handles.app_about as *mut Object, tr.app_about);
            set_title(handles.app_preferences as *mut Object, tr.app_preferences);
            set_title(handles.app_services_item as *mut Object, tr.app_services);
            set_title(handles.app_services_menu as *mut Object, tr.app_services);
            set_title(handles.app_hide as *mut Object, tr.app_hide);
            set_title(handles.app_hide_others as *mut Object, tr.app_hide_others);
            set_title(handles.app_show_all as *mut Object, tr.app_show_all);
            set_title(handles.app_quit as *mut Object, tr.app_quit);

            set_title(handles.file_menu_item as *mut Object, tr.menu_file);
            set_title(handles.file_menu as *mut Object, tr.menu_file);
            set_title(handles.file_new_window as *mut Object, tr.file_new_window);
            set_title(handles.file_new_tab as *mut Object, tr.file_new_tab);
            set_title(
                handles.file_split_horizontal as *mut Object,
                tr.file_split_horizontal,
            );
            set_title(
                handles.file_split_vertical as *mut Object,
                tr.file_split_vertical,
            );
            set_title(handles.file_close_tab as *mut Object, tr.file_close_tab);
            set_title(
                handles.file_close_window as *mut Object,
                tr.file_close_window,
            );

            set_title(handles.edit_menu_item as *mut Object, tr.menu_edit);
            set_title(handles.edit_menu as *mut Object, tr.menu_edit);
            set_title(handles.edit_undo as *mut Object, tr.edit_undo);
            set_title(handles.edit_redo as *mut Object, tr.edit_redo);
            set_title(handles.edit_cut as *mut Object, tr.edit_cut);
            set_title(handles.edit_copy as *mut Object, tr.edit_copy);
            set_title(handles.edit_paste as *mut Object, tr.edit_paste);
            set_title(handles.edit_select_all as *mut Object, tr.edit_select_all);
            set_title(handles.edit_find as *mut Object, tr.edit_find);
            set_title(
                handles.edit_clear_scrollback as *mut Object,
                tr.edit_clear_scrollback,
            );

            set_title(handles.help_menu_item as *mut Object, tr.menu_help);
            set_title(handles.help_menu as *mut Object, tr.menu_help);
            set_title(handles.help_github as *mut Object, tr.help_github);
            set_title(
                handles.help_report_issue as *mut Object,
                tr.help_report_issue,
            );
            set_title(handles.help_open_config as *mut Object, tr.help_open_config);

            let main_menu = handles.main_menu as *mut Object;
            if !main_menu.is_null() {
                let _: () = msg_send![main_menu, update];
            }
            return;
        }

        let ns_app: *mut Object = msg_send![class!(NSApplication), sharedApplication];
        if ns_app.is_null() {
            return;
        }

        let main_menu = create_menu("MainMenu");

        // ====================================================================
        // 1. Application Menu (skTerm)
        // ====================================================================
        let app_menu_item: *mut Object = msg_send![class!(NSMenuItem), new];
        set_title(app_menu_item, "skTerm");
        let app_menu = create_menu("skTerm");

        let app_about = create_selector_item(
            tr.app_about,
            sel!(orderFrontStandardAboutPanel:),
            ns_app,
            "",
            0,
        );
        let _: () = msg_send![app_menu, addItem: app_about];
        let _: () = msg_send![app_menu, addItem: create_separator()];

        let app_preferences = create_action_item(
            tr.app_preferences,
            NativeMenuAction::OpenPreferences,
            ",",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![app_menu, addItem: app_preferences];
        let _: () = msg_send![app_menu, addItem: create_separator()];

        let app_services_item: *mut Object = msg_send![class!(NSMenuItem), new];
        set_title(app_services_item, tr.app_services);
        let existing_services: *mut Object = msg_send![ns_app, servicesMenu];
        let app_services_menu = if !existing_services.is_null() {
            let old_main: *mut Object = msg_send![ns_app, mainMenu];
            detach_submenu_from_existing_menus(old_main, existing_services);
            set_title(existing_services, tr.app_services);
            existing_services
        } else {
            let sm = create_menu(tr.app_services);
            let _: () = msg_send![ns_app, setServicesMenu: sm];
            sm
        };
        let _: () = msg_send![app_services_item, setSubmenu: app_services_menu];
        let _: () = msg_send![app_menu, addItem: app_services_item];
        let _: () = msg_send![app_menu, addItem: create_separator()];

        let app_hide = create_selector_item(
            tr.app_hide,
            sel!(hide:),
            ns_app,
            "h",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![app_menu, addItem: app_hide];

        let app_hide_others = create_selector_item(
            tr.app_hide_others,
            sel!(hideOtherApplications:),
            ns_app,
            "h",
            NSEVENT_MODIFIER_COMMAND | NSEVENT_MODIFIER_OPTION,
        );
        let _: () = msg_send![app_menu, addItem: app_hide_others];

        let app_show_all = create_selector_item(
            tr.app_show_all,
            sel!(unhideAllApplications:),
            ns_app,
            "",
            0,
        );
        let _: () = msg_send![app_menu, addItem: app_show_all];
        let _: () = msg_send![app_menu, addItem: create_separator()];

        let app_quit = create_action_item(
            tr.app_quit,
            NativeMenuAction::Quit,
            "q",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![app_menu, addItem: app_quit];

        let _: () = msg_send![app_menu_item, setSubmenu: app_menu];
        let _: () = msg_send![main_menu, addItem: app_menu_item];

        // ====================================================================
        // 2. File Menu (檔案 / File)
        // ====================================================================
        let file_menu_item: *mut Object = msg_send![class!(NSMenuItem), new];
        set_title(file_menu_item, tr.menu_file);
        let file_menu = create_menu(tr.menu_file);

        let file_new_window = create_action_item(
            tr.file_new_window,
            NativeMenuAction::NewWindow,
            "n",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![file_menu, addItem: file_new_window];

        let file_new_tab = create_action_item(
            tr.file_new_tab,
            NativeMenuAction::NewTab,
            "t",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![file_menu, addItem: file_new_tab];
        let _: () = msg_send![file_menu, addItem: create_separator()];

        let file_split_horizontal = create_action_item(
            tr.file_split_horizontal,
            NativeMenuAction::SplitHorizontal,
            "d",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![file_menu, addItem: file_split_horizontal];

        let file_split_vertical = create_action_item(
            tr.file_split_vertical,
            NativeMenuAction::SplitVertical,
            "d",
            NSEVENT_MODIFIER_COMMAND | NSEVENT_MODIFIER_SHIFT,
        );
        let _: () = msg_send![file_menu, addItem: file_split_vertical];
        let _: () = msg_send![file_menu, addItem: create_separator()];

        let file_close_tab = create_action_item(
            tr.file_close_tab,
            NativeMenuAction::CloseTabOrPane,
            "w",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![file_menu, addItem: file_close_tab];

        let file_close_window = create_action_item(
            tr.file_close_window,
            NativeMenuAction::CloseWindow,
            "w",
            NSEVENT_MODIFIER_COMMAND | NSEVENT_MODIFIER_SHIFT,
        );
        let _: () = msg_send![file_menu, addItem: file_close_window];

        let _: () = msg_send![file_menu_item, setSubmenu: file_menu];
        let _: () = msg_send![main_menu, addItem: file_menu_item];

        // ====================================================================
        // 3. Edit Menu (編輯 / Edit)
        // ====================================================================
        let edit_menu_item: *mut Object = msg_send![class!(NSMenuItem), new];
        set_title(edit_menu_item, tr.menu_edit);
        let edit_menu = create_menu(tr.menu_edit);

        let edit_undo = create_selector_item(
            tr.edit_undo,
            sel!(undo:),
            std::ptr::null_mut(),
            "z",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_undo];

        let edit_redo = create_selector_item(
            tr.edit_redo,
            sel!(redo:),
            std::ptr::null_mut(),
            "z",
            NSEVENT_MODIFIER_COMMAND | NSEVENT_MODIFIER_SHIFT,
        );
        let _: () = msg_send![edit_menu, addItem: edit_redo];
        let _: () = msg_send![edit_menu, addItem: create_separator()];

        let edit_cut = create_selector_item(
            tr.edit_cut,
            sel!(cut:),
            std::ptr::null_mut(),
            "x",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_cut];

        let edit_copy = create_action_item(
            tr.edit_copy,
            NativeMenuAction::Copy,
            "c",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_copy];

        let edit_paste = create_action_item(
            tr.edit_paste,
            NativeMenuAction::Paste,
            "v",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_paste];

        let edit_select_all = create_action_item(
            tr.edit_select_all,
            NativeMenuAction::SelectAll,
            "a",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_select_all];
        let _: () = msg_send![edit_menu, addItem: create_separator()];

        let edit_find = create_action_item(
            tr.edit_find,
            NativeMenuAction::Find,
            "f",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_find];

        let edit_clear_scrollback = create_action_item(
            tr.edit_clear_scrollback,
            NativeMenuAction::ClearScrollback,
            "k",
            NSEVENT_MODIFIER_COMMAND,
        );
        let _: () = msg_send![edit_menu, addItem: edit_clear_scrollback];

        let _: () = msg_send![edit_menu_item, setSubmenu: edit_menu];
        let _: () = msg_send![main_menu, addItem: edit_menu_item];

        // ====================================================================
        // 4. Help Menu (輔助說明 / Help)
        // ====================================================================
        let help_menu_item: *mut Object = msg_send![class!(NSMenuItem), new];
        set_title(help_menu_item, tr.menu_help);
        let help_menu = create_menu(tr.menu_help);

        let help_github =
            create_action_item(tr.help_github, NativeMenuAction::OpenGitHub, "", 0);
        let _: () = msg_send![help_menu, addItem: help_github];

        let help_report_issue = create_action_item(
            tr.help_report_issue,
            NativeMenuAction::ReportIssue,
            "",
            0,
        );
        let _: () = msg_send![help_menu, addItem: help_report_issue];
        let _: () = msg_send![help_menu, addItem: create_separator()];

        let help_open_config = create_action_item(
            tr.help_open_config,
            NativeMenuAction::OpenConfigFile,
            "",
            0,
        );
        let _: () = msg_send![help_menu, addItem: help_open_config];

        let _: () = msg_send![help_menu_item, setSubmenu: help_menu];
        let _: () = msg_send![main_menu, addItem: help_menu_item];

        let _: () = msg_send![ns_app, setMainMenu: main_menu];
        let _: () = msg_send![ns_app, setHelpMenu: help_menu];

        *guard = Some(MacMenuHandles {
            main_menu: main_menu as usize,
            app_about: app_about as usize,
            app_preferences: app_preferences as usize,
            app_services_item: app_services_item as usize,
            app_services_menu: app_services_menu as usize,
            app_hide: app_hide as usize,
            app_hide_others: app_hide_others as usize,
            app_show_all: app_show_all as usize,
            app_quit: app_quit as usize,
            file_menu_item: file_menu_item as usize,
            file_menu: file_menu as usize,
            file_new_window: file_new_window as usize,
            file_new_tab: file_new_tab as usize,
            file_split_horizontal: file_split_horizontal as usize,
            file_split_vertical: file_split_vertical as usize,
            file_close_tab: file_close_tab as usize,
            file_close_window: file_close_window as usize,
            edit_menu_item: edit_menu_item as usize,
            edit_menu: edit_menu as usize,
            edit_undo: edit_undo as usize,
            edit_redo: edit_redo as usize,
            edit_cut: edit_cut as usize,
            edit_copy: edit_copy as usize,
            edit_paste: edit_paste as usize,
            edit_select_all: edit_select_all as usize,
            edit_find: edit_find as usize,
            edit_clear_scrollback: edit_clear_scrollback as usize,
            help_menu_item: help_menu_item as usize,
            help_menu: help_menu as usize,
            help_github: help_github as usize,
            help_report_issue: help_report_issue as usize,
            help_open_config: help_open_config as usize,
        });
    }
}
