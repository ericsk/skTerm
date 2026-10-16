//! Dedicated Native Preferences Window adhering to macOS Human Interface Guidelines (HIG).
//! Features a left category sidebar, rounded grouped inset cards, native toggle switches,
//! auto-discovered system monospace fonts, color picker, and real-time live preview.

use crate::config::{Config, ConfigManager, ShellConfig};
use egui::{
    Align, Color32, Context, CornerRadius, Frame, Layout, Margin, RichText, Sense, Stroke, Ui,
    Visuals,
};
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
                Stroke::new(1.0_f32, Color32::from_rgb(46, 49, 63)),
            );
            self.ui.add_space(8.0);
        } else {
            self.ui.add_space(2.0);
        }

        self.ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(label)
                        .color(Color32::from_rgb(230, 234, 242))
                        .size(13.5),
                );
                if let Some(sub) = subtitle {
                    ui.label(
                        RichText::new(sub)
                            .color(Color32::from_rgb(135, 142, 160))
                            .size(11.0),
                    );
                }
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                content(ui);
            });
        });

        self.ui.add_space(6.0);
        self.row_count += 1;
    }

    /// A row whose content spans the full card width, stacked under the label.
    ///
    /// `row` puts the content in a right-aligned column, which is fine for a
    /// slider or a checkbox but far too cramped for a font list.
    pub fn block<F>(&mut self, label: &str, subtitle: Option<&str>, content: F)
    where
        F: FnOnce(&mut Ui),
    {
        if self.row_count > 0 {
            self.ui.add_space(4.0);
            let avail_w = self.ui.available_width().max(10.0);
            let min_pos = self.ui.cursor().min;
            let end_pos = min_pos + egui::vec2(avail_w, 0.0);
            self.ui.painter().line_segment(
                [min_pos, end_pos],
                Stroke::new(1.0_f32, Color32::from_rgb(46, 49, 63)),
            );
            self.ui.add_space(8.0);
        } else {
            self.ui.add_space(2.0);
        }

        self.ui.vertical(|ui| {
            ui.label(
                RichText::new(label)
                    .color(Color32::from_rgb(230, 234, 242))
                    .size(13.5),
            );
            if let Some(sub) = subtitle {
                ui.label(
                    RichText::new(sub)
                        .color(Color32::from_rgb(135, 142, 160))
                        .size(11.0),
                );
            }
            ui.add_space(6.0);
            content(ui);
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
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(52, 55, 72)))
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

    ui.painter()
        .rect_filled(rect, CornerRadius::same(11), bg_color);

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
    ui.painter()
        .circle_filled(knob_center, knob_radius, Color32::WHITE);

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
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 49, 64)))
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
                            Stroke::new(1.0_f32, Color32::from_rgb(82, 87, 112)),
                        )
                    } else {
                        (
                            Color32::TRANSPARENT,
                            Color32::from_rgb(145, 152, 172),
                            Stroke::NONE,
                        )
                    };

                    let btn = egui::Button::new(
                        RichText::new(*opt_label)
                            .color(text_col)
                            .size(12.0)
                            .strong(),
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
                Stroke::new(1.0_f32, Color32::from_black_alpha(100)),
            );
        }
    });
}

/// Render macOS keyboard badge pill (e.g. ⌘ Cmd, ⌥ Opt, ⌃ Ctrl, ⇧ Shift)
pub fn render_macos_keyboard_badge(ui: &mut Ui, text: &str) {
    Frame::new()
        .fill(Color32::from_rgb(42, 45, 58))
        .stroke(Stroke::new(1.0_f32, Color32::from_rgb(68, 72, 92)))
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
        ui.label(
            RichText::new(label)
                .color(Color32::from_rgb(230, 234, 242))
                .size(13.5),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if !hex_val.is_empty() && ui.button("✖").on_hover_text("重設為預設值").clicked()
            {
                hex_val.clear();
            }

            let mut current_col =
                parse_hex_color(hex_val).unwrap_or(Color32::from_rgb(100, 100, 120));
            let resp = ui.color_edit_button_srgba(&mut current_col);
            if resp.changed() {
                *hex_val = format!(
                    "#{:02x}{:02x}{:02x}",
                    current_col.r(),
                    current_col.g(),
                    current_col.b()
                );
            }

            ui.add(
                egui::TextEdit::singleline(hex_val)
                    .hint_text("預設 (主題繼承)")
                    .desired_width(110.0),
            );
        });
    });
}

