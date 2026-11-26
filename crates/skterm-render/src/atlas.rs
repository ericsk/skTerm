//! Dynamic Glyph Texture Atlas, Font Resolution Engine, Multi-Font Fallback, and Fontdue-based text rasterization.

use ab_glyph::{ScaleFont as _, VariableFont as _};
use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tracing::{info, warn};
use wgpu::{
    Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDimension, TextureFormat, TextureUsages,
};

/// Standard OpenType `wght` coordinate for Regular (400) weight.
pub const REGULAR_WEIGHT: f32 = 400.0;
/// Standard OpenType `wght` coordinate for Bold (700) weight.
pub const BOLD_WEIGHT: f32 = 700.0;

fn px_to_ab_scale(font: &ab_glyph::FontVec, px: f32) -> ab_glyph::PxScale {
    let upem = ab_glyph::Font::units_per_em(font)
        .unwrap_or(1000.0)
        .max(1.0);
    let height = ab_glyph::Font::height_unscaled(font);
    ab_glyph::PxScale::from(px * height / upem)
}

/// Unified font face supporting both static fonts (via `fontdue`) and OpenType
/// Variable Fonts with a `wght` variation axis (via `ab_glyph`), so variable fonts
/// such as `NotoSansTC-VariableFont_wght.ttf` render at Regular (400) and Bold (700)
/// rather than defaulting to their Thin (100) base master.
pub enum FontFace {
    Static(Font),
    Variable {
        font: ab_glyph::FontVec,
        collection_index: u32,
        weight: f32,
    },
}

impl FontFace {
    pub fn from_bytes(bytes: Vec<u8>, settings: FontSettings) -> Result<Self, &'static str> {
        Self::from_bytes_with_weight(bytes, REGULAR_WEIGHT, settings)
    }

    pub fn from_bytes_with_weight(
        bytes: Vec<u8>,
        weight: f32,
        settings: FontSettings,
    ) -> Result<Self, &'static str> {
        let collection_index = settings.collection_index;
        if let Ok(mut var_font) = ab_glyph::FontVec::try_from_vec_and_index(bytes, collection_index)
        {
            let has_wght = var_font
                .variations()
                .iter()
                .any(|axis| &axis.tag == b"wght");
            if has_wght {
                var_font.set_variation(b"wght", weight);
                return Ok(Self::Variable {
                    font: var_font,
                    collection_index,
                    weight,
                });
            }
            let raw_bytes = var_font.into_vec();
            return Font::from_bytes(raw_bytes, settings).map(Self::Static);
        }
        Err("Failed to parse font data")
    }

    /// Derive a new instance of this face at a different `wght` coordinate if it is a Variable Font.
    pub fn with_weight(&self, weight: f32) -> Option<Self> {
        match self {
            Self::Static(_) => None,
            Self::Variable {
                font,
                collection_index,
                ..
            } => {
                let mut cloned = ab_glyph::FontVec::try_from_vec_and_index(
                    font.as_slice().to_vec(),
                    *collection_index,
                )
                .ok()?;
                if cloned.set_variation(b"wght", weight) {
                    Some(Self::Variable {
                        font: cloned,
                        collection_index: *collection_index,
                        weight,
                    })
                } else {
                    None
                }
            }
        }
    }

    /// Returns the configured OpenType `wght` coordinate if this is a Variable Font.
    pub fn variable_weight(&self) -> Option<f32> {
        match self {
            Self::Static(_) => None,
            Self::Variable { weight, .. } => Some(*weight),
        }
    }

    pub fn lookup_glyph_index(&self, c: char) -> u16 {
        match self {
            Self::Static(font) => font.lookup_glyph_index(c),
            Self::Variable { font, .. } => ab_glyph::Font::glyph_id(font, c).0,
        }
    }

    pub fn horizontal_line_metrics(&self, px: f32) -> Option<fontdue::LineMetrics> {
        match self {
            Self::Static(font) => font.horizontal_line_metrics(px),
            Self::Variable { font, .. } => {
                let scale = px_to_ab_scale(font, px);
                let sf = ab_glyph::Font::as_scaled(font, scale);
                let ascent = sf.ascent();
                let descent = sf.descent();
                let line_gap = sf.line_gap();
                Some(fontdue::LineMetrics {
                    ascent,
                    descent,
                    line_gap,
                    new_line_size: ascent - descent + line_gap,
                })
            }
        }
    }

    pub fn metrics(&self, c: char, px: f32) -> fontdue::Metrics {
        match self {
            Self::Static(font) => font.metrics(c, px),
            Self::Variable { font, .. } => {
                let scale = px_to_ab_scale(font, px);
                let sf = ab_glyph::Font::as_scaled(font, scale);
                let glyph_id = sf.glyph_id(c);
                let advance_width = sf.h_advance(glyph_id);
                let advance_height = sf.v_advance(glyph_id);
                let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
                if let Some(outlined) = sf.outline_glyph(glyph) {
                    let px_bounds = outlined.px_bounds();
                    let width = px_bounds.width().max(0.0) as usize;
                    let height = px_bounds.height().max(0.0) as usize;
                    let xmin = px_bounds.min.x as i32;
                    let ymin = (-px_bounds.max.y) as i32;
                    fontdue::Metrics {
                        xmin,
                        ymin,
                        width,
                        height,
                        advance_width,
                        advance_height,
                        bounds: fontdue::OutlineBounds {
                            xmin: px_bounds.min.x,
                            ymin: -px_bounds.max.y,
                            width: px_bounds.width().max(0.0),
                            height: px_bounds.height().max(0.0),
                        },
                    }
                } else {
                    fontdue::Metrics {
                        advance_width,
                        advance_height,
                        ..Default::default()
                    }
                }
            }
        }
    }

    pub fn rasterize(&self, c: char, px: f32) -> (fontdue::Metrics, Vec<u8>) {
        match self {
            Self::Static(font) => font.rasterize(c, px),
            Self::Variable { font, .. } => {
                let scale = px_to_ab_scale(font, px);
                let sf = ab_glyph::Font::as_scaled(font, scale);
                let glyph_id = sf.glyph_id(c);
                let advance_width = sf.h_advance(glyph_id);
                let advance_height = sf.v_advance(glyph_id);
                let glyph = glyph_id.with_scale_and_position(scale, ab_glyph::point(0.0, 0.0));
                if let Some(outlined) = sf.outline_glyph(glyph) {
                    let px_bounds = outlined.px_bounds();
                    let width = px_bounds.width().max(0.0) as usize;
                    let height = px_bounds.height().max(0.0) as usize;
                    let xmin = px_bounds.min.x as i32;
                    let ymin = (-px_bounds.max.y) as i32;
                    let mut bitmap = vec![0u8; width * height];
                    outlined.draw(|x, y, cov| {
                        let idx = (y as usize) * width + (x as usize);
                        if idx < bitmap.len() {
                            bitmap[idx] = (cov.clamp(0.0, 1.0) * 255.0).round() as u8;
                        }
                    });
                    let metrics = fontdue::Metrics {
                        xmin,
                        ymin,
                        width,
                        height,
                        advance_width,
                        advance_height,
                        bounds: fontdue::OutlineBounds {
                            xmin: px_bounds.min.x,
                            ymin: -px_bounds.max.y,
                            width: px_bounds.width().max(0.0),
                            height: px_bounds.height().max(0.0),
                        },
                    };
                    (metrics, bitmap)
                } else {
                    (
                        fontdue::Metrics {
                            advance_width,
                            advance_height,
                            ..Default::default()
                        },
                        Vec::new(),
                    )
                }
            }
        }
    }
}

/// Text weight/slant style combination for font face resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontStyle {
    Regular,
    Bold,
    Italic,
    BoldItalic,
}

impl FontStyle {
    #[inline]
    pub const fn from_flags(is_bold: bool, is_italic: bool) -> Self {
        match (is_bold, is_italic) {
            (false, false) => Self::Regular,
            (true, false) => Self::Bold,
            (false, true) => Self::Italic,
            (true, true) => Self::BoldItalic,
        }
    }
}

/// A fallback font entry holding Regular, Bold, Italic, and Bold+Italic faces.
pub struct FallbackFont {
    pub regular: FontFace,
    pub bold: Option<FontFace>,
    pub italic: Option<FontFace>,
    pub bold_italic: Option<FontFace>,
}

impl FallbackFont {
    /// Select the best available face for `(is_bold, is_italic)` and report whether the
    /// returned face is natively italic (so callers know whether synthetic slant is needed).
    pub fn select_face(&self, is_bold: bool, is_italic: bool) -> (&FontFace, bool) {
        match FontStyle::from_flags(is_bold, is_italic) {
            FontStyle::BoldItalic => {
                if let Some(bi) = self.bold_italic.as_ref() {
                    (bi, true)
                } else if let Some(b) = self.bold.as_ref() {
                    (b, false)
                } else if let Some(i) = self.italic.as_ref() {
                    (i, true)
                } else {
                    (&self.regular, false)
                }
            }
            FontStyle::Bold => (self.bold.as_ref().unwrap_or(&self.regular), false),
            FontStyle::Italic => {
                if let Some(i) = self.italic.as_ref() {
                    (i, true)
                } else {
                    (&self.regular, false)
                }
            }
            FontStyle::Regular => (&self.regular, false),
        }
    }
}

/// Decoded RGBA8 color glyph bitmap ready for upload to the color emoji texture atlas.
#[derive(Debug, Clone)]
pub struct DecodedColorGlyph {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Platform color emoji font backed by embedded bitmap strikes (`sbix` / `CBDT` / `EBDT`)
/// decoded via `ttf-parser` and `image`.
#[derive(Clone)]
pub struct ColorEmojiFont {
    data: Arc<Vec<u8>>,
    face_index: u32,
}

const SYSTEM_COLOR_EMOJI_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Apple Color Emoji.ttc",
    // Linux
    "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
    "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
    "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
    "/usr/share/fonts/NotoColorEmoji.ttf",
    "/usr/share/fonts/TTF/NotoColorEmoji.ttf",
    // Windows
    "C:\\Windows\\Fonts\\seguiemj.ttf",
];

static SYSTEM_COLOR_EMOJI_CACHE: OnceLock<Option<ColorEmojiFont>> = OnceLock::new();

impl ColorEmojiFont {
    /// Load a color emoji font from `path`, locating the first collection face containing
    /// embedded PNG bitmap strikes (`sbix` / `CBDT` / `EBDT`).
    pub fn from_path(path: impl AsRef<Path>) -> Option<Self> {
        let bytes = std::fs::read(path.as_ref()).ok()?;
        Self::from_bytes(Arc::new(bytes))
    }

    /// Parse a color emoji font from shared font bytes.
    pub fn from_bytes(data: Arc<Vec<u8>>) -> Option<Self> {
        let face_count = ttf_parser::fonts_in_collection(&data).unwrap_or(1);
        for idx in 0..face_count {
            if let Ok(face) = ttf_parser::Face::parse(&data, idx) {
                let probe_gid = face
                    .glyph_index('😀')
                    .or_else(|| face.glyph_index('🔥'))
                    .or_else(|| face.glyph_index('❤'));
                if let Some(gid) = probe_gid {
                    if face
                        .glyph_raster_image(gid, 64)
                        .is_some_and(|img| img.format == ttf_parser::RasterImageFormat::PNG)
                    {
                        return Some(Self {
                            data,
                            face_index: idx,
                        });
                    }
                }
            }
        }
        None
    }

    /// Resolve a `ttf_parser::GlyphId` for a single Unicode scalar or multi-codepoint emoji
    /// grapheme (handling `VS16` variation selectors and OpenType `GSUB` ligature chains).
    fn resolve_glyph_id(
        &self,
        face: &ttf_parser::Face<'_>,
        text: &str,
    ) -> Option<ttf_parser::GlyphId> {
        let chars: Vec<char> = text.chars().collect();
        if chars.is_empty() {
            return None;
        }
        // VS15 (`U+FE0E`) explicitly requests monochrome text presentation.
        if chars.contains(&'\u{FE0E}') {
            return None;
        }

        if chars.len() == 1 {
            return face.glyph_index(chars[0]);
        }

        if chars.len() == 2 && chars[1] == '\u{FE0F}' {
            return face
                .glyph_variation_index(chars[0], '\u{FE0F}')
                .or_else(|| face.glyph_index(chars[0]));
        }

        // Collect glyph IDs for OpenType GSUB ligature matching (used by NotoColorEmoji).
        let gids_full: Option<Vec<ttf_parser::GlyphId>> =
            chars.iter().map(|&c| face.glyph_index(c)).collect();
        if let Some(ref gids) = gids_full {
            if let Some(lig_gid) = resolve_gsub_ligature(face, gids) {
                return Some(lig_gid);
            }
        }

        // Retry GSUB matching with VS16 stripped (some emoji fonts omit FE0F in ligatures).
        if chars.contains(&'\u{FE0F}') {
            let gids_no_vs: Option<Vec<ttf_parser::GlyphId>> = chars
                .iter()
                .filter(|&&c| c != '\u{FE0F}')
                .map(|&c| face.glyph_index(c))
                .collect();
            if let Some(ref gids) = gids_no_vs {
                if gids.len() == 1 {
                    return Some(gids[0]);
                }
                if let Some(lig_gid) = resolve_gsub_ligature(face, gids) {
                    return Some(lig_gid);
                }
            }
        }

        // Graceful fallback: render the leading base emoji's color strike instead of '?'.
        for &c in &chars {
            if c == '\u{200D}' || c == '\u{FE0F}' {
                continue;
            }
            if let Some(gid) = face
                .glyph_variation_index(c, '\u{FE0F}')
                .or_else(|| face.glyph_index(c))
            {
                return Some(gid);
            }
        }
        None
    }

