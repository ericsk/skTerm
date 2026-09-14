//! Configuration file parsing (TOML), theme presets, default template generation, and filesystem hot-reload.

use crossbeam_channel::{unbounded, Receiver, Sender};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use tracing::{info, warn};

/// Root Configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Config {
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub terminal: TerminalConfig,
    #[serde(default)]
    pub colors: ColorsConfig,
    #[serde(default)]
    pub cursor: CursorConfig,
    #[serde(default)]
    pub keybindings: KeybindingsConfig,
}

/// Font settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FontConfig {
    #[serde(default = "default_font_family")]
    pub family: String,
    #[serde(default = "default_font_size")]
    pub size: f32,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
    pub bold_family: Option<String>,
    pub italic_family: Option<String>,
    pub bold_italic_family: Option<String>,
    #[serde(default)]
    pub fallback_families: Vec<String>,
}

fn default_font_family() -> String {
    "Menlo".to_string()
}
fn default_font_size() -> f32 {
    16.0
}
fn default_line_height() -> f32 {
    1.2
}

impl Default for FontConfig {
    fn default() -> Self {
        Self {
            family: default_font_family(),
            size: default_font_size(),
            line_height: default_line_height(),
            bold_family: None,
            italic_family: None,
            bold_italic_family: None,
            fallback_families: Vec::new(),
        }
    }
}

/// Window appearance & dimensions settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_columns")]
    pub columns: u16,
    #[serde(default = "default_rows")]
    pub rows: u16,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_padding")]
    pub padding_x: f32,
    #[serde(default = "default_padding")]
    pub padding_y: f32,
    #[serde(default = "default_startup_mode")]
    pub startup_mode: String,
    #[serde(default = "default_decorations")]
    pub decorations: String,
    #[serde(default = "default_tab_bar_style")]
    pub tab_bar_style: String,
}

fn default_columns() -> u16 {
    80
}
fn default_rows() -> u16 {
    24
}
fn default_opacity() -> f32 {
    1.0
}
fn default_padding() -> f32 {
    8.0
}
fn default_startup_mode() -> String {
    "normal".to_string()
}
fn default_decorations() -> String {
    "full".to_string()
}
fn default_tab_bar_style() -> String {
    #[cfg(target_os = "macos")]
    {
        "auto".to_string()
    }
    #[cfg(not(target_os = "macos"))]
    {
        "always".to_string()
    }
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            columns: default_columns(),
            rows: default_rows(),
            opacity: default_opacity(),
            padding_x: default_padding(),
            padding_y: default_padding(),
            startup_mode: default_startup_mode(),
            decorations: default_decorations(),
            tab_bar_style: default_tab_bar_style(),
        }
    }
}

/// Terminal emulator & shell process settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerminalConfig {
    #[serde(default = "default_term")]
    pub term: String,
    pub shell: Option<ShellConfig>,
    pub working_directory: Option<String>,
    #[serde(default = "default_scrollback_lines")]
    pub scrollback_lines: usize,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default = "default_option_as_alt")]
    pub option_as_alt: String,
    #[serde(default = "default_bell")]
    pub bell: String,
}

fn default_term() -> String {
    "xterm-256color".to_string()
}
fn default_scrollback_lines() -> usize {
    10000
}
fn default_option_as_alt() -> String {
    "both".to_string()
}
fn default_bell() -> String {
    "visual".to_string()
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            term: default_term(),
            shell: None,
            working_directory: None,
            scrollback_lines: default_scrollback_lines(),
            env: HashMap::new(),
            option_as_alt: default_option_as_alt(),
            bell: default_bell(),
        }
    }
}

/// Shell executable and arguments
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShellConfig {
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// Color theme settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorsConfig {
    #[serde(default = "default_theme_name")]
    pub theme: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub cursor: Option<String>,
    pub selection: Option<String>,
    pub palette: Option<PaletteConfig>,
}

fn default_theme_name() -> String {
    "pure-black".to_string()
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            theme: default_theme_name(),
            foreground: None,
            background: None,
            cursor: None,
            selection: None,
            palette: None,
        }
    }
}

/// Custom 16-color ANSI Palette configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PaletteConfig {
    pub black: Option<String>,
    pub red: Option<String>,
    pub green: Option<String>,
    pub yellow: Option<String>,
    pub blue: Option<String>,
    pub magenta: Option<String>,
    pub cyan: Option<String>,
    pub white: Option<String>,
    pub bright_black: Option<String>,
    pub bright_red: Option<String>,
    pub bright_green: Option<String>,
    pub bright_yellow: Option<String>,
    pub bright_blue: Option<String>,
    pub bright_magenta: Option<String>,
    pub bright_cyan: Option<String>,
    pub bright_white: Option<String>,
}

