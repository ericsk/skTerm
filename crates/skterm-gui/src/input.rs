//! Keyboard and mouse input event mapping and escape sequence encoding.

use winit::event::{ElementState, KeyEvent};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

/// Calculate xterm parameter code for modifier combinations.
/// Shift = 2, Alt = 3, Shift+Alt = 4, Ctrl = 5, Shift+Ctrl = 6, Alt+Ctrl = 7, Shift+Alt+Ctrl = 8.
fn modifier_param(modifiers: ModifiersState) -> Option<u8> {
    let shift = modifiers.shift_key();
    let alt = modifiers.alt_key();
    let ctrl = modifiers.control_key();

    match (shift, alt, ctrl) {
        (true, false, false) => Some(2),
        (false, true, false) => Some(3),
        (true, true, false) => Some(4),
        (false, false, true) => Some(5),
        (true, false, true) => Some(6),
        (false, true, true) => Some(7),
        (true, true, true) => Some(8),
        _ => None,
    }
}

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

    let mod_code = modifier_param(modifiers);

    // 2. Named special keys
    match logical_key {
        Key::Named(NamedKey::Enter) => Some(vec![b'\r']),
        Key::Named(NamedKey::Tab) => {
            if modifiers.shift_key() {
                // Standard DEC / xterm Cursor Backward Tabulation (Backtab / CBT)
                Some(b"\x1b[Z".to_vec())
            } else {
                Some(vec![b'\t'])
            }
        }
        Key::Named(NamedKey::Backspace) => Some(vec![0x7f]),
        Key::Named(NamedKey::Escape) => Some(vec![0x1b]),
        Key::Named(NamedKey::ArrowUp) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[1;{}A", m).into_bytes())
            } else if app_cursor_keys {
                Some(b"\x1bOA".to_vec())
            } else {
                Some(b"\x1b[A".to_vec())
            }
        }
        Key::Named(NamedKey::ArrowDown) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[1;{}B", m).into_bytes())
            } else if app_cursor_keys {
                Some(b"\x1bOB".to_vec())
            } else {
                Some(b"\x1b[B".to_vec())
            }
        }
        Key::Named(NamedKey::ArrowRight) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[1;{}C", m).into_bytes())
            } else if app_cursor_keys {
                Some(b"\x1bOC".to_vec())
            } else {
                Some(b"\x1b[C".to_vec())
            }
        }
        Key::Named(NamedKey::ArrowLeft) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[1;{}D", m).into_bytes())
            } else if app_cursor_keys {
                Some(b"\x1bOD".to_vec())
            } else {
                Some(b"\x1b[D".to_vec())
            }
        }
        Key::Named(NamedKey::Home) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[1;{}H", m).into_bytes())
            } else {
                Some(b"\x1b[H".to_vec())
            }
        }
        Key::Named(NamedKey::End) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[1;{}F", m).into_bytes())
            } else {
                Some(b"\x1b[F".to_vec())
            }
        }
        Key::Named(NamedKey::PageUp) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[5;{}~", m).into_bytes())
            } else {
                Some(b"\x1b[5~".to_vec())
            }
        }
        Key::Named(NamedKey::PageDown) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[6;{}~", m).into_bytes())
            } else {
                Some(b"\x1b[6~".to_vec())
            }
        }
        Key::Named(NamedKey::Delete) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[3;{}~", m).into_bytes())
            } else {
                Some(b"\x1b[3~".to_vec())
            }
        }
        Key::Named(NamedKey::Insert) => {
            if let Some(m) = mod_code {
                Some(format!("\x1b[2;{}~", m).into_bytes())
            } else {
                Some(b"\x1b[2~".to_vec())
            }
        }
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

/// Parse a tab switch digit (1..=9) from logical and physical key inputs.
/// Checks both the logical key and physical key so it works reliably across various keyboard layouts
/// (including US, AZERTY, QWERTZ, and numpads).
pub fn parse_tab_switch_digit(logical_key: &Key, physical_key: PhysicalKey) -> Option<usize> {
    // 1. Check logical key as character
    if let Key::Character(ref s) = logical_key {
        if let Ok(digit) = s.parse::<usize>() {
            if (1..=9).contains(&digit) {
                return Some(digit);
            }
        }
    }

    // 2. Fall back to physical key (e.g. AZERTY top row or numpad)
    match physical_key {
        PhysicalKey::Code(KeyCode::Digit1 | KeyCode::Numpad1) => Some(1),
        PhysicalKey::Code(KeyCode::Digit2 | KeyCode::Numpad2) => Some(2),
        PhysicalKey::Code(KeyCode::Digit3 | KeyCode::Numpad3) => Some(3),
        PhysicalKey::Code(KeyCode::Digit4 | KeyCode::Numpad4) => Some(4),
        PhysicalKey::Code(KeyCode::Digit5 | KeyCode::Numpad5) => Some(5),
        PhysicalKey::Code(KeyCode::Digit6 | KeyCode::Numpad6) => Some(6),
        PhysicalKey::Code(KeyCode::Digit7 | KeyCode::Numpad7) => Some(7),
        PhysicalKey::Code(KeyCode::Digit8 | KeyCode::Numpad8) => Some(8),
        PhysicalKey::Code(KeyCode::Digit9 | KeyCode::Numpad9) => Some(9),
        _ => None,
    }
}

/// Determine if the given modifier state matches the tab switching shortcut for the specified OS:
/// - Windows & Linux: CTRL (without Alt, Super)
/// - macOS: CMD (Super) (without Ctrl, Alt)
pub fn is_tab_switch_modifier_for_os(modifiers: ModifiersState, is_macos: bool) -> bool {
    if is_macos {
        modifiers.super_key() && !modifiers.control_key() && !modifiers.alt_key()
    } else {
        modifiers.control_key() && !modifiers.super_key() && !modifiers.alt_key()
    }
}

/// Determine if the current modifiers match the tab switching shortcut for the current target OS:
/// - Windows & Linux: CTRL (without Alt, Super)
/// - macOS: CMD (Super) (without Ctrl, Alt)
pub fn is_tab_switch_modifier(modifiers: ModifiersState) -> bool {
    #[cfg(target_os = "macos")]
    let is_macos = true;
    #[cfg(not(target_os = "macos"))]
    let is_macos = false;

    is_tab_switch_modifier_for_os(modifiers, is_macos)
}

/// Check if a keyboard event is a tab switch shortcut for the specified OS.
/// Returns `Some(tab_number)` (1..=9) if matching:
/// - Windows & Linux: CTRL + 1..=9
/// - macOS: CMD + 1..=9
pub fn check_tab_switch_for_os(
    logical_key: &Key,
    physical_key: PhysicalKey,
    state: ElementState,
    modifiers: ModifiersState,
    is_macos: bool,
) -> Option<usize> {
    if state != ElementState::Pressed {
        return None;
    }
    if is_tab_switch_modifier_for_os(modifiers, is_macos) {
        parse_tab_switch_digit(logical_key, physical_key)
    } else {
        None
    }
}

/// Check if a keyboard event is a tab switch shortcut for the current target OS.
/// Returns `Some(tab_number)` (1..=9) if matching:
/// - Windows & Linux: CTRL + 1..=9
/// - macOS: CMD + 1..=9
pub fn check_tab_switch(event: &KeyEvent, modifiers: ModifiersState) -> Option<usize> {
    #[cfg(target_os = "macos")]
    let is_macos = true;
    #[cfg(not(target_os = "macos"))]
    let is_macos = false;

    check_tab_switch_for_os(
        &event.logical_key,
        event.physical_key,
        event.state,
        modifiers,
        is_macos,
    )
}
