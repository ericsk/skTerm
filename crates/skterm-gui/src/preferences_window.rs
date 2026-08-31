//! Dedicated Native Preferences Window adhering to macOS Human Interface Guidelines (HIG).
//! Features a left category sidebar, rounded grouped inset cards, native toggle switches,
//! auto-discovered system monospace fonts, color picker, and real-time live preview.

use crate::config::{Config, ConfigManager, ShellConfig};
use egui::{
    Align, Color32, Context, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui, Visuals,
};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use wgpu::{
    CompositeAlphaMode, Device, DeviceDescriptor, Features, Instance, InstanceDescriptor, Limits,
    PowerPreference, PresentMode, Queue, RequestAdapterOptions, Surface, SurfaceConfiguration,
    TextureUsages,
};
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesCategory {
    Appearance,
    Colors,
    Terminal,
    Cursor,
    Keybindings,
}

/// Render a macOS Sonoma Inset Grouped Table card with subtle 1px internal dividers.
pub struct InsetCardBuilder<'a> {
    pub ui: &'a mut Ui,
    pub row_count: usize,
}

impl<'a> InsetCardBuilder<'a> {
    pub fn row<F>(&mut self, label: &str, subtitle: Option<&str>, content: F)
    where
        F: FnOnce(&mut Ui),
    {
        if self.row_count > 0 {
            // Subtle 1px horizontal divider between grouped rows
            self.ui.add_space(4.0);
            let avail_w = self.ui.available_width().max(10.0);
            let min_pos = self.ui.cursor().min;
            let end_pos = min_pos + egui::vec2(avail_w, 0.0);
            self.ui.painter().line_segment(
                [min_pos, end_pos],
                Stroke::new(1.0, Color32::from_rgb(46, 49, 63)),
            );
            self.ui.add_space(8.0);
        } else {
            self.ui.add_space(2.0);
        }

        self.ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new(label).color(Color32::from_rgb(230, 234, 242)).size(13.5));
                if let Some(sub) = subtitle {
                    ui.label(RichText::new(sub).color(Color32::from_rgb(135, 142, 160)).size(11.0));
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                content(ui);
            });
        });

        self.ui.add_space(6.0);
        self.row_count += 1;
    }
}

pub fn render_inset_card<F>(ui: &mut Ui, title: &str, add_rows: F)
where
    F: FnOnce(&mut InsetCardBuilder),
{
    // UPPERCASE subtle grey section header
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .color(Color32::from_rgb(140, 148, 168))
            .strong(),
    );
    ui.add_space(4.0);

    Frame::new()
        .fill(Color32::from_rgb(34, 36, 48))
        .stroke(Stroke::new(1.0, Color32::from_rgb(52, 55, 72)))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(16, 12))
        .show(ui, |ui| {
            let mut builder = InsetCardBuilder { ui, row_count: 0 };
            add_rows(&mut builder);
        });

    ui.add_space(16.0);
}

/// Render native macOS Sonoma green toggle switch (#30D158) with animated knob
pub fn render_macos_toggle_switch(ui: &mut Ui, value: &mut bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::new(38.0, 22.0), Sense::click());
    let hover = resp.hovered();

    let bg_color = if *value {
        if hover {
            Color32::from_rgb(52, 219, 98)
        } else {
            Color32::from_rgb(48, 209, 88) // Sonoma Toggle Green
        }
    } else if hover {
        Color32::from_rgb(68, 71, 88)
    } else {
        Color32::from_rgb(55, 58, 74)
    };

    ui.painter().rect_filled(rect, CornerRadius::same(11), bg_color);

    let knob_radius = 8.5;
    let knob_x = if *value {
        rect.right() - knob_radius - 2.5
    } else {
        rect.left() + knob_radius + 2.5
    };
    let knob_center = egui::Pos2::new(knob_x, rect.center().y);

    // Subtle drop shadow under knob
    ui.painter().circle_filled(
        knob_center + egui::vec2(0.0, 1.5),
        knob_radius,
        Color32::from_black_alpha(90),
    );
    ui.painter().circle_filled(knob_center, knob_radius, Color32::WHITE);

    if resp.clicked() {
        *value = !*value;
        true
    } else {
        false
    }
}