    /// Decode a single emoji character into an RGBA8 bitmap scaled cleanly to fit
    /// `(max_w, max_h)` while preserving aspect ratio.
    pub fn decode_char_rgba(
        &self,
        c: char,
        ppem: u16,
        max_w: u32,
        max_h: u32,
    ) -> Option<DecodedColorGlyph> {
        let mut buf = [0u8; 4];
        self.decode_grapheme_rgba(c.encode_utf8(&mut buf), ppem, max_w, max_h)
    }

    /// Decode an emoji character or multi-codepoint grapheme cluster into a scaled RGBA8 bitmap.
    pub fn decode_grapheme_rgba(
        &self,
        text: &str,
        ppem: u16,
        max_w: u32,
        max_h: u32,
    ) -> Option<DecodedColorGlyph> {
        if max_w == 0 || max_h == 0 {
            return None;
        }
        let face = ttf_parser::Face::parse(&self.data, self.face_index).ok()?;
        let gid = self.resolve_glyph_id(&face, text)?;
        let raster = face.glyph_raster_image(gid, ppem.max(16))?;
        if raster.format != ttf_parser::RasterImageFormat::PNG {
            return None;
        }

        let img = image::load_from_memory(raster.data).ok()?;
        let rgba = img.to_rgba8();
        let (src_w, src_h) = rgba.dimensions();
        if src_w == 0 || src_h == 0 {
            return None;
        }

        let scale = (max_w as f32 / src_w as f32).min(max_h as f32 / src_h as f32);
        let dst_w = ((src_w as f32 * scale).round() as u32).clamp(1, max_w);
        let dst_h = ((src_h as f32 * scale).round() as u32).clamp(1, max_h);

        let pixels = if dst_w == src_w && dst_h == src_h {
            rgba.into_raw()
        } else {
            image::imageops::resize(&rgba, dst_w, dst_h, image::imageops::FilterType::Lanczos3)
                .into_raw()
        };

        Some(DecodedColorGlyph {
            width: dst_w,
            height: dst_h,
            rgba: pixels,
        })
    }
}

