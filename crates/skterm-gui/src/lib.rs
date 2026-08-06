//! `skterm-gui`: Application window loop, input handling, config, tabs, and split panes multiplexer.

pub mod app_window;
pub mod config;
pub mod input;
pub mod layout;

pub use app_window::App;
pub use config::{Config, ConfigManager, Theme};
pub use input::{encode_key, encode_key_event};
pub use layout::{PaneId, PaneNode, Rect, SplitDirection, Tab, TabId, Workspace};

/// skterm-gui version and basic info
pub fn gui_info() -> &'static str {
    "skterm-gui v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::ElementState;
    use winit::keyboard::{Key, ModifiersState, NamedKey};

    #[test]
    fn test_gui_info() {
        assert_eq!(gui_info(), "skterm-gui v0.1.0");
    }

    #[test]
    fn test_key_encoding_enter_and_backspace() {
        let enter = encode_key(&Key::Named(NamedKey::Enter), None, ElementState::Pressed, ModifiersState::empty(), false);
        assert_eq!(enter, Some(vec![b'\r']));

        let backspace = encode_key(&Key::Named(NamedKey::Backspace), None, ElementState::Pressed, ModifiersState::empty(), false);
        assert_eq!(backspace, Some(vec![0x7f]));
    }

    #[test]
    fn test_key_encoding_ctrl_c() {
        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        let ctrl_c = encode_key(&Key::Character("c".into()), None, ElementState::Pressed, ctrl, false);
        assert_eq!(ctrl_c, Some(vec![0x03]));
    }

    #[test]
    fn test_config_parsing() {
        let toml_str = r#"
            [font]
            family = "Menlo"
            size = 16.0
            line_height = 1.2

            [window]
            opacity = 0.95
            padding_x = 10.0
            padding_y = 10.0

            [colors]
            theme = "catppuccin-mocha"

            [cursor]
            shape = "beam"
            blink = false
        "#;

        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.font.family, "Menlo");
        assert_eq!(cfg.font.size, 16.0);
        assert_eq!(cfg.window.opacity, 0.95);
        assert_eq!(cfg.colors.theme, "catppuccin-mocha");
        assert_eq!(cfg.cursor.shape, "beam");
        assert!(!cfg.cursor.blink);
    }

    #[test]
    fn test_theme_presets() {
        let dracula = Theme::get("dracula");
        assert_eq!(dracula.name, "dracula");

        let tokyo = Theme::get("tokyo-night");
        assert_eq!(tokyo.name, "tokyo-night");

        let default_dark = Theme::get("unknown");
        assert_eq!(default_dark.name, "default-dark");
    }

    #[test]
    fn test_bsp_workspace_and_tabs() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");
        assert_eq!(ws.tabs.len(), 1);
        assert_eq!(ws.active_tab_idx, 0);

        // Test New Tab
        ws.new_tab(24, 80).expect("Failed to create new tab");
        assert_eq!(ws.tabs.len(), 2);
        assert_eq!(ws.active_tab_idx, 1);

        // Test Tab Switching
        ws.prev_tab();
        assert_eq!(ws.active_tab_idx, 0);
        ws.next_tab();
        assert_eq!(ws.active_tab_idx, 1);

        // Test Pane Splitting in Active Tab
        let split_res = ws.split_active_pane(SplitDirection::Horizontal, 24, 80);
        assert!(split_res.is_ok());

        let rects = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert_eq!(rects.len(), 2);
        assert!(rects[0].1.width > 0.0);
        assert!(rects[1].1.width > 0.0);
        assert_eq!(rects[0].1.y, 32.0);

        // Test Pane Closing
        let closed = ws.close_active_pane_or_tab();
        assert!(closed);
        assert_eq!(ws.tabs.len(), 2);

        // Close the whole second tab
        let closed_tab = ws.close_active_pane_or_tab();
        assert!(closed_tab);
        assert_eq!(ws.tabs.len(), 1);
    }
}
