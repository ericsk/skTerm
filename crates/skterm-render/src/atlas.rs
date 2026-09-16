//! Dynamic Glyph Texture Atlas, Font Resolution Engine, Multi-Font Fallback, and Fontdue-based text rasterization.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{info, warn};
use wgpu::{
    Extent3d, Origin3d, Queue, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect,
    TextureDimension, TextureFormat, TextureUsages,
};

/// Key identifying a cached glyph in the texture atlas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub c: char,
    pub is_bold: bool,
    pub is_italic: bool,
}

/// Metadata and UV bounds of an allocated glyph in the atlas.
#[derive(Debug, Clone, Copy)]
pub struct GlyphInfo {
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size: [f32; 2],
    pub offset: [f32; 2],
}

/// 2D dynamic texture atlas for font glyph caching with user-configured font chains & CJK fallback.
pub struct GlyphAtlas {
    pub texture: Texture,
    pub texture_view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub width: u32,
    pub height: u32,
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    cache: HashMap<GlyphKey, GlyphInfo>,
    regular_font: Font,
    bold_font: Option<Font>,
    italic_font: Option<Font>,
    fallback_fonts: Vec<Font>,
    fallback_initialized: bool,
    pub font_size: f32,
    pub cell_width: f32,
    pub cell_height: f32,
    pub baseline_offset: f32,
}

impl GlyphAtlas {
    pub const ATLAS_SIZE: u32 = 2048;

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
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        // Load configured font chain
        let (regular_font, bold_font, italic_font, fallback_fonts) =
            if let Some(data) = custom_font_data {
                let font = Font::from_bytes(data, FontSettings::default())
                    .map_err(|e| anyhow::anyhow!("Failed to parse custom font: {}", e))?;
                (font, None, None, Vec::new())
            } else {
                load_configured_font_chain(font_family, fallback_families)
            };

        // Compute metrics for cell dimensions
        let regular_metrics = regular_font.metrics('M', font_size);
        let line_metrics = regular_font.horizontal_line_metrics(font_size);

        let cell_width = regular_metrics.advance_width.ceil().max(1.0);
        let cell_height = line_metrics
            .map(|m| (m.ascent - m.descent + m.line_gap).ceil())
            .unwrap_or(font_size * 1.3)
            .max(1.0);
        let baseline_offset = line_metrics.map(|m| m.ascent.ceil()).unwrap_or(font_size);

        info!(
            "Font Engine Loaded: family='{}', size={}, cell_w={}, cell_h={}, fallbacks_count={}",
            font_family,
            font_size,
            cell_width,
            cell_height,
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
            cache: HashMap::new(),
            regular_font,
            bold_font,
            italic_font,
            fallback_fonts,
            fallback_initialized: false,
            font_size,
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

    /// Lazy-load standard system CJK / Emoji fallback fonts on first demand if not already in chain
    fn ensure_fallback_loaded(&mut self) {
        if !self.fallback_initialized {
            self.fallback_initialized = true;
            let cjk_candidates = [
                // macOS
                "/System/Library/Fonts/Hiragino Sans GB.ttc",
                "/System/Library/Fonts/STHeiti Light.ttc",
                "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
                "/System/Library/Fonts/Supplemental/Songti.ttc",
                "/System/Library/Fonts/PingFang.ttc",
                // Linux
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
                "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
                // Windows
                "C:\\Windows\\Fonts\\msyh.ttc",
                "C:\\Windows\\Fonts\\simsun.ttc",
            ];

            for path in &cjk_candidates {
                if let Ok(bytes) = std::fs::read(path) {
                    if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                        info!("Loaded system CJK fallback font: {}", path);
                        self.fallback_fonts.push(font);
                        break;
                    }
                }
            }
        }
    }

    /// Retrieve glyph from cache or rasterize (checking primary font, configured fallbacks, then system fallbacks) and upload to GPU texture.
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

