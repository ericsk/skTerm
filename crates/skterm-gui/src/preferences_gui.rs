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
        cfg.colors.foreground = if self.fg_override.trim().is_empty() { None } else { Some(self.fg_override.trim().to_string()) };
        cfg.colors.background = if self.bg_override.trim().is_empty() { None } else { Some(self.bg_override.trim().to_string()) };
        cfg.colors.cursor = if self.cursor_override.trim().is_empty() { None } else { Some(self.cursor_override.trim().to_string()) };
        cfg.colors.selection = if self.selection_override.trim().is_empty() { None } else { Some(self.selection_override.trim().to_string()) };

        cfg.terminal.term = self.term.clone();
        if self.custom_shell_enabled && !self.shell_program.trim().is_empty() {
            let args = self.shell_args.split_whitespace().map(|s| s.to_string()).collect();
            cfg.terminal.shell = Some(ShellConfig {
                program: self.shell_program.trim().to_string(),
                args,
            });
        } else {
            cfg.terminal.shell = None;
        }
        cfg.terminal.working_directory = if self.working_directory.trim().is_empty() { None } else { Some(self.working_directory.trim().to_string()) };
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

        Window::new(RichText::new("⚙ skTerm 偏好設定 (Preferences)").heading().color(Color32::WHITE))
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_size(Vec2::new(580.0, 480.0))
            .min_size(Vec2::new(480.0, 380.0))
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.active_tab, PreferencesTab::Appearance, " 外觀與字型 ");
                    ui.selectable_value(&mut self.active_tab, PreferencesTab::Colors, " 色彩與主題 ");
                    ui.selectable_value(&mut self.active_tab, PreferencesTab::Terminal, " 終端與 Shell ");
                    ui.selectable_value(&mut self.active_tab, PreferencesTab::Cursor, " 游標與動態 ");
                });

                ui.separator();

                egui::ScrollArea::vertical()
                    .max_height(340.0)
                    .show(ui, |ui| {
                        match self.active_tab {
                            PreferencesTab::Appearance => self.render_appearance_tab(ui),
                            PreferencesTab::Colors => self.render_colors_tab(ui),
                            PreferencesTab::Terminal => self.render_terminal_tab(ui),
                            PreferencesTab::Cursor => self.render_cursor_tab(ui),
                        }
                    });

                ui.separator();

                ui.horizontal(|ui| {
                    if ui.button(RichText::new("💾 儲存並套用 (Save & Apply)").strong()).clicked() {
                        let new_cfg = self.to_config(&config_manager.current_config);
                        if let Ok(toml_str) = toml::to_string_pretty(&new_cfg) {
                            let _ = fs::write(&config_manager.path, toml_str);
                            config_manager.current_config = new_cfg;
                            info!("Saved preferences directly from GUI to {:?}", config_manager.path);
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
        ui.heading("字型設定 (Font)");
        egui::Grid::new("font_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("字型家族 (Family):");
            ui.horizontal(|ui| {
                ui.text_edit_singleline(&mut self.font_family);
                egui::ComboBox::from_id_salt("font_family_preset")
                    .selected_text("常用預設...")
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.font_family, "NotoSansMNerdFontMono, NotoSansTC, Menlo".to_string(), "NotoSansM Nerd Font (推薦 🚀)");
                        ui.selectable_value(&mut self.font_family, "JetBrainsMono Nerd Font, PingFang TC".to_string(), "JetBrainsMono Nerd Font");
                        ui.selectable_value(&mut self.font_family, "FiraCode Nerd Font, PingFang TC".to_string(), "FiraCode Nerd Font");
                        ui.selectable_value(&mut self.font_family, "Menlo, PingFang TC".to_string(), "Menlo (macOS Default)");
                        ui.selectable_value(&mut self.font_family, "SF Mono, PingFang TC".to_string(), "SF Mono (macOS)");
                        ui.selectable_value(&mut self.font_family, "DejaVu Sans Mono".to_string(), "DejaVu Sans Mono (Linux)");
                        ui.selectable_value(&mut self.font_family, "Cascadia Mono, Microsoft YaHei".to_string(), "Cascadia Mono (Windows)");
                        ui.selectable_value(&mut self.font_family, "Consolas".to_string(), "Consolas (Windows)");
                    });
            });
            ui.end_row();

            ui.label("字型大小 (Size pt):");
            ui.add(egui::Slider::new(&mut self.font_size, 9.0..=32.0).suffix(" pt"));
            ui.end_row();

            ui.label("行高倍率 (Line Height):");
            ui.add(egui::Slider::new(&mut self.line_height, 1.0..=2.0).step_by(0.05));
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.heading("視窗幾何與佈局 (Window & Layout)");
        egui::Grid::new("window_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("預設欄數 (Columns):");
            ui.add(egui::Slider::new(&mut self.columns, 40..=240));
            ui.end_row();

            ui.label("預設行數 (Rows):");
            ui.add(egui::Slider::new(&mut self.rows, 10..=100));
            ui.end_row();

            ui.label("邊距 (Padding X / Y):");
            ui.horizontal(|ui| {
                ui.label("X:");
                ui.add(egui::DragValue::new(&mut self.padding_x).range(0.0..=40.0));
                ui.label("Y:");
                ui.add(egui::DragValue::new(&mut self.padding_y).range(0.0..=40.0));
            });
            ui.end_row();

            ui.label("背景不透明度 (Opacity):");
            ui.add(egui::Slider::new(&mut self.opacity, 0.2..=1.0).step_by(0.05));
            ui.end_row();

            ui.label("啟動模式 (Startup Mode):");
            egui::ComboBox::from_id_salt("startup_mode")
                .selected_text(&self.startup_mode)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.startup_mode, "normal".to_string(), "一般視窗 (Normal)");
                    ui.selectable_value(&mut self.startup_mode, "maximized".to_string(), "最大化 (Maximized)");
                    ui.selectable_value(&mut self.startup_mode, "fullscreen".to_string(), "全螢幕 (Fullscreen)");
                });
            ui.end_row();

            ui.label("分頁列顯示 (Tab Bar):");
            egui::ComboBox::from_id_salt("tab_bar_style")
                .selected_text(&self.tab_bar_style)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.tab_bar_style, "auto".to_string(), "自動 (多於1個分頁時顯示)");
                    ui.selectable_value(&mut self.tab_bar_style, "always".to_string(), "恆常顯示 (Always)");
                    ui.selectable_value(&mut self.tab_bar_style, "never".to_string(), "永不顯示 (Never)");
                });
            ui.end_row();
        });
    }

    fn render_colors_tab(&mut self, ui: &mut Ui) {
        ui.heading("色彩主題 (Themes)");
        egui::Grid::new("theme_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("主題預設 (Preset Theme):");
            egui::ComboBox::from_id_salt("theme_preset")
                .selected_text(&self.theme)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.theme, "catppuccin-mocha".to_string(), "Catppuccin Mocha ☕");
                    ui.selectable_value(&mut self.theme, "tokyo-night".to_string(), "Tokyo Night 🌃");
                    ui.selectable_value(&mut self.theme, "dracula".to_string(), "Dracula 🧛");
                    ui.selectable_value(&mut self.theme, "solarized-dark".to_string(), "Solarized Dark ☀️");
                    ui.selectable_value(&mut self.theme, "nord".to_string(), "Nord ❄️");
                    ui.selectable_value(&mut self.theme, "default-dark".to_string(), "Default Dark 🌑");
                });
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.heading("自訂色彩覆蓋 (Hex Overrides)");
        ui.label("若留空則套用主題預設值：");

        egui::Grid::new("color_overrides").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("前景色彩 (Foreground):");
            ui.text_edit_singleline(&mut self.fg_override);
            ui.end_row();

            ui.label("背景色彩 (Background):");
            ui.text_edit_singleline(&mut self.bg_override);
            ui.end_row();

            ui.label("游標色彩 (Cursor):");
            ui.text_edit_singleline(&mut self.cursor_override);
            ui.end_row();

            ui.label("選取區域 (Selection):");
            ui.text_edit_singleline(&mut self.selection_override);
            ui.end_row();
        });
    }

    fn render_terminal_tab(&mut self, ui: &mut Ui) {
        ui.heading("模擬器與環境 (Terminal & Environment)");
        egui::Grid::new("term_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("TERM 變數:");
            ui.text_edit_singleline(&mut self.term);
            ui.end_row();

            ui.label("滾動歷史行數 (Scrollback):");
            ui.add(egui::Slider::new(&mut self.scrollback_lines, 1000..=50000).step_by(1000.0));
            ui.end_row();

            ui.label("初始工作目錄 (Working Dir):");
            ui.text_edit_singleline(&mut self.working_directory);
            ui.end_row();

            ui.label("macOS Option 鍵 (Meta):");
            egui::ComboBox::from_id_salt("option_as_alt")
                .selected_text(&self.option_as_alt)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.option_as_alt, "both".to_string(), "左右 Option 皆發送 Esc+ (Both)");
                    ui.selectable_value(&mut self.option_as_alt, "only_left".to_string(), "僅左 Option (Only Left)");
                    ui.selectable_value(&mut self.option_as_alt, "only_right".to_string(), "僅右 Option (Only Right)");
                    ui.selectable_value(&mut self.option_as_alt, "none".to_string(), "不轉換 (None)");
                });
            ui.end_row();

            ui.label("鈴聲模式 (Bell):");
            egui::ComboBox::from_id_salt("bell_mode")
                .selected_text(&self.bell)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.bell, "visual".to_string(), "視覺閃爍 (Visual)");
                    ui.selectable_value(&mut self.bell, "sound".to_string(), "系統聲音 (Sound)");
                    ui.selectable_value(&mut self.bell, "none".to_string(), "靜音 (None)");
                });
            ui.end_row();
        });

        ui.add_space(10.0);
        ui.heading("自訂 Shell (Custom Shell)");
        ui.checkbox(&mut self.custom_shell_enabled, "指定自訂 Shell 程式 (預設使用系統 $SHELL)");

        if self.custom_shell_enabled {
            egui::Grid::new("shell_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
                ui.label("Shell 執行檔路徑:");
                ui.text_edit_singleline(&mut self.shell_program);
                ui.end_row();

                ui.label("啟動參數 (Args):");
                ui.text_edit_singleline(&mut self.shell_args);
                ui.end_row();
            });
        }
    }

    fn render_cursor_tab(&mut self, ui: &mut Ui) {
        ui.heading("游標樣式與動態 (Cursor)");
        egui::Grid::new("cursor_grid").num_columns(2).spacing([16.0, 8.0]).show(ui, |ui| {
            ui.label("游標形狀 (Shape):");
            egui::ComboBox::from_id_salt("cursor_shape")
                .selected_text(&self.cursor_shape)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.cursor_shape, "block".to_string(), "方塊游標 (Block █)");
                    ui.selectable_value(&mut self.cursor_shape, "beam".to_string(), "直條游標 (Beam ❘)");
                    ui.selectable_value(&mut self.cursor_shape, "underline".to_string(), "底線游標 (Underline  )");
                });
            ui.end_row();

            ui.label("游標閃爍 (Blink):");
            ui.checkbox(&mut self.cursor_blink, "開啟閃爍");
            ui.end_row();

            if self.cursor_blink {
                ui.label("閃爍頻率 (Interval):");
                ui.add(egui::Slider::new(&mut self.blink_interval_ms, 200..=1000).suffix(" ms"));
                ui.end_row();
            }
        });
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

    egui_renderer.update_buffers(device, queue, &mut encoder, clipped_primitives, screen_descriptor);

    {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
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
        }).forget_lifetime();

        egui_renderer.render(&mut render_pass, clipped_primitives, screen_descriptor);
    }

    queue.submit(std::iter::once(encoder.finish()));

    for id in &textures_delta.free {
        egui_renderer.free_texture(id);
    }
}
