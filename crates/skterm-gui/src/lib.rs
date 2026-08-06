//! `skterm-gui`: Windowing, tabs, split panes (multiplexing), input handling, and clipboard.

pub mod app_window;
pub mod config;
pub mod input;
pub mod layout;

pub use app_window::App;
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
        assert_eq!(encoded, Some(vec![0x03])); // ASCII ETX (Ctrl+C)
    }
}
