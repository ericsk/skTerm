//! GPU render pipeline setup, instance buffers, and batch submission for background, text, and images.

use crate::atlas::GlyphAtlas;
use crate::shader::TERMINAL_SHADER;
use bytemuck::{Pod, Zeroable};
use skterm_core::{Color, NamedColor};
use wgpu::util::DeviceExt;
use wgpu::{
    BindGroup, BindGroupLayout, Buffer, BufferUsages, ColorTargetState, ColorWrites, Device,
    FragmentState, MultisampleState, PipelineCompilationOptions, PipelineLayoutDescriptor,
    PrimitiveState, PrimitiveTopology, Queue, RenderPipeline, RenderPipelineDescriptor,
    SurfaceConfiguration, VertexBufferLayout, VertexState, VertexStepMode,
};

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct Uniforms {
    pub screen_size: [f32; 2],
    pub padding: [f32; 2],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct BgInstance {
    pub rect: [f32; 4],
    pub color: [f32; 4],
}

impl BgInstance {
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<BgInstance>() as wgpu::BufferAddress,
            step_mode: VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct GlyphInstance {
    pub rect: [f32; 4],
    pub uv: [f32; 4],
    pub color: [f32; 4],
}

impl GlyphInstance {
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<GlyphInstance>() as wgpu::BufferAddress,
            step_mode: VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: std::mem::size_of::<[f32; 4]>() as wgpu::BufferAddress,
                    shader_location: 1,
                    format: wgpu::VertexFormat::Float32x4,
                },
                wgpu::VertexAttribute {
                    offset: (std::mem::size_of::<[f32; 4]>() * 2) as wgpu::BufferAddress,
                    shader_location: 2,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct ImageInstance {
    pub rect: [f32; 4],
}

impl ImageInstance {
    pub fn desc() -> VertexBufferLayout<'static> {
        VertexBufferLayout {
            array_stride: std::mem::size_of::<ImageInstance>() as wgpu::BufferAddress,
            step_mode: VertexStepMode::Instance,
            attributes: &[
                wgpu::VertexAttribute {
                    offset: 0,
                    shader_location: 0,
                    format: wgpu::VertexFormat::Float32x4,
                },
            ],
        }
    }
}

pub struct TerminalPipeline {
    pub bg_pipeline: RenderPipeline,
    pub glyph_pipeline: RenderPipeline,
    pub image_pipeline: RenderPipeline,
    pub uniform_buffer: Buffer,
    pub bg_instance_buffer: Buffer,
    pub glyph_instance_buffer: Buffer,
    pub image_instance_buffer: Buffer,
    pub bind_group: BindGroup,
    pub bind_group_layout: BindGroupLayout,
    pub image_bind_group_layout: BindGroupLayout,
    pub max_instances: usize,
}

impl TerminalPipeline {
    pub const INITIAL_MAX_INSTANCES: usize = 32768;

    pub fn new(
        device: &Device,
        config: &SurfaceConfiguration,
        atlas: &GlyphAtlas,
    ) -> anyhow::Result<Self> {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("skterm_terminal_shader"),
            source: wgpu::ShaderSource::Wgsl(TERMINAL_SHADER.into()),
        });

        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("skterm_uniform_buffer"),
            contents: bytemuck::cast_slice(&[Uniforms {
                screen_size: [config.width as f32, config.height as f32],
                padding: [0.0, 0.0],
            }]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skterm_bind_group_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("skterm_bind_group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&atlas.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&atlas.sampler),
                },
            ],
        });

        let image_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("skterm_image_bind_group_layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("skterm_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let image_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("skterm_image_pipeline_layout"),
            bind_group_layouts: &[&bind_group_layout, &image_bind_group_layout],
            push_constant_ranges: &[],
        });

        let bg_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("skterm_bg_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_bg"),
                buffers: &[BgInstance::desc()],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_bg"),
                targets: &[Some(ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let glyph_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("skterm_glyph_pipeline"),
            layout: Some(&pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_glyph"),
                buffers: &[GlyphInstance::desc()],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_glyph"),
                targets: &[Some(ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let image_pipeline = device.create_render_pipeline(&RenderPipelineDescriptor {
            label: Some("skterm_image_pipeline"),
            layout: Some(&image_pipeline_layout),
            vertex: VertexState {
                module: &shader,
                entry_point: Some("vs_image"),
                buffers: &[ImageInstance::desc()],
                compilation_options: PipelineCompilationOptions::default(),
            },
            fragment: Some(FragmentState {
                module: &shader,
                entry_point: Some("fs_image"),
                targets: &[Some(ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: ColorWrites::ALL,
                })],
                compilation_options: PipelineCompilationOptions::default(),
            }),
            primitive: PrimitiveState {
                topology: PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                unclipped_depth: false,
                polygon_mode: wgpu::PolygonMode::Fill,
                conservative: false,
            },
            depth_stencil: None,
            multisample: MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        let bg_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skterm_bg_instance_buffer"),
            size: (Self::INITIAL_MAX_INSTANCES * std::mem::size_of::<BgInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let glyph_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skterm_glyph_instance_buffer"),
            size: (Self::INITIAL_MAX_INSTANCES * std::mem::size_of::<GlyphInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let image_instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skterm_image_instance_buffer"),
            size: (1024 * std::mem::size_of::<ImageInstance>()) as u64,
            usage: BufferUsages::VERTEX | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            bg_pipeline,
            glyph_pipeline,
            image_pipeline,
            uniform_buffer,
            bg_instance_buffer,
            glyph_instance_buffer,
            image_instance_buffer,
            bind_group,
            bind_group_layout,
            image_bind_group_layout,
            max_instances: Self::INITIAL_MAX_INSTANCES,
        })
    }

    pub fn update_screen_size(&self, queue: &Queue, width: f32, height: f32) {
        queue.write_buffer(
            &self.uniform_buffer,
            0,
            bytemuck::cast_slice(&[Uniforms {
                screen_size: [width, height],
                padding: [0.0, 0.0],
            }]),
        );
    }
}

/// Convert Color to [r, g, b, a] f32
pub fn color_to_rgba(color: Color, is_bg: bool) -> [f32; 4] {
    match color {
        Color::DefaultFg => [0.85, 0.85, 0.90, 1.0],
        Color::DefaultBg => [0.10, 0.10, 0.15, 1.0],
        Color::Rgb(r, g, b) => [
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            1.0,
        ],
        Color::Named(named) => named_color_to_rgba(named),
        Color::Indexed(idx) => indexed_color_to_rgba(idx, is_bg),
    }
}

fn named_color_to_rgba(named: NamedColor) -> [f32; 4] {
    match named {
        NamedColor::Black => [0.0, 0.0, 0.0, 1.0],
        NamedColor::Red => [0.80, 0.20, 0.20, 1.0],
        NamedColor::Green => [0.20, 0.80, 0.20, 1.0],
        NamedColor::Yellow => [0.80, 0.80, 0.20, 1.0],
        NamedColor::Blue => [0.20, 0.40, 0.80, 1.0],
        NamedColor::Magenta => [0.80, 0.20, 0.80, 1.0],
        NamedColor::Cyan => [0.20, 0.80, 0.80, 1.0],
        NamedColor::White => [0.80, 0.80, 0.80, 1.0],
        NamedColor::BrightBlack => [0.40, 0.40, 0.40, 1.0],
        NamedColor::BrightRed => [1.0, 0.35, 0.35, 1.0],
        NamedColor::BrightGreen => [0.35, 1.0, 0.35, 1.0],
        NamedColor::BrightYellow => [1.0, 1.0, 0.35, 1.0],
        NamedColor::BrightBlue => [0.35, 0.65, 1.0, 1.0],
        NamedColor::BrightMagenta => [1.0, 0.35, 1.0, 1.0],
        NamedColor::BrightCyan => [0.35, 1.0, 1.0, 1.0],
        NamedColor::BrightWhite => [1.0, 1.0, 1.0, 1.0],
    }
}

fn indexed_color_to_rgba(idx: u8, _is_bg: bool) -> [f32; 4] {
    if idx < 16 {
        let named = match idx {
            0 => NamedColor::Black,
            1 => NamedColor::Red,
            2 => NamedColor::Green,
            3 => NamedColor::Yellow,
            4 => NamedColor::Blue,
            5 => NamedColor::Magenta,
            6 => NamedColor::Cyan,
            7 => NamedColor::White,
            8 => NamedColor::BrightBlack,
            9 => NamedColor::BrightRed,
            10 => NamedColor::BrightGreen,
            11 => NamedColor::BrightYellow,
            12 => NamedColor::BrightBlue,
            13 => NamedColor::BrightMagenta,
            14 => NamedColor::BrightCyan,
            _ => NamedColor::BrightWhite,
        };
        return named_color_to_rgba(named);
    }

    if idx < 232 {
        let mut n = idx - 16;
        let b = (n % 6) as f32 / 5.0;
        n /= 6;
        let g = (n % 6) as f32 / 5.0;
        n /= 6;
        let r = (n % 6) as f32 / 5.0;
        return [r, g, b, 1.0];
    }

    let gray = (idx - 232) as f32 / 23.0;
    [gray, gray, gray, 1.0]
}
