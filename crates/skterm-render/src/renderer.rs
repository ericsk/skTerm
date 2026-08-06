//! Main Terminal Renderer combining wgpu Device, Queue, Surface, Atlas, and Pipeline with HiDPI support.

use crate::atlas::GlyphAtlas;
use crate::pipeline::{color_to_rgba, BgInstance, GlyphInstance, TerminalPipeline};
use skterm_core::{CellFlags, Color, CursorShape, ScreenGrid};
use std::sync::Arc;
use tracing::info;
use wgpu::{
    Backends, CompositeAlphaMode, Device, DeviceDescriptor, Features, Instance, InstanceDescriptor,
    Limits, PowerPreference, PresentMode, Queue, RequestAdapterOptions, Surface,
    SurfaceConfiguration, TextureUsages,
};
use winit::window::Window;

/// Theme color overrides for custom themes
#[derive(Debug, Clone, Copy)]
pub struct RenderTheme {
    pub default_fg: [f32; 4],
    pub default_bg: [f32; 4],
    pub cursor_color: [f32; 4],
    pub selection_color: [f32; 4],
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            default_fg: [0.80, 0.84, 0.96, 1.0],
            default_bg: [0.12, 0.12, 0.18, 1.0],
            cursor_color: [0.96, 0.88, 0.79, 0.8],
            selection_color: [0.36, 0.39, 0.49, 0.45],
        }
    }
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

        // Calculate physical font size based on Retina / HiDPI scale factor
        let physical_font_size = font_size_pt * scale_factor;
        let padding_x = 8.0 * scale_factor;
        let padding_y = 8.0 * scale_factor;

        let atlas = GlyphAtlas::new(&device, &queue, physical_font_size, None)?;
        let pipeline = TerminalPipeline::new(&device, &config, &atlas)?;

        info!(
            "HiDPI Init: scale_factor={}, font_size_pt={}, physical_font_size={}, cell_w={}, cell_h={}",
            scale_factor, font_size_pt, physical_font_size, atlas.cell_width, atlas.cell_height
        );

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

    pub fn render(&mut self, grid: &ScreenGrid) -> anyhow::Result<()> {
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

        let active_buf = grid.active();
        let cell_w = self.atlas.cell_width;
        let cell_h = self.atlas.cell_height;
        let baseline = self.atlas.baseline_offset;

        // 1. Draw Default Window Background Quad across full screen
        bg_instances.push(BgInstance {
            rect: [
                0.0,
                0.0,
                self.config.width as f32,
                self.config.height as f32,
            ],
            color: self.theme.default_bg,
        });

        // 2. Iterate through visible lines (support scrollback history)
        for r_idx in 0..active_buf.rows {
            let y = self.padding_y + (r_idx as f32 * cell_h);
            if y >= self.config.height as f32 {
                break;
            }

            if let Some(row) = active_buf.get_visible_row(r_idx) {
                for (c_idx, cell) in row.cells.iter().enumerate() {
                    let x = self.padding_x + (c_idx as f32 * cell_w);
                    if x >= self.config.width as f32 {
                        break;
                    }

                    // Selection highlight
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

                    // Draw Cell Background
                    if bg_rgba != self.theme.default_bg {
                        bg_instances.push(BgInstance {
                            rect: [x, y, cell_w, cell_h],
                            color: bg_rgba,
                        });
                    }

                    // Draw Selection Highlight
                    if is_selected {
                        bg_instances.push(BgInstance {
                            rect: [x, y, cell_w, cell_h],
                            color: self.theme.selection_color,
                        });
                    }

                    // Render Char Glyph
                    if cell.c != ' ' && cell.c != '\0' && !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER)
                    {
                        let is_bold = cell.flags.contains(CellFlags::BOLD);
                        let is_italic = cell.flags.contains(CellFlags::ITALIC);

                        if let Some(glyph_info) =
                            self.atlas.get_or_insert_glyph(&self.queue, cell.c, is_bold, is_italic)
                        {
                            if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                                let glyph_x = x + glyph_info.offset[0];
                                let glyph_y = y + baseline - glyph_info.offset[1] - glyph_info.size[1];

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

        // 3. Render Cursor (only when at bottom / active view)
        if active_buf.cursor.visible && active_buf.scroll_offset == 0 {
            let cur_x = self.padding_x + (active_buf.cursor.col as f32 * cell_w);
            let cur_y = self.padding_y + (active_buf.cursor.row as f32 * cell_h);

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

        // Write instance data to GPU buffers
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

            // Draw Background Quads
            if !bg_instances.is_empty() {
                render_pass.set_pipeline(&self.pipeline.bg_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.bg_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..bg_instances.len() as u32);
            }

            // Draw Text Glyphs
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