/// Walk OpenType GSUB Type-4 (Ligature Substitution) lookups to resolve multi-codepoint emoji
/// glyph sequences (e.g. flags, skin tones, ZWJ families in `NotoColorEmoji.ttf`).
fn resolve_gsub_ligature(
    face: &ttf_parser::Face<'_>,
    gids: &[ttf_parser::GlyphId],
) -> Option<ttf_parser::GlyphId> {
    if gids.len() < 2 {
        return None;
    }
    let gsub = face.tables().gsub?;
    let first = gids[0];
    let tail = &gids[1..];
    for lookup in gsub.lookups.into_iter() {
        for subtable in lookup
            .subtables
            .into_iter::<ttf_parser::gsub::SubstitutionSubtable<'_>>()
        {
            if let ttf_parser::gsub::SubstitutionSubtable::Ligature(lig_sub) = subtable {
                if let Some(cov_idx) = lig_sub.coverage.get(first) {
                    if let Some(set) = lig_sub.ligature_sets.get(cov_idx) {
                        for lig in set.into_iter() {
                            if lig.components.len() as usize == tail.len()
                                && lig
                                    .components
                                    .into_iter()
                                    .zip(tail.iter())
                                    .all(|(a, &b)| a == b)
                            {
                                return Some(lig.glyph);
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Load (and process-wide cache) the platform color emoji font lazily so the ~192 MiB
/// `Apple Color Emoji.ttc` is read at most once and shared across all windows and font zooms.
pub fn load_system_color_emoji_font() -> Option<ColorEmojiFont> {
    SYSTEM_COLOR_EMOJI_CACHE
        .get_or_init(|| {
            for path in SYSTEM_COLOR_EMOJI_CANDIDATES {
                if let Some(font) = ColorEmojiFont::from_path(path) {
                    info!("Loaded system color emoji font: {}", path);
                    return Some(font);
                }
            }
            None
        })
        .clone()
}

/// Returns `true` for Unicode codepoints whose default presentation is Emoji_Presentation
/// (so they render in full color rather than monochrome outlines, while leaving text-default
/// symbols like `#`, `0-9`, `⚙`, `⌘`, `▶`, `✓` crisp monochrome unless paired with `U+FE0F`).
#[inline]
pub fn is_default_emoji_presentation(c: char) -> bool {
    let cp = c as u32;
    matches!(
        cp,
        0x231A..=0x231B
            | 0x23E9..=0x23EC
            | 0x23F0
            | 0x23F3
            | 0x25FD..=0x25FE
            | 0x2614..=0x2615
            | 0x2648..=0x2653
            | 0x267F
            | 0x2693
            | 0x26A1
            | 0x26AA..=0x26AB
            | 0x26BD..=0x26BE
            | 0x26C4..=0x26C5
            | 0x26CE
            | 0x26D4
            | 0x26EA
            | 0x26F2..=0x26F3
            | 0x26F5
            | 0x26FA
            | 0x26FD
            | 0x2705
            | 0x270A..=0x270B
            | 0x2728
            | 0x274C
            | 0x274E
            | 0x2753..=0x2755
            | 0x2757
            | 0x2795..=0x2797
            | 0x27B0
            | 0x27BF
            | 0x2B1B..=0x2B1C
            | 0x2B50
            | 0x2B55
            | 0x1F004
            | 0x1F0CF
            | 0x1F1E6..=0x1F1FF
            | 0x1F201
            | 0x1F21A
            | 0x1F22F
            | 0x1F232..=0x1F236
            | 0x1F238..=0x1F23A
            | 0x1F250..=0x1F251
            | 0x1F300..=0x1FAFF
    )
}

/// Returns `true` if a multi-codepoint grapheme should prefer the color emoji atlas.
#[inline]
pub fn is_color_emoji_grapheme(text: &str) -> bool {
    let mut has_vs15 = false;
    let mut has_emoji_cue = false;
    let mut regional_count = 0usize;

    for c in text.chars() {
        let cp = c as u32;
        if cp == 0xFE0E {
            has_vs15 = true;
        } else if cp == 0xFE0F
            || cp == 0x200D
            || cp == 0x20E3
            || (0x1F3FB..=0x1F3FF).contains(&cp)
            || is_default_emoji_presentation(c)
        {
            has_emoji_cue = true;
        }
        if (0x1F1E6..=0x1F1FF).contains(&cp) {
            regional_count += 1;
        }
    }

    !has_vs15 && (has_emoji_cue || regional_count >= 2)
}

/// Key identifying a cached single-codepoint glyph in the texture atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub c: char,
    pub is_bold: bool,
    pub is_italic: bool,
}

/// Key identifying a cached multi-codepoint grapheme cluster in the texture atlas.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GraphemeKey {
    pub text: String,
    pub is_bold: bool,
    pub is_italic: bool,
    pub target_cells: u8,
}

/// Metadata, UV bounds, color flag, and typographic advance width of an allocated glyph.
#[derive(Debug, Clone, Copy)]
pub struct GlyphInfo {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [f32; 2],
    pub offset: [f32; 2],
    pub advance: f32,
    pub is_color: bool,
}

/// 2D dynamic texture atlas pair: monochrome `R8Unorm` for text glyphs and `Rgba8Unorm` for
/// full-color emoji bitmaps, with deferred frame-boundary overflow resets to prevent mid-frame
/// UV corruption.
pub struct GlyphAtlas {
    pub texture: Texture,
    pub texture_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub width: u32,
    pub height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    pub color_texture: Texture,
    pub color_texture_view: wgpu::TextureView,
    pub color_sampler: wgpu::Sampler,
    pub color_width: u32,
    pub color_height: u32,
    color_cursor_x: u32,
    color_cursor_y: u32,
    color_row_height: u32,
    mono_needs_clear: bool,
    color_needs_clear: bool,
    pub atlas_generation: u64,
    cache: HashMap<GlyphKey, GlyphInfo>,
    grapheme_cache: HashMap<GraphemeKey, GlyphInfo>,
    regular_font: FontFace,
    bold_font: Option<FontFace>,
    italic_font: Option<FontFace>,
    bold_italic_font: Option<FontFace>,
    fallback_fonts: Vec<FallbackFont>,
    system_fallback_cursor: usize,
    pub font_size: f32,
    pub base_cell_height: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub baseline_offset: f32,
}

const SYSTEM_CJK_CANDIDATES: &[&str] = &[
    // macOS
    "/System/Library/Fonts/Menlo.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/STHeiti Medium.ttc",
    "/System/Library/Fonts/STHeiti Light.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    "/System/Library/Fonts/Supplemental/Songti.ttc",
    "/System/Library/Fonts/PingFang.ttc",
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    "/System/Library/Fonts/Apple Symbols.ttf",
    // Linux
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
    // Windows
    "C:\\Windows\\Fonts\\msyh.ttc",
    "C:\\Windows\\Fonts\\simsun.ttc",
    "C:\\Windows\\Fonts\\msgothic.ttc",
    "C:\\Windows\\Fonts\\malgun.ttf",
];

impl GlyphAtlas {
    pub const ATLAS_SIZE: u32 = 2048;
    pub const COLOR_ATLAS_SIZE: u32 = 2048;

    pub fn new(
        device: &wgpu::Device,
        queue: &Queue,
        font_size: f32,
        custom_font_data: Option<&[u8]>,
    ) -> anyhow::Result<Self> {
        Self::new_with_family(device, queue, font_size, "Menlo", &[], custom_font_data)
    }

    pub fn new_with_family(
        device: &wgpu::Device,
        queue: &Queue,
        font_size: f32,
        font_family: &str,
        fallback_families: &[String],
        custom_font_data: Option<&[u8]>,
    ) -> anyhow::Result<Self> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("skterm_glyph_atlas"),
            size: Extent3d {
                width: Self::ATLAS_SIZE,
                height: Self::ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skterm_glyph_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("skterm_color_emoji_atlas"),
            size: Extent3d {
                width: Self::COLOR_ATLAS_SIZE,
                height: Self::COLOR_ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let color_texture_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let color_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skterm_color_emoji_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        // Load configured font chain
        let (regular_font, bold_font, italic_font, bold_italic_font, fallback_fonts) =
            if let Some(data) = custom_font_data {
                let font = FontFace::from_bytes(data.to_vec(), FontSettings::default())
                    .map_err(|e| anyhow::anyhow!("Failed to parse custom font: {}", e))?;
                let bold = load_ttc_style_face(data, true, false)
                    .or_else(|| font.with_weight(BOLD_WEIGHT));
                let italic = load_ttc_style_face(data, false, true);
                let bold_italic = load_ttc_style_face(data, true, true);
                (font, bold, italic, bold_italic, Vec::new())
            } else {
                let chain = load_font_chain(font_family, fallback_families);
                (
                    chain.regular,
                    chain.bold,
                    chain.italic,
                    chain.bold_italic,
                    chain.fallbacks,
                )
            };

        // Compute metrics for cell dimensions
        let regular_metrics = regular_font.metrics('M', font_size);
        let line_metrics = regular_font.horizontal_line_metrics(font_size);

        let cell_width = regular_metrics.advance_width.ceil().max(1.0);
        let base_cell_height = line_metrics
            .map(|m| (m.ascent - m.descent + m.line_gap).ceil())
            .unwrap_or(font_size * 1.3)
            .max(1.0);
        let cell_height = base_cell_height;

        let gt_metric = regular_font.metrics('>', font_size);
        let baseline_offset = if gt_metric.height > 0 {
            let gt_h = gt_metric.height as f32;
            let gt_ymin = gt_metric.ymin as f32;
            let top_margin = ((cell_height - gt_h) / 2.0).round();
            (top_margin + gt_h + gt_ymin).clamp(1.0, cell_height - 1.0)
        } else if regular_metrics.height > 0 {
            let cap_h = regular_metrics.height as f32;
            let cap_ymin = regular_metrics.ymin as f32;
            let top_margin = ((cell_height - cap_h) / 2.0).round();
            (top_margin + cap_h + cap_ymin).clamp(1.0, cell_height - 1.0)
        } else {
            let font_ascent = line_metrics.map(|m| m.ascent).unwrap_or(font_size);
            font_ascent.round().clamp(1.0, cell_height - 1.0)
        };

        info!(
            "Font Engine Loaded: family='{}', size={}, cell_w={}, cell_h={}, bold={}, italic={}, bold_italic={}, fallbacks_count={}",
            font_family,
            font_size,
            cell_width,
            cell_height,
            bold_font.is_some(),
            italic_font.is_some(),
            bold_italic_font.is_some(),
            fallback_fonts.len()
        );

        let mut atlas = Self {
            texture,
            texture_view,
            sampler,
            width: Self::ATLAS_SIZE,
            height: Self::ATLAS_SIZE,
            cursor_x: 1,
            cursor_y: 1,
            row_height: 0,
            color_texture,
            color_texture_view,
            color_sampler,
            color_width: Self::COLOR_ATLAS_SIZE,
            color_height: Self::COLOR_ATLAS_SIZE,
            color_cursor_x: 1,
            color_cursor_y: 1,
            color_row_height: 0,
            mono_needs_clear: false,
            color_needs_clear: false,
            atlas_generation: 0,
            cache: HashMap::new(),
            grapheme_cache: HashMap::new(),
            regular_font,
            bold_font,
            italic_font,
            bold_italic_font,
            fallback_fonts,
            system_fallback_cursor: 0,
            font_size,
            base_cell_height,
            cell_width,
            cell_height,
            baseline_offset,
        };

        // Pre-warm common printable ASCII glyphs (32..=126) so typing never hits on-demand rasterization lag
        for ch in 32u8..=126u8 {
            let c = ch as char;
            let _ = atlas.get_or_insert_glyph(queue, c, false, false);
            let _ = atlas.get_or_insert_glyph(queue, c, true, false);
        }

        Ok(atlas)
    }

    /// Flush any deferred atlas clears at the start of a new frame before instance UVs are built,
    /// avoiding mid-frame texture overwrites that would corrupt already-queued glyph quads.
    pub fn begin_frame(&mut self) {
        if self.mono_needs_clear {
            warn!("Monochrome glyph atlas full; resetting cache at frame boundary.");
            self.cache.retain(|_, info| info.is_color);
            self.grapheme_cache.retain(|_, info| info.is_color);
            self.cursor_x = 1;
            self.cursor_y = 1;
            self.row_height = 0;
            self.mono_needs_clear = false;
            self.atlas_generation = self.atlas_generation.wrapping_add(1);
        }
        if self.color_needs_clear {
            warn!("Color emoji atlas full; resetting cache at frame boundary.");
            self.cache.retain(|_, info| !info.is_color);
            self.grapheme_cache.retain(|_, info| !info.is_color);
            self.color_cursor_x = 1;
            self.color_cursor_y = 1;
            self.color_row_height = 0;
            self.color_needs_clear = false;
            self.atlas_generation = self.atlas_generation.wrapping_add(1);
        }
    }

    /// Returns `true` if either texture atlas overflowed during the current frame and will reset
    /// cleanly at the next `begin_frame()`.
    #[inline]
    pub fn needs_redraw_for_atlas_reset(&self) -> bool {
        self.mono_needs_clear || self.color_needs_clear
    }

    /// Update vertical cell height and baseline offset based on the user's `line_height` multiplier
    /// (where `1.2` is the standard baseline 1.0x cell height).
    pub fn set_line_height(&mut self, line_height: f32) {
        let ratio = (line_height.clamp(0.8, 2.5)) / 1.2;
        self.cell_height = (self.base_cell_height * ratio).round().max(1.0);
        let regular_metrics = self.regular_font.metrics('M', self.font_size);
        let line_metrics = self.regular_font.horizontal_line_metrics(self.font_size);
        let gt_metric = self.regular_font.metrics('>', self.font_size);
        self.baseline_offset = if gt_metric.height > 0 {
            let gt_h = gt_metric.height as f32;
            let gt_ymin = gt_metric.ymin as f32;
            let top_margin = ((self.cell_height - gt_h) / 2.0).round();
            (top_margin + gt_h + gt_ymin).clamp(1.0, self.cell_height - 1.0)
        } else if regular_metrics.height > 0 {
            let cap_h = regular_metrics.height as f32;
            let cap_ymin = regular_metrics.ymin as f32;
            let top_margin = ((self.cell_height - cap_h) / 2.0).round();
            (top_margin + cap_h + cap_ymin).clamp(1.0, self.cell_height - 1.0)
        } else {
            let font_ascent = line_metrics.map(|m| m.ascent).unwrap_or(self.font_size);
            font_ascent.round().clamp(1.0, self.cell_height - 1.0)
        };
    }

    /// Select the best primary face for `(is_bold, is_italic)` and whether it is natively italic.
    fn select_primary_face(&self, is_bold: bool, is_italic: bool) -> (&FontFace, bool) {
        match FontStyle::from_flags(is_bold, is_italic) {
            FontStyle::BoldItalic => {
                if let Some(bi) = self.bold_italic_font.as_ref() {
                    (bi, true)
                } else if let Some(b) = self.bold_font.as_ref() {
                    (b, false)
                } else if let Some(i) = self.italic_font.as_ref() {
                    (i, true)
                } else {
                    (&self.regular_font, false)
                }
            }
            FontStyle::Bold => (self.bold_font.as_ref().unwrap_or(&self.regular_font), false),
            FontStyle::Italic => {
                if let Some(i) = self.italic_font.as_ref() {
                    (i, true)
                } else {
                    (&self.regular_font, false)
                }
            }
            FontStyle::Regular => (&self.regular_font, false),
        }
    }

    /// Lazily load the next available system CJK / Unicode fallback font on demand.
    fn load_next_system_fallback(&mut self) -> Option<usize> {
        while self.system_fallback_cursor < SYSTEM_CJK_CANDIDATES.len() {
            let path = SYSTEM_CJK_CANDIDATES[self.system_fallback_cursor];
            self.system_fallback_cursor += 1;
            if let Ok(bytes) = std::fs::read(path) {
                let ttc_bold = load_ttc_style_face(&bytes, true, false);
                let ttc_italic = load_ttc_style_face(&bytes, false, true);
                let ttc_bold_italic = load_ttc_style_face(&bytes, true, true);
                if let Ok(regular) = FontFace::from_bytes(bytes, FontSettings::default()) {
                    info!("Loaded system CJK fallback font: {}", path);
                    let bold = ttc_bold.or_else(|| regular.with_weight(BOLD_WEIGHT));
                    let idx = self.fallback_fonts.len();
                    self.fallback_fonts.push(FallbackFont {
                        regular,
                        bold,
                        italic: ttc_italic,
                        bold_italic: ttc_bold_italic,
                    });
                    return Some(idx);
                }
            }
        }
        None
    }

    /// Allocate and upload a monochrome R8 bitmap into the monochrome atlas.
    fn upload_mono_bitmap(
        &mut self,
        queue: &Queue,
        metrics: &fontdue::Metrics,
        bitmap: &[u8],
        advance: f32,
    ) -> Option<GlyphInfo> {
        let glyph_w = metrics.width as u32;
        let glyph_h = metrics.height as u32;

        if glyph_w == 0 || glyph_h == 0 {
            return Some(GlyphInfo {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size: [0.0, 0.0],
                offset: [0.0, 0.0],
                advance,
                is_color: false,
            });
        }

        if self.cursor_x + glyph_w + 1 >= self.width {
            self.cursor_x = 1;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        if self.cursor_y + glyph_h + 1 >= self.height {
            self.mono_needs_clear = true;
            return None;
        }

        let alloc_x = self.cursor_x;
        let alloc_y = self.cursor_y;

        self.cursor_x += glyph_w + 1;
        self.row_height = self.row_height.max(glyph_h);

        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: Origin3d {
                    x: alloc_x,
                    y: alloc_y,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            bitmap,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(glyph_w),
                rows_per_image: Some(glyph_h),
            },
            Extent3d {
                width: glyph_w,
                height: glyph_h,
                depth_or_array_layers: 1,
            },
        );

        let uv_min = [
            alloc_x as f32 / self.width as f32,
            alloc_y as f32 / self.height as f32,
        ];
        let uv_max = [
            (alloc_x + glyph_w) as f32 / self.width as f32,
            (alloc_y + glyph_h) as f32 / self.height as f32,
        ];

        Some(GlyphInfo {
            uv_min,
            uv_max,
            size: [glyph_w as f32, glyph_h as f32],
            offset: [metrics.xmin as f32, metrics.ymin as f32],
            advance,
            is_color: false,
        })
    }

    /// Allocate and upload an RGBA8 color emoji bitmap into the color atlas, centered inside
    /// `target_cells * cell_width` horizontally and `cell_height` vertically.
    fn upload_color_bitmap(
        &mut self,
        queue: &Queue,
        decoded: &DecodedColorGlyph,
        target_cells: usize,
    ) -> Option<GlyphInfo> {
        let glyph_w = decoded.width;
        let glyph_h = decoded.height;
        if glyph_w == 0 || glyph_h == 0 {
            return None;
        }

        if self.color_cursor_x + glyph_w + 1 >= self.color_width {
            self.color_cursor_x = 1;
            self.color_cursor_y += self.color_row_height + 1;
            self.color_row_height = 0;
        }

        if self.color_cursor_y + glyph_h + 1 >= self.color_height {
            self.color_needs_clear = true;
            return None;
        }

        let alloc_x = self.color_cursor_x;
        let alloc_y = self.color_cursor_y;

        self.color_cursor_x += glyph_w + 1;
        self.color_row_height = self.color_row_height.max(glyph_h);

        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &self.color_texture,
                mip_level: 0,
                origin: Origin3d {
                    x: alloc_x,
                    y: alloc_y,
                    z: 0,
                },
                aspect: TextureAspect::All,
            },
            &decoded.rgba,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(glyph_w * 4),
                rows_per_image: Some(glyph_h),
            },
            Extent3d {
                width: glyph_w,
                height: glyph_h,
                depth_or_array_layers: 1,
            },
        );

        let uv_min = [
            alloc_x as f32 / self.color_width as f32,
            alloc_y as f32 / self.color_height as f32,
        ];
        let uv_max = [
            (alloc_x + glyph_w) as f32 / self.color_width as f32,
            (alloc_y + glyph_h) as f32 / self.color_height as f32,
        ];

        let span_w = (target_cells.max(1) as f32 * self.cell_width).max(glyph_w as f32);
        let offset_x = ((span_w - glyph_w as f32) * 0.5).round();
        // `renderer.rs` computes `glyph_y = y + baseline - offset[1] - size[1]`.
        // Setting `offset[1]` below centers the bitmap vertically within `[y, y + cell_height]`.
        let top_pad = ((self.cell_height - glyph_h as f32) * 0.5).round();
        let offset_y = self.baseline_offset - glyph_h as f32 - top_pad;

        Some(GlyphInfo {
            uv_min,
            uv_max,
            size: [glyph_w as f32, glyph_h as f32],
            offset: [offset_x, offset_y],
            advance: span_w,
            is_color: true,
        })
    }

    /// Attempt to rasterize `c` via the monochrome primary + fallback font chain.
    /// Returns `(metrics, bitmap, found_in_font)`.
    fn rasterize_mono_char(
        &mut self,
        c: char,
        is_bold: bool,
        is_italic: bool,
    ) -> Option<(fontdue::Metrics, Vec<u8>)> {
        // 1. Check primary font
        let (primary, primary_is_italic) = self.select_primary_face(is_bold, is_italic);
        if primary.lookup_glyph_index(c) != 0 {
            let (m, b) = rasterize_fitted_glyph(
                primary,
                c,
                self.font_size,
                self.cell_width,
                self.cell_height,
            );
            return Some(if is_italic && !primary_is_italic {
                shear_mono_bitmap(m, b)
            } else {
                (m, b)
            });
        }

        let target_cells = unicode_width::UnicodeWidthChar::width(c)
            .unwrap_or(1)
            .max(1);
        let mut found: Option<(fontdue::Metrics, Vec<u8>, bool)> = None;

        // For single-cell characters (e.g. `⌘`, `⇧`, `⌥`, `⌃`, `↩`), prefer a fallback
        // font whose glyph is natively single-cell (`advance_width <= cell_width * 1.25`)
        // over a CJK font where the same codepoint is drawn as a 2-cell full-width glyph.
        if target_cells == 1 {
            for fallback in &self.fallback_fonts {
                let (face, face_italic) = fallback.select_face(is_bold, is_italic);
                if face.lookup_glyph_index(c) != 0 {
                    let m = face.metrics(c, self.font_size);
                    if m.advance_width <= self.cell_width * 1.25 {
                        let (rm, rb) = rasterize_fitted_glyph(
                            face,
                            c,
                            self.font_size,
                            self.cell_width,
                            self.cell_height,
                        );
                        found = Some((rm, rb, face_italic));
                        break;
                    }
                }
            }

            if found.is_none() && self.system_fallback_cursor == 0 {
                if let Some(idx) = self.load_next_system_fallback() {
                    let fallback = &self.fallback_fonts[idx];
                    let (face, face_italic) = fallback.select_face(is_bold, is_italic);
                    if face.lookup_glyph_index(c) != 0 {
                        let m = face.metrics(c, self.font_size);
                        if m.advance_width <= self.cell_width * 1.25 {
                            let (rm, rb) = rasterize_fitted_glyph(
                                face,
                                c,
                                self.font_size,
                                self.cell_width,
                                self.cell_height,
                            );
                            found = Some((rm, rb, face_italic));
                        }
                    }
                }
            }
        }

        // 2. Check already-loaded fallback chain
        if found.is_none() {
            for fallback in &self.fallback_fonts {
                let (face, face_italic) = fallback.select_face(is_bold, is_italic);
                if face.lookup_glyph_index(c) != 0 {
                    let (rm, rb) = rasterize_fitted_glyph(
                        face,
                        c,
                        self.font_size,
                        self.cell_width,
                        self.cell_height,
                    );
                    found = Some((rm, rb, face_italic));
                    break;
                }
            }
        }

        // 3. Progressively load system fallback fonts until one covers `c`
        while found.is_none() {
            let Some(idx) = self.load_next_system_fallback() else {
                break;
            };
            let fallback = &self.fallback_fonts[idx];
            let (face, face_italic) = fallback.select_face(is_bold, is_italic);
            if face.lookup_glyph_index(c) != 0 {
                let (rm, rb) = rasterize_fitted_glyph(
                    face,
                    c,
                    self.font_size,
                    self.cell_width,
                    self.cell_height,
                );
                found = Some((rm, rb, face_italic));
            }
        }

        found.map(|(m, b, face_italic)| {
            if is_italic && !face_italic {
                shear_mono_bitmap(m, b)
            } else {
                (m, b)
            }
        })
    }

    /// Try decoding an emoji string into the color emoji atlas.
    fn try_insert_color_emoji(
        &mut self,
        queue: &Queue,
        text: &str,
        target_cells: usize,
    ) -> Option<GlyphInfo> {
        let emoji_font = load_system_color_emoji_font()?;
        let cells = target_cells.max(1);
        let max_w = ((cells as f32 * self.cell_width).floor() as u32).max(1);
        let max_h = (self.cell_height.floor() as u32).max(1);
        let ppem = (self.font_size.ceil().clamp(16.0, 256.0)) as u16;
        let decoded = emoji_font.decode_grapheme_rgba(text, ppem, max_w, max_h)?;
        self.upload_color_bitmap(queue, &decoded, cells)
    }

    /// Retrieve glyph from cache or rasterize (routing default-presentation color emoji to the
    /// RGBA8 color atlas and regular text to the R8 monochrome atlas) and upload to GPU texture.
    pub fn get_or_insert_glyph(
        &mut self,
        queue: &Queue,
        c: char,
        is_bold: bool,
        is_italic: bool,
    ) -> Option<GlyphInfo> {
        let key = GlyphKey {
            c,
            is_bold,
            is_italic,
        };

        if let Some(&info) = self.cache.get(&key) {
            return Some(info);
        }

        let target_cells = unicode_width::UnicodeWidthChar::width(c)
            .unwrap_or(1)
            .max(1);

        // 1. Default-presentation color emoji (e.g. 😀, 🔥, 🚀, ✅, ⚡, 🦀) -> RGBA color atlas first
        if is_default_emoji_presentation(c) {
            let mut buf = [0u8; 4];
            let s = c.encode_utf8(&mut buf);
            if let Some(info) = self.try_insert_color_emoji(queue, s, target_cells) {
                self.cache.insert(key, info);
                return Some(info);
            }
        }

        // 2. Monochrome primary + fallback font chain
        let mono_res = self.rasterize_mono_char(c, is_bold, is_italic);

        // 3. Last-resort color emoji check for non-ASCII chars missing from all mono fonts
        let (metrics, bitmap) = match mono_res {
            Some(res) => res,
            None => {
                if !c.is_ascii() {
                    let mut buf = [0u8; 4];
                    let s = c.encode_utf8(&mut buf);
                    if let Some(info) = self.try_insert_color_emoji(queue, s, target_cells) {
                        self.cache.insert(key, info);
                        return Some(info);
                    }
                }
                self.regular_font.rasterize('?', self.font_size)
            }
        };

        let advance = if metrics.advance_width > 0.0 {
            metrics.advance_width
        } else {
            target_cells as f32 * self.cell_width
        };

        let info = self.upload_mono_bitmap(queue, &metrics, &bitmap, advance)?;
        self.cache.insert(key, info);
        Some(info)
    }

    /// Retrieve or rasterize a multi-codepoint grapheme cluster (e.g. `❤️` with VS16, ZWJ emoji,
    /// skin-tone modifiers, flags, or NFD base + combining marks like `e\u{0301}`).
    ///
    /// Designed as the Stage 2 integration entry point for `ScreenBuffer::cell_grapheme`.
    pub fn get_or_insert_grapheme(
        &mut self,
        queue: &Queue,
        text: &str,
        is_bold: bool,
        is_italic: bool,
        target_cells: usize,
    ) -> Option<GlyphInfo> {
        let mut chars = text.chars();
        let first = chars.next()?;
        if chars.next().is_none() {
            return self.get_or_insert_glyph(queue, first, is_bold, is_italic);
        }

        let clamped_cells = target_cells.clamp(1, 2) as u8;
        let key = GraphemeKey {
            text: text.to_string(),
            is_bold,
            is_italic,
            target_cells: clamped_cells,
        };
        if let Some(&info) = self.grapheme_cache.get(&key) {
            return Some(info);
        }

        // 1. Color emoji grapheme clusters (VS16, ZWJ, skin tones, regional indicator flags, keycaps)
        if is_color_emoji_grapheme(text) {
            if let Some(info) = self.try_insert_color_emoji(queue, text, clamped_cells as usize) {
                self.grapheme_cache.insert(key, info);
                return Some(info);
            }
        }

        // 2. Monochrome compositing for base + combining marks (e.g. NFD macOS filenames) or VS15 text
        let visible_chars: Vec<char> = text
            .chars()
            .filter(|&ch| ch != '\u{FE0E}' && ch != '\u{FE0F}' && ch != '\u{200D}')
            .collect();
        let &base_char = visible_chars.first()?;
        if visible_chars.len() == 1 {
            let info = self.get_or_insert_glyph(queue, base_char, is_bold, is_italic)?;
            self.grapheme_cache.insert(key, info);
            return Some(info);
        }

        let (base_m, base_bmp) = self
            .rasterize_mono_char(base_char, is_bold, is_italic)
            .unwrap_or_else(|| self.regular_font.rasterize(base_char, self.font_size));

        let mut layers: Vec<(fontdue::Metrics, Vec<u8>)> = Vec::with_capacity(visible_chars.len());
        layers.push((base_m, base_bmp));
        for &mark_char in &visible_chars[1..] {
            if let Some(layer) = self.rasterize_mono_char(mark_char, is_bold, is_italic) {
                if layer.0.width > 0 && layer.0.height > 0 {
                    layers.push(layer);
                }
            }
        }

        let (comp_metrics, comp_bitmap) = composite_mono_grapheme(&layers);
        let advance = if comp_metrics.advance_width > 0.0 {
            comp_metrics.advance_width
        } else {
            clamped_cells as f32 * self.cell_width
        };

        let info = self.upload_mono_bitmap(queue, &comp_metrics, &comp_bitmap, advance)?;
        self.grapheme_cache.insert(key, info);
        Some(info)
    }

    /// Typographic advance width in physical pixels for `c` when rendered in UI text.
    pub fn glyph_advance(&mut self, queue: &Queue, c: char, is_bold: bool, is_italic: bool) -> f32 {
        self.get_or_insert_glyph(queue, c, is_bold, is_italic)
            .map(|g| g.advance)
            .unwrap_or_else(|| {
                unicode_width::UnicodeWidthChar::width(c)
                    .unwrap_or(1)
                    .max(1) as f32
                    * self.cell_width
            })
    }

    /// Total typographic advance width in physical pixels for `text` when rendered in UI text.
    pub fn measure_text_width(&mut self, queue: &Queue, text: &str, is_bold: bool) -> f32 {
        text.chars()
            .map(|c| self.glyph_advance(queue, c, is_bold, false))
            .sum()
    }
}

/// Apply a subtle synthetic oblique shear (~12 degrees) to a monochrome bitmap when a font family
/// (such as `Monaco.ttf` or a CJK fallback) does not provide a native Italic face.
pub fn shear_mono_bitmap(
    mut metrics: fontdue::Metrics,
    bitmap: Vec<u8>,
) -> (fontdue::Metrics, Vec<u8>) {
    let src_w = metrics.width;
    let src_h = metrics.height;
    if src_w == 0 || src_h == 0 || bitmap.len() < src_w * src_h {
        return (metrics, bitmap);
    }

    const SLOPE: f32 = 0.21;
    let max_shift = ((src_h as f32) * SLOPE).ceil() as usize;
    if max_shift == 0 {
        return (metrics, bitmap);
    }

    let dst_w = src_w + max_shift;
    let mut dst = vec![0u8; dst_w * src_h];
    for y in 0..src_h {
        // Higher rows (smaller y in top-down bitmap) shift further right.
        let shift = ((src_h - 1 - y) as f32) * SLOPE;
        let shift_i = shift.floor() as isize;
        let frac = shift - shift.floor();
        for dx in 0..dst_w {
            let sx = dx as isize - shift_i;
            let s0 = if sx >= 0 && (sx as usize) < src_w {
                bitmap[y * src_w + sx as usize] as f32
            } else {
                0.0
            };
            let s1 = if sx > 0 && ((sx - 1) as usize) < src_w {
                bitmap[y * src_w + (sx - 1) as usize] as f32
            } else {
                0.0
            };
            dst[y * dst_w + dx] = (s0 * (1.0 - frac) + s1 * frac).round().clamp(0.0, 255.0) as u8;
        }
    }

    metrics.width = dst_w;
    metrics.xmin -= (max_shift as i32) / 3;
    (metrics, dst)
}

/// Composite a base glyph and one or more combining mark bitmaps into a single monochrome bitmap
/// aligned on the baseline (`ymin`) and centered over the base glyph.
pub fn composite_mono_grapheme(
    layers: &[(fontdue::Metrics, Vec<u8>)],
) -> (fontdue::Metrics, Vec<u8>) {
    if layers.is_empty() {
        return (fontdue::Metrics::default(), Vec::new());
    }
    if layers.len() == 1 {
        return layers[0].clone();
    }

    let base_m = layers[0].0;
    let base_center_x = base_m.xmin as f32 + base_m.width as f32 * 0.5;

    // Resolve each layer's (x0, y_bottom) in baseline-relative integer pixel coordinates.
    let mut placed: Vec<(i32, i32, usize, usize, &[u8])> = Vec::with_capacity(layers.len());
    for (idx, (m, bmp)) in layers.iter().enumerate() {
        if m.width == 0 || m.height == 0 {
            continue;
        }
        let px_x = if idx == 0 {
            m.xmin
        } else if m.xmin < 0 {
            // Combining marks with negative left side bearing anchor relative to the base advance.
            (base_m.advance_width.round() as i32) + m.xmin
        } else {
            (base_center_x - (m.width as f32) * 0.5).round() as i32
        };
        placed.push((px_x, m.ymin, m.width, m.height, bmp.as_slice()));
    }

    if placed.is_empty() {
        return (base_m, Vec::new());
    }

    let min_x = placed.iter().map(|p| p.0).min().unwrap();
    let max_x = placed.iter().map(|p| p.0 + p.2 as i32).max().unwrap();
    let min_y = placed.iter().map(|p| p.1).min().unwrap();
    let max_y = placed.iter().map(|p| p.1 + p.3 as i32).max().unwrap();

    let out_w = (max_x - min_x).max(1) as usize;
    let out_h = (max_y - min_y).max(1) as usize;
    let mut out = vec![0u8; out_w * out_h];

    for (px_x, px_ymin, w, h, bmp) in placed {
        let dst_x0 = (px_x - min_x) as usize;
        // Bitmap row 0 is the top edge (`max_y - 1` in baseline-up coordinates).
        let layer_top = px_ymin + h as i32;
        let dst_y0 = (max_y - layer_top) as usize;
        for row in 0..h {
            let dy = dst_y0 + row;
            if dy >= out_h {
                continue;
            }
            for col in 0..w {
                let dx = dst_x0 + col;
                if dx >= out_w {
                    continue;
                }
                let src_a = bmp[row * w + col];
                let dst_slot = &mut out[dy * out_w + dx];
                if src_a > *dst_slot {
                    *dst_slot = src_a;
                }
            }
        }
    }

    let mut out_metrics = base_m;
    out_metrics.xmin = min_x;
    out_metrics.ymin = min_y;
    out_metrics.width = out_w;
    out_metrics.height = out_h;
    (out_metrics, out)
}

/// Rasterize `c` using `face` at `font_size`, fitting and centering the glyph
/// within its allocated terminal cell width (`unicode_width(c) * cell_width`)
/// when the font's glyph metrics exceed the cell bounds (e.g., 1-cell modifier
/// symbols like `⌘`, `⇧`, `⌥`, `⌃`, `↩` rasterized from a full-width CJK fallback
/// font or a wide symbol glyph).
pub fn rasterize_fitted_glyph(
    face: &FontFace,
    c: char,
    font_size: f32,
    cell_width: f32,
    cell_height: f32,
) -> (fontdue::Metrics, Vec<u8>) {
    let (mut metrics, mut bitmap) = face.rasterize(c, font_size);
    if c.is_ascii() || metrics.width == 0 || metrics.height == 0 || cell_width <= 0.0 {
        return (metrics, bitmap);
    }

    let target_cells = unicode_width::UnicodeWidthChar::width(c)
        .unwrap_or(1)
        .max(1) as f32;
    let allocated_width = (cell_width * target_cells).max(1.0);

    let orig_w = metrics.width as f32;
    let orig_h = metrics.height as f32;
    let orig_xmin = metrics.xmin as f32;
    let orig_ymin = metrics.ymin as f32;
    let orig_advance = metrics.advance_width;
    let orig_center_y = orig_ymin + orig_h * 0.5;

    let advance_overflows = orig_advance > allocated_width * 1.05;

    // Glyphs authored for a single-cell monospace advance box (such as MesloLGS NF,
    // Menlo, Nerd Font icons, and Powerline seam triangles like U+E0B0/U+E0B2 which
    // intentionally overhang cell edges by ~1px to form gapless diamonds/ribbons)
    // must keep their exact native bitmap and bearings untouched.
    if !advance_overflows && orig_w <= allocated_width * 1.15 {
        return (metrics, bitmap);
    }

    // Allow symbols from wider fallback fonts (e.g. 2-cell CJK fonts) to use the
    // cell width minus a 1px separation margin so scaled symbols remain legible
    // without colliding with adjacent cells.
    let side_pad = if allocated_width >= 8.0 { 0.5 } else { 0.0 };
    let max_ink_w = (allocated_width - 2.0 * side_pad).max(1.0);
    let max_ink_h = (cell_height - 2.0).max(1.0);

    let width_overflows = orig_w > max_ink_w;
    let height_overflows = orig_h > max_ink_h;

    if width_overflows || height_overflows {
        let scale_w = if orig_w > max_ink_w {
            max_ink_w / orig_w
        } else {
            1.0
        };
        let scale_h = if orig_h > max_ink_h {
            max_ink_h / orig_h
        } else {
            1.0
        };
        let scale = scale_w.min(scale_h).clamp(0.35, 1.0);

        if scale < 0.99 {
            let mut scaled_px = (font_size * scale).max(4.0);
            // When scaling a Variable Font down to fit a narrower cell, compensate its
            // `wght` axis inversely so stroke thickness and brightness are preserved.
            let weighted_face = face
                .variable_weight()
                .and_then(|w| face.with_weight((w / scale).clamp(w, 900.0)));
            let raster_face = weighted_face.as_ref().unwrap_or(face);

            let (mut sm, mut sb) = raster_face.rasterize(c, scaled_px);

            // Guard against integer grid-fitting rounding the rasterized width 1px over `max_ink_w`
            if sm.width as f32 > max_ink_w.ceil() && sm.width > 1 {
                scaled_px = (scaled_px * (max_ink_w / sm.width as f32)).max(4.0);
                let (sm2, sb2) = raster_face.rasterize(c, scaled_px);
                if sm2.width > 0 && sm2.height > 0 {
                    sm = sm2;
                    sb = sb2;
                }
            }

            if sm.width > 0 && sm.height > 0 {
                metrics = sm;
                bitmap = sb;
                metrics.ymin = (orig_center_y - (metrics.height as f32) * 0.5).round() as i32;
            }
        }

        metrics.xmin = ((allocated_width - metrics.width as f32) * 0.5).round() as i32;
    } else if advance_overflows || orig_xmin < 0.0 || (orig_xmin + orig_w) > allocated_width {
        // Glyph ink already fits within `max_ink_w`, but its advance box or horizontal
        // bearing was authored for a wider cell; center it inside its allocated cell(s).
        metrics.xmin = ((allocated_width - orig_w) * 0.5).round() as i32;
    }

    metrics.advance_width = allocated_width;
    (metrics, bitmap)
}

fn clean_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

static FONT_FILE_INDEX: std::sync::OnceLock<Vec<(PathBuf, String)>> = std::sync::OnceLock::new();

const MAX_FONT_SCAN_DEPTH: usize = 6;

fn collect_font_files_recursive(
    dir: &std::path::Path,
    depth: usize,
    seen_paths: &mut std::collections::HashSet<PathBuf>,
    files: &mut Vec<(PathBuf, String)>,
) {
    if depth > MAX_FONT_SCAN_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();

    for path in paths {
        if path.is_dir() {
            collect_font_files_recursive(&path, depth + 1, seen_paths, files);
        } else if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "ttf" || ext_lower == "ttc" || ext_lower == "otf" {
                    // Index the stem, not the file name: the extension
                    // would otherwise count as "extra" characters when
                    // ranking candidates below.
                    if let Some(stem) = path.file_stem().and_then(|n| n.to_str()) {
                        let clean = clean_name(stem);
                        if !clean.is_empty() && seen_paths.insert(path.clone()) {
                            files.push((path, clean));
                        }
                    }
                }
            }
        }
    }
}

fn get_cached_font_index() -> &'static [(PathBuf, String)] {
    FONT_FILE_INDEX.get_or_init(|| {
        let mut search_dirs = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let home_path = PathBuf::from(home);
            search_dirs.push(home_path.join("Library/Fonts"));
            search_dirs.push(home_path.join(".local/share/fonts"));
            search_dirs.push(home_path.join(".fonts"));
        }
        if let Ok(xdg_data_home) = std::env::var("XDG_DATA_HOME") {
            let xdg_fonts = PathBuf::from(xdg_data_home).join("fonts");
            if !search_dirs.contains(&xdg_fonts) {
                search_dirs.push(xdg_fonts);
            }
        }
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            search_dirs.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Fonts"));
        }
        search_dirs.push(PathBuf::from("/Library/Fonts"));
        search_dirs.push(PathBuf::from("/System/Library/Fonts"));
        search_dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        search_dirs.push(PathBuf::from("/usr/share/fonts"));
        search_dirs.push(PathBuf::from("/usr/local/share/fonts"));
        if let Ok(xdg_data_dirs) = std::env::var("XDG_DATA_DIRS") {
            for dir in xdg_data_dirs.split(':').filter(|s| !s.is_empty()) {
                let fonts_dir = PathBuf::from(dir).join("fonts");
                if !search_dirs.contains(&fonts_dir) {
                    search_dirs.push(fonts_dir);
                }
            }
        }
        search_dirs.push(PathBuf::from("C:\\Windows\\Fonts"));

        let mut files = Vec::new();
        let mut seen_paths = std::collections::HashSet::new();
        for dir in search_dirs {
            collect_font_files_recursive(&dir, 0, &mut seen_paths, &mut files);
        }
        // Sort by file stem first so TitleCase names (e.g. "Georgia") precede
        // lowercase duplicates (e.g. "georgia") when building the family picker.
        files.sort_by(|a, b| {
            let stem_a = a.0.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let stem_b = b.0.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            stem_a.cmp(stem_b).then_with(|| a.0.cmp(&b.0))
        });
        files
    })
}

