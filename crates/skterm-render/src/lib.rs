//! `skterm-render`: wgpu-based GPU text & quad rendering pipeline, glyph atlas, and shaders.

pub mod atlas;
pub mod box_drawing;
pub mod pipeline;
pub mod renderer;
pub mod shader;

pub use atlas::{GlyphAtlas, GlyphInfo, GlyphKey};
pub use pipeline::{color_to_rgba, BgInstance, GlyphInstance, TerminalPipeline};
pub use renderer::{
    color_to_rgba_themed, AgentSidebarItem, PaneRenderItem, RenderTheme, TabItem, TermRenderer,
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
}
