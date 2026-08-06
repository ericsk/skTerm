//! Keyboard and mouse input event mapping and escape sequence encoding.

use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, ModifiersState, NamedKey};

/// Encode key parameters into raw bytes to send to the child PTY.
pub fn encode_key(
    logical_key: &Key,
    text: Option<&str>,
    state: ElementState,
    modifiers: ModifiersState,
    app_cursor_keys: bool,
) -> Option<Vec<u8>> {
    if state != ElementState::Pressed {
        return None;
    }

    // 1. Check Ctrl + Letter combinations (ASCII 0x01 to 0x1A)
    if modifiers.control_key() && !modifiers.alt_key() {
        if let Key::Character(ref s) = logical_key {
            if let Some(first_char) = s.chars().next() {
                let lower = first_char.to_ascii_lowercase();
                if lower.is_ascii_lowercase() {
                    let code = (lower as u8) - b'a' + 1;
                    return Some(vec![code]);
                }
            }
        }
    }

    // 2. Named special keys
    match logical_key {
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Tab) => Some(vec![b'\t']),
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        Key::Named(NamedKey::ArrowUp) => {
            if app_cursor_keys {
                Some(b"\x1bOA".to_vec())
            } else {
                Some(b"\x1b[A".to_vec())
            }
        }
        Key::Named(NamedKey::ArrowDown) => {
            if app_cursor_keys {
                Some(b"\x1bOB".to_vec())
            } else {
                Some(b"\x1b[B".to_vec())
            }
        }
        Key::Named(NamedKey::ArrowRight) => {
            if app_cursor_keys {
                Some(b"\x1bOC".to_vec())
            } else {
                Some(b"\x1b[C".to_vec())
            }
        }
        Key::Named(NamedKey::ArrowLeft) => {
            if app_cursor_keys {
                Some(b"\x1bOD".to_vec())
            } else {
                Some(b"\x1b[D".to_vec())
            }
        }
        Key::Named(NamedKey::Home) => Some(b"\x1b[H".to_vec()),
        Key::Named(NamedKey::End) => Some(b"\x1b[F".to_vec()),
        Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
        Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
        Key::Named(NamedKey::Insert) => Some(b"\x1b[2~".to_vec()),
        Key::Named(NamedKey::F1) => Some(b"\x1bOP".to_vec()),
        Key::Named(NamedKey::F2) => Some(b"\x1bOQ".to_vec()),
        Key::Named(NamedKey::F3) => Some(b"\x1bOR".to_vec()),
        Key::Named(NamedKey::F4) => Some(b"\x1bOS".to_vec()),
        Key::Named(NamedKey::F5) => Some(b"\x1b[15~".to_vec()),
        Key::Named(NamedKey::F6) => Some(b"\x1b[17~".to_vec()),
        Key::Named(NamedKey::F7) => Some(b"\x1b[18~".to_vec()),
        Key::Named(NamedKey::F8) => Some(b"\x1b[19~".to_vec()),
        Key::Named(NamedKey::F9) => Some(b"\x1b[20~".to_vec()),
        Key::Named(NamedKey::F10) => Some(b"\x1b[21~".to_vec()),
        Key::Named(NamedKey::F11) => Some(b"\x1b[23~".to_vec()),
        Key::Named(NamedKey::F12) => Some(b"\x1b[24~".to_vec()),
        _ => {
            // 3. Printable characters / IME output text
            if let Some(text_str) = text {
                Some(text_str.as_bytes().to_vec())
            } else if let Key::Character(ref s) = logical_key {
                Some(s.as_bytes().to_vec())
            } else {
                None
            }
        }
    }
}

/// Encode a winit KeyEvent into raw bytes to send to the child PTY.
pub fn encode_key_event(
    event: &KeyEvent,
    modifiers: ModifiersState,
    app_cursor_keys: bool,
) -> Option<Vec<u8>> {
    encode_key(
        &event.logical_key,
        event.text.as_deref(),
        event.state,
        modifiers,
        app_cursor_keys,
    )
}