/// Render macOS Sonoma Segmented Control pill container
pub fn render_macos_segmented_control<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    options: &[(T, &str)],
) -> bool {
    let mut changed = false;

    Frame::new()
        .fill(Color32::from_rgb(25, 27, 36))
        .stroke(Stroke::new(1.0, Color32::from_rgb(46, 49, 64)))
        .corner_radius(CornerRadius::same(7))
        .inner_margin(Margin::symmetric(3, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                for (opt_val, opt_label) in options {
                    let is_selected = *current == *opt_val;
                    let (bg_col, text_col, stroke) = if is_selected {
                        (
                            Color32::from_rgb(58, 62, 82),
                            Color32::WHITE,
                            Stroke::new(1.0, Color32::from_rgb(82, 87, 112)),
                        )
                    } else {
                        (
                            Color32::TRANSPARENT,
                            Color32::from_rgb(145, 152, 172),
                            Stroke::NONE,
                        )
                    };

                    let btn = egui::Button::new(
                        RichText::new(*opt_label).color(text_col).size(12.0).strong(),
                    )
                    .fill(bg_col)
                    .stroke(stroke)
                    .corner_radius(CornerRadius::same(5));

                    if ui.add(btn).clicked() && !is_selected {
                        *current = *opt_val;
                        changed = true;
                    }
                }
            });
        });

    changed
}

/// Render 5 color preview dots for preset themes
pub fn render_theme_color_preview(ui: &mut Ui, theme: &str) {
    let palette = match theme {
        "catppuccin-mocha" => [
            Color32::from_rgb(245, 224, 220),
            Color32::from_rgb(249, 226, 175),
            Color32::from_rgb(166, 227, 161),
            Color32::from_rgb(137, 220, 235),
            Color32::from_rgb(180, 190, 254),
        ],
        "tokyo-night" => [
            Color32::from_rgb(192, 202, 245),
            Color32::from_rgb(255, 158, 100),
            Color32::from_rgb(158, 206, 106),
            Color32::from_rgb(125, 205, 245),
            Color32::from_rgb(187, 154, 247),
        ],
        "dracula" => [
            Color32::from_rgb(248, 248, 242),
            Color32::from_rgb(241, 250, 140),
            Color32::from_rgb(80, 250, 123),
            Color32::from_rgb(139, 233, 253),
            Color32::from_rgb(189, 147, 249),
        ],
        "solarized-dark" => [
            Color32::from_rgb(131, 148, 150),
            Color32::from_rgb(181, 137, 0),
            Color32::from_rgb(133, 153, 0),
            Color32::from_rgb(42, 161, 152),
            Color32::from_rgb(108, 113, 196),
        ],
        "nord" => [
            Color32::from_rgb(216, 222, 233),
            Color32::from_rgb(235, 203, 139),
            Color32::from_rgb(163, 190, 140),
            Color32::from_rgb(136, 192, 208),
            Color32::from_rgb(180, 142, 173),
        ],
        _ => [
            Color32::from_rgb(220, 220, 220),
            Color32::from_rgb(245, 190, 80),
            Color32::from_rgb(120, 210, 110),
            Color32::from_rgb(110, 190, 240),
            Color32::from_rgb(170, 140, 230),
        ],
    };

    ui.horizontal(|ui| {
        for col in palette {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(14.0, 14.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 7.0, col);
            ui.painter().circle_stroke(
                rect.center(),
                7.0,
                Stroke::new(1.0, Color32::from_black_alpha(100)),
            );
        }
    });
}

/// Render macOS keyboard badge pill (e.g. ⌘ Cmd, ⌥ Opt, ⌃ Ctrl, ⇧ Shift)
pub fn render_macos_keyboard_badge(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(Color32::from_rgb(42, 45, 58))
        .stroke(Stroke::new(1.0, Color32::from_rgb(68, 72, 92)))
        .corner_radius(CornerRadius::same(5))
        .inner_margin(Margin::symmetric(7, 3))
        .show(ui, |ui| {
            ui.label(
                RichText::new(text)
                    .color(Color32::from_rgb(235, 240, 255))
                    .size(11.5)
                    .strong(),
            );
        });
}

pub fn render_color_row(ui: &mut Ui, label: &str, hex_val: &mut String) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(Color32::from_rgb(230, 234, 242)).size(13.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !hex_val.is_empty() {
                if ui.button("✖").on_hover_text("重設為預設值").clicked() {
                    hex_val.clear();
                }
            }

            let mut current_col = parse_hex_color(hex_val).unwrap_or(Color32::from_rgb(100, 100, 120));
            let resp = ui.color_edit_button_srgba(&mut current_col);
            if resp.changed() {
                *hex_val = format!("#{:02x}{:02x}{:02x}", current_col.r(), current_col.g(), current_col.b());
            }

            ui.add(
                egui::TextEdit::singleline(hex_val)
                    .hint_text("預設 (主題繼承)")
                    .desired_width(110.0),
            );
        });
    });
}

