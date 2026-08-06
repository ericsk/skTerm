use fontdue::{Font, FontSettings};

#[test]
fn test_cjk_rasterize() {
    let candidate_paths = [
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
    ];

    let mut found_cjk = false;
    for path in &candidate_paths {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes.as_slice(), FontSettings::default()) {
                let (metrics, bitmap) = font.rasterize('中', 14.0);
                if metrics.width > 0 && !bitmap.is_empty() {
                    println!("SUCCESS: Font {}: '中' w={}, h={}, len={}", path, metrics.width, metrics.height, bitmap.len());
                    found_cjk = true;
                }
            }
        }
    }
    assert!(found_cjk);
}