/// Style keywords that change how a face looks.
///
/// Order matters only for readability; every token is tested independently.
/// Longer spellings are listed next to the shorter ones they contain so it is
/// obvious that e.g. "semicondensed" also trips "condensed" -- that double count
/// is harmless because it only makes an unwanted face rank even lower.
const STYLE_TOKENS: &[&str] = &[
    "italic",
    "oblique",
    "thin",
    "extralight",
    "ultralight",
    "semilight",
    "light",
    "extrabold",
    "ultrabold",
    "semibold",
    "demibold",
    "bold",
    "black",
    "heavy",
    "medium",
    "ultracondensed",
    "extracondensed",
    "semicondensed",
    "condensed",
    "semiexpanded",
    "extraexpanded",
    "expanded",
    "narrow",
];

/// Count style keywords the candidate has but the query never asked for.
///
/// Returns `None` when the candidate is missing a style the query *did* ask for,
/// which is what stops an italic request from being satisfied by an upright face.
fn style_penalty(query: &str, candidate: &str) -> Option<u32> {
    let mut penalty = 0;
    for token in STYLE_TOKENS {
        let wanted = query.contains(token);
        let present = candidate.contains(token);
        if wanted && !present {
            return None;
        }
        if present && !wanted {
            penalty += 1;
        }
    }
    Some(penalty)
}

