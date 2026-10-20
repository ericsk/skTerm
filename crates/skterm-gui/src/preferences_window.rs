//! Dedicated Native Preferences Window adhering to macOS Human Interface Guidelines (HIG).
//! Features a left category sidebar, rounded grouped inset cards, native toggle switches,
//! auto-discovered system monospace fonts, color picker, and real-time live preview.

use crate::config::{Config, ConfigManager, ShellConfig};
use crate::i18n::{current_locale, Locale};
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

#[inline]
fn l<'a>(zh_tw: &'a str, zh_cn: &'a str, en: &'a str) -> &'a str {
    let locale = current_locale();
    match locale {
        Locale::ZhTw => zh_tw,
        Locale::ZhCn => zh_cn,
        Locale::En => en,
        Locale::Ja | Locale::Ko | Locale::Es | Locale::Fr | Locale::De => {
            let (ja, ko, es, fr, de) = translate_extra(zh_tw, en);
            match locale {
                Locale::Ja => ja,
                Locale::Ko => ko,
                Locale::Es => es,
                Locale::Fr => fr,
                Locale::De => de,
                _ => en,
            }
        }
    }
}

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
            if !hex_val.is_empty()
                && ui
                    .button("✖")
                    .on_hover_text(l("重設為預設值", "重置为默认值", "Reset to default"))
                    .clicked()
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
                    .hint_text(l("預設 (主題繼承)", "默认 (主题继承)", "Default (Theme)"))
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
            .on_hover_text(l(
                "點擊開啟調色盤",
                "点击打开调色板",
                "Click to open color picker",
            ));
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
                .hint_text(l("預設 (主題繼承)", "默认 (主题继承)", "Default (Theme)"))
                .desired_width(96.0)
                .font(egui::TextStyle::Monospace),
        );

        if !hex_val.trim().is_empty()
            && ui
                .button("✖")
                .on_hover_text(l(
                    "清除自訂值，重設為主題預設顏色",
                    "清除自定义值，重置为主题默认颜色",
                    "Clear custom override and reset to theme default",
                ))
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
    pub auto_check_updates: bool,

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
            .with_title(l(
                "skTerm 偏好設定 (Preferences)",
                "skTerm 偏好设置 (Preferences)",
                "skTerm Settings (Preferences)",
            ))
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

        let instance = Instance::new(InstanceDescriptor::new_without_display_handle());
        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await?;

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("skterm_pref_device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                memory_hints: Default::default(),
                ..Default::default()
            })
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
            color_space: wgpu::SurfaceColorSpace::Auto,
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

        let egui_renderer = egui_wgpu::Renderer::new(
            &device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

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
            auto_check_updates: true,

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
        self.auto_check_updates = config.window.auto_check_updates;

        self.theme = config.colors.theme.clone();
        self.fg_override = config.colors.foreground.clone().unwrap_or_default();
        self.bg_override = config.colors.background.clone().unwrap_or_default();
        self.cursor_override = config.colors.cursor.clone().unwrap_or_default();
        self.selection_override = config.colors.selection.clone().unwrap_or_default();
        if let Some(pal) = &config.colors.palette {
            for idx in 0..16 {
                self.palette_overrides[idx] = pal
                    .get_index(idx)
                    .map(|s| s.to_string())
                    .unwrap_or_default();
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
        cfg.window.auto_check_updates = self.auto_check_updates;

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
        let expected_title = l(
            "skTerm 偏好設定 (Preferences)",
            "skTerm 偏好设置 (Preferences)",
            "skTerm Settings (Preferences)",
        );
        if self.window.title() != expected_title {
            self.window.set_title(expected_title);
        }

        let raw_input = self.egui_state.take_egui_input(&self.window);

        let egui_ctx = self.egui_ctx.clone();
        let full_output = egui_ctx.run_ui(raw_input, |ui| {
            // Support closing the Preferences window via Esc (when no text box has focus) or Ctrl/Cmd+W
            let wants_kb = ui.ctx().egui_wants_keyboard_input();
            let close_shortcut = ui.ctx().input(|i| {
                let esc = i.key_pressed(egui::Key::Escape) && !wants_kb;
                let cmd_w =
                    i.key_pressed(egui::Key::W) && (i.modifiers.command || i.modifiers.ctrl);
                esc || cmd_w
            });
            if close_shortcut {
                self.should_close = true;
            }

            // macOS Sonoma Sidebar Navigation (with embedded bottom utility actions)
            egui::Panel::left("pref_sidebar")
                .exact_size(195.0)
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(22, 23, 30))
                        .inner_margin(Margin::symmetric(14, 18)),
                )
                .show(ui, |ui| {
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
                                RichText::new(l(
                                    "skTerm 偏好設定",
                                    "skTerm 偏好设置",
                                    "skTerm Settings",
                                ))
                                .size(15.0)
                                .strong()
                                .color(Color32::WHITE),
                            );
                        });

                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(l(
                                "即時熱重載同步中 (Live Sync)",
                                "实时热重载同步中 (Live Sync)",
                                "Live Hot-Reload Sync Active",
                            ))
                            .size(11.0)
                            .color(Color32::from_rgb(120, 130, 155)),
                        );

                        ui.add_space(16.0);

                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Appearance,
                            l("🔤 外觀與字型", "🔤 外观与字体", "🔤 Appearance"),
                            "Appearance",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Colors,
                            l("🎨 色彩與主題", "🎨 色彩与主题", "🎨 Colors"),
                            "Colors",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Terminal,
                            l("🖥️ 終端與 Shell", "🖥️ 终端与 Shell", "🖥️ Terminal"),
                            "Terminal",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Cursor,
                            l("🖱️ 游標與動態", "🖱️ 光标与动画", "🖱️ Cursor"),
                            "Cursor",
                        );
                        self.render_sidebar_item(
                            ui,
                            PreferencesCategory::Keybindings,
                            l("⌨️ 快捷鍵指南", "⌨️ 快捷键指南", "⌨️ Keybindings"),
                            "Keybindings",
                        );
                    });

                    // Bottom docked actions in Sidebar
                    ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
                        ui.add_space(2.0);
                        if ui
                            .button(
                                RichText::new(l(
                                    "✕ 關閉視窗 (Esc)",
                                    "✕ 关闭窗口 (Esc)",
                                    "✕ Close Window (Esc)",
                                ))
                                .size(12.0)
                                .color(Color32::from_rgb(210, 155, 160)),
                            )
                            .on_hover_text(l(
                                "關閉偏好設定視窗 (快速鍵: Esc 或 Ctrl/⌘+W)",
                                "关闭偏好设置窗口 (快捷键: Esc 或 Ctrl/⌘+W)",
                                "Close preferences window (Esc or Ctrl/⌘+W)",
                            ))
                            .clicked()
                        {
                            self.should_close = true;
                        }
                        ui.add_space(4.0);
                        if ui
                            .button(
                                RichText::new(l(
                                    "📝 編輯 config.toml",
                                    "📝 编辑 config.toml",
                                    "📝 Edit config.toml",
                                ))
                                .size(12.0)
                                .color(Color32::from_rgb(180, 195, 225)),
                            )
                            .on_hover_text(l(
                                "在系統文字編輯器開啟 TOML 設定檔",
                                "在系统文本编辑器中打开 TOML 配置文件",
                                "Open TOML config file in default text editor",
                            ))
                            .clicked()
                        {
                            config_manager.open_in_editor();
                        }
                        ui.add_space(4.0);
                        if ui
                            .button(
                                RichText::new(l(
                                    "🔄 重設為預設值",
                                    "🔄 重置为默认值",
                                    "🔄 Reset to Defaults",
                                ))
                                .size(12.0)
                                .color(Color32::from_rgb(160, 165, 185)),
                            )
                            .on_hover_text(l(
                                "還原所有偏好設定至出廠預設值",
                                "还原所有偏好设置至出厂默认值",
                                "Restore all settings to factory defaults",
                            ))
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
                        ui.add_space(6.0);
                        self.render_sidebar_ota_card(ui);
                    });
                });

            // Right Main Content Panel (Grouped Inset Cards)
            egui::CentralPanel::default()
                .frame(
                    Frame::new()
                        .fill(Color32::from_rgb(28, 29, 38))
                        .inner_margin(Margin::symmetric(26, 22)),
                )
                .show(ui, |ui| {
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
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            other => {
                info!("Surface status in preferences window: {:?}", other);
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

        for (&id, deltas) in &full_output.textures_delta.set {
            for delta in deltas {
                self.egui_renderer
                    .update_texture(&self.device, &self.queue, id, delta);
            }
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
                        depth_slice: None,
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
                    multiview_mask: None,
                })
                .forget_lifetime();

            self.egui_renderer
                .render(&mut render_pass, &clipped_primitives, &screen_descriptor);
        }

        for id in &full_output.textures_delta.free {
            self.egui_renderer.free_texture(id);
        }

        self.queue.submit(Some(encoder.finish()));
        self.queue.present(output);

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

    fn render_sidebar_ota_card(&self, ui: &mut Ui) {
        use crate::updater::{
            current_update_status, restart_application, trigger_check_for_updates,
            trigger_download_and_install, UpdateStatus, CURRENT_VERSION, GITHUB_RELEASES_PAGE,
        };

        let status = current_update_status();
        Frame::new()
            .fill(Color32::from_rgb(28, 31, 42))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(46, 50, 68)))
            .corner_radius(CornerRadius::same(7))
            .inner_margin(Margin::symmetric(8, 6))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("skTerm v{}", CURRENT_VERSION))
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(190, 198, 220)),
                        );
                    });
                    ui.add_space(3.0);
                    match status {
                        UpdateStatus::Idle => {
                            if ui
                                .small_button(l("🔄 檢查更新", "🔄 检查更新", "🔄 Check Updates"))
                                .clicked()
                            {
                                trigger_check_for_updates(true);
                            }
                        }
                        UpdateStatus::Checking => {
                            ui.label(
                                RichText::new(l(
                                    "⏳ 正在檢查更新...",
                                    "⏳ 正在检查更新...",
                                    "⏳ Checking for updates...",
                                ))
                                .size(10.5)
                                .color(Color32::from_rgb(140, 180, 250)),
                            );
                        }
                        UpdateStatus::UpToDate { .. } => {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(l(
                                        "✓ 已是最新版本",
                                        "✓ 已是最新版本",
                                        "✓ Up to date",
                                    ))
                                    .size(10.5)
                                    .color(Color32::from_rgb(80, 205, 120)),
                                );
                                if ui
                                    .small_button("↻")
                                    .on_hover_text(l("重新檢查", "重新检查", "Check again"))
                                    .clicked()
                                {
                                    trigger_check_for_updates(true);
                                }
                            });
                        }
                        UpdateStatus::Available(info) => {
                            ui.label(
                                RichText::new(format!(
                                    "✨ {} {}",
                                    l("發現新版", "发现新版", "New:"),
                                    info.tag_name
                                ))
                                .size(11.0)
                                .strong()
                                .color(Color32::from_rgb(255, 205, 80)),
                            );
                            ui.add_space(2.0);
                            if ui
                                .small_button(l(
                                    "⬇ 下載並安裝更新",
                                    "⬇ 下载并安装更新",
                                    "⬇ Install Update",
                                ))
                                .clicked()
                            {
                                trigger_download_and_install(info);
                            }
                        }
                        UpdateStatus::Downloading { stage, .. } => {
                            ui.label(
                                RichText::new(format!(
                                    "⬇ {}: {}",
                                    l("下載中", "下载中", "Downloading"),
                                    stage
                                ))
                                .size(10.5)
                                .color(Color32::from_rgb(120, 185, 255)),
                            );
                        }
                        UpdateStatus::ReadyToRestart {
                            version,
                            target_path,
                        } => {
                            ui.label(
                                RichText::new(format!(
                                    "✅ v{} {}",
                                    version,
                                    l("已安裝就緒", "已安装就绪", "Ready")
                                ))
                                .size(10.5)
                                .strong()
                                .color(Color32::from_rgb(80, 215, 130)),
                            );
                            if ui
                                .small_button(l(
                                    "🚀 立即重啟套用",
                                    "🚀 立即重启应用",
                                    "🚀 Restart Now",
                                ))
                                .clicked()
                            {
                                restart_application(&target_path);
                            }
                        }
                        UpdateStatus::Failed(err) => {
                            ui.label(
                                RichText::new(l(
                                    "⚠ 檢查或更新失敗",
                                    "⚠ 检查或更新失败",
                                    "⚠ Update check failed",
                                ))
                                .size(10.5)
                                .color(Color32::from_rgb(235, 115, 110)),
                            )
                            .on_hover_text(&err);
                            ui.horizontal(|ui| {
                                if ui
                                    .small_button(l("重試", "重试", "Retry"))
                                    .on_hover_text(&err)
                                    .clicked()
                                {
                                    trigger_check_for_updates(true);
                                }
                                if ui
                                    .small_button("GitHub")
                                    .on_hover_text(GITHUB_RELEASES_PAGE)
                                    .clicked()
                                {
                                    let _ = crate::app::open_url_in_browser(GITHUB_RELEASES_PAGE);
                                }
                            });
                        }
                    }
                });
            });
    }

    /// Resolve effective theme + custom overrides for live preview inside Preferences.
    fn effective_colors(&self) -> (Color32, Color32, Color32, Color32, [Color32; 16]) {
        let preset = crate::config::Theme::get(&self.theme);
        let bg = parse_hex_color(&self.bg_override)
            .unwrap_or_else(|| rgba_to_color32(preset.background));
        let fg = parse_hex_color(&self.fg_override)
            .unwrap_or_else(|| rgba_to_color32(preset.foreground));
        let cur = parse_hex_color(&self.cursor_override)
            .unwrap_or_else(|| rgba_to_color32(preset.cursor));
        let sel = parse_hex_color(&self.selection_override)
            .unwrap_or_else(|| rgba_to_color32(preset.selection));
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
                        l(
                            "系統預設等寬字型",
                            "系统默认等宽字体",
                            "System Default Monospace",
                        )
                        .to_string()
                    } else {
                        self.font_family
                            .split(',')
                            .next()
                            .unwrap_or(l("預設", "默认", "Default"))
                            .trim()
                            .to_string()
                    };
                    let preview_header = match crate::i18n::current_locale() {
                        crate::i18n::Locale::ZhTw => format!(
                            "即時預覽 — 主題: {} ｜ 字型: {} ({:.1}pt, 行高 {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::ZhCn => format!(
                            "实时预览 — 主题: {} ｜ 字体: {} ({:.1}pt, 行高 {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::En => format!(
                            "Live Preview — Theme: {} ｜ Font: {} ({:.1}pt, Line Height {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::Ja => format!(
                            "ライブプレビュー — テーマ: {} ｜ フォント: {} ({:.1}pt, 行の高さ {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::Ko => format!(
                            "실시간 미리보기 — 테마: {} ｜ 글꼴: {} ({:.1}pt, 줄 높이 {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::Es => format!(
                            "Vista previa — Tema: {} ｜ Fuente: {} ({:.1}pt, Interlineado {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::Fr => format!(
                            "Aperçu en direct — Thème : {} ｜ Police : {} ({:.1}pt, Interligne {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                        crate::i18n::Locale::De => format!(
                            "Live-Vorschau — Thema: {} ｜ Schrift: {} ({:.1}pt, Zeilenhöhe {:.2}x)",
                            self.theme, font_summary, self.font_size, self.line_height
                        ),
                    };
                    ui.label(
                        RichText::new(preview_header)
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
                                RichText::new(l(
                                    "skterm-gui v0.1.0 (選取反白預覽)",
                                    "skterm-gui v0.1.0 (选中高亮预览)",
                                    "skterm-gui v0.1.0 (Selection Preview)",
                                ))
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
                    ui.label(RichText::new("ERR: 0").monospace().size(12.0).color(pal[1]));
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
                .hint_text(l(
                    "🔍 搜尋字型名稱...",
                    "🔍 搜索字体名称...",
                    "🔍 Search font family...",
                ))
                .desired_width(250.0),
        );
        ui.add_space(4.0);
        let mut only_recommended = !self.show_all_fonts;
        if ui
            .checkbox(
                &mut only_recommended,
                l(
                    "僅顯示終端機推薦字型 (等寬 / Nerd / CJK)",
                    "仅显示终端推荐字体 (等宽 / Nerd / CJK)",
                    "Show terminal-friendly fonts only (Monospace / Nerd / CJK)",
                ),
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
                let type_match = self.show_all_fonts || skterm_render::is_terminal_friendly_font(f);
                name_match && type_match
            })
            .collect();

        let count_text = match crate::i18n::current_locale() {
            crate::i18n::Locale::ZhTw => format!(
                "符合條件：{} / {} 個系統字型家族",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::ZhCn => format!(
                "符合条件：{} / {} 个系统字体家族",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::En => format!(
                "Matching: {} / {} system font families",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::Ja => format!(
                "一致：{} / {} システムフォントファミリー",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::Ko => format!(
                "일치: {} / {} 시스템 글꼴 패밀리",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::Es => format!(
                "Coincidencias: {} / {} familias de fuentes del sistema",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::Fr => format!(
                "Correspondances : {} / {} familles de polices système",
                matches.len(),
                self.detected_fonts.len()
            ),
            crate::i18n::Locale::De => format!(
                "Treffer: {} / {} System-Schriftfamilien",
                matches.len(),
                self.detected_fonts.len()
            ),
        };
        ui.label(
            RichText::new(count_text)
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
        render_inset_card(
            ui,
            l(
                "字型與備援鏈 (Typography & Fallback Chain)",
                "字体与后备链 (Typography & Fallback Chain)",
                "Typography & Fallback Chain",
            ),
            |b| {
                b.block(
                    l(
                        "字型家族與優先順序 (Font Family & Fallback Order)",
                        "字体家族与优先级 (Font Family & Fallback Order)",
                        "Font Family & Fallback Order",
                    ),
                    Some(l(
                        "排在第一順位的為主字型，遇到缺字（如 Nerd 圖示或中文）時自動依序往後遞補。",
                        "排在第一顺位的为主字体，遇到缺字（如 Nerd 图标或中文）时自动依序向后递补。",
                        "First font is primary; missing glyphs (Nerd icons or CJK) fall back in order.",
                    )),
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
                                        RichText::new(l(
                                            "目前未指定自訂字型，使用系統預設等寬字型。",
                                            "当前未指定自定义字体，使用系统默认等宽字体。",
                                            "No custom font configured; using system default monospace font.",
                                        ))
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
                                                l("主字型", "主字体", "Primary").to_string()
                                            } else {
                                                format!("{} #{}", l("備援", "后备", "Fallback"), idx)
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
                                                        RichText::new(l(
                                                            "→ 找不到此字型，將自動略過",
                                                            "→ 找不到此字体，将自动跳过",
                                                            "→ Font not found, will be skipped",
                                                        ))
                                                        .size(11.0)
                                                        .color(Color32::from_rgb(235, 110, 100)),
                                                    );
                                                }
                                            }

                                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                                if ui
                                                    .small_button("✖")
                                                    .on_hover_text(l(
                                                        "從字型鏈移除",
                                                        "从字体链移除",
                                                        "Remove from font chain",
                                                    ))
                                                    .clicked()
                                                {
                                                    remove_idx = Some(idx);
                                                }
                                                if ui
                                                    .add_enabled(
                                                        idx + 1 < len,
                                                        egui::Button::new("↓").small(),
                                                    )
                                                    .on_hover_text(l(
                                                        "往後移（降低優先順序）",
                                                        "向后移（降低优先级）",
                                                        "Move down (lower priority)",
                                                    ))
                                                    .clicked()
                                                {
                                                    move_down = Some(idx);
                                                }
                                                if ui
                                                    .add_enabled(
                                                        idx > 0,
                                                        egui::Button::new("↑").small(),
                                                    )
                                                    .on_hover_text(l(
                                                        "往前移（提高優先順序）",
                                                        "向前移（提高优先级）",
                                                        "Move up (higher priority)",
                                                    ))
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
                                .selected_text(l(
                                    "🎯 變更主字型",
                                    "🎯 变更主字体",
                                    "🎯 Primary Font",
                                ))
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
                                .selected_text(l(
                                    "＋ 新增備援字型",
                                    "＋ 新增后备字体",
                                    "＋ Add Fallback",
                                ))
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
                                .selected_text(l(
                                    "✨ 套用組合範本",
                                    "✨ 套用组合模板",
                                    "✨ Font Presets",
                                ))
                                .width(155.0)
                                .show_ui(ui, |ui| {
                                    #[cfg(target_os = "linux")]
                                    let presets = [
                                        (
                                            l(
                                                "NotoSansM Nerd + 思源黑體 TC (推薦 🚀)",
                                                "NotoSansM Nerd + 思源黑体 TC",
                                                "NotoSansM Nerd + Noto CJK TC (Recommended 🚀)",
                                            ),
                                            "NotoSansMNerdFontMono, Noto Sans CJK TC, DejaVu Sans Mono",
                                        ),
                                        (
                                            l(
                                                "NotoSansM Nerd + 思源黑體 SC (簡中)",
                                                "NotoSansM Nerd + 思源黑体 SC (推荐 🚀)",
                                                "NotoSansM Nerd + Noto CJK SC",
                                            ),
                                            "NotoSansMNerdFontMono, Noto Sans CJK SC, DejaVu Sans Mono",
                                        ),
                                        (
                                            l(
                                                "JetBrainsMono Nerd + 思源黑體",
                                                "JetBrainsMono Nerd + 思源黑体",
                                                "JetBrainsMono Nerd + Noto CJK",
                                            ),
                                            l(
                                                "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono",
                                                "JetBrainsMono Nerd Font, Noto Sans CJK SC, DejaVu Sans Mono",
                                                "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono",
                                            ),
                                        ),
                                        (
                                            l(
                                                "FiraCode Nerd + 思源黑體",
                                                "FiraCode Nerd + 思源黑体",
                                                "FiraCode Nerd + Noto CJK",
                                            ),
                                            l(
                                                "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono",
                                                "FiraCode Nerd Font, Noto Sans CJK SC, Ubuntu Mono",
                                                "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono",
                                            ),
                                        ),
                                        (
                                            l(
                                                "Ubuntu / DejaVu (Linux 經典)",
                                                "Ubuntu / DejaVu (Linux 经典)",
                                                "Ubuntu / DejaVu (Linux Classic)",
                                            ),
                                            l(
                                                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC",
                                                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK SC",
                                                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC",
                                            ),
                                        ),
                                    ];
                                    #[cfg(target_os = "macos")]
                                    let presets = [
                                        (
                                            l(
                                                "NotoSansM Nerd Font + 繁中 (推薦 🚀)",
                                                "NotoSansM Nerd Font + 繁中",
                                                "NotoSansM Nerd Font + TC (Recommended 🚀)",
                                            ),
                                            "NotoSansMNerdFontMono, NotoSansTC, Menlo",
                                        ),
                                        (
                                            l(
                                                "NotoSansM Nerd Font + 簡中",
                                                "NotoSansM Nerd Font + 简中 (推荐 🚀)",
                                                "NotoSansM Nerd Font + SC",
                                            ),
                                            "NotoSansMNerdFontMono, PingFang SC, Menlo",
                                        ),
                                        (
                                            "JetBrainsMono Nerd Font",
                                            l(
                                                "JetBrainsMono Nerd Font, PingFang TC",
                                                "JetBrainsMono Nerd Font, PingFang SC",
                                                "JetBrainsMono Nerd Font, PingFang TC",
                                            ),
                                        ),
                                        (
                                            "FiraCode Nerd Font",
                                            l(
                                                "FiraCode Nerd Font, PingFang TC",
                                                "FiraCode Nerd Font, PingFang SC",
                                                "FiraCode Nerd Font, PingFang TC",
                                            ),
                                        ),
                                        (
                                            l("SF Mono + 蘋方", "SF Mono + 苹方", "SF Mono + PingFang"),
                                            l("SF Mono, PingFang TC", "SF Mono, PingFang SC", "SF Mono, PingFang TC"),
                                        ),
                                        (
                                            l("Menlo (macOS 預設)", "Menlo (macOS 默认)", "Menlo (macOS Default)"),
                                            l("Menlo, PingFang TC", "Menlo, PingFang SC", "Menlo, PingFang TC"),
                                        ),
                                    ];
                                    #[cfg(target_os = "windows")]
                                    let presets = [
                                        (
                                            l(
                                                "Cascadia Mono + 微軟正黑體 (推薦 🚀)",
                                                "Cascadia Mono + 微软正黑体",
                                                "Cascadia Mono + JhengHei (Recommended 🚀)",
                                            ),
                                            "Cascadia Mono, Microsoft JhengHei UI, Consolas",
                                        ),
                                        (
                                            l(
                                                "Cascadia Mono + 微軟雅黑 (簡中)",
                                                "Cascadia Mono + 微软雅黑 (推荐 🚀)",
                                                "Cascadia Mono + YaHei",
                                            ),
                                            "Cascadia Mono, Microsoft YaHei UI, Consolas",
                                        ),
                                        (
                                            "JetBrainsMono Nerd Font",
                                            l(
                                                "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
                                                "JetBrainsMono Nerd Font, Microsoft YaHei UI",
                                                "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
                                            ),
                                        ),
                                        (
                                            l(
                                                "Consolas (Windows 預設)",
                                                "Consolas (Windows 默认)",
                                                "Consolas (Windows Default)",
                                            ),
                                            l(
                                                "Consolas, Microsoft JhengHei UI",
                                                "Consolas, Microsoft YaHei UI",
                                                "Consolas, Microsoft JhengHei UI",
                                            ),
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
                                    l("▲ 收合手動編輯", "▲ 收起手动编辑", "▲ Hide Manual Input")
                                } else {
                                    l("✎ 手動編輯字串", "✎ 手动编辑字符串", "✎ Manual Edit")
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
                                    .button(l("清除", "清除", "Clear"))
                                    .on_hover_text(l(
                                        "清空自訂字型，回復系統預設",
                                        "清空自定义字体，恢复系统默认",
                                        "Clear custom fonts and use system default",
                                    ))
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
                                        .hint_text(l(
                                            "以逗號分隔，例如：JetBrainsMono Nerd Font, Noto Sans CJK TC",
                                            "以逗号分隔，例如：JetBrainsMono Nerd Font, Noto Sans CJK SC",
                                            "Comma-separated, e.g.: JetBrainsMono Nerd Font, Noto Sans CJK TC",
                                        ))
                                        .font(egui::TextStyle::Monospace),
                                );
                                let enter_pressed = resp.lost_focus()
                                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                if ui.button(l("✔ 套用", "✔ 套用", "✔ Apply")).clicked()
                                    || enter_pressed
                                {
                                    self.font_family = self.font_family_input.trim().to_string();
                                }
                            });
                        }
                    },
                );

                b.row(
                    l("字型大小 (Font Size)", "字体大小 (Font Size)", "Font Size"),
                    Some(l(
                        "終端文字大小 (pt)",
                        "终端文字大小 (pt)",
                        "Terminal text size (pt)",
                    )),
                    |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.font_size, 9.0..=36.0)
                                .suffix(" pt")
                                .step_by(0.5),
                        );
                    },
                );

                b.row(
                    l(
                        "行高倍率 (Line Height)",
                        "行高倍率 (Line Height)",
                        "Line Height",
                    ),
                    Some(l(
                        "1.0 為緊湊，1.2 為標準舒適（即時套用）",
                        "1.0 为紧凑，1.2 为标准舒适（实时应用）",
                        "1.0 is compact, 1.2 is comfortable (applied live)",
                    )),
                    |ui| {
                        ui.add(egui::Slider::new(&mut self.line_height, 1.0..=2.0).step_by(0.05));
                    },
                );
            },
        );

        render_inset_card(
            ui,
            l(
                "視窗幾何與邊距 (Window & Layout)",
                "窗口几何与边距 (Window & Layout)",
                "Window & Layout",
            ),
            |b| {
                b.row(
                    l(
                        "預設終端大小 (Columns × Rows)",
                        "默认终端大小 (Columns × Rows)",
                        "Default Terminal Size (Columns × Rows)",
                    ),
                    Some(l(
                        "啟動時初始網格欄列數 (寬 × 高)",
                        "启动时初始网格行列数 (宽 × 高)",
                        "Initial grid columns and rows on startup",
                    )),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::DragValue::new(&mut self.columns)
                                    .range(40..=400)
                                    .prefix(l("寬 (Cols): ", "宽 (Cols): ", "Cols: ")),
                            );
                            ui.label("×");
                            ui.add(
                                egui::DragValue::new(&mut self.rows)
                                    .range(10..=200)
                                    .prefix(l("高 (Rows): ", "高 (Rows): ", "Rows: ")),
                            );
                        });
                    },
                );

                b.row(
                    l(
                        "水平與垂直內距 (Padding X / Y)",
                        "水平与垂直内边距 (Padding X / Y)",
                        "Padding X / Y",
                    ),
                    Some(l(
                        "終端文字四周保留的留白像素",
                        "终端文字四周保留的留白像素",
                        "Inner margin around terminal cells in pixels",
                    )),
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.add(
                                egui::Slider::new(&mut self.padding_x, 0.0..=40.0)
                                    .suffix(" px")
                                    .prefix(l("左右 X: ", "左右 X: ", "X: ")),
                            );
                            ui.add_space(6.0);
                            ui.add(
                                egui::Slider::new(&mut self.padding_y, 0.0..=40.0)
                                    .suffix(" px")
                                    .prefix(l("上下 Y: ", "上下 Y: ", "Y: ")),
                            );
                        });
                    },
                );

                b.row(
                    l(
                        "背景不透明度 (Opacity)",
                        "背景不透明度 (Opacity)",
                        "Background Opacity",
                    ),
                    Some(l(
                        "1.0 為完全不透明，調低可搭配桌面透明效果",
                        "1.0 为完全不透明，调低可搭配桌面透明效果",
                        "1.0 is fully opaque; lower values enable window transparency",
                    )),
                    |ui| {
                        ui.add(egui::Slider::new(&mut self.opacity, 0.1..=1.0).step_by(0.05));
                    },
                );

                b.row(
                    l(
                        "視窗邊框樣式 (Decorations)",
                        "窗口边框样式 (Decorations)",
                        "Window Decorations",
                    ),
                    Some(l(
                        "控制系統標題列與視窗外框顯示",
                        "控制系统标题栏与窗口外框显示",
                        "Control native title bar and window frame style",
                    )),
                    |ui| {
                        let mut cur = self.decorations.as_str();
                        let opts = [
                            ("full", l("完整標題列", "完整标题栏", "Full")),
                            ("transparent", l("透明融合", "透明融合", "Transparent")),
                            ("none", l("無邊框", "无边框", "None")),
                        ];
                        if render_macos_segmented_control(ui, &mut cur, &opts) {
                            self.decorations = cur.to_string();
                        }
                    },
                );

                b.row(
                    l(
                        "啟動模式 (Startup Mode)",
                        "启动模式 (Startup Mode)",
                        "Startup Mode",
                    ),
                    Some(l(
                        "應用程式啟動時的初始視窗狀態",
                        "应用程序启动时的初始窗口状态",
                        "Initial window state when launching the application",
                    )),
                    |ui| {
                        let mut cur = self.startup_mode.as_str();
                        let opts = [
                            ("normal", l("標準視窗", "标准窗口", "Normal")),
                            ("maximized", l("最大化", "最大化", "Maximized")),
                            ("fullscreen", l("全螢幕", "全屏幕", "Fullscreen")),
                        ];
                        if render_macos_segmented_control(ui, &mut cur, &opts) {
                            self.startup_mode = cur.to_string();
                        }
                    },
                );

                b.row(
                    l(
                        "分頁列顯示策略 (Tab Bar)",
                        "标签栏显示策略 (Tab Bar)",
                        "Tab Bar Visibility",
                    ),
                    Some(l(
                        "隱藏分頁列時，滑鼠移至視窗右上角仍會浮現 [AI] 與 [⚙] 快捷膠囊",
                        "隐藏标签栏时，鼠标移至窗口右上角仍会浮现 [AI] 与 [⚙] 快捷胶囊",
                        "When hidden, hovering top-right still reveals the [AI] and [⚙] quick buttons",
                    )),
                    |ui| {
                        let mut cur = self.tab_bar_style.as_str();
                        let opts = [
                            ("always", l("恆常顯示", "始终显示", "Always")),
                            ("auto", l("單頁自動隱藏", "单页自动隐藏", "Auto-hide")),
                            ("never", l("永不顯示", "从不显示", "Never")),
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
                        let opts = [
                            ("auto", s.pref_language_auto),
                            ("zh-TW", s.pref_language_zh_tw),
                            ("zh-CN", s.pref_language_zh_cn),
                            ("en", s.pref_language_en),
                            ("ja", s.pref_language_ja),
                            ("ko", s.pref_language_ko),
                            ("es", s.pref_language_es),
                            ("fr", s.pref_language_fr),
                            ("de", s.pref_language_de),
                        ];
                        let active_code = if self.language.trim().is_empty()
                            || self.language.trim().eq_ignore_ascii_case("auto")
                        {
                            "auto"
                        } else {
                            crate::i18n::Locale::from_tag(&self.language)
                                .map(|loc| loc.code())
                                .unwrap_or("auto")
                        };
                        let current_label = opts
                            .iter()
                            .find(|(code, _)| *code == active_code)
                            .map(|(_, label)| *label)
                            .unwrap_or(s.pref_language_auto);
                        egui::ComboBox::from_id_salt("pref_window_language_cb")
                            .selected_text(current_label)
                            .width(190.0)
                            .show_ui(ui, |ui| {
                                for (code, label) in opts {
                                    if ui.selectable_label(active_code == code, label).clicked() {
                                        self.language = code.to_string();
                                    }
                                }
                            });
                    },
                );

                b.row(
                    l(
                        "啟動時自動檢查更新 (Auto-Check Updates)",
                        "启动时自动检查更新 (Auto-Check Updates)",
                        "Auto-Check for Updates on Startup",
                    ),
                    Some(l(
                        "啟動時於背景非阻塞檢查 GitHub Releases 是否有新版可 OTA 升級",
                        "启动时于后台非阻塞检查 GitHub Releases 是否有新版可 OTA 升级",
                        "Check GitHub Releases in the background on startup for OTA updates",
                    )),
                    |ui| {
                        render_macos_toggle_switch(ui, &mut self.auto_check_updates);
                    },
                );
            },
        );
    }

    // 2. Colors Tab View
    fn render_colors_view(&mut self, ui: &mut Ui) {
        self.render_terminal_live_preview(ui);

        render_inset_card(
            ui,
            l(
                "色彩主題 (Preset Themes)",
                "色彩主题 (Preset Themes)",
                "Preset Themes",
            ),
            |b| {
                b.block(
                    l(
                        "一鍵主題卡片畫廊 (Theme Gallery)",
                        "一键主题卡片画廊 (Theme Gallery)",
                        "Theme Gallery",
                    ),
                    Some(l(
                        "點擊卡片立即套用主題底色與 ANSI 16 色調色盤（並重設自訂覆寫色）",
                        "点击卡片立即套用主题底色与 ANSI 16 色调色板（并重置自定义覆盖色）",
                        "Click a card to apply its colors and ANSI 16-color palette (resets custom overrides)",
                    )),
                    |ui| {
                        let themes = [
                            (
                                "pure-black",
                                l(
                                    "Pure Black 經典黑 🖤",
                                    "Pure Black 经典黑 🖤",
                                    "Pure Black 🖤",
                                ),
                            ),
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
                        let card_w =
                            ((avail_w - spacing_x * (cols as f32 - 1.0)) / cols as f32).max(150.0);

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
            },
        );

        let preset = crate::config::Theme::get(&self.theme);
        render_inset_card(
            ui,
            l(
                "自訂背景與文字顏色 (Custom Overrides)",
                "自定义背景与文字颜色 (Custom Overrides)",
                "Custom Color Overrides",
            ),
            |b| {
                b.row(
                    l(
                        "背景底色 (Background)",
                        "背景底色 (Background)",
                        "Background",
                    ),
                    Some(l(
                        "留空則繼承主題預設背景色",
                        "留空则继承主题默认背景色",
                        "Leave empty to inherit theme background",
                    )),
                    |ui| {
                        render_color_row_with_presets(
                            ui,
                            rgba_to_color32(preset.background),
                            &mut self.bg_override,
                            &[
                                (
                                    "#000000",
                                    Color32::from_rgb(0, 0, 0),
                                    l("OLED 純黑", "OLED 纯黑", "OLED Black"),
                                ),
                                (
                                    "#0a0a0e",
                                    Color32::from_rgb(10, 10, 14),
                                    l("深墨黑", "深墨黑", "Ink Black"),
                                ),
                                (
                                    "#18181f",
                                    Color32::from_rgb(24, 24, 31),
                                    l("藏青黑", "藏青黑", "Midnight"),
                                ),
                            ],
                        );
                    },
                );
                b.row(
                    l(
                        "前景文字色 (Foreground)",
                        "前景文字色 (Foreground)",
                        "Foreground",
                    ),
                    Some(l(
                        "留空則繼承主題預設文字色",
                        "留空则继承主题默认文字色",
                        "Leave empty to inherit theme foreground",
                    )),
                    |ui| {
                        render_color_row_with_presets(
                            ui,
                            rgba_to_color32(preset.foreground),
                            &mut self.fg_override,
                            &[
                                (
                                    "#f5f5f7",
                                    Color32::from_rgb(245, 245, 247),
                                    l("亮白", "亮白", "Bright White"),
                                ),
                                (
                                    "#30d158",
                                    Color32::from_rgb(48, 209, 88),
                                    l("駭客綠", "黑客绿", "Matrix Green"),
                                ),
                                (
                                    "#ff9f0a",
                                    Color32::from_rgb(255, 159, 10),
                                    l("琥珀黃", "琥珀黄", "Amber"),
                                ),
                            ],
                        );
                    },
                );
                b.row(
                    l("游標顏色 (Cursor)", "光标颜色 (Cursor)", "Cursor Color"),
                    Some(l(
                        "留空則繼承主題預設游標色",
                        "留空则继承主题默认光标色",
                        "Leave empty to inherit theme cursor color",
                    )),
                    |ui| {
                        render_color_row_with_presets(
                            ui,
                            rgba_to_color32(preset.cursor),
                            &mut self.cursor_override,
                            &[
                                (
                                    "#0a84ff",
                                    Color32::from_rgb(10, 132, 255),
                                    l("macOS 藍", "macOS 蓝", "macOS Blue"),
                                ),
                                (
                                    "#30d158",
                                    Color32::from_rgb(48, 209, 88),
                                    l("翠綠", "翠绿", "Green"),
                                ),
                                (
                                    "#ff453a",
                                    Color32::from_rgb(255, 69, 58),
                                    l("鮮紅", "鲜红", "Red"),
                                ),
                            ],
                        );
                    },
                );
                b.row(
                    l(
                        "選取文字底色 (Selection)",
                        "选中文字底色 (Selection)",
                        "Selection Background",
                    ),
                    Some(l(
                        "滑鼠反白選取範圍的背景色",
                        "鼠标高亮选中范围的背景色",
                        "Highlight background color for selected text",
                    )),
                    |ui| {
                        render_color_row_with_presets(
                            ui,
                            rgba_to_color32(preset.selection),
                            &mut self.selection_override,
                            &[
                                (
                                    "#2a4b7c",
                                    Color32::from_rgb(42, 75, 124),
                                    l("經典藍選取", "经典蓝选中", "Classic Blue"),
                                ),
                                (
                                    "#383844",
                                    Color32::from_rgb(56, 56, 68),
                                    l("柔和灰選取", "柔和灰选中", "Soft Gray"),
                                ),
                            ],
                        );
                    },
                );
            },
        );

        render_inset_card(
            ui,
            l(
                "進階 ANSI 16 色調色盤 (ANSI 16-Color Palette)",
                "高级 ANSI 16 色调色板 (ANSI 16-Color Palette)",
                "ANSI 16-Color Palette",
            ),
            |b| {
                b.block(
                    l(
                        "個別覆寫終端機 ANSI 16 色 (color0 ~ color15)",
                        "单独覆盖终端 ANSI 16 色 (color0 ~ color15)",
                        "Override Individual ANSI 16 Colors (color0 ~ color15)",
                    ),
                    Some(l(
                        "可微調標準 8 色與高亮 8 色；留空代表使用當前主題預設值。",
                        "可微调标准 8 色与高亮 8 色；留空代表使用当前主题默认值。",
                        "Fine-tune normal and bright ANSI colors; leave empty to use theme defaults.",
                    )),
                    |ui| {
                        ui.horizontal(|ui| {
                            let btn_label = if self.show_ansi_palette {
                                l(
                                    "▼ 收合 ANSI 16 色設定表",
                                    "▼ 收起 ANSI 16 色设置表",
                                    "▼ Hide ANSI 16-Color Table",
                                )
                            } else {
                                l(
                                    "▶ 展開 ANSI 16 色設定表 (0~15)",
                                    "▶ 展开 ANSI 16 色设置表 (0~15)",
                                    "▶ Show ANSI 16-Color Table (0~15)",
                                )
                            };
                            if ui.button(btn_label).clicked() {
                                self.show_ansi_palette = !self.show_ansi_palette;
                            }

                            let any_override =
                                self.palette_overrides.iter().any(|s| !s.trim().is_empty());
                            if any_override
                                && ui
                                    .button(l(
                                        "🔄 全部重設為主題預設",
                                        "🔄 全部重置为主题默认",
                                        "🔄 Reset All to Theme Default",
                                    ))
                                    .on_hover_text(l(
                                        "清除所有 color0 ~ color15 自訂覆寫",
                                        "清除所有 color0 ~ color15 自定义覆盖",
                                        "Clear all color0 ~ color15 overrides",
                                    ))
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
                                l("0 黑 (Black)", "0 黑 (Black)", "0 Black"),
                                l("1 紅 (Red)", "1 红 (Red)", "1 Red"),
                                l("2 綠 (Green)", "2 绿 (Green)", "2 Green"),
                                l("3 黃 (Yellow)", "3 黄 (Yellow)", "3 Yellow"),
                                l("4 藍 (Blue)", "4 蓝 (Blue)", "4 Blue"),
                                l("5 洋紅 (Magenta)", "5 洋红 (Magenta)", "5 Magenta"),
                                l("6 青 (Cyan)", "6 青 (Cyan)", "6 Cyan"),
                                l("7 白 (White)", "7 白 (White)", "7 White"),
                                l("8 亮黑 (BrBlack)", "8 亮黑 (BrBlack)", "8 BrBlack"),
                                l("9 亮紅 (BrRed)", "9 亮红 (BrRed)", "9 BrRed"),
                                l("10 亮綠 (BrGreen)", "10 亮绿 (BrGreen)", "10 BrGreen"),
                                l("11 亮黃 (BrYellow)", "11 亮黄 (BrYellow)", "11 BrYellow"),
                                l("12 亮藍 (BrBlue)", "12 亮蓝 (BrBlue)", "12 BrBlue"),
                                l("13 亮洋紅 (BrMag)", "13 亮洋红 (BrMag)", "13 BrMag"),
                                l("14 亮青 (BrCyan)", "14 亮青 (BrCyan)", "14 BrCyan"),
                                l("15 亮白 (BrWhite)", "15 亮白 (BrWhite)", "15 BrWhite"),
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
                                                let mut cur =
                                                    parse_hex_color(&self.palette_overrides[idx])
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
                                                        .on_hover_text(l(
                                                            "重設為主題預設",
                                                            "重置为主题默认",
                                                            "Reset to theme default",
                                                        ))
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
            },
        );
    }

    // 3. Terminal & Shell Tab View
    fn render_terminal_view(&mut self, ui: &mut Ui) {
        render_inset_card(
            ui,
            l(
                "Shell 與啟動環境 (Shell & Environment)",
                "Shell 与启动环境 (Shell & Environment)",
                "Shell & Environment",
            ),
            |b| {
                b.row(
                    l("TERM 模擬器代碼", "TERM 模拟器代码", "TERM Variable"),
                    Some(l(
                        "環境變數 $TERM 預設值",
                        "环境变量 $TERM 默认值",
                        "Default value for $TERM environment variable",
                    )),
                    |ui| {
                        ui.add(egui::TextEdit::singleline(&mut self.term).desired_width(150.0));
                    },
                );

                let presets = skterm_core::available_shell_presets();
                let current_label = if !self.custom_shell_enabled
                    || self.shell_program.trim().is_empty()
                {
                    #[cfg(not(target_os = "windows"))]
                    {
                        let env_shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into());
                        format!(
                            "{} ({})",
                            l("系統預設", "系统默认", "System Default"),
                            env_shell
                        )
                    }
                    #[cfg(target_os = "windows")]
                    {
                        l(
                            "系統預設 (System Default)",
                            "系统默认 (System Default)",
                            "System Default",
                        )
                        .to_string()
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
                        format!("{} ({})", l("自訂", "自定义", "Custom"), self.shell_program)
                    }
                };

                #[cfg(target_os = "windows")]
                let shell_subtitle = l(
                    "自由切換命令提示字元、Windows PowerShell 或 PowerShell 7",
                    "自由切换命令提示符、Windows PowerShell 或 PowerShell 7",
                    "Switch between Command Prompt, Windows PowerShell, or PowerShell 7",
                );
                #[cfg(not(target_os = "windows"))]
                let shell_subtitle = l(
                    "自動偵測系統已安裝的 Shell（如 Bash、Zsh、Fish 等）並去除重複連結",
                    "自动检测系统已安装的 Shell（如 Bash、Zsh、Fish 等）并去除重复链接",
                    "Automatically detects installed system shells (Bash, Zsh, Fish, etc.)",
                );

                b.row(
                    l("預設 Shell 核心", "默认 Shell 核心", "Default Shell"),
                    Some(shell_subtitle),
                    |ui| {
                        egui::ComboBox::from_id_salt("pref_window_shell_preset")
                            .selected_text(&current_label)
                            .width(230.0)
                            .show_ui(ui, |ui| {
                                if ui
                                    .selectable_label(
                                        !self.custom_shell_enabled,
                                        l(
                                            "系統預設 ($SHELL / System Default)",
                                            "系统默认 ($SHELL / System Default)",
                                            "System Default ($SHELL)",
                                        ),
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
                                        format!(
                                            "{} ({})",
                                            p.name,
                                            l("未安裝", "未安装", "Not Installed")
                                        )
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
                                    .selectable_label(
                                        is_custom,
                                        l(
                                            "自訂 Shell 路徑 (Custom...)",
                                            "自定义 Shell 路径 (Custom...)",
                                            "Custom Shell Path (Custom...)",
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.custom_shell_enabled = true;
                                }
                            });
                    },
                );

                b.row(
                    l(
                        "快速切換常用 Shell",
                        "快速切换常用 Shell",
                        "Quick Shell Switch",
                    ),
                    Some(l(
                        "點擊按鈕一鍵切換新分頁預設啟動的 Shell",
                        "点击按钮一键切换新标签页默认启动的 Shell",
                        "One-click switch for the default shell of new tabs",
                    )),
                    |ui| {
                        ui.horizontal_wrapped(|ui| {
                            let is_sys_default =
                                !self.custom_shell_enabled || self.shell_program.trim().is_empty();
                            if ui
                                .selectable_label(
                                    is_sys_default,
                                    l("🌟 系統預設", "🌟 系统默认", "🌟 System Default"),
                                )
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
                    l(
                        "手動指定執行檔路徑與參數",
                        "手动指定可执行文件路径与参数",
                        "Custom Executable Path & Arguments",
                    ),
                    Some(l(
                        "需要傳遞特殊啟動 flag 時開啟",
                        "需要传递特殊启动 flag 时开启",
                        "Enable when custom startup flags are needed",
                    )),
                    |ui| {
                        render_macos_toggle_switch(ui, &mut self.custom_shell_enabled);
                    },
                );

                if self.custom_shell_enabled {
                    #[cfg(target_os = "windows")]
                    let (prog_hint, args_hint) = ("pwsh.exe / powershell.exe / cmd.exe", "-NoLogo");
                    #[cfg(not(target_os = "windows"))]
                    let (prog_hint, args_hint) = (
                        l(
                            "/bin/zsh 或 /usr/bin/fish",
                            "/bin/zsh 或 /usr/bin/fish",
                            "/bin/zsh or /usr/bin/fish",
                        ),
                        "-l",
                    );

                    b.row(
                        l(
                            "執行檔路徑 (Program)",
                            "可执行文件路径 (Program)",
                            "Program",
                        ),
                        None,
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.shell_program)
                                    .hint_text(prog_hint)
                                    .desired_width(240.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                        },
                    );

                    b.row(
                        l("啟動參數 (Arguments)", "启动参数 (Arguments)", "Arguments"),
                        None,
                        |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut self.shell_args)
                                    .hint_text(args_hint)
                                    .desired_width(240.0)
                                    .font(egui::TextStyle::Monospace),
                            );
                        },
                    );
                }

                b.row(
                    l(
                        "初始工作目錄 (Working Directory)",
                        "初始工作目录 (Working Directory)",
                        "Working Directory",
                    ),
                    Some(l(
                        "終端啟動時的預設路徑（留空則為家目錄 ~/）",
                        "终端启动时的默认路径（留空则为主目录 ~/）",
                        "Default directory on startup (leave empty for home ~/)",
                    )),
                    |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.working_directory)
                                .hint_text(l(
                                    "預設使用者家目錄 (~/)",
                                    "默认用户主目录 (~/)",
                                    "Default user home (~/)",
                                ))
                                .desired_width(240.0),
                        );
                    },
                );
            },
        );

        render_inset_card(
            ui,
            l(
                "緩衝區與操作行為 (Buffer & Interaction)",
                "缓冲区与操作行为 (Buffer & Interaction)",
                "Buffer & Interaction",
            ),
            |b| {
                b.row(
                    l(
                        "滑鼠右鍵行為 (Right-Click Action)",
                        "鼠标右键行为 (Right-Click Action)",
                        "Right-Click Action",
                    ),
                    Some(l(
                        "按住 Shift + 右鍵 可隨時執行另一種行為",
                        "按住 Shift + 右键 可随时执行另一种行为",
                        "Hold Shift + Right-Click to perform the alternate action",
                    )),
                    |ui| {
                        let mut cur = self.right_click_action.as_str();
                        let opts = [
                            (
                                "menu",
                                l("開啟快捷選單 (預設)", "打开快捷菜单 (默认)", "Context Menu"),
                            ),
                            (
                                "paste",
                                l("直接貼上剪貼簿", "直接粘贴剪贴板", "Paste Clipboard"),
                            ),
                        ];
                        if render_macos_segmented_control(ui, &mut cur, &opts) {
                            self.right_click_action = cur.to_string();
                        }
                    },
                );

                b.row(
                    l(
                        "滾動歷史行數 (Scrollback)",
                        "滚动历史行数 (Scrollback)",
                        "Scrollback Lines",
                    ),
                    Some(l(
                        "保留向上捲動緩衝行數",
                        "保留向上滚动缓冲行数",
                        "Number of scrollback history lines retained",
                    )),
                    |ui| {
                        ui.add(
                            egui::Slider::new(&mut self.scrollback_lines, 1000..=50000)
                                .step_by(1000.0)
                                .suffix(l(" 行", " 行", " lines")),
                        );
                    },
                );

                b.row(
                    l(
                        "Alt / Option 鍵 Meta 模式",
                        "Alt / Option 键 Meta 模式",
                        "Alt / Option as Meta",
                    ),
                    Some(l(
                        "控制 Alt / Option 鍵是否發送終端機 Meta (Esc+) 序列",
                        "控制 Alt / Option 键是否发送终端 Meta (Esc+) 序列",
                        "Control whether Alt / Option keys send terminal Meta (Esc+) sequences",
                    )),
                    |ui| {
                        let mut cur = self.option_as_alt.as_str();
                        let opts = [
                            ("both", l("左右均作 Meta", "左右均作 Meta", "Both")),
                            ("only_left", l("僅左側", "仅左侧", "Left Only")),
                            ("only_right", l("僅右側", "仅右侧", "Right Only")),
                            ("none", l("關閉", "关闭", "None")),
                        ];
                        if render_macos_segmented_control(ui, &mut cur, &opts) {
                            self.option_as_alt = cur.to_string();
                        }
                    },
                );

                b.row(
                    l(
                        "終端鈴聲模式 (Bell)",
                        "终端响铃模式 (Bell)",
                        "Terminal Bell",
                    ),
                    Some(l(
                        "收到 BEL 警示訊號時的回饋方式",
                        "收到 BEL 警示信号时的反馈方式",
                        "Feedback style when BEL character is received",
                    )),
                    |ui| {
                        let mut cur = self.bell.as_str();
                        let opts = [
                            ("visual", l("視覺閃爍", "视觉闪烁", "Visual")),
                            ("sound", l("系統聲音", "系统声音", "Sound")),
                            ("none", l("靜音", "静音", "Muted")),
                        ];
                        if render_macos_segmented_control(ui, &mut cur, &opts) {
                            self.bell = cur.to_string();
                        }
                    },
                );
            },
        );
    }

    // 4. Cursor Tab View
    fn render_cursor_view(&mut self, ui: &mut Ui) {
        self.render_terminal_live_preview(ui);

        render_inset_card(
            ui,
            l(
                "游標樣式與動態 (Cursor Style)",
                "光标样式与动态 (Cursor Style)",
                "Cursor Style & Animation",
            ),
            |b| {
                b.row(
                    l("游標形狀 (Shape)", "光标形状 (Shape)", "Cursor Shape"),
                    Some(l(
                        "終端游標幾何外觀（上方預覽盒可即時檢視）",
                        "终端光标几何外观（上方预览框可实时查看）",
                        "Terminal cursor geometry (previewed live above)",
                    )),
                    |ui| {
                        let mut cur = self.cursor_shape.as_str();
                        let opts = [
                            ("block", l("方塊 (Block █)", "方块 (Block █)", "Block █")),
                            ("beam", l("直條 (Beam ❘)", "竖线 (Beam ❘)", "Beam ❘")),
                            (
                                "underline",
                                l("底線 (Underline _)", "下划线 (Underline _)", "Underline _"),
                            ),
                        ];
                        if render_macos_segmented_control(ui, &mut cur, &opts) {
                            self.cursor_shape = cur.to_string();
                        }
                    },
                );

                b.row(
                    l("游標閃爍 (Blinking)", "光标闪烁 (Blinking)", "Cursor Blink"),
                    Some(l(
                        "開啟游標平滑呼吸動畫",
                        "开启光标平滑呼吸动画",
                        "Enable smooth cursor blink animation",
                    )),
                    |ui| {
                        render_macos_toggle_switch(ui, &mut self.cursor_blink);
                    },
                );

                if self.cursor_blink {
                    b.row(
                        l(
                            "閃爍週期 (Interval)",
                            "闪烁周期 (Interval)",
                            "Blink Interval",
                        ),
                        Some(l(
                            "游標閃爍頻率毫秒",
                            "光标闪烁频率毫秒",
                            "Cursor blink interval in milliseconds",
                        )),
                        |ui| {
                            ui.add(
                                egui::Slider::new(&mut self.blink_interval_ms, 200..=1000)
                                    .step_by(50.0)
                                    .suffix(" ms"),
                            );
                        },
                    );
                }
            },
        );
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
                            .hint_text(l(
                                "🔍 搜尋快捷鍵或功能名稱 (例如：分割、搜尋、AI、偏好設定)...",
                                "🔍 搜索快捷键或功能名称 (例如：分屏、搜索、AI、偏好设置)...",
                                "🔍 Search shortcuts or actions (e.g. split, find, AI, preferences)...",
                            ))
                            .desired_width(280.0),
                    );
                    if !self.keybinding_filter.is_empty() && ui.small_button("✖").clicked() {
                        self.keybinding_filter.clear();
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let opts = [
                            (0u8, l("當前系統", "当前系统", "Current OS")),
                            (3u8, l("雙平台對照", "双平台对照", "Both")),
                            (1u8, "macOS"),
                            (2u8, "Linux / Win"),
                        ];
                        render_macos_segmented_control(ui, &mut self.keybinding_os_mode, &opts);
                    });
                });
            });

        ui.add_space(12.0);

        type ShortcutGroup<'a> = (
            &'a str,
            &'a [(&'a str, Option<&'a str>, &'a [&'a str], &'a [&'a str])],
        );

        let groups: &[ShortcutGroup] = &[
            (
                l(
                    "視窗與分頁管理 (Window & Tabs)",
                    "窗口与标签页管理 (Window & Tabs)",
                    "Window & Tabs",
                ),
                &[
                    (
                        l(
                            "開啟偏好設定視窗",
                            "打开偏好设置窗口",
                            "Open Preferences Window",
                        ),
                        Some(l(
                            "亦可點擊分頁列右上角 ⚙ 按鈕或終端機右鍵選單開啟",
                            "也可点击标签栏右上角 ⚙ 按钮或终端右键菜单打开",
                            "Also accessible via the top-right ⚙ button or context menu",
                        )),
                        &["⌘", ","],
                        &["Ctrl", ","],
                    ),
                    (
                        l(
                            "新增終端機視窗 (New Window)",
                            "新建终端窗口 (New Window)",
                            "New Terminal Window",
                        ),
                        None,
                        &["⌘", "N"],
                        &["Ctrl", "Shift", "N"],
                    ),
                    (
                        l("新增分頁 (New Tab)", "新建标签页 (New Tab)", "New Tab"),
                        Some(l(
                            "亦可點擊分頁列 ＋ 按鈕",
                            "也可点击标签栏 ＋ 按钮",
                            "Also accessible via the tab bar ＋ button",
                        )),
                        &["⌘", "T"],
                        &["Ctrl", "Shift", "T"],
                    ),
                    (
                        l(
                            "關閉當前窗格或分頁",
                            "关闭当前窗格或标签页",
                            "Close Active Pane or Tab",
                        ),
                        None,
                        &["⌘", "W"],
                        &["Ctrl", "Shift", "W"],
                    ),
                    (
                        l(
                            "切換至下一個 / 上一個分頁",
                            "切换至下一个 / 上一个标签页",
                            "Next / Previous Tab",
                        ),
                        None,
                        &["⌘", "⇧", "] / ["],
                        &["Ctrl", "Tab / Shift+Tab"],
                    ),
                    (
                        l(
                            "快速切換指定第 1~9 分頁",
                            "快速切换指定第 1~9 标签页",
                            "Switch to Tab 1~9",
                        ),
                        None,
                        &["⌘", "1 ~ 9"],
                        &["Ctrl", "1 ~ 9"],
                    ),
                ],
            ),
            (
                l(
                    "分割窗格與導覽 (Split Panes & Navigation)",
                    "分屏窗格与导航 (Split Panes & Navigation)",
                    "Split Panes & Navigation",
                ),
                &[
                    (
                        l(
                            "左右垂直切分窗格 (Split Vertical)",
                            "左右垂直分屏 (Split Vertical)",
                            "Split Pane Vertically",
                        ),
                        None,
                        &["⌘", "D"],
                        &["Ctrl", "Shift", "D"],
                    ),
                    (
                        l(
                            "上下水平切分窗格 (Split Horizontal)",
                            "上下水平分屏 (Split Horizontal)",
                            "Split Pane Horizontally",
                        ),
                        None,
                        &["⌘", "⇧", "D"],
                        &["Ctrl", "Shift", "H"],
                    ),
                    (
                        l(
                            "切換焦點至相鄰窗格",
                            "切换焦点至相邻窗格",
                            "Focus Next / Previous Pane",
                        ),
                        Some(l(
                            "Linux/Win 使用 Alt+[ 與 Alt+]，避免攔截終端機 Esc (Ctrl+[)",
                            "Linux/Win 使用 Alt+[ 与 Alt+]，避免拦截终端 Esc (Ctrl+[)",
                            "Linux/Win uses Alt+[ and Alt+] to avoid intercepting Esc (Ctrl+[)",
                        )),
                        &["⌘", "[ / ]"],
                        &["Alt", "[ / ]"],
                    ),
                    (
                        l(
                            "最大化 / 還原當前分割窗格 (Zoom Pane)",
                            "最大化 / 还原当前分屏窗格 (Zoom Pane)",
                            "Zoom / Restore Active Pane",
                        ),
                        None,
                        &["⌘", "⇧", "Z"],
                        &["Ctrl", "Shift", "Z"],
                    ),
                    (
                        l(
                            "微調分割窗格邊界比例",
                            "微调分屏窗格边界比例",
                            "Resize Split Pane Ratio",
                        ),
                        None,
                        &["⌘", "⌃", l("方向鍵", "方向键", "Arrow Keys")],
                        &["Ctrl", "Shift", l("方向鍵", "方向键", "Arrow Keys")],
                    ),
                ],
            ),
            (
                l(
                    "編輯、搜尋與 AI 助手 (Edit, Search & AI)",
                    "编辑、搜索与 AI 助手 (Edit, Search & AI)",
                    "Edit, Search & AI Assistant",
                ),
                &[
                    (
                        l(
                            "複製選取文字 (Copy)",
                            "复制选中文字 (Copy)",
                            "Copy Selection",
                        ),
                        None,
                        &["⌘", "C"],
                        &["Ctrl", "Shift", "C"],
                    ),
                    (
                        l(
                            "貼上剪貼簿文字 (Paste)",
                            "粘贴剪贴板文字 (Paste)",
                            "Paste Clipboard",
                        ),
                        None,
                        &["⌘", "V"],
                        &["Ctrl", "Shift", "V"],
                    ),
                    (
                        l(
                            "開啟畫面文字搜尋列 (Find)",
                            "打开画面文字搜索栏 (Find)",
                            "Find in Terminal",
                        ),
                        None,
                        &["⌘", "F"],
                        &["Ctrl", "Shift", "F"],
                    ),
                    (
                        l(
                            "切換 AI Agents 側邊欄",
                            "切换 AI Agents 侧边栏",
                            "Toggle AI Agents Sidebar",
                        ),
                        Some(l(
                            "亦可點擊右上角 AI 膠囊按鈕",
                            "也可点击右上角 AI 胶囊按钮",
                            "Also accessible via the top-right AI badge",
                        )),
                        &["⌘", "⇧", "A"],
                        &["Ctrl", "Shift", "A"],
                    ),
                    (
                        l(
                            "清除畫面與捲動緩衝區 (Clear Scrollback)",
                            "清除屏幕与滚动缓冲区 (Clear Scrollback)",
                            "Clear Screen & Scrollback",
                        ),
                        None,
                        &["⌘", "K"],
                        &["Ctrl", "Shift", "K"],
                    ),
                    (
                        l(
                            "開啟終端機右鍵選單 / 快速貼上",
                            "打开终端右键菜单 / 快速粘贴",
                            "Context Menu / Quick Paste",
                        ),
                        Some(l(
                            "可在「終端與 Shell」分頁自訂預設右鍵行為",
                            "可在「终端与 Shell」标签页自定义默认右键行为",
                            "Default right-click behavior is configurable under Terminal & Shell",
                        )),
                        &[l("滑鼠右鍵", "鼠标右键", "Right-Click")],
                        &[
                            l("滑鼠右鍵", "鼠标右键", "Right-Click"),
                            l("Shift+右鍵", "Shift+右键", "Shift+Right-Click"),
                        ],
                    ),
                ],
            ),
            (
                l(
                    "字型縮放與捲動控制 (Zoom, Scroll & Signals)",
                    "字体缩放与滚动控制 (Zoom, Scroll & Signals)",
                    "Zoom, Scroll & Signals",
                ),
                &[
                    (
                        l("字型放大 (Zoom In)", "字体放大 (Zoom In)", "Zoom In"),
                        None,
                        &["⌘", "="],
                        &["Ctrl", "="],
                    ),
                    (
                        l("字型縮小 (Zoom Out)", "字体缩小 (Zoom Out)", "Zoom Out"),
                        None,
                        &["⌘", "-"],
                        &["Ctrl", "-"],
                    ),
                    (
                        l(
                            "重設預設字型大小 (Reset Zoom)",
                            "重置默认字体大小 (Reset Zoom)",
                            "Reset Font Size",
                        ),
                        None,
                        &["⌘", "0"],
                        &["Ctrl", "0"],
                    ),
                    (
                        l(
                            "向上 / 向下翻頁捲動歷史",
                            "向上 / 向下翻页滚动历史",
                            "Scroll Page Up / Page Down",
                        ),
                        None,
                        &["⇧", "PageUp / PageDown"],
                        &["Shift", "PageUp / PageDown"],
                    ),
                    (
                        l(
                            "中斷目前執行行程 (SIGINT)",
                            "中断当前运行进程 (SIGINT)",
                            "Send Interrupt Signal (SIGINT)",
                        ),
                        None,
                        &["⌃", "C"],
                        &["Ctrl", "C"],
                    ),
                    (
                        l(
                            "傳送 EOF / 登出 Shell",
                            "发送 EOF / 退出 Shell",
                            "Send EOF / Logout Shell",
                        ),
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
                        || sub
                            .map(|s| s.to_lowercase().contains(&needle))
                            .unwrap_or(false)
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

/// Returns `(ja, ko, es, fr, de)` static translations for Preferences UI strings.
fn translate_extra<'a>(
    zh_tw: &'a str,
    en: &'a str,
) -> (&'a str, &'a str, &'a str, &'a str, &'a str) {
    match (zh_tw, en) {
        ("無邊框", "None") => ("枠なし", "테두리 없음", "Ninguno", "Aucune", "Keine"),
        ("關閉", "None") => ("無効", "사용 안 함", "Desactivado", "Désactivé", "Aus"),
        ("左右均作 Meta", "Both") => ("両方とも Meta", "양쪽 모두", "Ambos", "Les deux", "Beide"),
        ("雙平台對照", "Both") => ("両方表示", "모두 표시", "Ambos", "Les deux", "Beide"),
        (_, en) => match en {
            "Reset to default" => (
                "デフォルトに戻す",
                "기본값으로 재설정",
                "Restablecer al valor predeterminado",
                "Réinitialiser par défaut",
                "Auf Standard zurücksetzen",
            ),
            "Default (Theme)" => (
                "デフォルト (テーマ)",
                "기본값 (테마)",
                "Predeterminado (Tema)",
                "Par défaut (Thème)",
                "Standard (Thema)",
            ),
            "Click to open color picker" => (
                "クリックしてカラーピッカーを開く",
                "클릭하여 색상 선택기 열기",
                "Haz clic para abrir el selector de color",
                "Cliquer pour ouvrir le sélecteur de couleur",
                "Klicken, um Farbwähler zu öffnen",
            ),
            "Clear custom override and reset to theme default" => (
                "カスタム設定を解除し、テーマのデフォルトに戻す",
                "사용자 지정 색상을 지우고 테마 기본값으로 재설정",
                "Borrar color personalizado y volver al tema",
                "Effacer la couleur personnalisée et rétablir le thème",
                "Benutzerdefinierte Farbe löschen und Thema wiederherstellen",
            ),
            "skTerm Settings (Preferences)" => (
                "skTerm 設定 (Preferences)",
                "skTerm 설정 (Preferences)",
                "Ajustes de skTerm (Preferences)",
                "Réglages de skTerm (Preferences)",
                "skTerm Einstellungen (Preferences)",
            ),
            "skTerm Settings" => (
                "skTerm 設定",
                "skTerm 설정",
                "Ajustes de skTerm",
                "Réglages skTerm",
                "skTerm Einstellungen",
            ),
            "Live Hot-Reload Sync Active" => (
                "リアルタイム同期中 (Live Sync)",
                "실시간 핫 리로드 동기화 중 (Live Sync)",
                "Sincronización en vivo activa (Live Sync)",
                "Synchronisation en direct active (Live Sync)",
                "Live-Synchronisierung aktiv (Live Sync)",
            ),
            "🔤 Appearance" => (
                "🔤 外観とフォント",
                "🔤 모양 및 글꼴",
                "🔤 Apariencia",
                "🔤 Apparence",
                "🔤 Darstellung",
            ),
            "🎨 Colors" => (
                "🎨 カラーとテーマ",
                "🎨 색상 및 테마",
                "🎨 Colores",
                "🎨 Couleurs",
                "🎨 Farben",
            ),
            "🖥️ Terminal" => (
                "🖥️ ターミナルとシェル",
                "🖥️ 터미널 및 셸",
                "🖥️ Terminal",
                "🖥️ Terminal",
                "🖥️ Terminal",
            ),
            "🖱️ Cursor" => (
                "🖱️ カーソル",
                "🖱️ 커서",
                "🖱️ Cursor",
                "🖱️ Curseur",
                "🖱️ Cursor",
            ),
            "⌨️ Keybindings" => (
                "⌨️ ショートカット",
                "⌨️ 단축키",
                "⌨️ Atajos",
                "⌨️ Raccourcis",
                "⌨️ Kurzbefehle",
            ),
            "✕ Close Window (Esc)" => (
                "✕ ウインドウを閉じる (Esc)",
                "✕ 창 닫기 (Esc)",
                "✕ Cerrar ventana (Esc)",
                "✕ Fermer la fenêtre (Esc)",
                "✕ Fenster schließen (Esc)",
            ),
            "Close preferences window (Esc or Ctrl/⌘+W)" => (
                "設定ウインドウを閉じる (Esc または Ctrl/⌘+W)",
                "설정 창 닫기 (Esc 또는 Ctrl/⌘+W)",
                "Cerrar ventana de ajustes (Esc o Ctrl/⌘+W)",
                "Fermer la fenêtre des réglages (Esc ou Ctrl/⌘+W)",
                "Einstellungsfenster schließen (Esc oder Ctrl/⌘+W)",
            ),
            "📝 Edit config.toml" => (
                "📝 config.toml を編集",
                "📝 config.toml 편집",
                "📝 Editar config.toml",
                "📝 Modifier config.toml",
                "📝 config.toml bearbeiten",
            ),
            "Open TOML config file in default text editor" => (
                "標準のテキストエディタで TOML 設定ファイルを開く",
                "기본 텍스트 편집기에서 TOML 설정 파일 열기",
                "Abrir archivo TOML en el editor de texto predeterminado",
                "Ouvrir le fichier TOML dans l’éditeur de texte par défaut",
                "TOML-Konfigurationsdatei im Standardeditor öffnen",
            ),
            "🔄 Reset to Defaults" => (
                "🔄 初期設定に戻す",
                "🔄 기본값으로 재설정",
                "🔄 Restablecer valores",
                "🔄 Réinitialiser",
                "🔄 Zurücksetzen",
            ),
            "Restore all settings to factory defaults" => (
                "すべての設定を初期状態に戻す",
                "모든 설정을 초기 기본값으로 복원",
                "Restaurar todos los ajustes a los valores de fábrica",
                "Rétablir tous les réglages aux valeurs d’usine",
                "Alle Einstellungen auf Werkseinstellungen zurücksetzen",
            ),
            "🔄 Check Updates" => (
                "🔄 アップデート確認",
                "🔄 업데이트 확인",
                "🔄 Buscar actualizaciones",
                "🔄 Mises à jour",
                "🔄 Updates prüfen",
            ),
            "⏳ Checking for updates..." => (
                "⏳ アップデートを確認中...",
                "⏳ 업데이트 확인 중...",
                "⏳ Buscando actualizaciones...",
                "⏳ Recherche de mises à jour...",
                "⏳ Suche nach Updates...",
            ),
            "✓ Up to date" => (
                "✓ 最新バージョンです",
                "✓ 최신 버전입니다",
                "✓ Actualizado",
                "✓ À jour",
                "✓ Aktuell",
            ),
            "Check again" => (
                "再確認",
                "다시 확인",
                "Comprobar de nuevo",
                "Vérifier à nouveau",
                "Erneut prüfen",
            ),
            "New:" => ("新着:", "새 버전:", "Nuevo:", "Nouveau :", "Neu:"),
            "⬇ Install Update" => (
                "⬇ 更新をインストール",
                "⬇ 업데이트 설치",
                "⬇ Instalar actualización",
                "⬇ Installer la mise à jour",
                "⬇ Update installieren",
            ),
            "Downloading" => (
                "ダウンロード中",
                "다운로드 중",
                "Descargando",
                "Téléchargement",
                "Wird geladen",
            ),
            "Ready" => ("準備完了", "설치 완료", "Listo", "Prêt", "Bereit"),
            "🚀 Restart Now" => (
                "🚀 今すぐ再起動",
                "🚀 지금 다시 시작",
                "🚀 Reiniciar ahora",
                "🚀 Redémarrer",
                "🚀 Jetzt neu starten",
            ),
            "⚠ Update check failed" => (
                "⚠ 更新の確認に失敗しました",
                "⚠ 업데이트 확인 실패",
                "⚠ Error al buscar actualizaciones",
                "⚠ Échec de la vérification",
                "⚠ Update-Prüfung fehlgeschlagen",
            ),
            "Retry" => ("再試行", "재시도", "Reintentar", "Réessayer", "Wiederholen"),
            "System Default Monospace" => (
                "システム標準等幅フォント",
                "시스템 기본 고정폭 글꼴",
                "Monoespaciada del sistema",
                "Monospace système par défaut",
                "System-Standard-Monospace",
            ),
            "Default" => ("デフォルト", "기본값", "Predeterminado", "Par défaut", "Standard"),
            "skterm-gui v0.1.0 (Selection Preview)" => (
                "skterm-gui v0.1.0 (選択範囲プレビュー)",
                "skterm-gui v0.1.0 (선택 영역 미리보기)",
                "skterm-gui v0.1.0 (Vista previa de selección)",
                "skterm-gui v0.1.0 (Aperçu de la sélection)",
                "skterm-gui v0.1.0 (Auswahl-Vorschau)",
            ),
            "🔍 Search font family..." => (
                "🔍 フォント名を検索...",
                "🔍 글꼴 이름 검색...",
                "🔍 Buscar familia de fuente...",
                "🔍 Rechercher une police...",
                "🔍 Schriftfamilie suchen...",
            ),
            "Show terminal-friendly fonts only (Monospace / Nerd / CJK)" => (
                "ターミナル向けフォントのみ表示 (等幅 / Nerd / CJK)",
                "터미널 권장 글꼴만 표시 (고정폭 / Nerd / CJK)",
                "Mostrar solo fuentes para terminal (Monoespaciada / Nerd / CJK)",
                "Afficher uniquement les polices adaptées au terminal (Monospace / Nerd / CJK)",
                "Nur terminalfreundliche Schriften anzeigen (Monospace / Nerd / CJK)",
            ),
            "Typography & Fallback Chain" => (
                "タイポグラフィとフォールバック順序 (Typography & Fallback Chain)",
                "타이포그래피 및 대체 글꼴 체인 (Typography & Fallback Chain)",
                "Tipografía y cadena de respaldo",
                "Typographie et chaîne de polices de secours",
                "Typografie & Fallback-Reihenfolge",
            ),
            "Font Family & Fallback Order" => (
                "フォントファミリーと優先順位",
                "글꼴 패밀리 및 우선순위",
                "Familia de fuente y orden de respaldo",
                "Famille de polices et ordre de priorité",
                "Schriftfamilie & Fallback-Reihenfolge",
            ),
            "First font is primary; missing glyphs (Nerd icons or CJK) fall back in order." => (
                "先頭がメインフォントになり、不足する文字（Nerd アイコンや CJK など）は順番に補完されます。",
                "첫 번째 글꼴이 기본 글꼴이며, 누락된 글리프(Nerd 아이콘 및 CJK)는 순서대로 대체됩니다.",
                "La primera fuente es la principal; los glifos faltantes (iconos Nerd o CJK) usan las siguientes en orden.",
                "La première police est principale ; les glyphes manquants (icônes Nerd ou CJK) utilisent les suivantes dans l’ordre.",
                "Die erste Schrift ist primär; fehlende Glyphen (Nerd-Icons oder CJK) werden der Reihe nach ersetzt.",
            ),
            "No custom font configured; using system default monospace font." => (
                "カスタムフォント未設定。システム標準の等幅フォントを使用中です。",
                "사용자 지정 글꼴이 설정되지 않아 시스템 기본 고정폭 글꼴을 사용합니다.",
                "No se ha configurado ninguna fuente personalizada; usando la fuente monoespaciada del sistema.",
                "Aucune police personnalisée configurée ; utilisation de la police monospace système.",
                "Keine benutzerdefinierte Schrift konfiguriert; System-Monospace-Schrift wird verwendet.",
            ),
            "Primary" => ("メイン", "주 글꼴", "Principal", "Principale", "Primär"),
            "Fallback" => ("代替", "대체", "Respaldo", "Secours", "Fallback"),
            "→ Font not found, will be skipped" => (
                "→ フォントが見つかりません（自動的にスキップされます）",
                "→ 글꼴을 찾을 수 없음 (자동으로 건너뜀)",
                "→ Fuente no encontrada, se omitirá",
                "→ Police introuvable, sera ignorée",
                "→ Schrift nicht gefunden, wird übersprungen",
            ),
            "Remove from font chain" => (
                "フォントチェーンから削除",
                "글꼴 체인에서 제거",
                "Quitar de la cadena de fuentes",
                "Retirer de la chaîne de polices",
                "Aus der Schriftkette entfernen",
            ),
            "Move down (lower priority)" => (
                "下へ移動（優先度を下げる）",
                "아래로 이동 (우선순위 낮춤)",
                "Bajar (menor prioridad)",
                "Descendre (priorité moindre)",
                "Nach unten (niedrigere Priorität)",
            ),
            "Move up (higher priority)" => (
                "上へ移動（優先度を上げる）",
                "위로 이동 (우선순위 높임)",
                "Subir (mayor prioridad)",
                "Monter (priorité supérieure)",
                "Nach oben (höhere Priorität)",
            ),
            "🎯 Primary Font" => (
                "🎯 メインフォント変更",
                "🎯 주 글꼴 변경",
                "🎯 Fuente principal",
                "🎯 Police principale",
                "🎯 Primärschrift",
            ),
            "＋ Add Fallback" => (
                "＋ 代替フォント追加",
                "＋ 대체 글꼴 추가",
                "＋ Añadir respaldo",
                "＋ Police de secours",
                "＋ Fallback hinzufügen",
            ),
            "✨ Font Presets" => (
                "✨ プリセット適用",
                "✨ 글꼴 프리셋",
                "✨ Preajustes de fuente",
                "✨ Préréglages",
                "✨ Schrift-Presets",
            ),
            "NotoSansM Nerd + Noto CJK TC (Recommended 🚀)" => (
                "NotoSansM Nerd + Noto CJK (推奨 🚀)",
                "NotoSansM Nerd + Noto CJK (추천 🚀)",
                "NotoSansM Nerd + Noto CJK TC (Recomendado 🚀)",
                "NotoSansM Nerd + Noto CJK TC (Recommandé 🚀)",
                "NotoSansM Nerd + Noto CJK TC (Empfohlen 🚀)",
            ),
            "NotoSansM Nerd + Noto CJK SC" => (
                "NotoSansM Nerd + Noto CJK SC",
                "NotoSansM Nerd + Noto CJK SC",
                "NotoSansM Nerd + Noto CJK SC",
                "NotoSansM Nerd + Noto CJK SC",
                "NotoSansM Nerd + Noto CJK SC",
            ),
            "JetBrainsMono Nerd + Noto CJK" => (
                "JetBrainsMono Nerd + Noto CJK",
                "JetBrainsMono Nerd + Noto CJK",
                "JetBrainsMono Nerd + Noto CJK",
                "JetBrainsMono Nerd + Noto CJK",
                "JetBrainsMono Nerd + Noto CJK",
            ),
            "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono" => (
                "JetBrainsMono Nerd Font, Noto Sans CJK JP, DejaVu Sans Mono",
                "JetBrainsMono Nerd Font, Noto Sans CJK KR, DejaVu Sans Mono",
                "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono",
                "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono",
                "JetBrainsMono Nerd Font, Noto Sans CJK TC, DejaVu Sans Mono",
            ),
            "FiraCode Nerd + Noto CJK" => (
                "FiraCode Nerd + Noto CJK",
                "FiraCode Nerd + Noto CJK",
                "FiraCode Nerd + Noto CJK",
                "FiraCode Nerd + Noto CJK",
                "FiraCode Nerd + Noto CJK",
            ),
            "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono" => (
                "FiraCode Nerd Font, Noto Sans CJK JP, Ubuntu Mono",
                "FiraCode Nerd Font, Noto Sans CJK KR, Ubuntu Mono",
                "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono",
                "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono",
                "FiraCode Nerd Font, Noto Sans CJK TC, Ubuntu Mono",
            ),
            "Ubuntu / DejaVu (Linux Classic)" => (
                "Ubuntu / DejaVu (Linux 定番)",
                "Ubuntu / DejaVu (Linux 기본)",
                "Ubuntu / DejaVu (Clásico de Linux)",
                "Ubuntu / DejaVu (Classique Linux)",
                "Ubuntu / DejaVu (Linux-Klassiker)",
            ),
            "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC" => (
                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK JP",
                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK KR",
                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC",
                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC",
                "Ubuntu Mono, DejaVu Sans Mono, Noto Sans CJK TC",
            ),
            "NotoSansM Nerd Font + TC (Recommended 🚀)" => (
                "NotoSansM Nerd Font + CJK (推奨 🚀)",
                "NotoSansM Nerd Font + CJK (추천 🚀)",
                "NotoSansM Nerd Font + TC (Recomendado 🚀)",
                "NotoSansM Nerd Font + TC (Recommandé 🚀)",
                "NotoSansM Nerd Font + TC (Empfohlen 🚀)",
            ),
            "NotoSansM Nerd Font + SC" => (
                "NotoSansM Nerd Font + SC",
                "NotoSansM Nerd Font + SC",
                "NotoSansM Nerd Font + SC",
                "NotoSansM Nerd Font + SC",
                "NotoSansM Nerd Font + SC",
            ),
            "JetBrainsMono Nerd Font, PingFang TC" => (
                "JetBrainsMono Nerd Font, Hiragino Sans, PingFang TC",
                "JetBrainsMono Nerd Font, Apple SD Gothic Neo, PingFang TC",
                "JetBrainsMono Nerd Font, PingFang TC",
                "JetBrainsMono Nerd Font, PingFang TC",
                "JetBrainsMono Nerd Font, PingFang TC",
            ),
            "FiraCode Nerd Font, PingFang TC" => (
                "FiraCode Nerd Font, Hiragino Sans, PingFang TC",
                "FiraCode Nerd Font, Apple SD Gothic Neo, PingFang TC",
                "FiraCode Nerd Font, PingFang TC",
                "FiraCode Nerd Font, PingFang TC",
                "FiraCode Nerd Font, PingFang TC",
            ),
            "SF Mono + PingFang" => (
                "SF Mono + ヒラギノ角ゴ",
                "SF Mono + Apple SD 산돌고딕 Neo",
                "SF Mono + PingFang",
                "SF Mono + PingFang",
                "SF Mono + PingFang",
            ),
            "SF Mono, PingFang TC" => (
                "SF Mono, Hiragino Sans, PingFang TC",
                "SF Mono, Apple SD Gothic Neo, PingFang TC",
                "SF Mono, PingFang TC",
                "SF Mono, PingFang TC",
                "SF Mono, PingFang TC",
            ),
            "Menlo (macOS Default)" => (
                "Menlo (macOS デフォルト)",
                "Menlo (macOS 기본값)",
                "Menlo (Predeterminado de macOS)",
                "Menlo (Par défaut sur macOS)",
                "Menlo (macOS-Standard)",
            ),
            "Menlo, PingFang TC" => (
                "Menlo, Hiragino Sans, PingFang TC",
                "Menlo, Apple SD Gothic Neo, PingFang TC",
                "Menlo, PingFang TC",
                "Menlo, PingFang TC",
                "Menlo, PingFang TC",
            ),
            "Cascadia Mono + JhengHei (Recommended 🚀)" => (
                "Cascadia Mono + JhengHei (推奨 🚀)",
                "Cascadia Mono + JhengHei (추천 🚀)",
                "Cascadia Mono + JhengHei (Recomendado 🚀)",
                "Cascadia Mono + JhengHei (Recommandé 🚀)",
                "Cascadia Mono + JhengHei (Empfohlen 🚀)",
            ),
            "Cascadia Mono + YaHei" => (
                "Cascadia Mono + YaHei",
                "Cascadia Mono + YaHei",
                "Cascadia Mono + YaHei",
                "Cascadia Mono + YaHei",
                "Cascadia Mono + YaHei",
            ),
            "JetBrainsMono Nerd Font, Microsoft JhengHei UI" => (
                "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
                "JetBrainsMono Nerd Font, Malgun Gothic, Microsoft JhengHei UI",
                "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
                "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
                "JetBrainsMono Nerd Font, Microsoft JhengHei UI",
            ),
            "Consolas (Windows Default)" => (
                "Consolas (Windows デフォルト)",
                "Consolas (Windows 기본값)",
                "Consolas (Predeterminado de Windows)",
                "Consolas (Par défaut sur Windows)",
                "Consolas (Windows-Standard)",
            ),
            "Consolas, Microsoft JhengHei UI" => (
                "Consolas, Microsoft JhengHei UI",
                "Consolas, Malgun Gothic, Microsoft JhengHei UI",
                "Consolas, Microsoft JhengHei UI",
                "Consolas, Microsoft JhengHei UI",
                "Consolas, Microsoft JhengHei UI",
            ),
            "▲ Hide Manual Input" => (
                "▲ 手動編集を隠す",
                "▲ 직접 입력 숨기기",
                "▲ Ocultar edición manual",
                "▲ Masquer la saisie manuelle",
                "▲ Manuelle Eingabe ausblenden",
            ),
            "✎ Manual Edit" => (
                "✎ 手動で編集",
                "✎ 직접 편집",
                "✎ Edición manual",
                "✎ Édition manuelle",
                "✎ Manuell bearbeiten",
            ),
            "Clear" => ("クリア", "지우기", "Limpiar", "Effacer", "Löschen"),
            "Clear custom fonts and use system default" => (
                "カスタムフォントを消去し、システム標準に戻す",
                "사용자 지정 글꼴을 지우고 시스템 기본값 사용",
                "Borrar fuentes personalizadas y usar la predeterminada del sistema",
                "Effacer les polices personnalisées et utiliser la police système",
                "Benutzerdefinierte Schriften löschen und Systemstandard verwenden",
            ),
            "Comma-separated, e.g.: JetBrainsMono Nerd Font, Noto Sans CJK TC" => (
                "カンマ区切り、例: JetBrainsMono Nerd Font, Noto Sans CJK JP",
                "쉼표로 구분, 예: JetBrainsMono Nerd Font, Noto Sans CJK KR",
                "Separado por comas, ej.: JetBrainsMono Nerd Font, Noto Sans CJK TC",
                "Séparées par des virgules, ex. : JetBrainsMono Nerd Font, Noto Sans CJK TC",
                "Kommagetrennt, z. B.: JetBrainsMono Nerd Font, Noto Sans CJK TC",
            ),
            "✔ Apply" => ("✔ 適用", "✔ 적용", "✔ Aplicar", "✔ Appliquer", "✔ Anwenden"),
            "Font Size" => (
                "フォントサイズ (Font Size)",
                "글꼴 크기 (Font Size)",
                "Tamaño de fuente",
                "Taille de la police",
                "Schriftgröße",
            ),
            "Terminal text size (pt)" => (
                "ターミナルの文字サイズ (pt)",
                "터미널 텍스트 크기 (pt)",
                "Tamaño del texto de la terminal (pt)",
                "Taille du texte du terminal (pt)",
                "Textgröße im Terminal (pt)",
            ),
            "Line Height" => (
                "行の高さ (Line Height)",
                "줄 높이 (Line Height)",
                "Altura de línea",
                "Hauteur de ligne",
                "Zeilenhöhe",
            ),
            "1.0 is compact, 1.2 is comfortable (applied live)" => (
                "1.0 はコンパクト、1.2 は標準（即時反映）",
                "1.0은 좁게, 1.2는 표준 간격 (즉시 적용)",
                "1.0 es compacto, 1.2 es cómodo (se aplica en vivo)",
                "1.0 est compact, 1.2 est confortable (appliqué en direct)",
                "1.0 ist kompakt, 1.2 ist komfortabel (sofort wirksam)",
            ),
            "Window & Layout" => (
                "ウインドウとレイアウト (Window & Layout)",
                "창 및 레이아웃 (Window & Layout)",
                "Ventana y diseño",
                "Fenêtre et disposition",
                "Fenster & Layout",
            ),
            "Default Terminal Size (Columns × Rows)" => (
                "初期ターミナルサイズ (列 × 行)",
                "기본 터미널 크기 (열 × 행)",
                "Tamaño inicial de la terminal (Columnas × Filas)",
                "Taille initiale du terminal (Colonnes × Lignes)",
                "Standard-Terminalgröße (Spalten × Zeilen)",
            ),
            "Initial grid columns and rows on startup" => (
                "起動時の初期グリッドサイズ（列数 × 行数）",
                "시작 시 초기 그리드 열 및 행 수",
                "Columnas y filas iniciales al iniciar",
                "Nombre initial de colonnes et de lignes au démarrage",
                "Anfängliche Spalten- und Zeilenanzahl beim Start",
            ),
            "Cols: " => ("列: ", "열: ", "Cols.: ", "Col. : ", "Spalten: "),
            "Rows: " => ("行: ", "행: ", "Filas: ", "Lig. : ", "Zeilen: "),
            "Padding X / Y" => (
                "水平・垂直パディング (Padding X / Y)",
                "가로 / 세로 여백 (Padding X / Y)",
                "Relleno X / Y (Padding)",
                "Marge interne X / Y (Padding)",
                "Innenabstand X / Y (Padding)",
            ),
            "Inner margin around terminal cells in pixels" => (
                "ターミナル画面周囲の余白ピクセル数",
                "터미널 텍스트 주위의 내부 여백 픽셀",
                "Margen interior alrededor del texto en píxeles",
                "Marge intérieure autour des cellules en pixels",
                "Innerer Rand um die Terminalzellen in Pixeln",
            ),
            "X: " => ("左右 X: ", "가로 X: ", "X: ", "X : ", "X: "),
            "Y: " => ("上下 Y: ", "세로 Y: ", "Y: ", "Y : ", "Y: "),
            "Background Opacity" => (
                "背景の不透明度 (Opacity)",
                "배경 불투명도 (Opacity)",
                "Opacidad del fondo",
                "Opacité de l’arrière-plan",
                "Hintergrund-Deckkraft",
            ),
            "1.0 is fully opaque; lower values enable window transparency" => (
                "1.0 で完全不透明。下げると背景が透過します",
                "1.0은 완전 불투명하며, 낮추면 창 투명 효과가 적용됩니다",
                "1.0 es totalmente opaco; valores menores activan la transparencia",
                "1.0 est totalement opaque ; des valeurs plus basses activent la transparence",
                "1.0 ist vollständig deckend; kleinere Werte aktivieren Transparenz",
            ),
            "Window Decorations" => (
                "ウインドウ枠のスタイル (Decorations)",
                "창 테두리 스타일 (Decorations)",
                "Decoración de la ventana",
                "Décorations de la fenêtre",
                "Fensterdekoration",
            ),
            "Control native title bar and window frame style" => (
                "システムタイトルバーとウインドウ枠の表示方法",
                "시스템 제목 표시줄 및 창 테두리 표시 방식 제어",
                "Controla la barra de título nativa y el marco de la ventana",
                "Contrôler la barre de titre native et le style de cadre",
                "Titelleiste und Fensterrahmen-Stil steuern",
            ),
            "Full" => ("標準枠", "전체 표시", "Completo", "Complète", "Voll"),
            "Transparent" => (
                "透明統合",
                "투명 통합",
                "Transparente",
                "Transparente",
                "Transparent",
            ),
            "Startup Mode" => (
                "起動モード (Startup Mode)",
                "시작 모드 (Startup Mode)",
                "Modo de inicio",
                "Mode de démarrage",
                "Startmodus",
            ),
            "Initial window state when launching the application" => (
                "アプリケーション起動時のウインドウ状態",
                "애플리케이션 시작 시 초기 창 상태",
                "Estado inicial de la ventana al abrir la aplicación",
                "État initial de la fenêtre au lancement de l’application",
                "Anfänglicher Fensterzustand beim Starten der Anwendung",
            ),
            "Normal" => ("標準", "일반 창", "Normal", "Normal", "Normal"),
            "Maximized" => ("最大化", "최대화", "Maximizado", "Maximisé", "Maximiert"),
            "Fullscreen" => (
                "フルスクリーン",
                "전체 화면",
                "Pantalla completa",
                "Plein écran",
                "Vollbild",
            ),
            "Tab Bar Visibility" => (
                "タブバーの表示ポリシー (Tab Bar)",
                "탭 막대 표시 정책 (Tab Bar)",
                "Visibilidad de la barra de pestañas",
                "Affichage de la barre d’onglets",
                "Tab-Leiste anzeigen",
            ),
            "When hidden, hovering top-right still reveals the [AI] and [⚙] quick buttons" => (
                "非表示時も右上にカーソルを合わせると [AI] と [⚙] ボタンが表示されます",
                "숨겨진 상태에서도 오른쪽 상단에 마우스를 올리면 [AI] 및 [⚙] 빠른 버튼이 표시됩니다",
                "Al ocultarse, pasar el cursor arriba a la derecha muestra los botones [AI] y [⚙]",
                "Même masquée, survoler le coin supérieur droit révèle les boutons [AI] et [⚙]",
                "Wenn ausgeblendet, erscheinen oben rechts beim Überfahren weiterhin [AI] und [⚙]",
            ),
            "Always" => ("常に表示", "항상 표시", "Siempre", "Toujours", "Immer"),
            "Auto-hide" => (
                "1タブ時は隠す",
                "단일 탭 자동 숨김",
                "Ocultar auto.",
                "Auto",
                "Automatisch",
            ),
            "Never" => ("非表示", "표시 안 함", "Nunca", "Jamais", "Nie"),
            "Auto-Check for Updates on Startup" => (
                "起動時にアップデートを自動確認",
                "시작 시 자동으로 업데이트 확인",
                "Buscar actualizaciones automáticamente al iniciar",
                "Vérifier automatiquement les mises à jour au démarrage",
                "Beim Start automatisch nach Updates suchen",
            ),
            "Check GitHub Releases in the background on startup for OTA updates" => (
                "起動時にバックグラウンドで GitHub Releases の新バージョンを確認します",
                "시작 시 백그라운드에서 GitHub Releases의 새 OTA 업데이트를 확인합니다",
                "Comprobar GitHub Releases en segundo plano al iniciar para actualizaciones OTA",
                "Vérifier les nouvelles versions GitHub Releases en arrière-plan au démarrage",
                "Beim Start im Hintergrund auf GitHub Releases nach OTA-Updates suchen",
            ),
            "Preset Themes" => (
                "プリセットテーマ (Preset Themes)",
                "프리셋 테마 (Preset Themes)",
                "Temas predefinidos",
                "Thèmes prédéfinis",
                "Vordefinierte Themen",
            ),
            "Theme Gallery" => (
                "テーマギャラリー (Theme Gallery)",
                "테마 갤러리 (Theme Gallery)",
                "Galería de temas",
                "Galerie de thèmes",
                "Themen-Galerie",
            ),
            "Click a card to apply its colors and ANSI 16-color palette (resets custom overrides)" => (
                "カードをクリックするとテーマ配色と ANSI 16 色パレットを即座に適用します（カスタム色はリセット）",
                "카드를 클릭하면 테마 색상과 ANSI 16색 팔레트가 즉시 적용됩니다 (사용자 지정 색상 초기화)",
                "Haz clic en una tarjeta para aplicar sus colores y paleta ANSI de 16 colores",
                "Cliquez sur une carte pour appliquer ses couleurs et sa palette ANSI 16 couleurs",
                "Auf eine Karte klicken, um deren Farben und ANSI-16-Farbpalette anzuwenden",
            ),
            "Pure Black 🖤" => (
                "Pure Black 漆黒 🖤",
                "Pure Black 퓨어 블랙 🖤",
                "Pure Black Negro puro 🖤",
                "Pure Black Noir pur 🖤",
                "Pure Black Tiefschwarz 🖤",
            ),
            "Custom Color Overrides" => (
                "カスタム配色の上書き (Custom Overrides)",
                "사용자 지정 색상 덮어쓰기 (Custom Overrides)",
                "Colores personalizados",
                "Couleurs personnalisées",
                "Benutzerdefinierte Farben",
            ),
            "Background" => (
                "背景色 (Background)",
                "배경색 (Background)",
                "Fondo (Background)",
                "Arrière-plan (Background)",
                "Hintergrund (Background)",
            ),
            "Leave empty to inherit theme background" => (
                "空欄の場合はテーマのデフォルト背景色を継承します",
                "비워 두면 테마의 기본 배경색을 상속합니다",
                "Dejar vacío para heredar el color de fondo del tema",
                "Laisser vide pour hériter de l’arrière-plan du thème",
                "Leer lassen, um die Hintergrundfarbe des Themas zu übernehmen",
            ),
            "OLED Black" => (
                "OLED 純黒",
                "OLED 순수 검정",
                "Negro OLED",
                "Noir OLED",
                "OLED-Schwarz",
            ),
            "Ink Black" => (
                "墨黒",
                "먹색 검정",
                "Negro tinta",
                "Noir d’encre",
                "Tintenschwarz",
            ),
            "Midnight" => (
                "ミッドナイト",
                "미드나잇",
                "Medianoche",
                "Minuit",
                "Mitternacht",
            ),
            "Foreground" => (
                "文字色 (Foreground)",
                "전경 텍스트 색상 (Foreground)",
                "Texto (Foreground)",
                "Premier plan (Foreground)",
                "Vordergrund (Foreground)",
            ),
            "Leave empty to inherit theme foreground" => (
                "空欄の場合はテーマのデフォルト文字色を継承します",
                "비워 두면 테마의 기본 텍스트 색상을 상속합니다",
                "Dejar vacío para heredar el color de texto del tema",
                "Laisser vide pour hériter de la couleur de texte du thème",
                "Leer lassen, um die Textfarbe des Themas zu übernehmen",
            ),
            "Bright White" => (
                "ブライトホワイト",
                "밝은 흰색",
                "Blanco brillante",
                "Blanc vif",
                "Hellweiß",
            ),
            "Matrix Green" => (
                "マトリックスグリーン",
                "매트릭스 그린",
                "Verde Matrix",
                "Vert Matrix",
                "Matrix-Grün",
            ),
            "Amber" => ("アンバー", "앰버 (호박색)", "Ámbar", "Ambre", "Bernstein"),
            "Cursor Color" => (
                "カーソル色 (Cursor)",
                "커서 색상 (Cursor)",
                "Color del cursor",
                "Couleur du curseur",
                "Cursorfarbe",
            ),
            "Leave empty to inherit theme cursor color" => (
                "空欄の場合はテーマのデフォルトカーソル色を継承します",
                "비워 두면 테마의 기본 커서 색상을 상속합니다",
                "Dejar vacío para heredar el color de cursor del tema",
                "Laisser vide pour hériter de la couleur du curseur du thème",
                "Leer lassen, um die Cursorfarbe des Themas zu übernehmen",
            ),
            "macOS Blue" => (
                "macOS ブルー",
                "macOS 블루",
                "Azul macOS",
                "Bleu macOS",
                "macOS-Blau",
            ),
            "Green" => ("グリーン", "초록", "Verde", "Vert", "Grün"),
            "Red" => ("レッド", "빨강", "Rojo", "Rouge", "Rot"),
            "Selection Background" => (
                "選択範囲の背景色 (Selection)",
                "선택 영역 배경색 (Selection)",
                "Fondo de selección",
                "Fond de sélection",
                "Auswahl-Hintergrund",
            ),
            "Highlight background color for selected text" => (
                "マウスで選択したテキスト範囲のハイライト背景色",
                "마우스로 선택한 텍스트의 강조 배경색",
                "Color de fondo al resaltar texto seleccionado",
                "Couleur de surbrillance du texte sélectionné",
                "Hervorhebungsfarbe für ausgewählten Text",
            ),
            "Classic Blue" => (
                "クラシックブルー",
                "클래식 블루",
                "Azul clásico",
                "Bleu classique",
                "Klassisches Blau",
            ),
            "Soft Gray" => (
                "ソフトグレー",
                "소프트 그레이",
                "Gris suave",
                "Gris doux",
                "Sanftes Grau",
            ),
            "ANSI 16-Color Palette" => (
                "詳細 ANSI 16 色パレット (ANSI 16-Color Palette)",
                "고급 ANSI 16색 팔레트 (ANSI 16-Color Palette)",
                "Paleta ANSI de 16 colores",
                "Palette ANSI 16 couleurs",
                "ANSI-16-Farbpalette",
            ),
            "Override Individual ANSI 16 Colors (color0 ~ color15)" => (
                "ANSI 16 色の個別カスタマイズ (color0 ~ color15)",
                "개별 ANSI 16색 덮어쓰기 (color0 ~ color15)",
                "Personalizar los 16 colores ANSI individuales (color0 ~ color15)",
                "Personnaliser individuellement les 16 couleurs ANSI (color0 ~ color15)",
                "Einzelne ANSI-16-Farben überschreiben (color0 ~ color15)",
            ),
            "Fine-tune normal and bright ANSI colors; leave empty to use theme defaults." => (
                "標準 8 色と高輝度 8 色を微調整できます。空欄でテーマのデフォルト値を使用します。",
                "표준 8색과 밝은 8색을 미세 조정합니다. 비워 두면 테마 기본값을 사용합니다.",
                "Ajusta los colores ANSI normales y brillantes; déjalo vacío para usar el tema.",
                "Ajustez les couleurs ANSI normales et vives ; laissez vide pour les valeurs du thème.",
                "Normale und helle ANSI-Farben anpassen; leer lassen für Themen-Standard.",
            ),
            "▼ Hide ANSI 16-Color Table" => (
                "▼ ANSI 16 色テーブルを隠す",
                "▼ ANSI 16색 설정표 접기",
                "▼ Ocultar tabla ANSI de 16 colores",
                "▼ Masquer la table ANSI 16 couleurs",
                "▼ ANSI-16-Farbtabelle ausblenden",
            ),
            "▶ Show ANSI 16-Color Table (0~15)" => (
                "▶ ANSI 16 色テーブルを展開 (0~15)",
                "▶ ANSI 16색 설정표 펼치기 (0~15)",
                "▶ Mostrar tabla ANSI de 16 colores (0~15)",
                "▶ Afficher la table ANSI 16 couleurs (0~15)",
                "▶ ANSI-16-Farbtabelle anzeigen (0~15)",
            ),
            "🔄 Reset All to Theme Default" => (
                "🔄 すべてテーマのデフォルトに戻す",
                "🔄 모두 테마 기본값으로 재설정",
                "🔄 Restablecer todo al tema",
                "🔄 Tout réinitialiser au thème",
                "🔄 Alle auf Themen-Standard zurücksetzen",
            ),
            "Clear all color0 ~ color15 overrides" => (
                "color0 〜 color15 のすべてのカスタム設定をクリア",
                "모든 color0 ~ color15 사용자 지정 색상 지우기",
                "Borrar todas las personalizaciones de color0 ~ color15",
                "Effacer toutes les couleurs personnalisées color0 ~ color15",
                "Alle Überschreibungen für color0 ~ color15 löschen",
            ),
            "0 Black" => ("0 黒 (Black)", "0 검정 (Black)", "0 Negro", "0 Noir", "0 Schwarz"),
            "1 Red" => ("1 赤 (Red)", "1 빨강 (Red)", "1 Rojo", "1 Rouge", "1 Rot"),
            "2 Green" => ("2 緑 (Green)", "2 초록 (Green)", "2 Verde", "2 Vert", "2 Grün"),
            "3 Yellow" => ("3 黄 (Yellow)", "3 노랑 (Yellow)", "3 Amarillo", "3 Jaune", "3 Gelb"),
            "4 Blue" => ("4 青 (Blue)", "4 파랑 (Blue)", "4 Azul", "4 Bleu", "4 Blau"),
            "5 Magenta" => (
                "5 マゼンタ (Magenta)",
                "5 마젠타 (Magenta)",
                "5 Magenta",
                "5 Magenta",
                "5 Magenta",
            ),
            "6 Cyan" => ("6 シアン (Cyan)", "6 시안 (Cyan)", "6 Cian", "6 Cyan", "6 Cyan"),
            "7 White" => ("7 白 (White)", "7 흰색 (White)", "7 Blanco", "7 Blanc", "7 Weiß"),
            "8 BrBlack" => (
                "8 明るい黒 (BrBlack)",
                "8 밝은 검정 (BrBlack)",
                "8 Negro br.",
                "8 Noir vif",
                "8 Hellschwarz",
            ),
            "9 BrRed" => (
                "9 明るい赤 (BrRed)",
                "9 밝은 빨강 (BrRed)",
                "9 Rojo br.",
                "9 Rouge vif",
                "9 Hellrot",
            ),
            "10 BrGreen" => (
                "10 明るい緑 (BrGreen)",
                "10 밝은 초록 (BrGreen)",
                "10 Verde br.",
                "10 Vert vif",
                "10 Hellgrün",
            ),
            "11 BrYellow" => (
                "11 明るい黄 (BrYellow)",
                "11 밝은 노랑 (BrYellow)",
                "11 Amarillo br.",
                "11 Jaune vif",
                "11 Hellgelb",
            ),
            "12 BrBlue" => (
                "12 明るい青 (BrBlue)",
                "12 밝은 파랑 (BrBlue)",
                "12 Azul br.",
                "12 Bleu vif",
                "12 Hellblau",
            ),
            "13 BrMag" => (
                "13 明るいマゼンタ (BrMag)",
                "13 밝은 마젠타 (BrMag)",
                "13 Magenta br.",
                "13 Magenta vif",
                "13 Hellmagenta",
            ),
            "14 BrCyan" => (
                "14 明るいシアン (BrCyan)",
                "14 밝은 시안 (BrCyan)",
                "14 Cian br.",
                "14 Cyan vif",
                "14 Hellcyan",
            ),
            "15 BrWhite" => (
                "15 明るい白 (BrWhite)",
                "15 밝은 흰색 (BrWhite)",
                "15 Blanco br.",
                "15 Blanc vif",
                "15 Hellweiß",
            ),
            "Reset to theme default" => (
                "テーマのデフォルトに戻す",
                "테마 기본값으로 재설정",
                "Restablecer al color del tema",
                "Rétablir la couleur du thème",
                "Auf Themen-Standard zurücksetzen",
            ),
            "Shell & Environment" => (
                "シェルと起動環境 (Shell & Environment)",
                "셸 및 시작 환경 (Shell & Environment)",
                "Shell y entorno",
                "Shell et environnement",
                "Shell & Umgebung",
            ),
            "TERM Variable" => (
                "TERM 環境変数",
                "TERM 환경 변수",
                "Variable TERM",
                "Variable TERM",
                "TERM-Variable",
            ),
            "Default value for $TERM environment variable" => (
                "環境変数 $TERM のデフォルト値",
                "$TERM 환경 변수의 기본값",
                "Valor predeterminado para la variable de entorno $TERM",
                "Valeur par défaut de la variable d’environnement $TERM",
                "Standardwert für die Umgebungsvariable $TERM",
            ),
            "System Default" => (
                "システム標準",
                "시스템 기본값",
                "Predeterminado del sistema",
                "Par défaut du système",
                "Systemstandard",
            ),
            "Custom" => ("カスタム", "사용자 지정", "Personalizado", "Personnalisé", "Benutzerdefiniert"),
            "Switch between Command Prompt, Windows PowerShell, or PowerShell 7" => (
                "コマンド プロンプト、Windows PowerShell、PowerShell 7 を切り替え",
                "명령 프롬프트, Windows PowerShell 또는 PowerShell 7 간 전환",
                "Cambiar entre Símbolo del sistema, Windows PowerShell o PowerShell 7",
                "Basculer entre Invite de commandes, Windows PowerShell ou PowerShell 7",
                "Zwischen Eingabeaufforderung, Windows PowerShell und PowerShell 7 wechseln",
            ),
            "Automatically detects installed system shells (Bash, Zsh, Fish, etc.)" => (
                "インストール済みのシステムシェル（Bash、Zsh、Fish など）を自動検出します",
                "시스템에 설치된 셸(Bash, Zsh, Fish 등)을 자동으로 감지합니다",
                "Detecta automáticamente los shells instalados (Bash, Zsh, Fish, etc.)",
                "Détecte automatiquement les shells installés (Bash, Zsh, Fish, etc.)",
                "Erkennt automatisch installierte System-Shells (Bash, Zsh, Fish usw.)",
            ),
            "Default Shell" => (
                "デフォルトシェル",
                "기본 셸",
                "Shell predeterminado",
                "Shell par défaut",
                "Standard-Shell",
            ),
            "System Default ($SHELL)" => (
                "システム標準 ($SHELL)",
                "시스템 기본값 ($SHELL)",
                "Predeterminado del sistema ($SHELL)",
                "Système par défaut ($SHELL)",
                "Systemstandard ($SHELL)",
            ),
            "Not Installed" => (
                "未インストール",
                "설치되지 않음",
                "No instalado",
                "Non installé",
                "Nicht installiert",
            ),
            "Custom Shell Path (Custom...)" => (
                "カスタムシェルパス (Custom...)",
                "사용자 지정 셸 경로 (Custom...)",
                "Ruta de shell personalizada (Custom...)",
                "Chemin de shell personnalisé (Custom...)",
                "Benutzerdefinierter Shell-Pfad (Custom...)",
            ),
            "Quick Shell Switch" => (
                "シェルのクイック切り替え",
                "빠른 셸 전환",
                "Cambio rápido de shell",
                "Changement rapide de shell",
                "Schneller Shell-Wechsel",
            ),
            "One-click switch for the default shell of new tabs" => (
                "ワンクリックで新規タブのデフォルトシェルを切り替えます",
                "버튼 클릭 한 번으로 새 탭의 기본 셸을 전환합니다",
                "Cambia con un clic el shell predeterminado para nuevas pestañas",
                "Changez en un clic le shell par défaut des nouveaux onglets",
                "Standard-Shell für neue Tabs mit einem Klick wechseln",
            ),
            "🌟 System Default" => (
                "🌟 システム標準",
                "🌟 시스템 기본값",
                "🌟 Sistema",
                "🌟 Système",
                "🌟 Systemstandard",
            ),
            "Custom Executable Path & Arguments" => (
                "実行ファイルパスと引数を手動指定",
                "실행 파일 경로 및 인수 직접 지정",
                "Ruta de ejecutable y argumentos personalizados",
                "Chemin d’exécutable et arguments personnalisés",
                "Benutzerdefinierter Programm-Pfad & Argumente",
            ),
            "Enable when custom startup flags are needed" => (
                "特別な起動フラグを渡す必要がある場合に有効化します",
                "특별한 시작 플래그를 전달해야 할 때 활성화합니다",
                "Actívalo cuando necesites indicadores de inicio personalizados",
                "Activer lorsque des options de lancement spécifiques sont nécessaires",
                "Aktivieren, wenn spezielle Startparameter benötigt werden",
            ),
            "/bin/zsh or /usr/bin/fish" => (
                "/bin/zsh または /usr/bin/fish",
                "/bin/zsh 또는 /usr/bin/fish",
                "/bin/zsh o /usr/bin/fish",
                "/bin/zsh ou /usr/bin/fish",
                "/bin/zsh oder /usr/bin/fish",
            ),
            "Program" => (
                "実行ファイルパス (Program)",
                "실행 파일 경로 (Program)",
                "Programa (Program)",
                "Programme (Program)",
                "Programm (Program)",
            ),
            "Arguments" => (
                "起動引数 (Arguments)",
                "시작 인수 (Arguments)",
                "Argumentos (Arguments)",
                "Arguments",
                "Argumente (Arguments)",
            ),
            "Working Directory" => (
                "初期作業ディレクトリ (Working Directory)",
                "초기 작업 디렉터리 (Working Directory)",
                "Directorio de trabajo",
                "Répertoire de travail",
                "Arbeitsverzeichnis",
            ),
            "Default directory on startup (leave empty for home ~/)" => (
                "起動時のデフォルトパス（空欄でホームディレクトリ ~/）",
                "시작 시 기본 경로 (비워 두면 홈 디렉터리 ~/ 사용)",
                "Directorio predeterminado al iniciar (vacío para usar ~/)",
                "Répertoire par défaut au démarrage (vide pour le dossier personnel ~/ )",
                "Standardverzeichnis beim Start (leer lassen für Home ~/)",
            ),
            "Default user home (~/)" => (
                "ユーザーのホームディレクトリ (~/)",
                "기본 사용자 홈 디렉터리 (~/)",
                "Carpeta personal del usuario (~/)",
                "Dossier personnel par défaut (~/)",
                "Standard-Benutzerordner (~/)",
            ),
            "Buffer & Interaction" => (
                "バッファと操作動作 (Buffer & Interaction)",
                "버퍼 및 상호작용 동작 (Buffer & Interaction)",
                "Búfer e interacción",
                "Tampon et interaction",
                "Puffer & Interaktion",
            ),
            "Right-Click Action" => (
                "右クリックの動作 (Right-Click Action)",
                "마우스 오른쪽 클릭 동작 (Right-Click Action)",
                "Acción de clic derecho",
                "Action du clic droit",
                "Rechtsklick-Aktion",
            ),
            "Hold Shift + Right-Click to perform the alternate action" => (
                "Shift + 右クリックでもう一方の動作を実行できます",
                "Shift + 오른쪽 클릭으로 다른 동작을 언제든 실행할 수 있습니다",
                "Mantén Shift + clic derecho para realizar la acción alternativa",
                "Maintenez Maj + clic droit pour effectuer l’action alternative",
                "Umschalt + Rechtsklick gedrückt halten, um die alternative Aktion auszuführen",
            ),
            "Context Menu" => (
                "コンテキストメニュー (標準)",
                "컨텍스트 메뉴 열기 (기본값)",
                "Menú contextual",
                "Menu contextuel",
                "Kontextmenü",
            ),
            "Paste Clipboard" => (
                "クリップボードを貼り付け",
                "클립보드 붙여넣기",
                "Pegar portapapeles",
                "Coller le presse-papiers",
                "Zwischenablage einfügen",
            ),
            "Scrollback Lines" => (
                "スクロールバック行数 (Scrollback)",
                "스크롤백 줄 수 (Scrollback)",
                "Líneas de historial (Scrollback)",
                "Lignes d’historique (Scrollback)",
                "Scrollback-Zeilen",
            ),
            "Number of scrollback history lines retained" => (
                "上スクロール用に保持する履歴行数",
                "위로 스크롤할 때 유지되는 기록 줄 수",
                "Número de líneas de historial conservadas al desplazar",
                "Nombre de lignes conservées dans l’historique de défilement",
                "Anzahl der gespeicherten Zeilen im Scrollback-Verlauf",
            ),
            " lines" => (" 行", " 줄", " líneas", " lignes", " Zeilen"),
            "Alt / Option as Meta" => (
                "Alt / Option キーの Meta モード",
                "Alt / Option 키 Meta 모드",
                "Tecla Alt / Option como Meta",
                "Touche Alt / Option comme Meta",
                "Alt / Option als Meta-Taste",
            ),
            "Control whether Alt / Option keys send terminal Meta (Esc+) sequences" => (
                "Alt / Option キーがターミナルの Meta (Esc+) シーケンスを送信するか制御します",
                "Alt / Option 키가 터미널 Meta (Esc+) 시퀀스를 보낼지 제어합니다",
                "Controla si las teclas Alt / Option envían secuencias Meta (Esc+)",
                "Contrôle si les touches Alt / Option envoient des séquences Meta (Esc+)",
                "Steuert, ob Alt / Option-Tasten Terminal-Meta-Sequenzen (Esc+) senden",
            ),
            "Left Only" => (
                "左のみ",
                "왼쪽만",
                "Solo izquierda",
                "Gauche seule",
                "Nur links",
            ),
            "Right Only" => (
                "右のみ",
                "오른쪽만",
                "Solo derecha",
                "Droite seule",
                "Nur rechts",
            ),
            "Terminal Bell" => (
                "ターミナルベル (Bell)",
                "터미널 벨 모드 (Bell)",
                "Campana de la terminal (Bell)",
                "Cloche du terminal (Bell)",
                "Terminal-Glocke (Bell)",
            ),
            "Feedback style when BEL character is received" => (
                "BEL 制御文字を受信した際の通知方法",
                "BEL 경고 신호를 받았을 때의 피드백 방식",
                "Tipo de aviso al recibir el carácter BEL",
                "Type de retour lors de la réception du caractère BEL",
                "Rückmeldung beim Empfang des BEL-Zeichens",
            ),
            "Visual" => ("視覚点滅", "시각적 깜빡임", "Visual", "Visuel", "Visuell"),
            "Sound" => ("システム音", "시스템 소리", "Sonido", "Son", "Ton"),
            "Muted" => ("ミュート", "무음", "Silenciado", "Muet", "Stumm"),
            "Cursor Style & Animation" => (
                "カーソル形状とアニメーション (Cursor Style)",
                "커서 스타일 및 애니메이션 (Cursor Style)",
                "Estilo y animación del cursor",
                "Style et animation du curseur",
                "Cursor-Stil & Animation",
            ),
            "Cursor Shape" => (
                "カーソル形状 (Shape)",
                "커서 모양 (Shape)",
                "Forma del cursor",
                "Forme du curseur",
                "Cursorform",
            ),
            "Terminal cursor geometry (previewed live above)" => (
                "ターミナルカーソルの形状（上部プレビューに即時反映）",
                "터미널 커서 모양 (위 미리보기에서 실시간 확인 가능)",
                "Geometría del cursor en la terminal (vista previa arriba)",
                "Géométrie du curseur du terminal (aperçu en direct ci-dessus)",
                "Geometrie des Terminal-Cursors (Live-Vorschau oben)",
            ),
            "Block █" => (
                "ブロック (Block █)",
                "블록 (Block █)",
                "Bloque █",
                "Bloc █",
                "Block █",
            ),
            "Beam ❘" => (
                "縦線 (Beam ❘)",
                "세로선 (Beam ❘)",
                "Barra ❘",
                "Barre ❘",
                "Strich ❘",
            ),
            "Underline _" => (
                "下線 (Underline _)",
                "밑줄 (Underline _)",
                "Subrayado _",
                "Souligné _",
                "Unterstrich _",
            ),
            "Cursor Blink" => (
                "カーソル点滅 (Blinking)",
                "커서 깜빡임 (Blinking)",
                "Parpadeo del cursor",
                "Clignotement du curseur",
                "Cursor-Blinken",
            ),
            "Enable smooth cursor blink animation" => (
                "カーソルの滑らかな点滅アニメーションを有効化",
                "부드러운 커서 깜빡임 애니메이션 활성화",
                "Activar animación suave de parpadeo del cursor",
                "Activer l’animation fluide de clignotement du curseur",
                "Sanfte Blink-Animation des Cursors aktivieren",
            ),
            "Blink Interval" => (
                "点滅間隔 (Interval)",
                "깜빡임 주기 (Interval)",
                "Intervalo de parpadeo",
                "Intervalle de clignotement",
                "Blink-Intervall",
            ),
            "Cursor blink interval in milliseconds" => (
                "カーソル点滅周期（ミリ秒）",
                "커서 깜빡임 주기 (밀리초)",
                "Intervalo de parpadeo del cursor en milisegundos",
                "Intervalle de clignotement du curseur en millisecondes",
                "Blink-Intervall des Cursors in Millisekunden",
            ),
            "🔍 Search shortcuts or actions (e.g. split, find, AI, preferences)..." => (
                "🔍 ショートカットや機能名を検索（例: 分割、検索、AI、設定）...",
                "🔍 단축키 또는 기능 검색 (예: 분할, 찾기, AI, 설정)...",
                "🔍 Buscar atajos o acciones (ej.: dividir, buscar, IA, ajustes)...",
                "🔍 Rechercher des raccourcis ou actions (ex. : scinder, rechercher, IA)...",
                "🔍 Kurzbefehle oder Aktionen suchen (z. B. teilen, suchen, KI)...",
            ),
            "Current OS" => (
                "現在の OS",
                "현재 OS",
                "SO actual",
                "OS actuel",
                "Aktuelles OS",
            ),
            "Window & Tabs" => (
                "ウインドウとタブ管理 (Window & Tabs)",
                "창 및 탭 관리 (Window & Tabs)",
                "Ventanas y pestañas",
                "Fenêtres et onglets",
                "Fenster & Tabs",
            ),
            "Open Preferences Window" => (
                "設定ウインドウを開く",
                "설정 창 열기",
                "Abrir ventana de ajustes",
                "Ouvrir la fenêtre des réglages",
                "Einstellungsfenster öffnen",
            ),
            "Also accessible via the top-right ⚙ button or context menu" => (
                "右上の ⚙ ボタンや右クリックメニューからも開けます",
                "오른쪽 상단 ⚙ 버튼이나 우클릭 메뉴에서도 열 수 있습니다",
                "También accesible desde el botón ⚙ superior derecho o el menú contextual",
                "Également accessible via le bouton ⚙ en haut à droite ou le menu contextuel",
                "Auch über die Schaltfläche ⚙ oben rechts oder das Kontextmenü erreichbar",
            ),
            "New Terminal Window" => (
                "新規ターミナルウインドウ",
                "새 터미널 창",
                "Nueva ventana de terminal",
                "Nouvelle fenêtre de terminal",
                "Neues Terminalfenster",
            ),
            "New Tab" => (
                "新規タブ (New Tab)",
                "새 탭 (New Tab)",
                "Nueva pestaña",
                "Nouvel onglet",
                "Neuer Tab",
            ),
            "Also accessible via the tab bar ＋ button" => (
                "タブバーの ＋ ボタンからも作成できます",
                "탭 막대의 ＋ 버튼으로도 추가할 수 있습니다",
                "También accesible mediante el botón ＋ de la barra de pestañas",
                "Également accessible via le bouton ＋ de la barre d’onglets",
                "Auch über die Schaltfläche ＋ in der Tab-Leiste erreichbar",
            ),
            "Close Active Pane or Tab" => (
                "現在のペインまたはタブを閉じる",
                "현재 패널 또는 탭 닫기",
                "Cerrar panel o pestaña activa",
                "Fermer le volet ou l’onglet actif",
                "Aktiven Bereich oder Tab schließen",
            ),
            "Next / Previous Tab" => (
                "次のタブ / 前のタブへ切り替え",
                "다음 / 이전 탭으로 전환",
                "Pestaña siguiente / anterior",
                "Onglet suivant / précédent",
                "Nächster / vorheriger Tab",
            ),
            "Switch to Tab 1~9" => (
                "指定のタブ 1〜9 へ直接切り替え",
                "지정한 1~9번 탭으로 바로 전환",
                "Cambiar a la pestaña 1~9",
                "Basculer vers l’onglet 1~9",
                "Zu Tab 1~9 wechseln",
            ),
            "Split Panes & Navigation" => (
                "ペイン分割とナビゲーション (Split Panes & Navigation)",
                "패널 분할 및 탐색 (Split Panes & Navigation)",
                "División de paneles y navegación",
                "Division des volets et navigation",
                "Geteilte Bereiche & Navigation",
            ),
            "Split Pane Vertically" => (
                "左右にペインを分割 (Split Vertical)",
                "좌우로 패널 분할 (Split Vertical)",
                "Dividir panel verticalmente",
                "Scinder le volet verticalement",
                "Bereich vertikal teilen",
            ),
            "Split Pane Horizontally" => (
                "上下にペインを分割 (Split Horizontal)",
                "상하로 패널 분할 (Split Horizontal)",
                "Dividir panel horizontalmente",
                "Scinder le volet horizontalement",
                "Bereich horizontal teilen",
            ),
            "Focus Next / Previous Pane" => (
                "前後のペインへフォーカス移動",
                "인접한 패널로 포커스 전환",
                "Enfocar panel siguiente / anterior",
                "Sélectionner le volet suivant / précédent",
                "Nächsten / vorherigen Bereich fokussieren",
            ),
            "Linux/Win uses Alt+[ and Alt+] to avoid intercepting Esc (Ctrl+[)" => (
                "Linux/Win では Esc (Ctrl+[) との競合を避けるため Alt+[ と Alt+] を使用します",
                "Linux/Win에서는 Esc(Ctrl+[) 가로채기를 피하기 위해 Alt+[ 및 Alt+]를 사용합니다",
                "Linux/Win usa Alt+[ y Alt+] para no interceptar Esc (Ctrl+[)",
                "Linux/Win utilise Alt+[ et Alt+] pour éviter d’intercepter Échap (Ctrl+[)",
                "Linux/Win nutzt Alt+[ und Alt+], um Esc (Ctrl+[) nicht abzufangen",
            ),
            "Zoom / Restore Active Pane" => (
                "現在のペインを最大化 / 復元 (Zoom Pane)",
                "현재 분할 패널 최대화 / 복원 (Zoom Pane)",
                "Ampliar / restaurar panel activo",
                "Agrandir / restaurer le volet actif",
                "Aktiven Bereich maximieren / wiederherstellen",
            ),
            "Resize Split Pane Ratio" => (
                "分割ペインの境界比率を調整",
                "분할 패널 경계 비율 미세 조정",
                "Ajustar proporción de paneles divididos",
                "Redimensionner les volets scindés",
                "Größenverhältnis geteilter Bereiche anpassen",
            ),
            "Arrow Keys" => (
                "矢印キー",
                "방향키",
                "Flechas",
                "Flèches",
                "Pfeiltasten",
            ),
            "Edit, Search & AI Assistant" => (
                "編集・検索・AI アシスタント (Edit, Search & AI)",
                "편집, 검색 및 AI 어시스턴트 (Edit, Search & AI)",
                "Edición, búsqueda y asistente IA",
                "Édition, recherche et assistant IA",
                "Bearbeiten, Suchen & KI-Assistent",
            ),
            "Copy Selection" => (
                "選択テキストをコピー (Copy)",
                "선택한 텍스트 복사 (Copy)",
                "Copiar selección",
                "Copier la sélection",
                "Auswahl kopieren",
            ),
            "Find in Terminal" => (
                "ターミナル内テキスト検索 (Find)",
                "터미널 내 텍스트 찾기 (Find)",
                "Buscar en la terminal",
                "Rechercher dans le terminal",
                "Im Terminal suchen",
            ),
            "Toggle AI Agents Sidebar" => (
                "AI エージェントサイドバーを切り替え",
                "AI 에이전트 사이드바 토글",
                "Mostrar/ocultar barra lateral de agentes IA",
                "Afficher/masquer la barre latérale des agents IA",
                "KI-Agenten-Seitenleiste ein-/ausblenden",
            ),
            "Also accessible via the top-right AI badge" => (
                "右上の AI バッジボタンからも切り替えられます",
                "오른쪽 상단 AI 배지 버튼으로도 토글할 수 있습니다",
                "También accesible desde el botón AI superior derecho",
                "Également accessible via le badge AI en haut à droite",
                "Auch über das AI-Badge oben rechts erreichbar",
            ),
            "Clear Screen & Scrollback" => (
                "画面とスクロールバックを消去 (Clear Scrollback)",
                "화면 및 스크롤백 지우기 (Clear Scrollback)",
                "Limpiar pantalla e historial",
                "Effacer l’écran et l’historique",
                "Bildschirm & Verlauf löschen",
            ),
            "Context Menu / Quick Paste" => (
                "右クリックメニュー / クイック貼り付け",
                "우클릭 컨텍스트 메뉴 / 빠른 붙여넣기",
                "Menú contextual / Pegado rápido",
                "Menu contextuel / Collage rapide",
                "Kontextmenü / Schnelles Einfügen",
            ),
            "Default right-click behavior is configurable under Terminal & Shell" => (
                "デフォルトの右クリック動作は「ターミナルとシェル」タブで変更できます",
                "기본 우클릭 동작은 '터미널 및 셸' 탭에서 설정할 수 있습니다",
                "El comportamiento predeterminado del clic derecho se configura en Terminal y Shell",
                "Le comportement par défaut du clic droit est configurable sous Terminal et Shell",
                "Das Standardverhalten für Rechtsklick ist unter Terminal & Shell einstellbar",
            ),
            "Right-Click" => (
                "右クリック",
                "우클릭",
                "Clic derecho",
                "Clic droit",
                "Rechtsklick",
            ),
            "Shift+Right-Click" => (
                "Shift+右クリック",
                "Shift+우클릭",
                "Shift+Clic derecho",
                "Maj+Clic droit",
                "Umschalt+Rechtsklick",
            ),
            "Zoom, Scroll & Signals" => (
                "フォント拡大縮小・スクロール・シグナル",
                "글꼴 확대/축소, 스크롤 및 시그널",
                "Zoom, desplazamiento y señales",
                "Zoom, défilement et signaux",
                "Zoom, Scrollen & Signale",
            ),
            "Zoom In" => (
                "文字を拡大 (Zoom In)",
                "글꼴 확대 (Zoom In)",
                "Ampliar fuente",
                "Agrandir la police",
                "Schrift vergrößern",
            ),
            "Zoom Out" => (
                "文字を縮小 (Zoom Out)",
                "글꼴 축소 (Zoom Out)",
                "Reducir fuente",
                "Réduire la police",
                "Schrift verkleinern",
            ),
            "Reset Font Size" => (
                "標準文字サイズに戻す (Reset Zoom)",
                "기본 글꼴 크기로 재설정 (Reset Zoom)",
                "Restablecer tamaño de fuente",
                "Réinitialiser la taille de police",
                "Schriftgröße zurücksetzen",
            ),
            "Scroll Page Up / Page Down" => (
                "履歴を 1 ページ上 / 下へスクロール",
                "기록 페이지 위로 / 아래로 스크롤",
                "Desplazar página arriba / abajo",
                "Défiler d’une page vers le haut / bas",
                "Seite nach oben / unten scrollen",
            ),
            "Send Interrupt Signal (SIGINT)" => (
                "実行中プロセスを中断 (SIGINT)",
                "현재 실행 중인 프로세스 중단 (SIGINT)",
                "Enviar señal de interrupción (SIGINT)",
                "Envoyer le signal d’interruption (SIGINT)",
                "Unterbrechungssignal senden (SIGINT)",
            ),
            "Send EOF / Logout Shell" => (
                "EOF を送信 / シェルをログアウト",
                "EOF 전송 / 셸 로그아웃",
                "Enviar EOF / Cerrar sesión del shell",
                "Envoyer EOF / Déconnecter le shell",
                "EOF senden / Shell abmelden",
            ),
            _ => (en, en, en, en, en),
        },
    }
}
