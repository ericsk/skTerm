//! Configuration file parsing (TOML), theme presets, and filesystem hot-reload.

use crossbeam_channel::{unbounded, Receiver, Sender};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{info, warn};

/// Root Configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub font: FontConfig,
    #[serde(default)]
    pub window: WindowConfig,
    #[serde(default)]
    pub colors: ColorsConfig,
    #[serde(default)]
    pub cursor: CursorConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            font: FontConfig::default(),
            window: WindowConfig::default(),
            colors: ColorsConfig::default(),
            cursor: CursorConfig::default(),
        }
    }
}

/// Font settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontConfig {
    #[serde(default = "default_font_family")]
    pub family: String,
    #[serde(default = "default_font_size")]
    pub size: f32,
    #[serde(default = "default_line_height")]
    pub line_height: f32,
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
        }
    }
}

/// Window appearance settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowConfig {
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_padding")]
    pub padding_x: f32,
    #[serde(default = "default_padding")]
    pub padding_y: f32,
}

fn default_opacity() -> f32 {
    1.0
}
fn default_padding() -> f32 {
    8.0
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            opacity: default_opacity(),
            padding_x: default_padding(),
            padding_y: default_padding(),
        }
    }
}

/// Color theme settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ColorsConfig {
    #[serde(default = "default_theme_name")]
    pub theme: String,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub cursor: Option<String>,
    pub selection: Option<String>,
}

fn default_theme_name() -> String {
    "catppuccin-mocha".to_string()
}

impl Default for ColorsConfig {
    fn default() -> Self {
        Self {
            theme: default_theme_name(),
            foreground: None,
            background: None,
            cursor: None,
            selection: None,
        }
    }
}

/// Cursor visual settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CursorConfig {
    #[serde(default = "default_cursor_shape")]
    pub shape: String,
    #[serde(default = "default_cursor_blink")]
    pub blink: bool,
}

fn default_cursor_shape() -> String {
    "block".to_string()
}
fn default_cursor_blink() -> bool {
    true
}

impl Default for CursorConfig {
    fn default() -> Self {
        Self {
            shape: default_cursor_shape(),
            blink: default_cursor_blink(),
        }
    }
}

/// Theme color definition
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: &'static str,
    pub foreground: [f32; 4],
    pub background: [f32; 4],
    pub cursor: [f32; 4],
    pub selection: [f32; 4],
}

impl Theme {
    pub fn get(theme_name: &str) -> Self {
        match theme_name.to_lowercase().as_str() {
            "catppuccin-mocha" | "catppuccin" => Self {
                name: "catppuccin-mocha",
                foreground: [0.80, 0.84, 0.96, 1.0],
                background: [0.12, 0.12, 0.18, 1.0],
                cursor: [0.96, 0.88, 0.79, 0.8],
                selection: [0.36, 0.39, 0.49, 0.45],
            },
            "dracula" => Self {
                name: "dracula",
                foreground: [0.97, 0.97, 0.95, 1.0],
                background: [0.16, 0.16, 0.21, 1.0],
                cursor: [0.74, 0.47, 0.86, 0.8],
                selection: [0.27, 0.28, 0.35, 0.5],
            },
            "solarized-dark" => Self {
                name: "solarized-dark",
                foreground: [0.51, 0.58, 0.59, 1.0],
                background: [0.00, 0.17, 0.21, 1.0],
                cursor: [0.58, 0.63, 0.63, 0.8],
                selection: [0.03, 0.21, 0.26, 0.5],
            },
            "tokyo-night" => Self {
                name: "tokyo-night",
                foreground: [0.75, 0.79, 0.92, 1.0],
                background: [0.10, 0.10, 0.16, 1.0],
                cursor: [0.75, 0.79, 0.92, 0.8],
                selection: [0.20, 0.24, 0.38, 0.5],
            },
            "nord" => Self {
                name: "nord",
                foreground: [0.85, 0.87, 0.91, 1.0],
                background: [0.18, 0.20, 0.25, 1.0],
                cursor: [0.85, 0.87, 0.91, 0.8],
                selection: [0.26, 0.30, 0.37, 0.5],
            },
            _ => Self {
                name: "default-dark",
                foreground: [0.92, 0.92, 0.95, 1.0],
                background: [0.08, 0.08, 0.12, 1.0],
                cursor: [0.85, 0.85, 0.95, 0.75],
                selection: [0.30, 0.45, 0.70, 0.4],
            },
        }
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
                        warn!("Failed to parse config at {:?}: {:?}. Using default.", path, e);
                        Config::default()
                    }
                },
                Err(e) => {
                    warn!("Failed to read config at {:?}: {:?}. Using default.", path, e);
                    Config::default()
                }
            }
        } else {
            let default_cfg = Config::default();
            if let Some(parent) = path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(serialized) = toml::to_string_pretty(&default_cfg) {
                let _ = fs::write(&path, serialized);
                info!("Created default config at {:?}", path);
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