pub fn render_color_row_with_presets(
    ui: &mut Ui,
    label: &str,
    hex_val: &mut String,
    presets: &[(&str, Color32, &str)],
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).color(Color32::from_rgb(230, 234, 242)).size(13.5));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !hex_val.is_empty() {
                if ui.button("✖").on_hover_text("重設為預設值").clicked() {
                    hex_val.clear();
                }
            }

            for (code, col, tooltip) in presets.iter().rev() {
                ui.push_id(code, |ui| {
                    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::new(17.0, 17.0), Sense::click());
                    let stroke = if *hex_val == *code {
                        Stroke::new(2.0, Color32::from_rgb(10, 132, 255))
                    } else if resp.hovered() {
                        Stroke::new(1.3, Color32::WHITE)
                    } else {
                        Stroke::new(0.8, Color32::from_white_alpha(90))
                    };
                    ui.painter().circle_filled(rect.center(), 6.5, *col);
                    ui.painter().circle_stroke(rect.center(), 6.5, stroke);
                    let resp = resp.on_hover_text(format!("{} ({})", tooltip, code));
                    if resp.clicked() {
                        *hex_val = code.to_string();
                    }
                });
            }

            let mut current_col = parse_hex_color(hex_val).unwrap_or(Color32::from_rgb(100, 100, 120));
            let resp = ui.color_edit_button_srgba(&mut current_col);
            if resp.changed() {
                *hex_val = format!("#{:02x}{:02x}{:02x}", current_col.r(), current_col.g(), current_col.b());
            }

            ui.add(
                egui::TextEdit::singleline(hex_val)
                    .hint_text("預設 (主題繼承)")
                    .desired_width(95.0),
            );
        });
    });
}

/// Standalone macOS HIG-compliant Preferences Window state and WGPU rendering context.
pub struct PreferencesWindow {
    pub window: Arc<Window>,
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub egui_ctx: Context,
    pub egui_state: egui_winit::State,
    pub egui_renderer: egui_wgpu::Renderer,

    // Internal navigation & state
    pub active_category: PreferencesCategory,
    pub detected_fonts: Vec<String>,

    // Editable form state
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

    pub last_synced_snap: String,
}

impl PreferencesWindow {
    pub const WINDOW_WIDTH: f64 = 780.0;
    pub const WINDOW_HEIGHT: f64 = 540.0;

    pub async fn create(
        event_loop: &ActiveEventLoop,
        initial_config: &Config,
    ) -> anyhow::Result<Self> {
        let window_attrs = WindowAttributes::default()
            .with_title("skTerm 偏好設定 (Preferences)")
            .with_inner_size(LogicalSize::new(Self::WINDOW_WIDTH, Self::WINDOW_HEIGHT))
            .with_min_inner_size(LogicalSize::new(660.0, 440.0))
            .with_resizable(true);

        #[cfg(target_os = "macos")]
        let window_attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attrs.with_title_hidden(false)
        };

        let window = Arc::new(event_loop.create_window(window_attrs)?);

