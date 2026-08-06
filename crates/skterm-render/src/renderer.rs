//! Main Terminal Renderer combining wgpu Device, Queue, Surface, Atlas, and Pipeline.

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
}

impl TermRenderer {
    pub async fn new(window: Arc<Window>, font_size: f32) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

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

        let atlas = GlyphAtlas::new(&device, &queue, font_size, None)?;
        let pipeline = TerminalPipeline::new(&device, &config, &atlas)?;

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            atlas,
            pipeline,
            padding_x: 8.0,
            padding_y: 8.0,
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
            color: color_to_rgba(Color::DefaultBg, true),
        });

        // 2. Iterate through grid cells
        for (r_idx, row) in active_buf.grid.iter().enumerate() {
            let y = self.padding_y + (r_idx as f32 * cell_h);
            if y >= self.config.height as f32 {
                break;
            }

            for (c_idx, cell) in row.cells.iter().enumerate() {
                let x = self.padding_x + (c_idx as f32 * cell_w);
                if x >= self.config.width as f32 {
                    break;
                }

                // Custom Background
                let is_inverse = cell.flags.contains(CellFlags::INVERSE);
                let (fg_color, bg_color) = if is_inverse {
                    (cell.bg, cell.fg)
                } else {
                    (cell.fg, cell.bg)
                };

                if bg_color != Color::DefaultBg {
                    bg_instances.push(BgInstance {
                        rect: [x, y, cell_w, cell_h],
                        color: color_to_rgba(bg_color, true),
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
                                color: color_to_rgba(fg_color, false),
                            });
                        }
                    }
                }
            }
        }

        // 3. Render Cursor
        if active_buf.cursor.visible {
            let cur_x = self.padding_x + (active_buf.cursor.col as f32 * cell_w);
            let cur_y = self.padding_y + (active_buf.cursor.row as f32 * cell_h);
            let cursor_color = [0.85, 0.85, 0.95, 0.75];

            let (cx, cy, cw, ch) = match active_buf.cursor.shape {
                CursorShape::Block => (cur_x, cur_y, cell_w, cell_h),
                CursorShape::Beam => (cur_x, cur_y, 2.0, cell_h),
                CursorShape::Underline => (cur_x, cur_y + cell_h - 2.0, cell_w, 2.0),
            };

            bg_instances.push(BgInstance {
                rect: [cx, cy, cw, ch],
                color: cursor_color,
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
                            r: 0.08,
                            g: 0.08,
                            b: 0.12,
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
