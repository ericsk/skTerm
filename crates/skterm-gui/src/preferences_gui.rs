//! In-App Graphical Preferences Window & Modal rendered via egui.

use crate::config::{Config, ConfigManager, ShellConfig};
use egui::{Color32, Context, RichText, Ui, Vec2, Window};
use std::fs;
use tracing::info;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesTab {
    Appearance,
    Colors,
    Terminal,
    Cursor,
}

pub struct PreferencesGui {
    pub is_open: bool,
    pub active_tab: PreferencesTab,
    // Form fields
    pub font_family: String,
    pub font_size: f32,
    pub line_height: f32,
    pub columns: u16,
    pub rows: u16,
    pub padding_x: f32,
    pub padding_y: f32,
    pub opacity: f32,
    pub startup_mode: String,
    pub decorations: String,
    pub tab_bar_style: String,

    pub theme: String,
    pub fg_override: String,
    pub bg_override: String,
    pub cursor_override: String,
    pub selection_override: String,

    pub term: String,
    pub custom_shell_enabled: bool,
    pub shell_program: String,
    pub shell_args: String,
    pub working_directory: String,
    pub scrollback_lines: usize,
    pub option_as_alt: String,
    pub bell: String,

    pub cursor_shape: String,
    pub cursor_blink: bool,
    pub blink_interval_ms: u64,
}

/// Setup CJK font support for egui so Chinese/Japanese/Korean text displays properly.
pub fn setup_egui_cjk_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();

    let cjk_candidates = [
        // macOS
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
    ];

    for path in cjk_candidates {
        if let Ok(font_data) = fs::read(path) {
            info!("Loaded CJK font into egui preferences: {}", path);
            fonts.font_data.insert(
                "cjk_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(font_data)),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk_font".to_owned());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("cjk_font".to_owned());

            break;
        }
    }

    ctx.set_fonts(fonts);
}

impl Default for PreferencesGui {
    fn default() -> Self {
        Self::new()
    }
}

impl PreferencesGui {
    pub fn new() -> Self {
        Self {
            is_open: false,
            active_tab: PreferencesTab::Appearance,
            font_family: "Menlo".to_string(),
            font_size: 16.0,
            line_height: 1.2,
            columns: 80,
            rows: 24,
            padding_x: 8.0,
            padding_y: 8.0,
            opacity: 1.0,
            startup_mode: "normal".to_string(),
            decorations: "full".to_string(),
            tab_bar_style: "auto".to_string(),

            theme: "catppuccin-mocha".to_string(),
            fg_override: "".to_string(),
            bg_override: "".to_string(),
            cursor_override: "".to_string(),
            selection_override: "".to_string(),

            term: "xterm-256color".to_string(),
            custom_shell_enabled: false,
            shell_program: "/bin/zsh".to_string(),
            shell_args: "".to_string(),
            working_directory: "".to_string(),
            scrollback_lines: 10000,
            option_as_alt: "both".to_string(),
            bell: "visual".to_string(),

            cursor_shape: "block".to_string(),
            cursor_blink: true,
            blink_interval_ms: 500,
        }
    }

    /// Load form state from current Config
    pub fn load_from_config(&mut self, cfg: &Config) {
        self.font_family = cfg.font.family.clone();
        self.font_size = cfg.font.size;
        self.line_height = cfg.font.line_height;
        self.columns = cfg.window.columns;
        self.rows = cfg.window.rows;
        self.padding_x = cfg.window.padding_x;
        self.padding_y = cfg.window.padding_y;
        self.opacity = cfg.window.opacity;
        self.startup_mode = cfg.window.startup_mode.clone();
        self.decorations = cfg.window.decorations.clone();
        self.tab_bar_style = cfg.window.tab_bar_style.clone();

        self.theme = cfg.colors.theme.clone();
        self.fg_override = cfg.colors.foreground.clone().unwrap_or_default();
        self.bg_override = cfg.colors.background.clone().unwrap_or_default();
        self.cursor_override = cfg.colors.cursor.clone().unwrap_or_default();
        self.selection_override = cfg.colors.selection.clone().unwrap_or_default();

        self.term = cfg.terminal.term.clone();
        if let Some(s) = &cfg.terminal.shell {
            self.custom_shell_enabled = true;
            self.shell_program = s.program.clone();
            self.shell_args = s.args.join(" ");
        } else {
            self.custom_shell_enabled = false;
            self.shell_program = "".to_string();
            self.shell_args = "".to_string();
        }
        self.working_directory = cfg.terminal.working_directory.clone().unwrap_or_default();
        self.scrollback_lines = cfg.terminal.scrollback_lines;
        self.option_as_alt = cfg.terminal.option_as_alt.clone();
        self.bell = cfg.terminal.bell.clone();

        self.cursor_shape = cfg.cursor.shape.clone();
        self.cursor_blink = cfg.cursor.blink;
        self.blink_interval_ms = cfg.cursor.blink_interval_ms;
    }

