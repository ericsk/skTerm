//! `skterm-render`: wgpu-based GPU text & quad rendering pipeline, glyph atlas, and shaders.

pub mod atlas;
pub mod pipeline;
pub mod shader;

/// skterm-render version and basic info
pub fn render_info() -> &'static str {
    "skterm-render v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_info() {
        assert_eq!(render_info(), "skterm-render v0.1.0");
    }
}
