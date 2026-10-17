//! Lightweight, strongly-typed internationalization (i18n) framework for skTerm.
//!
//! Provides compile-time checked translation tables (`ZhTw` and `En`) with zero
//! heap allocations on lookup, automatic OS locale detection, and live switching.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

/// Supported UI locales in skTerm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum Locale {
    #[default]
    ZhTw = 0,
    En = 1,
}

static CURRENT_LOCALE: AtomicU8 = AtomicU8::new(Locale::ZhTw as u8);

impl Locale {
    /// Parse a BCP-47 or POSIX locale tag (e.g. `"zh-TW"`, `"zh_TW.UTF-8"`, `"zh-Hant-TW"`, `"en-US"`)
    /// into a supported [`Locale`]. Returns `None` for `"auto"` or unrecognized tags.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let cleaned = tag
            .trim()
            .split('.')
            .next()
            .unwrap_or("")
            .split('@')
            .next()
            .unwrap_or("")
            .trim()
            .to_lowercase();

        if cleaned.is_empty() || cleaned == "auto" || cleaned == "c" || cleaned == "posix" {
            return None;
        }

        if cleaned.starts_with("zh") {
            Some(Self::ZhTw)
        } else if cleaned.starts_with("en") {
            Some(Self::En)
        } else {
            None
        }
    }

    /// Detect the user's preferred system locale from the OS or environment.
    pub fn detect_system() -> Self {
        #[cfg(target_os = "macos")]
        {
            if let Some(mac_lang) = detect_macos_preferred_language() {
                if let Some(loc) = Self::from_tag(&mac_lang) {
                    return loc;
                }
            }
        }

        for var in ["LC_ALL", "LC_MESSAGES", "LANG"] {
            if let Ok(val) = std::env::var(var) {
                if let Some(loc) = Self::from_tag(&val) {
                    return loc;
                }
            }
        }

        Self::ZhTw
    }

    /// Resolve a configuration language string (`"auto"`, `"zh-TW"`, `"en"`) into an active [`Locale`].
    pub fn resolve(config_lang: &str) -> Self {
        let trimmed = config_lang.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
            Self::detect_system()
        } else {
            Self::from_tag(trimmed).unwrap_or(Self::ZhTw)
        }
    }

    /// Canonical configuration string for this locale.
    pub const fn code(self) -> &'static str {
        match self {
            Self::ZhTw => "zh-TW",
            Self::En => "en",
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_preferred_language() -> Option<String> {
    use objc::runtime::Object;
    use objc::{class, msg_send, sel, sel_impl};
    use std::ffi::CStr;

    unsafe {
        let langs: *mut Object = msg_send![class!(NSLocale), preferredLanguages];
        if langs.is_null() {
            return None;
        }
        let count: usize = msg_send![langs, count];
        if count == 0 {
            return None;
        }
        let first: *mut Object = msg_send![langs, objectAtIndex: 0usize];
        if first.is_null() {
            return None;
        }
        let utf8: *const std::os::raw::c_char = msg_send![first, UTF8String];
        if utf8.is_null() {
            return None;
        }
        Some(CStr::from_ptr(utf8).to_string_lossy().into_owned())
    }
}

/// Returns the currently active process-wide [`Locale`].
#[inline]
pub fn current_locale() -> Locale {
    match CURRENT_LOCALE.load(Ordering::Relaxed) {
        1 => Locale::En,
        _ => Locale::ZhTw,
    }
}

/// Updates the process-wide active [`Locale`].
#[inline]
pub fn set_locale(locale: Locale) {
    CURRENT_LOCALE.store(locale as u8, Ordering::Relaxed);
}

/// Returns the static translation table for the currently active [`Locale`].
#[inline]
pub fn tr() -> &'static I18nStrings {
    strings(current_locale())
}

/// Returns the static translation table for the specified [`Locale`].
#[inline]
pub const fn strings(locale: Locale) -> &'static I18nStrings {
    match locale {
        Locale::ZhTw => &ZH_TW,
        Locale::En => &EN,
    }
}

/// Complete strongly-typed UI string table for menus and language preferences.
#[derive(Debug, Clone, Copy)]
pub struct I18nStrings {
    // macOS Application Menu (skTerm)
    pub app_about: &'static str,
    pub app_preferences: &'static str,
    pub app_services: &'static str,
    pub app_hide: &'static str,
    pub app_hide_others: &'static str,
    pub app_show_all: &'static str,
    pub app_quit: &'static str,

    // macOS File Menu
    pub menu_file: &'static str,
    pub file_new_window: &'static str,
    pub file_new_tab: &'static str,
    pub file_split_horizontal: &'static str,
    pub file_split_vertical: &'static str,
    pub file_close_tab: &'static str,
    pub file_close_window: &'static str,

    // macOS Edit Menu
    pub menu_edit: &'static str,
    pub edit_undo: &'static str,
    pub edit_redo: &'static str,
    pub edit_cut: &'static str,
    pub edit_copy: &'static str,
    pub edit_paste: &'static str,
    pub edit_select_all: &'static str,
    pub edit_find: &'static str,
    pub edit_clear_scrollback: &'static str,