    /// Convert current form into Config struct
    pub fn to_config(&self, original: &Config) -> Config {
        let mut cfg = original.clone();
        cfg.font.family = self.font_family.clone();
        cfg.font.size = self.font_size;
        cfg.font.line_height = self.line_height;

        cfg.window.columns = self.columns;
        cfg.window.rows = self.rows;
        cfg.window.padding_x = self.padding_x;
        cfg.window.padding_y = self.padding_y;
        cfg.window.opacity = self.opacity;
        cfg.window.startup_mode = self.startup_mode.clone();
        cfg.window.decorations = self.decorations.clone();
        cfg.window.tab_bar_style = self.tab_bar_style.clone();

        cfg.colors.theme = self.theme.clone();
        cfg.colors.foreground = if self.fg_override.trim().is_empty() {
            None
        } else {
            Some(self.fg_override.trim().to_string())
        };
        cfg.colors.background = if self.bg_override.trim().is_empty() {
            None
        } else {
            Some(self.bg_override.trim().to_string())
        };
        cfg.colors.cursor = if self.cursor_override.trim().is_empty() {
            None
        } else {
            Some(self.cursor_override.trim().to_string())
        };
        cfg.colors.selection = if self.selection_override.trim().is_empty() {
            None
        } else {
            Some(self.selection_override.trim().to_string())
        };

        cfg.terminal.term = self.term.clone();
        if self.custom_shell_enabled && !self.shell_program.trim().is_empty() {
            let args = self
                .shell_args
                .split_whitespace()
                .map(|s| s.to_string())
                .collect();
            cfg.terminal.shell = Some(ShellConfig {
                program: self.shell_program.trim().to_string(),
                args,
            });
        } else {
            cfg.terminal.shell = None;
        }
        cfg.terminal.working_directory = if self.working_directory.trim().is_empty() {
            None
        } else {
            Some(self.working_directory.trim().to_string())
        };
        cfg.terminal.scrollback_lines = self.scrollback_lines;
        cfg.terminal.option_as_alt = self.option_as_alt.clone();
        cfg.terminal.bell = self.bell.clone();

        cfg.cursor.shape = self.cursor_shape.clone();
        cfg.cursor.blink = self.cursor_blink;
        cfg.cursor.blink_interval_ms = self.blink_interval_ms;

        cfg
    }

    /// Toggle open/close modal
    pub fn toggle(&mut self, cfg: &Config) {
        self.is_open = !self.is_open;
        if self.is_open {
            self.load_from_config(cfg);
        }
    }

