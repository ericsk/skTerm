//! Top dropdown menu, right-click context menu, and native OS menu action execution.

use super::events::NativeMenuAction;
use super::App;
use crate::i18n::{current_locale, strings, Locale};
use crate::layout::SplitDirection;
use skterm_render::{TopDropdownMenu, TopDropdownMenuItem};
use std::time::Instant;
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

/// Action triggered from the top dropdown menu or terminal right-click context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopMenuAction {
    Copy,
    Paste,
    Find,
    ClearScrollback,
    SplitHorizontal,
    SplitVertical,
    NewTabDefault,
    NewTabBash,
    NewTabZsh,
    NewTabFish,
    NewTabCmd,
    NewTabWinPowerShell,
    NewTabPwsh,
    NewTabGitBash,
    ToggleAiSidebar,
    OpenPreferences,
    Exit,
}

pub fn build_top_dropdown_menu(
    scale: f32,
    menu_x: f32,
    tab_bar_h: f32,
    hovered_idx: Option<usize>,
) -> (TopDropdownMenu, Vec<Option<TopMenuAction>>) {
    build_top_dropdown_menu_for_locale(current_locale(), scale, menu_x, tab_bar_h, hovered_idx)
}

pub fn build_top_dropdown_menu_for_locale(
    locale: Locale,
    scale: f32,
    menu_x: f32,
    tab_bar_h: f32,
    hovered_idx: Option<usize>,
) -> (TopDropdownMenu, Vec<Option<TopMenuAction>>) {
    let mut items = Vec::new();
    let mut actions = Vec::new();
    let is_mac = cfg!(target_os = "macos");
    let tr = strings(locale);

    // 1. Preferences
    items.push(TopDropdownMenuItem {
        label: tr.ctx_preferences.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+,".to_string()
        } else {
            "Ctrl+,".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::OpenPreferences));

    // Separator
    items.push(TopDropdownMenuItem {
        label: String::new(),
        shortcut: None,
        is_separator: true,
    });
    actions.push(None);

    // 2. Default New Tab
    items.push(TopDropdownMenuItem {
        label: tr.ctx_new_tab_default.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+T".to_string()
        } else {
            "Ctrl+Shift+T".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::NewTabDefault));

    // 3. Split Panes
    items.push(TopDropdownMenuItem {
        label: tr.ctx_split_horizontal.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+D".to_string()
        } else {
            "Ctrl+Shift+D".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitHorizontal));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_split_vertical.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+Shift+D".to_string()
        } else {
            "Ctrl+Shift+H".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitVertical));

    // 4. Toggle AI Sidebar
    items.push(TopDropdownMenuItem {
        label: tr.ctx_toggle_ai_sidebar.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+Shift+A".to_string()
        } else {
            "Ctrl+Shift+A".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::ToggleAiSidebar));

    // Shell Presets on Unix (Linux / macOS)
    #[cfg(not(target_os = "windows"))]
    {
        let presets = skterm_core::available_shell_presets();
        let has_bash = presets.iter().any(|p| p.is_available && p.id == "bash");
        let has_zsh = presets.iter().any(|p| p.is_available && p.id == "zsh");
        let has_fish = presets.iter().any(|p| p.is_available && p.id == "fish");

        if has_bash || has_zsh || has_fish {
            items.push(TopDropdownMenuItem {
                label: String::new(),
                shortcut: None,
                is_separator: true,
            });
            actions.push(None);
        }
        if has_bash {
            items.push(TopDropdownMenuItem {
                label: tr.ctx_new_tab_bash.to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabBash));
        }
        if has_zsh {
            items.push(TopDropdownMenuItem {
                label: tr.ctx_new_tab_zsh.to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabZsh));
        }
        if has_fish {
            items.push(TopDropdownMenuItem {
                label: tr.ctx_new_tab_fish.to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabFish));
        }
    }

    // Shell Presets (on Windows: Command Prompt, Windows PowerShell, PowerShell 7, Git Bash)
    #[cfg(target_os = "windows")]
    {
        items.push(TopDropdownMenuItem {
            label: String::new(),
            shortcut: None,
            is_separator: true,
        });
        actions.push(None);

        items.push(TopDropdownMenuItem {
            label: tr.ctx_new_tab_cmd.to_string(),
            shortcut: None,
            is_separator: false,
        });
        actions.push(Some(TopMenuAction::NewTabCmd));

        if skterm_core::find_windows_powershell().is_some() {
            items.push(TopDropdownMenuItem {
                label: tr.ctx_new_tab_win_powershell.to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabWinPowerShell));
        }

        if skterm_core::find_pwsh_path().is_some() {
            items.push(TopDropdownMenuItem {
                label: tr.ctx_new_tab_pwsh.to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabPwsh));
        }

        if skterm_core::find_git_bash_path().is_some() {
            items.push(TopDropdownMenuItem {
                label: tr.ctx_new_tab_git_bash.to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabGitBash));
        }
    }

    // Separator
    items.push(TopDropdownMenuItem {
        label: String::new(),
        shortcut: None,
        is_separator: true,
    });
    actions.push(None);

    // Exit
    items.push(TopDropdownMenuItem {
        label: tr.ctx_exit.to_string(),
        shortcut: None,
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Exit));

    let menu_y = if tab_bar_h > 0.0 {
        tab_bar_h
    } else {
        32.0 * scale
    };

    (
        TopDropdownMenu {
            x: menu_x,
            y: menu_y,
            width: 240.0 * scale,
            items,
            hovered_idx,
        },
        actions,
    )
}