/// Rank a candidate against a cleaned query. Lower is better; `None` means the
/// candidate is not an acceptable match at all.
///
/// A family name is normally a prefix of every style in that family, so a query
/// like `notosansmnerdfontmono` matches three dozen files. Ranking keeps that from
/// being decided by `read_dir` order. Style mismatches dominate the score because
/// length alone would prefer "Italic" (6 chars) over "Regular" (7).
fn match_score(query: &str, candidate: &str) -> Option<u32> {
    let at = candidate.find(query)?;
    let penalty = style_penalty(query, candidate)?;
    let extra = (candidate.len() - query.len()) as u32;
    // Prefer the family name at the start ("Family-Regular") over buried in the
    // middle of some unrelated name.
    let position = if at == 0 { 0 } else { 1 };
    Some(penalty * 10_000 + position * 1_000 + extra)
}

/// A hard-coded path for a well-known system font, if `clean_q` names one.
///
/// Lets the common case skip the directory scan entirely on startup.
fn direct_candidate(clean_q: &str) -> Option<PathBuf> {
    let direct_candidates: &[(&str, &str)] = &[
        ("menlo", "/System/Library/Fonts/Menlo.ttc"),
        ("monaco", "/System/Library/Fonts/Monaco.ttf"),
        ("sfmono", "/System/Library/Fonts/SFMono-Regular.otf"),
        ("pingfang", "/System/Library/Fonts/PingFang.ttc"),
        ("pingfangtc", "/System/Library/Fonts/PingFang.ttc"),
        ("pingfangsc", "/System/Library/Fonts/PingFang.ttc"),
        (
            "dejavusansmono",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        ),
        (
            "notosansmono",
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
        ),
        (
            "liberationmono",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        ),
        ("consolas", "C:\\Windows\\Fonts\\consola.ttf"),
        ("microsoftyahei", "C:\\Windows\\Fonts\\msyh.ttc"),
        ("microsoftyaheiui", "C:\\Windows\\Fonts\\msyh.ttc"),
        ("microsoftjhenghei", "C:\\Windows\\Fonts\\msjh.ttc"),
        ("microsoftjhengheiui", "C:\\Windows\\Fonts\\msjh.ttc"),
    ];
    direct_candidates
        .iter()
        .find(|(name, _)| clean_q == *name)
        .map(|(_, path)| PathBuf::from(path))
        .filter(|path| path.exists())
}

/// Every acceptable file for `query`, best first.
///
/// Shared by the real font loader and by the preferences UI, so what the user is
/// shown is exactly what will be loaded.
fn ranked_candidates(query: &str) -> Vec<PathBuf> {
    let clean_q = clean_name(query);
    if clean_q.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    if let Some(path) = direct_candidate(&clean_q) {
        out.push(path);
    }

    let files = get_cached_font_index();

    // 1. Best contains match
    let mut scored: Vec<(u32, &PathBuf)> = files
        .iter()
        .filter_map(|(path, clean_f)| Some((match_score(&clean_q, clean_f)?, path)))
        .collect();

    // 2. Multi-token fallback, only if nothing matched the name directly
    if scored.is_empty() {
        let tokens: Vec<&str> = if clean_q.contains("noto") && clean_q.contains("mono") {
            vec!["noto", "mono"]
        } else if clean_q.contains("noto") && clean_q.contains("nerd") {
            vec!["noto", "nerd"]
        } else if clean_q.contains("sanstc") {
            // Anchor on "sanstc" rather than a bare "tc": "tc" also occurs inside
            // "NotoSansMNerdFont-Condensed..." ("fon-tc-ondensed"), which would pull in
            // a completely unrelated family.
            vec!["sanstc"]
        } else if clean_q.contains("sanssc") {
            // Anchor on "sanssc" rather than a bare "sc": "sc" also occurs inside
            // "NotoSans-Condensed..." ("san-sc-ondensed").
            vec!["sanssc"]
        } else if clean_q.contains("noto") && clean_q.contains("tc") {
            vec!["noto", "tc"]
        } else if clean_q.contains("cjksc") {
            vec!["noto", "cjk"]
        } else {
            vec![]
        };

        if !tokens.is_empty() {
            scored = files
                .iter()
                .filter(|(_, clean_f)| tokens.iter().all(|t| clean_f.contains(t)))
                .filter_map(|(path, clean_f)| {
                    // Same style rules as above: a loose family match must still not
                    // hand back an upright face for an italic request.
                    let penalty = style_penalty(&clean_q, clean_f)?;
                    Some((penalty * 10_000 + clean_f.len() as u32, path))
                })
                .collect();
        }
    }

    // Sort by path as the tie-break so the result does not depend on `read_dir`.
    scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
    out.extend(scored.into_iter().map(|(_, path)| path.clone()));
    out
}

/// Resolve a family name to the file that would be used, without parsing it.
///
/// Cheap enough to call while the user types: it only scores the cached index.
/// A file that turns out to be unparseable is therefore still reported here,
/// whereas [`find_font_file`] would skip to the next candidate.
pub fn resolve_font_path(query: &str) -> Option<PathBuf> {
    ranked_candidates(query).into_iter().next()
}

/// Infer the desired OpenType `wght` coordinate from a font query string.
///
/// Unstyled family names (e.g. `"NotoSansTC"`) default to [`REGULAR_WEIGHT`] (`400.0`),
/// while explicit weight suffixes (e.g. `"NotoSansTCBold"`) select their corresponding weight.
fn infer_weight_from_query(query: &str) -> f32 {
    let q = clean_name(query);
    if q.contains("thin") || q.contains("hairline") {
        100.0
    } else if q.contains("extralight") || q.contains("ultralight") {
        200.0
    } else if q.contains("semilight") {
        350.0
    } else if q.contains("light") {
        300.0
    } else if q.contains("medium") {
        500.0
    } else if q.contains("semibold") || q.contains("demibold") {
        600.0
    } else if q.contains("extrabold") || q.contains("ultrabold") {
        800.0
    } else if q.contains("black") || q.contains("heavy") {
        900.0
    } else if q.contains("bold") {
        BOLD_WEIGHT
    } else {
        REGULAR_WEIGHT
    }
}