/// Cursor visual settings
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CursorConfig {
    #[serde(default = "default_cursor_shape")]
    pub shape: String,
    #[serde(default = "default_cursor_blink")]
    pub blink: bool,
    #[serde(default = "default_blink_interval_ms")]
    pub blink_interval_ms: u64,
}

fn default_cursor_shape() -> String {
    "block".to_string()
}
fn default_cursor_blink() -> bool {
    true
}
fn default_blink_interval_ms() -> u64 {
    500
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            shape: default_cursor_shape(),
            blink: default_cursor_blink(),
            blink_interval_ms: default_blink_interval_ms(),
        }
    }
}

/// Keybindings configuration
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct KeybindingsConfig {
    #[serde(default)]
    pub bindings: HashMap<String, String>,
}

/// Theme color definition
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub foreground: [f32; 4],
    pub background: [f32; 4],
    pub cursor: [f32; 4],
    pub selection: [f32; 4],
    pub palette: [[f32; 4]; 16],
}

impl Theme {
    pub fn get(theme_name: &str) -> Self {
        match theme_name.to_lowercase().as_str() {
            "catppuccin-mocha" | "catppuccin" => Self {
                name: "catppuccin-mocha",
                foreground: [0.78, 0.82, 0.94, 1.0],
                background: [0.12, 0.12, 0.18, 1.0],
                cursor: [0.96, 0.88, 0.79, 0.8],
                selection: [0.36, 0.39, 0.49, 0.45],
                palette: [
                    [0.27, 0.29, 0.38, 1.0],
                    [0.86, 0.44, 0.54, 1.0],
                    [0.54, 0.78, 0.52, 1.0],
                    [0.88, 0.78, 0.54, 1.0],
                    [0.44, 0.60, 0.88, 1.0],
                    [0.85, 0.64, 0.80, 1.0],
                    [0.48, 0.78, 0.76, 1.0],
                    [0.73, 0.76, 0.89, 1.0],
                    [0.42, 0.45, 0.56, 1.0],
                    [0.98, 0.62, 0.72, 1.0],
                    [0.72, 0.96, 0.70, 1.0],
                    [1.00, 0.94, 0.75, 1.0],
                    [0.64, 0.80, 1.00, 1.0],
                    [0.98, 0.82, 0.95, 1.0],
                    [0.66, 0.96, 0.94, 1.0],
                    [0.98, 0.99, 1.00, 1.0],
                ],
            },
            "dracula" => Self {
                name: "dracula",
                foreground: [0.90, 0.90, 0.88, 1.0],
                background: [0.16, 0.16, 0.21, 1.0],
                cursor: [0.74, 0.47, 0.86, 0.8],
                selection: [0.27, 0.28, 0.35, 0.5],
                palette: [
                    [0.13, 0.13, 0.17, 1.0],
                    [0.90, 0.28, 0.28, 1.0],
                    [0.26, 0.86, 0.42, 1.0],
                    [0.86, 0.88, 0.46, 1.0],
                    [0.64, 0.48, 0.90, 1.0],
                    [0.90, 0.40, 0.70, 1.0],
                    [0.46, 0.80, 0.88, 1.0],
                    [0.88, 0.88, 0.86, 1.0],
                    [0.38, 0.45, 0.56, 1.0],
                    [1.00, 0.45, 0.45, 1.0],
                    [0.42, 1.00, 0.58, 1.0],
                    [1.00, 1.00, 0.65, 1.0],
                    [0.84, 0.67, 1.00, 1.0],
                    [1.00, 0.57, 0.88, 1.0],
                    [0.65, 1.00, 1.00, 1.0],
                    [1.00, 1.00, 1.00, 1.0],
                ],
            },
            "solarized-dark" => Self {
                name: "solarized-dark",
                foreground: [0.58, 0.63, 0.63, 1.0],
                background: [0.00, 0.17, 0.21, 1.0],
                cursor: [0.58, 0.63, 0.63, 0.8],
                selection: [0.03, 0.21, 0.26, 0.5],
                palette: [
                    [0.03, 0.21, 0.26, 1.0],
                    [0.86, 0.20, 0.18, 1.0],
                    [0.52, 0.60, 0.00, 1.0],
                    [0.71, 0.54, 0.00, 1.0],
                    [0.15, 0.55, 0.82, 1.0],
                    [0.83, 0.21, 0.51, 1.0],
                    [0.16, 0.63, 0.60, 1.0],
                    [0.83, 0.81, 0.73, 1.0],
                    [0.20, 0.35, 0.40, 1.0],
                    [0.98, 0.40, 0.35, 1.0],
                    [0.68, 0.80, 0.15, 1.0],
                    [0.90, 0.72, 0.15, 1.0],
                    [0.35, 0.75, 0.98, 1.0],
                    [0.95, 0.42, 0.72, 1.0],
                    [0.36, 0.85, 0.82, 1.0],
                    [0.99, 0.96, 0.89, 1.0],
                ],
            },
            "tokyo-night" => Self {
                name: "tokyo-night",
                foreground: [0.75, 0.79, 0.92, 1.0],
                background: [0.10, 0.10, 0.16, 1.0],
                cursor: [0.75, 0.79, 0.92, 0.8],
                selection: [0.20, 0.24, 0.38, 0.5],
                palette: [
                    [0.26, 0.27, 0.40, 1.0],
                    [0.86, 0.38, 0.38, 1.0],
                    [0.38, 0.74, 0.68, 1.0],
                    [0.78, 0.60, 0.36, 1.0],
                    [0.40, 0.68, 0.88, 1.0],
                    [0.64, 0.50, 0.86, 1.0],
                    [0.40, 0.70, 0.88, 1.0],
                    [0.75, 0.79, 0.92, 1.0],
                    [0.40, 0.42, 0.58, 1.0],
                    [1.00, 0.56, 0.56, 1.0],
                    [0.54, 0.94, 0.88, 1.0],
                    [0.98, 0.80, 0.54, 1.0],
                    [0.58, 0.88, 1.00, 1.0],
                    [0.84, 0.72, 1.00, 1.0],
                    [0.58, 0.90, 1.00, 1.0],
                    [0.98, 0.99, 1.00, 1.0],
                ],
            },
            "nord" => Self {
                name: "nord",
                foreground: [0.82, 0.84, 0.89, 1.0],
                background: [0.18, 0.20, 0.25, 1.0],
                cursor: [0.85, 0.87, 0.91, 0.8],
                selection: [0.26, 0.30, 0.37, 0.5],
                palette: [
                    [0.23, 0.26, 0.32, 1.0],
                    [0.75, 0.38, 0.41, 1.0],
                    [0.64, 0.75, 0.55, 1.0],
                    [0.86, 0.74, 0.48, 1.0],
                    [0.51, 0.63, 0.76, 1.0],
                    [0.71, 0.56, 0.68, 1.0],
                    [0.53, 0.75, 0.82, 1.0],
                    [0.84, 0.86, 0.90, 1.0],
                    [0.36, 0.40, 0.50, 1.0],
                    [0.92, 0.52, 0.56, 1.0],
                    [0.78, 0.90, 0.68, 1.0],
                    [0.98, 0.88, 0.64, 1.0],
                    [0.66, 0.78, 0.92, 1.0],
                    [0.86, 0.70, 0.84, 1.0],
                    [0.68, 0.90, 0.96, 1.0],
                    [0.98, 0.99, 1.00, 1.0],
                ],
            },
            "pure-black" => Self {
                name: "pure-black",
                foreground: [0.83, 0.85, 0.88, 1.0], // Balanced Silver-White (#D4D9E0) so Bold/Bright (#FFFFFF) stands out clearly
                background: [0.00, 0.00, 0.00, 1.0], // OLED Pure Black (#000000)
                cursor: [0.039, 0.518, 1.00, 0.85],  // macOS System Blue Accent
                selection: [0.18, 0.28, 0.45, 0.5],
                palette: [
                    // Normal Colors (0..7): Balanced saturation
                    [0.08, 0.09, 0.11, 1.0],
                    [0.82, 0.26, 0.26, 1.0],
                    [0.18, 0.74, 0.32, 1.0],
                    [0.84, 0.66, 0.14, 1.0],
                    [0.22, 0.52, 0.88, 1.0],
                    [0.78, 0.30, 0.75, 1.0],
                    [0.20, 0.74, 0.80, 1.0],
                    [0.82, 0.84, 0.88, 1.0],
                    // Bright Colors (8..15): High-luminance vivid highlights
                    [0.42, 0.45, 0.52, 1.0],
                    [1.00, 0.46, 0.46, 1.0],
                    [0.36, 0.98, 0.52, 1.0],
                    [1.00, 0.92, 0.38, 1.0],
                    [0.46, 0.78, 1.00, 1.0],
                    [1.00, 0.54, 0.96, 1.0],
                    [0.46, 0.98, 1.00, 1.0],
                    [1.00, 1.00, 1.00, 1.0], // Pure Crisp White for Bold/Bright text
                ],
            },
            _ => Self {
                name: "default-dark",
                foreground: [0.96, 0.96, 0.97, 1.0],
                background: [0.00, 0.00, 0.00, 1.0], // Pure black
                cursor: [0.039, 0.518, 1.00, 0.85],
                selection: [0.18, 0.28, 0.45, 0.5],
                palette: [
                    [0.00, 0.00, 0.00, 1.0],
                    [0.95, 0.30, 0.30, 1.0],
                    [0.20, 0.85, 0.35, 1.0],
                    [0.95, 0.75, 0.15, 1.0],
                    [0.25, 0.60, 1.00, 1.0],
                    [0.90, 0.35, 0.85, 1.0],
                    [0.25, 0.85, 0.90, 1.0],
                    [0.92, 0.92, 0.94, 1.0],
                    [0.30, 0.30, 0.35, 1.0],
                    [1.00, 0.45, 0.45, 1.0],
                    [0.35, 0.95, 0.50, 1.0],
                    [1.00, 0.88, 0.35, 1.0],
                    [0.45, 0.75, 1.00, 1.0],
                    [1.00, 0.50, 0.95, 1.0],
                    [0.45, 0.95, 1.00, 1.0],
                    [1.00, 1.00, 1.00, 1.0],
                ],
            },
        }
    }
}

