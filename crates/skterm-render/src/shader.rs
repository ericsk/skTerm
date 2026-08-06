//! WGSL Shaders for background cell quads, glyph text, and cursor rendering.

/// Combined WGSL shader source for instanced terminal cell backgrounds and glyph quads.
pub const TERMINAL_SHADER: &str = r#"
struct Uniforms {
    screen_size: vec2<f32>,
    padding: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(0) @binding(1)
var glyph_texture: texture_2d<f32>;

@group(0) @binding(2)
var glyph_sampler: sampler;

// --- Background Instanced Quad ---
struct BgInstance {
    @location(0) rect: vec4<f32>,   // [x, y, w, h] in screen pixels
    @location(1) color: vec4<f32>,  // [r, g, b, a]
};

struct BgVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vs_bg(
    @builtin(vertex_index) vertex_index: u32,
    instance: BgInstance,
) -> BgVertexOutput {
    // 0: top-left, 1: bottom-left, 2: top-right, 3: bottom-right
    var pos = vec2<f32>(0.0, 0.0);
    switch (vertex_index) {
        case 0u: { pos = vec2<f32>(instance.rect.x, instance.rect.y); }
        case 1u: { pos = vec2<f32>(instance.rect.x, instance.rect.y + instance.rect.w); }
        case 2u: { pos = vec2<f32>(instance.rect.x + instance.rect.z, instance.rect.y); }
        case 3u: { pos = vec2<f32>(instance.rect.x + instance.rect.z, instance.rect.y + instance.rect.w); }
        case 4u: { pos = vec2<f32>(instance.rect.x, instance.rect.y + instance.rect.w); }
        case 5u: { pos = vec2<f32>(instance.rect.x + instance.rect.z, instance.rect.y); }
        default: {}
    }

    // Convert pixel position to NDC [-1, 1]
    let ndc_x = (pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y / uniforms.screen_size.y) * 2.0;

    var out: BgVertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = instance.color;
    return out;
}

@fragment
fn fs_bg(in: BgVertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}

// --- Text Glyph Instanced Quad ---
struct GlyphInstance {
    @location(0) rect: vec4<f32>,   // [x, y, w, h] in screen pixels
    @location(1) uv: vec4<f32>,     // [u_min, v_min, u_max, v_max]
    @location(2) color: vec4<f32>,  // [r, g, b, a]
};

struct GlyphVertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_glyph(
    @builtin(vertex_index) vertex_index: u32,
    instance: GlyphInstance,
) -> GlyphVertexOutput {
    var pos = vec2<f32>(0.0, 0.0);
    var uv = vec2<f32>(0.0, 0.0);

    let x0 = instance.rect.x;
    let y0 = instance.rect.y;
    let x1 = instance.rect.x + instance.rect.z;
    let y1 = instance.rect.y + instance.rect.w;

    let u0 = instance.uv.x;
    let v0 = instance.uv.y;
    let u1 = instance.uv.z;
    let v1 = instance.uv.w;

    switch (vertex_index) {
        case 0u: { pos = vec2<f32>(x0, y0); uv = vec2<f32>(u0, v0); }
        case 1u: { pos = vec2<f32>(x0, y1); uv = vec2<f32>(u0, v1); }
        case 2u: { pos = vec2<f32>(x1, y0); uv = vec2<f32>(u1, v0); }
        case 3u: { pos = vec2<f32>(x1, y1); uv = vec2<f32>(u1, v1); }
        case 4u: { pos = vec2<f32>(x0, y1); uv = vec2<f32>(u0, v1); }
        case 5u: { pos = vec2<f32>(x1, y0); uv = vec2<f32>(u1, v0); }
        default: {}
    }

    let ndc_x = (pos.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y / uniforms.screen_size.y) * 2.0;

    var out: GlyphVertexOutput;
    out.position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    out.color = instance.color;
    return out;
}

@fragment
fn fs_glyph(in: GlyphVertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(glyph_texture, glyph_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;
