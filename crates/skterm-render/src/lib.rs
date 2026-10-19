//! `skterm-render`: wgpu-based GPU text & quad rendering pipeline, glyph atlas, and shaders.

pub mod agent_panel;
pub mod atlas;
pub mod box_drawing;
pub mod pipeline;
pub mod renderer;
pub mod shader;

pub use agent_panel::{AgentPanelHit, AgentPanelLayout, PanelRect};
pub use atlas::{is_terminal_friendly_font, GlyphAtlas, GlyphInfo, GlyphKey};
pub use pipeline::{color_to_rgba, BgInstance, GlyphInstance, TerminalPipeline};
pub use renderer::{
    color_to_rgba_themed, color_to_rgba_with_dynamic_palette, text_display_cells, AgentPanelState,
    AgentSidebarItem, AgentStatus, PaneRenderItem, PreeditOverlayLayout, RenderSurfaceOutcome,
    RenderTheme, ScrollbarMetrics, TabItem, TermRenderer, TopDropdownMenu, TopDropdownMenuItem,
    TopRightButtons,
};

/// skterm-render version and basic info
pub fn render_info() -> &'static str {
    "skterm-render v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_info() {
        assert_eq!(render_info(), "skterm-render v0.1.0");
    }

    #[test]
    fn test_color_conversions() {
        use skterm_core::{Color, NamedColor};
        let red = color_to_rgba(Color::Named(NamedColor::Red), false);
        assert!(red[0] >= 0.8);
        assert!(red[1] < 0.5);

        let rgb = color_to_rgba(Color::Rgb(255, 128, 0), false);
        assert_eq!(rgb, [1.0, 128.0 / 255.0, 0.0, 1.0]);
    }

    #[test]
    fn test_theme_color_overrides() {
        use skterm_core::{Color, NamedColor};
        use std::collections::HashMap;
        let theme = RenderTheme::default();
        assert_eq!(
            color_to_rgba_themed(Color::DefaultFg, false, false, &theme),
            theme.default_fg
        );
        assert_eq!(
            color_to_rgba_themed(Color::DefaultBg, true, false, &theme),
            theme.default_bg
        );
        // Verify bold_is_bright promotes ANSI 0..7 to 8..15 when palette is present
        let mut themed = theme;
        let mut palette = [[0.0f32; 4]; 16];
        palette[9] = [1.0, 0.2, 0.2, 1.0];
        themed.ansi_palette = Some(palette);
        assert_eq!(
            color_to_rgba_themed(Color::Indexed(1), false, true, &themed),
            palette[9]
        );

        // Verify per-pane dynamic OSC 4 palette overrides take precedence over static theme
        let mut dyn_pal = HashMap::new();
        dyn_pal.insert(2u8, (0, 255, 128));
        dyn_pal.insert(10u8, (128, 255, 0));
        assert_eq!(
            color_to_rgba_with_dynamic_palette(
                Color::Named(NamedColor::Green),
                false,
                false,
                &themed,
                Some(&dyn_pal),
            ),
            [0.0, 1.0, 128.0 / 255.0, 1.0]
        );
        assert_eq!(
            color_to_rgba_with_dynamic_palette(
                Color::Named(NamedColor::Green),
                false,
                true,
                &themed,
                Some(&dyn_pal),
            ),
            [128.0 / 255.0, 1.0, 0.0, 1.0]
        );
    }

    #[test]
    fn test_scrollbar_metrics() {
        // No scrollback lines -> no scrollbar
        assert!(
            ScrollbarMetrics::compute(0.0, 0.0, 800.0, 600.0, 1.0, 0, 24, 0, false, false)
                .is_none()
        );

        // With scrollback lines
        let m_bottom =
            ScrollbarMetrics::compute(0.0, 0.0, 800.0, 600.0, 1.0, 100, 25, 0, false, false)
                .expect("Should compute scrollbar metrics");

        // At scroll_offset = 0 (bottom), thumb should be at bottom of travel
        assert_eq!(m_bottom.thumb_y, m_bottom.track_y + m_bottom.travel);
        assert!(m_bottom.thumb_h > 0.0);
        assert!(m_bottom.thumb_x < 800.0);
        assert!(m_bottom.hit_x <= m_bottom.thumb_x);
        assert!(800.0 <= m_bottom.hit_x + m_bottom.hit_w + 1.0);

        // At scroll_offset = scrollback_len (top), thumb should be at top of travel
        let m_top =
            ScrollbarMetrics::compute(0.0, 0.0, 800.0, 600.0, 1.0, 100, 25, 100, false, false)
                .expect("Should compute scrollbar metrics");
        assert_eq!(m_top.thumb_y, m_top.track_y);
    }

    #[test]
    fn test_preedit_overlay_layout_zhuyin_and_kana() {
        // Empty preedit returns None
        assert!(PreeditOverlayLayout::compute("", None, 20.0, 0.0, 800.0, 10.0, 10.0).is_none());

        // Zhuyin radicals "ㄐㄧˇ" -> 'ㄐ' (2 cells) + 'ㄧ' (2 cells) + 'ˇ' (1 cell) = 5 cells
        let zhuyin = "ㄐㄧˇ";
        assert_eq!(text_display_cells(zhuyin), 5);
        let layout = PreeditOverlayLayout::compute(
            zhuyin,
            Some((zhuyin.len(), zhuyin.len())),
            50.0,
            0.0,
            800.0,
            10.0,
            10.0,
        )
        .expect("zhuyin preedit layout");
        assert_eq!(layout.start_x, 50.0);
        assert_eq!(layout.box_w, 50.0);
        assert_eq!(layout.selected_clause, None);
        assert_eq!(layout.caret_x, 100.0);

        // Japanese Kana multi-clause conversion: "きょうは" (4 full-width kana = 8 cells)
        // with first 2 kana ("きょ" = 6 UTF-8 bytes = 4 cells) selected as active clause
        let kana = "きょうは";
        assert_eq!(text_display_cells(kana), 8);
        let layout_kana =
            PreeditOverlayLayout::compute(kana, Some((0, 6)), 100.0, 0.0, 800.0, 10.0, 10.0)
                .expect("kana preedit layout");
        assert_eq!(layout_kana.start_x, 100.0);
        assert_eq!(layout_kana.box_w, 80.0);
        assert_eq!(layout_kana.selected_clause, Some((100.0, 40.0)));
        assert_eq!(layout_kana.caret_x, 140.0);

        // Right-edge clamping when cursor is near the right border of the pane
        let clamped = PreeditOverlayLayout::compute(kana, None, 760.0, 0.0, 800.0, 10.0, 10.0)
            .expect("clamped preedit layout");
        // max_right = 800 - 10 = 790, box_w = 80 -> start_x shifts left to 710
        assert_eq!(clamped.start_x, 710.0);
        assert_eq!(clamped.box_w, 80.0);

        // Ghostty/cmux grid-column span calculation (`compute_grid_cols`)
        assert!(PreeditOverlayLayout::compute_grid_cols("", 5, 80).is_none());
        // Cangjie radical "日月" -> 4 cells at col 10 -> 10..14
        assert_eq!(
            PreeditOverlayLayout::compute_grid_cols("日月", 10, 80),
            Some((10, 14))
        );
        // Zhuyin "ㄐㄧˇ" -> 5 cells at col 5 -> 5..10
        assert_eq!(
            PreeditOverlayLayout::compute_grid_cols(zhuyin, 5, 80),
            Some((5, 10))
        );
        // Right-edge shift when cursor_col + width > cols (78 + 8 > 80 -> shifts to 72..80)
        assert_eq!(
            PreeditOverlayLayout::compute_grid_cols(kana, 78, 80),
            Some((72, 80))
        );
        // Truncates cleanly if wider than the entire row (cols = 5, kana = 8 cells -> 2 chars = 4 cells)
        assert_eq!(
            PreeditOverlayLayout::compute_grid_cols(kana, 2, 5),
            Some((0, 4))
        );
    }
}
