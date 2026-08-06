//! Dynamic Glyph Texture Atlas, CJK Multi-Font Fallback, and Fontdue-based text rasterization.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
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

/// 2D dynamic texture atlas for font glyph caching with CJK fallback.
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

        // Initialize atlas texture with zero bytes
        let empty_data = vec![0u8; (Self::ATLAS_SIZE * Self::ATLAS_SIZE) as usize];
        queue.write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            &empty_data,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(Self::ATLAS_SIZE),
                rows_per_image: Some(Self::ATLAS_SIZE),
            },
            Extent3d {
                width: Self::ATLAS_SIZE,
                height: Self::ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
        );

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

        let (regular_font, bold_font, italic_font, fallback_fonts) =
            load_system_fonts_with_cjk(custom_font_data)?;

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
            "Loaded font metrics: font_size={}, cell_width={}, cell_height={}, baseline={}, fallback_fonts={}",
            font_size, cell_width, cell_height, baseline_offset, fallback_fonts.len()
        );

        Ok(Self {
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
            font_size,
            cell_width,
            cell_height,
            baseline_offset,
        })
    }

    /// Retrieve glyph from cache or rasterize (checking primary font then CJK fallbacks) and upload to GPU texture.
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

        let primary = if is_bold && self.bold_font.is_some() {
            self.bold_font.as_ref().unwrap()
        } else if is_italic && self.italic_font.is_some() {
            self.italic_font.as_ref().unwrap()
        } else {
            &self.regular_font
        };

        // Try rasterizing from primary font first
        let (metrics, bitmap) = if primary.lookup_glyph_index(c) != 0 {
            primary.rasterize(c, self.font_size)
        } else {
            // Find in fallback fonts (CJK, Emoji, Symbols)
            let mut found = None;
            for fallback in &self.fallback_fonts {
                if fallback.lookup_glyph_index(c) != 0 {
                    let res = fallback.rasterize(c, self.font_size);
                    if res.0.width > 0 && !res.1.is_empty() {
                        found = Some(res);
                        break;
                    }
                }
            }
            found.unwrap_or_else(|| primary.rasterize(c, self.font_size))
        };

        let glyph_w = metrics.width as u32;
        let glyph_h = metrics.height as u32;

        if glyph_w == 0 || glyph_h == 0 {
            let info = GlyphInfo {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size: [0.0, 0.0],
                offset: [metrics.xmin as f32, metrics.ymin as f32],
            };
            self.cache.insert(key, info);
            return Some(info);
        }

        // Shelf packing with 1px padding
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

/// Load primary monospace fonts and CJK / Symbol fallback fonts.
fn load_system_fonts_with_cjk(
    custom_font_data: Option<&[u8]>,
) -> anyhow::Result<(Font, Option<Font>, Option<Font>, Vec<Font>)> {
    let mut primary_font = None;
    let mut fallback_fonts = Vec::new();

    if let Some(data) = custom_font_data {
        let font = Font::from_bytes(data, FontSettings::default())
            .map_err(|e| anyhow::anyhow!("Failed to parse custom font: {}", e))?;
        primary_font = Some(font);
    }

    // Primary monospace candidates
    let primary_candidates = [
        "/System/Library/Fonts/Menlo.ttc",
        "/System/Library/Fonts/Monaco.ttf",
        "/System/Library/Fonts/SFMono-Regular.otf",
        "/usr/share/fonts/truetype/dejavu/DejaVuSansMono.ttf",
        "/usr/share/fonts/TTF/DejaVuSansMono.ttf",
        "/usr/share/fonts/truetype/liberation/LiberationMono-Regular.ttf",
        "C:\\Windows\\Fonts\\consola.ttf",
        "C:\\Windows\\Fonts\\cascadiamono.ttf",
    ];

    if primary_font.is_none() {
        for path in &primary_candidates {
            if let Ok(bytes) = std::fs::read(path) {
                if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                    info!("Using primary monospace font: {}", path);
                    primary_font = Some(font);
                    break;
                }
            }
        }
    }

    // CJK and International Fallback font candidates
    let cjk_candidates = [
        // macOS
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/System/Library/Fonts/STHeiti Medium.ttc",
        "/System/Library/Fonts/STHeiti Light.ttc",
        "/System/Library/Fonts/Supplemental/Songti.ttc",
        "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        "/System/Library/Fonts/Apple Color Emoji.ttc",
        // Linux
        "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc",
        "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
        // Windows
        "C:\\Windows\\Fonts\\msyh.ttc",
        "C:\\Windows\\Fonts\\simsun.ttc",
        "C:\\Windows\\Fonts\\mingliu.ttc",
        "C:\\Windows\\Fonts\\meiryo.ttc",
    ];

    for path in &cjk_candidates {
        if let Ok(bytes) = std::fs::read(path) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                info!("Loaded CJK/international fallback font: {}", path);
                fallback_fonts.push(font);
            }
        }
    }

    let primary = primary_font.ok_or_else(|| {
        anyhow::anyhow!("No standard system monospace font found on host system.")
    })?;

    Ok((primary, None, None, fallback_fonts))
}
