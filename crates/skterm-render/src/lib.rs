//! `skterm-render`: wgpu-based GPU text & quad rendering pipeline, glyph atlas, and shaders.

pub mod agent_panel;
pub mod atlas;
pub mod box_drawing;
pub mod pipeline;
pub mod renderer;
pub mod shader;

pub use agent_panel::{AgentPanelHit, AgentPanelLayout, PanelRect};
pub use atlas::{GlyphAtlas, GlyphInfo, GlyphKey};
pub use pipeline::{color_to_rgba, BgInstance, GlyphInstance, TerminalPipeline};
pub use renderer::{
    color_to_rgba_themed, AgentPanelState, AgentSidebarItem, PaneRenderItem, RenderTheme,
    ScrollbarMetrics, TabItem, TermRenderer, TopDropdownMenu, TopDropdownMenuItem,
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
        use skterm_core::Color;
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
}