        // 1. Check primary font
        let has_glyph = if is_bold && self.bold_font.is_some() {
            self.bold_font.as_ref().unwrap().lookup_glyph_index(c) != 0
        } else if is_italic && self.italic_font.is_some() {
            self.italic_font.as_ref().unwrap().lookup_glyph_index(c) != 0
        } else {
            self.regular_font.lookup_glyph_index(c) != 0
        };

        let (metrics, bitmap) = if has_glyph {
            let primary = if is_bold && self.bold_font.is_some() {
                self.bold_font.as_ref().unwrap()
            } else if is_italic && self.italic_font.is_some() {
                self.italic_font.as_ref().unwrap()
            } else {
                &self.regular_font
            };
            primary.rasterize(c, self.font_size)
        } else {
            // 2. Check configured fallback chain
            let mut found = None;
            for fallback in &self.fallback_fonts {
                if fallback.lookup_glyph_index(c) != 0 {
                    found = Some(fallback.rasterize(c, self.font_size));
                    break;
                }
            }

            // 3. Check system fallback fonts
            if found.is_none() {
                self.ensure_fallback_loaded();
                for fallback in &self.fallback_fonts {
                    if fallback.lookup_glyph_index(c) != 0 {
                        found = Some(fallback.rasterize(c, self.font_size));
                        break;
                    }
                }
            }

            match found {
                Some(res) => res,
                None => self.regular_font.rasterize('?', self.font_size),
            }
        };

        let glyph_w = metrics.width as u32;
        let glyph_h = metrics.height as u32;

        if glyph_w == 0 || glyph_h == 0 {
            let info = GlyphInfo {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size: [0.0, 0.0],
                offset: [0.0, 0.0],
            };
            self.cache.insert(key, info);
            return Some(info);
        }

        // Shelf packing algorithm
        if self.cursor_x + glyph_w + 1 >= self.width {
            self.cursor_x = 1;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        if self.cursor_y + glyph_h + 1 >= self.height {
            warn!("Glyph atlas full! Clearing cache.");
            self.cache.clear();
            self.cursor_x = 1;
            self.cursor_y = 1;
            self.row_height = 0;
        }

        let alloc_x = self.cursor_x;
        let alloc_y = self.cursor_y;

        self.cursor_x += glyph_w + 1;
        self.row_height = self.row_height.max(glyph_h);

        // Upload glyph bitmap to GPU atlas texture
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
            &bitmap,
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

        let info = GlyphInfo {
            uv_min,
            uv_max,
            size: [glyph_w as f32, glyph_h as f32],
            offset: [metrics.xmin as f32, metrics.ymin as f32],
        };

        self.cache.insert(key, info);
        Some(info)
    }
}

fn clean_name(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

static FONT_FILE_INDEX: std::sync::OnceLock<Vec<(PathBuf, String)>> = std::sync::OnceLock::new();

fn get_cached_font_index() -> &'static [(PathBuf, String)] {
    FONT_FILE_INDEX.get_or_init(|| {
        let mut search_dirs = Vec::new();
        if let Ok(home) = std::env::var("HOME") {
            let home_path = PathBuf::from(home);
            search_dirs.push(home_path.join("Library/Fonts"));
            search_dirs.push(home_path.join(".local/share/fonts"));
            search_dirs.push(home_path.join(".fonts"));
        }
        if let Ok(appdata) = std::env::var("LOCALAPPDATA") {
            search_dirs.push(PathBuf::from(appdata).join("Microsoft\\Windows\\Fonts"));
        }
        search_dirs.push(PathBuf::from("/Library/Fonts"));
        search_dirs.push(PathBuf::from("/System/Library/Fonts"));
        search_dirs.push(PathBuf::from("/System/Library/Fonts/Supplemental"));
        search_dirs.push(PathBuf::from("/usr/share/fonts"));
        search_dirs.push(PathBuf::from("/usr/local/share/fonts"));
        search_dirs.push(PathBuf::from("C:\\Windows\\Fonts"));

        let mut files = Vec::new();
        for dir in search_dirs {
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                            let ext_lower = ext.to_lowercase();
                            if ext_lower == "ttf" || ext_lower == "ttc" || ext_lower == "otf" {
                                // Index the stem, not the file name: the extension
                                // would otherwise count as "extra" characters when
                                // ranking candidates below.
                                if let Some(stem) = path.file_stem().and_then(|n| n.to_str()) {
                                    let clean = clean_name(stem);
                                    files.push((path, clean));
                                }
                            }
                        }
                    }
                }
            }
        }
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
        ("dejavusansmono", "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf"),
        ("consolas", "C:\\Windows\\Fonts\\consola.ttf"),
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
        } else if clean_q.contains("noto") && clean_q.contains("tc") {
            vec!["noto", "tc"]
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

/// Search user and system font directories for a font family name, returning the parsed Font.
pub fn find_font_file(query: &str) -> Option<(Font, PathBuf)> {
    for path in ranked_candidates(query) {
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                return Some((font, path));
            }
        }
    }
    None
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

