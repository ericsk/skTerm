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
    char_display_cells, color_to_rgba_themed, color_to_rgba_with_dynamic_palette,
    for_each_ellipsized_glyph, text_display_cells, wide_cell_center_offset, AgentPanelState,
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

    #[test]
    fn test_tab_title_cjk_glyph_layout_and_ellipsis() {
        // Realistic 16pt @ 2x Retina metrics:
        // Primary monospace Latin/digits ('Menlo' 32px): advance = 19.265625px (cell_w = 20.0px)
        // CJK fallback ideographs ('Hiragino Sans GB' / 'PingFang' 32px): advance = 32.0px (1.0em, NOT 2 * cell_w = 40.0px!)
        let ascii_adv = 19.265625_f32;
        let cjk_adv = 32.0_f32;
        let advance_of = |c: char| -> f32 {
            if unicode_width::UnicodeWidthChar::width(c) == Some(0) {
                0.0
            } else if char_display_cells(c) > 1 {
                cjk_adv
            } else {
                ascii_adv
            }
        };

        let collect = |text: &str, x: f32, max_w: f32| -> Vec<(char, f32)> {
            let mut out = Vec::new();
            for_each_ellipsized_glyph(
                text,
                x,
                max_w,
                |c| (advance_of(c), ()),
                |c, gx, ()| out.push((c, gx)),
            );
            out
        };

        // 1. Mixed ASCII + CJK title that fits completely: advances by true typographic advance
        // rather than inflated 2 * cell_w (40px), keeping inter-character spacing uniform.
        let title = "16級資優生記事";
        let glyphs = collect(title, 20.0, 316.0);
        assert_eq!(glyphs.len(), 8);
        assert_eq!(glyphs[0], ('1', 20.0));
        assert!((glyphs[1].1 - (20.0 + ascii_adv)).abs() < 1e-3);
        assert!((glyphs[2].1 - (20.0 + 2.0 * ascii_adv)).abs() < 1e-3);
        // Each CJK char steps by exactly 32.0px (1.0em), NOT 40.0px (2 * cell_w)
        for pair in glyphs[2..].windows(2) {
            assert!((pair[1].1 - pair[0].1 - cjk_adv).abs() < 1e-3);
        }
        let &(last_c, last_x) = glyphs.last().unwrap();
        assert!(last_x + advance_of(last_c) <= 20.0 + 316.0);

        // 2. Overflowing mixed ASCII + CJK tab title ("16級資優班生活大小事") inside a 2x Retina
        // tab capsule (max_text_w = 316px): fits 8 CJK chars ("16級資優班生活大小…") instead of 6!
        let folder_title = "16級資優班生活大小事";
        let truncated_folder = collect(folder_title, 20.0, 316.0);
        let folder_str: String = truncated_folder.iter().map(|(c, _)| *c).collect();
        assert_eq!(folder_str, "16級資優班生活大小…");
        let &(end_c, end_x) = truncated_folder.last().unwrap();
        assert_eq!(end_c, '…');
        assert!(end_x + advance_of(end_c) <= 20.0 + 316.0 + 1e-3);

        // 3. Overflowing agent tab title ("agy · 16級資優班生活大小事") inside 316px
        let agent_title = "agy · 16級資優班生活大小事";
        let truncated_agent = collect(agent_title, 10.0, 316.0);
        let agent_str: String = truncated_agent.iter().map(|(c, _)| *c).collect();
        assert_eq!(agent_str, "agy · 16級資優班…");
        let &(a_end_c, a_end_x) = truncated_agent.last().unwrap();
        assert_eq!(a_end_c, '…');
        assert!(a_end_x + advance_of(a_end_c) <= 10.0 + 316.0 + 1e-3);

        // 4. Edge cases: zero/sub-ellipsis widths, leading wide CJK char, and 0-advance combining mark
        assert!(collect(agent_title, 0.0, 0.0).is_empty());
        assert!(collect(agent_title, 0.0, 15.0).is_empty());
        assert_eq!(collect("中文", 5.0, 25.0), vec![('…', 5.0)]);
        let combined = collect("e\u{0301}中文測試", 0.0, 45.0);
        let combined_str: String = combined.iter().map(|(c, _)| *c).collect();
        assert_eq!(combined_str, "e\u{0301}…");
    }

    #[test]
    fn test_wide_cell_center_offset() {
        // Single-cell characters (ASCII, Powerline triangles, modifier symbols) never shift
        assert_eq!(wide_cell_center_offset('6', 19.265, 20.0), 0.0);
        assert_eq!(wide_cell_center_offset('\u{E0B0}', 19.265, 20.0), 0.0);
        assert_eq!(wide_cell_center_offset('⌘', 20.0, 20.0), 0.0);

        // 2-cell CJK character (32px 1.0em advance inside 2 * 20px = 40px double-cell) centers by +4px
        assert_eq!(wide_cell_center_offset('級', 32.0, 20.0), 4.0);

        // True 2:1 CJK monospace font (32px advance inside 2 * 16px = 32px double-cell) needs 0px offset
        assert_eq!(wide_cell_center_offset('級', 32.0, 16.0), 0.0);
        assert_eq!(wide_cell_center_offset('級', 40.0, 20.0), 0.0);
    }
}