/// Search user and system font directories for a font family name, returning the parsed [`FontFace`].
pub fn find_font_file(query: &str) -> Option<(FontFace, PathBuf)> {
    let target_weight = infer_weight_from_query(query);
    for path in ranked_candidates(query) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(font) =
                FontFace::from_bytes_with_weight(bytes, target_weight, FontSettings::default())
            {
                return Some((font, path));
            }
        }
    }
    None
}

/// Scan a TrueType Collection (`.ttc`) buffer using `ttf-parser` to locate the exact face matching
/// `(want_bold, want_italic)` (e.g. `Menlo.ttc` index 1 = Bold, 2 = Italic, 3 = Bold Italic).
fn load_ttc_style_face(bytes: &[u8], want_bold: bool, want_italic: bool) -> Option<FontFace> {
    let count = ttf_parser::fonts_in_collection(bytes)?;
    for idx in 0..count {
        if let Ok(face) = ttf_parser::Face::parse(bytes, idx) {
            let face_bold = face.is_bold();
            let face_italic = face.is_italic() || face.is_oblique();
            if face_bold == want_bold && face_italic == want_italic {
                if let Ok(loaded) = FontFace::from_bytes(
                    bytes.to_vec(),
                    FontSettings {
                        collection_index: idx,
                        ..Default::default()
                    },
                ) {
                    return Some(loaded);
                }
            }
        }
    }
    None
}

/// Locate or derive a Bold (`700`) companion face for `query` given its loaded `regular_face`.
fn load_bold_companion(
    query: &str,
    regular_face: &FontFace,
    regular_path: Option<&Path>,
) -> Option<FontFace> {
    if let Some(path) = regular_path {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ttc"))
        {
            if let Ok(bytes) = std::fs::read(path) {
                if let Some(face) = load_ttc_style_face(&bytes, true, false) {
                    return Some(face);
                }
            }
        }
    }

    let clean_q = clean_name(query);
    if clean_q == "menlo" {
        if let Some(path) = direct_candidate("menlo") {
            if let Ok(bytes) = std::fs::read(path) {
                if let Some(face) = load_ttc_style_face(&bytes, true, false) {
                    return Some(face);
                }
            }
        }
        return None;
    }

    let base_q = family_of(query.trim());
    let bold_query = format!("{}Bold", base_q);
    if let Some((b_font, _)) = find_font_file(&bold_query) {
        return Some(b_font);
    }

    if let Some(var_bold) = regular_face.with_weight(BOLD_WEIGHT) {
        return Some(var_bold);
    }

    for path in ranked_candidates(query) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(face) =
                FontFace::from_bytes_with_weight(bytes, BOLD_WEIGHT, FontSettings::default())
            {
                if matches!(face, FontFace::Variable { .. }) {
                    return Some(face);
                }
            }
        }
    }

    None
}

/// Locate an Italic / Oblique companion face for `query` (including TTC collections like `Menlo.ttc`).
fn load_italic_companion(query: &str, regular_path: Option<&Path>) -> Option<FontFace> {
    if let Some(path) = regular_path {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ttc"))
        {
            if let Ok(bytes) = std::fs::read(path) {
                if let Some(face) = load_ttc_style_face(&bytes, false, true) {
                    return Some(face);
                }
            }
        }
    }

    let base_q = family_of(query.trim());
    let italic_query = format!("{}Italic", base_q);
    if let Some((i_font, _)) = find_font_file(&italic_query) {
        return Some(i_font);
    }
    let oblique_query = format!("{}Oblique", base_q);
    if let Some((o_font, _)) = find_font_file(&oblique_query) {
        return Some(o_font);
    }
    None
}

/// Locate a Bold+Italic / Bold+Oblique companion face for `query` (including TTC collections
/// like `Menlo.ttc` and Variable Italic fonts).
fn load_bold_italic_companion(
    query: &str,
    regular_path: Option<&Path>,
    italic_face: Option<&FontFace>,
) -> Option<FontFace> {
    if let Some(path) = regular_path {
        if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("ttc"))
        {
            if let Ok(bytes) = std::fs::read(path) {
                if let Some(face) = load_ttc_style_face(&bytes, true, true) {
                    return Some(face);
                }
            }
        }
    }

    let base_q = family_of(query.trim());
    let bi_query = format!("{}BoldItalic", base_q);
    if let Some((bi_font, _)) = find_font_file(&bi_query) {
        return Some(bi_font);
    }
    let bo_query = format!("{}BoldOblique", base_q);
    if let Some((bo_font, _)) = find_font_file(&bo_query) {
        return Some(bo_font);
    }
    italic_face.and_then(|f| f.with_weight(BOLD_WEIGHT))
}

/// Distinct font family names installed on this machine, for the settings UI.
///
/// The index holds one entry per *file*, so a single family shows up dozens of
/// times ("NotoSansMNerdFontMono-SemiCondensedBlack" and friends). Strip the
/// trailing style segment and de-duplicate so the list is pickable.
pub fn installed_font_families() -> Vec<String> {
    let mut families: Vec<String> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for (path, _) in get_cached_font_index() {
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let family = family_of(stem);
        if family.is_empty() {
            continue;
        }
        if seen.insert(clean_name(&family)) {
            families.push(family);
        }
    }

    families.sort_by_key(|f| f.to_lowercase());
    families
}

/// Returns `true` if a font family name looks like a monospace, coding, Nerd Font, or CJK font
/// suitable for terminal primary or fallback use.
pub fn is_terminal_friendly_font(family: &str) -> bool {
    let lower = clean_name(family);
    const KEYWORDS: &[&str] = &[
        "mono",
        "nerd",
        "code",
        "term",
        "console",
        "consolas",
        "courier",
        "menlo",
        "meslo",
        "monaco",
        "cascadia",
        "jetbrains",
        "firacode",
        "iosevka",
        "inconsolata",
        "hack",
        "ubuntu",
        "dejavusansmono",
        "liberationmono",
        "notosansm",
        "notomono",
        "cjk",
        "sanstc",
        "sanssc",
        "sansjp",
        "sanskr",
        "pingfang",
        "hiragino",
        "wqy",
        "wenquanyi",
        "jhenghei",
        "yahei",
        "msyh",
        "simsun",
    ];
    KEYWORDS.iter().any(|k| lower.contains(k))
}

/// Strip a trailing style segment from a font file stem.
///
/// "NotoSansMNerdFontMono-SemiCondensedBlack" -> "NotoSansMNerdFontMono"
/// "NotoSansTC-VariableFont_wght"             -> "NotoSansTC"
/// "Andale Mono"                              -> "Andale Mono"
fn family_of(stem: &str) -> String {
    if let Some((head, tail)) = stem.rsplit_once('-') {
        let clean_tail = clean_name(tail);
        let is_style = clean_tail.starts_with("regular")
            || clean_tail.starts_with("variablefont")
            || clean_tail == "vf"
            || clean_tail == "roman"
            || clean_tail == "book"
            || clean_tail == "bdita"
            || clean_tail.contains("demi")
            || clean_tail.contains("hairline")
            || STYLE_TOKENS.iter().any(|t| clean_tail.contains(t));
        if is_style && !head.is_empty() {
            return head.to_string();
        }
        return stem.to_string();
    }

    // Handle underscore-separated style suffixes common in Linux msttcorefonts / TeX fonts
    // (e.g. "Courier_New_Bold_Italic" -> "Courier_New", "Aegean_hint" -> "Aegean").
    let mut current = stem;
    while let Some((head, tail)) = current.rsplit_once('_') {
        let clean_tail = clean_name(tail);
        let is_style = clean_tail == "hint" || STYLE_TOKENS.iter().any(|t| clean_tail == *t);
        if is_style && !head.is_empty() {
            current = head;
        } else {
            break;
        }
    }
    // Handle space-separated style suffixes (e.g. "MesloLGS NF Regular" -> "MesloLGS NF",
    // "MesloLGS NF Bold Italic" -> "MesloLGS NF").
    while let Some((head, tail)) = current.rsplit_once(' ') {
        let clean_tail = clean_name(tail);
        let is_style = clean_tail == "regular"
            || clean_tail == "roman"
            || clean_tail == "book"
            || STYLE_TOKENS.iter().any(|t| clean_tail == *t);
        if is_style && !head.is_empty() {
            current = head;
        } else {
            break;
        }
    }
    current.to_string()
}

/// Full resolved font chain including Bold, Italic, Bold+Italic, and Fallback families.
pub struct LoadedFontChain {
    pub regular: FontFace,
    pub bold: Option<FontFace>,
    pub italic: Option<FontFace>,
    pub bold_italic: Option<FontFace>,
    pub fallbacks: Vec<FallbackFont>,
}

/// Load full configured font chain (Primary Monospace + Bold/Italic/BoldItalic + Fallbacks).
pub fn load_font_chain(family_str: &str, fallback_families: &[String]) -> LoadedFontChain {
    let mut queries = Vec::new();

    for token in family_str.split(',') {
        let t = token.trim();
        if !t.is_empty() {
            queries.push(t.to_string());
        }
    }

    for f in fallback_families {
        let t = f.trim();
        if !t.is_empty() && !queries.contains(&t.to_string()) {
            queries.push(t.to_string());
        }
    }

    let mut primary_font: Option<FontFace> = None;
    let mut bold_font: Option<FontFace> = None;
    let mut italic_font: Option<FontFace> = None;
    let mut bold_italic_font: Option<FontFace> = None;
    let mut fallback_fonts: Vec<FallbackFont> = Vec::new();

    for q in &queries {
        if let Some((font, path)) = find_font_file(q) {
            if primary_font.is_none() {
                info!("Found primary font match for '{}': {:?}", q, path);
                bold_font = load_bold_companion(q, &font, Some(&path));
                italic_font = load_italic_companion(q, Some(&path));
                bold_italic_font = load_bold_italic_companion(q, Some(&path), italic_font.as_ref());
                primary_font = Some(font);
            } else {
                info!("Found fallback font match for '{}': {:?}", q, path);
                let bold = load_bold_companion(q, &font, Some(&path));
                let italic = load_italic_companion(q, Some(&path));
                let bold_italic = load_bold_italic_companion(q, Some(&path), italic.as_ref());
                fallback_fonts.push(FallbackFont {
                    regular: font,
                    bold,
                    italic,
                    bold_italic,
                });
            }
        }
    }

    // If primary font still not found, fallback to system standard monospace fonts
    let primary = primary_font.unwrap_or_else(|| {
        let defaults = [
            "/System/Library/Fonts/Menlo.ttc",
            "/System/Library/Fonts/Monaco.ttf",
            "/System/Library/Fonts/SFMono-Regular.otf",
            "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
            "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
            "/usr/share/fonts/truetype/noto/NotoSansMono-Regular.ttf",
            "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
            "C:\\Windows\\Fonts\\consola.ttf",
        ];
        for d in defaults {
            if let Ok(b) = std::fs::read(d) {
                let ttc_bold = load_ttc_style_face(&b, true, false);
                let ttc_italic = load_ttc_style_face(&b, false, true);
                let ttc_bold_italic = load_ttc_style_face(&b, true, true);
                if let Ok(f) = FontFace::from_bytes(b, FontSettings::default()) {
                    info!("Loaded default fallback primary monospace font: {}", d);
                    bold_font = ttc_bold.or_else(|| f.with_weight(BOLD_WEIGHT));
                    italic_font = ttc_italic;
                    bold_italic_font = ttc_bold_italic;
                    return f;
                }
            }
        }
        panic!("FATAL: No valid monospace font found on system!");
    });

    LoadedFontChain {
        regular: primary,
        bold: bold_font,
        italic: italic_font,
        bold_italic: bold_italic_font,
        fallbacks: fallback_fonts,
    }
}

