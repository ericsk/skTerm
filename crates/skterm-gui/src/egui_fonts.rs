//! egui font setup shared by the preferences window.

use egui::Context;
use std::fs;
use tracing::info;

/// Setup CJK (Chinese / Japanese / Korean) font support for egui so all localized
/// text in the Preferences window displays properly without missing glyphs.
pub fn setup_egui_cjk_fonts(ctx: &Context) {
    let mut fonts = egui::FontDefinitions::default();

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

    // Load dedicated Korean Hangul fallback font on macOS/Windows where the primary
    // Chinese/Japanese system font may not include precomposed Hangul syllables.
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

    // Load dedicated Japanese fallback font on Windows if available
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

    ctx.set_fonts(fonts);
}
