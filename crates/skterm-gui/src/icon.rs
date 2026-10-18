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
            #[cfg(target_os = "windows")]
            {
                use winit::platform::windows::IconExtWindows;
                // Attempt to load the native multi-resolution icon (ID 1) embedded into the PE executable by winres.
                if let Ok(icon) = Icon::from_resource(1, None) {
                    return Some(icon);
                }
            }

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
        attrs = attrs.with_window_icon(Some(icon.clone()));

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs = attrs.with_taskbar_icon(Some(icon));
        }
    }

    #[cfg(all(
        unix,
        not(target_os = "macos"),
        not(target_os = "android"),
        not(target_os = "ios")
    ))]
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
        window.set_window_icon(Some(icon.clone()));

        #[cfg(target_os = "windows")]
        {
            use winit::platform::windows::WindowExtWindows;
            window.set_taskbar_icon(Some(icon));

            use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
            if let Ok(handle) = window.window_handle() {
                if let RawWindowHandle::Win32(win32_handle) = handle.as_raw() {
                    let hwnd = win32_handle.hwnd.get() as isize;
                    unsafe {
                        #[cfg(target_pointer_width = "64")]
                        #[link(name = "user32")]
                        extern "system" {
                            fn SetClassLongPtrW(
                                hWnd: isize,
                                nIndex: i32,
                                dwNewLong: usize,
                            ) -> usize;
                        }
                        #[cfg(target_pointer_width = "32")]
                        #[link(name = "user32")]
                        extern "system" {
                            #[link_name = "SetClassLongW"]
                            fn SetClassLongPtrW(
                                hWnd: isize,
                                nIndex: i32,
                                dwNewLong: usize,
                            ) -> usize;
                        }

                        #[link(name = "user32")]
                        extern "system" {
                            fn SendMessageW(
                                hWnd: isize,
                                Msg: u32,
                                wParam: usize,
                                lParam: isize,
                            ) -> isize;
                            fn LoadImageW(
                                hInst: isize,
                                name: *const u16,
                                type_: u32,
                                cx: i32,
                                cy: i32,
                                fuLoad: u32,
                            ) -> isize;
                        }
                        #[link(name = "kernel32")]
                        extern "system" {
                            fn GetModuleHandleW(lpModuleName: *const u16) -> isize;
                        }

                        const GCLP_HICON: i32 = -14;
                        const GCLP_HICONSM: i32 = -34;
                        const WM_SETICON: u32 = 0x0080;
                        const ICON_SMALL: usize = 0;
                        const ICON_BIG: usize = 1;
                        const IMAGE_ICON: u32 = 1;
                        const LR_DEFAULTSIZE: u32 = 0x0040;
                        const LR_SHARED: u32 = 0x8000;

                        let hinstance = GetModuleHandleW(std::ptr::null());
                        let hicon_big = LoadImageW(
                            hinstance,
                            1 as *const u16,
                            IMAGE_ICON,
                            0,
                            0,
                            LR_DEFAULTSIZE | LR_SHARED,
                        );
                        let hicon_small =
                            LoadImageW(hinstance, 1 as *const u16, IMAGE_ICON, 16, 16, LR_SHARED);

                        if hicon_big != 0 {
                            SendMessageW(hwnd, WM_SETICON, ICON_BIG, hicon_big);
                            SetClassLongPtrW(hwnd, GCLP_HICON, hicon_big as usize);
                        }
                        if hicon_small != 0 {
                            SendMessageW(hwnd, WM_SETICON, ICON_SMALL, hicon_small);
                            SetClassLongPtrW(hwnd, GCLP_HICONSM, hicon_small as usize);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_app_icon() {
        let icon = get_app_icon();
        assert!(
            icon.is_some(),
            "Expected embedded app icon to load successfully"
        );
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
