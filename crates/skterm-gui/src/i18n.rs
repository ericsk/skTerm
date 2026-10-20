//! Lightweight, strongly-typed internationalization (i18n) framework for skTerm.
//!
//! Provides compile-time checked translation tables (`ZhTw`, `ZhCn`, `En`, `Ja`, `Ko`,
//! `Es`, `Fr`, and `De`) with zero heap allocations on lookup, automatic OS locale
//! detection, and live switching.

use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU8, Ordering};

/// Supported UI locales in skTerm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum Locale {
    #[default]
    ZhTw = 0,
    En = 1,
    ZhCn = 2,
    Ja = 3,
    Ko = 4,
    Es = 5,
    Fr = 6,
    De = 7,
}

static CURRENT_LOCALE: AtomicU8 = AtomicU8::new(Locale::ZhTw as u8);

impl Locale {
    /// Parse a BCP-47 or POSIX locale tag (e.g. `"zh-TW"`, `"zh-CN"`, `"zh_CN.UTF-8"`,
    /// `"zh-Hans-CN"`, `"zh-Hant-TW"`, `"en-US"`, `"ja-JP"`, `"ko-KR"`, `"es-ES"`,
    /// `"fr-FR"`, `"de-DE"`) into a supported [`Locale`].
    /// Returns `None` for `"auto"` or unrecognized tags.
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

        let primary = cleaned.split(['-', '_']).next().unwrap_or("");

        match primary {
            "zh" => {
                if cleaned.contains("hans") {
                    Some(Self::ZhCn)
                } else if cleaned.contains("hant") {
                    Some(Self::ZhTw)
                } else if cleaned.contains("cn") || cleaned.contains("sg") || cleaned.contains("my")
                {
                    Some(Self::ZhCn)
                } else {
                    Some(Self::ZhTw)
                }
            }
            "en" => Some(Self::En),
            "ja" | "jp" => Some(Self::Ja),
            "ko" | "kr" => Some(Self::Ko),
            "es" => Some(Self::Es),
            "fr" => Some(Self::Fr),
            "de" => Some(Self::De),
            _ => None,
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

    /// Resolve a configuration language string (`"auto"`, `"zh-TW"`, `"zh-CN"`, `"en"`,
    /// `"ja"`, `"ko"`, `"es"`, `"fr"`, `"de"`) into an active [`Locale`].
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
            Self::ZhCn => "zh-CN",
            Self::En => "en",
            Self::Ja => "ja",
            Self::Ko => "ko",
            Self::Es => "es",
            Self::Fr => "fr",
            Self::De => "de",
        }
    }
}

#[cfg(target_os = "macos")]
fn detect_macos_preferred_language() -> Option<String> {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use std::ffi::CStr;

    unsafe {
        let langs: *mut AnyObject = msg_send![class!(NSLocale), preferredLanguages];
        if langs.is_null() {
            return None;
        }
        let count: usize = msg_send![langs, count];
        if count == 0 {
            return None;
        }
        let first: *mut AnyObject = msg_send![langs, objectAtIndex: 0usize];
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
        2 => Locale::ZhCn,
        3 => Locale::Ja,
        4 => Locale::Ko,
        5 => Locale::Es,
        6 => Locale::Fr,
        7 => Locale::De,
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
        Locale::ZhCn => &ZH_CN,
        Locale::En => &EN,
        Locale::Ja => &JA,
        Locale::Ko => &KO,
        Locale::Es => &ES,
        Locale::Fr => &FR,
        Locale::De => &DE,
    }
}

/// Complete strongly-typed UI string table for menus and language preferences.
#[derive(Debug, Clone, Copy)]
pub struct I18nStrings {
    // macOS Application Menu (skTerm)
    pub app_about: &'static str,
    pub app_check_updates: &'static str,
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
    pub ctx_check_updates: &'static str,
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
    pub pref_language_zh_cn: &'static str,
    pub pref_language_en: &'static str,
    pub pref_language_ja: &'static str,
    pub pref_language_ko: &'static str,
    pub pref_language_es: &'static str,
    pub pref_language_fr: &'static str,
    pub pref_language_de: &'static str,
}