/// Builds the right-click context menu anchored at `(anchor_x, anchor_y)` and clamped to screen bounds.
pub fn build_context_menu(
    scale: f32,
    anchor_x: f32,
    anchor_y: f32,
    screen_w: f32,
    screen_h: f32,
    hovered_idx: Option<usize>,
) -> (TopDropdownMenu, Vec<Option<TopMenuAction>>) {
    build_context_menu_for_locale(
        current_locale(),
        scale,
        anchor_x,
        anchor_y,
        screen_w,
        screen_h,
        hovered_idx,
    )
}

pub fn build_context_menu_for_locale(
    locale: Locale,
    scale: f32,
    anchor_x: f32,
    anchor_y: f32,
    screen_w: f32,
    screen_h: f32,
    hovered_idx: Option<usize>,
) -> (TopDropdownMenu, Vec<Option<TopMenuAction>>) {
    let mut items = Vec::new();
    let mut actions = Vec::new();
    let is_mac = cfg!(target_os = "macos");
    let tr = strings(locale);

    items.push(TopDropdownMenuItem {
        label: tr.ctx_copy.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+C".to_string()
        } else {
            "Ctrl+Shift+C".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Copy));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_paste.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+V".to_string()
        } else {
            "Ctrl+Shift+V".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Paste));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_find.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+F".to_string()
        } else {
            "Ctrl+Shift+F".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Find));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_clear_scrollback.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+K".to_string()
        } else {
            "Ctrl+Shift+K".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::ClearScrollback));

    items.push(TopDropdownMenuItem {
        label: String::new(),
        shortcut: None,
        is_separator: true,
    });
    actions.push(None);

    items.push(TopDropdownMenuItem {
        label: tr.ctx_split_horizontal.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+D".to_string()
        } else {
            "Ctrl+Shift+D".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitHorizontal));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_split_vertical.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+Shift+D".to_string()
        } else {
            "Ctrl+Shift+H".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitVertical));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_new_tab.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+T".to_string()
        } else {
            "Ctrl+Shift+T".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::NewTabDefault));

    items.push(TopDropdownMenuItem {
        label: tr.ctx_toggle_ai_sidebar.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+Shift+A".to_string()
        } else {
            "Ctrl+Shift+A".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::ToggleAiSidebar));

    items.push(TopDropdownMenuItem {
        label: String::new(),
        shortcut: None,
        is_separator: true,
    });
    actions.push(None);

    items.push(TopDropdownMenuItem {
        label: tr.ctx_preferences.to_string(),
        shortcut: Some(if is_mac {
            "Cmd+,".to_string()
        } else {
            "Ctrl+,".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::OpenPreferences));

    let mut menu = TopDropdownMenu {
        x: anchor_x,
        y: anchor_y,
        width: 240.0 * scale,
        items,
        hovered_idx,
    };
    menu.clamp_to_screen(screen_w, screen_h, scale);

    (menu, actions)
}

pub fn hit_test_dropdown_menu(
    menu: &TopDropdownMenu,
    scale: f32,
    mx: f32,
    my: f32,
) -> Option<usize> {
    if mx < menu.x || mx > menu.x + menu.width || my < menu.y {
        return None;
    }

    let row_h = 24.0 * scale;
    let sep_h = 6.0 * scale;
    let mut cur_y = menu.y + 3.0 * scale;

    for (idx, item) in menu.items.iter().enumerate() {
        let item_h = if item.is_separator { sep_h } else { row_h };
        if my >= cur_y && my <= cur_y + item_h {
            if item.is_separator {
                return None;
            }
            return Some(idx);
        }
        cur_y += item_h;
    }

    None
}

impl App {
    pub fn execute_top_menu_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        tab_bar_h: f32,
        action: TopMenuAction,
    ) {
        match action {
            TopMenuAction::Copy => {
                if let Some(session) = self.sessions.get(&window_id) {
                    session.copy_to_clipboard(self.clipboard.as_ref());
                }
            }
            TopMenuAction::Paste => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    session.paste_from_clipboard(self.clipboard.as_ref());
                }
            }
            TopMenuAction::Find => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    session.search_open = true;
                    session.update_search_matches();
                    session.window.request_redraw();
                }
            }
            TopMenuAction::ClearScrollback => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    if let Some(tab) = session.workspace.active_tab_mut() {
                        if let Some(terminal) = tab.active_terminal_mut() {
                            terminal.grid_mut().active_mut().clear_scrollback_and_screen();
                            let _ = terminal.write_to_pty(b"\x0c");
                            session.window.request_redraw();
                        }
                    }
                }
            }
            TopMenuAction::SplitHorizontal => {
                let pty_opts = self.build_pty_options();
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    let _ = session.workspace.split_active_pane(
                        SplitDirection::Horizontal,
                        24,
                        80,
                        scrollback,
                        pty_opts,
                    );
                    session.sync_all_pane_sizes(tab_bar_h);
                    session.window.request_redraw();
                }
            }
            TopMenuAction::SplitVertical => {
                let pty_opts = self.build_pty_options();
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::ToggleAiSidebar => {
                self.toggle_sidebar();
            }
            TopMenuAction::OpenPreferences => {
                self.open_preferences_window(event_loop);
            }
            TopMenuAction::NewTabDefault => {
                let pty_opts = self.build_pty_options();
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabBash => {
                let mut pty_opts = self.build_pty_options();
                let bash_path = if std::path::Path::new("/bin/bash").exists() {
                    "/bin/bash"
                } else {
                    "/usr/bin/bash"
                };
                pty_opts.shell = Some(bash_path.to_string());
                pty_opts.args = vec!["--login".to_string()];
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabZsh => {
                let mut pty_opts = self.build_pty_options();
                let zsh_path = if std::path::Path::new("/bin/zsh").exists() {
                    "/bin/zsh"
                } else {
                    "/usr/bin/zsh"
                };
                pty_opts.shell = Some(zsh_path.to_string());
                pty_opts.args = vec!["--login".to_string()];
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabFish => {
                let mut pty_opts = self.build_pty_options();
                let fish_path = [
                    "/usr/bin/fish",
                    "/usr/local/bin/fish",
                    "/opt/homebrew/bin/fish",
                    "/bin/fish",
                ]
                .into_iter()
                .find(|p| std::path::Path::new(p).exists())
                .unwrap_or("/usr/bin/fish");
                pty_opts.shell = Some(fish_path.to_string());
                pty_opts.args = vec!["--login".to_string()];
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabCmd => {
                #[allow(unused_mut)]
                let mut pty_opts = self.build_pty_options();
                #[cfg(target_os = "windows")]
                {
                    pty_opts.shell = Some(skterm_core::find_cmd());
                    pty_opts.args = vec![];
                }
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabWinPowerShell => {
                #[allow(unused_mut)]
                let mut pty_opts = self.build_pty_options();
                #[cfg(target_os = "windows")]
                {
                    if let Some(path) = skterm_core::find_windows_powershell() {
                        pty_opts.shell = Some(path);
                        pty_opts.args = vec!["-NoLogo".to_string()];
                    }
                }
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabPwsh => {
                #[allow(unused_mut)]
                let mut pty_opts = self.build_pty_options();
                #[cfg(target_os = "windows")]
                {
                    if let Some(path) = skterm_core::find_pwsh_path() {
                        pty_opts.shell = Some(path);
                        pty_opts.args = vec!["-NoLogo".to_string()];
                    }
                }
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::NewTabGitBash => {
                #[allow(unused_mut)]
                let mut pty_opts = self.build_pty_options();
                #[cfg(target_os = "windows")]
                {
                    if let Some(path) = skterm_core::find_git_bash_path() {
                        pty_opts.shell = Some(path);
                        pty_opts.args = vec!["--login".to_string(), "-i".to_string()];
                    }
                }
                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
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
            }
            TopMenuAction::Exit => {
                event_loop.exit();
            }
        }
    }

    /// Executes an action dispatched from a native OS menu bar item (such as macOS `NSMenu`).
    pub fn execute_native_menu_action(
        &mut self,
        event_loop: &ActiveEventLoop,
        action: NativeMenuAction,
    ) {
        self.last_native_menu_action_time = Some(Instant::now());

        let pref_focused = self
            .preferences_window
            .as_ref()
            .is_some_and(|p| p.window.has_focus());

        if pref_focused {
            match action {
                NativeMenuAction::CloseTabOrPane | NativeMenuAction::CloseWindow => {
                    self.preferences_window = None;
                    return;
                }
                NativeMenuAction::Copy => {
                    if let Some(pref) = &mut self.preferences_window {
                        pref.egui_ctx
                            .input_mut(|i| i.events.push(egui::Event::Copy));
                        pref.window.request_redraw();
                    }
                    return;
                }
                NativeMenuAction::Paste => {
                    let pasted = self
                        .clipboard
                        .as_ref()
                        .and_then(|cb| cb.lock().ok().and_then(|mut c| c.get_text().ok()));
                    if let (Some(pref), Some(text)) = (&mut self.preferences_window, pasted) {
                        pref.egui_ctx
                            .input_mut(|i| i.events.push(egui::Event::Paste(text)));
                        pref.window.request_redraw();
                    }
                    return;
                }
                NativeMenuAction::SelectAll => {
                    if let Some(pref) = &mut self.preferences_window {
                        pref.egui_ctx.input_mut(|i| {
                            i.events.push(egui::Event::Key {
                                key: egui::Key::A,
                                physical_key: Some(egui::Key::A),
                                pressed: true,
                                repeat: false,
                                modifiers: egui::Modifiers::COMMAND,
                            });
                        });
                        pref.window.request_redraw();
                    }
                    return;
                }
                _ => {}
            }
        }

        let target_wid = self
            .active_session_id
            .or_else(|| self.session_order.last().copied());
        let tab_bar_h = self.get_tab_bar_height();

        match action {
            NativeMenuAction::OpenPreferences => {
                self.open_preferences_window(event_loop);
            }
            NativeMenuAction::Quit => {
                event_loop.exit();
            }
            NativeMenuAction::NewWindow => {
                let _ = self.create_terminal_session(event_loop);
            }
            NativeMenuAction::NewTab => {
                if let Some(wid) = target_wid {
                    self.execute_top_menu_action(
                        event_loop,
                        wid,
                        tab_bar_h,
                        TopMenuAction::NewTabDefault,
                    );
                } else {
                    let _ = self.create_terminal_session(event_loop);
                }
            }
            NativeMenuAction::SplitHorizontal => {
                if let Some(wid) = target_wid {
                    self.execute_top_menu_action(
                        event_loop,
                        wid,
                        tab_bar_h,
                        TopMenuAction::SplitHorizontal,
                    );
                }
            }
            NativeMenuAction::SplitVertical => {
                if let Some(wid) = target_wid {
                    self.execute_top_menu_action(
                        event_loop,
                        wid,
                        tab_bar_h,
                        TopMenuAction::SplitVertical,
                    );
                }
            }
            NativeMenuAction::CloseTabOrPane => {
                if let Some(wid) = target_wid {
                    let all_panes_closed = if let Some(session) = self.sessions.get_mut(&wid) {
                        let closed = session.workspace.close_active_pane();
                        session.last_tab_label_refresh = None;
                        closed
                    } else {
                        false
                    };
                    if all_panes_closed {
                        self.handle_close_requested(event_loop, wid);
                    } else {
                        let new_tab_bar_h = self.get_tab_bar_height();
                        if let Some(session) = self.sessions.get_mut(&wid) {
                            session.sync_all_pane_sizes(new_tab_bar_h);
                            session.window.request_redraw();
                        }
                    }
                }
            }
            NativeMenuAction::CloseWindow => {
                if let Some(wid) = target_wid {
                    self.handle_close_requested(event_loop, wid);
                }
            }
            NativeMenuAction::Copy => {
                if let Some(wid) = target_wid {
                    self.execute_top_menu_action(event_loop, wid, tab_bar_h, TopMenuAction::Copy);
                }
            }
            NativeMenuAction::Paste => {
                if let Some(wid) = target_wid {
                    self.execute_top_menu_action(event_loop, wid, tab_bar_h, TopMenuAction::Paste);
                }
            }
            NativeMenuAction::SelectAll => {
                if let Some(wid) = target_wid {
                    if let Some(session) = self.sessions.get_mut(&wid) {
                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(term) = tab.active_terminal_mut() {
                                term.grid_mut().active_mut().select_all();
                                session.window.request_redraw();
                            }
                        }
                    }
                }
            }
            NativeMenuAction::Find => {
                if let Some(wid) = target_wid {
                    if let Some(session) = self.sessions.get_mut(&wid) {
                        session.search_open = !session.search_open;
                        if session.search_open {
                            session.update_search_matches();
                        } else {
                            session.search_matches.clear();
                            session.active_search_idx = None;
                        }
                        session.window.request_redraw();
                    }
                }
            }
            NativeMenuAction::ClearScrollback => {
                if let Some(wid) = target_wid {
                    self.execute_top_menu_action(
                        event_loop,
                        wid,
                        tab_bar_h,
                        TopMenuAction::ClearScrollback,
                    );
                }
            }
            NativeMenuAction::OpenGitHub => {
                let _ = super::input_mouse::open_url_in_browser("https://github.com/ericsk/skTerm");
            }
            NativeMenuAction::ReportIssue => {
                let _ = super::input_mouse::open_url_in_browser(
                    "https://github.com/ericsk/skTerm/issues",
                );
            }
            NativeMenuAction::OpenConfigFile => {
                self.config_manager.open_in_editor();
            }
        }
    }
}