pub fn rgba_to_color32(rgba: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (rgba[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (rgba[2].clamp(0.0, 1.0) * 255.0).round() as u8,
        255,
    )
}

pub fn render_color_row_with_presets(
    ui: &mut Ui,
    fallback_col: Color32,
    hex_val: &mut String,
    presets: &[(&str, Color32, &str)],
) {
    // Called inside `b.row`, which uses `Layout::right_to_left`.
    // Wrapping in `ui.horizontal` restores natural left-to-right ordering inside the right-aligned slot.
    ui.horizontal(|ui| {
        for (code, col, tooltip) in presets {
            ui.push_id(code, |ui| {
                let (rect, resp) =
                    ui.allocate_exact_size(egui::Vec2::new(18.0, 18.0), Sense::click());
                let stroke = if hex_val.eq_ignore_ascii_case(code) {
                    Stroke::new(2.0_f32, Color32::from_rgb(10, 132, 255))
                } else if resp.hovered() {
                    Stroke::new(1.3_f32, Color32::WHITE)
                } else {
                    Stroke::new(0.8_f32, Color32::from_white_alpha(90))
                };
                ui.painter().circle_filled(rect.center(), 7.0, *col);
                ui.painter().circle_stroke(rect.center(), 7.0, stroke);
                let resp = resp.on_hover_text(format!("{} ({})", tooltip, code));
                if resp.clicked() {
                    *hex_val = code.to_string();
                }
            });
        }

        let mut current_col = parse_hex_color(hex_val).unwrap_or(fallback_col);
        let resp = ui
            .color_edit_button_srgba(&mut current_col)
            .on_hover_text("點擊開啟調色盤");
        if resp.changed() {
            *hex_val = format!(
                "#{:02x}{:02x}{:02x}",
                current_col.r(),
                current_col.g(),
                current_col.b()
            );
        }

        ui.add(
            egui::TextEdit::singleline(hex_val)
                .hint_text("預設 (主題繼承)")
                .desired_width(96.0)
                .font(egui::TextStyle::Monospace),
        );

        if !hex_val.trim().is_empty()
            && ui
                .button("✖")
                .on_hover_text("清除自訂值，重設為主題預設顏色")
                .clicked()
        {
            hex_val.clear();
        }
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
    pub should_close: bool,
    pub show_all_fonts: bool,
    pub show_manual_font_input: bool,
    pub show_ansi_palette: bool,
    pub keybinding_filter: String,
    pub keybinding_os_mode: u8,

    // Editable form state
    /// Committed value, compared against the live config to detect changes.
    pub font_family: String,
    /// What the user is currently typing in manual mode.
    pub font_family_input: String,
    /// Cached `family -> resolved file` rows shown under the input.
    pub font_preview: Vec<(String, Option<String>)>,
    /// The input string `font_preview` was built from, so it is only recomputed
    /// when the text actually changes rather than every frame.
    pub font_preview_src: String,
    /// Search box inside the "add installed font" picker.
    pub font_picker_filter: String,
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
    pub language: String,

    pub theme: String,
    pub fg_override: String,
    pub bg_override: String,
    pub cursor_override: String,
    pub selection_override: String,
    pub palette_overrides: [String; 16],

    pub term: String,
    pub custom_shell_enabled: bool,
    pub shell_program: String,
    pub shell_args: String,
    pub working_directory: String,
    pub scrollback_lines: usize,
    pub option_as_alt: String,
    pub bell: String,
    pub right_click_action: String,

    pub cursor_shape: String,
    pub cursor_blink: bool,
    pub blink_interval_ms: u64,
}

impl PreferencesWindow {
    pub const WINDOW_WIDTH: f64 = 820.0;
    pub const WINDOW_HEIGHT: f64 = 600.0;

    pub async fn create(
        event_loop: &ActiveEventLoop,
        initial_config: &Config,
    ) -> anyhow::Result<Self> {
        let window_attrs = WindowAttributes::default()
            .with_title("skTerm 偏好設定 (Preferences)")
            .with_inner_size(LogicalSize::new(Self::WINDOW_WIDTH, Self::WINDOW_HEIGHT))
            .with_min_inner_size(LogicalSize::new(680.0, 460.0))
            .with_resizable(true);

        #[cfg(target_os = "macos")]
        let window_attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attrs.with_title_hidden(false)
        };

        let window_attrs = crate::icon::configure_window_attributes(window_attrs);

        let window = Arc::new(event_loop.create_window(window_attrs)?);
        crate::icon::apply_window_icon(&window);

        let instance = Instance::new(&InstanceDescriptor::default());
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| {
                anyhow::anyhow!("Failed to find suitable GPU adapter for Preferences")
            })?;

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
        crate::egui_fonts::setup_egui_cjk_fonts(&egui_ctx);
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

        let detected_fonts = skterm_render::atlas::installed_font_families();

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
            should_close: false,
            show_all_fonts: false,
            show_manual_font_input: false,
            show_ansi_palette: false,
            keybinding_filter: String::new(),
            keybinding_os_mode: 0,

            font_family: String::new(),
            font_family_input: String::new(),
            font_preview: Vec::new(),
            font_preview_src: String::new(),
            font_picker_filter: String::new(),
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
            language: "auto".to_string(),

            theme: "catppuccin-mocha".to_string(),
            fg_override: String::new(),
            bg_override: String::new(),
            cursor_override: String::new(),
            selection_override: String::new(),
            palette_overrides: std::array::from_fn(|_| String::new()),

            term: "xterm-256color".to_string(),
            custom_shell_enabled: false,
            shell_program: String::new(),
            shell_args: String::new(),
            working_directory: String::new(),
            scrollback_lines: 10000,
            option_as_alt: "both".to_string(),
            bell: "visual".to_string(),
            right_click_action: "menu".to_string(),

            cursor_shape: "block".to_string(),
            cursor_blink: true,
            blink_interval_ms: 500,
        };

        instance.load_from_config(initial_config);
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

    /// Returns the form's configuration if it differs from what is currently applied.
    pub fn check_changed(&mut self, config_manager: &ConfigManager) -> Option<Config> {
        let cur = self.to_config(&config_manager.current_config);
        if cur != config_manager.current_config {
            Some(cur)
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

    /// Re-sync the runtime-derived window geometry into the form.
    pub fn sync_window_geometry(&mut self, columns: u16, rows: u16) {
        self.columns = columns;
        self.rows = rows;
    }

    pub fn load_from_config(&mut self, config: &Config) {
        self.font_family = config.font.family.clone();
        self.font_family_input = config.font.family.clone();
        self.font_preview_src.clear();
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
        self.language = config.window.language.clone();

        self.theme = config.colors.theme.clone();
        self.fg_override = config.colors.foreground.clone().unwrap_or_default();
        self.bg_override = config.colors.background.clone().unwrap_or_default();
        self.cursor_override = config.colors.cursor.clone().unwrap_or_default();
        self.selection_override = config.colors.selection.clone().unwrap_or_default();
        if let Some(pal) = &config.colors.palette {
            for idx in 0..16 {
                self.palette_overrides[idx] =
                    pal.get_index(idx).map(|s| s.to_string()).unwrap_or_default();
            }
        } else {
            for slot in &mut self.palette_overrides {
                slot.clear();
            }
        }

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
        self.working_directory = config
            .terminal
            .working_directory
            .clone()
            .unwrap_or_default();
        self.scrollback_lines = config.terminal.scrollback_lines;
        self.option_as_alt = config.terminal.option_as_alt.clone();
        self.bell = config.terminal.bell.clone();
        self.right_click_action = config.terminal.right_click_action.clone();

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
        cfg.window.language = self.language.clone();

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

        let mut pal = crate::config::PaletteConfig::default();
        for (idx, val) in self.palette_overrides.iter().enumerate() {
            let trimmed = val.trim();
            if !trimmed.is_empty() {
                pal.set_index(idx, Some(trimmed.to_string()));
            }
        }
        cfg.colors.palette = if pal.is_all_none() { None } else { Some(pal) };

        cfg.terminal.term = self.term.clone();
        if self.custom_shell_enabled && !self.shell_program.trim().is_empty() {
            let args: Vec<String> = self
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
        cfg.terminal.right_click_action = self.right_click_action.clone();

        cfg.cursor.shape = self.cursor_shape.clone();
        cfg.cursor.blink = self.cursor_blink;
        cfg.cursor.blink_interval_ms = self.blink_interval_ms;

        cfg
    }

    /// Render Preferences Window UI and return Option<Config> if any values were modified (for Live Sync).
    pub fn render_frame(&mut self, config_manager: &ConfigManager) -> Option<Config> {
        let raw_input = self.egui_state.take_egui_input(&self.window);

        let egui_ctx = self.egui_ctx.clone();
        let full_output = egui_ctx.run(raw_input, |ctx| {
            // Support closing the Preferences window via Esc (when no text box has focus) or Ctrl/Cmd+W
            let close_shortcut = ctx.input(|i| {
                let esc = i.key_pressed(egui::Key::Escape) && !ctx.wants_keyboard_input();
                let cmd_w = i.key_pressed(egui::Key::W) && (i.modifiers.command || i.modifiers.ctrl);
                esc || cmd_w
            });
            if close_shortcut {
                self.should_close = true;
            }

            // macOS Sonoma Sidebar Navigation (with embedded bottom utility actions)
            egui::SidePanel::left("pref_sidebar")
                .exact_width(195.0)
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(22, 23, 30))
                        .inner_margin(Margin::symmetric(14, 18)),
                )
                .show(ctx, |ui| {
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            // macOS green status dot + title
                            let (rect, _) =
                                ui.allocate_exact_size(egui::Vec2::new(9.0, 9.0), Sense::hover());
                            ui.painter().circle_filled(
                                rect.center(),
                                4.5,
                                Color32::from_rgb(48, 209, 88),
                            );
                            ui.label(
                                RichText::new("skTerm 偏好設定")
                                    .size(15.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                        });

                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("即時熱重載同步中 (Live Sync)")
                                .size(11.0)
                                .color(Color32::from_rgb(120, 130, 155)),
                        );

                        ui.add_space(16.0);

                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Appearance,
                            "🔤 外觀與字型",
                            "Appearance",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Colors,
                            "🎨 色彩與主題",
                            "Colors",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Terminal,
                            "🖥️ 終端與 Shell",
                            "Terminal",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Cursor,
                            "🖱️ 游標與動態",
                            "Cursor",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Keybindings,
                            "⌨️ 快捷鍵指南",
                            "Keybindings",
                        );
                    });

                    // Bottom docked actions in Sidebar
                    ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                        ui.add_space(2.0);
                        if ui
                            .button(
                                RichText::new("✕ 關閉視窗 (Esc)")
                                    .size(12.0)
                                    .color(Color32::from_rgb(210, 155, 160)),
                            )
                            .on_hover_text("關閉偏好設定視窗 (快速鍵: Esc 或 Ctrl/⌘+W)")
                            .clicked()
                        {
                            self.should_close = true;
                        }
                        ui.add_space(4.0);
                        if ui
                            .button(
                                RichText::new("📝 編輯 config.toml")
                                    .size(12.0)
                                    .color(Color32::from_rgb(180, 195, 225)),
                            )
                            .on_hover_text("在系統文字編輯器開啟 TOML 設定檔")
                            .clicked()
                        {
                            config_manager.open_in_editor();
                        }
                        ui.add_space(4.0);
                        if ui
                            .button(
                                RichText::new("🔄 重設為預設值")
                                    .size(12.0)
                                    .color(Color32::from_rgb(160, 165, 185)),
                            )
                            .on_hover_text("還原所有偏好設定至出廠預設值")
                            .clicked()
                        {
                            self.load_from_config(&Config::default());
                        }
                        ui.add_space(8.0);
                        let (divider_rect, _) = ui.allocate_exact_size(
                            egui::Vec2::new(ui.available_width(), 1.0),
                            Sense::hover(),
                        );
                        ui.painter().line_segment(
                            [divider_rect.left_center(), divider_rect.right_center()],
                            Stroke::new(1.0_f32, Color32::from_rgb(42, 45, 60)),
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
                    egui::ScrollArea::vertical().show(ui, |ui| match self.active_category {
                        PreferencesCategory::Appearance => self.render_appearance_view(ui),
                        PreferencesCategory::Colors => self.render_colors_view(ui),
                        PreferencesCategory::Terminal => self.render_terminal_view(ui),
                        PreferencesCategory::Cursor => self.render_cursor_view(ui),
                        PreferencesCategory::Keybindings => self.render_keybindings_view(ui),
                    });
                });
        });

        self.egui_state
            .handle_platform_output(&self.window, full_output.platform_output);

        // Render to GPU surface
        let output = match self.surface.get_current_texture() {
            Ok(t) => t,
            Err(e) => {
                info!("Surface error in preferences window: {:?}", e);
                return self.check_changed(config_manager);
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("pref_encoder"),
            });

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.config.width, self.config.height],
            pixels_per_point: self.window.scale_factor() as f32,
        };

        let clipped_primitives = self
            .egui_ctx
            .tessellate(full_output.shapes, screen_descriptor.pixels_per_point);

        for (id, delta) in full_output.textures_delta.set {
            self.egui_renderer
                .update_texture(&self.device, &self.queue, id, &delta);
        }

        self.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &clipped_primitives,
            &screen_descriptor,
        );

        {
            let mut render_pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("pref_render_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.11,
                                g: 0.11,
                                b: 0.15,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                })
                .forget_lifetime();

            self.egui_renderer
                .render(&mut render_pass, &clipped_primitives, &screen_descriptor);
        }

        for id in full_output.textures_delta.free {
            self.egui_renderer.free_texture(&id);
        }

        self.queue.submit(Some(encoder.finish()));
        output.present();

        self.check_changed(config_manager)
    }

    fn render_sidebar_item(
        &mut self,
        ui: &mut Ui,
        cat: PreferencesCategory,
        label: &str,
        _sub: &str,
    ) {
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

    /// Resolve effective theme + custom overrides for live preview inside Preferences.
    fn effective_colors(&self) -> (Color32, Color32, Color32, Color32, [Color32; 16]) {
        let preset = crate::config::Theme::get(&self.theme);
        let bg = parse_hex_color(&self.bg_override).unwrap_or_else(|| rgba_to_color32(preset.background));
        let fg = parse_hex_color(&self.fg_override).unwrap_or_else(|| rgba_to_color32(preset.foreground));
        let cur = parse_hex_color(&self.cursor_override).unwrap_or_else(|| rgba_to_color32(preset.cursor));
        let sel = parse_hex_color(&self.selection_override).unwrap_or_else(|| rgba_to_color32(preset.selection));
        let pal = std::array::from_fn(|i| {
            parse_hex_color(&self.palette_overrides[i])
                .unwrap_or_else(|| rgba_to_color32(preset.palette[i]))
        });
        (bg, fg, cur, sel, pal)
    }

    /// Render a live terminal preview card reflecting current theme, overrides, cursor, and line height.
    fn render_terminal_live_preview(&self, ui: &mut Ui) {
        let (bg, fg, cur_col, sel_col, pal) = self.effective_colors();

        Frame::new()
            .fill(bg)
            .stroke(Stroke::new(1.2_f32, Color32::from_rgb(58, 62, 82)))
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                let row_spacing = ((self.line_height - 1.0).max(0.0) * 14.0).round();

                // Header bar inside preview
                ui.horizontal(|ui| {
                    for dot_col in [
                        Color32::from_rgb(255, 95, 86),
                        Color32::from_rgb(255, 189, 46),
                        Color32::from_rgb(39, 201, 63),
                    ] {
                        let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), Sense::hover());
                        ui.painter().circle_filled(r.center(), 4.5, dot_col);
                    }
                    ui.add_space(6.0);
                    let font_summary = if self.font_family.trim().is_empty() {
                        "系統預設等寬字型".to_string()
                    } else {
                        self.font_family
                            .split(',')
                            .next()
                            .unwrap_or("預設")
                            .trim()
                            .to_string()
                    };
                    ui.label(
                        RichText::new(format!(
                            "即時預覽 — 主題: {} ｜ 字型: {} ({:.1}pt, 行高 {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ))
                        .size(11.0)
                        .color(fg.gamma_multiply(0.65)),
                    );
                });

                ui.add_space(6.0 + row_spacing * 0.5);

                // Line 1: Prompt + command
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(
                        RichText::new("ericsk@skterm")
                            .monospace()
                            .size(12.5)
                            .strong()
                            .color(pal[2]),
                    );
                    ui.label(RichText::new(":").monospace().size(12.5).color(fg));
                    ui.label(
                        RichText::new("~/Projects/skTerm")
                            .monospace()
                            .size(12.5)
                            .strong()
                            .color(pal[4]),
                    );
                    ui.label(
                        RichText::new(" (main) ")
                            .monospace()
                            .size(12.5)
                            .color(pal[3]),
                    );
                    ui.label(RichText::new("$ ").monospace().size(12.5).color(pal[6]));
                    ui.label(
                        RichText::new("cargo test --workspace")
                            .monospace()
                            .size(12.5)
                            .color(fg),
                    );
                });

                ui.add_space(row_spacing);

                // Line 2: Simulated selection highlight + ANSI status colors
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(
                        RichText::new("   Compiling")
                            .monospace()
                            .size(12.0)
                            .strong()
                            .color(pal[2]),
                    );
                    Frame::new()
                        .fill(sel_col)
                        .corner_radius(CornerRadius::same(3))
                        .inner_margin(Margin::symmetric(4, 1))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("skterm-gui v0.1.0 (選取反白預覽)")
                                    .monospace()
                                    .size(12.0)
                                    .color(fg),
                            );
                        });
                    ui.label(
                        RichText::new("WARN: 0")
                            .monospace()
                            .size(12.0)
                            .color(pal[3]),
                    );
                    ui.label(
                        RichText::new("ERR: 0")
                            .monospace()
                            .size(12.0)
                            .color(pal[1]),
                    );
                });

                ui.add_space(row_spacing);

                // Line 3: Active cursor preview
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.label(
                        RichText::new("✓ 42 passed; 0 failed  $")
                            .monospace()
                            .size(12.5)
                            .color(pal[10]),
                    );
                    ui.add_space(6.0);
                    let (cur_rect, _) =
                        ui.allocate_exact_size(egui::vec2(9.0, 16.0), Sense::hover());
                    match self.cursor_shape.as_str() {
                        "beam" => {
                            let beam = egui::Rect::from_min_size(
                                cur_rect.min,
                                egui::vec2(2.2, cur_rect.height()),
                            );
                            ui.painter().rect_filled(beam, CornerRadius::ZERO, cur_col);
                        }
                        "underline" => {
                            let ul = egui::Rect::from_min_size(
                                egui::pos2(cur_rect.min.x, cur_rect.max.y - 2.5),
                                egui::vec2(cur_rect.width(), 2.5),
                            );
                            ui.painter().rect_filled(ul, CornerRadius::ZERO, cur_col);
                        }
                        _ => {
                            ui.painter()
                                .rect_filled(cur_rect, CornerRadius::same(1), cur_col);
                        }
                    }
                });
            });
        ui.add_space(12.0);
    }

    fn render_font_picker_popup(&mut self, ui: &mut Ui) -> Option<String> {
        ui.add(
            egui::TextEdit::singleline(&mut self.font_picker_filter)
                .hint_text("🔍 搜尋字型名稱...")
                .desired_width(250.0),
        );
        ui.add_space(4.0);
        let mut only_recommended = !self.show_all_fonts;
        if ui
            .checkbox(
                &mut only_recommended,
                "僅顯示終端機推薦字型 (等寬 / Nerd / CJK)",
            )
            .changed()
        {
            self.show_all_fonts = !only_recommended;
        }
        ui.add_space(4.0);

        let needle = self.font_picker_filter.to_lowercase();
        let matches: Vec<&String> = self
            .detected_fonts
            .iter()
            .filter(|f| {
                let name_match = needle.is_empty() || f.to_lowercase().contains(&needle);
                let type_match =
                    self.show_all_fonts || skterm_render::is_terminal_friendly_font(f);
                name_match && type_match
            })
            .collect();

        ui.label(
            RichText::new(format!(
                "符合條件：{} / {} 個系統字型家族",
                matches.len(),
                self.detected_fonts.len()
            ))
            .size(10.5)
            .color(Color32::from_rgb(120, 128, 145)),
        );
        ui.add_space(2.0);

        let mut pick: Option<String> = None;
        egui::ScrollArea::vertical()
            .max_height(250.0)
            .show(ui, |ui| {
                for f in matches {
                    if ui.selectable_label(false, f).clicked() {
                        pick = Some(f.clone());
                    }
                }
            });
        pick
    }

    // 1. Appearance Tab View
    fn render_appearance_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "字型與備援鏈 (Typography & Fallback Chain)", |b| {
            b.block(
                "字型家族與優先順序 (Font Family & Fallback Order)",
                Some("排在第一順位的為主字型，遇到缺字（如 Nerd 圖示或中文）時自動依序往後遞補。"),
                |ui| {
                    // Rebuild resolution preview only when committed font_family changes
                    if self.font_preview_src != self.font_family {
                        self.font_preview_src = self.font_family.clone();
                        self.font_preview = self
                            .font_family
                            .split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(|name| {
                                let resolved =
                                    skterm_render::atlas::resolve_font_path(name).map(|p| {
                                        p.file_name()
                                            .map(|f| f.to_string_lossy().into_owned())
                                            .unwrap_or_default()
                                    });
                                (name.to_string(), resolved)
                            })
                            .collect();
                    }

                    let mut chain: Vec<String> = self
                        .font_family
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                    let mut chain_modified = false;

                    if self.font_preview.is_empty() {
                        Frame::new()
                            .fill(Color32::from_rgb(26, 28, 37))
                            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 49, 64)))
                            .corner_radius(CornerRadius::same(7))
                            .inner_margin(Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("目前未指定自訂字型，使用系統預設等寬字型。")
                                        .size(12.0)
                                        .color(Color32::from_rgb(150, 158, 178)),
                                );
                            });
                    } else {
                        let len = self.font_preview.len();
                        let mut move_up: Option<usize> = None;
                        let mut move_down: Option<usize> = None;
                        let mut remove_idx: Option<usize> = None;

                        for (idx, (name, resolved)) in self.font_preview.iter().enumerate() {
                            Frame::new()
                                .fill(Color32::from_rgb(26, 28, 37))
                                .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 49, 64)))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::symmetric(10, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let badge_text = if idx == 0 {
                                            "主字型".to_string()
                                        } else {
                                            format!("備援 #{}", idx)
                                        };
                                        let badge_bg = if idx == 0 {
                                            Color32::from_rgb(28, 68, 125)
                                        } else {
                                            Color32::from_rgb(42, 45, 58)
                                        };
                                        Frame::new()
                                            .fill(badge_bg)
                                            .corner_radius(CornerRadius::same(4))
                                            .inner_margin(Margin::symmetric(6, 2))
                                            .show(ui, |ui| {
                                                ui.label(
                                                    RichText::new(badge_text)
                                                        .size(10.5)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                            });

                                        match resolved {
                                            Some(file) => {
                                                ui.label(
                                                    RichText::new("●")
                                                        .size(10.5)
                                                        .color(Color32::from_rgb(90, 200, 120)),
                                                );
                                                ui.label(
                                                    RichText::new(name)
                                                        .size(12.5)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                                ui.label(
                                                    RichText::new(format!("→ {}", file))
                                                        .size(11.0)
                                                        .color(Color32::from_rgb(135, 142, 160)),
                                                );
                                            }
                                            None => {
                                                ui.label(
                                                    RichText::new("●")
                                                        .size(10.5)
                                                        .color(Color32::from_rgb(235, 110, 100)),
                                                );
                                                ui.label(
                                                    RichText::new(name)
                                                        .size(12.5)
                                                        .color(Color32::from_rgb(235, 180, 175)),
                                                );
                                                ui.label(
                                                    RichText::new("→ 找不到此字型，將自動略過")
                                                        .size(11.0)
                                                        .color(Color32::from_rgb(235, 110, 100)),
                                                );
                                            }
                                        }

                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            if ui
                                                .small_button("✖")
                                                .on_hover_text("從字型鏈移除")
                                                .clicked()
                                            {
                                                remove_idx = Some(idx);
                                            }
                                            if ui
                                                .add_enabled(
                                                    idx + 1 < len,
                                                    egui::Button::new("↓").small(),
                                                )
                                                .on_hover_text("往後移（降低優先順序）")
                                                .clicked()
                                            {
                                                move_down = Some(idx);
                                            }
                                            if ui
                                                .add_enabled(
                                                    idx > 0,
                                                    egui::Button::new("↑").small(),
                                                )
                                                .on_hover_text("往前移（提高優先順序）")
                                                .clicked()
                                            {
                                                move_up = Some(idx);
                                            }
                                        });
                                    });
                                });
                            ui.add_space(4.0);
                        }

                        if let Some(i) = move_up {
                            chain.swap(i, i - 1);
                            chain_modified = true;
                        } else if let Some(i) = move_down {
                            chain.swap(i, i + 1);
                            chain_modified = true;
                        } else if let Some(i) = remove_idx {
                            chain.remove(i);
                            chain_modified = true;
                        }
                    }

                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        // 1. Set Primary Font picker
                        let mut primary_pick: Option<String> = None;
                        egui::ComboBox::from_id_salt("set_primary_font_cb")
                            .selected_text("🎯 變更主字型")
                            .width(155.0)
                            .show_ui(ui, |ui| {
                                primary_pick = self.render_font_picker_popup(ui);
                            });
                        if let Some(f) = primary_pick {
                            chain.retain(|existing| !existing.eq_ignore_ascii_case(&f));
                            chain.insert(0, f);
                            chain_modified = true;
                        }

                        // 2. Append Fallback Font picker
                        let mut fallback_pick: Option<String> = None;
                        egui::ComboBox::from_id_salt("append_fallback_font_cb")
                            .selected_text("＋ 新增備援字型")
                            .width(155.0)
                            .show_ui(ui, |ui| {
                                fallback_pick = self.render_font_picker_popup(ui);
                            });
                        if let Some(f) = fallback_pick {
                            if !chain.iter().any(|existing| existing.eq_ignore_ascii_case(&f)) {
                                chain.push(f);
                                chain_modified = true;
                            }
                        }

                        // 3. OS-aware Font Templates
                        egui::ComboBox::from_id_salt("font_preset_cb")
                            .selected_text("✨ 套用組合範本")
                            .width(155.0)
                            .show_ui(ui, |ui| {
                                #[cfg(target_os = "linux")]
                                let presets = [
                                    (
                                        "NotoSansM Nerd + 思源黑體 (推薦 🚀)",
                                        "NotoSansMNerdFontMono, Noto Sans CJK TC, DejaVu Sans Mono",
                                    ),
                                    (
                                        "JetBrainsMono Nerd + 思源黑體",
                                        "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono",
                                    ),
                                    (
                                        "FiraCode Nerd + 思源黑體",
                                        "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono",
                                    ),
                                    (
                                        "Ubuntu / DejaVu (Linux 經典)",
                                        "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC",
                                    ),
                                ];
                                #[cfg(target_os = "macos")]
                                let presets = [
                                    (
                                        "NotoSansM Nerd Font (推薦 🚀)",
                                        "NotoSansMNerdFontMono, NotoSansTC, Menlo",
                                    ),
                                    (
                                        "JetBrainsMono Nerd Font",
                                        "JetBrainsMono Nerd Font, PingFang TC",
                                    ),
                                    ("FiraCode Nerd Font", "FiraCode Nerd Font, PingFang TC"),
                                    ("SF Mono + 蘋方", "SF Mono, PingFang TC"),
                                    ("Menlo (macOS 預設)", "Menlo, PingFang TC"),
                                ];
                                #[cfg(target_os = "windows")]
                                let presets = [
                                    (
                                        "Cascadia Mono + 微軟正黑體 (推薦 🚀)",
                                        "Cascadia Mono, Microsoft JhengHei UI, Consolas",
                                    ),
                                    (
                                        "JetBrainsMono Nerd Font",
                                        "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
                                    ),
                                    (
                                        "Consolas (Windows 預設)",
                                        "Consolas, Microsoft JhengHei UI",
                                    ),
                                ];

                                for (label, value) in presets {
                                    if ui.selectable_label(false, label).clicked() {
                                        self.font_family = value.to_string();
                                        self.font_family_input = value.to_string();
                                    }
                                }
                            });

                        if ui
                            .button(if self.show_manual_font_input {
                                "▲ 收合手動編輯"
                            } else {
                                "✎ 手動編輯字串"
                            })
                            .clicked()
                        {
                            self.show_manual_font_input = !self.show_manual_font_input;
                            if self.show_manual_font_input {
                                self.font_family_input = self.font_family.clone();
                            }
                        }

                        if !self.font_family.is_empty()
                            && ui
                                .button("清除")
                                .on_hover_text("清空自訂字型，回復系統預設")
                                .clicked()
                        {
                            chain.clear();
                            chain_modified = true;
                        }
                    });

                    if chain_modified {
                        self.font_family = chain.join(", ");
                        self.font_family_input = self.font_family.clone();
                    }

                    if self.show_manual_font_input {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.font_family_input)
                                    .desired_width(ui.available_width() - 68.0)
                                    .hint_text("以逗號分隔，例如：JetBrainsMono Nerd Font, Noto Sans CJK TC")
                                    .font(egui::TextStyle::Monospace),
                            );
                            let enter_pressed = resp.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter));
                            if ui.button("✔ 套用").clicked() || enter_pressed {
                                self.font_family = self.font_family_input.trim().to_string();
                            }
                        });
                    }
                },
            );

            b.row("字型大小 (Font Size)", Some("終端文字大小 (pt)"), |ui| {
                ui.add(
                    egui::Slider::new(&mut self.font_size, 9.0..=36.0)
                        .suffix(" pt")
                        .step_by(0.5),
                );
            });

            b.row(
                "行高倍率 (Line Height)",
                Some("1.0 為緊湊，1.2 為標準舒適（即時套用）"),
                |ui| {
                    ui.add(egui::Slider::new(&mut self.line_height, 1.0..=2.0).step_by(0.05));
                },
            );
        });

        render_inset_card(ui, "視窗幾何與邊距 (Window & Layout)", |b| {
            b.row(
                "預設終端大小 (Columns × Rows)",
                Some("啟動時初始網格欄列數 (寬 × 高)"),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.columns)
                                .range(40..=400)
                                .prefix("寬 (Cols): "),
                        );
                        ui.label("×");
                        ui.add(
                            egui::DragValue::new(&mut self.rows)
                                .range(10..=200)
                                .prefix("高 (Rows): "),
                        );
                    });
                },
            );

            b.row(
                "水平與垂直內距 (Padding X / Y)",
                Some("終端文字四周保留的留白像素"),
                |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Slider::new(&mut self.padding_x, 0.0..=40.0)
                                .suffix(" px")
                                .prefix("左右 X: "),
                        );
                        ui.add_space(6.0);
                        ui.add(
                            egui::Slider::new(&mut self.padding_y, 0.0..=40.0)
                                .suffix(" px")
                                .prefix("上下 Y: "),
                        );
                    });
                },
            );

            b.row(
                "背景不透明度 (Opacity)",
                Some("1.0 為完全不透明，調低可搭配桌面透明效果"),
                |ui| {
                    ui.add(egui::Slider::new(&mut self.opacity, 0.1..=1.0).step_by(0.05));
                },
            );

            b.row(
                "視窗邊框樣式 (Decorations)",
                Some("控制系統標題列與視窗外框顯示"),
                |ui| {
                    let mut cur = self.decorations.as_str();
                    let opts = [
                        ("full", "完整標題列"),
                        ("transparent", "透明融合"),
                        ("none", "無邊框"),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.decorations = cur.to_string();
                    }
                },
            );

            b.row(
                "啟動模式 (Startup Mode)",
                Some("應用程式啟動時的初始視窗狀態"),
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
                "分頁列顯示策略 (Tab Bar)",
                Some("隱藏分頁列時，滑鼠移至視窗右上角仍會浮現 [AI] 與 [⚙] 快捷膠囊"),
                |ui| {
                    let mut cur = self.tab_bar_style.as_str();
                    let opts = [
                        ("always", "恆常顯示"),
                        ("auto", "單頁自動隱藏"),
                        ("never", "永不顯示"),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.tab_bar_style = cur.to_string();
                    }
                },
            );

            let s = crate::i18n::tr();
            b.row(
                s.pref_language_label,
                Some(s.pref_language_subtitle),
                |ui| {
                    let mut cur = self.language.as_str();
                    let opts = [
                        ("auto", s.pref_language_auto),
                        ("zh-TW", s.pref_language_zh_tw),
                        ("en", s.pref_language_en),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.language = cur.to_string();
                    }
                },
            );
        });
    }

    // 2. Colors Tab View
    fn render_colors_view(&mut self, ui: &mut Ui) {
        self.render_terminal_live_preview(ui);

        render_inset_card(ui, "色彩主題 (Preset Themes)", |b| {
            b.block(
                "一鍵主題卡片畫廊 (Theme Gallery)",
                Some("點擊卡片立即套用主題底色與 ANSI 16 色調色盤（並重設自訂覆寫色）"),
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
                    let avail_w = ui.available_width();
                    let cols = 3usize;
                    let spacing_x = 12.0;
                    let card_w = ((avail_w - spacing_x * (cols as f32 - 1.0)) / cols as f32)
                        .max(150.0);

                    egui::Grid::new("theme_swatch_grid")
                        .num_columns(cols)
                        .spacing(egui::vec2(spacing_x, 10.0))
                        .show(ui, |ui| {
                            for (idx, (id, label)) in themes.iter().enumerate() {
                                ui.push_id(id, |ui| {
                                    let is_selected = self.theme == *id;
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(card_w, 60.0),
                                        Sense::click(),
                                    );
                                    let hover = resp.hovered();
                                    let preset = crate::config::Theme::get(id);
                                    let theme_bg = rgba_to_color32(preset.background);
                                    let theme_fg = rgba_to_color32(preset.foreground);

                                    let stroke = if is_selected {
                                        Stroke::new(2.0_f32, Color32::from_rgb(10, 132, 255))
                                    } else if hover {
                                        Stroke::new(1.2_f32, Color32::from_rgb(110, 120, 155))
                                    } else {
                                        Stroke::new(1.0_f32, Color32::from_rgb(52, 56, 74))
                                    };

                                    ui.painter().rect(
                                        rect,
                                        CornerRadius::same(8),
                                        theme_bg,
                                        stroke,
                                        egui::StrokeKind::Outside,
                                    );

                                    ui.painter().text(
                                        rect.left_top() + egui::vec2(10.0, 8.0),
                                        egui::Align2::LEFT_TOP,
                                        label,
                                        egui::FontId::proportional(12.0),
                                        theme_fg,
                                    );

                                    if is_selected {
                                        ui.painter().text(
                                            rect.right_top() + egui::vec2(-10.0, 8.0),
                                            egui::Align2::RIGHT_TOP,
                                            "✓",
                                            egui::FontId::proportional(13.0),
                                            Color32::from_rgb(10, 132, 255),
                                        );
                                    }

                                    let dot_colors = [
                                        preset.palette[1],
                                        preset.palette[2],
                                        preset.palette[3],
                                        preset.palette[4],
                                        preset.palette[5],
                                        preset.palette[6],
                                    ];
                                    let start_x = rect.left() + 14.0;
                                    let dot_y = rect.bottom() - 13.0;
                                    for (i, c_rgba) in dot_colors.iter().enumerate() {
                                        let c32 = rgba_to_color32(*c_rgba);
                                        let dot_x = start_x + (i as f32 * 16.0);
                                        ui.painter().circle_filled(
                                            egui::pos2(dot_x, dot_y),
                                            4.8,
                                            c32,
                                        );
                                        ui.painter().circle_stroke(
                                            egui::pos2(dot_x, dot_y),
                                            4.8,
                                            Stroke::new(0.7_f32, Color32::from_white_alpha(80)),
                                        );
                                    }

                                    if resp.clicked() {
                                        self.theme = id.to_string();
                                    }
                                });

                                if (idx + 1) % cols == 0 {
                                    ui.end_row();
                                }
                            }
                        });

                    if old_theme != self.theme {
                        self.fg_override.clear();
                        self.bg_override.clear();
                        self.cursor_override.clear();
                        self.selection_override.clear();
                        for slot in &mut self.palette_overrides {
                            slot.clear();
                        }
                    }
                },
            );
        });

        let preset = crate::config::Theme::get(&self.theme);
        render_inset_card(ui, "自訂背景與文字顏色 (Custom Overrides)", |b| {
            b.row(
                "背景底色 (Background)",
                Some("留空則繼承主題預設背景色"),
                |ui| {
                    render_color_row_with_presets(
                        ui,
                        rgba_to_color32(preset.background),
                        &mut self.bg_override,
                        &[
                            ("#000000", Color32::from_rgb(0, 0, 0), "OLED 純黑"),
                            ("#0a0a0e", Color32::from_rgb(10, 10, 14), "深墨黑"),
                            ("#18181f", Color32::from_rgb(24, 24, 31), "藏青黑"),
                        ],
                    );
                },
            );
            b.row(
                "前景文字色 (Foreground)",
                Some("留空則繼承主題預設文字色"),
                |ui| {
                    render_color_row_with_presets(
                        ui,
                        rgba_to_color32(preset.foreground),
                        &mut self.fg_override,
                        &[
                            ("#f5f5f7", Color32::from_rgb(245, 245, 247), "亮白"),
                            ("#30d158", Color32::from_rgb(48, 209, 88), "駭客綠"),
                            ("#ff9f0a", Color32::from_rgb(255, 159, 10), "琥珀黃"),
                        ],
                    );
                },
            );
            b.row("游標顏色 (Cursor)", Some("留空則繼承主題預設游標色"), |ui| {
                render_color_row_with_presets(
                    ui,
                    rgba_to_color32(preset.cursor),
                    &mut self.cursor_override,
                    &[
                        ("#0a84ff", Color32::from_rgb(10, 132, 255), "macOS 藍"),
                        ("#30d158", Color32::from_rgb(48, 209, 88), "翠綠"),
                        ("#ff453a", Color32::from_rgb(255, 69, 58), "鮮紅"),
                    ],
                );
            });
            b.row(
                "選取文字底色 (Selection)",
                Some("滑鼠反白選取範圍的背景色"),
                |ui| {
                    render_color_row_with_presets(
                        ui,
                        rgba_to_color32(preset.selection),
                        &mut self.selection_override,
                        &[
                            ("#2a4b7c", Color32::from_rgb(42, 75, 124), "經典藍選取"),
                            ("#383844", Color32::from_rgb(56, 56, 68), "柔和灰選取"),
                        ],
                    );
                },
            );
        });

        render_inset_card(ui, "進階 ANSI 16 色調色盤 (ANSI 16-Color Palette)", |b| {
            b.block(
                "個別覆寫終端機 ANSI 16 色 (color0 ~ color15)",
                Some("可微調標準 8 色與高亮 8 色；留空代表使用當前主題預設值。"),
                |ui| {
                    ui.horizontal(|ui| {
                        let btn_label = if self.show_ansi_palette {
                            "▼ 收合 ANSI 16 色設定表"
                        } else {
                            "▶ 展開 ANSI 16 色設定表 (0~15)"
                        };
                        if ui.button(btn_label).clicked() {
                            self.show_ansi_palette = !self.show_ansi_palette;
                        }

                        let any_override = self
                            .palette_overrides
                            .iter()
                            .any(|s| !s.trim().is_empty());
                        if any_override
                            && ui
                                .button("🔄 全部重設為主題預設")
                                .on_hover_text("清除所有 color0 ~ color15 自訂覆寫")
                                .clicked()
                        {
                            for slot in &mut self.palette_overrides {
                                slot.clear();
                            }
                        }
                    });

                    if self.show_ansi_palette {
                        ui.add_space(8.0);
                        let color_names = [
                            "0 黑 (Black)",
                            "1 紅 (Red)",
                            "2 綠 (Green)",
                            "3 黃 (Yellow)",
                            "4 藍 (Blue)",
                            "5 洋紅 (Magenta)",
                            "6 青 (Cyan)",
                            "7 白 (White)",
                            "8 亮黑 (BrBlack)",
                            "9 亮紅 (BrRed)",
                            "10 亮綠 (BrGreen)",
                            "11 亮黃 (BrYellow)",
                            "12 亮藍 (BrBlue)",
                            "13 亮洋紅 (BrMag)",
                            "14 亮青 (BrCyan)",
                            "15 亮白 (BrWhite)",
                        ];

                        egui::Grid::new("ansi_16_palette_grid")
                            .num_columns(2)
                            .spacing(egui::vec2(20.0, 8.0))
                            .show(ui, |ui| {
                                for (idx, color_name) in color_names.iter().enumerate() {
                                    ui.push_id(idx, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{:<16}", color_name))
                                                    .size(11.5)
                                                    .monospace()
                                                    .color(Color32::from_rgb(200, 206, 222)),
                                            );
                                            let fallback = rgba_to_color32(preset.palette[idx]);
                                            let mut cur = parse_hex_color(&self.palette_overrides[idx])
                                                .unwrap_or(fallback);
                                            if ui.color_edit_button_srgba(&mut cur).changed() {
                                                self.palette_overrides[idx] = format!(
                                                    "#{:02x}{:02x}{:02x}",
                                                    cur.r(),
                                                    cur.g(),
                                                    cur.b()
                                                );
                                            }
                                            ui.add(
                                                egui::TextEdit::singleline(
                                                    &mut self.palette_overrides[idx],
                                                )
                                                .hint_text(format!(
                                                    "#{:02x}{:02x}{:02x}",
                                                    fallback.r(),
                                                    fallback.g(),
                                                    fallback.b()
                                                ))
                                                .desired_width(74.0)
                                                .font(egui::TextStyle::Monospace),
                                            );
                                            if !self.palette_overrides[idx].trim().is_empty()
                                                && ui
                                                    .small_button("✖")
                                                    .on_hover_text("重設為主題預設")
                                                    .clicked()
                                            {
                                                self.palette_overrides[idx].clear();
                                            }
                                        });
                                    });
                                    if idx % 2 == 1 {
                                        ui.end_row();
                                    }
                                }
                            });
                    }
                },
            );
        });
    }

    // 3. Terminal & Shell Tab View
    fn render_terminal_view(&mut self, ui: &mut Ui) {
        render_inset_card(ui, "Shell 與啟動環境 (Shell & Environment)", |b| {
            b.row(
                "TERM 模擬器代碼",
                Some("環境變數 $TERM 預設值"),
                |ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.term).desired_width(150.0));
                },
            );

            let presets = skterm_core::available_shell_presets();
            let current_label =
                if !self.custom_shell_enabled || self.shell_program.trim().is_empty() {
                    #[cfg(not(target_os = "windows"))]
                    {
                        let env_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                        format!("系統預設 ({})", env_shell)
                    }
                    #[cfg(target_os = "windows")]
                    {
                        "系統預設 (System Default)".to_string()
                    }
                } else {
                    let prog_lower = self.shell_program.to_lowercase();
                    if let Some(p) = presets.iter().find(|p| {
                        prog_lower == p.program.to_lowercase()
                            || prog_lower == p.id
                            || prog_lower.ends_with(&format!("{}.exe", p.id))
                            || prog_lower.ends_with(&format!("\\{}", p.id))
                    }) {
                        p.name.clone()
                    } else {
                        format!("自訂 ({})", self.shell_program)
                    }
                };

            #[cfg(target_os = "windows")]
            let shell_subtitle = "自由切換命令提示字元、Windows PowerShell 或 PowerShell 7";
            #[cfg(not(target_os = "windows"))]
            let shell_subtitle = "自動偵測系統已安裝的 Shell（如 Bash、Zsh、Fish 等）並去除重複連結";

            b.row("預設 Shell 核心", Some(shell_subtitle), |ui| {
                egui::ComboBox::from_id_salt("pref_window_shell_preset")
                    .selected_text(&current_label)
                    .width(230.0)
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_label(
                                !self.custom_shell_enabled,
                                "系統預設 ($SHELL / System Default)",
                            )
                            .clicked()
                        {
                            self.custom_shell_enabled = false;
                            self.shell_program.clear();
                            self.shell_args.clear();
                        }
                        for p in &presets {
                            let is_sel = self.custom_shell_enabled
                                && (self.shell_program.eq_ignore_ascii_case(&p.program)
                                    || self.shell_program.eq_ignore_ascii_case(&p.id)
                                    || self
                                        .shell_program
                                        .to_lowercase()
                                        .ends_with(&format!("{}.exe", p.id)));
                            let label_text = if p.is_available {
                                p.name.clone()
                            } else {
                                format!("{} (未安裝)", p.name)
                            };
                            if ui.selectable_label(is_sel, label_text).clicked() {
                                self.custom_shell_enabled = true;
                                self.shell_program = p.program.clone();
                                self.shell_args = p.args.join(" ");
                            }
                        }
                        let is_custom = self.custom_shell_enabled
                            && !presets.iter().any(|p| {
                                self.shell_program.eq_ignore_ascii_case(&p.program)
                                    || self.shell_program.eq_ignore_ascii_case(&p.id)
                                    || self
                                        .shell_program
                                        .to_lowercase()
                                        .ends_with(&format!("{}.exe", p.id))
                            });
                        if ui
                            .selectable_label(is_custom, "自訂 Shell 路徑 (Custom...)")
                            .clicked()
                        {
                            self.custom_shell_enabled = true;
                        }
                    });
            });

            b.row(
                "快速切換常用 Shell",
                Some("點擊按鈕一鍵切換新分頁預設啟動的 Shell"),
                |ui| {
                    ui.horizontal_wrapped(|ui| {
                        let is_sys_default =
                            !self.custom_shell_enabled || self.shell_program.trim().is_empty();
                        if ui
                            .selectable_label(is_sys_default, "🌟 系統預設")
                            .clicked()
                        {
                            self.custom_shell_enabled = false;
                            self.shell_program.clear();
                            self.shell_args.clear();
                        }
                        for p in presets.iter().filter(|p| p.is_available) {
                            let is_active = self.custom_shell_enabled
                                && self.shell_program.eq_ignore_ascii_case(&p.program);
                            let short_label = match p.id.as_str() {
                                "bash" => "🐚 Bash".to_string(),
                                "zsh" => "⚡ Zsh".to_string(),
                                "fish" => "🐟 Fish".to_string(),
                                "sh" => "📟 POSIX sh".to_string(),
                                "pwsh" => "🚀 PowerShell 7".to_string(),
                                "powershell" => "⚡ Win PowerShell".to_string(),
                                "cmd" => "💻 CMD".to_string(),
                                _ => p.name.clone(),
                            };
                            if ui
                                .selectable_label(is_active, short_label)
                                .on_hover_text(&p.program)
                                .clicked()
                            {
                                self.custom_shell_enabled = true;
                                self.shell_program = p.program.clone();
                                self.shell_args = p.args.join(" ");
                            }
                        }
                    });
                },
            );

            b.row(
                "手動指定執行檔路徑与參數",
                Some("需要傳遞特殊啟動 flag 時開啟"),
                |ui| {
                    render_macos_toggle_switch(ui, &mut self.custom_shell_enabled);
                },
            );

            if self.custom_shell_enabled {
                #[cfg(target_os = "windows")]
                let (prog_hint, args_hint) = ("pwsh.exe / powershell.exe / cmd.exe", "-NoLogo");
                #[cfg(not(target_os = "windows"))]
                let (prog_hint, args_hint) = ("/bin/zsh 或 /usr/bin/fish", "-l");

                b.row("執行檔路徑 (Program)", None, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shell_program)
                            .hint_text(prog_hint)
                            .desired_width(240.0)
                            .font(egui::TextStyle::Monospace),
                    );
                });

                b.row("啟動參數 (Arguments)", None, |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.shell_args)
                            .hint_text(args_hint)
                            .desired_width(240.0)
                            .font(egui::TextStyle::Monospace),
                    );
                });
            }

            b.row(
                "初始工作目錄 (Working Directory)",
                Some("終端啟動時的預設路徑（留空則為家目錄 ~/）"),
                |ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.working_directory)
                            .hint_text("預設使用者家目錄 (~/)")
                            .desired_width(240.0),
                    );
                },
            );
        });

        render_inset_card(ui, "緩衝區與操作行為 (Buffer & Interaction)", |b| {
            b.row(
                "滑鼠右鍵行為 (Right-Click Action)",
                Some("按住 Shift + 右鍵 可隨時執行另一種行為"),
                |ui| {
                    let mut cur = self.right_click_action.as_str();
                    let opts = [
                        ("menu", "開啟快捷選單 (預設)"),
                        ("paste", "直接貼上剪貼簿"),
                    ];
                    if render_macos_segmented_control(ui, &mut cur, &opts) {
                        self.right_click_action = cur.to_string();
                    }
                },
            );

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
                "Alt / Option 鍵 Meta 模式",
                Some("控制 Alt / Option 鍵是否發送終端機 Meta (Esc+) 序列"),
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
                Some("收到 BEL 警示訊號時的回饋方式"),
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
        self.render_terminal_live_preview(ui);

        render_inset_card(ui, "游標樣式與動態 (Cursor Style)", |b| {
            b.row(
                "游標形狀 (Shape)",
                Some("終端游標幾何外觀（上方預覽盒可即時檢視）"),
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
        // Filter & Platform Selector Bar
        Frame::new()
            .fill(Color32::from_rgb(34, 36, 48))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(52, 55, 72)))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.keybinding_filter)
                            .hint_text("🔍 搜尋快捷鍵或功能名稱 (例如：分割、搜尋、AI、偏好設定)...")
                            .desired_width(280.0),
                    );
                    if !self.keybinding_filter.is_empty() && ui.small_button("✖").clicked() {
                        self.keybinding_filter.clear();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let opts = [
                            (0u8, "當前系統"),
                            (3u8, "雙平台對照"),
                            (1u8, "macOS"),
                            (2u8, "Linux / Win"),
                        ];
                        render_macos_segmented_control(ui, &mut self.keybinding_os_mode, &opts);
                    });
                });
            });

        ui.add_space(12.0);

        type ShortcutGroup<'a> = (&'a str, &'a [(&'a str, Option<&'a str>, &'a [&'a str], &'a [&'a str])]);

        let groups: &[ShortcutGroup] = &[
            (
                "視窗與分頁管理 (Window & Tabs)",
                &[
                    (
                        "開啟偏好設定視窗",
                        Some("亦可點擊分頁列右上角 ⚙ 按鈕或終端機右鍵選單開啟"),
                        &["⌘", ","],
                        &["Ctrl", ","],
                    ),
                    (
                        "新增終端機視窗 (New Window)",
                        None,
                        &["⌘", "N"],
                        &["Ctrl", "Shift", "N"],
                    ),
                    (
                        "新增分頁 (New Tab)",
                        Some("亦可點擊分頁列 ＋ 按鈕"),
                        &["⌘", "T"],
                        &["Ctrl", "Shift", "T"],
                    ),
                    (
                        "關閉當前窗格或分頁",
                        None,
                        &["⌘", "W"],
                        &["Ctrl", "Shift", "W"],
                    ),
                    (
                        "切換至下一個 / 上一個分頁",
                        None,
                        &["⌘", "⇧", "] / ["],
                        &["Ctrl", "Tab / Shift+Tab"],
                    ),
                    (
                        "快速切換指定第 1~9 分頁",
                        None,
                        &["⌘", "1 ~ 9"],
                        &["Ctrl", "1 ~ 9"],
                    ),
                ],
            ),
            (
                "分割窗格與導覽 (Split Panes & Navigation)",
                &[
                    (
                        "左右垂直切分窗格 (Split Vertical)",
                        None,
                        &["⌘", "D"],
                        &["Ctrl", "Shift", "D"],
                    ),
                    (
                        "上下水平切分窗格 (Split Horizontal)",
                        None,
                        &["⌘", "⇧", "D"],
                        &["Ctrl", "Shift", "H"],
                    ),
                    (
                        "切換焦點至相鄰窗格",
                        Some("Linux/Win 使用 Alt+[ 與 Alt+]，避免攔截终端機 Esc (Ctrl+[)"),
                        &["⌘", "[ / ]"],
                        &["Alt", "[ / ]"],
                    ),
                    (
                        "最大化 / 還原當前分割窗格 (Zoom Pane)",
                        None,
                        &["⌘", "⇧", "Z"],
                        &["Ctrl", "Shift", "Z"],
                    ),
                    (
                        "微調分割窗格邊界比例",
                        None,
                        &["⌘", "⌃", "方向鍵"],
                        &["Ctrl", "Shift", "方向鍵"],
                    ),
                ],
            ),
            (
                "編輯、搜尋與 AI 助手 (Edit, Search & AI)",
                &[
                    (
                        "複製選取文字 (Copy)",
                        None,
                        &["⌘", "C"],
                        &["Ctrl", "Shift", "C"],
                    ),
                    (
                        "貼上剪貼簿文字 (Paste)",
                        None,
                        &["⌘", "V"],
                        &["Ctrl", "Shift", "V"],
                    ),
                    (
                        "開啟畫面文字搜尋列 (Find)",
                        None,
                        &["⌘", "F"],
                        &["Ctrl", "Shift", "F"],
                    ),
                    (
                        "切換 AI Agents 側邊欄",
                        Some("亦可點擊右上角 AI 膠囊按鈕"),
                        &["⌘", "⇧", "A"],
                        &["Ctrl", "Shift", "A"],
                    ),
                    (
                        "清除畫面與捲動緩衝區 (Clear Scrollback)",
                        None,
                        &["⌘", "K"],
                        &["Ctrl", "Shift", "K"],
                    ),
                    (
                        "開啟終端機右鍵選單 / 快速貼上",
                        Some("可在「終端與 Shell」分頁自訂預設右鍵行為"),
                        &["滑鼠右鍵"],
                        &["滑鼠右鍵", "Shift+右鍵"],
                    ),
                ],
            ),
            (
                "字型縮放與捲動控制 (Zoom, Scroll & Signals)",
                &[
                    ("字型放大 (Zoom In)", None, &["⌘", "="], &["Ctrl", "="]),
                    ("字型縮小 (Zoom Out)", None, &["⌘", "-"], &["Ctrl", "-"]),
                    (
                        "重設預設字型大小 (Reset Zoom)",
                        None,
                        &["⌘", "0"],
                        &["Ctrl", "0"],
                    ),
                    (
                        "向上 / 向下翻頁捲動歷史",
                        None,
                        &["⇧", "PageUp / PageDown"],
                        &["Shift", "PageUp / PageDown"],
                    ),
                    (
                        "中斷目前執行行程 (SIGINT)",
                        None,
                        &["⌃", "C"],
                        &["Ctrl", "C"],
                    ),
                    (
                        "傳送 EOF / 登出 Shell",
                        None,
                        &["⌃", "D"],
                        &["Ctrl", "D"],
                    ),
                ],
            ),
        ];

        let needle = self.keybinding_filter.trim().to_lowercase();
        let show_mac = match self.keybinding_os_mode {
            0 => cfg!(target_os = "macos"),
            1 => true,
            2 => false,
            _ => true,
        };
        let show_linux_win = match self.keybinding_os_mode {
            0 => !cfg!(target_os = "macos"),
            1 => false,
            2 => true,
            _ => true,
        };

        for (group_title, items) in groups {
            let filtered: Vec<_> = items
                .iter()
                .filter(|(desc, sub, mac_k, win_k)| {
                    if needle.is_empty() {
                        return true;
                    }
                    desc.to_lowercase().contains(&needle)
                        || sub.map(|s| s.to_lowercase().contains(&needle)).unwrap_or(false)
                        || mac_k.iter().any(|k| k.to_lowercase().contains(&needle))
                        || win_k.iter().any(|k| k.to_lowercase().contains(&needle))
                })
                .collect();

            if filtered.is_empty() {
                continue;
            }

            render_inset_card(ui, group_title, |b| {
                for (desc, sub, mac_keys, win_keys) in filtered {
                    b.row(desc, *sub, |ui| {
                        ui.horizontal(|ui| {
                            if show_mac {
                                if show_linux_win {
                                    ui.label(
                                        RichText::new("macOS:")
                                            .color(Color32::from_rgb(135, 142, 160))
                                            .size(11.0),
                                    );
                                }
                                for k in *mac_keys {
                                    render_macos_keyboard_badge(ui, k);
                                }
                            }
                            if show_mac && show_linux_win {
                                ui.add_space(8.0);
                            }
                            if show_linux_win {
                                if show_mac {
                                    ui.label(
                                        RichText::new("Linux/Win:")
                                            .color(Color32::from_rgb(110, 115, 130))
                                            .size(10.5),
                                    );
                                }
                                for k in *win_keys {
                                    render_macos_keyboard_badge(ui, k);
                                }
                            }
                        });
                    });
                }
            });
        }
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
