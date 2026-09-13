//! Application window icon and platform-specific window identification.

use std::sync::OnceLock;
use tracing::warn;
use winit::window::{Icon, Window, WindowAttributes};

static APP_ICON: OnceLock<Option<Icon>> = OnceLock::new();

/// Embedded icon PNG bytes (256x256 PNG for optimal performance and quality).
const ICON_PNG_BYTES: &[u8] = include_bytes!("../../../assets/icon_256.png");

/// Returns the cached application icon, or `None` if loading fails.
pub fn get_app_icon() -> Option<Icon> {
    APP_ICON
        .get_or_init(|| {
            match image::load_from_memory_with_format(ICON_PNG_BYTES, image::ImageFormat::Png) {
                Ok(img) => {
                    let rgba = img.to_rgba8();
                    let (width, height) = rgba.dimensions();
                    match Icon::from_rgba(rgba.into_raw(), width, height) {
                        Ok(icon) => Some(icon),
                        Err(err) => {
                            warn!("Failed to create window icon from RGBA: {err}");
                            None
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to decode embedded application icon: {err}");
                    None
                }
            }
        })
        .clone()
}

/// Configures platform-specific window attributes (app_id / WM_CLASS on Linux, window icon).
pub fn configure_window_attributes(mut attrs: WindowAttributes) -> WindowAttributes {
    if let Some(icon) = get_app_icon() {
        attrs = attrs.with_window_icon(Some(icon));
    }

    #[cfg(all(unix, not(target_os = "macos"), not(target_os = "android"), not(target_os = "ios")))]
    {
        use winit::platform::wayland::WindowAttributesExtWayland;
        use winit::platform::x11::WindowAttributesExtX11;
        attrs = WindowAttributesExtWayland::with_name(attrs, "skterm", "skterm");
        attrs = WindowAttributesExtX11::with_name(attrs, "skterm", "skterm");
    }

    attrs
}

/// Applies the application icon to an existing window.
pub fn apply_window_icon(window: &Window) {
    if let Some(icon) = get_app_icon() {
        window.set_window_icon(Some(icon));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_app_icon() {
        let icon = get_app_icon();
        assert!(icon.is_some(), "Expected embedded app icon to load successfully");
    }

    #[test]
    fn test_configure_window_attributes() {
        let attrs = WindowAttributes::default().with_title("Test Window");
        let configured = configure_window_attributes(attrs);
        assert!(
            configured.window_icon.is_some(),
            "Expected window icon to be set in WindowAttributes"
        );
    }
}
