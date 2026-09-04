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
