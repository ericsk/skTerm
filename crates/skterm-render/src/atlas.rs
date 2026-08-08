//! Dynamic Glyph Texture Atlas, Font Resolution Engine, Multi-Font Fallback, and Fontdue-based text rasterization.

use fontdue::{Font, FontSettings};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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

        // Load configured font chain
        let (regular_font, bold_font, italic_font, fallback_fonts) = if let Some(data) = custom_font_data {
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
            font_family, font_size, cell_width, cell_height, fallback_fonts.len()
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
            fallback_initialized: false,
            font_size,
            cell_width,
            cell_height,
            baseline_offset,
        })
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
    s.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase()
}

/// Search user and system font directories for a font family name.
pub fn find_font_file(query: &str) -> Option<(Vec<u8>, PathBuf)> {
    let clean_q = clean_name(query);
    if clean_q.is_empty() {
        return None;
    }

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

    for dir in search_dirs {
        if dir.exists() {
            if let Some(res) = scan_dir_for_font(&dir, &clean_q) {
                return Some(res);
            }
        }
    }
    None
}

fn scan_dir_for_font(dir: &Path, clean_q: &str) -> Option<(Vec<u8>, PathBuf)> {
    let entries = std::fs::read_dir(dir).ok()?;
    let mut files = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                let ext_lower = ext.to_lowercase();
                if ext_lower == "ttf" || ext_lower == "ttc" || ext_lower == "otf" {
                    if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                        let clean = clean_name(file_name);
                        files.push((path, clean));
                    }
                }
            }
        }
    }

    // 1. Direct contains match (e.g. "notosansmnerdfontmono")
    for (path, clean_f) in &files {
        if clean_f.contains(clean_q) {
            if let Ok(bytes) = std::fs::read(path) {
                if Font::from_bytes(bytes.as_slice(), FontSettings::default()).is_ok() {
                    return Some((bytes, path.clone()));
                }
            }
        }
    }

    // 2. Multi-token fallback match (e.g. "noto mono" matching "notosansmnerdfontmono" or "notosanstc")
    let tokens: Vec<&str> = if clean_q.contains("noto") && clean_q.contains("mono") {
        vec!["noto", "mono"]
    } else if clean_q.contains("noto") && clean_q.contains("nerd") {
        vec!["noto", "nerd"]
    } else if clean_q.contains("noto") && clean_q.contains("tc") {
        vec!["noto", "tc"]
    } else {
        vec![]
    };

    if !tokens.is_empty() {
        for (path, clean_f) in &files {
            if tokens.iter().all(|t| clean_f.contains(t)) {
                if let Ok(bytes) = std::fs::read(path) {
                    if Font::from_bytes(bytes.as_slice(), FontSettings::default()).is_ok() {
                        return Some((bytes, path.clone()));
                    }
                }
            }
        }
    }

    None
}

/// Load full configured font chain (Primary Monospace + Fallbacks)
pub fn load_configured_font_chain(
    family_str: &str,
    fallback_families: &[String],
) -> (Font, Option<Font>, Option<Font>, Vec<Font>) {
    let mut queries = Vec::new();

    // Comma-separated entries in family string
    for token in family_str.split(',') {
        let t = token.trim();
        if !t.is_empty() {
            queries.push(t.to_string());
        }
    }

    // Additional fallback families
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
        if let Some((bytes, path)) = find_font_file(q) {
            if let Ok(font) = Font::from_bytes(bytes, FontSettings::default()) {
                if primary_font.is_none() {
                    info!("Found primary font match for '{}': {:?}", q, path);
                    primary_font = Some(font);

                    // Try finding bold/italic variants
                    let bold_query = format!("{}Bold", q);
                    if let Some((b_bytes, _)) = find_font_file(&bold_query) {
                        bold_font = Font::from_bytes(b_bytes, FontSettings::default()).ok();
                    }
                    let italic_query = format!("{}Italic", q);
                    if let Some((i_bytes, _)) = find_font_file(&italic_query) {
                        italic_font = Font::from_bytes(i_bytes, FontSettings::default()).ok();
                    }
                } else {
                    info!("Found fallback font match for '{}': {:?}", q, path);
                    fallback_fonts.push(font);
                }
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