    /// Render egui preferences modal
    pub fn render(&mut self, ctx: &Context, config_manager: &mut ConfigManager) {
        if !self.is_open {
            return;
        }

        let mut is_open = self.is_open;

        Window::new(
            RichText::new("⚙ skTerm 偏好設定 (Preferences)")
                .heading()
                .color(Color32::WHITE),
        )
        .open(&mut is_open)
        .collapsible(false)
        .resizable(true)
        .default_size(Vec2::new(580.0, 480.0))
        .min_size(Vec2::new(480.0, 380.0))
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.active_tab,
                    PreferencesTab::Appearance,
                    " 外觀與字型 ",
                );
                ui.selectable_value(&mut self.active_tab, PreferencesTab::Colors, " 色彩與主題 ");
                ui.selectable_value(
                    &mut self.active_tab,
                    PreferencesTab::Terminal,
                    " 終端與 Shell ",
                );
                ui.selectable_value(&mut self.active_tab, PreferencesTab::Cursor, " 游標與動態 ");
            });

            ui.separator();

            egui::ScrollArea::vertical()
                .max_height(340.0)
                .show(ui, |ui| match self.active_tab {
                    PreferencesTab::Appearance => self.render_appearance_tab(ui),
                    PreferencesTab::Colors => self.render_colors_tab(ui),
                    PreferencesTab::Terminal => self.render_terminal_tab(ui),
                    PreferencesTab::Cursor => self.render_cursor_tab(ui),
                });

            ui.separator();

            ui.horizontal(|ui| {
                if ui
                    .button(RichText::new("💾 儲存並套用 (Save & Apply)").strong())
                    .clicked()
                {
                    let new_cfg = self.to_config(&config_manager.current_config);
                    if let Ok(toml_str) = toml::to_string_pretty(&new_cfg) {
                        let _ =
                            crate::config::write_config_atomically(&config_manager.path, &toml_str);
                        config_manager.current_config = new_cfg;
                        info!(
                            "Saved preferences directly from GUI to {:?}",
                            config_manager.path
                        );
                    }
                    self.is_open = false;
                }

                if ui.button("❌ 取消 (Cancel)").clicked() {
                    self.is_open = false;
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📝 開啟 config.toml").clicked() {
                        config_manager.open_in_editor();
                    }
                    if ui.button("🔄 重設為預設值").clicked() {
                        self.load_from_config(&Config::default());
                    }
                });
            });
        });

        self.is_open = is_open;
    }

    fn render_appearance_tab(&mut self, ui: &mut Ui) {
        crate::preferences_window::render_inset_card(ui, "字型與排版 (Typography)", |b| {
            b.row(
                "主要等寬字型",
                Some("支援複合 Nerd Fonts 與 CJK 字型混合鏈"),
                |ui| {
                    egui::ComboBox::from_id_salt("font_family_preset")
                        .selected_text(if self.font_family.is_empty() {
                            "選擇或自訂字型..."
                        } else {
                            &self.font_family
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.font_family,
                                "NotoSansMNerdFontMono, NotoSansTC, Menlo".to_string(),
                                "NotoSansM Nerd Font (推薦 🚀)",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "JetBrainsMono Nerd Font, PingFang TC".to_string(),
                                "JetBrainsMono Nerd Font",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "FiraCode Nerd Font, PingFang TC".to_string(),
                                "FiraCode Nerd Font",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "Menlo, PingFang TC".to_string(),
                                "Menlo (macOS Default)",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "SF Mono, PingFang TC".to_string(),
                                "SF Mono (macOS)",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "DejaVu Sans Mono".to_string(),
                                "DejaVu Sans Mono (Linux)",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "Cascadia Mono, Microsoft YaHei".to_string(),
                                "Cascadia Mono (Windows)",
                            );
                            ui.selectable_value(
                                &mut self.font_family,
                                "Consolas".to_string(),
                                "Consolas (Windows)",
                            );
                        });
                },
            );

            b.row(
                "自訂字型名稱",
                Some("直接輸入字型家族名稱"),
                |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.font_family).desired_width(160.0));
                },
            );

            b.row("字型大小 (Size pt)", Some("以 pt 為單位"), |ui| {
                ui.add(egui::Slider::new(&mut self.font_size, 9.0..=32.0).suffix(" pt"));
            });

            b.row(
                "行高倍率 (Line Height)",
                Some("1.0 為緊湊，1.2 為標準舒適"),
                |ui| {
                    ui.add(egui::Slider::new(&mut self.line_height, 1.0..=2.0).step_by(0.05));
                },
            );
        });

        crate::preferences_window::render_inset_card(
            ui,
            "視窗幾何與邊距 (Window & Layout)",
            |b| {
                b.row(
                    "預設終端大小",
                    Some("啟動時初始網格行列數 (列 × 行)"),
                    |ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.columns)
                                .range(40..=240)
                                .prefix("W: "),
                        );
                        ui.label("×");
                        ui.add(
                            egui::DragValue::new(&mut self.rows)
                                .range(10..=100)
                                .prefix("H: "),
                        );
                    },
                );

                b.row(
                    "內距 (Padding X / Y)",
                    Some("文字四周保留空白像素"),
                    |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.padding_x, 0.0..=40.0)
                                .suffix(" pt")
                                .prefix("X: "),
                        );
                        ui.add(
                            egui::Slider::new(&mut self.padding_y, 0.0..=40.0)
                                .suffix(" pt")
                                .prefix("Y: "),
                        );
                    },
                );

                b.row(
                    "背景不透明度 (Opacity)",
                    Some("1.0 為完全不透明，搭配毛玻璃"),
                    |ui| {
                        ui.add(egui::Slider::new(&mut self.opacity, 0.2..=1.0).step_by(0.05));
                    },
                );

                b.row(
                    "啟動模式 (Startup Mode)",
                    Some("應用程式啟動時視窗狀態"),
                    |ui| {
                        let mut cur = self.startup_mode.as_str();
                        let opts = [
                            ("normal", "一般視窗"),
                            ("maximized", "最大化"),
                            ("fullscreen", "全螢幕"),
                        ];
                        if crate::preferences_window::render_macos_segmented_control(
                            ui, &mut cur, &opts,
                        ) {
                            self.startup_mode = cur.to_string();
                        }
                    },
                );

                b.row(
                    "分頁列顯示 (Tab Bar)",
                    Some("控制多分頁標籤列何時出現"),
                    |ui| {
                        let mut cur = self.tab_bar_style.as_str();
                        let opts = [
                            ("auto", "自動隱藏"),
                            ("always", "恆常顯示"),
                            ("never", "永不顯示"),
                        ];
                        if crate::preferences_window::render_macos_segmented_control(
                            ui, &mut cur, &opts,
                        ) {
                            self.tab_bar_style = cur.to_string();
                        }
                    },
                );
            },
        );
    }

    fn render_colors_tab(&mut self, ui: &mut Ui) {
        crate::preferences_window::render_inset_card(ui, "色彩主題 (Preset Themes)", |b| {
            b.row(
                "一鍵主題卡片畫廊 (Theme Gallery)",
                Some("點擊卡片立即套用底色與 ANSI 16 色調色盤"),
                |ui| {
                    let themes = [
                        ("pure-black", "Pure Black 經典黑 🖤"),
                        ("catppuccin-mocha", "Catppuccin Mocha ☕"),
                        ("tokyo-night", "Tokyo Night 🌃"),
                        ("dracula", "Dracula 🧛"),
                        ("solarized-dark", "Solarized Dark ☀️"),
                        ("nord", "Nord ❄️"),
                    ];

                    let old_theme = self.theme.clone();
                    egui::Grid::new("gui_theme_swatch_grid")
                        .num_columns(2)
                        .spacing(egui::vec2(10.0, 8.0))
                        .show(ui, |ui| {
                            for (idx, (id, label)) in themes.iter().enumerate() {
                                ui.push_id(id, |ui| {
                                    let is_selected = self.theme == *id;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(165.0, 48.0),
                                        egui::Sense::click(),
                                    );
                                    let hover = resp.hovered();

                                    let bg = if is_selected {
                                        Color32::from_rgb(36, 48, 76)
                                    } else if hover {
                                        Color32::from_rgb(44, 47, 62)
                                    } else {
                                        Color32::from_rgb(28, 30, 40)
                                    };

                                    let stroke = if is_selected {
                                        egui::Stroke::new(1.8_f32, Color32::from_rgb(10, 132, 255))
                                    } else if hover {
                                        egui::Stroke::new(1.0_f32, Color32::from_rgb(75, 82, 108))
                                    } else {
                                        egui::Stroke::new(1.0_f32, Color32::from_rgb(48, 51, 68))
                                    };

                                    ui.painter().rect(
                                        rect,
                                        egui::CornerRadius::same(8),
                                        bg,
                                        stroke,
                                        egui::StrokeKind::Outside,
                                    );

                                    let text_color = if is_selected {
                                        Color32::WHITE
                                    } else {
                                        Color32::from_rgb(215, 220, 235)
                                    };

                                    ui.painter().text(
                                        rect.left_top() + egui::vec2(10.0, 7.0),
                                        egui::Align2::LEFT_TOP,
                                        label,
                                        egui::FontId::proportional(11.5),
                                        text_color,
                                    );

                                    let preset = crate::config::Theme::get(id);
                                    let dot_colors = [
                                        preset.background,
                                        preset.foreground,
                                        preset.palette[1],
                                        preset.palette[2],
                                        preset.palette[4],
                                    ];
                                    let start_x = rect.left() + 10.0;
                                    let dot_y = rect.bottom() - 10.0;
                                    for (i, c_rgba) in dot_colors.iter().enumerate() {
                                        let c32 = Color32::from_rgba_unmultiplied(
                                            (c_rgba[0] * 255.0) as u8,
                                            (c_rgba[1] * 255.0) as u8,
                                            (c_rgba[2] * 255.0) as u8,
                                            255,
                                        );
                                        let dot_x = start_x + (i as f32 * 14.0);
                                        ui.painter().circle_filled(
                                            egui::pos2(dot_x, dot_y),
                                            4.0,
                                            c32,
                                        );
                                        ui.painter().circle_stroke(
                                            egui::pos2(dot_x, dot_y),
                                            4.0,
                                            egui::Stroke::new(
                                                0.5_f32,
                                                Color32::from_white_alpha(70),
                                            ),
                                        );
                                    }

                                    if resp.clicked() {
                                        self.theme = id.to_string();
                                    }
                                });

                                if idx % 2 == 1 {
                                    ui.end_row();
                                }
                            }
                        });

                    if old_theme != self.theme {
                        self.fg_override.clear();
                        self.bg_override.clear();
                        self.cursor_override.clear();
                        self.selection_override.clear();
                    }
                },
            );
        });

        crate::preferences_window::render_inset_card(
            ui,
            "自訂色彩覆蓋 (Color Overrides)",
            |b| {
                b.row(
                    "前景文字色 (Foreground)",
                    Some("留空則繼承主題預設值"),
                    |ui| {
                        crate::preferences_window::render_color_row(ui, "", &mut self.fg_override);
                    },
                );
                b.row(
                    "背景底色 (Background)",
                    Some("留空則繼承主題預設值"),
                    |ui| {
                        crate::preferences_window::render_color_row(ui, "", &mut self.bg_override);
                    },
                );
                b.row(
                    "游標顏色 (Cursor)",
                    Some("留空則繼承主題預設值"),
                    |ui| {
                        crate::preferences_window::render_color_row(
                            ui,
                            "",
                            &mut self.cursor_override,
                        );
                    },
                );
                b.row(
                    "選取文字底色 (Selection)",
                    Some("留空則繼承主題預設值"),
                    |ui| {
                        crate::preferences_window::render_color_row(
                            ui,
                            "",
                            &mut self.selection_override,
                        );
                    },
                );
            },
        );
    }

    fn render_terminal_tab(&mut self, ui: &mut Ui) {
        crate::preferences_window::render_inset_card(
            ui,
            "模擬器與環境 (Terminal & Environment)",
            |b| {
                b.row("TERM 變數", Some("環境變數 $TERM 預設值"), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.term).desired_width(140.0));
                });

                b.row(
                    "指定自訂 Shell 程式",
                    Some("預設使用系統 $SHELL"),
                    |ui| {
                        crate::preferences_window::render_macos_toggle_switch(
                            ui,
                            &mut self.custom_shell_enabled,
                        );
                    },
                );

                if self.custom_shell_enabled {
                    b.row("Shell 執行檔路徑", None, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.shell_program)
                                .hint_text("/bin/zsh"),
                        );
                    });

                    b.row("啟動參數 (Args)", None, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.shell_args).hint_text("--login"),
                        );
                    });
                }

                b.row(
                    "初始工作目錄",
                    Some("終端啟動預設路徑"),
                    |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.working_directory)
                                .hint_text("預設使用者家目錄 (~/)"),
                        );
                    },
                );
            },
        );

        crate::preferences_window::render_inset_card(
            ui,
            "緩衝區與操作行為 (Buffer & Interaction)",
            |b| {
                b.row(
                    "滾動歷史行數 (Scrollback)",
                    Some("保留向上捲動緩衝行數"),
                    |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.scrollback_lines, 1000..=50000)
                                .step_by(1000.0),
                        );
                    },
                );

                b.row(
                    "macOS Option 鍵 (Meta)",
                    Some("是否傳遞 Meta (Esc+) 序列"),
                    |ui| {
                        let mut cur = self.option_as_alt.as_str();
                        let opts = [
                            ("both", "左右 Option 均作 Meta"),
                            ("only_left", "僅左 Option"),
                            ("only_right", "僅右 Option"),
                            ("none", "不轉換"),
                        ];
                        if crate::preferences_window::render_macos_segmented_control(
                            ui, &mut cur, &opts,
                        ) {
                            self.option_as_alt = cur.to_string();
                        }
                    },
                );

                b.row(
                    "終端鈴聲模式 (Bell)",
                    Some("遇警示訊號時回饋方式"),
                    |ui| {
                        let mut cur = self.bell.as_str();
                        let opts = [
                            ("visual", "視覺閃爍"),
                            ("sound", "系統聲音"),
                            ("none", "靜音"),
                        ];
                        if crate::preferences_window::render_macos_segmented_control(
                            ui, &mut cur, &opts,
                        ) {
                            self.bell = cur.to_string();
                        }
                    },
                );
            },
        );
    }

    fn render_cursor_tab(&mut self, ui: &mut Ui) {
        crate::preferences_window::render_inset_card(
            ui,
            "游標樣式與動態 (Cursor Style)",
            |b| {
                b.row(
                    "游標形狀 (Shape)",
                    Some("終端游標幾何外觀"),
                    |ui| {
                        let mut cur = self.cursor_shape.as_str();
                        let opts = [
                            ("block", "方塊 (Block █)"),
                            ("beam", "直條 (Beam ❘)"),
                            ("underline", "底線 (Underline _)"),
                        ];
                        if crate::preferences_window::render_macos_segmented_control(
                            ui, &mut cur, &opts,
                        ) {
                            self.cursor_shape = cur.to_string();
                        }
                    },
                );

                b.row(
                    "游標閃爍 (Blink)",
                    Some("開啟游標平滑呼吸動畫"),
                    |ui| {
                        crate::preferences_window::render_macos_toggle_switch(
                            ui,
                            &mut self.cursor_blink,
                        );
                    },
                );

                if self.cursor_blink {
                    b.row(
                        "閃爍頻率 (Interval)",
                        Some("游標閃爍頻率毫秒"),
                        |ui| {
                            ui.add(
                                egui::Slider::new(&mut self.blink_interval_ms, 200..=1000)
                                    .suffix(" ms"),
                            );
                        },
                    );
                }
            },
        );
    }
}

/// Helper function to execute egui wgpu rendering pass
pub fn render_egui_overlay(
    egui_renderer: &mut egui_wgpu::Renderer,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    textures_delta: &egui::TexturesDelta,
    clipped_primitives: &[egui::ClippedPrimitive],
    screen_descriptor: &egui_wgpu::ScreenDescriptor,
) {
    for (id, delta) in &textures_delta.set {
        egui_renderer.update_texture(device, queue, *id, delta);
    }

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("egui_command_encoder"),
    });

    egui_renderer.update_buffers(
        device,
        queue,
        &mut encoder,
        clipped_primitives,
        screen_descriptor,
    );

    {
        let mut render_pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("egui_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            })
            .forget_lifetime();

        egui_renderer.render(&mut render_pass, clipped_primitives, screen_descriptor);
    }

    queue.submit(std::iter::once(encoder.finish()));

    for id in &textures_delta.free {
        egui_renderer.free_texture(id);
    }
}
