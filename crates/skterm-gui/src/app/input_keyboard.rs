//! Keyboard and IME event handling: shortcuts, inline editors, and PTY key encoding.

#[cfg(target_os = "macos")]
use super::macos::get_nswindow;
use super::App;
use crate::input::{check_tab_switch, normalize_shortcut_char};
use crate::layout::SplitDirection;
use std::time::{Duration, Instant};
use winit::event::{ElementState, Ime, KeyEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey};
use winit::window::WindowId;

impl App {
    /// Handles `WindowEvent::Ime` for a terminal window.
    pub(crate) fn handle_ime(&mut self, window_id: WindowId, tab_bar_h: f32, ime_event: Ime) {
        match ime_event {
            Ime::Commit(text) => {
                let had_preedit = !self.ime_preedit.is_empty();
                self.last_ime_commit_time = Some(Instant::now());
                self.ime_preedit.clear();
                self.ime_preedit_cursor = None;
                if self.modifiers.super_key() {
                    if text.eq_ignore_ascii_case("c") {
                        if let Some(session) = self.sessions.get(&window_id) {
                            session.copy_to_clipboard(self.clipboard.as_ref());
                        }
                    } else if text.eq_ignore_ascii_case("v") {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.paste_from_clipboard(self.clipboard.as_ref());
                        }
                    }
                    return;
                }
                if self.editing_agent_pane.is_some() {
                    self.insert_editing_text(&text);
                    if let Some(session) = self.sessions.get(&window_id) {
                        session.window.request_redraw();
                    }
                } else if let Some(session) = self.sessions.get_mut(&window_id) {
                    if session.search_open {
                        session.search_query.push_str(&text);
                        session.update_search_matches();
                        session.update_ime_position(tab_bar_h, &self.ime_preedit);
                        session.window.request_redraw();
                    } else if let Some(tab) = session.workspace.active_tab_mut() {
                        if let Some(term) = tab.active_terminal_mut() {
                            let was_scrolled = term.grid().active().scroll_offset != 0;
                            term.grid_mut().active_mut().scroll_offset = 0;
                            let _ = term.write_to_pty(text.as_bytes());
                            std::thread::yield_now();
                            let pty_updated = term.poll_pty();
                            session.update_ime_position(tab_bar_h, &self.ime_preedit);
                            if had_preedit || was_scrolled || pty_updated {
                                session.window.request_redraw();
                            }
                        }
                    }
                }
            }
            Ime::Preedit(text, cursor) => {
                self.ime_preedit = text;
                self.ime_preedit_cursor = cursor;
                if self.editing_agent_pane.is_none() {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if !session.search_open && !self.ime_preedit.is_empty() {
                            if let Some(tab) = session.workspace.active_tab_mut() {
                                if let Some(term) = tab.active_terminal_mut() {
                                    if term.grid().active().scroll_offset != 0 {
                                        term.grid_mut().active_mut().scroll_offset = 0;
                                    }
                                }
                            }
                        }
                        session.update_ime_position(tab_bar_h, &self.ime_preedit);
                        session.window.request_redraw();
                    }
                } else if let Some(session) = self.sessions.get(&window_id) {
                    session.window.request_redraw();
                }
            }
            Ime::Enabled => {
                if self.editing_agent_pane.is_none() {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        session.update_ime_position(tab_bar_h, &self.ime_preedit);
                    }
                }
            }
            Ime::Disabled => {
                let had_preedit = !self.ime_preedit.is_empty();
                self.ime_preedit.clear();
                self.ime_preedit_cursor = None;
                if had_preedit {
                    if let Some(session) = self.sessions.get(&window_id) {
                        session.window.request_redraw();
                    }
                }
            }
        }
    }

    /// Handles `WindowEvent::KeyboardInput` for a terminal window.
    pub(crate) fn handle_keyboard_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        tab_bar_h: f32,
        event: &KeyEvent,
    ) {
        if event.state != ElementState::Pressed {
            if let Some(session) = self.sessions.get_mut(&window_id) {
                if let Some(tab) = session.workspace.active_tab_mut() {
                    if let Some(term) = tab.active_terminal_mut() {
                        let kitty_flags = term.state.kitty_keyboard_flags();
                        if (kitty_flags & 2) != 0 {
                            if let Some(bytes) = crate::input::encode_key_event_with_modes(
                                event,
                                self.modifiers,
                                term.state.application_cursor_keys,
                                kitty_flags,
                                term.state.modify_other_keys,
                            ) {
                                let _ = term.write_to_pty(&bytes);
                            }
                        }
                    }
                }
            }
            return;
        }

        // 0. If top dropdown or right-click context menu is open, Escape dismisses it
        if event.logical_key == Key::Named(NamedKey::Escape) {
            if let Some(session) = self.sessions.get_mut(&window_id) {
                if session.menu_open {
                    session.menu_open = false;
                    session.menu_hovered_idx = None;
                    session.window.request_redraw();
                    return;
                }
            }
        }

        // 1. Tab Switching by Number (Ctrl+1~9 on Windows/Linux, Cmd+1~9 on macOS)
        if let Some(digit) = check_tab_switch(event, self.modifiers) {
            let target_idx = digit - 1;

            #[cfg(target_os = "macos")]
            {
                if self.session_order.len() > 1
                    && self
                        .sessions
                        .get(&window_id)
                        .is_none_or(|s| s.workspace.tabs.len() <= 1)
                {
                    let order_idx = if digit == 9 && self.session_order.len() < 9 {
                        self.session_order.len() - 1
                    } else {
                        target_idx
                    };
                    if let Some(&target_wid) = self.session_order.get(order_idx) {
                        self.active_session_id = Some(target_wid);
                        if let Some(target_session) = self.sessions.get_mut(&target_wid) {
                            target_session.window.focus_window();
                            let ns_win = get_nswindow(&target_session.window);
                            if !ns_win.is_null() {
                                use objc::{msg_send, sel, sel_impl};
                                let _: () = unsafe {
                                    msg_send![
                                        ns_win,
                                        makeKeyAndOrderFront: std::ptr::null::<objc::runtime::Object>()
                                    ]
                                };
                            }
                            target_session.sync_all_pane_sizes(tab_bar_h);
                            target_session.window.request_redraw();
                        }
                        return;
                    }
                }
            }

            if let Some(session) = self.sessions.get_mut(&window_id) {
                let final_idx = if target_idx < session.workspace.tabs.len() {
                    Some(target_idx)
                } else if digit == 9 && !session.workspace.tabs.is_empty() {
                    Some(session.workspace.tabs.len() - 1)
                } else {
                    None
                };

                if let Some(idx) = final_idx {
                    session.workspace.select_tab(idx);
                    session.sync_all_pane_sizes(tab_bar_h);
                    session.window.request_redraw();
                }
            }
            return;
        }

        // 2. Tab Switching by Ctrl+Tab / Ctrl+Shift+Tab on Windows/Linux
        #[cfg(not(target_os = "macos"))]
        if event.logical_key == Key::Named(NamedKey::Tab)
            && self.modifiers.control_key()
            && !self.modifiers.super_key()
            && !self.modifiers.alt_key()
        {
            if let Some(session) = self.sessions.get_mut(&window_id) {
                if self.modifiers.shift_key() {
                    session.workspace.prev_tab();
                } else {
                    session.workspace.next_tab();
                }
                session.sync_all_pane_sizes(tab_bar_h);
                session.window.request_redraw();
            }
            return;
        }

        // 3. Zoom / Reset Font Size (Ctrl + = / - / 0 on Windows/Linux, Cmd + = / - / 0 on macOS)
        #[cfg(target_os = "macos")]
        let is_zoom_or_reset = self.modifiers.super_key() && !self.modifiers.control_key();

        #[cfg(not(target_os = "macos"))]
        let is_zoom_or_reset = self.modifiers.control_key() && !self.modifiers.super_key();

        if is_zoom_or_reset {
            if let Some(s) = normalize_shortcut_char(&event.logical_key, event.physical_key) {
                match s.as_str() {
                    "+" | "=" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            if self.modifiers.alt_key() {
                                session.workspace.equalize_active_panes();
                            } else {
                                let cur_pt = session.renderer.font_size_pt;
                                let _ = session.renderer.update_font_size(cur_pt + 1.0);
                            }
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "-" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let cur_pt = session.renderer.font_size_pt;
                            let _ = session.renderer.update_font_size((cur_pt - 1.0).max(8.0));
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "0" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let def_pt = self.config_manager.current_config.font.size;
                            let _ = session.renderer.update_font_size(def_pt);
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    _ => {}
                }
            }
        }

        // Focus panes with Alt + [ / ] on Windows and Linux. The Ctrl+Shift
        // prefix below is already taken by tab switching, and plain Ctrl+[
        // has to stay available as ESC for vi and readline users.
        #[cfg(not(target_os = "macos"))]
        if self.modifiers.alt_key() && !self.modifiers.control_key() && !self.modifiers.super_key()
        {
            if let Some(ch) = normalize_shortcut_char(&event.logical_key, event.physical_key) {
                if ch == "[" || ch == "]" {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if ch == "[" {
                                tab.focus_prev();
                            } else {
                                tab.focus_next();
                            }
                        }
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                    }
                    return;
                }
            }
        }

        // Preferences Window Shortcut:
        // Windows/Linux: Ctrl + , (or CJK comma ， or <)
        // macOS: Cmd + ,
        let is_ctrl = self.modifiers.control_key() && !self.modifiers.alt_key();
        let is_super = self.modifiers.super_key();
        let is_comma = match &event.logical_key {
            Key::Character(s) => s == "," || s == "，" || s == "<",
            _ => matches!(event.physical_key, PhysicalKey::Code(KeyCode::Comma)),
        };
        if (is_ctrl || is_super) && is_comma {
            self.open_preferences_window(event_loop);
            return;
        }

        #[cfg(target_os = "macos")]
        let is_cmd = self.modifiers.super_key();

        #[cfg(not(target_os = "macos"))]
        let is_cmd = self.modifiers.control_key() && self.modifiers.shift_key();

        let is_shift = self.modifiers.shift_key();

        // Selects the alternate action of a shortcut (e.g. the other split
        // axis). macOS prefixes with Cmd alone, so Shift is still free to make
        // that choice; on Windows/Linux the prefix is Ctrl+Shift itself, which
        // pins `is_shift` to true, so those platforms get their own bindings
        // (such as Ctrl+Shift+H) instead of a Shift variant.
        #[cfg(target_os = "macos")]
        let is_alt_variant = is_shift;

        #[cfg(not(target_os = "macos"))]
        let is_alt_variant = false;

        if is_cmd {
            self.ime_preedit.clear();
            self.ime_preedit_cursor = None;
            // Check NamedKey first (Enter, Arrow keys)
            match event.logical_key {
                Key::Named(NamedKey::Enter) if is_shift => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        session.workspace.toggle_zoom_active_pane();
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                        return;
                    }
                }
                Key::Named(NamedKey::ArrowLeft) if self.modifiers.alt_key() => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        session.workspace.adjust_active_pane_ratio(-0.05);
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                        return;
                    }
                }
                Key::Named(NamedKey::ArrowRight) if self.modifiers.alt_key() => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        session.workspace.adjust_active_pane_ratio(0.05);
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                        return;
                    }
                }
                Key::Named(NamedKey::ArrowUp) if self.modifiers.alt_key() => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        session.workspace.adjust_active_pane_ratio(-0.05);
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                        return;
                    }
                }
                Key::Named(NamedKey::ArrowDown) if self.modifiers.alt_key() => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        session.workspace.adjust_active_pane_ratio(0.05);
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                        return;
                    }
                }
                Key::Named(NamedKey::PageUp) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                let page_lines = term.grid().active().rows.saturating_sub(2).max(1);
                                term.grid_mut()
                                    .active_mut()
                                    .scroll_display(page_lines as isize);
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                Key::Named(NamedKey::PageDown) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                let page_lines = term.grid().active().rows.saturating_sub(2).max(1);
                                term.grid_mut()
                                    .active_mut()
                                    .scroll_display(-(page_lines as isize));
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                Key::Named(NamedKey::Home) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                let max_scroll = term.grid().active().scrollback.len();
                                term.grid_mut().active_mut().scroll_offset = max_scroll;
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                Key::Named(NamedKey::End) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                term.grid_mut().active_mut().scroll_offset = 0;
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                _ => {}
            }

            if let Some(lower) = normalize_shortcut_char(&event.logical_key, event.physical_key) {
                match lower.as_str() {
                    "," => {
                        self.open_preferences_window(event_loop);
                        return;
                    }
                    "z" if is_shift => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.workspace.toggle_zoom_active_pane();
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                            return;
                        }
                    }
                    "a" if is_shift => {
                        self.toggle_sidebar();
                        return;
                    }
                    "t" => {
                        let pty_opts = self.build_pty_options();
                        let scrollback =
                            self.config_manager.current_config.terminal.scrollback_lines;
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let _ = session
                                .workspace
                                .new_tab_with_options(24, 80, scrollback, pty_opts);
                            session.last_tab_label_refresh = None;
                        }
                        let new_tab_bar_h = self.get_tab_bar_height();
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.sync_all_pane_sizes(new_tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "n" => {
                        let _ = self.create_terminal_session(event_loop);
                        return;
                    }
                    "w" => {
                        // Close Pane or Window Session
                        let all_panes_closed =
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                let closed = session.workspace.close_active_pane();
                                session.last_tab_label_refresh = None;
                                closed
                            } else {
                                false
                            };
                        if all_panes_closed {
                            self.sessions.remove(&window_id);
                            self.session_order.retain(|&id| id != window_id);
                            if self.sessions.is_empty() {
                                event_loop.exit();
                            }
                        } else {
                            let new_tab_bar_h = self.get_tab_bar_height();
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.sync_all_pane_sizes(new_tab_bar_h);
                                session.window.request_redraw();
                            }
                        }
                        return;
                    }
                    "d" => {
                        // Split Panes: Cmd+D / Ctrl+Shift+D = Left/Right,
                        // Cmd+Shift+D on macOS and Ctrl+Shift+H elsewhere = Top/Bottom
                        let dir = if is_alt_variant {
                            SplitDirection::Vertical
                        } else {
                            SplitDirection::Horizontal
                        };
                        let pty_opts = self.build_pty_options();
                        let scrollback =
                            self.config_manager.current_config.terminal.scrollback_lines;
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let _ = session
                                .workspace
                                .split_active_pane(dir, 24, 80, scrollback, pty_opts);
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    // Ctrl+Shift+H stands in for macOS' Cmd+Shift+D, which cannot
                    // be expressed here because the Ctrl+Shift prefix consumes Shift.
                    #[cfg(not(target_os = "macos"))]
                    "h" => {
                        let pty_opts = self.build_pty_options();
                        let scrollback =
                            self.config_manager.current_config.terminal.scrollback_lines;
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let _ = session.workspace.split_active_pane(
                                SplitDirection::Vertical,
                                24,
                                80,
                                scrollback,
                                pty_opts,
                            );
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "[" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            if self.modifiers.alt_key() {
                                session.workspace.move_tab_left();
                            } else if is_shift {
                                session.workspace.prev_tab();
                            } else if let Some(tab) = session.workspace.active_tab_mut() {
                                tab.focus_prev();
                            }
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "]" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            if self.modifiers.alt_key() {
                                session.workspace.move_tab_right();
                            } else if is_shift {
                                session.workspace.next_tab();
                            } else if let Some(tab) = session.workspace.active_tab_mut() {
                                tab.focus_next();
                            }
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "c" => {
                        if let Some(session) = self.sessions.get(&window_id) {
                            session.copy_to_clipboard(self.clipboard.as_ref());
                        }
                        return;
                    }
                    "v" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.paste_from_clipboard(self.clipboard.as_ref());
                        }
                        return;
                    }
                    "k" => {
                        // Clear active buffer and scrollback history (Cmd+K)
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            if let Some(tab) = session.workspace.active_tab_mut() {
                                if let Some(term) = tab.active_terminal_mut() {
                                    term.grid_mut().active_mut().clear_scrollback_and_screen();
                                    let _ = term.write_to_pty(b"\x0c");
                                    session.window.request_redraw();
                                }
                            }
                        }
                        return;
                    }
                    "f" => {
                        // Toggle Find in Scrollback bar (Cmd+F)
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.search_open = !session.search_open;
                            if session.search_open {
                                session.update_search_matches();
                            } else {
                                session.search_matches.clear();
                                session.active_search_idx = None;
                            }
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "r" if is_shift => {
                        // Rename active agent/pane task label (Cmd+Shift+R)
                        if !self.sidebar_open {
                            self.toggle_sidebar();
                        }
                        if let Some(session) = self.sessions.get(&window_id) {
                            if let Some(tab) = session.workspace.active_tab() {
                                let pane_id = tab.active_pane_id;
                                let current = tab
                                    .root
                                    .find_terminal(pane_id)
                                    .and_then(|t| t.custom_label.clone())
                                    .unwrap_or_default();
                                self.editing_agent_pane = Some((window_id, tab.id, pane_id));
                                self.editing_agent_text = current;
                                self.editing_agent_cursor = self.editing_agent_text.chars().count();
                            }
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "+" | "=" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            if self.modifiers.alt_key() {
                                session.workspace.equalize_active_panes();
                            } else {
                                let cur_pt = session.renderer.font_size_pt;
                                let _ = session.renderer.update_font_size(cur_pt + 1.0);
                            }
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "-" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let cur_pt = session.renderer.font_size_pt;
                            let _ = session.renderer.update_font_size((cur_pt - 1.0).max(8.0));
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    "0" => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            let def_pt = self.config_manager.current_config.font.size;
                            let _ = session.renderer.update_font_size(def_pt);
                            session.sync_all_pane_sizes(tab_bar_h);
                            session.window.request_redraw();
                        }
                        return;
                    }
                    _ => {}
                }
            }

            #[cfg(target_os = "macos")]
            return;
        }

        // Handle Agent Task Label Inline Rename input when editing_agent_pane is active
        if self.editing_agent_pane.is_some() {
            if event.state == ElementState::Pressed {
                // While IME preedit composition is active, let the IME handle keys
                if !self.ime_preedit.is_empty() {
                    return;
                }

                // If an IME commit occurred in the exact same event tick (<20ms),
                // ignore the trailing KeyboardInput (e.g. Enter that confirmed the IME word)
                let just_committed_ime = self
                    .last_ime_commit_time
                    .is_some_and(|t| t.elapsed() < Duration::from_millis(20));
                if just_committed_ime {
                    return;
                }

                match &event.logical_key {
                    Key::Named(NamedKey::Escape) => {
                        self.editing_agent_pane = None;
                        self.ime_preedit.clear();
                        self.ime_preedit_cursor = None;
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    Key::Named(NamedKey::Enter) => {
                        self.commit_agent_label_edit();
                        return;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        self.backspace_editing_text();
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    Key::Named(NamedKey::Delete) => {
                        self.delete_editing_text();
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    Key::Named(NamedKey::ArrowLeft) => {
                        if self.modifiers.super_key() {
                            self.editing_agent_cursor = 0;
                        } else {
                            self.editing_agent_cursor = self.editing_agent_cursor.saturating_sub(1);
                        }
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    Key::Named(NamedKey::ArrowRight) => {
                        let len = self.editing_agent_text.chars().count();
                        if self.modifiers.super_key() {
                            self.editing_agent_cursor = len;
                        } else {
                            self.editing_agent_cursor = (self.editing_agent_cursor + 1).min(len);
                        }
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    Key::Named(NamedKey::Home) => {
                        self.editing_agent_cursor = 0;
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    Key::Named(NamedKey::End) => {
                        self.editing_agent_cursor = self.editing_agent_text.chars().count();
                        if let Some(s) = self.sessions.get(&window_id) {
                            s.window.request_redraw();
                        }
                        return;
                    }
                    _ => {
                        if self.modifiers.control_key() {
                            if let Key::Character(ref s) = event.logical_key {
                                if s.eq_ignore_ascii_case("a") {
                                    self.editing_agent_cursor = 0;
                                    if let Some(sess) = self.sessions.get(&window_id) {
                                        sess.window.request_redraw();
                                    }
                                    return;
                                } else if s.eq_ignore_ascii_case("e") {
                                    self.editing_agent_cursor =
                                        self.editing_agent_text.chars().count();
                                    if let Some(sess) = self.sessions.get(&window_id) {
                                        sess.window.request_redraw();
                                    }
                                    return;
                                }
                            }
                        } else if !self.modifiers.super_key() {
                            if let Some(ref text) = event.text {
                                let filtered: String =
                                    text.chars().filter(|c| !c.is_control()).collect();
                                if !filtered.is_empty() {
                                    self.insert_editing_text(&filtered);
                                    if let Some(s) = self.sessions.get(&window_id) {
                                        s.window.request_redraw();
                                    }
                                    return;
                                }
                            }
                        }
                    }
                }
            }
            // Always consume all key events while editing_agent_pane is active so they never fall through to PTY
            return;
        }

        // Handle Find Bar input when search_open is active
        if let Some(session) = self.sessions.get_mut(&window_id) {
            if session.search_open {
                if event.state == ElementState::Pressed {
                    let just_committed_ime = self
                        .last_ime_commit_time
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(20));
                    if !self.ime_preedit.is_empty() || just_committed_ime {
                        return;
                    }

                    match &event.logical_key {
                        Key::Named(NamedKey::Escape) => {
                            session.search_open = false;
                            session.search_matches.clear();
                            session.active_search_idx = None;
                            session.window.request_redraw();
                            return;
                        }
                        Key::Named(NamedKey::Enter) => {
                            if self.modifiers.shift_key() {
                                session.prev_search_match();
                            } else {
                                session.next_search_match();
                            }
                            session.window.request_redraw();
                            return;
                        }
                        Key::Named(NamedKey::Backspace) => {
                            session.search_query.pop();
                            session.update_search_matches();
                            session.window.request_redraw();
                            return;
                        }
                        _ => {
                            if !self.modifiers.super_key() && !self.modifiers.control_key() {
                                if let Some(ref text) = event.text {
                                    let filtered: String =
                                        text.chars().filter(|c| !c.is_control()).collect();
                                    if !filtered.is_empty() {
                                        session.search_query.push_str(&filtered);
                                        session.update_search_matches();
                                        session.window.request_redraw();
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }
                return;
            }
        }

        // Shift+PageUp / Shift+PageDown / Shift+Home / Shift+End quick scrolling
        if is_shift && !self.modifiers.control_key() && !self.modifiers.alt_key() {
            match event.logical_key {
                Key::Named(NamedKey::PageUp) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                let page_lines = term.grid().active().rows.saturating_sub(2).max(1);
                                term.grid_mut()
                                    .active_mut()
                                    .scroll_display(page_lines as isize);
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                Key::Named(NamedKey::PageDown) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                let page_lines = term.grid().active().rows.saturating_sub(2).max(1);
                                term.grid_mut()
                                    .active_mut()
                                    .scroll_display(-(page_lines as isize));
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                Key::Named(NamedKey::Home) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                let max_scroll = term.grid().active().scrollback.len();
                                term.grid_mut().active_mut().scroll_offset = max_scroll;
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                Key::Named(NamedKey::End) => {
                    if let Some(session) = self.sessions.get_mut(&window_id) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                term.grid_mut().active_mut().scroll_offset = 0;
                                session.last_scroll_time = Some(Instant::now());
                                session.window.request_redraw();
                                return;
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        // Send Normal Key to PTY
        if let Some(session) = self.sessions.get_mut(&window_id) {
            if let Some(tab) = session.workspace.active_tab_mut() {
                if let Some(term) = tab.active_terminal_mut() {
                    let app_cursor = term.state.application_cursor_keys;
                    let kitty_flags = term.state.kitty_keyboard_flags();
                    let modify_other_keys = term.state.modify_other_keys;
                    if let Some(bytes) = crate::input::encode_key_event_with_modes(
                        event,
                        self.modifiers,
                        app_cursor,
                        kitty_flags,
                        modify_other_keys,
                    ) {
                        let was_scrolled = term.grid().active().scroll_offset != 0;
                        term.grid_mut().active_mut().scroll_offset = 0;
                        let _ = term.write_to_pty(&bytes);
                        std::thread::yield_now();
                        if was_scrolled || term.poll_pty() {
                            session.window.request_redraw();
                        }
                    }
                }
            }
        }
    }
}