    // macOS View Menu
    pub menu_view: &'static str,
    pub view_toggle_ai_sidebar: &'static str,
    pub view_prev_tab: &'static str,
    pub view_next_tab: &'static str,
    pub view_move_tab_left: &'static str,
    pub view_move_tab_right: &'static str,
    pub view_focus_prev_pane: &'static str,
    pub view_focus_next_pane: &'static str,
    pub view_zoom_pane: &'static str,
    pub view_equalize_panes: &'static str,
    pub view_zoom_in: &'static str,
    pub view_zoom_out: &'static str,
    pub view_reset_zoom: &'static str,
    pub view_toggle_fullscreen: &'static str,

    // macOS Help Menu
    pub menu_help: &'static str,
    pub help_github: &'static str,
    pub help_report_issue: &'static str,
    pub help_open_config: &'static str,

    // In-window Top Dropdown & Context Menu
    pub ctx_preferences: &'static str,
    pub ctx_new_tab_default: &'static str,
    pub ctx_new_tab: &'static str,
    pub ctx_split_horizontal: &'static str,
    pub ctx_split_vertical: &'static str,
    pub ctx_toggle_ai_sidebar: &'static str,
    pub ctx_new_tab_bash: &'static str,
    pub ctx_new_tab_zsh: &'static str,
    pub ctx_new_tab_fish: &'static str,
    pub ctx_new_tab_cmd: &'static str,
    pub ctx_new_tab_win_powershell: &'static str,
    pub ctx_new_tab_pwsh: &'static str,
    pub ctx_new_tab_git_bash: &'static str,
    pub ctx_exit: &'static str,
    pub ctx_copy: &'static str,
    pub ctx_paste: &'static str,
    pub ctx_find: &'static str,
    pub ctx_clear_scrollback: &'static str,

    // Preferences Window Language Selector
    pub pref_language_label: &'static str,
    pub pref_language_subtitle: &'static str,
    pub pref_language_auto: &'static str,
    pub pref_language_zh_tw: &'static str,
    pub pref_language_en: &'static str,
}

pub const ZH_TW: I18nStrings = I18nStrings {
    app_about: "關於 skTerm",
    app_preferences: "偏好設定...",
    app_services: "服務",
    app_hide: "隱藏 skTerm",
    app_hide_others: "隱藏其他",
    app_show_all: "顯示全部",
    app_quit: "結束 skTerm",

    menu_file: "檔案",
    file_new_window: "新增視窗",
    file_new_tab: "新增分頁",
    file_split_horizontal: "左右分割窗格",
    file_split_vertical: "上下分割窗格",
    file_close_tab: "關閉分頁/窗格",
    file_close_window: "關閉視窗",

    menu_edit: "編輯",
    edit_undo: "復原",
    edit_redo: "重做",
    edit_cut: "剪下",
    edit_copy: "複製",
    edit_paste: "貼上",
    edit_select_all: "全選",
    edit_find: "尋找...",
    edit_clear_scrollback: "清除畫面",

    menu_view: "顯示方式",
    view_toggle_ai_sidebar: "切換 AI 代理側邊欄",
    view_prev_tab: "切換至上一個分頁",
    view_next_tab: "切換至下一個分頁",
    view_move_tab_left: "將分頁向左移動",
    view_move_tab_right: "將分頁向右移動",
    view_focus_prev_pane: "聚焦上一個窗格",
    view_focus_next_pane: "聚焦下一個窗格",
    view_zoom_pane: "放大/還原當前窗格",
    view_equalize_panes: "平均分配窗格大小",
    view_zoom_in: "放大字型",
    view_zoom_out: "縮小字型",
    view_reset_zoom: "實際大小",
    view_toggle_fullscreen: "進入/離開全螢幕",

    menu_help: "輔助說明",
    help_github: "GitHub 專案首頁",
    help_report_issue: "回報問題...",
    help_open_config: "開啟設定檔 (config.toml)",

    ctx_preferences: "⚙ 偏好設定...",
    ctx_new_tab_default: "＋ 新增分頁 (預設)",
    ctx_new_tab: "＋ 新增分頁",
    ctx_split_horizontal: "◫ 左右分割窗格",
    ctx_split_vertical: "⬒ 上下分割窗格",
    ctx_toggle_ai_sidebar: "🤖 切換 AI 側邊欄",
    ctx_new_tab_bash: "🐚 新增 Bash 分頁",
    ctx_new_tab_zsh: "⚡ 新增 Zsh 分頁",
    ctx_new_tab_fish: "🐟 新增 Fish 分頁",
    ctx_new_tab_cmd: "💻 命令提示字元",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ 結束 skTerm",
    ctx_copy: "📋 複製",
    ctx_paste: "📄 貼上",
    ctx_find: "🔍 搜尋...",
    ctx_clear_scrollback: "🧹 清除畫面",

    pref_language_label: "介面語言 (Language)",
    pref_language_subtitle: "控制系統選單列與快捷選單的顯示語言（即時生效）",
    pref_language_auto: "自動 (系統預設)",
    pref_language_zh_tw: "繁體中文",
    pref_language_en: "English",
};

