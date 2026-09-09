//! Main Terminal Renderer combining wgpu Device, Queue, Surface, Atlas, Graphics Images, and Pipelines.

use crate::atlas::GlyphAtlas;
use crate::box_drawing::draw_custom_block_or_box_char;
use crate::pipeline::{color_to_rgba, BgInstance, GlyphInstance, ImageInstance, TerminalPipeline};
use skterm_core::{CellFlags, Color, CursorShape, ScreenGrid};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;
use wgpu::{
    Backends, CompositeAlphaMode, Device, DeviceDescriptor, Extent3d, Features, Instance,
    InstanceDescriptor, Limits, Origin3d, PowerPreference, PresentMode, Queue,
    RequestAdapterOptions, Surface, SurfaceConfiguration, TexelCopyBufferLayout,
    TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat, TextureUsages,
};
use winit::window::Window;

/// Theme color overrides for custom themes and ANSI palettes
#[derive(Debug, Clone, Copy)]
pub struct RenderTheme {
    pub default_fg: [f32; 4],
    pub default_bg: [f32; 4],
    pub cursor_color: [f32; 4],
    pub selection_color: [f32; 4],
    pub tab_bar_bg: [f32; 4],
    pub tab_active_bg: [f32; 4],
    pub tab_active_fg: [f32; 4],
    pub tab_inactive_fg: [f32; 4],
    pub border_color: [f32; 4],
    pub focused_border_color: [f32; 4],
    pub ansi_palette: Option<[[f32; 4]; 16]>,
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            default_fg: [0.96, 0.96, 0.97, 1.0],
            default_bg: [0.00, 0.00, 0.00, 1.0], // OLED Pure Black (#000000)
            cursor_color: [0.039, 0.518, 1.00, 0.85],
            selection_color: [0.18, 0.28, 0.45, 0.5],
            tab_bar_bg: [0.00, 0.00, 0.00, 1.0],
            tab_active_bg: [0.00, 0.00, 0.00, 1.0],
            tab_active_fg: [0.96, 0.96, 0.97, 1.0],
            tab_inactive_fg: [0.55, 0.58, 0.65, 1.0],
            border_color: [0.20, 0.22, 0.28, 1.0],
            focused_border_color: [0.039, 0.518, 1.00, 1.0],
            ansi_palette: None,
        }
    }
}

/// Render descriptor for an individual pane on screen
pub struct PaneRenderItem<'a> {
    pub grid: &'a ScreenGrid,
    pub rect_x: f32,
    pub rect_y: f32,
    pub rect_w: f32,
    pub rect_h: f32,
    pub is_focused: bool,
    pub search_matches: &'a [skterm_core::SearchMatch],
    pub active_search_idx: Option<usize>,
    pub search_query: Option<&'a str>,
}

/// Render descriptor for a tab item in the tab bar
pub struct TabItem {
    pub title: String,
    pub is_active: bool,
}

/// Render descriptor for an AI Agent item in the left sidebar
#[derive(Debug, Clone)]
pub struct AgentSidebarItem {
    pub tab_id: u32,
    pub pane_id: u32,
    pub agent_name: String,
    pub project_name: String,
    pub location_label: String,
    pub title_summary: String,
    pub is_active_output: bool,
    pub is_focused: bool,
}

pub struct TermRenderer {
    pub window: Arc<Window>,
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub atlas: GlyphAtlas,
    pub pipeline: TerminalPipeline,
    pub padding_x: f32,
    pub padding_y: f32,
    pub theme: RenderTheme,
    pub scale_factor: f32,
    pub font_size_pt: f32,
    pub font_family: String,
    pub fallback_families: Vec<String>,
    pub opacity: f32,
    pub image_sampler: wgpu::Sampler,
    pub image_cache: HashMap<u32, (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
}

impl TermRenderer {
    pub async fn new(window: Arc<Window>, font_size_pt: f32) -> anyhow::Result<Self> {
        Self::new_with_font(window, font_size_pt, "Menlo", &[]).await
    }

    pub async fn new_with_font(
        window: Arc<Window>,
        font_size_pt: f32,
        font_family: &str,
        fallback_families: &[String],
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let scale_factor = (window.scale_factor() as f32).max(1.0);

        let instance = Instance::new(&InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .ok_or_else(|| anyhow::anyhow!("Failed to find suitable GPU adapter"))?;

        info!("Selected GPU Adapter: {:?}", adapter.get_info());

        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    label: Some("skterm_render_device"),
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
                    memory_hints: Default::default(),
                },
                None,
            )
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width,
            height,
            present_mode: PresentMode::AutoVsync,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&device, &config);

        let physical_font_size = font_size_pt * scale_factor;
        let padding_x = 8.0 * scale_factor;
        let padding_y = 8.0 * scale_factor;