/// Strip a trailing style segment from a font file stem.
///
/// "NotoSansMNerdFontMono-SemiCondensedBlack" -> "NotoSansMNerdFontMono"
/// "NotoSansTC-VariableFont_wght"             -> "NotoSansTC"
/// "Andale Mono"                              -> "Andale Mono"
fn family_of(stem: &str) -> String {
    let Some((head, tail)) = stem.rsplit_once('-') else {
        return stem.to_string();
    };
    let clean_tail = clean_name(tail);
    let is_style = clean_tail.starts_with("regular")
        || clean_tail.starts_with("variablefont")
        || STYLE_TOKENS.iter().any(|t| clean_tail.contains(t));
    if is_style && !head.is_empty() {
        head.to_string()
    } else {
        stem.to_string()
    }
}

/// Load full configured font chain (Primary Monospace + Fallbacks)
pub fn load_configured_font_chain(
    family_str: &str,
    fallback_families: &[String],
) -> (Font, Option<Font>, Option<Font>, Vec<Font>) {
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

    let mut primary_font: Option<Font> = None;
    let mut bold_font: Option<Font> = None;
    let mut italic_font: Option<Font> = None;
    let mut fallback_fonts: Vec<Font> = Vec::new();

    for q in &queries {
        if let Some((font, path)) = find_font_file(q) {
            if primary_font.is_none() {
                info!("Found primary font match for '{}': {:?}", q, path);
                primary_font = Some(font);

                // Only query bold/italic variants if not a single-file TTC like Menlo
                let clean_q = clean_name(q);
                if clean_q != "menlo" && clean_q != "monaco" {
                    let bold_query = format!("{}Bold", q);
                    if let Some((b_font, _)) = find_font_file(&bold_query) {
                        bold_font = Some(b_font);
                    }
                    let italic_query = format!("{}Italic", q);
                    if let Some((i_font, _)) = find_font_file(&italic_query) {
                        italic_font = Some(i_font);
                    }
                }
            } else {
                info!("Found fallback font match for '{}': {:?}", q, path);
                fallback_fonts.push(font);
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
            "C:\\Windows\\Fonts\\consola.ttf",
        ];
        for d in defaults {
            if let Ok(b) = std::fs::read(d) {
                if let Ok(f) = Font::from_bytes(b, FontSettings::default()) {
                    info!("Loaded default fallback primary monospace font: {}", d);
                    return f;
                }
            }
        }
        panic!("FATAL: No valid monospace font found on system!");
    });

    (primary, bold_font, italic_font, fallback_fonts)
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
        assert_eq!(family_of("NotoSansMNerdFontMono-SemiCondensedBlack"), "NotoSansMNerdFontMono");
        assert_eq!(family_of("NotoSansTC-VariableFont_wght"), "NotoSansTC");
        assert_eq!(family_of("SFMono-Regular"), "SFMono");
        assert_eq!(family_of("NotoSansMNerdFontMono-Italic"), "NotoSansMNerdFontMono");
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
        for family in installed_font_families().iter().take(40) {
            assert!(
                resolve_font_path(family).is_some(),
                "picker offers {:?} but it resolves to nothing",
                family
            );
        }
    }
}