        let instance = Instance::new(&InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to find suitable GPU adapter for Preferences"))?;

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("skterm_pref_device"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await?;

        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: PresentMode::AutoVsync,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let egui_ctx = Context::default();
        crate::preferences_gui::setup_egui_cjk_fonts(&egui_ctx);
        Self::apply_macos_hig_theme(&egui_ctx);

        let scale = window.scale_factor() as f32;
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::viewport::ViewportId::ROOT,
            &window,
            Some(scale),
            None,
            None,
        );

        let egui_renderer = egui_wgpu::Renderer::new(&device, surface_format, None, 1, false);

        let detected_fonts = scan_available_monospace_fonts();

        let mut instance = Self {
            window,
            surface,
            device,
            queue,
            config,
            egui_ctx,
            egui_state,
            egui_renderer,
            active_category: PreferencesCategory::Appearance,
            detected_fonts,

            font_family: String::new(),
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
            fg_override: String::new(),
            bg_override: String::new(),
            cursor_override: String::new(),
            selection_override: String::new(),

            term: "xterm-256color".to_string(),
            custom_shell_enabled: false,
            shell_program: String::new(),
            shell_args: String::new(),
            working_directory: String::new(),
            scrollback_lines: 10000,
            option_as_alt: "both".to_string(),
            bell: "visual".to_string(),

            cursor_shape: "block".to_string(),
            cursor_blink: true,
            blink_interval_ms: 500,
            last_synced_snap: String::new(),
        };

        instance.load_from_config(initial_config);
        let snap = instance.snapshot_values();
        instance.last_synced_snap = snap;
        Ok(instance)
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn resize(&mut self, new_size: PhysicalSize<u32>) {
        if new_size.width > 0 && new_size.height > 0 {
            self.config.width = new_size.width;
            self.config.height = new_size.height;
            self.surface.configure(&self.device, &self.config);
        }
    }

    pub fn check_changed(&mut self, config_manager: &ConfigManager) -> Option<Config> {
        let cur = self.snapshot_values();
        if cur != self.last_synced_snap {
            self.last_synced_snap = cur;
            Some(self.to_config(&config_manager.current_config))
        } else {
            None
        }
    }

    /// Configure modern macOS HIG aesthetics with dark translucent tones & rounded elements.
    fn apply_macos_hig_theme(ctx: &Context) {
        let mut visuals = Visuals::dark();
        visuals.window_fill = Color32::from_rgb(26, 27, 34);
        visuals.panel_fill = Color32::from_rgb(28, 29, 38);
        visuals.override_text_color = Some(Color32::from_rgb(230, 234, 242));

        // Rounded styling
        visuals.widgets.noninteractive.corner_radius = CornerRadius::same(6);
        visuals.widgets.inactive.corner_radius = CornerRadius::same(6);
        visuals.widgets.hovered.corner_radius = CornerRadius::same(6);
        visuals.widgets.active.corner_radius = CornerRadius::same(6);

        // macOS selection blue
        visuals.selection.bg_fill = Color32::from_rgb(10, 132, 255); // Sonoma Accent Blue
        visuals.selection.stroke = Stroke::NONE;

        ctx.set_visuals(visuals);
    }

    pub fn load_from_config(&mut self, config: &Config) {
        self.font_family = config.font.family.clone();
        self.font_size = config.font.size;
        self.line_height = config.font.line_height;
        self.columns = config.window.columns;
        self.rows = config.window.rows;
        self.padding_x = config.window.padding_x;
        self.padding_y = config.window.padding_y;
        self.opacity = config.window.opacity;
        self.startup_mode = config.window.startup_mode.clone();
        self.decorations = config.window.decorations.clone();
        self.tab_bar_style = config.window.tab_bar_style.clone();

        self.theme = config.colors.theme.clone();
        self.fg_override = config.colors.foreground.clone().unwrap_or_default();
        self.bg_override = config.colors.background.clone().unwrap_or_default();
        self.cursor_override = config.colors.cursor.clone().unwrap_or_default();
        self.selection_override = config.colors.selection.clone().unwrap_or_default();

        self.term = config.terminal.term.clone();
        if let Some(shell) = &config.terminal.shell {
            self.custom_shell_enabled = true;
            self.shell_program = shell.program.clone();
            self.shell_args = shell.args.join(" ");
        } else {
            self.custom_shell_enabled = false;
            self.shell_program = String::new();
            self.shell_args = String::new();
        }
        self.working_directory = config.terminal.working_directory.clone().unwrap_or_default();
        self.scrollback_lines = config.terminal.scrollback_lines;
        self.option_as_alt = config.terminal.option_as_alt.clone();
        self.bell = config.terminal.bell.clone();

        self.cursor_shape = config.cursor.shape.clone();
        self.cursor_blink = config.cursor.blink;
        self.blink_interval_ms = config.cursor.blink_interval_ms;
    }

    pub fn to_config(&self, base: &Config) -> Config {
        let mut cfg = base.clone();
        cfg.font.family = self.font_family.trim().to_string();
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
            let args: Vec<String> = self.shell_args.split_whitespace().map(|s| s.to_string()).collect();
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

    /// Render Preferences Window UI and return Option<Config> if any values were modified (for Live Sync).
    pub fn render_frame(&mut self, config_manager: &ConfigManager) -> Option<Config> {
        let raw_input = self.egui_state.take_egui_input(&self.window);
        let mut changed = false;

        let egui_ctx = self.egui_ctx.clone();
        let full_output = egui_ctx.run(raw_input, |ctx| {
            // macOS Sonoma Sidebar Navigation (with embedded bottom utility actions)
            egui::SidePanel::left("pref_sidebar")
                .exact_width(190.0)
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(22, 23, 30))
                        .inner_margin(Margin::symmetric(14, 18)),
                )
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            // macOS green status dot + title
                            let (rect, _) = ui.allocate_exact_size(egui::Vec2::new(9.0, 9.0), Sense::hover());
                            ui.painter().circle_filled(rect.center(), 4.5, Color32::from_rgb(48, 209, 88));
                            ui.label(RichText::new("skTerm 設定").size(15.0).strong().color(Color32::WHITE));
                        });

                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("即時熱重載同步中")
                                .size(11.0)
                                .color(Color32::from_rgb(120, 130, 155)),
                        );

                        ui.add_space(16.0);

