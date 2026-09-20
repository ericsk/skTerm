use fontdue::{Font, FontSettings};

/// Candidate faces that ship with the supported platforms. The Linux entries that
/// carry CJK coverage are installed by CI (`fonts-noto-cjk`).
const CANDIDATE_PATHS: &[&str] = &[
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

/// A glyph that rasterized to the right size but is entirely transparent would
/// render as a blank cell, which is exactly the failure this test exists to catch.
fn has_visible_coverage(bitmap: &[u8]) -> bool {
    bitmap.iter().any(|&a| a != 0)
}

#[test]
fn test_cjk_rasterize() {
    let mut ascii_font: Option<&str> = None;
    let mut cjk_font: Option<&str> = None;

    for path in CANDIDATE_PATHS {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        let Ok(font) = Font::from_bytes(bytes.as_slice(), FontSettings::default()) else {
            continue;
        };

        let (metrics, bitmap) = font.rasterize('A', 14.0);
        if metrics.width > 0 && has_visible_coverage(&bitmap) {
            println!(
                "ASCII ok  {}: 'A' w={} h={} len={}",
                path,
                metrics.width,
                metrics.height,
                bitmap.len()
            );
            ascii_font.get_or_insert(path);
        }

        let (cjk_metrics, cjk_bitmap) = font.rasterize('中', 14.0);
        if cjk_metrics.width > 0 && has_visible_coverage(&cjk_bitmap) {
            println!(
                "CJK   ok  {}: '中' w={} h={} len={}",
                path,
                cjk_metrics.width,
                cjk_metrics.height,
                cjk_bitmap.len()
            );

            // A CJK ideograph is nominally square and full-width. Guard against a
            // face that reports coverage but rasterizes something degenerate.
            assert!(
                cjk_metrics.height > 0,
                "{path}: '中' rasterized with zero height"
            );
            assert_eq!(
                cjk_bitmap.len(),
                cjk_metrics.width * cjk_metrics.height,
                "{path}: '中' bitmap length does not match its metrics"
            );

            cjk_font = Some(path);
            // Only stop once we have found real CJK coverage. Stopping at the first
            // font that merely *loads* is how this test previously ended on
            // DejaVu/Liberation and never exercised a CJK glyph at all.
            break;
        }
    }

    assert!(
        ascii_font.is_some(),
        "no usable system font found in any of the {} candidate paths -- \
         cannot validate rasterization at all",
        CANDIDATE_PATHS.len()
    );

    assert!(
        cjk_font.is_some(),
        "no CJK-capable font found. CJK rendering is a headline feature, so this \
         is a real failure rather than a skip. Install one: \
         Linux `apt install fonts-noto-cjk` (CI already does), \
         macOS/Windows ship one by default."
    );
}