/// Helper function to parse hex color code like "#1e1e2e" or "1e1e2e" to [f32; 4]
pub fn parse_hex_color(hex: &str) -> Option<[f32; 4]> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        Some([r, g, b, 1.0])
    } else if s.len() == 8 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()? as f32 / 255.0;
        let g = u8::from_str_radix(&s[2..4], 16).ok()? as f32 / 255.0;
        let b = u8::from_str_radix(&s[4..6], 16).ok()? as f32 / 255.0;
        let a = u8::from_str_radix(&s[6..8], 16).ok()? as f32 / 255.0;
        Some([r, g, b, a])
    } else {
        None
    }
}

/// Hot-reloading configuration manager
pub struct ConfigManager {
    pub current_config: Config,
    pub path: PathBuf,
    rx: Receiver<Config>,
    _watcher: Option<RecommendedWatcher>,
}

impl ConfigManager {
    pub fn load_or_create() -> Self {
        let path = default_config_path();
        let (tx, rx) = unbounded();

        let config = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(content) => match toml::from_str::<Config>(&content) {
                    Ok(cfg) => {
                        info!("Loaded config from {:?}", path);
                        cfg
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse config at {:?}: {:?}. Using default.",
                            path, e
                        );
                        Config::default()
                    }
                },
                Err(e) => {
                    warn!(
                        "Failed to read config at {:?}: {:?}. Using default.",
                        path, e
                    );
                    Config::default()
                }
            }
        } else {
            let default_cfg = Config::default();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let template = default_config_template();
            if fs::write(&path, template).is_ok() {
                info!("Created default config with template at {:?}", path);
            }
            default_cfg
        };

        let watch_path = path.clone();
        let watcher = setup_watcher(watch_path, tx);

        Self {
            current_config: config,
            path,
            rx,
            _watcher: watcher,
        }
    }

    pub fn check_reload(&mut self) -> Option<&Config> {
        let mut updated = false;
        while let Ok(new_cfg) = self.rx.try_recv() {
            self.current_config = new_cfg;
            updated = true;
        }
        if updated {
            Some(&self.current_config)
        } else {
            None
        }
    }

    /// Open configuration file in default text editor
    pub fn open_in_editor(&self) {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::write(&self.path, default_config_template());
        }

        #[cfg(target_os = "macos")]
        {
            let _ = Command::new("open").arg(&self.path).spawn();
        }

        #[cfg(target_os = "linux")]
        {
            let _ = Command::new("xdg-open").arg(&self.path).spawn();
        }

        #[cfg(target_os = "windows")]
        {
            let _ = Command::new("explorer")
                .arg(&self.path)
                .spawn();
        }
    }
}

