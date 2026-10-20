//! Dynamic Glyph Texture Atlas, Font Resolution Engine, Multi-Font Fallback, and Fontdue-based text rasterization.

use ab_glyph::{ScaleFont as _, VariableFont as _};
use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::PathBuf;
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

/// A fallback font entry holding both its Regular (400) face and an optional Bold (700) face.
pub struct FallbackFont {
    pub regular: FontFace,
    pub bold: Option<FontFace>,
}

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
    regular_font: FontFace,
    bold_font: Option<FontFace>,
    italic_font: Option<FontFace>,
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

        // Load configured font chain
        let (regular_font, bold_font, italic_font, fallback_fonts) =
            if let Some(data) = custom_font_data {
                let font = FontFace::from_bytes(data.to_vec(), FontSettings::default())
                    .map_err(|e| anyhow::anyhow!("Failed to parse custom font: {}", e))?;
                let bold = font.with_weight(BOLD_WEIGHT);
                (font, bold, None, Vec::new())
            } else {
                load_configured_font_chain(font_family, fallback_families)
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

        // Vertically center glyphs within character cell height.
        // Terminal text lines and cursors are visually judged by prompt characters (e.g. '>') and capital letters.
        // Aligning the visual center of standard glyphs to cell_height / 2 ensures that the cursor quad
        // (which spans the entire cell_height) frames text symmetrically with equal margins above and below,
        // preventing the cursor from appearing noticeably lower or sagging beneath the text line.
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

    /// Lazily load the next available system CJK / Unicode fallback font on demand.
    fn load_next_system_fallback(&mut self) -> Option<usize> {
        while self.system_fallback_cursor < SYSTEM_CJK_CANDIDATES.len() {
            let path = SYSTEM_CJK_CANDIDATES[self.system_fallback_cursor];
            self.system_fallback_cursor += 1;
            if let Ok(bytes) = std::fs::read(path) {
                let companion_bold =
                    if path.ends_with("Hiragino Sans GB.ttc") || path.ends_with("Menlo.ttc") {
                        FontFace::from_bytes(
                            bytes.clone(),
                            FontSettings {
                                collection_index: 1,
                                ..Default::default()
                            },
                        )
                        .ok()
                    } else {
                        None
                    };
                if let Ok(regular) = FontFace::from_bytes(bytes, FontSettings::default()) {
                    info!("Loaded system CJK fallback font: {}", path);
                    let bold = companion_bold.or_else(|| regular.with_weight(BOLD_WEIGHT));
                    let idx = self.fallback_fonts.len();
                    self.fallback_fonts.push(FallbackFont { regular, bold });
                    return Some(idx);
                }
            }
        }
        None
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
        let primary = if let (true, Some(bold)) = (is_bold, self.bold_font.as_ref()) {
            bold
        } else if let (true, Some(italic)) = (is_italic, self.italic_font.as_ref()) {
            italic
        } else {
            &self.regular_font
        };

        let (metrics, bitmap) = if primary.lookup_glyph_index(c) != 0 {
            rasterize_fitted_glyph(
                primary,
                c,
                self.font_size,
                self.cell_width,
                self.cell_height,
            )
        } else {
            let target_cells = unicode_width::UnicodeWidthChar::width(c)
                .unwrap_or(1)
                .max(1);
            let mut found = None;

            // For single-cell characters (e.g. `⌘`, `⇧`, `⌥`, `⌃`, `↩`), prefer a fallback
            // font whose glyph is natively single-cell (`advance_width <= cell_width * 1.25`)
            // over a CJK font where the same codepoint is drawn as a 2-cell full-width glyph.
            if target_cells == 1 {
                for fallback in &self.fallback_fonts {
                    let face = if is_bold {
                        fallback.bold.as_ref().unwrap_or(&fallback.regular)
                    } else {
                        &fallback.regular
                    };
                    if face.lookup_glyph_index(c) != 0 {
                        let m = face.metrics(c, self.font_size);
                        if m.advance_width <= self.cell_width * 1.25 {
                            found = Some(rasterize_fitted_glyph(
                                face,
                                c,
                                self.font_size,
                                self.cell_width,
                                self.cell_height,
                            ));
                            break;
                        }
                    }
                }

                // If no configured fallback had a native 1-cell glyph, also check the first
                // system monospace fallback (e.g. Menlo.ttc on macOS) before accepting a
                // 2-cell CJK glyph.
                if found.is_none() && self.system_fallback_cursor == 0 {
                    if let Some(idx) = self.load_next_system_fallback() {
                        let fallback = &self.fallback_fonts[idx];
                        let face = if is_bold {
                            fallback.bold.as_ref().unwrap_or(&fallback.regular)
                        } else {
                            &fallback.regular
                        };
                        if face.lookup_glyph_index(c) != 0 {
                            let m = face.metrics(c, self.font_size);
                            if m.advance_width <= self.cell_width * 1.25 {
                                found = Some(rasterize_fitted_glyph(
                                    face,
                                    c,
                                    self.font_size,
                                    self.cell_width,
                                    self.cell_height,
                                ));
                            }
                        }
                    }
                }
            }

            // 2. Check already-loaded fallback chain (selecting Bold face when `is_bold` is true)
            if found.is_none() {
                for fallback in &self.fallback_fonts {
                    let face = if is_bold {
                        fallback.bold.as_ref().unwrap_or(&fallback.regular)
                    } else {
                        &fallback.regular
                    };
                    if face.lookup_glyph_index(c) != 0 {
                        found = Some(rasterize_fitted_glyph(
                            face,
                            c,
                            self.font_size,
                            self.cell_width,
                            self.cell_height,
                        ));
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
                let face = if is_bold {
                    fallback.bold.as_ref().unwrap_or(&fallback.regular)
                } else {
                    &fallback.regular
                };
                if face.lookup_glyph_index(c) != 0 {
                    found = Some(rasterize_fitted_glyph(
                        face,
                        c,
                        self.font_size,
                        self.cell_width,
                        self.cell_height,
                    ));
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

/// Locate or derive a Bold (`700`) companion face for `query` given its loaded `regular_face`.
///
/// 1. Prefers a dedicated static `*Bold` font file if one is installed.
/// 2. Otherwise, if `regular_face` is a Variable Font with a `wght` axis (such as
///    `NotoSansTC-VariableFont_wght.ttf`), derives a `wght = 700` instance directly from it.
/// 3. Finally, if `regular_face` came from a static file without a matching static `*Bold`
///    file, checks if a Variable Font candidate for `query` exists and loads it at `wght = 700`.
fn load_bold_companion(query: &str, regular_face: &FontFace) -> Option<FontFace> {
    let clean_q = clean_name(query);
    if clean_q == "menlo" {
        if let Some(path) = direct_candidate("menlo") {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(face) = FontFace::from_bytes(
                    bytes,
                    FontSettings {
                        collection_index: 1,
                        ..Default::default()
                    },
                ) {
                    return Some(face);
                }
            }
        }
        return None;
    }
    if clean_q == "monaco" {
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

/// Load full configured font chain (Primary Monospace + Fallbacks)
pub fn load_configured_font_chain(
    family_str: &str,
    fallback_families: &[String],
) -> (
    FontFace,
    Option<FontFace>,
    Option<FontFace>,
    Vec<FallbackFont>,
) {
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
    let mut fallback_fonts: Vec<FallbackFont> = Vec::new();

    for q in &queries {
        if let Some((font, path)) = find_font_file(q) {
            if primary_font.is_none() {
                info!("Found primary font match for '{}': {:?}", q, path);
                bold_font = load_bold_companion(q, &font);

                // Query italic/oblique variants if not a single-file TTC like Menlo
                let clean_q = clean_name(q);
                if clean_q != "menlo" && clean_q != "monaco" {
                    let base_q = family_of(q.trim());
                    let italic_query = format!("{}Italic", base_q);
                    if let Some((i_font, _)) = find_font_file(&italic_query) {
                        italic_font = Some(i_font);
                    } else {
                        let oblique_query = format!("{}Oblique", base_q);
                        if let Some((o_font, _)) = find_font_file(&oblique_query) {
                            italic_font = Some(o_font);
                        }
                    }
                }
                primary_font = Some(font);
            } else {
                info!("Found fallback font match for '{}': {:?}", q, path);
                let bold = load_bold_companion(q, &font);
                fallback_fonts.push(FallbackFont {
                    regular: font,
                    bold,
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
                if let Ok(f) = FontFace::from_bytes(b, FontSettings::default()) {
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
            let bold = load_bold_companion("NotoSansTC", &regular)
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
}
