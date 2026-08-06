//! `skterm-gui`: Windowing, tabs, split panes (multiplexing), input handling, clipboard, and config.

pub mod app_window;
pub mod config;
pub mod input;
pub mod layout;

pub use app_window::App;
pub use config::{Config, ConfigManager, Theme};
pub use input::{encode_key, encode_key_event};

/// skterm-gui version and basic info
pub fn gui_info() -> &'static str {
    "skterm-gui v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::event::ElementState;
    use winit::keyboard::{Key, ModifiersState, NamedKey, SmolStr};

    #[test]
    fn test_gui_info() {
        assert_eq!(gui_info(), "skterm-gui v0.1.0");
    }

    #[test]
    fn test_key_encoding_enter_and_backspace() {
        let enter = Key::Named(NamedKey::Enter);
        let encoded = encode_key(&enter, None, ElementState::Pressed, ModifiersState::empty(), false);
        assert_eq!(encoded, Some(vec![b'\r']));

        let bs = Key::Named(NamedKey::Backspace);
        let encoded = encode_key(&bs, None, ElementState::Pressed, ModifiersState::empty(), false);
        assert_eq!(encoded, Some(vec![0x7f]));
    }

    #[test]
    fn test_key_encoding_ctrl_c() {
        let key_c = Key::Character(SmolStr::new("c"));
        let mut mods = ModifiersState::empty();
        mods.insert(ModifiersState::CONTROL);
        let encoded = encode_key(&key_c, None, ElementState::Pressed, mods, false);
        assert_eq!(encoded, Some(vec![0x03]));
    }

    #[test]
    fn test_theme_presets() {
        let mocha = Theme::get("catppuccin-mocha");
        assert_eq!(mocha.name, "catppuccin-mocha");

        let dracula = Theme::get("dracula");
        assert_eq!(dracula.name, "dracula");

        let solarized = Theme::get("solarized-dark");
        assert_eq!(solarized.name, "solarized-dark");
    }

    #[test]
    fn test_config_parsing() {
        let toml_str = r#"
        [font]
        family = "JetBrains Mono"
        size = 16.0
        line_height = 1.3

        [colors]
        theme = "tokyo-night"

        [cursor]
        shape = "beam"
        blink = false
        "#;

        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.font.family, "JetBrains Mono");
        assert_eq!(cfg.font.size, 16.0);
        assert_eq!(cfg.colors.theme, "tokyo-night");
        assert_eq!(cfg.cursor.shape, "beam");
        assert!(!cfg.cursor.blink);
    }
}
