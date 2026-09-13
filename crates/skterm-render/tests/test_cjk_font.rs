use fontdue::{Font, FontSettings};

#[test]
fn test_cjk_rasterize() {
    let candidate_paths = [
        // macOS
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\msyh.ttf",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\msgothic.ttc",
        "C:\\Windows\\Fonts\\malgun.ttf",
        "C:\\Windows\\Fonts\\arial.ttf",
        "C:\\Windows\\Fonts\\seguiemj.ttf",
        // Linux
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    ];

    let mut tested_any_font = false;
    for path in &candidate_paths {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes.as_slice(), FontSettings::default()) {
                let (metrics, bitmap) = font.rasterize('A', 14.0);
                if metrics.width > 0 && !bitmap.is_empty() {
                    println!(
                        "SUCCESS: Font {}: 'A' w={}, h={}, len={}",
                        path,
                        metrics.width,
                        metrics.height,
                        bitmap.len()
                    );
                    tested_any_font = true;
                }

                let (cjk_metrics, cjk_bitmap) = font.rasterize('中', 14.0);
                if cjk_metrics.width > 0 && !cjk_bitmap.is_empty() {
                    println!(
                        "SUCCESS: Font {}: '中' w={}, h={}, len={}",
                        path,
                        cjk_metrics.width,
                        cjk_metrics.height,
                        cjk_bitmap.len()
                    );
                }
                break;
            }
        }
    }

    if !tested_any_font {
        println!("Note: No standard system fonts found on this headless environment. Rasterize test skipped gracefully.");
    }
}

#[test]
fn test_font_baseline_centering() {
    let candidate_paths = [
        // Windows
        "C:\\Windows\\Fonts\\consola.ttf",
        "C:\\Windows\\Fonts\\CascadiaMono.ttf",
        "C:\\Windows\\Fonts\\CascadiaCode.ttf",
        "C:\\Windows\\Fonts\\lucon.ttf",
        "C:\\Windows\\Fonts\\cour.ttf",
        "C:\\Windows\\Fonts\\NotoMono-Regular.ttf",
        "C:\\Windows\\Fonts\\NotoSansMonoCJKtc-Regular.otf",
        // macOS
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Monaco.dfont",
        // Linux
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
    ];
    let mut tested = false;
    for path in candidate_paths {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes.as_slice(), FontSettings::default()) {
                let font_size = 16.0;
                let m_metrics = font.metrics('M', font_size);
                let line_metrics = font.horizontal_line_metrics(font_size).unwrap_or(fontdue::LineMetrics {
                    ascent: font_size,
                    descent: -font_size * 0.25,
                    line_gap: 0.0,
                    new_line_size: font_size * 1.25,
                });
                let cell_width = m_metrics.advance_width.ceil().max(1.0);
                let cell_height = (line_metrics.ascent - line_metrics.descent + line_metrics.line_gap).ceil().max(1.0);
                let font_ascent = line_metrics.ascent;

                let gt_metric = font.metrics('>', font_size);
                let baseline_offset = if gt_metric.height > 0 {
                    let gt_h = gt_metric.height as f32;
                    let gt_ymin = gt_metric.ymin as f32;
                    let top_margin = ((cell_height - gt_h) / 2.0).round();
                    (top_margin + gt_h + gt_ymin).clamp(1.0, cell_height - 1.0)
                } else if m_metrics.height > 0 {
                    let cap_h = m_metrics.height as f32;
                    let cap_ymin = m_metrics.ymin as f32;
                    let top_margin = ((cell_height - cap_h) / 2.0).round();
                    (top_margin + cap_h + cap_ymin).clamp(1.0, cell_height - 1.0)
                } else {
                    font_ascent.round().clamp(1.0, cell_height - 1.0)
                };

                assert!(cell_width > 0.0);
                assert!(baseline_offset >= 1.0);
                assert!(baseline_offset < cell_height);

                let (gt_metrics, _) = font.rasterize('>', font_size);
                if gt_metrics.height > 0 {
                    let gt_top = baseline_offset - gt_metrics.ymin as f32 - gt_metrics.height as f32;
                    let gt_bottom = baseline_offset - gt_metrics.ymin as f32;
                    let top_margin = gt_top;
                    let bottom_margin = cell_height - gt_bottom;

                    // Prompt '>' should be approximately centered in the cell (margin difference <= 2px)
                    assert!(
                        (top_margin - bottom_margin).abs() <= 2.0,
                        "Font {} '>' not centered: top_margin={}, bottom_margin={}",
                        path,
                        top_margin,
                        bottom_margin
                    );
                }
                tested = true;
            }
        }
    }
    if !tested {
        println!("Note: No candidate font found on this environment, skipped gracefully.");
    }
}




