//! Main Terminal Renderer combining wgpu Device, Queue, Surface, Atlas, Graphics Images, and Pipelines.

use crate::atlas::GlyphAtlas;
use crate::pipeline::{color_to_rgba, BgInstance, GlyphInstance, ImageInstance, TerminalPipeline};
use skterm_core::{CellFlags, Color, CursorShape, ScreenGrid};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;
use wgpu::{
    Backends, CompositeAlphaMode, Device, DeviceDescriptor, Extent3d, Features, Instance,
    InstanceDescriptor, Limits, Origin3d, PowerPreference, PresentMode, Queue, RequestAdapterOptions,
    Surface, SurfaceConfiguration, TexelCopyBufferLayout, TexelCopyTextureInfo, TextureAspect,
    TextureDimension, TextureFormat, TextureUsages,
};
use winit::window::Window;

/// Theme color overrides for custom themes
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
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            default_fg: [0.80, 0.84, 0.96, 1.0],
            default_bg: [0.12, 0.12, 0.18, 1.0],
            cursor_color: [0.96, 0.88, 0.79, 0.8],
            selection_color: [0.36, 0.39, 0.49, 0.45],
            tab_bar_bg: [0.09, 0.09, 0.14, 1.0],
            tab_active_bg: [0.12, 0.12, 0.18, 1.0],
            tab_active_fg: [0.80, 0.84, 0.96, 1.0],
            tab_inactive_fg: [0.50, 0.52, 0.60, 1.0],
            border_color: [0.19, 0.20, 0.27, 1.0],
            focused_border_color: [0.55, 0.65, 0.90, 1.0],
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
}

/// Render descriptor for a tab item in the tab bar
pub struct TabItem {
    pub title: String,
    pub is_active: bool,
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
    pub image_sampler: wgpu::Sampler,
    pub image_cache: HashMap<u32, (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,
}

impl TermRenderer {
    pub async fn new(window: Arc<Window>, font_size_pt: f32) -> anyhow::Result<Self> {
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

        let atlas = GlyphAtlas::new(&device, &queue, physical_font_size, None)?;
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
            image_sampler,
            image_cache: HashMap::new(),
        })
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
            self.atlas = GlyphAtlas::new(&self.device, &self.queue, physical_font_size, None)?;
            self.pipeline = TerminalPipeline::new(&self.device, &self.config, &self.atlas)?;
        }
        Ok(())
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

    /// Render tabs, split panes, graphic images (Sixel/Kitty), separators, and text grids
    pub fn render_workspace(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
    ) -> anyhow::Result<()> {
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

        // 1. Full Screen Background
        bg_instances.push(BgInstance {
            rect: [
                0.0,
                0.0,
                self.config.width as f32,
                self.config.height as f32,
            ],
            color: self.theme.default_bg,
        });

        // 2. Tab Bar
        if tab_bar_h > 0.0 && !tabs.is_empty() {
            bg_instances.push(BgInstance {
                rect: [0.0, 0.0, self.config.width as f32, tab_bar_h],
                color: self.theme.tab_bar_bg,
            });

            let tab_w = 140.0 * self.scale_factor;
            for (idx, tab) in tabs.iter().enumerate() {
                let tab_x = (idx as f32 * tab_w).min(self.config.width as f32 - tab_w);
                let bg_color = if tab.is_active {
                    self.theme.tab_active_bg
                } else {
                    self.theme.tab_bar_bg
                };
                let fg_color = if tab.is_active {
                    self.theme.tab_active_fg
                } else {
                    self.theme.tab_inactive_fg
                };

                bg_instances.push(BgInstance {
                    rect: [tab_x, 0.0, tab_w, tab_bar_h],
                    color: bg_color,
                });

                bg_instances.push(BgInstance {
                    rect: [tab_x + tab_w - 1.0, 0.0, 1.0, tab_bar_h],
                    color: self.theme.border_color,
                });

                let text_start_x = tab_x + 12.0 * self.scale_factor;
                let text_y = (tab_bar_h - cell_h) / 2.0;

                for (ch_idx, c) in tab.title.chars().enumerate() {
                    let gx = text_start_x + (ch_idx as f32 * cell_w);
                    if gx + cell_w > tab_x + tab_w - 10.0 {
                        break;
                    }
                    if let Some(glyph_info) = self.atlas.get_or_insert_glyph(&self.queue, c, false, false) {
                        if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                            let draw_x = gx + glyph_info.offset[0];
                            let draw_y = text_y + baseline - glyph_info.offset[1] - glyph_info.size[1];
                            glyph_instances.push(GlyphInstance {
                                rect: [draw_x, draw_y, glyph_info.size[0], glyph_info.size[1]],
                                uv: [
                                    glyph_info.uv_min[0],
                                    glyph_info.uv_min[1],
                                    glyph_info.uv_max[0],
                                    glyph_info.uv_max[1],
                                ],
                                color: fg_color,
                            });
                        }
                    }
                }
            }

            bg_instances.push(BgInstance {
                rect: [0.0, tab_bar_h - 1.0, self.config.width as f32, 1.0],
                color: self.theme.border_color,
            });
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
                if !self.image_cache.contains_key(&img.id) && img.width_px > 0 && img.height_px > 0 {
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
            for r_idx in 0..active_buf.rows {
                let y = py + self.padding_y + (r_idx as f32 * cell_h);
                if y + cell_h > py + ph {
                    break;
                }

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
                        let (fg_rgba, bg_rgba) = if is_inverse {
                            (
                                color_to_rgba_themed(cell.bg, false, &self.theme),
                                color_to_rgba_themed(cell.fg, true, &self.theme),
                            )
                        } else {
                            (
                                color_to_rgba_themed(cell.fg, false, &self.theme),
                                color_to_rgba_themed(cell.bg, true, &self.theme),
                            )
                        };

                        if bg_rgba != self.theme.default_bg {
                            bg_instances.push(BgInstance {
                                rect: [x, y, cell_w, cell_h],
                                color: bg_rgba,
                            });
                        }

                        if is_selected {
                            bg_instances.push(BgInstance {
                                rect: [x, y, cell_w, cell_h],
                                color: self.theme.selection_color,
                            });
                        }

                        if cell.c != ' ' && cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                            let is_bold = cell.flags.contains(CellFlags::BOLD);
                            let is_italic = cell.flags.contains(CellFlags::ITALIC);

                            if let Some(glyph_info) = self.atlas.get_or_insert_glyph(&self.queue, cell.c, is_bold, is_italic) {
                                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                                    let glyph_x = x + glyph_info.offset[0];
                                    let glyph_y = y + baseline - glyph_info.offset[1] - glyph_info.size[1];

                                    glyph_instances.push(GlyphInstance {
                                        rect: [glyph_x, glyph_y, glyph_info.size[0], glyph_info.size[1]],
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
                        CursorShape::Underline => (cur_x, cur_y + cell_h - 2.0 * self.scale_factor, cell_w, 2.0 * self.scale_factor),
                    };

                    bg_instances.push(BgInstance {
                        rect: [cx, cy, cw, ch],
                        color: self.theme.cursor_color,
                    });
                }
            }
        }

        // Upload instances to GPU buffers
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
                            a: 1.0,
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
        output.present();

        Ok(())
    }
}

/// Convert skterm Color to RGBA considering active theme.
pub fn color_to_rgba_themed(color: Color, is_bg: bool, theme: &RenderTheme) -> [f32; 4] {
    match color {
        Color::DefaultFg => theme.default_fg,
        Color::DefaultBg => theme.default_bg,
        _ => color_to_rgba(color, is_bg),
    }
}
