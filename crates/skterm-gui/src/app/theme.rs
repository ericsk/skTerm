//! Colour-theme construction and live application of configuration changes.

use super::App;
use crate::config::{parse_hex_color, Config, Theme};
use skterm_render::{color_to_rgba, RenderTheme};
use std::time::Instant;
use tracing::info;

/// Extracted 8-bit RGB theme defaults and dark-mode classification for syncing into `skterm-core`
/// (`OSC 4/10/11/12/21;?` queries and DEC Mode `?2031` / `CSI ? 996 n` color scheme reporting).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CoreThemeColors {
    pub fg: [u8; 3],
    pub bg: [u8; 3],
    pub cursor: [u8; 3],
    pub palette_16: [[u8; 3]; 16],
    pub is_dark: bool,
}

#[inline]
fn rgba_f32_to_rgb_u8(c: [f32; 4]) -> [u8; 3] {
    [
        (c[0].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[1].clamp(0.0, 1.0) * 255.0).round() as u8,
        (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
    ]
}

/// WCAG 2.x sRGB relative luminance in `[0.0, 1.0]`.
pub fn relative_luminance(rgb: [f32; 4]) -> f32 {
    #[inline]
    fn linearize(c: f32) -> f32 {
        let c = c.clamp(0.0, 1.0);
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * linearize(rgb[0]) + 0.7152 * linearize(rgb[1]) + 0.0722 * linearize(rgb[2])
}

/// Extract `(fg, bg, cursor, palette_16, is_dark)` from the active [`RenderTheme`].
/// `is_dark` is determined by WCAG relative luminance of `theme.default_bg` (`< 0.179`).
pub fn extract_core_theme_colors(theme: &RenderTheme) -> CoreThemeColors {
    let fg = rgba_f32_to_rgb_u8(theme.default_fg);
    let bg = rgba_f32_to_rgb_u8(theme.default_bg);
    let cursor = rgba_f32_to_rgb_u8(theme.cursor_color);
    let palette_16 = match theme.ansi_palette {
        Some(pal) => std::array::from_fn(|i| rgba_f32_to_rgb_u8(pal[i])),
        None => std::array::from_fn(|i| {
            rgba_f32_to_rgb_u8(color_to_rgba(skterm_core::Color::Indexed(i as u8), false))
        }),
    };
    let is_dark = relative_luminance(theme.default_bg) < 0.179;
    CoreThemeColors {
        fg,
        bg,
        cursor,
        palette_16,
        is_dark,
    }
}

impl App {
    pub fn build_render_theme(&self) -> RenderTheme {
        let config = &self.config_manager.current_config;
        let preset = Theme::get(&config.colors.theme);
        let mut render_theme = RenderTheme {
            default_fg: preset.foreground,
            default_bg: preset.background,
            cursor_color: preset.cursor,
            selection_color: preset.selection,
            tab_bar_bg: [
                preset.background[0] * 0.85,
                preset.background[1] * 0.85,
                preset.background[2] * 0.85,
                1.0,
            ],
            tab_active_bg: preset.background,
            tab_active_fg: preset.foreground,
            tab_inactive_fg: [
                preset.foreground[0] * 0.6,
                preset.foreground[1] * 0.6,
                preset.foreground[2] * 0.6,
                1.0,
            ],
            border_color: [0.3, 0.3, 0.35, 1.0],
            focused_border_color: [0.5, 0.7, 1.0, 1.0],
            ansi_palette: Some(preset.palette),
        };

        if let Some(fg_hex) = &config.colors.foreground {
            if let Some(c) = parse_hex_color(fg_hex) {
                render_theme.default_fg = c;
                render_theme.tab_active_fg = c;
            }
        }
        if let Some(bg_hex) = &config.colors.background {
            if let Some(c) = parse_hex_color(bg_hex) {
                render_theme.default_bg = c;
                render_theme.tab_active_bg = c;
            }
        }
        if let Some(cur_hex) = &config.colors.cursor {
            if let Some(c) = parse_hex_color(cur_hex) {
                render_theme.cursor_color = c;
            }
        }
        if let Some(sel_hex) = &config.colors.selection {
            if let Some(c) = parse_hex_color(sel_hex) {
                render_theme.selection_color = c;
            }
        }

        if let Some(palette_cfg) = &config.colors.palette {
            let mut pal = preset.palette;

            if let Some(c) = palette_cfg.black.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[0] = c;
            }
            if let Some(c) = palette_cfg.red.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[1] = c;
            }
            if let Some(c) = palette_cfg.green.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[2] = c;
            }
            if let Some(c) = palette_cfg.yellow.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[3] = c;
            }
            if let Some(c) = palette_cfg.blue.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[4] = c;
            }
            if let Some(c) = palette_cfg
                .magenta
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[5] = c;
            }
            if let Some(c) = palette_cfg.cyan.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[6] = c;
            }
            if let Some(c) = palette_cfg.white.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[7] = c;
            }
            if let Some(c) = palette_cfg
                .bright_black
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[8] = c;
            }
            if let Some(c) = palette_cfg
                .bright_red
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[9] = c;
            }
            if let Some(c) = palette_cfg
                .bright_green
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[10] = c;
            }
            if let Some(c) = palette_cfg
                .bright_yellow
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[11] = c;
            }
            if let Some(c) = palette_cfg
                .bright_blue
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[12] = c;
            }
            if let Some(c) = palette_cfg
                .bright_magenta
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[13] = c;
            }
            if let Some(c) = palette_cfg
                .bright_cyan
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[14] = c;
            }
            if let Some(c) = palette_cfg
                .bright_white
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[15] = c;
            }

            render_theme.ansi_palette = Some(pal);
        }

        render_theme
    }

    /// Applies a configuration to every live session.
    ///
    /// `persist` controls whether the config is also written back to `config.toml`.
    /// It MUST be `false` when the config originated from the file watcher, otherwise
    /// the write re-triggers the watcher and the two feed each other in a loop.
    pub fn apply_config_live(&mut self, new_cfg: &Config, persist: bool) {
        info!("Applying live configuration update to all terminal sessions...");
        let old_font = self.config_manager.current_config.font.clone();
        let old_decorations = self
            .config_manager
            .current_config
            .window
            .decorations
            .clone();
        let font_changed = old_font.family != new_cfg.font.family
            || (old_font.size - new_cfg.font.size).abs() > 0.01
            || old_font.fallback_families != new_cfg.font.fallback_families;
        let decorations_changed = old_decorations != new_cfg.window.decorations;

        self.config_manager.current_config = new_cfg.clone();
        let new_locale = crate::i18n::Locale::resolve(&new_cfg.window.language);
        crate::i18n::set_locale(new_locale);
        #[cfg(target_os = "macos")]
        crate::macos_menu::install_or_update_macos_menu(new_locale);
        if let Some(pref_win) = &mut self.preferences_window {
            // If config was reloaded from disk (not from the prefs UI itself), sync the open form.
            if !persist {
                pref_win.load_from_config(new_cfg);
            }
            pref_win.needs_redraw = true;
            pref_win.window.request_redraw();
        }

        let render_theme = self.build_render_theme();
        let core_colors = extract_core_theme_colors(&render_theme);
        let tab_bar_h = self.get_tab_bar_height();

        let cursor_shape = match new_cfg.cursor.shape.as_str() {
            "beam" => skterm_core::CursorShape::Beam,
            "underline" => skterm_core::CursorShape::Underline,
            _ => skterm_core::CursorShape::Block,
        };

        for session in self.sessions.values_mut() {
            if font_changed {
                let _ = session.renderer.update_font(
                    &new_cfg.font.family,
                    new_cfg.font.size,
                    &new_cfg.font.fallback_families,
                );
            }
            session
                .renderer
                .update_line_height(new_cfg.font.line_height);
            session
                .renderer
                .update_padding(new_cfg.window.padding_x, new_cfg.window.padding_y);
            session.renderer.update_opacity(new_cfg.window.opacity);
            session.renderer.set_theme(render_theme);
            session.workspace.apply_theme_colors(
                core_colors.fg,
                core_colors.bg,
                core_colors.cursor,
                core_colors.palette_16,
                core_colors.is_dark,
            );
            if decorations_changed {
                session
                    .window
                    .set_decorations(new_cfg.window.decorations != "none");
            }
            #[cfg(target_os = "macos")]
            {
                use winit::platform::macos::WindowExtMacOS;
                session
                    .window
                    .set_option_as_alt(super::macos::parse_macos_option_as_alt(
                        &new_cfg.terminal.option_as_alt,
                    ));
            }
            session.workspace.update_cursor_shape(cursor_shape);
            session.sync_all_pane_sizes(tab_bar_h);
            session.request_redraw();
        }

        // Mark dirty rather than writing here. `check_changed` fires on *every*
        // preferences-window event, so dragging a slider used to spawn a writer
        // thread per mouse move; those raced each other inside the same file and
        // could leave invalid TOML on disk.
        if persist {
            self.pending_config_save = true;
            self.last_config_change = Some(Instant::now());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_core_theme_colors_classifies_dark_and_light_backgrounds() {
        // Dark theme (OLED black default)
        let dark = RenderTheme::default();
        let dark_colors = extract_core_theme_colors(&dark);
        assert!(dark_colors.is_dark);
        assert_eq!(dark_colors.bg, [0, 0, 0]);
        assert_eq!(dark_colors.fg, [245, 245, 247]);
        // Fallback ANSI palette (when `ansi_palette` is `None`) matches `color_to_rgba`
        assert_eq!(dark_colors.palette_16[0], [0, 0, 0]);
        assert_eq!(dark_colors.palette_16[15], [255, 255, 255]);

        // Dark preset (Dracula)
        let dracula = Theme::get("dracula");
        let dracula_render = RenderTheme {
            default_fg: dracula.foreground,
            default_bg: dracula.background,
            cursor_color: dracula.cursor,
            ansi_palette: Some(dracula.palette),
            ..RenderTheme::default()
        };
        let dracula_colors = extract_core_theme_colors(&dracula_render);
        assert!(dracula_colors.is_dark);
        assert_eq!(dracula_colors.palette_16[1], [230, 71, 71]);

        // Light background (Solarized Light `#fdf6e3`)
        let light = RenderTheme {
            default_fg: [0.40, 0.48, 0.51, 1.0],
            default_bg: [0.992, 0.965, 0.890, 1.0],
            cursor_color: [0.40, 0.48, 0.51, 1.0],
            ansi_palette: None,
            ..RenderTheme::default()
        };
        let light_colors = extract_core_theme_colors(&light);
        assert!(!light_colors.is_dark);
        assert!(relative_luminance(light.default_bg) > 0.8);
        assert_eq!(light_colors.bg, [253, 246, 227]);
    }
}
