//! Top dropdown menu model, builder, hit-testing, and action execution.

use super::App;
use skterm_render::{TopDropdownMenu, TopDropdownMenuItem};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

/// Action triggered from the top dropdown menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopMenuAction {
    NewTabDefault,
    NewTabCmd,
    NewTabWinPowerShell,
    NewTabPwsh,
    NewTabGitBash,
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

    // 1. Preferences
    items.push(TopDropdownMenuItem {
        label: "⚙ 偏好設定...".to_string(),
        shortcut: Some(if cfg!(target_os = "macos") {
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
        shortcut: Some(if cfg!(target_os = "macos") {
            "Cmd+T".to_string()
        } else {
            "Ctrl+Shift+T".to_string()
        }),
        is_separator: false,
    });
    actions.push(Some(TopMenuAction::NewTabDefault));

    // Shell Presets (on Windows: Command Prompt, Windows PowerShell, PowerShell 7, Git Bash)
    #[cfg(target_os = "windows")]
    {
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

    (
        TopDropdownMenu {
            x: menu_x,
            y: tab_bar_h,
            width: 230.0 * scale,
            items,
            hovered_idx,
        },
        actions,
    )
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
                    session.sync_all_pane_sizes(tab_bar_h);
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
                    session.sync_all_pane_sizes(tab_bar_h);
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
                    session.sync_all_pane_sizes(tab_bar_h);
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
                    session.sync_all_pane_sizes(tab_bar_h);
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
                    session.sync_all_pane_sizes(tab_bar_h);
                    session.window.request_redraw();
                }
            }
            TopMenuAction::Exit => {
                event_loop.exit();
            }
        }
    }
}