        let atlas = GlyphAtlas::new_with_family(
            &device,
            &queue,
            physical_font_size,
            font_family,
            fallback_families,
            None,
        )?;
        let pipeline = TerminalPipeline::new(&device, &config, &atlas)?;

        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skterm_image_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            atlas,
            pipeline,
            padding_x,
            padding_y,
            theme: RenderTheme::default(),
            scale_factor,
            font_size_pt,
            font_family: font_family.to_string(),
            fallback_families: fallback_families.to_vec(),
            opacity: 1.0,
            image_sampler,
            image_cache: HashMap::new(),
        })
    }

    pub fn update_padding(&mut self, pad_x: f32, pad_y: f32) {
        self.padding_x = pad_x * self.scale_factor;
        self.padding_y = pad_y * self.scale_factor;
    }

    pub fn update_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.05, 1.0);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.pipeline
                .update_screen_size(&self.queue, width as f32, height as f32);
        }
    }

    pub fn update_scale_factor(&mut self, new_scale: f32) -> anyhow::Result<()> {
        if (self.scale_factor - new_scale).abs() > 0.01 {
            self.scale_factor = new_scale;
            let physical_font_size = self.font_size_pt * new_scale;
            self.padding_x = 8.0 * new_scale;
            self.padding_y = 8.0 * new_scale;
            self.atlas = GlyphAtlas::new_with_family(
                &self.device,
                &self.queue,
                physical_font_size,
                &self.font_family,
                &self.fallback_families,
                None,
            )?;
            self.pipeline = TerminalPipeline::new(&self.device, &self.config, &self.atlas)?;
        }
        Ok(())
    }

    pub fn update_font(
        &mut self,
        font_family: &str,
        font_size_pt: f32,
        fallback_families: &[String],
    ) -> anyhow::Result<()> {
        self.font_family = font_family.to_string();
        self.font_size_pt = font_size_pt.clamp(8.0, 72.0);
        self.fallback_families = fallback_families.to_vec();

        let physical_font_size = self.font_size_pt * self.scale_factor;
        self.atlas = GlyphAtlas::new_with_family(
            &self.device,
            &self.queue,
            physical_font_size,
            &self.font_family,
            &self.fallback_families,
            None,
        )?;
        self.pipeline = TerminalPipeline::new(&self.device, &self.config, &self.atlas)?;
        Ok(())
    }

    pub fn update_font_size(&mut self, new_pt: f32) -> anyhow::Result<()> {
        let fam = self.font_family.clone();
        let fallbacks = self.fallback_families.clone();
        self.update_font(&fam, new_pt, &fallbacks)
    }

    pub fn set_theme(&mut self, theme: RenderTheme) {
        self.theme = theme;
    }

    pub fn cell_width(&self) -> f32 {
        self.atlas.cell_width
    }

    pub fn cell_height(&self) -> f32 {
        self.atlas.cell_height
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_line(
        &mut self,
        glyph_instances: &mut Vec<GlyphInstance>,
        text: &str,
        x: f32,
        y: f32,
        max_w: f32,
        color: [f32; 4],
        is_bold: bool,
    ) {
        let cell_w = self.atlas.cell_width;
        let baseline = self.atlas.baseline_offset;
        let mut cur_x = x;
        for c in text.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1) as f32 * cell_w;
            if cur_x + cw > x + max_w {
                break;
            }
            if let Some(glyph_info) = self.atlas.get_or_insert_glyph(&self.queue, c, is_bold, false)
            {
                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                    glyph_instances.push(GlyphInstance {
                        rect: [
                            cur_x + glyph_info.offset[0],
                            y + baseline - glyph_info.offset[1] - glyph_info.size[1],
                            glyph_info.size[0],
                            glyph_info.size[1],
                        ],
                        uv: [
                            glyph_info.uv_min[0],
                            glyph_info.uv_min[1],
                            glyph_info.uv_max[0],
                            glyph_info.uv_max[1],
                        ],
                        color,
                    });
                }
            }
            cur_x += cw;
        }
    }

    /// Render tabs, split panes, graphic images (Sixel/Kitty), separators, and text grids
    pub fn render_workspace(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
    ) -> anyhow::Result<()> {
        self.render_workspace_with_sidebar(
            tabs,
            panes,
            tab_bar_h,
            false,
            0.0,
            &[],
            None::<fn(&Device, &Queue, &wgpu::TextureView, &SurfaceConfiguration)>,
        )
    }

    /// Render workspace with optional custom overlay (e.g. egui UI).
    pub fn render_workspace_with_overlay<F>(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
        overlay_fn: Option<F>,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(&Device, &Queue, &wgpu::TextureView, &SurfaceConfiguration),
    {
        self.render_workspace_with_sidebar(tabs, panes, tab_bar_h, false, 0.0, &[], overlay_fn)
    }

    /// Render workspace with AI Agents Sidebar and optional overlay.
    #[allow(clippy::too_many_arguments)]
    pub fn render_workspace_with_sidebar<F>(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
        sidebar_open: bool,
        sidebar_w: f32,
        agents: &[AgentSidebarItem],
        overlay_fn: Option<F>,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(&Device, &Queue, &wgpu::TextureView, &SurfaceConfiguration),
    {
        let output = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Lost) => {
                self.resize(self.config.width, self.config.height);
                return Ok(());
            }
            Err(wgpu::SurfaceError::OutOfMemory) => {
                return Err(anyhow::anyhow!("GPU Out of memory"));
            }
            Err(e) => {
                return Err(anyhow::anyhow!("Surface error: {:?}", e));
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut bg_instances = Vec::new();
        let mut glyph_instances = Vec::new();
        let mut image_draw_calls: Vec<(u32, ImageInstance)> = Vec::new();

        let cell_w = self.atlas.cell_width;
        let cell_h = self.atlas.cell_height;
        let baseline = self.atlas.baseline_offset;
        let scale = self.scale_factor;
        let screen_w = self.config.width as f32;
        let screen_h = self.config.height as f32;

        // 1. Full Screen Background
        let mut full_bg = self.theme.default_bg;
        full_bg[3] *= self.opacity;

        bg_instances.push(BgInstance {
            rect: [0.0, 0.0, screen_w, screen_h],
            color: full_bg,
        });

        // 2. Tab Bar
        if tab_bar_h > 0.0 && !tabs.is_empty() {
            // 2.1 Tab Bar Header Background (Deep translucent frosted bar)
            bg_instances.push(BgInstance {
                rect: [0.0, 0.0, screen_w, tab_bar_h],
                color: [0.06, 0.07, 0.09, 1.0],
            });

            // Bottom 1px subtle divider
            bg_instances.push(BgInstance {
                rect: [0.0, tab_bar_h - 1.0, screen_w, 1.0],
                color: [0.18, 0.20, 0.26, 0.8],
            });

            // 2.1.5 AI Sidebar Toggle Button on the left (after macOS window controls)
            let base_x = if cfg!(target_os = "macos") {
                78.0 * scale
            } else {
                8.0 * scale
            };
            let tab_h = (tab_bar_h - 8.0 * scale).max(18.0);
            let tab_y = (tab_bar_h - tab_h) / 2.0;
            let ai_btn_w = 34.0 * scale;

            let ai_btn_bg = if sidebar_open {
                [0.08, 0.38, 0.78, 0.95]
            } else if !agents.is_empty() {
                [0.12, 0.24, 0.42, 0.85]
            } else {
                [0.12, 0.14, 0.18, 0.7]
            };
            bg_instances.push(BgInstance {
                rect: [base_x, tab_y, ai_btn_w, tab_h],
                color: ai_btn_bg,
            });
            bg_instances.push(BgInstance {
                rect: [base_x, tab_y, ai_btn_w, 1.0],
                color: [0.28, 0.35, 0.48, 0.8],
            });
            let ai_label = if agents.is_empty() {
                "AI".to_string()
            } else {
                format!("AI{}", agents.len())
            };
            let ai_text_x = base_x + (ai_btn_w - (ai_label.len() as f32 * cell_w)) / 2.0;
            let ai_text_y = (tab_bar_h - cell_h) / 2.0;
            self.draw_text_line(
                &mut glyph_instances,
                &ai_label,
                ai_text_x,
                ai_text_y,
                ai_btn_w,
                [0.95, 0.97, 1.0, 1.0],
                true,
            );

            // 2.2 Calculate Tab Capsule Geometry
            let start_x = base_x + ai_btn_w + 6.0 * scale;
            let plus_btn_w = 24.0 * scale;
            let tab_gap = 4.0 * scale;
            let avail_w = (screen_w - start_x - plus_btn_w - 16.0 * scale).max(80.0);
            let total_gaps = (tabs.len().saturating_sub(1) as f32) * tab_gap;
            let tab_w =
                ((avail_w - total_gaps) / tabs.len() as f32).clamp(75.0 * scale, 190.0 * scale);
            let tab_h = (tab_bar_h - 8.0 * scale).max(18.0);
            let tab_y = (tab_bar_h - tab_h) / 2.0;

            for (idx, tab) in tabs.iter().enumerate() {
                let tab_x = start_x + (idx as f32 * (tab_w + tab_gap));

                if tab.is_active {
                    // Active Tab: Floating Card Capsule with subtle border and Active Blue Indicator
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, tab_w, tab_h],
                        color: [0.15, 0.17, 0.23, 1.0],
                    });
                    // Top / Left / Right subtle border line
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, tab_w, 1.0],
                        color: [0.28, 0.32, 0.42, 0.8],
                    });
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, 1.0, tab_h],
                        color: [0.28, 0.32, 0.42, 0.5],
                    });
                    bg_instances.push(BgInstance {
                        rect: [tab_x + tab_w - 1.0, tab_y, 1.0, tab_h],
                        color: [0.28, 0.32, 0.42, 0.5],
                    });
                    // Active Blue Indicator line at bottom of capsule (#0A84FF)
                    bg_instances.push(BgInstance {
                        rect: [
                            tab_x + 3.0 * scale,
                            tab_y + tab_h - 2.0,
                            tab_w - 6.0 * scale,
                            2.0,
                        ],
                        color: [0.039, 0.518, 1.00, 1.0],
                    });
                } else {
                    // Inactive Tab: Subtle dark capsule
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, tab_w, tab_h],
                        color: [0.09, 0.10, 0.13, 0.6],
                    });
                }

                // Close "x" button on right side of tab
                let close_size = 12.0 * scale;
                let close_x = tab_x + tab_w - close_size - 6.0 * scale;
                let close_y = tab_y + (tab_h - close_size) / 2.0;

                if let Some(glyph_info) =
                    self.atlas
                        .get_or_insert_glyph(&self.queue, '×', false, false)
                {
                    if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                        let draw_x = close_x + (close_size - glyph_info.size[0]) / 2.0;
                        let draw_y = close_y + (close_size - glyph_info.size[1]) / 2.0;
                        let close_color = if tab.is_active {
                            [0.72, 0.76, 0.85, 0.9]
                        } else {
                            [0.45, 0.48, 0.55, 0.6]
                        };
                        glyph_instances.push(GlyphInstance {
                            rect: [draw_x, draw_y, glyph_info.size[0], glyph_info.size[1]],
                            uv: [
                                glyph_info.uv_min[0],
                                glyph_info.uv_min[1],
                                glyph_info.uv_max[0],
                                glyph_info.uv_max[1],
                            ],
                            color: close_color,
                        });
                    }
                }

                // Tab Title Text (Centered vertically)
                let text_start_x = tab_x + 10.0 * scale;
                let max_text_w = close_x - text_start_x - 4.0 * scale;
                let text_y = (tab_bar_h - cell_h) / 2.0;
                let text_color = if tab.is_active {
                    [0.98, 0.98, 1.00, 1.0]
                } else {
                    [0.62, 0.65, 0.73, 1.0]
                };

                for (ch_idx, c) in tab.title.chars().enumerate() {
                    let gx = text_start_x + (ch_idx as f32 * cell_w);
                    if gx + cell_w > text_start_x + max_text_w {
                        break;
                    }
                    if let Some(glyph_info) =
                        self.atlas.get_or_insert_glyph(&self.queue, c, false, false)
                    {
                        if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                            let draw_x = gx + glyph_info.offset[0];
                            let draw_y =
                                text_y + baseline - glyph_info.offset[1] - glyph_info.size[1];
                            glyph_instances.push(GlyphInstance {
                                rect: [draw_x, draw_y, glyph_info.size[0], glyph_info.size[1]],
                                uv: [
                                    glyph_info.uv_min[0],
                                    glyph_info.uv_min[1],
                                    glyph_info.uv_max[0],
                                    glyph_info.uv_max[1],
                                ],
                                color: text_color,
                            });
                        }
                    }
                }
            }

            // 2.3 New Tab "+" Button
            let last_tab_right = start_x + (tabs.len() as f32 * (tab_w + tab_gap));
            let plus_btn_x = last_tab_right + 2.0 * scale;
            let plus_btn_y = tab_y + (tab_h - plus_btn_w) / 2.0;

            bg_instances.push(BgInstance {
                rect: [plus_btn_x, plus_btn_y, plus_btn_w, plus_btn_w],
                color: [0.12, 0.14, 0.18, 0.7],
            });
            bg_instances.push(BgInstance {
                rect: [plus_btn_x, plus_btn_y, plus_btn_w, 1.0],
                color: [0.22, 0.25, 0.33, 0.6],
            });

            if let Some(glyph_info) = self
                .atlas
                .get_or_insert_glyph(&self.queue, '+', false, false)
            {
                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                    let draw_x = plus_btn_x + (plus_btn_w - glyph_info.size[0]) / 2.0;
                    let draw_y = plus_btn_y + (plus_btn_w - glyph_info.size[1]) / 2.0;
                    glyph_instances.push(GlyphInstance {
                        rect: [draw_x, draw_y, glyph_info.size[0], glyph_info.size[1]],
                        uv: [
                            glyph_info.uv_min[0],
                            glyph_info.uv_min[1],
                            glyph_info.uv_max[0],
                            glyph_info.uv_max[1],
                        ],
                        color: [0.80, 0.84, 0.94, 0.9],
                    });
                }
            }
        }

        // 2.5 AI Agents Sidebar Panel
        if sidebar_open && sidebar_w > 0.0 {
            let panel_y = tab_bar_h;
            let panel_h = (screen_h - tab_bar_h).max(0.0);

            // Sidebar background
            bg_instances.push(BgInstance {
                rect: [0.0, panel_y, sidebar_w, panel_h],
                color: [0.04, 0.05, 0.07, 0.98],
            });
            // Right border divider line
            bg_instances.push(BgInstance {
                rect: [sidebar_w - 1.0, panel_y, 1.0, panel_h],
                color: [0.18, 0.20, 0.26, 0.9],
            });

            // Header section
            let header_top = panel_y;
            let header_h = 38.0 * scale;
            bg_instances.push(BgInstance {
                rect: [0.0, header_top + header_h - 1.0, sidebar_w, 1.0],
                color: [0.15, 0.17, 0.22, 0.8],
            });

            let header_title = format!("AGENTS ({})", agents.len());
            self.draw_text_line(
                &mut glyph_instances,
                &header_title,
                12.0 * scale,
                header_top + (header_h - cell_h) / 2.0,
                sidebar_w - 78.0 * scale,
                [0.75, 0.80, 0.90, 1.0],
                true,
            );

            // "+ New" button in sidebar header
            let new_btn_w = 56.0 * scale;
            let new_btn_h = 22.0 * scale;
            let new_btn_x = sidebar_w - new_btn_w - 10.0 * scale;
            let new_btn_y = header_top + (header_h - new_btn_h) / 2.0;
            bg_instances.push(BgInstance {
                rect: [new_btn_x, new_btn_y, new_btn_w, new_btn_h],
                color: [0.10, 0.36, 0.72, 0.9],
            });
            let new_label = "+ New";
            let new_label_x = new_btn_x + (new_btn_w - (new_label.len() as f32 * cell_w)) / 2.0;
            self.draw_text_line(
                &mut glyph_instances,
                new_label,
                new_label_x,
                new_btn_y + (new_btn_h - cell_h) / 2.0,
                new_btn_w,
                [0.98, 0.98, 1.0, 1.0],
                true,
            );

            // Agent Cards List
            let card_h = 66.0 * scale;
            let card_gap = 6.0 * scale;
            let list_start_y = header_top + header_h + 8.0 * scale;
            let card_x = 8.0 * scale;
            let card_w = (sidebar_w - 16.0 * scale).max(20.0);

            if agents.is_empty() {
                self.draw_text_line(
                    &mut glyph_instances,
                    "No active CLI agents",
                    12.0 * scale,
                    list_start_y + 12.0 * scale,
                    card_w - 8.0 * scale,
                    [0.50, 0.54, 0.62, 0.9],
                    false,
                );
                self.draw_text_line(
                    &mut glyph_instances,
                    "Click '+ New' to spawn",
                    12.0 * scale,
                    list_start_y + 12.0 * scale + cell_h + 4.0 * scale,
                    card_w - 8.0 * scale,
                    [0.42, 0.46, 0.54, 0.85],
                    false,
                );
            } else {
                for (idx, item) in agents.iter().enumerate() {
                    let cy = list_start_y + idx as f32 * (card_h + card_gap);
                    if cy + card_h > screen_h {
                        break;
                    }

                    let card_bg = if item.is_focused {
                        [0.13, 0.21, 0.34, 0.95]
                    } else {
                        [0.09, 0.11, 0.15, 0.9]
                    };
                    bg_instances.push(BgInstance {
                        rect: [card_x, cy, card_w, card_h],
                        color: card_bg,
                    });

                    // Border
                    let border_col = if item.is_focused {
                        [0.039, 0.518, 1.00, 0.9]
                    } else {
                        [0.20, 0.23, 0.30, 0.6]
                    };
                    bg_instances.push(BgInstance {
                        rect: [card_x, cy, card_w, 1.0],
                        color: border_col,
                    });
                    bg_instances.push(BgInstance {
                        rect: [card_x, cy + card_h - 1.0, card_w, 1.0],
                        color: border_col,
                    });

                    // Left active accent bar
                    if item.is_focused {
                        bg_instances.push(BgInstance {
                            rect: [card_x, cy, 3.0 * scale, card_h],
                            color: [0.039, 0.518, 1.00, 1.0],
                        });
                    }

                    // Status dot (Green = Active/Thinking, Yellow = Waiting/Idle)
                    let dot_size = 7.0 * scale;
                    let dot_x = card_x + 10.0 * scale;
                    let dot_y = cy + 9.0 * scale + (cell_h - dot_size) / 2.0;
                    let dot_color = if item.is_active_output {
                        [0.18, 0.88, 0.46, 1.0]
                    } else {
                        [0.96, 0.76, 0.22, 1.0]
                    };
                    bg_instances.push(BgInstance {
                        rect: [dot_x, dot_y, dot_size, dot_size],
                        color: dot_color,
                    });

                    // Determine if a custom task name or active rename is present
                    let has_custom_task = !item.title_summary.is_empty()
                        && item.title_summary != "Double-click to name...";

                    let (top_title, bottom_subtitle) = if has_custom_task {
                        let status_str = if item.is_active_output {
                            "Running"
                        } else {
                            "Idle"
                        };
                        (
                            item.title_summary.as_str(),
                            format!("{} • {}", item.agent_name, status_str),
                        )
                    } else {
                        let status_str = if item.is_active_output {
                            "Running"
                        } else {
                            "Idle"
                        };
                        (
                            item.agent_name.as_str(),
                            format!("{} • Double-click to name...", status_str),
                        )
                    };

                    // Line 1 (Prominent Title): Custom Task Name (or Agent Name if unnamed) + Location label
                    let text_x = dot_x + dot_size + 6.0 * scale;
                    let loc_w = (item.location_label.len() as f32 * cell_w) + 8.0 * scale;
                    self.draw_text_line(
                        &mut glyph_instances,
                        top_title,
                        text_x,
                        cy + 6.0 * scale,
                        (card_w - (text_x - card_x) - loc_w).max(20.0),
                        [0.98, 0.99, 1.0, 1.0],
                        true,
                    );
                    self.draw_text_line(
                        &mut glyph_instances,
                        &item.location_label,
                        card_x + card_w - loc_w + 2.0 * scale,
                        cy + 6.0 * scale,
                        loc_w,
                        [0.65, 0.75, 0.90, 0.9],
                        false,
                    );

                    // Line 2: Project / Directory Name
                    let proj_label = format!("▸ {}", item.project_name);
                    self.draw_text_line(
                        &mut glyph_instances,
                        &proj_label,
                        card_x + 10.0 * scale,
                        cy + 24.0 * scale,
                        card_w - 16.0 * scale,
                        [0.45, 0.82, 0.96, 0.95],
                        false,
                    );

                    // Line 3 (Smaller/Subtler Subtitle): Agent Type • Status
                    self.draw_text_line(
                        &mut glyph_instances,
                        &bottom_subtitle,
                        card_x + 10.0 * scale,
                        cy + 42.0 * scale,
                        card_w - 16.0 * scale,
                        [0.58, 0.63, 0.72, 0.88],
                        false,
                    );
                }
            }
        }

        // 3. Panes and Images
        for pane in panes {
            let px = pane.rect_x;
            let py = pane.rect_y;
            let pw = pane.rect_w;
            let ph = pane.rect_h;

            bg_instances.push(BgInstance {
                rect: [px, py, pw, ph],
                color: self.theme.default_bg,
            });

            let border_col = if pane.is_focused && panes.len() > 1 {
                self.theme.focused_border_color
            } else {
                self.theme.border_color
            };

            if panes.len() > 1 {
                bg_instances.push(BgInstance {
                    rect: [px, py, pw, 1.0],
                    color: border_col,
                });
                bg_instances.push(BgInstance {
                    rect: [px, py + ph - 1.0, pw, 1.0],
                    color: border_col,
                });
                bg_instances.push(BgInstance {
                    rect: [px, py, 1.0, ph],
                    color: border_col,
                });
                bg_instances.push(BgInstance {
                    rect: [px + pw - 1.0, py, 1.0, ph],
                    color: border_col,
                });
            }

            let active_buf = pane.grid.active();

            // Sixel / Kitty Images in this pane
            for img in &active_buf.images {
                if !self.image_cache.contains_key(&img.id) && img.width_px > 0 && img.height_px > 0
                {
                    let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some(&format!("skterm_image_{}", img.id)),
                        size: Extent3d {
                            width: img.width_px,
                            height: img.height_px,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: TextureFormat::Rgba8Unorm,
                        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                        view_formats: &[],
                    });

                    self.queue.write_texture(
                        TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: 0,
                            origin: Origin3d::ZERO,
                            aspect: TextureAspect::All,
                        },
                        &img.rgba_data,
                        TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(img.width_px * 4),
                            rows_per_image: Some(img.height_px),
                        },
                        Extent3d {
                            width: img.width_px,
                            height: img.height_px,
                            depth_or_array_layers: 1,
                        },
                    );

                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("skterm_image_bg_{}", img.id)),
                        layout: &self.pipeline.image_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                            },
                        ],
                    });

                    self.image_cache.insert(img.id, (texture, view, bind_group));
                }

                let ix = px + self.padding_x + (img.col as f32 * cell_w);
                let iy = py + self.padding_y + (img.row as f32 * cell_h);
                let iw = (img.width_cells as f32 * cell_w).min(pw);
                let ih = (img.height_cells as f32 * cell_h).min(ph);

                if ix < px + pw && iy < py + ph {
                    image_draw_calls.push((
                        img.id,
                        ImageInstance {
                            rect: [ix, iy, iw, ih],
                        },
                    ));
                }
            }

            // Grid Cells
            let top_global_row = active_buf
                .scrollback
                .len()
                .saturating_sub(active_buf.scroll_offset);

            for r_idx in 0..active_buf.rows {
                let y = py + self.padding_y + (r_idx as f32 * cell_h);
                if y + cell_h > py + ph {
                    break;
                }
                let global_row = top_global_row + r_idx;

                if let Some(row) = active_buf.get_visible_row(r_idx) {
                    for (c_idx, cell) in row.cells.iter().enumerate() {
                        let x = px + self.padding_x + (c_idx as f32 * cell_w);
                        if x + cell_w > px + pw {
                            break;
                        }

                        let is_selected = active_buf
                            .selection
                            .as_ref()
                            .map(|s| s.contains(r_idx, c_idx))
                            .unwrap_or(false);

                        let is_inverse = cell.flags.contains(CellFlags::INVERSE);
                        let is_bold_flag = cell.flags.contains(CellFlags::BOLD);
                        let (fg_rgba, bg_rgba) = if is_inverse {
                            (
                                color_to_rgba_themed(cell.bg, false, is_bold_flag, &self.theme),
                                color_to_rgba_themed(cell.fg, true, false, &self.theme),
                            )
                        } else {
                            (
                                color_to_rgba_themed(cell.fg, false, is_bold_flag, &self.theme),
                                color_to_rgba_themed(cell.bg, true, false, &self.theme),
                            )
                        };

                        if bg_rgba != self.theme.default_bg {
                            bg_instances.push(BgInstance {
                                rect: [x, y, cell_w, cell_h],
                                color: bg_rgba,
                            });
                        }

                        // Highlight search matches
                        if !pane.search_matches.is_empty() {
                            let mut match_highlight = None;
                            for (m_idx, m) in pane.search_matches.iter().enumerate() {
                                if m.global_row == global_row
                                    && c_idx >= m.start_col
                                    && c_idx <= m.end_col
                                {
                                    if pane.active_search_idx == Some(m_idx) {
                                        match_highlight = Some([0.98, 0.68, 0.12, 0.90]);
                                        break;
                                    } else {
                                        match_highlight = Some([0.85, 0.65, 0.15, 0.42]);
                                    }
                                }
                            }
                            if let Some(hl_col) = match_highlight {
                                bg_instances.push(BgInstance {
                                    rect: [x, y, cell_w, cell_h],
                                    color: hl_col,
                                });
                            }
                        }

                        if is_selected {
                            bg_instances.push(BgInstance {
                                rect: [x, y, cell_w, cell_h],
                                color: self.theme.selection_color,
                            });
                        }

                        if cell.c != ' '
                            && cell.c != '\0'
                            && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER)
                        {
                            // Procedural gap-free rendering for Unicode Block Elements and Box Drawing
                            if draw_custom_block_or_box_char(
                                cell.c,
                                x,
                                y,
                                cell_w,
                                cell_h,
                                fg_rgba,
                                &mut bg_instances,
                            ) {
                                continue;
                            }

                            let is_bold = cell.flags.contains(CellFlags::BOLD);
                            let is_italic = cell.flags.contains(CellFlags::ITALIC);

                            if let Some(glyph_info) = self.atlas.get_or_insert_glyph(
                                &self.queue,
                                cell.c,
                                is_bold,
                                is_italic,
                            ) {
                                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                                    let glyph_x = x + glyph_info.offset[0];
                                    let glyph_y =
                                        y + baseline - glyph_info.offset[1] - glyph_info.size[1];

                                    glyph_instances.push(GlyphInstance {
                                        rect: [
                                            glyph_x,
                                            glyph_y,
                                            glyph_info.size[0],
                                            glyph_info.size[1],
                                        ],
                                        uv: [
                                            glyph_info.uv_min[0],
                                            glyph_info.uv_min[1],
                                            glyph_info.uv_max[0],
                                            glyph_info.uv_max[1],
                                        ],
                                        color: fg_rgba,
                                    });
                                }
                            }
                        }
                    }
                }
            }

            // Cursor
            if pane.is_focused && active_buf.cursor.visible && active_buf.scroll_offset == 0 {
                let cur_x = px + self.padding_x + (active_buf.cursor.col as f32 * cell_w);
                let cur_y = py + self.padding_y + (active_buf.cursor.row as f32 * cell_h);

                if cur_x + cell_w <= px + pw && cur_y + cell_h <= py + ph {
                    let (cx, cy, cw, ch) = match active_buf.cursor.shape {
                        CursorShape::Block => (cur_x, cur_y, cell_w, cell_h),
                        CursorShape::Beam => (cur_x, cur_y, 2.0 * self.scale_factor, cell_h),
                        CursorShape::Underline => (
                            cur_x,
                            cur_y + cell_h - 2.0 * self.scale_factor,
                            cell_w,
                            2.0 * self.scale_factor,
                        ),
                    };

                    bg_instances.push(BgInstance {
                        rect: [cx, cy, cw, ch],
                        color: self.theme.cursor_color,
                    });
                }
            }

            // Floating Find Bar overlay (Cmd+F)
            if let Some(query) = pane.search_query {
                let count_str = if pane.search_matches.is_empty() {
                    "0/0".to_string()
                } else {
                    format!(
                        "{}/{}",
                        pane.active_search_idx.unwrap_or(0) + 1,
                        pane.search_matches.len()
                    )
                };
                let label = format!("Find: {}▏  ({})", query, count_str);
                let bar_w = ((label.chars().count() as f32 * cell_w) + 24.0 * scale)
                    .clamp(220.0 * scale, (pw - 24.0 * scale).max(120.0));
                let bar_h = cell_h + 12.0 * scale;
                let bar_x = (px + pw - bar_w - 12.0 * scale).max(px + 4.0);
                let bar_y = py + 8.0 * scale;

                // Capsule background
                bg_instances.push(BgInstance {
                    rect: [bar_x, bar_y, bar_w, bar_h],
                    color: [0.10, 0.12, 0.17, 0.96],
                });
                // Top accent line
                bg_instances.push(BgInstance {
                    rect: [bar_x, bar_y, bar_w, 2.0 * scale],
                    color: [0.039, 0.518, 1.00, 0.95],
                });

                self.draw_text_line(
                    &mut glyph_instances,
                    &label,
                    bar_x + 12.0 * scale,
                    bar_y + 6.0 * scale,
                    bar_w - 16.0 * scale,
                    [0.96, 0.97, 1.0, 1.0],
                    false,
                );
            }
        }

        // Upload instances to GPU buffers
        let required_capacity = bg_instances.len().max(glyph_instances.len());
        self.pipeline
            .ensure_bg_capacity(&self.device, required_capacity);

        if !bg_instances.is_empty() {
            self.queue.write_buffer(
                &self.pipeline.bg_instance_buffer,
                0,
                bytemuck::cast_slice(&bg_instances),
            );
        }

        if !glyph_instances.is_empty() {
            self.queue.write_buffer(
                &self.pipeline.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&glyph_instances),
            );
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("skterm_render_encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("skterm_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.theme.default_bg[0] as f64,
                            g: self.theme.default_bg[1] as f64,
                            b: self.theme.default_bg[2] as f64,
                            a: self.theme.default_bg[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            render_pass.set_bind_group(0, &self.pipeline.bind_group, &[]);

            // 1. Draw Background Quads
            if !bg_instances.is_empty() {
                render_pass.set_pipeline(&self.pipeline.bg_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.bg_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..bg_instances.len() as u32);
            }

            // 2. Draw Graphic Images (Sixel / Kitty)
            for (id, img_inst) in &image_draw_calls {
                if let Some((_, _, img_bg)) = self.image_cache.get(id) {
                    self.queue.write_buffer(
                        &self.pipeline.image_instance_buffer,
                        0,
                        bytemuck::cast_slice(&[*img_inst]),
                    );
                    render_pass.set_pipeline(&self.pipeline.image_pipeline);
                    render_pass.set_bind_group(1, img_bg, &[]);
                    render_pass.set_vertex_buffer(0, self.pipeline.image_instance_buffer.slice(..));
                    render_pass.draw(0..6, 0..1);
                }
            }

            // 3. Draw Text Glyphs
            if !glyph_instances.is_empty() {
                render_pass.set_pipeline(&self.pipeline.glyph_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.glyph_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..glyph_instances.len() as u32);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some(overlay) = overlay_fn {
            overlay(&self.device, &self.queue, &view, &self.config);
        }

        output.present();

        Ok(())
    }
}

/// Convert skterm Color to RGBA considering active theme, custom palette, and bold_is_bright.
pub fn color_to_rgba_themed(
    color: Color,
    is_bg: bool,
    is_bold: bool,
    theme: &RenderTheme,
) -> [f32; 4] {
    match color {
        Color::DefaultFg => {
            if is_bold && !is_bg {
                if let Some(palette) = theme.ansi_palette {
                    return palette[15];
                }
            }
            theme.default_fg
        }
        Color::DefaultBg => theme.default_bg,
        Color::Named(named) => {
            let mut idx = named as usize;
            if is_bold && !is_bg && idx < 8 {
                idx += 8;
            }
            if let Some(palette) = theme.ansi_palette {
                if idx < 16 {
                    return palette[idx];
                }
            }
            color_to_rgba(color, is_bg)
        }
        Color::Indexed(idx) if idx < 16 => {
            let mut target_idx = idx as usize;
            if is_bold && !is_bg && target_idx < 8 {
                target_idx += 8;
            }
            if let Some(palette) = theme.ansi_palette {
                return palette[target_idx];
            }
            color_to_rgba(color, is_bg)
        }
        _ => color_to_rgba(color, is_bg),
    }
}