                        self.render_sidebar_item(ui, PreferencesCategory::Appearance, "🔤 外觀與字型", "Appearance");
                        self.render_sidebar_item(ui, PreferencesCategory::Colors, "🎨 色彩與主題", "Colors");
                        self.render_sidebar_item(ui, PreferencesCategory::Terminal, "🖥️ 終端與 Shell", "Terminal");
                        self.render_sidebar_item(ui, PreferencesCategory::Cursor, "🖱️ 游標與動態", "Cursor");
                        self.render_sidebar_item(ui, PreferencesCategory::Keybindings, "⌨️ 快捷鍵指南", "Keybindings");
                    });

                    // Bottom docked actions in Sidebar
                    ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                        ui.add_space(4.0);
                        if ui
                            .button(
                                RichText::new("📝 編輯 config.toml")
                                    .size(12.0)
                                    .color(Color32::from_rgb(180, 195, 225)),
                            )
                            .on_hover_text("在系統文字編輯器開啟 TOML 檔案")
                            .clicked()
                        {
                            let _ = config_manager.open_in_editor();
                        }
                        ui.add_space(4.0);
                        if ui
                            .button(
                                RichText::new("🔄 重設為預設值")
                                    .size(12.0)
                                    .color(Color32::from_rgb(160, 165, 185)),
                            )
                            .on_hover_text("還原當前偏好設定至預設值")
                            .clicked()
                        {
                            self.load_from_config(&Config::default());
                            changed = true;
                        }
                        ui.add_space(8.0);
                        let (divider_rect, _) = ui.allocate_exact_size(
                            egui::Vec2::new(ui.available_width(), 1.0),
                            Sense::hover(),
                        );
                        ui.painter().line_segment(
                            [divider_rect.left_center(), divider_rect.right_center()],
                            Stroke::new(1.0, Color32::from_rgb(42, 45, 60)),
                        );
                        ui.add_space(8.0);
                    });
                });

            // Right Main Content Panel (Grouped Inset Cards)
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(28, 29, 38))
                        .inner_margin(Margin::symmetric(26, 22)),
                )
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let prev_snap = self.snapshot_values();
                        match self.active_category {
                            PreferencesCategory::Appearance => self.render_appearance_view(ui),
                            PreferencesCategory::Colors => self.render_colors_view(ui),
                            PreferencesCategory::Terminal => self.render_terminal_view(ui),
                            PreferencesCategory::Cursor => self.render_cursor_view(ui),
                            PreferencesCategory::Keybindings => self.render_keybindings_view(ui),
                        }
                        let next_snap = self.snapshot_values();
                        if prev_snap != next_snap {
                            changed = true;
                        }
                    });
                });
        });

        self.egui_state.handle_platform_output(&self.window, full_output.platform_output);

        // Render to GPU surface
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(e) => {
                info!("Surface error in preferences window: {:?}", e);
                return if changed { Some(self.to_config(&config_manager.current_config)) } else { None };
            }
        };

        let view = output.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("pref_encoder"),
        });

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let clipped_primitives = self.egui_ctx.tessellate(full_output.shapes, screen_descriptor.pixels_per_point);

        for (id, delta) in full_output.textures_delta.set {
            self.egui_renderer.update_texture(&self.device, &self.queue, id, &delta);
        }

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped_primitives,
            &screen_descriptor,
        );

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("pref_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.11, g: 0.11, b: 0.15, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            }).forget_lifetime();

            self.egui_renderer.render(&mut render_pass, &clipped_primitives, &screen_descriptor);
        }

        for id in full_output.textures_delta.free {
            self.egui_renderer.free_texture(&id);
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        self.check_changed(config_manager)
    }

    fn render_sidebar_item(&mut self, ui: &mut Ui, cat: PreferencesCategory, label: &str, _sub: &str) {
        let is_selected = self.active_category == cat;
        let bg_color = if is_selected {
            Color32::from_rgb(50, 60, 85)
        } else {
            Color32::TRANSPARENT
        };
        let text_color = if is_selected {
            Color32::WHITE
        } else {
            Color32::from_rgb(180, 185, 200)
        };

        let frame = Frame::new()
            .fill(bg_color)
            .corner_radius(CornerRadius::same(7))
            .inner_margin(Margin::symmetric(10, 8));

        let res = frame.show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(label).color(text_color).size(13.5).strong());
            });
        });

        if res.response.interact(Sense::click()).clicked() {
            self.active_category = cat;
        }
        ui.add_space(3.0);
    }

    // 1. Appearance Tab View
    fn render_appearance_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "字型與排版 (Typography)", |b| {
            b.row(
                "主要等寬字型",
                Some("支援複合 Nerd Fonts 與 CJK 字型混合鏈"),
                |ui| {
                    egui::ComboBox::from_id_salt("detected_font_cb")
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
                                "SF Mono",
                            );
                            ui.separator();
                            for f in &self.detected_fonts {
                                ui.selectable_value(&mut self.font_family, f.clone(), f);
                            }
                        });
                },
            );

            b.row("自訂字型名稱", Some("直接輸入字型家族名稱"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.font_family).desired_width(180.0));
            });

            b.row("字型大小 (Font Size)", Some("以 pt 為單位"), |ui| {
                ui.add(egui::Slider::new(&mut self.font_size, 9.0..=36.0).suffix(" pt").step_by(0.5));
            });

            b.row("行高倍率 (Line Height)", Some("1.0 為緊湊，1.2 為標準舒適"), |ui| {
                ui.add(egui::Slider::new(&mut self.line_height, 1.0..=2.0).step_by(0.05));
            });
        });

        render_inset_card(ui, "視窗幾何與邊距 (Window & Layout)", |b| {
            b.row(
                "預設終端大小",
                Some("啟動時初始網格行列數 (列 × 行)"),
                |ui| {
                    ui.add(egui::DragValue::new(&mut self.columns).range(40..=400).prefix("W: "));
                    ui.label("×");
                    ui.add(egui::DragValue::new(&mut self.rows).range(10..=200).prefix("H: "));
                },
            );

            b.row(
                "內距 (Padding)",
                Some("文字四周保留空白像素"),
                |ui| {
                    ui.add(egui::Slider::new(&mut self.padding_x, 0.0..=40.0).suffix(" pt").prefix("X: "));
                    ui.add(egui::Slider::new(&mut self.padding_y, 0.0..=40.0).suffix(" pt").prefix("Y: "));
                },
            );

            b.row(
                "背景不透明度",
                Some("1.0 為完全不透明，搭配毛玻璃效果"),
                |ui| {
                    ui.add(egui::Slider::new(&mut self.opacity, 0.1..=1.0).step_by(0.05));
                },
            );

            b.row(
                "啟動模式 (Startup)",
                Some("應用程式啟動時的視窗狀態"),
                |ui| {
                    let mut cur = self.startup_mode.as_str();
                    let opts = [
                        ("normal", "標準視窗"),
                        ("maximized", "最大化"),
                        ("fullscreen", "全螢幕"),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.startup_mode = cur.to_string();
                    }
                },
            );

            b.row(
                "分頁列顯示策略",
                Some("控制多分頁標籤列何時出現"),
                |ui| {
                    let mut cur = self.tab_bar_style.as_str();
                    let opts = [
                        ("auto", "自動隱藏"),
                        ("always", "恆常顯示"),
                        ("never", "永不顯示"),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.tab_bar_style = cur.to_string();
                    }
                },
            );
        });
    }

    // 2. Colors Tab View
    fn render_colors_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "色彩主題 (Preset Themes)", |b| {
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
                    egui::Grid::new("theme_swatch_grid")
                        .num_columns(2)
                        .spacing(egui::vec2(12.0, 10.0))
                        .show(ui, |ui| {
                            for (idx, (id, label)) in themes.iter().enumerate() {
                                ui.push_id(id, |ui| {
                                    let is_selected = self.theme == *id;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(178.0, 50.0),
                                        Sense::click(),
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
                                        Stroke::new(1.8, Color32::from_rgb(10, 132, 255))
                                    } else if hover {
                                        Stroke::new(1.0, Color32::from_rgb(75, 82, 108))
                                    } else {
                                        Stroke::new(1.0, Color32::from_rgb(48, 51, 68))
                                    };

                                    ui.painter().rect(
                                        rect,
                                        CornerRadius::same(8),
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
                                        rect.left_top() + egui::vec2(12.0, 8.0),
                                        egui::Align2::LEFT_TOP,
                                        label,
                                        egui::FontId::proportional(12.0),
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
                                    let start_x = rect.left() + 12.0;
                                    let dot_y = rect.bottom() - 11.0;
                                    for (i, c_rgba) in dot_colors.iter().enumerate() {
                                        let c32 = Color32::from_rgba_unmultiplied(
                                            (c_rgba[0] * 255.0) as u8,
                                            (c_rgba[1] * 255.0) as u8,
                                            (c_rgba[2] * 255.0) as u8,
                                            255,
                                        );
                                        let dot_x = start_x + (i as f32 * 15.0);
                                        ui.painter().circle_filled(
                                            egui::pos2(dot_x, dot_y),
                                            4.5,
                                            c32,
                                        );
                                        ui.painter().circle_stroke(
                                            egui::pos2(dot_x, dot_y),
                                            4.5,
                                            Stroke::new(0.6, Color32::from_white_alpha(70)),
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

        render_inset_card(ui, "自訂背景與文字顏色 (Custom Overrides)", |b| {
            b.row("背景底色 (Background)", Some("一鍵黑底或點調色盤自訂"), |ui| {
                render_color_row_with_presets(
                    ui,
                    "",
                    &mut self.bg_override,
                    &[
                        ("#000000", Color32::from_rgb(0, 0, 0), "OLED 純黑"),
                        ("#0a0a0e", Color32::from_rgb(10, 10, 14), "深墨黑"),
                        ("#18181f", Color32::from_rgb(24, 24, 31), "藏青黑"),
                    ],
                );
            });
            b.row("前景文字色 (Foreground)", Some("一鍵白/綠/黃或點調色盤自訂"), |ui| {
                render_color_row_with_presets(
                    ui,
                    "",
                    &mut self.fg_override,
                    &[
                        ("#f5f5f7", Color32::from_rgb(245, 245, 247), "亮白"),
                        ("#30d158", Color32::from_rgb(48, 209, 88), "駭客綠"),
                        ("#ff9f0a", Color32::from_rgb(255, 159, 10), "琥珀黃"),
                    ],
                );
            });
            b.row("游標顏色 (Cursor)", Some("游標方塊顏色"), |ui| {
                render_color_row_with_presets(
                    ui,
                    "",
                    &mut self.cursor_override,
                    &[
                        ("#0a84ff", Color32::from_rgb(10, 132, 255), "macOS 藍"),
                        ("#30d158", Color32::from_rgb(48, 209, 88), "翠綠"),
                        ("#ff453a", Color32::from_rgb(255, 69, 58), "鮮紅"),
                    ],
                );
            });
            b.row("選取文字底色 (Selection)", Some("滑鼠選取範圍底色"), |ui| {
                render_color_row_with_presets(
                    ui,
                    "",
                    &mut self.selection_override,
                    &[
                        ("#2a4b7c", Color32::from_rgb(42, 75, 124), "經典藍選取"),
                        ("#383844", Color32::from_rgb(56, 56, 68), "柔和灰選取"),
                    ],
                );
            });
        });
    }

    // 3. Terminal & Shell Tab View
    fn render_terminal_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "Shell 與啟動環境 (Shell & Environment)", |b| {
            b.row("TERM 模擬器代碼", Some("環境變數 $TERM 預設值"), |ui| {
                ui.add(egui::TextEdit::singleline(&mut self.term).desired_width(140.0));
            });

            b.row("指定自訂 Shell 程式", Some("覆蓋系統預設 $SHELL"), |ui| {
                render_macos_toggle_switch(ui, &mut self.custom_shell_enabled);
            });

            if self.custom_shell_enabled {
                b.row("執行檔路徑", None, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shell_program)
                            .hint_text("/bin/zsh")
                            .desired_width(180.0),
                    );
                });

                b.row("啟動參數 (Args)", None, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shell_args)
                            .hint_text("--login")
                            .desired_width(180.0),
                    );
                });
            }

            b.row("初始工作目錄 (CWD)", Some("終端啟動預設路徑"), |ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.working_directory)
                        .hint_text("預設使用者家目錄 (~/)")
                        .desired_width(180.0),
                );
            });
        });

        render_inset_card(ui, "緩衝區與操作行為 (Buffer & Interaction)", |b| {
            b.row(
                "滾動歷史行數 (Scrollback)",
                Some("保留向上捲動緩衝行數"),
                |ui| {
                    ui.add(
                        egui::Slider::new(&mut self.scrollback_lines, 1000..=50000)
                            .step_by(1000.0)
                            .suffix(" 行"),
                    );
                },
            );

            b.row(
                "macOS Option 鍵模式",
                Some("是否傳遞 Meta (Esc+) 序列"),
                |ui| {
                    let mut cur = self.option_as_alt.as_str();
                    let opts = [
                        ("both", "左右均作 Meta"),
                        ("only_left", "僅左側"),
                        ("only_right", "僅右側"),
                        ("none", "關閉"),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
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
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.bell = cur.to_string();
                    }
                },
            );
        });
    }

    // 4. Cursor Tab View
    fn render_cursor_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "游標樣式與動態 (Cursor Style)", |b| {
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
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.cursor_shape = cur.to_string();
                    }
                },
            );

            b.row(
                "游標閃爍 (Blinking)",
                Some("開啟游標平滑呼吸動畫"),
                |ui| {
                    render_macos_toggle_switch(ui, &mut self.cursor_blink);
                },
            );

            if self.cursor_blink {
                b.row(
                    "閃爍週期 (Interval)",
                    Some("游標閃爍頻率毫秒"),
                    |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.blink_interval_ms, 200..=1000)
                                .step_by(50.0)
                                .suffix(" ms"),
                        );
                    },
                );
            }
        });
    }

    // 5. Keybindings Tab View
    fn render_keybindings_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "常用快捷鍵速查 (macOS HIG Cheatsheet)", |b| {
            let shortcuts: &[(&str, &[&str], &[&str])] = &[
                ("開啟偏好設定視窗", &["⌘", "，"], &["Ctrl", "Shift", "，"]),
                ("新增分頁 (New Tab)", &["⌘", "T"], &["Ctrl", "Shift", "T"]),
                ("關閉分頁 / 窗格", &["⌘", "W"], &["Ctrl", "Shift", "W"]),
                ("左右垂直切分窗格", &["⌘", "D"], &["Ctrl", "Shift", "D"]),
                ("上下水平切分窗格", &["⌘", "⇧", "D"], &["Ctrl", "Shift", "H"]),
                ("字型放大", &["⌘", "="], &["Ctrl", "="]),
                ("字型縮小", &["⌘", "-"], &["Ctrl", "-"]),
                ("字型重設預設大小", &["⌘", "0"], &["Ctrl", "0"]),
                ("切換焦點相鄰窗格", &["⌘", "["], &["Ctrl", "["]),
                ("切換焦點相鄰分頁", &["⌘", "⇧", "["], &["Ctrl", "Tab"]),
                ("切換指定第 1~9 分頁", &["⌘", "1~9"], &["Ctrl", "1~9"]),
                ("複製 / 貼上", &["⌘", "C / V"], &["Ctrl", "Shift", "C / V"]),
                ("Shell EOF (登出連線)", &["⌃", "D"], &["Ctrl", "D"]),
                ("中斷行程 (SIGINT)", &["⌃", "C"], &["Ctrl", "C"]),
            ];

            for (desc, mac_keys, win_keys) in shortcuts {
                b.row(desc, None, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("macOS:").color(Color32::from_rgb(135, 142, 160)).size(11.0));
                        for k in *mac_keys {
                            render_macos_keyboard_badge(ui, k);
                        }
                        ui.add_space(8.0);
                        ui.label(RichText::new("Linux/Win:").color(Color32::from_rgb(110, 115, 130)).size(10.5));
                        for k in *win_keys {
                            render_macos_keyboard_badge(ui, k);
                        }
                    });
                });
            }
        });
    }

    fn snapshot_values(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.font_family,
            self.font_size,
            self.line_height,
            self.columns,
            self.rows,
            self.padding_x,
            self.padding_y,
            self.opacity,
            self.startup_mode,
            self.tab_bar_style,
            self.theme,
            self.fg_override,
            self.bg_override,
            self.cursor_override,
            self.selection_override,
            self.term,
            self.custom_shell_enabled,
            self.shell_program,
            self.shell_args,
            self.cursor_shape,
            self.cursor_blink,
            self.blink_interval_ms
        )
    }
}

