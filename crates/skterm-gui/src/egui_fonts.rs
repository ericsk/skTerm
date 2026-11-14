//! egui font setup shared by the preferences window.

use egui::Context;
use std::fs;
use tracing::info;

/// Setup UI, Monospace, CJK (Chinese / Japanese / Korean), and Symbol font support for egui
/// so all localized text and keyboard shortcut badges in the Preferences window display properly
/// without missing glyphs (note: `egui` 0.37+ does not bundle `default_fonts` by default).
pub fn setup_egui_cjk_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();

    // 1. Primary system monospace font for FontFamily::Monospace (live terminal preview, hex inputs)
    //    and Latin/symbol fallback for FontFamily::Proportional.
    let mono_candidates = [
        // macOS
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Monaco.ttf",
        // Linux
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "/usr/share/fonts/truetype/ubuntu/UbuntuMono-R.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
        // Windows
        "C:\\Windows\\Fonts\\consola.ttf",
    ];

    let mut loaded_mono = false;
    for path in mono_candidates {
        if let Ok(font_data) = fs::read(path) {
            info!("Loaded UI monospace font into egui preferences: {}", path);
            fonts.font_data.insert(
                "ui_mono_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(font_data)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "ui_mono_font".to_owned());
            loaded_mono = true;
            break;
        }
    }

    // 2. Primary CJK + Latin proportional UI font
    let cjk_candidates = [
        // macOS
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        // Linux (NotoSansCJK covers TC, SC, JP, and KR)
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
            info!("Loaded primary CJK font into egui preferences: {}", path);
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

    // 3. Dedicated Korean Hangul fallback font on macOS/Windows where the primary
    //    Chinese/Japanese system font may not include precomposed Hangul syllables.
    let korean_candidates = [
        "/System/Library/Fonts/AppleSDGothicNeo.ttc",
        "/System/Library/Fonts/Supplemental/AppleGothic.ttf",
        "C:\\Windows\\Fonts\\malgun.ttf",
    ];

    for path in korean_candidates {
        if let Ok(font_data) = fs::read(path) {
            info!(
                "Loaded Korean fallback font into egui preferences: {}",
                path
            );
            fonts.font_data.insert(
                "korean_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(font_data)),
            );

            let prop = fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default();
            let insert_idx = prop.len().min(1);
            prop.insert(insert_idx, "korean_font".to_owned());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("korean_font".to_owned());

            break;
        }
    }

    // 4. Dedicated Japanese fallback font on Windows if available
    let jp_win_candidates = [
        "C:\\Windows\\Fonts\\YuGothR.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
    ];

    for path in jp_win_candidates {
        if let Ok(font_data) = fs::read(path) {
            info!(
                "Loaded Japanese fallback font into egui preferences: {}",
                path
            );
            fonts.font_data.insert(
                "jp_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(font_data)),
            );

            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("jp_font".to_owned());

            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("jp_font".to_owned());

            break;
        }
    }

    // 5. Latin proportional fallback (ensures Proportional is populated even on systems
    //    without CJK fonts) + append monospace font as symbol fallback in Proportional.
    let sans_candidates = [
        "/System/Library/Fonts/SFNS.ttf",
        "/System/Library/Fonts/Supplemental/Arial.ttf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/TTF/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf",
        "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        "C:\\Windows\\Fonts\\segoeui.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
    ];

    for path in sans_candidates {
        if let Ok(font_data) = fs::read(path) {
            fonts.font_data.insert(
                "ui_sans_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(font_data)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("ui_sans_font".to_owned());
            if !loaded_mono {
                fonts
                    .families
                    .entry(egui::FontFamily::Monospace)
                    .or_default()
                    .push("ui_sans_font".to_owned());
            }
            break;
        }
    }

    if loaded_mono {
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("ui_mono_font".to_owned());
    }

    // 6. Dedicated monochrome symbol fallback font (⌘, ⌥, ⌃, ⇧, ✓, ↻, ✕, ✖, ⚠, ⚙, arrows)
    let symbol_candidates = [
        "/System/Library/Fonts/Apple Symbols.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols-Regular.ttf",
        "/usr/share/fonts/truetype/noto/NotoSansSymbols2-Regular.ttf",
        "C:\\Windows\\Fonts\\seguisym.ttf",
    ];

    for path in symbol_candidates {
        if let Ok(font_data) = fs::read(path) {
            fonts.font_data.insert(
                "symbol_font".to_owned(),
                std::sync::Arc::new(egui::FontData::from_owned(font_data)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("symbol_font".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .push("symbol_font".to_owned());
            break;
        }
    }

    ctx.set_fonts(fonts);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_egui_cjk_fonts_and_symbol_coverage() {
        let ctx = Context::default();
        setup_egui_cjk_fonts(&ctx);

        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(820.0, 600.0),
            )),
            ..Default::default()
        };
        raw.viewports
            .entry(egui::viewport::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(2.0);

        let out = ctx.run_ui(raw, |ui| {
            let prop_id = egui::FontId::proportional(14.0);
            let mono_id = egui::FontId::monospace(12.5);
            for ch in [
                'A', '設', '置', 'あ', '한', '⌘', '⌥', '⌃', '⇧', '✓', '↻', '✕', '✖', '⚠', '⚙', '↑',
                '↓',
            ] {
                assert!(
                    ui.fonts_mut(|f| f.has_glyph(&prop_id, ch)),
                    "Proportional font chain missing glyph for {ch:?}"
                );
                assert!(
                    ui.fonts_mut(|f| f.has_glyph(&mono_id, ch)),
                    "Monospace font chain missing glyph for {ch:?}"
                );
            }

            egui::Panel::left("pref_sidebar")
                .exact_size(195.0)
                .show(ui, |ui| {
                    ui.label("skTerm 偏好設定");
                });
            egui::CentralPanel::default().show(ui, |ui| {
                ui.label("Live Preview — 即時預覽");
            });
        });

        let prims = ctx.tessellate(out.shapes.clone(), out.pixels_per_point);
        assert!(!prims.is_empty());
        assert!(!out.textures_delta.set.is_empty());
        out.drop_without_applying_deltas();
    }
}