/// Load full configured font chain (backwards-compatible tuple wrapper).
pub fn load_configured_font_chain(
    family_str: &str,
    fallback_families: &[String],
) -> (
    FontFace,
    Option<FontFace>,
    Option<FontFace>,
    Vec<FallbackFont>,
) {
    let chain = load_font_chain(family_str, fallback_families);
    (chain.regular, chain.bold, chain.italic, chain.fallbacks)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real filenames that caused the bug: a 36-file Nerd Font family where
    /// every name starts with the family the user configured.
    ///
    /// Deliberately ordered with the *wrong* face first, which is what
    /// `read_dir` actually returned on the machine this was reported from. A
    /// first-match-wins implementation passes only if Regular happens to come
    /// first, so the fixture must not be sorted.
    fn nerd_family() -> Vec<String> {
        [
            "NotoSansMNerdFontMono-SemiCondensedRegular",
            "NotoSansMNerdFontMono-Thin",
            "NotoSansMNerdFontMono-CondensedBold",
            "NotoSansMNerdFontMono-Regular",
            "NotoSansMNerdFontMono-Bold",
            "NotoSansMNerdFontMono-ExtraCondensedThin",
        ]
        .iter()
        .map(|s| clean_name(s))
        .collect()
    }

    fn best<'a>(query: &str, candidates: &'a [String]) -> Option<&'a str> {
        let q = clean_name(query);
        candidates
            .iter()
            .filter_map(|c| Some((match_score(&q, c)?, c.as_str())))
            .min_by_key(|(score, _)| *score)
            .map(|(_, c)| c)
    }

    #[test]
    fn family_query_prefers_regular_over_width_variants() {
        // Was picking whichever file `read_dir` happened to yield first, which on
        // the reporter's machine was SemiCondensedRegular.
        let files = nerd_family();
        assert_eq!(
            best("NotoSansMNerdFontMono", &files),
            Some(clean_name("NotoSansMNerdFontMono-Regular").as_str())
        );
    }

    #[test]
    fn bold_query_picks_the_bold_face() {
        let files = nerd_family();
        assert_eq!(
            best("NotoSansMNerdFontMonoBold", &files),
            Some(clean_name("NotoSansMNerdFontMono-Bold").as_str())
        );
    }

    #[test]
    fn bold_query_rejects_faces_without_bold() {
        let q = clean_name("NotoSansMNerdFontMonoBold");
        // Same family, wrong weight: must never be offered as the bold face.
        assert_eq!(
            style_penalty(&q, &clean_name("NotoSansMNerdFontMono-Regular")),
            None
        );
        // Right weight but an extra width axis: usable, yet it must rank behind
        // the plain Bold.
        let plain = style_penalty(&q, &clean_name("NotoSansMNerdFontMono-Bold")).unwrap();
        let condensed =
            style_penalty(&q, &clean_name("NotoSansMNerdFontMono-CondensedBold")).unwrap();
        assert_eq!(plain, 0);
        assert!(plain < condensed);
    }

    #[test]
    fn italic_query_never_matches_an_upright_face() {
        // The old code answered an italic request with an upright SemiCondensed
        // face, so italic text silently rendered straight. That came from the
        // loose token fallback, so assert on `style_penalty` too -- it is the
        // guard that path relies on, and `best` alone would not cover it.
        let q = clean_name("NotoSansMNerdFontMonoItalic");
        assert_eq!(best("NotoSansMNerdFontMonoItalic", &nerd_family()), None);
        for upright in [
            "NotoSansMNerdFontMono-Regular",
            "NotoSansMNerdFontMono-SemiCondensedRegular",
            "NotoMonoNerdFont-Regular",
        ] {
            assert_eq!(
                style_penalty(&q, &clean_name(upright)),
                None,
                "{} is not an italic face",
                upright
            );
        }
        // A genuine italic is still accepted.
        assert!(style_penalty(&q, &clean_name("NotoSansMNerdFontMono-Italic")).is_some());
    }

    #[test]
    fn unstyled_query_rejects_nothing_but_ranks_plain_first() {
        let q = clean_name("NotoSansMNerdFontMono");
        let regular = match_score(&q, &clean_name("NotoSansMNerdFontMono-Regular")).unwrap();
        for styled in [
            "NotoSansMNerdFontMono-Thin",
            "NotoSansMNerdFontMono-SemiCondensedRegular",
            "NotoSansMNerdFontMono-CondensedBold",
        ] {
            let score = match_score(&q, &clean_name(styled)).unwrap();
            assert!(regular < score, "Regular should outrank {}", styled);
        }
    }

    #[test]
    fn exact_name_beats_a_longer_superstring() {
        let q = clean_name("NotoSansTC");
        let exact = match_score(&q, &clean_name("NotoSansTC")).unwrap();
        let variable = match_score(&q, &clean_name("NotoSansTC-VariableFont_wght")).unwrap();
        assert_eq!(exact, 0);
        assert!(exact < variable);
    }

    #[test]
    fn family_buried_mid_name_ranks_below_a_prefix_match() {
        let q = clean_name("mono");
        let prefix = match_score(&q, &clean_name("Mono-Regular")).unwrap();
        let infix = match_score(&q, &clean_name("NotoSansMono")).unwrap();
        assert!(prefix < infix);
    }

    #[test]
    fn family_of_strips_only_style_segments() {
        // The picker lists one entry per family, so the style suffix has to go...
        assert_eq!(
            family_of("NotoSansMNerdFontMono-SemiCondensedBlack"),
            "NotoSansMNerdFontMono"
        );
        assert_eq!(family_of("NotoSansTC-VariableFont_wght"), "NotoSansTC");
        assert_eq!(family_of("SFMono-Regular"), "SFMono");
        assert_eq!(
            family_of("NotoSansMNerdFontMono-Italic"),
            "NotoSansMNerdFontMono"
        );
        // Linux-common suffixes (-VF, -Roman, -Book, underscore styles)
        assert_eq!(family_of("Cantarell-VF"), "Cantarell");
        assert_eq!(family_of("URWGothic-BookOblique"), "URWGothic");
        assert_eq!(family_of("Courier_New_Bold_Italic"), "Courier_New");
        assert_eq!(family_of("Times_New_Roman_Bold"), "Times_New_Roman");
        assert_eq!(family_of("Andale_Mono"), "Andale_Mono");
        assert_eq!(family_of("MesloLGS NF Regular"), "MesloLGS NF");
        assert_eq!(family_of("MesloLGS NF Bold Italic"), "MesloLGS NF");
        // ...but a dash that is part of the name itself must survive, otherwise
        // the family would be truncated to something that resolves elsewhere.
        assert_eq!(family_of("Andale Mono"), "Andale Mono");
        assert_eq!(family_of("Menlo"), "Menlo");
        assert_eq!(family_of("Noto-Kufi"), "Noto-Kufi");
    }

    #[test]
    fn family_names_round_trip_to_a_real_file() {
        // A name offered by the picker must resolve, or the UI would suggest
        // entries that silently do nothing.
        let families = installed_font_families();
        #[cfg(target_os = "linux")]
        if std::path::Path::new("/usr/share/fonts").exists() {
            assert!(
                !families.is_empty(),
                "expected to discover fonts under /usr/share/fonts subdirectories"
            );
        }
        for family in &families {
            assert!(
                resolve_font_path(family).is_some(),
                "picker offers {:?} but it resolves to nothing",
                family
            );
        }
    }

    #[test]
    fn infer_weight_defaults_to_regular_and_recognizes_styles() {
        assert_eq!(infer_weight_from_query("NotoSansTC"), REGULAR_WEIGHT);
        assert_eq!(
            infer_weight_from_query("NotoSansTC-Regular"),
            REGULAR_WEIGHT
        );
        assert_eq!(infer_weight_from_query("NotoSansTCBold"), BOLD_WEIGHT);
        assert_eq!(infer_weight_from_query("NotoSansTC-Bold"), BOLD_WEIGHT);
        assert_eq!(infer_weight_from_query("NotoSansTC-Thin"), 100.0);
        assert_eq!(infer_weight_from_query("NotoSansTC-ExtraLight"), 200.0);
        assert_eq!(infer_weight_from_query("NotoSansTC-Light"), 300.0);
        assert_eq!(infer_weight_from_query("NotoSansTC-Medium"), 500.0);
        assert_eq!(infer_weight_from_query("NotoSansTC-SemiBold"), 600.0);
        assert_eq!(infer_weight_from_query("NotoSansTC-ExtraBold"), 800.0);
        assert_eq!(infer_weight_from_query("NotoSansTC-Black"), 900.0);
    }

    #[test]
    fn fallback_font_chain_loads_regular_and_bold_for_cjk() {
        if let Some((regular, path)) = find_font_file("NotoSansTC") {
            let bold = load_bold_companion("NotoSansTC", &regular, Some(&path))
                .expect("NotoSansTC should resolve a Bold companion face");

            if let Some(w) = regular.variable_weight() {
                assert_eq!(
                    w, REGULAR_WEIGHT,
                    "Variable font {:?} should use Regular (400) weight by default",
                    path
                );
            }
            if let Some(bw) = bold.variable_weight() {
                assert_eq!(
                    bw, BOLD_WEIGHT,
                    "Variable font {:?} bold companion should use Bold (700) weight",
                    path
                );
            }

            let (_, reg_bmp) = regular.rasterize('中', 32.0);
            let (_, bold_bmp) = bold.rasterize('中', 32.0);
            let reg_ink: u64 = reg_bmp.iter().map(|&a| a as u64).sum();
            let bold_ink: u64 = bold_bmp.iter().map(|&a| a as u64).sum();
            assert!(reg_ink > 0, "Regular '中' should have non-zero coverage");
            assert!(
                bold_ink > reg_ink,
                "Bold '中' coverage ({bold_ink}) should exceed Regular ({reg_ink})"
            );

            if let Some(thin) = regular.with_weight(100.0) {
                let (_, thin_bmp) = thin.rasterize('中', 32.0);
                let thin_ink: u64 = thin_bmp.iter().map(|&a| a as u64).sum();
                println!(
                    "NotoSansTC ({:?}): thin_ink(100)={}, reg_ink(400)={}, bold_ink(700)={}",
                    path, thin_ink, reg_ink, bold_ink
                );
                assert!(
                    reg_ink > thin_ink,
                    "Regular (400) '中' coverage ({reg_ink}) should exceed Thin (100) ({thin_ink})"
                );
            } else {
                println!(
                    "NotoSansTC ({:?}): reg_ink={}, bold_ink={}",
                    path, reg_ink, bold_ink
                );
            }

            let (_, _, _, fallbacks) = load_configured_font_chain("Menlo, NotoSansTC", &[]);
            assert!(
                !fallbacks.is_empty() && fallbacks[0].bold.is_some(),
                "Fallback chain entry for NotoSansTC must include a Bold companion"
            );
        }
    }

    #[test]
    fn modifier_and_technical_symbols_fit_within_single_cell_bounds() {
        let font_size = 32.0;
        let (primary, _, _, mut fallbacks) =
            load_configured_font_chain("NotoSansMNerdFontMono, NotoSansTC, Menlo", &[]);
        for path in SYSTEM_CJK_CANDIDATES {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(regular) = FontFace::from_bytes(bytes, FontSettings::default()) {
                    fallbacks.push(FallbackFont {
                        regular,
                        bold: None,
                        italic: None,
                        bold_italic: None,
                    });
                }
            }
        }

        let m_metrics = primary.metrics('M', font_size);
        let cell_width = m_metrics.advance_width.ceil().max(1.0);
        let cell_height = primary
            .horizontal_line_metrics(font_size)
            .map(|m| (m.ascent - m.descent + m.line_gap).ceil())
            .unwrap_or(font_size * 1.3)
            .max(1.0);

        let resolve_face = |c: char, is_bold: bool| -> &FontFace {
            if primary.lookup_glyph_index(c) != 0 {
                return &primary;
            }
            let target_cells = unicode_width::UnicodeWidthChar::width(c)
                .unwrap_or(1)
                .max(1);
            if target_cells == 1 {
                if let Some(f) = fallbacks.iter().find_map(|fb| {
                    let face = if is_bold {
                        fb.bold.as_ref().unwrap_or(&fb.regular)
                    } else {
                        &fb.regular
                    };
                    (face.lookup_glyph_index(c) != 0
                        && face.metrics(c, font_size).advance_width <= cell_width * 1.25)
                        .then_some(face)
                }) {
                    return f;
                }
            }
            fallbacks
                .iter()
                .map(|fb| {
                    if is_bold {
                        fb.bold.as_ref().unwrap_or(&fb.regular)
                    } else {
                        &fb.regular
                    }
                })
                .find(|f| f.lookup_glyph_index(c) != 0)
                .unwrap_or(&primary)
        };

        if let Some((meslo, _)) = find_font_file("MesloLGS NF Regular") {
            for sym in ['\u{E0B0}', '\u{E0B2}', '\u{E0B4}', '\u{E0B6}'] {
                let raw_m = meslo.metrics(sym, font_size);
                let (fit_m, _) =
                    rasterize_fitted_glyph(&meslo, sym, font_size, cell_width, cell_height);
                assert_eq!(
                    (fit_m.width, fit_m.height, fit_m.xmin, fit_m.ymin),
                    (raw_m.width, raw_m.height, raw_m.xmin, raw_m.ymin),
                    "Powerline glyph U+{:04X} must preserve its exact native metrics",
                    sym as u32
                );
            }
            // Powerlevel10k diamond test places U+E0B2 ('') at col 0 and U+E0B0 ('') at col 1;
            // their horizontal ink spans must meet or overlap with zero gap.
            let (left_half, _) =
                rasterize_fitted_glyph(&meslo, '\u{E0B2}', font_size, cell_width, cell_height);
            let (right_half, _) =
                rasterize_fitted_glyph(&meslo, '\u{E0B0}', font_size, cell_width, cell_height);
            let left_right_edge = left_half.xmin as f32 + left_half.width as f32;
            let right_left_edge = cell_width + right_half.xmin as f32;
            assert!(
                left_right_edge >= right_left_edge,
                "Powerline diamond halves U+E0B2 (right edge {:.1}) and U+E0B0 (left edge {:.1}) must not have a gap",
                left_right_edge,
                right_left_edge
            );
        }

        for sym in ['⌘', '⇧', '⌥', '⌃', '↩', '⚙'] {
            for is_bold in [false, true] {
                let face = resolve_face(sym, is_bold);
                if face.lookup_glyph_index(sym) == 0 {
                    continue;
                }
                let (fit_m, fit_bmp) =
                    rasterize_fitted_glyph(face, sym, font_size, cell_width, cell_height);
                assert!(fit_m.width > 0 && !fit_bmp.is_empty());
                assert!(
                    fit_m.xmin >= 0,
                    "Symbol '{}' (bold={}) left offset ({}) must not bleed into previous cell",
                    sym,
                    is_bold,
                    fit_m.xmin
                );
                assert!(
                    (fit_m.xmin as f32 + fit_m.width as f32) <= cell_width,
                    "Symbol '{}' (bold={}) right edge ({} + {}) must not exceed cell_width ({})",
                    sym,
                    is_bold,
                    fit_m.xmin,
                    fit_m.width,
                    cell_width
                );
            }
        }

        // Also verify direct rasterization against CJK / symbol fallback faces (e.g. Hiragino Sans GB,
        // NotoSansTC, Apple Symbols) where modifier symbols are authored on a 2-cell / proportional em-square.
        for fallback in &fallbacks {
            let face = &fallback.regular;
            for sym in ['⌘', '⇧', '⌥', '⌃', '↩', '⚙'] {
                if face.lookup_glyph_index(sym) == 0 {
                    continue;
                }
                let (fit_m, fit_bmp) =
                    rasterize_fitted_glyph(face, sym, font_size, cell_width, cell_height);
                assert!(fit_m.width > 0 && !fit_bmp.is_empty());
                assert!(
                    fit_m.xmin >= 0,
                    "Fallback symbol '{}' left offset ({}) must be >= 0",
                    sym,
                    fit_m.xmin
                );
                assert!(
                    (fit_m.xmin as f32 + fit_m.width as f32) <= cell_width,
                    "Fallback symbol '{}' right edge ({} + {}) must not exceed cell_width ({})",
                    sym,
                    fit_m.xmin,
                    fit_m.width,
                    cell_width
                );
            }
        }

        // Verify adjacent symbol sequences like "(⇧⌘A)", "(⇧⌘[)", "(⌥⌘[)", "(⇧⌘↩)", "(⌃⌘F)"
        // have strictly non-overlapping horizontal ink spans across consecutive columns.
        for seq in ["(⇧⌘A)", "(⇧⌘[)", "(⌥⌘[)", "(⌘[)", "(⇧⌘↩)", "(⌥⌘=)", "(⌃⌘F)"]
        {
            let mut prev_right: Option<(char, f32)> = None;
            let mut col = 0usize;
            for c in seq.chars() {
                let w_cells = unicode_width::UnicodeWidthChar::width(c)
                    .unwrap_or(1)
                    .max(1);
                let face = resolve_face(c, false);
                if face.lookup_glyph_index(c) != 0 {
                    let (m, _) =
                        rasterize_fitted_glyph(face, c, font_size, cell_width, cell_height);
                    let cell_x = col as f32 * cell_width;
                    let glyph_left = cell_x + m.xmin as f32;
                    let glyph_right = glyph_left + m.width as f32;
                    if let Some((prev_c, pr)) = prev_right {
                        assert!(
                            glyph_left >= pr,
                            "In sequence {:?}: '{}' at [{:.1}, {:.1}] overlaps previous '{}' ending at {:.1}",
                            seq,
                            c,
                            glyph_left,
                            glyph_right,
                            prev_c,
                            pr
                        );
                    }
                    prev_right = Some((c, glyph_right));
                }
                col += w_cells;
            }
        }
    }

    #[test]
    fn test_cjk_mixed_ascii_visual_spacing_and_grid_centering() {
        let font_size = 32.0_f32;
        let (primary, _, _, mut fallbacks) = load_configured_font_chain("Menlo", &[]);
        for path in SYSTEM_CJK_CANDIDATES {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(regular) = FontFace::from_bytes(bytes, FontSettings::default()) {
                    fallbacks.push(FallbackFont {
                        regular,
                        bold: None,
                        italic: None,
                        bold_italic: None,
                    });
                }
            }
        }

        let cell_width = primary
            .metrics('M', font_size)
            .advance_width
            .ceil()
            .max(1.0);
        let cell_height = primary
            .horizontal_line_metrics(font_size)
            .map(|m| (m.ascent - m.descent + m.line_gap).ceil())
            .unwrap_or(font_size * 1.3)
            .max(1.0);

        let resolve_face = |c: char| -> Option<&FontFace> {
            if primary.lookup_glyph_index(c) != 0 {
                return Some(&primary);
            }
            fallbacks
                .iter()
                .map(|fb| &fb.regular)
                .find(|f| f.lookup_glyph_index(c) != 0)
        };

        let sample = "16級資優班生活";
        if sample.chars().any(|c| resolve_face(c).is_none()) {
            return;
        }

        // 1. UI typographic advance layout: verify non-overlapping ink and compact spacing
        // between ASCII digits ('1','6'), ASCII->CJK ('6'->'級'), and CJK->CJK ('級'->'資'...).
        let mut pen_x = 0.0_f32;
        let mut ink_spans: Vec<(char, f32, f32)> = Vec::new();
        let mut cjk_advances: Vec<(char, f32)> = Vec::new();
        for c in sample.chars() {
            let face = resolve_face(c).unwrap();
            let (m, _) = rasterize_fitted_glyph(face, c, font_size, cell_width, cell_height);
            if !c.is_ascii() {
                cjk_advances.push((c, m.advance_width));
            }
            let gx = pen_x.round() + m.xmin as f32;
            ink_spans.push((c, gx, gx + m.width as f32));
            pen_x += m.advance_width;
        }

        for &(c, adv) in &cjk_advances {
            assert!(
                adv <= font_size * 1.1,
                "Expected CJK glyph '{}' to keep compact ~1.0em typographic advance (<= {:.1}px), got {:.2}px",
                c,
                font_size * 1.1,
                adv
            );
        }

        let gap_1_6 = ink_spans[1].1 - ink_spans[0].2;
        let gap_6_cjk = ink_spans[2].1 - ink_spans[1].2;
        let cjk_gaps: Vec<f32> = ink_spans[2..].windows(2).map(|w| w[1].1 - w[0].2).collect();

        assert!(gap_1_6 > 0.0 && gap_6_cjk > 0.0);
        for &g in &cjk_gaps {
            // Full-body Gothic CJK faces (e.g. Noto Sans CJK on Linux, Microsoft YaHei on Windows)
            // fill nearly 100% of the 1.0em square, so antialiased integer pixel bounding boxes of
            // adjacent wide ideographs can touch cleanly (0.0px gap) without overlapping.
            assert!(
                (0.0..=8.0).contains(&g),
                "Expected non-overlapping, compact CJK inter-glyph gap (0..=8px at 32px font), got {:.2}px",
                g
            );
            assert!(
                g <= gap_1_6 + 4.0,
                "Expected CJK gap ({:.2}px) not to exceed ASCII digit gap ({:.2}px) by more than 4px",
                g,
                gap_1_6
            );
        }

        // 2. Terminal 2-cell grid centering: verify CJK ideograph is horizontally balanced
        // inside [0, 2 * cell_width] instead of hugging the left edge next to preceding ASCII.
        let cjk_face = resolve_face('級').unwrap();
        let (cjk_m, _) = rasterize_fitted_glyph(cjk_face, '級', font_size, cell_width, cell_height);
        let pad_x = crate::wide_cell_center_offset('級', cjk_m.advance_width, cell_width);
        let left_margin = pad_x + cjk_m.xmin as f32;
        let right_margin = (2.0 * cell_width) - (left_margin + cjk_m.width as f32);
        assert!(left_margin >= 0.0 && right_margin >= 0.0);
        assert!(
            (left_margin - right_margin).abs() <= 3.0,
            "Expected symmetric 2-cell CJK margins, got left={:.1}px right={:.1}px",
            left_margin,
            right_margin
        );
    }

    #[test]
    fn test_menlo_loads_bold_italic_and_bold_italic_faces() {
        if !Path::new("/System/Library/Fonts/Menlo.ttc").exists() {
            return;
        }
        let chain = load_font_chain("Menlo", &[]);
        assert!(chain.bold.is_some(), "Menlo.ttc must resolve Bold face");
        assert!(chain.italic.is_some(), "Menlo.ttc must resolve Italic face");
        assert!(
            chain.bold_italic.is_some(),
            "Menlo.ttc must resolve Bold+Italic face"
        );

        let (_, reg_bmp) = chain.regular.rasterize('A', 32.0);
        let (_, ita_bmp) = chain.italic.as_ref().unwrap().rasterize('A', 32.0);
        let (_, bi_bmp) = chain.bold_italic.as_ref().unwrap().rasterize('A', 32.0);
        let (_, b_bmp) = chain.bold.as_ref().unwrap().rasterize('A', 32.0);
        assert_ne!(reg_bmp, ita_bmp, "Menlo Italic must differ from Regular");
        assert_ne!(b_bmp, bi_bmp, "Menlo BoldItalic must differ from Bold");
        assert_ne!(ita_bmp, bi_bmp, "Menlo BoldItalic must differ from Italic");
    }

    #[test]
    fn test_synthetic_italic_shear_produces_slanted_bitmap() {
        let chain = load_font_chain("Menlo", &[]);
        let (orig_m, orig_bmp) = chain.regular.rasterize('H', 32.0);
        let (sheared_m, sheared_bmp) = shear_mono_bitmap(orig_m, orig_bmp.clone());
        assert!(sheared_m.width > orig_m.width);
        assert_eq!(sheared_m.height, orig_m.height);
        let ink: u64 = sheared_bmp.iter().map(|&v| v as u64).sum();
        assert!(ink > 0);
    }

    #[test]
    fn test_composite_mono_grapheme_nfd_combining_mark() {
        let chain = load_font_chain("Menlo", &[]);
        let base = chain.regular.rasterize('e', 32.0);
        let acute = chain.regular.rasterize('\u{0301}', 32.0);
        if acute.0.width == 0 || acute.0.height == 0 {
            return;
        }
        let (comp_m, comp_bmp) = composite_mono_grapheme(&[base.clone(), acute]);
        assert!(comp_m.width >= base.0.width);
        assert!(comp_m.height >= base.0.height);
        let base_ink: u64 = base.1.iter().map(|&v| v as u64).sum();
        let comp_ink: u64 = comp_bmp.iter().map(|&v| v as u64).sum();
        assert!(
            comp_ink > base_ink,
            "Composited NFD 'é' ink ({comp_ink}) should exceed base 'e' ({base_ink})"
        );
    }

    #[test]
    fn test_color_emoji_decoding_when_system_font_present() {
        let Some(emoji_font) = load_system_color_emoji_font() else {
            // Gracefully skip on headless CI systems without Apple Color Emoji / Noto Color Emoji
            return;
        };

        // Single-codepoint default emoji
        let fire = emoji_font
            .decode_char_rgba('🔥', 32, 36, 36)
            .expect("🔥 should decode from system color emoji font");
        assert!(fire.width > 0 && fire.width <= 36);
        assert!(fire.height > 0 && fire.height <= 36);
        assert_eq!(fire.rgba.len(), (fire.width * fire.height * 4) as usize);
        // Must have non-zero alpha AND actual color variation (not all grayscale)
        let has_color = fire
            .rgba
            .as_chunks::<4>()
            .0
            .iter()
            .any(|px| px[3] > 32 && (px[0] != px[1] || px[1] != px[2]));
        assert!(has_color, "Decoded 🔥 emoji must contain RGBA color pixels");

        // VS16 emoji presentation grapheme (❤️ = U+2764 + U+FE0F)
        let heart = emoji_font
            .decode_grapheme_rgba("\u{2764}\u{FE0F}", 32, 36, 36)
            .expect("❤️ (VS16) should decode from system color emoji font");
        assert!(heart.width > 0 && heart.height > 0);
        assert_eq!(heart.rgba.len(), (heart.width * heart.height * 4) as usize);

        // VS15 text presentation selector must NOT return a color bitmap
        assert!(
            emoji_font
                .decode_grapheme_rgba("\u{2764}\u{FE0E}", 32, 36, 36)
                .is_none(),
            "VS15 text presentation must bypass color emoji bitmap"
        );
    }

    #[test]
    fn test_emoji_presentation_classification() {
        assert!(is_default_emoji_presentation('🔥'));
        assert!(is_default_emoji_presentation('🚀'));
        assert!(is_default_emoji_presentation('✅'));
        assert!(!is_default_emoji_presentation('A'));
        assert!(!is_default_emoji_presentation('#'));
        assert!(!is_default_emoji_presentation('⚙'));
        assert!(!is_default_emoji_presentation('⌘'));

        assert!(is_color_emoji_grapheme("\u{2764}\u{FE0F}"));
        assert!(is_color_emoji_grapheme("\u{2699}\u{FE0F}"));
        assert!(is_color_emoji_grapheme("👍🏽"));
        assert!(is_color_emoji_grapheme("🇺🇸"));
        assert!(is_color_emoji_grapheme("👨‍👩‍👧‍👦"));
        assert!(!is_color_emoji_grapheme("\u{2764}\u{FE0E}"));
        assert!(!is_color_emoji_grapheme("e\u{0301}"));
    }
}