fn parse_hex_color(hex: &str) -> Option<Color32> {
    let s = hex.trim().strip_prefix('#').unwrap_or(hex.trim());
    if s.len() == 6 {
        let r = u8::from_str_radix(&s[0..2], 16).ok()?;
        let g = u8::from_str_radix(&s[2..4], 16).ok()?;
        let b = u8::from_str_radix(&s[4..6], 16).ok()?;
        Some(Color32::from_rgb(r, g, b))
    } else {
        None
    }
}

/// Scan system font directories to detect installed monospace fonts for autocomplete
pub fn scan_available_monospace_fonts() -> Vec<String> {
    let mut search_dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        let home_p = PathBuf::from(home);
        search_dirs.push(home_p.join("Library/Fonts"));
        search_dirs.push(home_p.join(".local/share/fonts"));
    }
    search_dirs.push(PathBuf::from("/Library/Fonts"));
    search_dirs.push(PathBuf::from("/System/Library/Fonts"));
    search_dirs.push(PathBuf::from("/usr/share/fonts"));

    let mut fonts = Vec::new();
    for dir in search_dirs {
        if dir.exists() {
            if let Ok(entries) = fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        let ext_l = ext.to_lowercase();
                        if ext_l == "ttf" || ext_l == "otf" || ext_l == "ttc" {
                            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                                let stem_l = stem.to_lowercase();
                                if stem_l.contains("mono") || stem_l.contains("nerd") || stem_l.contains("code") {
                                    if !fonts.contains(&stem.to_string()) {
                                        fonts.push(stem.to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    fonts.sort();
    fonts.truncate(30);
    fonts
}