pub const EN: I18nStrings = I18nStrings {
    app_about: "About skTerm",
    app_preferences: "Settings...",
    app_services: "Services",
    app_hide: "Hide skTerm",
    app_hide_others: "Hide Others",
    app_show_all: "Show All",
    app_quit: "Quit skTerm",

    menu_file: "File",
    file_new_window: "New Window",
    file_new_tab: "New Tab",
    file_split_horizontal: "Split Pane Left/Right",
    file_split_vertical: "Split Pane Top/Bottom",
    file_close_tab: "Close Tab/Pane",
    file_close_window: "Close Window",

    menu_edit: "Edit",
    edit_undo: "Undo",
    edit_redo: "Redo",
    edit_cut: "Cut",
    edit_copy: "Copy",
    edit_paste: "Paste",
    edit_select_all: "Select All",
    edit_find: "Find...",
    edit_clear_scrollback: "Clear Scrollback",

    menu_view: "View",
    view_toggle_ai_sidebar: "Toggle AI Agents Sidebar",
    view_prev_tab: "Show Previous Tab",
    view_next_tab: "Show Next Tab",
    view_move_tab_left: "Move Tab Left",
    view_move_tab_right: "Move Tab Right",
    view_focus_prev_pane: "Select Previous Pane",
    view_focus_next_pane: "Select Next Pane",
    view_zoom_pane: "Zoom Active Pane",
    view_equalize_panes: "Equalize Pane Sizes",
    view_zoom_in: "Zoom In",
    view_zoom_out: "Zoom Out",
    view_reset_zoom: "Actual Size",
    view_toggle_fullscreen: "Toggle Full Screen",

    menu_help: "Help",
    help_github: "GitHub Repository",
    help_report_issue: "Report an Issue...",
    help_open_config: "Open Config File (config.toml)",

    ctx_preferences: "⚙ Settings...",
    ctx_new_tab_default: "＋ New Tab (Default)",
    ctx_new_tab: "＋ New Tab",
    ctx_split_horizontal: "◫ Split Pane Left/Right",
    ctx_split_vertical: "⬒ Split Pane Top/Bottom",
    ctx_toggle_ai_sidebar: "🤖 Toggle AI Sidebar",
    ctx_new_tab_bash: "🐚 New Bash Tab",
    ctx_new_tab_zsh: "⚡ New Zsh Tab",
    ctx_new_tab_fish: "🐟 New Fish Tab",
    ctx_new_tab_cmd: "💻 Command Prompt",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ Quit skTerm",
    ctx_copy: "📋 Copy",
    ctx_paste: "📄 Paste",
    ctx_find: "🔍 Find...",
    ctx_clear_scrollback: "🧹 Clear Scrollback",

    pref_language_label: "Interface Language",
    pref_language_subtitle: "Display language for the menu bar and context menus (applied live)",
    pref_language_auto: "Auto (System)",
    pref_language_zh_tw: "繁體中文",
    pref_language_en: "English",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_from_tag() {
        assert_eq!(Locale::from_tag("zh-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh_TW.UTF-8"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-Hant-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("en"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en_GB.UTF-8"), Some(Locale::En));
        assert_eq!(Locale::from_tag("auto"), None);
        assert_eq!(Locale::from_tag(""), None);
        assert_eq!(Locale::from_tag("C"), None);
    }

    #[test]
    fn test_locale_strings_lookup() {
        let zh = strings(Locale::ZhTw);
        assert_eq!(zh.menu_file, "檔案");
        assert_eq!(zh.menu_edit, "編輯");
        assert_eq!(zh.menu_view, "顯示方式");
        assert_eq!(zh.menu_help, "輔助說明");
        assert_eq!(zh.file_new_window, "新增視窗");
        assert_eq!(zh.file_new_tab, "新增分頁");
        assert_eq!(zh.view_toggle_ai_sidebar, "切換 AI 代理側邊欄");
        assert_eq!(zh.view_prev_tab, "切換至上一個分頁");
        assert_eq!(zh.view_next_tab, "切換至下一個分頁");
        assert_eq!(zh.view_focus_prev_pane, "聚焦上一個窗格");
        assert_eq!(zh.view_focus_next_pane, "聚焦下一個窗格");

        let en = strings(Locale::En);
        assert_eq!(en.menu_file, "File");
        assert_eq!(en.menu_edit, "Edit");
        assert_eq!(en.menu_view, "View");
        assert_eq!(en.menu_help, "Help");
        assert_eq!(en.file_new_window, "New Window");
        assert_eq!(en.file_new_tab, "New Tab");
        assert_eq!(en.view_toggle_ai_sidebar, "Toggle AI Agents Sidebar");
        assert_eq!(en.view_prev_tab, "Show Previous Tab");
        assert_eq!(en.view_next_tab, "Show Next Tab");
        assert_eq!(en.view_focus_prev_pane, "Select Previous Pane");
        assert_eq!(en.view_focus_next_pane, "Select Next Pane");
    }
}