pub const ZH_TW: I18nStrings = I18nStrings {
    app_about: "關於 skTerm",
    app_check_updates: "檢查更新...",
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
    ctx_check_updates: "🔄 檢查更新...",
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
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const ZH_CN: I18nStrings = I18nStrings {
    app_about: "关于 skTerm",
    app_check_updates: "检查更新...",
    app_preferences: "偏好设置...",
    app_services: "服务",
    app_hide: "隐藏 skTerm",
    app_hide_others: "隐藏其他",
    app_show_all: "显示全部",
    app_quit: "退出 skTerm",

    menu_file: "文件",
    file_new_window: "新建窗口",
    file_new_tab: "新建标签页",
    file_split_horizontal: "左右拆分窗格",
    file_split_vertical: "上下拆分窗格",
    file_close_tab: "关闭标签页/窗格",
    file_close_window: "关闭窗口",

    menu_edit: "编辑",
    edit_undo: "撤销",
    edit_redo: "重做",
    edit_cut: "剪切",
    edit_copy: "复制",
    edit_paste: "粘贴",
    edit_select_all: "全选",
    edit_find: "查找...",
    edit_clear_scrollback: "清除屏幕",

    menu_view: "显示",
    view_toggle_ai_sidebar: "切换 AI 代理侧边栏",
    view_prev_tab: "切换至上一个标签页",
    view_next_tab: "切换至下一个标签页",
    view_move_tab_left: "将标签页向左移动",
    view_move_tab_right: "将标签页向右移动",
    view_focus_prev_pane: "聚焦上一个窗格",
    view_focus_next_pane: "聚焦下一个窗格",
    view_zoom_pane: "放大/还原当前窗格",
    view_equalize_panes: "平均分配窗格大小",
    view_zoom_in: "放大字体",
    view_zoom_out: "缩小字体",
    view_reset_zoom: "实际大小",
    view_toggle_fullscreen: "进入/退出全屏幕",

    menu_help: "帮助",
    help_github: "GitHub 项目首页",
    help_report_issue: "反馈问题...",
    help_open_config: "打开配置文件 (config.toml)",

    ctx_preferences: "⚙ 偏好设置...",
    ctx_check_updates: "🔄 检查更新...",
    ctx_new_tab_default: "＋ 新建标签页 (默认)",
    ctx_new_tab: "＋ 新建标签页",
    ctx_split_horizontal: "◫ 左右拆分窗格",
    ctx_split_vertical: "⬒ 上下拆分窗格",
    ctx_toggle_ai_sidebar: "🤖 切换 AI 侧边栏",
    ctx_new_tab_bash: "🐚 新建 Bash 标签页",
    ctx_new_tab_zsh: "⚡ 新建 Zsh 标签页",
    ctx_new_tab_fish: "🐟 新建 Fish 标签页",
    ctx_new_tab_cmd: "💻 命令提示符",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ 退出 skTerm",
    ctx_copy: "📋 复制",
    ctx_paste: "📄 粘贴",
    ctx_find: "🔍 查找...",
    ctx_clear_scrollback: "🧹 清除屏幕",

    pref_language_label: "界面语言 (Language)",
    pref_language_subtitle: "控制系统菜单栏与快捷菜单的显示语言（即时生效）",
    pref_language_auto: "自动 (系统默认)",
    pref_language_zh_tw: "繁體中文",
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const EN: I18nStrings = I18nStrings {
    app_about: "About skTerm",
    app_check_updates: "Check for Updates...",
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
    ctx_check_updates: "🔄 Check for Updates...",
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
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const JA: I18nStrings = I18nStrings {
    app_about: "skTerm について",
    app_check_updates: "アップデートを確認...",
    app_preferences: "設定...",
    app_services: "サービス",
    app_hide: "skTerm を非表示",
    app_hide_others: "ほかを非表示",
    app_show_all: "すべてを表示",
    app_quit: "skTerm を終了",

    menu_file: "ファイル",
    file_new_window: "新規ウインドウ",
    file_new_tab: "新規タブ",
    file_split_horizontal: "ペインを左右に分割",
    file_split_vertical: "ペインを上下に分割",
    file_close_tab: "タブ/ペインを閉じる",
    file_close_window: "ウインドウを閉じる",

    menu_edit: "編集",
    edit_undo: "取り消す",
    edit_redo: "やり直す",
    edit_cut: "カット",
    edit_copy: "コピー",
    edit_paste: "ペースト",
    edit_select_all: "すべてを選択",
    edit_find: "検索...",
    edit_clear_scrollback: "スクロールバックを消去",

    menu_view: "表示",
    view_toggle_ai_sidebar: "AI エージェントサイドバーを切り替え",
    view_prev_tab: "前のタブを表示",
    view_next_tab: "次のタブを表示",
    view_move_tab_left: "タブを左へ移動",
    view_move_tab_right: "タブを右へ移動",
    view_focus_prev_pane: "前のペインを選択",
    view_focus_next_pane: "次のペインを選択",
    view_zoom_pane: "現在のペインを拡大/復元",
    view_equalize_panes: "ペインサイズを均等化",
    view_zoom_in: "拡大",
    view_zoom_out: "縮小",
    view_reset_zoom: "実際のサイズ",
    view_toggle_fullscreen: "フルスクリーンを切り替え",

    menu_help: "ヘルプ",
    help_github: "GitHub リポジトリ",
    help_report_issue: "問題を報告...",
    help_open_config: "設定ファイルを開く (config.toml)",

    ctx_preferences: "⚙ 設定...",
    ctx_check_updates: "🔄 アップデートを確認...",
    ctx_new_tab_default: "＋ 新規タブ (デフォルト)",
    ctx_new_tab: "＋ 新規タブ",
    ctx_split_horizontal: "◫ 左右に分割",
    ctx_split_vertical: "⬒ 上下に分割",
    ctx_toggle_ai_sidebar: "🤖 AI サイドバー",
    ctx_new_tab_bash: "🐚 新規 Bash タブ",
    ctx_new_tab_zsh: "⚡ 新規 Zsh タブ",
    ctx_new_tab_fish: "🐟 新規 Fish タブ",
    ctx_new_tab_cmd: "💻 コマンド プロンプト",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ skTerm を終了",
    ctx_copy: "📋 コピー",
    ctx_paste: "📄 ペースト",
    ctx_find: "🔍 検索...",
    ctx_clear_scrollback: "🧹 画面を消去",

    pref_language_label: "表示言語 (Language)",
    pref_language_subtitle: "メニューバーとコンテキストメニューの表示言語（即時反映）",
    pref_language_auto: "自動 (システム設定)",
    pref_language_zh_tw: "繁體中文",
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const KO: I18nStrings = I18nStrings {
    app_about: "skTerm에 관하여",
    app_check_updates: "업데이트 확인...",
    app_preferences: "설정...",
    app_services: "서비스",
    app_hide: "skTerm 가리기",
    app_hide_others: "기타 가리기",
    app_show_all: "모두 보기",
    app_quit: "skTerm 종료",

    menu_file: "파일",
    file_new_window: "새로운 윈도우",
    file_new_tab: "새로운 탭",
    file_split_horizontal: "좌우로 패널 분할",
    file_split_vertical: "상하로 패널 분할",
    file_close_tab: "탭/패널 닫기",
    file_close_window: "윈도우 닫기",

    menu_edit: "편집",
    edit_undo: "실행 취소",
    edit_redo: "실행 복귀",
    edit_cut: "오려두기",
    edit_copy: "복사하기",
    edit_paste: "붙여넣기",
    edit_select_all: "전체 선택",
    edit_find: "찾기...",
    edit_clear_scrollback: "스크롤백 지우기",

    menu_view: "보기",
    view_toggle_ai_sidebar: "AI 에이전트 사이드바 토글",
    view_prev_tab: "이전 탭 보기",
    view_next_tab: "다음 탭 보기",
    view_move_tab_left: "탭을 왼쪽으로 이동",
    view_move_tab_right: "탭을 오른쪽으로 이동",
    view_focus_prev_pane: "이전 패널 선택",
    view_focus_next_pane: "다음 패널 선택",
    view_zoom_pane: "현재 패널 확대/복원",
    view_equalize_panes: "패널 크기 균등화",
    view_zoom_in: "확대",
    view_zoom_out: "축소",
    view_reset_zoom: "실제 크기",
    view_toggle_fullscreen: "전체 화면 토글",

    menu_help: "도움말",
    help_github: "GitHub 저장소",
    help_report_issue: "문제 보고...",
    help_open_config: "설정 파일 열기 (config.toml)",

    ctx_preferences: "⚙ 설정...",
    ctx_check_updates: "🔄 업데이트 확인...",
    ctx_new_tab_default: "＋ 새 탭 (기본)",
    ctx_new_tab: "＋ 새 탭",
    ctx_split_horizontal: "◫ 좌우 분할",
    ctx_split_vertical: "⬒ 상하 분할",
    ctx_toggle_ai_sidebar: "🤖 AI 사이드바",
    ctx_new_tab_bash: "🐚 새 Bash 탭",
    ctx_new_tab_zsh: "⚡ 새 Zsh 탭",
    ctx_new_tab_fish: "🐟 새 Fish 탭",
    ctx_new_tab_cmd: "💻 명령 프롬프트",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ skTerm 종료",
    ctx_copy: "📋 복사",
    ctx_paste: "📄 붙여넣기",
    ctx_find: "🔍 찾기...",
    ctx_clear_scrollback: "🧹 화면 지우기",

    pref_language_label: "인터페이스 언어 (Language)",
    pref_language_subtitle: "메뉴 막대 및 컨텍스트 메뉴 표시 언어 (즉시 적용)",
    pref_language_auto: "자동 (시스템 기본값)",
    pref_language_zh_tw: "繁體中文",
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const ES: I18nStrings = I18nStrings {
    app_about: "Acerca de skTerm",
    app_check_updates: "Buscar actualizaciones...",
    app_preferences: "Ajustes...",
    app_services: "Servicios",
    app_hide: "Ocultar skTerm",
    app_hide_others: "Ocultar otros",
    app_show_all: "Mostrar todo",
    app_quit: "Salir de skTerm",

    menu_file: "Archivo",
    file_new_window: "Nueva ventana",
    file_new_tab: "Nueva pestaña",
    file_split_horizontal: "Dividir panel izquierda/derecha",
    file_split_vertical: "Dividir panel arriba/abajo",
    file_close_tab: "Cerrar pestaña/panel",
    file_close_window: "Cerrar ventana",

    menu_edit: "Edición",
    edit_undo: "Deshacer",
    edit_redo: "Rehacer",
    edit_cut: "Cortar",
    edit_copy: "Copiar",
    edit_paste: "Pegar",
    edit_select_all: "Seleccionar todo",
    edit_find: "Buscar...",
    edit_clear_scrollback: "Limpiar historial",

    menu_view: "Visualización",
    view_toggle_ai_sidebar: "Mostrar/ocultar barra lateral de IA",
    view_prev_tab: "Mostrar pestaña anterior",
    view_next_tab: "Mostrar pestaña siguiente",
    view_move_tab_left: "Mover pestaña a la izquierda",
    view_move_tab_right: "Mover pestaña a la derecha",
    view_focus_prev_pane: "Seleccionar panel anterior",
    view_focus_next_pane: "Seleccionar panel siguiente",
    view_zoom_pane: "Ampliar/restaurar panel activo",
    view_equalize_panes: "Igualar tamaño de paneles",
    view_zoom_in: "Ampliar",
    view_zoom_out: "Reducir",
    view_reset_zoom: "Tamaño real",
    view_toggle_fullscreen: "Pantalla completa",

    menu_help: "Ayuda",
    help_github: "Repositorio en GitHub",
    help_report_issue: "Informar de un problema...",
    help_open_config: "Abrir archivo de configuración (config.toml)",

    ctx_preferences: "⚙ Ajustes...",
    ctx_check_updates: "🔄 Buscar actualizaciones...",
    ctx_new_tab_default: "＋ Nueva pestaña",
    ctx_new_tab: "＋ Nueva pestaña",
    ctx_split_horizontal: "◫ Dividir izq./der.",
    ctx_split_vertical: "⬒ Dividir arr./abj.",
    ctx_toggle_ai_sidebar: "🤖 Barra lateral IA",
    ctx_new_tab_bash: "🐚 Nueva pestaña Bash",
    ctx_new_tab_zsh: "⚡ Nueva pestaña Zsh",
    ctx_new_tab_fish: "🐟 Nueva pestaña Fish",
    ctx_new_tab_cmd: "💻 Símbolo del sistema",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ Salir de skTerm",
    ctx_copy: "📋 Copiar",
    ctx_paste: "📄 Pegar",
    ctx_find: "🔍 Buscar...",
    ctx_clear_scrollback: "🧹 Limpiar pantalla",

    pref_language_label: "Idioma de la interfaz",
    pref_language_subtitle: "Idioma de la barra de menús y menús contextuales (aplicación inmediata)",
    pref_language_auto: "Automático (Sistema)",
    pref_language_zh_tw: "繁體中文",
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const FR: I18nStrings = I18nStrings {
    app_about: "À propos de skTerm",
    app_check_updates: "Rechercher des mises à jour...",
    app_preferences: "Réglages...",
    app_services: "Services",
    app_hide: "Masquer skTerm",
    app_hide_others: "Masquer les autres",
    app_show_all: "Tout afficher",
    app_quit: "Quitter skTerm",

    menu_file: "Fichier",
    file_new_window: "Nouvelle fenêtre",
    file_new_tab: "Nouvel onglet",
    file_split_horizontal: "Scinder le volet gauche/droite",
    file_split_vertical: "Scinder le volet haut/bas",
    file_close_tab: "Fermer l’onglet/le volet",
    file_close_window: "Fermer la fenêtre",

    menu_edit: "Édition",
    edit_undo: "Annuler",
    edit_redo: "Rétablir",
    edit_cut: "Couper",
    edit_copy: "Copier",
    edit_paste: "Coller",
    edit_select_all: "Tout sélectionner",
    edit_find: "Rechercher...",
    edit_clear_scrollback: "Effacer l’historique",

    menu_view: "Présentation",
    view_toggle_ai_sidebar: "Afficher/masquer la barre latérale IA",
    view_prev_tab: "Afficher l’onglet précédent",
    view_next_tab: "Afficher l’onglet suivant",
    view_move_tab_left: "Déplacer l’onglet vers la gauche",
    view_move_tab_right: "Déplacer l’onglet vers la droite",
    view_focus_prev_pane: "Sélectionner le volet précédent",
    view_focus_next_pane: "Sélectionner le volet suivant",
    view_zoom_pane: "Agrandir/restaurer le volet actif",
    view_equalize_panes: "Égaliser la taille des volets",
    view_zoom_in: "Agrandir",
    view_zoom_out: "Réduire",
    view_reset_zoom: "Taille réelle",
    view_toggle_fullscreen: "Activer/désactiver le plein écran",

    menu_help: "Aide",
    help_github: "Dépôt GitHub",
    help_report_issue: "Signaler un problème...",
    help_open_config: "Ouvrir le fichier de configuration (config.toml)",

    ctx_preferences: "⚙ Réglages...",
    ctx_check_updates: "🔄 Mises à jour...",
    ctx_new_tab_default: "＋ Nouvel onglet",
    ctx_new_tab: "＋ Nouvel onglet",
    ctx_split_horizontal: "◫ Scinder gauche/droite",
    ctx_split_vertical: "⬒ Scinder haut/bas",
    ctx_toggle_ai_sidebar: "🤖 Barre latérale IA",
    ctx_new_tab_bash: "🐚 Nouvel onglet Bash",
    ctx_new_tab_zsh: "⚡ Nouvel onglet Zsh",
    ctx_new_tab_fish: "🐟 Nouvel onglet Fish",
    ctx_new_tab_cmd: "💻 Invite de commandes",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ Quitter skTerm",
    ctx_copy: "📋 Copier",
    ctx_paste: "📄 Coller",
    ctx_find: "🔍 Rechercher...",
    ctx_clear_scrollback: "🧹 Effacer l’écran",

    pref_language_label: "Langue de l’interface",
    pref_language_subtitle:
        "Langue de la barre des menus et des menus contextuels (effet immédiat)",
    pref_language_auto: "Automatique (Système)",
    pref_language_zh_tw: "繁體中文",
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

pub const DE: I18nStrings = I18nStrings {
    app_about: "Über skTerm",
    app_check_updates: "Nach Updates suchen...",
    app_preferences: "Einstellungen...",
    app_services: "Dienste",
    app_hide: "skTerm ausblenden",
    app_hide_others: "Andere ausblenden",
    app_show_all: "Alle einblenden",
    app_quit: "skTerm beenden",

    menu_file: "Ablage",
    file_new_window: "Neues Fenster",
    file_new_tab: "Neuer Tab",
    file_split_horizontal: "Bereich links/rechts teilen",
    file_split_vertical: "Bereich oben/unten teilen",
    file_close_tab: "Tab/Bereich schließen",
    file_close_window: "Fenster schließen",

    menu_edit: "Bearbeiten",
    edit_undo: "Widerrufen",
    edit_redo: "Wiederholen",
    edit_cut: "Ausschneiden",
    edit_copy: "Kopieren",
    edit_paste: "Einsetzen",
    edit_select_all: "Alles auswählen",
    edit_find: "Suchen...",
    edit_clear_scrollback: "Verlauf löschen",

    menu_view: "Darstellung",
    view_toggle_ai_sidebar: "KI-Agenten-Seitenleiste ein-/ausblenden",
    view_prev_tab: "Vorherigen Tab zeigen",
    view_next_tab: "Nächsten Tab zeigen",
    view_move_tab_left: "Tab nach links bewegen",
    view_move_tab_right: "Tab nach rechts bewegen",
    view_focus_prev_pane: "Vorherigen Bereich auswählen",
    view_focus_next_pane: "Nächsten Bereich auswählen",
    view_zoom_pane: "Aktiven Bereich maximieren/wiederherstellen",
    view_equalize_panes: "Bereichsgrößen angleichen",
    view_zoom_in: "Vergrößern",
    view_zoom_out: "Verkleinern",
    view_reset_zoom: "Originalgröße",
    view_toggle_fullscreen: "Vollbild ein-/ausblenden",

    menu_help: "Hilfe",
    help_github: "GitHub-Repository",
    help_report_issue: "Problem melden...",
    help_open_config: "Konfigurationsdatei öffnen (config.toml)",

    ctx_preferences: "⚙ Einstellungen...",
    ctx_check_updates: "🔄 Updates suchen...",
    ctx_new_tab_default: "＋ Neuer Tab (Standard)",
    ctx_new_tab: "＋ Neuer Tab",
    ctx_split_horizontal: "◫ Links/rechts teilen",
    ctx_split_vertical: "⬒ Oben/unten teilen",
    ctx_toggle_ai_sidebar: "🤖 KI-Seitenleiste",
    ctx_new_tab_bash: "🐚 Neuer Bash-Tab",
    ctx_new_tab_zsh: "⚡ Neuer Zsh-Tab",
    ctx_new_tab_fish: "🐟 Neuer Fish-Tab",
    ctx_new_tab_cmd: "💻 Eingabeaufforderung",
    ctx_new_tab_win_powershell: "⚡ Windows PowerShell",
    ctx_new_tab_pwsh: "🚀 PowerShell 7 (pwsh)",
    ctx_new_tab_git_bash: "🦝 Git Bash",
    ctx_exit: "✖ skTerm beenden",
    ctx_copy: "📋 Kopieren",
    ctx_paste: "📄 Einsetzen",
    ctx_find: "🔍 Suchen...",
    ctx_clear_scrollback: "🧹 Bildschirm löschen",

    pref_language_label: "Oberflächensprache",
    pref_language_subtitle: "Anzeigesprache für Menüleiste und Kontextmenüs (sofort wirksam)",
    pref_language_auto: "Automatisch (System)",
    pref_language_zh_tw: "繁體中文",
    pref_language_zh_cn: "简体中文",
    pref_language_en: "English",
    pref_language_ja: "日本語",
    pref_language_ko: "한국어",
    pref_language_es: "Español",
    pref_language_fr: "Français",
    pref_language_de: "Deutsch",
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_locale_from_tag() {
        assert_eq!(Locale::from_tag("zh-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh_TW.UTF-8"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-Hant-TW"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-Hant-HK"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-HK"), Some(Locale::ZhTw));
        assert_eq!(Locale::from_tag("zh-CN"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh_CN.UTF-8"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh-Hans"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh-Hans-CN"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh-Hans-HK"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("zh_SG.UTF-8"), Some(Locale::ZhCn));
        assert_eq!(Locale::from_tag("en"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en-US"), Some(Locale::En));
        assert_eq!(Locale::from_tag("en_GB.UTF-8"), Some(Locale::En));
        assert_eq!(Locale::from_tag("ja"), Some(Locale::Ja));
        assert_eq!(Locale::from_tag("ja-JP"), Some(Locale::Ja));
        assert_eq!(Locale::from_tag("ja_JP.UTF-8"), Some(Locale::Ja));
        assert_eq!(Locale::from_tag("ko"), Some(Locale::Ko));
        assert_eq!(Locale::from_tag("ko-KR"), Some(Locale::Ko));
        assert_eq!(Locale::from_tag("ko_KR.UTF-8"), Some(Locale::Ko));
        assert_eq!(Locale::from_tag("es"), Some(Locale::Es));
        assert_eq!(Locale::from_tag("es-ES"), Some(Locale::Es));
        assert_eq!(Locale::from_tag("es-MX"), Some(Locale::Es));
        assert_eq!(Locale::from_tag("es_ES.UTF-8"), Some(Locale::Es));
        assert_eq!(Locale::from_tag("fr"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("fr-FR"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("fr-CA"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("fr_FR.UTF-8"), Some(Locale::Fr));
        assert_eq!(Locale::from_tag("de"), Some(Locale::De));
        assert_eq!(Locale::from_tag("de-DE"), Some(Locale::De));
        assert_eq!(Locale::from_tag("de-AT"), Some(Locale::De));
        assert_eq!(Locale::from_tag("de_DE.UTF-8"), Some(Locale::De));
        assert_eq!(Locale::from_tag("auto"), None);
        assert_eq!(Locale::from_tag(""), None);
        assert_eq!(Locale::from_tag("C"), None);

        assert_eq!(Locale::ZhTw.code(), "zh-TW");
        assert_eq!(Locale::ZhCn.code(), "zh-CN");
        assert_eq!(Locale::En.code(), "en");
        assert_eq!(Locale::Ja.code(), "ja");
        assert_eq!(Locale::Ko.code(), "ko");
        assert_eq!(Locale::Es.code(), "es");
        assert_eq!(Locale::Fr.code(), "fr");
        assert_eq!(Locale::De.code(), "de");
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
        assert_eq!(zh.pref_language_zh_cn, "简体中文");

        let zh_cn = strings(Locale::ZhCn);
        assert_eq!(zh_cn.menu_file, "文件");
        assert_eq!(zh_cn.menu_edit, "编辑");
        assert_eq!(zh_cn.menu_view, "显示");
        assert_eq!(zh_cn.menu_help, "帮助");
        assert_eq!(zh_cn.file_new_window, "新建窗口");
        assert_eq!(zh_cn.file_new_tab, "新建标签页");
        assert_eq!(zh_cn.view_toggle_ai_sidebar, "切换 AI 代理侧边栏");
        assert_eq!(zh_cn.view_prev_tab, "切换至上一个标签页");
        assert_eq!(zh_cn.view_next_tab, "切换至下一个标签页");
        assert_eq!(zh_cn.view_focus_prev_pane, "聚焦上一个窗格");
        assert_eq!(zh_cn.view_focus_next_pane, "聚焦下一个窗格");
        assert_eq!(zh_cn.pref_language_zh_cn, "简体中文");

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
        assert_eq!(en.pref_language_zh_cn, "简体中文");

        let ja = strings(Locale::Ja);
        assert_eq!(ja.menu_file, "ファイル");
        assert_eq!(ja.menu_edit, "編集");
        assert_eq!(ja.menu_view, "表示");
        assert_eq!(ja.menu_help, "ヘルプ");
        assert_eq!(ja.pref_language_ja, "日本語");

        let ko = strings(Locale::Ko);
        assert_eq!(ko.menu_file, "파일");
        assert_eq!(ko.menu_edit, "편집");
        assert_eq!(ko.menu_view, "보기");
        assert_eq!(ko.menu_help, "도움말");
        assert_eq!(ko.pref_language_ko, "한국어");

        let es = strings(Locale::Es);
        assert_eq!(es.menu_file, "Archivo");
        assert_eq!(es.menu_edit, "Edición");
        assert_eq!(es.menu_view, "Visualización");
        assert_eq!(es.menu_help, "Ayuda");
        assert_eq!(es.pref_language_es, "Español");

        let fr = strings(Locale::Fr);
        assert_eq!(fr.menu_file, "Fichier");
        assert_eq!(fr.menu_edit, "Édition");
        assert_eq!(fr.menu_view, "Présentation");
        assert_eq!(fr.menu_help, "Aide");
        assert_eq!(fr.pref_language_fr, "Français");

        let de = strings(Locale::De);
        assert_eq!(de.menu_file, "Ablage");
        assert_eq!(de.menu_edit, "Bearbeiten");
        assert_eq!(de.menu_view, "Darstellung");
        assert_eq!(de.menu_help, "Hilfe");
        assert_eq!(de.pref_language_de, "Deutsch");
    }
}
