//! Colour-theme construction and live application of configuration changes.

use super::App;
use crate::config::{parse_hex_color, Config, Theme};
use skterm_render::RenderTheme;
use std::time::Instant;
use tracing::info;

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

        let render_theme = self.build_render_theme();
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
            if decorations_changed {
                session
                    .window
                    .set_decorations(new_cfg.window.decorations != "none");
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
