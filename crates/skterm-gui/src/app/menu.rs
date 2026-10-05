//! Top dropdown menu and right-click context menu model, builder, hit-testing, and action execution.

use super::App;
use crate::layout::SplitDirection;
use skterm_render::{TopDropdownMenu, TopDropdownMenuItem};
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
    let mut items = Vec::new();
    let mut actions = Vec::new();
    let is_mac = cfg!(target_os = "macos");

    // 1. Preferences
    items.push(TopDropdownMenuItem {
        label: "⚙ 偏好設定...".to_string(),
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
        label: "＋ 新增分頁 (預設)".to_string(),
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
        label: "◫ 左右分割窗格".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+D".to_string()
        } else {
            "Ctrl+Shift+D".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitHorizontal));

    items.push(TopDropdownMenuItem {
        label: "⬒ 上下分割窗格".to_string(),
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
        label: "🤖 切換 AI 側邊欄".to_string(),
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
                label: "🐚 新增 Bash 分頁".to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabBash));
        }
        if has_zsh {
            items.push(TopDropdownMenuItem {
                label: "⚡ 新增 Zsh 分頁".to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabZsh));
        }
        if has_fish {
            items.push(TopDropdownMenuItem {
                label: "🐟 新增 Fish 分頁".to_string(),
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
            label: "💻 命令提示字元".to_string(),
            shortcut: None,
            is_separator: false,
        });
        actions.push(Some(TopMenuAction::NewTabCmd));

        if skterm_core::find_windows_powershell().is_some() {
            items.push(TopDropdownMenuItem {
                label: "⚡ Windows PowerShell".to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabWinPowerShell));
        }

        if skterm_core::find_pwsh_path().is_some() {
            items.push(TopDropdownMenuItem {
                label: "🚀 PowerShell 7 (pwsh)".to_string(),
                shortcut: None,
                is_separator: false,
            });
            actions.push(Some(TopMenuAction::NewTabPwsh));
        }

        if skterm_core::find_git_bash_path().is_some() {
            items.push(TopDropdownMenuItem {
                label: "🦝 Git Bash".to_string(),
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
        label: "✖ 結束 skTerm".to_string(),
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
    let mut items = Vec::new();
    let mut actions = Vec::new();
    let is_mac = cfg!(target_os = "macos");

    items.push(TopDropdownMenuItem {
        label: "📋 複製".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+C".to_string()
        } else {
            "Ctrl+Shift+C".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Copy));

    items.push(TopDropdownMenuItem {
        label: "📄 貼上".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+V".to_string()
        } else {
            "Ctrl+Shift+V".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Paste));

    items.push(TopDropdownMenuItem {
        label: "🔍 搜尋...".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+F".to_string()
        } else {
            "Ctrl+Shift+F".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::Find));

    items.push(TopDropdownMenuItem {
        label: "🧹 清除畫面".to_string(),
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
        label: "◫ 左右分割窗格".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+D".to_string()
        } else {
            "Ctrl+Shift+D".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitHorizontal));

    items.push(TopDropdownMenuItem {
        label: "⬒ 上下分割窗格".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+Shift+D".to_string()
        } else {
            "Ctrl+Shift+H".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::SplitVertical));

    items.push(TopDropdownMenuItem {
        label: "＋ 新增分頁".to_string(),
        shortcut: Some(if is_mac {
            "Cmd+T".to_string()
        } else {
            "Ctrl+Shift+T".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::NewTabDefault));

    items.push(TopDropdownMenuItem {
        label: "🤖 切換 AI 側邊欄".to_string(),
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
        label: "⚙ 偏好設定...".to_string(),
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
}