fn setup_watcher(path: PathBuf, tx: Sender<Config>) -> Option<RecommendedWatcher> {
    let watch_target = path.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        if let Ok(event) = res {
            if event.kind.is_modify() {
                if let Ok(content) = fs::read_to_string(&watch_target) {
                    if let Ok(new_config) = toml::from_str::<Config>(&content) {
                        info!("Config hot-reloaded successfully from {:?}", watch_target);
                        let _ = tx.send(new_config);
                    }
                }
            }
        }
    })
    .ok()?;

    if let Some(parent) = path.parent() {
        let _ = watcher.watch(parent, RecursiveMode::NonRecursive);
    }

    Some(watcher)
}

pub fn default_config_path() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        PathBuf::from(home).join(".config/skterm/config.toml")
    } else if let Ok(appdata) = std::env::var("APPDATA") {
        PathBuf::from(appdata).join("skTerm/config.toml")
    } else {
        PathBuf::from("config.toml")
    }
}

/// Generate default TOML configuration template with rich comments.
pub fn default_config_template() -> &'static str {
    r##"# ==============================================================================
# skTerm 偏好設定檔 (Preferences & Configuration)
# ==============================================================================
# 本設定檔支援即時熱重載（Hot-Reload），存檔後無需重啟程式即可即時套用！
# 快捷鍵：在終端機內按下 Cmd+, (macOS) 或 Ctrl+, (Linux/Windows) 可快速開啟此檔案。

[font]
# 主要等寬字型家族名稱 (例如: "Menlo", "JetBrains Mono", "Fira Code", "SF Mono")
family = "Menlo"

# 字型大小 (pt 單位，Retina 螢幕將自動以 physical pixels 進行高解析度光柵化)
size = 16.0

# 行高倍率 (預設 1.2)
line_height = 1.2

# 自訂粗體與斜體字型檔或字型名稱 (選填)
# bold_family = "Menlo-Bold"
# italic_family = "Menlo-Italic"

# 自訂 CJK 多重回退字型搜尋清單 (選填)
# fallback_families = ["Hiragino Sans GB", "PingFang TC", "Noto Sans CJK TC"]


[window]
# 初始終端幾何大小 (欄數與行數)
columns = 80
rows = 24

# 視窗邊距 (單位: pt)
padding_x = 8.0
padding_y = 8.0

# 視窗背景不透明度 (0.1 ~ 1.0)
opacity = 1.0

# 啟動視窗模式 ("normal", "maximized", "fullscreen")
startup_mode = "normal"

# 標題列與視窗外觀 ("full", "transparent", "none")
decorations = "full"

# 分頁標籤列顯示策略 ("auto": 多於1個分頁時自動顯示, "always": 恆顯示, "never": 恆隱藏)
tab_bar_style = "auto"


[terminal]
# 終端模擬器 TERM 環境變數 (預設: "xterm-256color")
term = "xterm-256color"

# 自訂預設 Shell 與啟動參數 (選填，若未指定則自動偵測系統預設登入 Shell)
# Windows 常用設定範例：
#   命令提示字元: shell = { program = "cmd.exe", args = [] }
#   Windows PowerShell: shell = { program = "powershell.exe", args = ["-NoLogo"] }
#   PowerShell 7+: shell = { program = "pwsh.exe", args = ["-NoLogo"] }
# macOS / Linux 常用設定範例：
#   shell = { program = "/bin/zsh", args = ["--login"] }

# 初始工作目錄 (選填，例如: "~" 或 "/Users/username/Projects")
# working_directory = "~"

# 歷史滾動緩衝區行數 (預設: 10000 行)
scrollback_lines = 10000

# 自訂環境變數表 (選填)
# [terminal.env]
# COLORTERM = "truecolor"
# EDITOR = "vim"

# macOS Option 鍵行為 ("both", "only_left", "only_right", "none")
# 設為 "both" 或 "only_left" 時，按下 Option 鍵會發送 Esc+ 序列 (Meta 鍵)
option_as_alt = "both"

# 終端鈴聲模式 ("visual", "sound", "none")
bell = "visual"


[colors]
# 內建色彩主題名稱
# 支援: "pure-black", "catppuccin-mocha", "tokyo-night", "dracula", "solarized-dark", "nord", "default-dark"
theme = "pure-black"

# 自訂色彩覆蓋 (選填 Hex 碼，若指定將覆蓋上述主題之預設值)
# foreground = "#cdd6f4"
# background = "#1e1e2e"
# cursor     = "#f5e0dc"
# selection  = "#585b70"

# 自訂 16 色 ANSI 調色盤覆蓋 (選填)
# [colors.palette]
# black   = "#45475a"
# red     = "#f38ba8"
# green   = "#a6e3a1"
# yellow  = "#f9e2af"
# blue    = "#89b4fa"
# magenta = "#f5c2e7"
# cyan    = "#94e2d5"
# white   = "#bac2de"


[cursor]
# 游標形狀 ("block", "beam", "underline")
shape = "block"

# 游標是否閃爍 (true / false)
blink = true

# 游標閃爍間隔毫秒數
blink_interval_ms = 500


[keybindings]
# 自訂按鍵綁定 (選填)
# "cmd+t" = "new_tab"
# "cmd+w" = "close_pane"
# "cmd+d" = "split_vertical"
# "cmd+shift+d" = "split_horizontal"
"##
}
