//! Keyboard and mouse input event mapping and escape sequence encoding.

use skterm_core::vt::{MouseFormat, MouseProtocol};
use winit::event::{ElementState, KeyEvent, MouseButton};
use winit::keyboard::{
    Key, KeyCode, KeyLocation, ModifiersState, NamedKey, NativeKeyCode, PhysicalKey,
};

#[cfg(any(
    target_os = "macos",
    target_os = "windows",
    target_os = "linux",
    target_os = "freebsd",
    target_os = "dragonfly",
    target_os = "netbsd",
    target_os = "openbsd"
))]
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;

/// Kitty Keyboard Protocol enhancement flags (`CSI > flags u`).
pub const KITTY_FLAG_DISAMBIGUATE: u16 = 1;
pub const KITTY_FLAG_REPORT_EVENTS: u16 = 2;
pub const KITTY_FLAG_REPORT_ALTERNATE: u16 = 4;
pub const KITTY_FLAG_REPORT_ALL_KEYS: u16 = 8;
pub const KITTY_FLAG_REPORT_TEXT: u16 = 16;

/// Full keyboard modifier state including KKP lock keys (CapsLock / NumLock).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyModifierFlags {
    pub shift: bool,
    pub alt: bool,
    pub ctrl: bool,
    pub super_key: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
}

impl KeyModifierFlags {
    pub fn from_modifiers_state(
        modifiers: ModifiersState,
        caps_lock: bool,
        num_lock: bool,
    ) -> Self {
        Self {
            shift: modifiers.shift_key(),
            alt: modifiers.alt_key(),
            ctrl: modifiers.control_key(),
            super_key: modifiers.super_key(),
            caps_lock,
            num_lock,
        }
    }

    /// Legacy xterm modifier bits (Shift=1, Alt=2, Ctrl=4).
    pub fn xterm_bits(self) -> u8 {
        let mut bits = 0u8;
        if self.shift {
            bits |= 1;
        }
        if self.alt {
            bits |= 2;
        }
        if self.ctrl {
            bits |= 4;
        }
        bits
    }

    /// Legacy xterm modifier parameter (`1 + bits`), or `None` when no xterm modifiers are active.
    pub fn xterm_param(self) -> Option<u8> {
        let bits = self.xterm_bits();
        if bits == 0 {
            None
        } else {
            Some(1 + bits)
        }
    }

    /// Kitty Keyboard Protocol modifier bitmask:
    /// Shift=1, Alt=2, Ctrl=4, Super=8, CapsLock=64, NumLock=128.
    pub fn kitty_bits(self) -> u8 {
        let mut bits = self.xterm_bits();
        if self.super_key {
            bits |= 8;
        }
        if self.caps_lock {
            bits |= 64;
        }
        if self.num_lock {
            bits |= 128;
        }
        bits
    }

    /// True if any non-lock modifier (`Shift`, `Alt`, `Ctrl`, or `Super`) is active.
    pub fn has_non_lock_modifiers(self) -> bool {
        self.shift || self.alt || self.ctrl || self.super_key
    }

    /// True if any non-Shift, non-lock modifier (`Alt`, `Ctrl`, or `Super`) is active.
    pub fn has_non_shift_modifiers(self) -> bool {
        self.alt || self.ctrl || self.super_key
    }
}

impl From<ModifiersState> for KeyModifierFlags {
    fn from(modifiers: ModifiersState) -> Self {
        Self::from_modifiers_state(modifiers, false, false)
    }
}

/// Platform-independent key input descriptor for encoding into PTY bytes.
#[derive(Debug, Clone)]
pub struct KeyInput<'a> {
    pub logical_key: &'a Key,
    pub key_without_modifiers: Option<&'a Key>,
    pub physical_key: PhysicalKey,
    pub location: KeyLocation,
    pub text: Option<&'a str>,
    pub state: ElementState,
    pub repeat: bool,
    pub modifiers: KeyModifierFlags,
}

/// Map a hardware `PhysicalKey` to its standard PC-101 / US-QWERTY unshifted codepoint
/// (used as the KKP `base-layout-key` under Flag 4 and for unshifted punctuation recovery).
pub fn physical_key_to_us_codepoint(physical_key: PhysicalKey) -> Option<u32> {
    let ch = match physical_key {
        PhysicalKey::Code(KeyCode::KeyA) => 'a',
        PhysicalKey::Code(KeyCode::KeyB) => 'b',
        PhysicalKey::Code(KeyCode::KeyC) => 'c',
        PhysicalKey::Code(KeyCode::KeyD) => 'd',
        PhysicalKey::Code(KeyCode::KeyE) => 'e',
        PhysicalKey::Code(KeyCode::KeyF) => 'f',
        PhysicalKey::Code(KeyCode::KeyG) => 'g',
        PhysicalKey::Code(KeyCode::KeyH) => 'h',
        PhysicalKey::Code(KeyCode::KeyI) => 'i',
        PhysicalKey::Code(KeyCode::KeyJ) => 'j',
        PhysicalKey::Code(KeyCode::KeyK) => 'k',
        PhysicalKey::Code(KeyCode::KeyL) => 'l',
        PhysicalKey::Code(KeyCode::KeyM) => 'm',
        PhysicalKey::Code(KeyCode::KeyN) => 'n',
        PhysicalKey::Code(KeyCode::KeyO) => 'o',
        PhysicalKey::Code(KeyCode::KeyP) => 'p',
        PhysicalKey::Code(KeyCode::KeyQ) => 'q',
        PhysicalKey::Code(KeyCode::KeyR) => 'r',
        PhysicalKey::Code(KeyCode::KeyS) => 's',
        PhysicalKey::Code(KeyCode::KeyT) => 't',
        PhysicalKey::Code(KeyCode::KeyU) => 'u',
        PhysicalKey::Code(KeyCode::KeyV) => 'v',
        PhysicalKey::Code(KeyCode::KeyW) => 'w',
        PhysicalKey::Code(KeyCode::KeyX) => 'x',
        PhysicalKey::Code(KeyCode::KeyY) => 'y',
        PhysicalKey::Code(KeyCode::KeyZ) => 'z',
        PhysicalKey::Code(KeyCode::Digit0) => '0',
        PhysicalKey::Code(KeyCode::Digit1) => '1',
        PhysicalKey::Code(KeyCode::Digit2) => '2',
        PhysicalKey::Code(KeyCode::Digit3) => '3',
        PhysicalKey::Code(KeyCode::Digit4) => '4',
        PhysicalKey::Code(KeyCode::Digit5) => '5',
        PhysicalKey::Code(KeyCode::Digit6) => '6',
        PhysicalKey::Code(KeyCode::Digit7) => '7',
        PhysicalKey::Code(KeyCode::Digit8) => '8',
        PhysicalKey::Code(KeyCode::Digit9) => '9',
        PhysicalKey::Code(KeyCode::Minus) => '-',
        PhysicalKey::Code(KeyCode::Equal) => '=',
        PhysicalKey::Code(KeyCode::BracketLeft) => '[',
        PhysicalKey::Code(KeyCode::BracketRight) => ']',
        PhysicalKey::Code(KeyCode::Backslash) => '\\',
        PhysicalKey::Code(KeyCode::Semicolon) => ';',
        PhysicalKey::Code(KeyCode::Quote) => '\'',
        PhysicalKey::Code(KeyCode::Backquote) => '`',
        PhysicalKey::Code(KeyCode::Comma) => ',',
        PhysicalKey::Code(KeyCode::Period) => '.',
        PhysicalKey::Code(KeyCode::Slash) => '/',
        PhysicalKey::Code(KeyCode::Space) => ' ',
        _ => return None,
    };
    Some(ch as u32)
}

/// Fallback US-QWERTY unshifted base character for common shifted ASCII punctuation
/// when neither `key_without_modifiers` nor `physical_key` is available.
fn unshift_us_ascii_symbol(ch: char) -> Option<char> {
    match ch {
        '!' => Some('1'),
        '@' => Some('2'),
        '#' => Some('3'),
        '$' => Some('4'),
        '%' => Some('5'),
        '^' => Some('6'),
        '&' => Some('7'),
        '*' => Some('8'),
        '(' => Some('9'),
        ')' => Some('0'),
        '_' => Some('-'),
        '+' => Some('='),
        '{' => Some('['),
        '}' => Some(']'),
        '|' => Some('\\'),
        ':' => Some(';'),
        '"' => Some('\''),
        '~' => Some('`'),
        '<' => Some(','),
        '>' => Some('.'),
        '?' => Some('/'),
        _ => None,
    }
}

/// Map standalone modifier or lock keys to their Kitty PUA keycodes (`57358`–`57360`, `57441`–`57452`).
fn standalone_modifier_or_lock_pua(
    logical_key: &Key,
    physical_key: PhysicalKey,
    location: KeyLocation,
) -> Option<u32> {
    match physical_key {
        PhysicalKey::Code(KeyCode::CapsLock) => return Some(57358),
        PhysicalKey::Code(KeyCode::ScrollLock) => return Some(57359),
        PhysicalKey::Code(KeyCode::NumLock) => return Some(57360),
        PhysicalKey::Code(KeyCode::ShiftLeft) => return Some(57441),
        PhysicalKey::Code(KeyCode::ControlLeft) => return Some(57442),
        PhysicalKey::Code(KeyCode::AltLeft) => return Some(57443),
        PhysicalKey::Code(KeyCode::SuperLeft) => return Some(57444),
        PhysicalKey::Code(KeyCode::ShiftRight) => return Some(57447),
        PhysicalKey::Code(KeyCode::ControlRight) => return Some(57448),
        PhysicalKey::Code(KeyCode::AltRight) => return Some(57449),
        PhysicalKey::Code(KeyCode::SuperRight) => return Some(57450),
        PhysicalKey::Code(KeyCode::Hyper) => {
            return Some(if location == KeyLocation::Right {
                57451
            } else {
                57445
            });
        }
        PhysicalKey::Code(KeyCode::Meta) => {
            return Some(if location == KeyLocation::Right {
                57452
            } else {
                57446
            });
        }
        _ => {}
    }

    match logical_key {
        Key::Named(NamedKey::CapsLock) => Some(57358),
        Key::Named(NamedKey::ScrollLock) => Some(57359),
        Key::Named(NamedKey::NumLock) => Some(57360),
        Key::Named(NamedKey::Shift) => Some(if location == KeyLocation::Right {
            57447
        } else {
            57441
        }),
        Key::Named(NamedKey::Control) => Some(if location == KeyLocation::Right {
            57448
        } else {
            57442
        }),
        Key::Named(NamedKey::Alt | NamedKey::AltGraph) => Some(if location == KeyLocation::Right {
            57449
        } else {
            57443
        }),
        Key::Named(NamedKey::Super) => Some(if location == KeyLocation::Right {
            57450
        } else {
            57444
        }),
        Key::Named(NamedKey::Hyper) => Some(if location == KeyLocation::Right {
            57451
        } else {
            57445
        }),
        Key::Named(NamedKey::Meta) => Some(if location == KeyLocation::Right {
            57452
        } else {
            57446
        }),
        _ => None,
    }
}

/// Map numpad keys to their Kitty PUA keycode (`57399`–`57427`) and optional default text.
fn numpad_pua_and_text(
    logical_key: &Key,
    physical_key: PhysicalKey,
    location: KeyLocation,
) -> Option<(u32, Option<&'static str>)> {
    let is_numpad_physical = matches!(
        physical_key,
        PhysicalKey::Code(
            KeyCode::Numpad0
                | KeyCode::Numpad1
                | KeyCode::Numpad2
                | KeyCode::Numpad3
                | KeyCode::Numpad4
                | KeyCode::Numpad5
                | KeyCode::Numpad6
                | KeyCode::Numpad7
                | KeyCode::Numpad8
                | KeyCode::Numpad9
                | KeyCode::NumpadDecimal
                | KeyCode::NumpadDivide
                | KeyCode::NumpadMultiply
                | KeyCode::NumpadStar
                | KeyCode::NumpadSubtract
                | KeyCode::NumpadAdd
                | KeyCode::NumpadEnter
                | KeyCode::NumpadEqual
                | KeyCode::NumpadComma
        )
    );
    if !is_numpad_physical && location != KeyLocation::Numpad {
        return None;
    }

    // When NumLock is off, the logical key is a navigation key on the keypad.
    match logical_key {
        Key::Named(NamedKey::ArrowLeft) => return Some((57417, None)),
        Key::Named(NamedKey::ArrowRight) => return Some((57418, None)),
        Key::Named(NamedKey::ArrowUp) => return Some((57419, None)),
        Key::Named(NamedKey::ArrowDown) => return Some((57420, None)),
        Key::Named(NamedKey::PageUp) => return Some((57421, None)),
        Key::Named(NamedKey::PageDown) => return Some((57422, None)),
        Key::Named(NamedKey::Home) => return Some((57423, None)),
        Key::Named(NamedKey::End) => return Some((57424, None)),
        Key::Named(NamedKey::Insert) => return Some((57425, None)),
        Key::Named(NamedKey::Delete) => return Some((57426, None)),
        Key::Named(NamedKey::Clear) => return Some((57427, None)),
        _ => {}
    }

    match physical_key {
        PhysicalKey::Code(KeyCode::Numpad0) => return Some((57399, Some("0"))),
        PhysicalKey::Code(KeyCode::Numpad1) => return Some((57400, Some("1"))),
        PhysicalKey::Code(KeyCode::Numpad2) => return Some((57401, Some("2"))),
        PhysicalKey::Code(KeyCode::Numpad3) => return Some((57402, Some("3"))),
        PhysicalKey::Code(KeyCode::Numpad4) => return Some((57403, Some("4"))),
        PhysicalKey::Code(KeyCode::Numpad5) => return Some((57404, Some("5"))),
        PhysicalKey::Code(KeyCode::Numpad6) => return Some((57405, Some("6"))),
        PhysicalKey::Code(KeyCode::Numpad7) => return Some((57406, Some("7"))),
        PhysicalKey::Code(KeyCode::Numpad8) => return Some((57407, Some("8"))),
        PhysicalKey::Code(KeyCode::Numpad9) => return Some((57408, Some("9"))),
        PhysicalKey::Code(KeyCode::NumpadDecimal) => return Some((57409, Some("."))),
        PhysicalKey::Code(KeyCode::NumpadDivide) => return Some((57410, Some("/"))),
        PhysicalKey::Code(KeyCode::NumpadMultiply | KeyCode::NumpadStar) => {
            return Some((57411, Some("*")));
        }
        PhysicalKey::Code(KeyCode::NumpadSubtract) => return Some((57412, Some("-"))),
        PhysicalKey::Code(KeyCode::NumpadAdd) => return Some((57413, Some("+"))),
        PhysicalKey::Code(KeyCode::NumpadEnter) => return Some((57414, None)),
        PhysicalKey::Code(KeyCode::NumpadEqual) => return Some((57415, Some("="))),
        PhysicalKey::Code(KeyCode::NumpadComma) => return Some((57416, Some(","))),
        _ => {}
    }

    // Fallback when `location == KeyLocation::Numpad` without a `KeyCode::Numpad*`.
    match logical_key {
        Key::Named(NamedKey::Enter) => Some((57414, None)),
        Key::Character(ref s) => match s.as_str() {
            "0" => Some((57399, Some("0"))),
            "1" => Some((57400, Some("1"))),
            "2" => Some((57401, Some("2"))),
            "3" => Some((57402, Some("3"))),
            "4" => Some((57403, Some("4"))),
            "5" => Some((57404, Some("5"))),
            "6" => Some((57405, Some("6"))),
            "7" => Some((57406, Some("7"))),
            "8" => Some((57407, Some("8"))),
            "9" => Some((57408, Some("9"))),
            "." => Some((57409, Some("."))),
            "/" => Some((57410, Some("/"))),
            "*" => Some((57411, Some("*"))),
            "-" => Some((57412, Some("-"))),
            "+" => Some((57413, Some("+"))),
            "=" => Some((57415, Some("="))),
            "," => Some((57416, Some(","))),
            _ => None,
        },
        _ => None,
    }
}

/// Map extended functional keys (`PrintScreen`, `Pause`, `ContextMenu`, `F13`–`F35`)
/// to their Kitty PUA keycodes.
fn extended_functional_pua(logical_key: &Key, physical_key: PhysicalKey) -> Option<u32> {
    match logical_key {
        Key::Named(NamedKey::PrintScreen) => return Some(57361),
        Key::Named(NamedKey::Pause) => return Some(57362),
        Key::Named(NamedKey::ContextMenu) => return Some(57363),
        Key::Named(NamedKey::F13) => return Some(57376),
        Key::Named(NamedKey::F14) => return Some(57377),
        Key::Named(NamedKey::F15) => return Some(57378),
        Key::Named(NamedKey::F16) => return Some(57379),
        Key::Named(NamedKey::F17) => return Some(57380),
        Key::Named(NamedKey::F18) => return Some(57381),
        Key::Named(NamedKey::F19) => return Some(57382),
        Key::Named(NamedKey::F20) => return Some(57383),
        Key::Named(NamedKey::F21) => return Some(57384),
        Key::Named(NamedKey::F22) => return Some(57385),
        Key::Named(NamedKey::F23) => return Some(57386),
        Key::Named(NamedKey::F24) => return Some(57387),
        Key::Named(NamedKey::F25) => return Some(57388),
        Key::Named(NamedKey::F26) => return Some(57389),
        Key::Named(NamedKey::F27) => return Some(57390),
        Key::Named(NamedKey::F28) => return Some(57391),
        Key::Named(NamedKey::F29) => return Some(57392),
        Key::Named(NamedKey::F30) => return Some(57393),
        Key::Named(NamedKey::F31) => return Some(57394),
        Key::Named(NamedKey::F32) => return Some(57395),
        Key::Named(NamedKey::F33) => return Some(57396),
        Key::Named(NamedKey::F34) => return Some(57397),
        Key::Named(NamedKey::F35) => return Some(57398),
        _ => {}
    }

    match physical_key {
        PhysicalKey::Code(KeyCode::PrintScreen) => Some(57361),
        PhysicalKey::Code(KeyCode::Pause) => Some(57362),
        PhysicalKey::Code(KeyCode::ContextMenu) => Some(57363),
        PhysicalKey::Code(KeyCode::F13) => Some(57376),
        PhysicalKey::Code(KeyCode::F14) => Some(57377),
        PhysicalKey::Code(KeyCode::F15) => Some(57378),
        PhysicalKey::Code(KeyCode::F16) => Some(57379),
        PhysicalKey::Code(KeyCode::F17) => Some(57380),
        PhysicalKey::Code(KeyCode::F18) => Some(57381),
        PhysicalKey::Code(KeyCode::F19) => Some(57382),
        PhysicalKey::Code(KeyCode::F20) => Some(57383),
        PhysicalKey::Code(KeyCode::F21) => Some(57384),
        PhysicalKey::Code(KeyCode::F22) => Some(57385),
        PhysicalKey::Code(KeyCode::F23) => Some(57386),
        PhysicalKey::Code(KeyCode::F24) => Some(57387),
        PhysicalKey::Code(KeyCode::F25) => Some(57388),
        PhysicalKey::Code(KeyCode::F26) => Some(57389),
        PhysicalKey::Code(KeyCode::F27) => Some(57390),
        PhysicalKey::Code(KeyCode::F28) => Some(57391),
        PhysicalKey::Code(KeyCode::F29) => Some(57392),
        PhysicalKey::Code(KeyCode::F30) => Some(57393),
        PhysicalKey::Code(KeyCode::F31) => Some(57394),
        PhysicalKey::Code(KeyCode::F32) => Some(57395),
        PhysicalKey::Code(KeyCode::F33) => Some(57396),
        PhysicalKey::Code(KeyCode::F34) => Some(57397),
        PhysicalKey::Code(KeyCode::F35) => Some(57398),
        _ => None,
    }
}

/// Format a Kitty Keyboard Protocol `CSI ... u` sequence.
#[allow(clippy::too_many_arguments)]
fn format_kitty_csi_u(
    base_cp: u32,
    shifted_cp: Option<u32>,
    base_layout_cp: Option<u32>,
    mods: KeyModifierFlags,
    event_type: u8,
    associated_text: Option<&str>,
    report_alternate: bool,
    report_text: bool,
) -> Vec<u8> {
    let mut key_part = base_cp.to_string();
    if report_alternate {
        let shifted = shifted_cp.filter(|&s| mods.shift && s != base_cp);
        let base_layout = base_layout_cp.filter(|&b| b != base_cp);
        match (shifted, base_layout) {
            (Some(s), Some(b)) => {
                key_part.push(':');
                key_part.push_str(&s.to_string());
                key_part.push(':');
                key_part.push_str(&b.to_string());
            }
            (Some(s), None) => {
                key_part.push(':');
                key_part.push_str(&s.to_string());
            }
            (None, Some(b)) => {
                key_part.push_str("::");
                key_part.push_str(&b.to_string());
            }
            (None, None) => {}
        }
    }

    let text_part = if report_text && event_type != 3 {
        associated_text.and_then(|s| {
            let cps: Vec<String> = s
                .chars()
                .filter(|&c| {
                    let cp = c as u32;
                    cp >= 32 && cp != 127 && !(0x80..=0x9f).contains(&cp)
                })
                .map(|c| (c as u32).to_string())
                .collect();
            if cps.is_empty() {
                None
            } else {
                Some(cps.join(":"))
            }
        })
    } else {
        None
    };

    let mod_bits = mods.kitty_bits();
    let mod_val = 1 + mod_bits;
    let mod_part = if event_type != 1 {
        Some(format!("{mod_val}:{event_type}"))
    } else if mod_bits != 0 {
        Some(mod_val.to_string())
    } else {
        None
    };

    match (mod_part, text_part) {
        (Some(m), Some(t)) => format!("\x1b[{key_part};{m};{t}u").into_bytes(),
        (None, Some(t)) => format!("\x1b[{key_part};;{t}u").into_bytes(),
        (Some(m), None) => format!("\x1b[{key_part};{m}u").into_bytes(),
        (None, None) => format!("\x1b[{key_part}u").into_bytes(),
    }
}

/// Format a CSI letter-terminated functional key (Arrows, Home, End, F1, F2, F4, legacy F3).
fn format_csi_letter(
    final_ch: char,
    app_cursor: bool,
    ss3_unmodified: bool,
    mods: KeyModifierFlags,
    event_type: u8,
    kitty_active: bool,
    kitty_report_all: bool,
) -> Vec<u8> {
    let mod_bits = if kitty_active {
        mods.kitty_bits()
    } else {
        mods.xterm_bits()
    };

    if event_type != 1 {
        format!("\x1b[1;{}:{}{}", 1 + mod_bits, event_type, final_ch).into_bytes()
    } else if mod_bits != 0 {
        format!("\x1b[1;{}{}", 1 + mod_bits, final_ch).into_bytes()
    } else if kitty_report_all {
        format!("\x1b[{final_ch}").into_bytes()
    } else if ss3_unmodified || app_cursor {
        format!("\x1bO{final_ch}").into_bytes()
    } else {
        format!("\x1b[{final_ch}").into_bytes()
    }
}

/// Format a CSI tilde-terminated functional key (`Insert`, `Delete`, `PageUp/Down`, `F3` under KKP, `F5`–`F12`).
fn format_csi_tilde(
    code: u8,
    mods: KeyModifierFlags,
    event_type: u8,
    kitty_active: bool,
) -> Vec<u8> {
    let mod_bits = if kitty_active {
        mods.kitty_bits()
    } else {
        mods.xterm_bits()
    };

    if event_type != 1 {
        format!("\x1b[{code};{}:{event_type}~", 1 + mod_bits).into_bytes()
    } else if mod_bits != 0 {
        format!("\x1b[{code};{}~", 1 + mod_bits).into_bytes()
    } else {
        format!("\x1b[{code}~").into_bytes()
    }
}

/// Resolve `(base_codepoint, shifted_codepoint, base_layout_codepoint)` for a character key.
fn resolve_char_key_codepoints(input: &KeyInput<'_>) -> Option<(u32, Option<u32>, Option<u32>)> {
    let base_layout_cp = physical_key_to_us_codepoint(input.physical_key);
    let logical_ch = match input.logical_key {
        Key::Character(ref s) => s.chars().next().filter(|c| !c.is_control()),
        _ => None,
    };
    let unshifted_from_os = match input.key_without_modifiers {
        Some(Key::Character(ref s)) => s
            .chars()
            .next()
            .filter(|c| !c.is_control())
            .map(|c| c.to_lowercase().next().unwrap_or(c)),
        _ => None,
    };

    if let Some(unshifted) = unshifted_from_os {
        let base_cp = unshifted as u32;
        let shifted_cp = logical_ch
            .map(|c| c as u32)
            .filter(|&sc| input.modifiers.shift && sc != base_cp);
        return Some((base_cp, shifted_cp, base_layout_cp));
    }

    if let Some(ch) = logical_ch {
        if ch.is_uppercase() {
            let lower = ch.to_lowercase().next().unwrap_or(ch);
            let shifted_cp = if input.modifiers.shift {
                Some(ch as u32)
            } else {
                None
            };
            return Some((lower as u32, shifted_cp, base_layout_cp));
        }

        if input.modifiers.shift {
            if let Some(bl) = base_layout_cp {
                if bl != ch as u32 && ch.is_ascii_punctuation() {
                    return Some((bl, Some(ch as u32), base_layout_cp));
                }
            }
            if let Some(unshifted) = unshift_us_ascii_symbol(ch) {
                return Some((unshifted as u32, Some(ch as u32), base_layout_cp));
            }
        }

        return Some((ch as u32, None, base_layout_cp));
    }

    base_layout_cp.map(|bl| (bl, None, Some(bl)))
}

/// Encode a full [`KeyInput`] into PTY bytes with full Kitty Keyboard Protocol (Flags 1/2/4/8/16),
/// xterm `modifyOtherKeys`, and legacy VT/xterm encodings.
pub fn encode_key_input(
    input: &KeyInput<'_>,
    app_cursor_keys: bool,
    kitty_flags: u16,
    modify_other_keys: u8,
) -> Option<Vec<u8>> {
    let mods = input.modifiers;
    let kitty_disambiguate = (kitty_flags & KITTY_FLAG_DISAMBIGUATE) != 0;
    let kitty_report_events = (kitty_flags & KITTY_FLAG_REPORT_EVENTS) != 0;
    let kitty_report_alternate = (kitty_flags & KITTY_FLAG_REPORT_ALTERNATE) != 0;
    let kitty_report_all = (kitty_flags & KITTY_FLAG_REPORT_ALL_KEYS) != 0;
    let kitty_report_text = (kitty_flags & KITTY_FLAG_REPORT_TEXT) != 0;
    let kitty_active = kitty_flags != 0;

    // Never leak Super/Cmd chords in legacy mode (macOS Cmd+C/V/etc. safety).
    if mods.super_key && !kitty_active {
        return None;
    }

    if input.state == ElementState::Released && !kitty_report_events {
        return None;
    }

    let event_type: u8 = if input.state == ElementState::Released {
        3
    } else if input.repeat && kitty_report_events {
        2
    } else {
        1
    };

    // 1. Standalone modifier & lock keys: only encoded under KKP Flag 8 (report all keys).
    if let Some(pua) =
        standalone_modifier_or_lock_pua(input.logical_key, input.physical_key, input.location)
    {
        if kitty_report_all {
            return Some(format_kitty_csi_u(
                pua, None, None, mods, event_type, None, false, false,
            ));
        }
        return None;
    }

    // 2. Numpad PUA keycodes (`57399`–`57427`) when KKP disambiguate or report-all is active.
    if kitty_disambiguate || kitty_report_all {
        if let Some((pua, default_text)) =
            numpad_pua_and_text(input.logical_key, input.physical_key, input.location)
        {
            let assoc_text = if !mods.has_non_shift_modifiers() {
                if pua == 57414 {
                    None
                } else {
                    input.text.or(default_text)
                }
            } else {
                None
            };
            return Some(format_kitty_csi_u(
                pua,
                None,
                None,
                mods,
                event_type,
                assoc_text,
                false,
                kitty_report_text,
            ));
        }
    }

    // 3. Extended functional keys (`PrintScreen`, `Pause`, `ContextMenu`, `F13`–`F35`).
    if let Some(pua) = extended_functional_pua(input.logical_key, input.physical_key) {
        if kitty_active {
            return Some(format_kitty_csi_u(
                pua, None, None, mods, event_type, None, false, false,
            ));
        }
        return None;
    }

    // 4. Standard named keys (functional navigation, F1-F12, and Esc/Enter/Tab/Backspace/Space).
    match input.logical_key {
        Key::Named(NamedKey::ArrowUp) => {
            return Some(format_csi_letter(
                'A',
                app_cursor_keys,
                false,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::ArrowDown) => {
            return Some(format_csi_letter(
                'B',
                app_cursor_keys,
                false,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::ArrowRight) => {
            return Some(format_csi_letter(
                'C',
                app_cursor_keys,
                false,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::ArrowLeft) => {
            return Some(format_csi_letter(
                'D',
                app_cursor_keys,
                false,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::Home) => {
            return Some(format_csi_letter(
                'H',
                app_cursor_keys,
                false,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::End) => {
            return Some(format_csi_letter(
                'F',
                app_cursor_keys,
                false,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::Insert) => {
            return Some(format_csi_tilde(2, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::Delete) => {
            return Some(format_csi_tilde(3, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::PageUp) => {
            return Some(format_csi_tilde(5, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::PageDown) => {
            return Some(format_csi_tilde(6, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F1) => {
            return Some(format_csi_letter(
                'P',
                false,
                true,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::F2) => {
            return Some(format_csi_letter(
                'Q',
                false,
                true,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::F3) => {
            // Under KKP, F3 uses `CSI 13 ; <m> ~` instead of `CSI 1 ; <m> R` so it never collides
            // with CPR (`CSI row ; col R`).
            if kitty_active
                && (kitty_disambiguate
                    || kitty_report_all
                    || mods.kitty_bits() != 0
                    || event_type != 1)
            {
                return Some(format_csi_tilde(13, mods, event_type, true));
            }
            return Some(format_csi_letter(
                'R',
                false,
                true,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::F4) => {
            return Some(format_csi_letter(
                'S',
                false,
                true,
                mods,
                event_type,
                kitty_active,
                kitty_report_all,
            ));
        }
        Key::Named(NamedKey::F5) => {
            return Some(format_csi_tilde(15, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F6) => {
            return Some(format_csi_tilde(17, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F7) => {
            return Some(format_csi_tilde(18, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F8) => {
            return Some(format_csi_tilde(19, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F9) => {
            return Some(format_csi_tilde(20, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F10) => {
            return Some(format_csi_tilde(21, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F11) => {
            return Some(format_csi_tilde(23, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::F12) => {
            return Some(format_csi_tilde(24, mods, event_type, kitty_active));
        }
        Key::Named(NamedKey::Escape) => {
            let use_kitty = kitty_disambiguate
                || kitty_report_all
                || (kitty_active && mods.has_non_lock_modifiers());
            if use_kitty {
                return Some(format_kitty_csi_u(
                    27, None, None, mods, event_type, None, false, false,
                ));
            }
            if input.state == ElementState::Released {
                return None;
            }
            if modify_other_keys > 0 {
                if let Some(m) = mods.xterm_param() {
                    return Some(format!("\x1b[27;{m};27~").into_bytes());
                }
            }
            return if mods.alt {
                Some(vec![0x1b, 0x1b])
            } else {
                Some(vec![0x1b])
            };
        }
        Key::Named(NamedKey::Enter) => {
            let use_kitty =
                kitty_report_all || (kitty_disambiguate && mods.has_non_lock_modifiers());
            if use_kitty {
                return Some(format_kitty_csi_u(
                    13, None, None, mods, event_type, None, false, false,
                ));
            }
            if input.state == ElementState::Released {
                return None;
            }
            if modify_other_keys > 0 {
                if let Some(m) = mods.xterm_param() {
                    return Some(format!("\x1b[27;{m};13~").into_bytes());
                }
            }
            return if mods.alt {
                Some(vec![0x1b, b'\r'])
            } else {
                Some(vec![b'\r'])
            };
        }
        Key::Named(NamedKey::Tab) => {
            let use_kitty =
                kitty_report_all || (kitty_disambiguate && mods.has_non_lock_modifiers());
            if use_kitty {
                return Some(format_kitty_csi_u(
                    9, None, None, mods, event_type, None, false, false,
                ));
            }
            if input.state == ElementState::Released {
                return None;
            }
            if modify_other_keys == 2 {
                if let Some(m) = mods.xterm_param() {
                    if m != 2 {
                        return Some(format!("\x1b[27;{m};9~").into_bytes());
                    }
                }
            }
            return if mods.alt && mods.shift {
                Some(b"\x1b\x1b[Z".to_vec())
            } else if mods.shift {
                Some(b"\x1b[Z".to_vec())
            } else if mods.alt {
                Some(vec![0x1b, b'\t'])
            } else {
                Some(vec![b'\t'])
            };
        }
        Key::Named(NamedKey::Backspace) => {
            let use_kitty =
                kitty_report_all || (kitty_disambiguate && mods.has_non_lock_modifiers());
            if use_kitty {
                return Some(format_kitty_csi_u(
                    127, None, None, mods, event_type, None, false, false,
                ));
            }
            if input.state == ElementState::Released {
                return None;
            }
            if modify_other_keys > 0 {
                if let Some(m) = mods.xterm_param() {
                    return Some(format!("\x1b[27;{m};127~").into_bytes());
                }
            }
            return if mods.alt {
                Some(vec![0x1b, 0x7f])
            } else {
                Some(vec![0x7f])
            };
        }
        Key::Named(NamedKey::Space) => {
            let use_kitty =
                kitty_report_all || (kitty_disambiguate && mods.has_non_shift_modifiers());
            if use_kitty {
                let assoc_text = if !mods.has_non_shift_modifiers() {
                    input.text.or(Some(" "))
                } else {
                    None
                };
                return Some(format_kitty_csi_u(
                    32,
                    None,
                    None,
                    mods,
                    event_type,
                    assoc_text,
                    false,
                    kitty_report_text,
                ));
            }
            if input.state == ElementState::Released {
                return None;
            }
            if modify_other_keys > 0 && mods.has_non_lock_modifiers() {
                if let Some(m) = mods.xterm_param() {
                    return Some(format!("\x1b[27;{m};32~").into_bytes());
                }
            }
            return if mods.ctrl {
                if mods.alt {
                    Some(vec![0x1b, 0x00])
                } else {
                    Some(vec![0x00])
                }
            } else if mods.alt {
                Some(vec![0x1b, b' '])
            } else {
                Some(vec![b' '])
            };
        }
        _ => {}
    }

    // 5. Printable / character keys (including Ctrl/Alt/Super chords and KKP Flag 4/8/16).
    let use_kitty = kitty_report_all || (kitty_disambiguate && mods.has_non_shift_modifiers());
    if use_kitty {
        if let Some((base_cp, shifted_cp, base_layout_cp)) = resolve_char_key_codepoints(input) {
            let logical_str = match input.logical_key {
                Key::Character(ref s) => Some(s.as_str()),
                _ => None,
            };
            let assoc_text = if !mods.has_non_shift_modifiers() {
                input.text.or(logical_str)
            } else {
                None
            };
            return Some(format_kitty_csi_u(
                base_cp,
                shifted_cp,
                base_layout_cp,
                mods,
                event_type,
                assoc_text,
                kitty_report_alternate,
                kitty_report_text,
            ));
        }
    }

    // Never emit release events for keys that are sent as plain legacy text (KKP Flag 2 without Flag 8).
    if input.state == ElementState::Released {
        return None;
    }

    // Legacy / xterm modifyOtherKeys path for character keys.
    if let Key::Character(ref s) = input.logical_key {
        if let Some(first_char) = s.chars().next() {
            let lower = first_char.to_ascii_lowercase();
            let cp = lower as u32;
            if let Some(m) = mods.xterm_param() {
                if modify_other_keys == 2 && (mods.ctrl || mods.alt) {
                    return Some(format!("\x1b[27;{m};{cp}~").into_bytes());
                }
                if modify_other_keys == 1
                    && mods.ctrl
                    && (!lower.is_ascii_lowercase() || mods.shift || mods.alt)
                {
                    return Some(format!("\x1b[27;{m};{cp}~").into_bytes());
                }
            }

            // Physical key fallback for Ctrl + Letter when a non-ASCII IME/layout is active.
            if mods.ctrl && !mods.alt && !mods.shift && !lower.is_ascii_lowercase() {
                if let Some(norm) = normalize_shortcut_char(input.logical_key, input.physical_key) {
                    if norm.len() == 1 {
                        let ch = norm.as_bytes()[0];
                        if ch.is_ascii_lowercase() {
                            return Some(vec![ch - b'a' + 1]);
                        }
                    }
                }
            }

            if mods.ctrl {
                let ctrl_byte = if lower.is_ascii_lowercase() {
                    Some((lower as u8) - b'a' + 1)
                } else {
                    match first_char {
                        ' ' | '@' | '2' => Some(0x00),
                        '[' | '3' => Some(0x1b),
                        '\\' | '4' => Some(0x1c),
                        ']' | '5' => Some(0x1d),
                        '^' | '6' | '~' => Some(0x1e),
                        '/' | '_' | '7' => Some(0x1f),
                        '?' | '8' => Some(0x7f),
                        _ => None,
                    }
                };
                if let Some(code) = ctrl_byte {
                    if mods.alt {
                        return Some(vec![0x1b, code]);
                    }
                    return Some(vec![code]);
                }
            }
            if mods.alt && !mods.ctrl && first_char.is_ascii() {
                return Some(vec![0x1b, first_char as u8]);
            }
        }
    } else if mods.ctrl && !mods.alt && !mods.shift {
        if let Some(norm) = normalize_shortcut_char(input.logical_key, input.physical_key) {
            if norm.len() == 1 {
                let ch = norm.as_bytes()[0];
                if ch.is_ascii_lowercase() {
                    return Some(vec![ch - b'a' + 1]);
                }
            }
        }
    }

    // Printable characters / IME text fallback.
    if let Some(text_str) = input.text {
        if !text_str.is_empty() {
            return Some(text_str.as_bytes().to_vec());
        }
    }
    if let Key::Character(ref s) = input.logical_key {
        return Some(s.as_bytes().to_vec());
    }
    None
}

/// Encode key parameters into raw bytes to send to the child PTY.
pub fn encode_key(
    logical_key: &Key,
    text: Option<&str>,
    state: ElementState,
    modifiers: ModifiersState,
    app_cursor_keys: bool,
) -> Option<Vec<u8>> {
    encode_key_with_modes(logical_key, text, state, modifiers, app_cursor_keys, 0, 0)
}

/// Encode key parameters with optional Kitty Keyboard Protocol (`kitty_flags`) and
/// xterm `modifyOtherKeys` (`modify_other_keys`) modes.
pub fn encode_key_with_modes(
    logical_key: &Key,
    text: Option<&str>,
    state: ElementState,
    modifiers: ModifiersState,
    app_cursor_keys: bool,
    kitty_flags: u16,
    modify_other_keys: u8,
) -> Option<Vec<u8>> {
    let input = KeyInput {
        logical_key,
        key_without_modifiers: None,
        physical_key: PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
        location: KeyLocation::Standard,
        text,
        state,
        repeat: false,
        modifiers: KeyModifierFlags::from(modifiers),
    };
    encode_key_input(&input, app_cursor_keys, kitty_flags, modify_other_keys)
}

/// Encode a winit KeyEvent into raw bytes to send to the child PTY.
pub fn encode_key_event(
    event: &KeyEvent,
    modifiers: ModifiersState,
    app_cursor_keys: bool,
) -> Option<Vec<u8>> {
    encode_key_event_with_modes(event, modifiers, app_cursor_keys, 0, 0)
}

/// Encode a winit KeyEvent with optional Kitty Keyboard Protocol and xterm modifyOtherKeys modes.
pub fn encode_key_event_with_modes(
    event: &KeyEvent,
    modifiers: ModifiersState,
    app_cursor_keys: bool,
    kitty_flags: u16,
    modify_other_keys: u8,
) -> Option<Vec<u8>> {
    encode_key_event_with_full_modes(
        event,
        KeyModifierFlags::from(modifiers),
        app_cursor_keys,
        kitty_flags,
        modify_other_keys,
    )
}

/// Encode a winit KeyEvent using full [`KeyModifierFlags`] (including CapsLock / NumLock),
/// `event.repeat`, `event.location`, `event.physical_key`, and OS `key_without_modifiers`.
pub fn encode_key_event_with_full_modes(
    event: &KeyEvent,
    modifiers: KeyModifierFlags,
    app_cursor_keys: bool,
    kitty_flags: u16,
    modify_other_keys: u8,
) -> Option<Vec<u8>> {
    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    let unmod = event.key_without_modifiers();
    #[cfg(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    ))]
    let key_without_modifiers = Some(&unmod);
    #[cfg(not(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux",
        target_os = "freebsd",
        target_os = "dragonfly",
        target_os = "netbsd",
        target_os = "openbsd"
    )))]
    let key_without_modifiers = None;

    let input = KeyInput {
        logical_key: &event.logical_key,
        key_without_modifiers,
        physical_key: event.physical_key,
        location: event.location,
        text: event.text.as_deref(),
        state: event.state,
        repeat: event.repeat,
        modifiers,
    };
    encode_key_input(&input, app_cursor_keys, kitty_flags, modify_other_keys)
}

// ============================================================================
// Mouse Protocol Encoding (`?9`, `?1000`, `?1002`, `?1003`, `?1006`, X10)
// ============================================================================

/// Compute xterm mouse modifier bitmask (Shift = +4, Alt = +8, Ctrl = +16).
pub fn mouse_modifier_bits(modifiers: ModifiersState) -> u8 {
    let mut bits = 0u8;
    if modifiers.shift_key() {
        bits |= 4;
    }
    if modifiers.alt_key() {
        bits |= 8;
    }
    if modifiers.control_key() {
        bits |= 16;
    }
    bits
}

fn mouse_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        MouseButton::Back => Some(128),
        MouseButton::Forward => Some(129),
        MouseButton::Other(_) => None,
    }
}

/// Select the 0-based `(y, x)` coordinates for mouse reporting according to `format`:
/// - `MouseFormat::SgrPixels` (`?1016`): physical pixel offsets `(px_y, px_x)` relative to pane content origin
/// - `MouseFormat::Sgr` (`?1006`) / `MouseFormat::X10`: cell coordinates `(row, col)`
#[inline]
pub fn resolve_mouse_report_coords(
    format: MouseFormat,
    row: usize,
    col: usize,
    px_x: usize,
    px_y: usize,
) -> (usize, usize) {
    match format {
        MouseFormat::SgrPixels => (px_y, px_x),
        MouseFormat::Sgr | MouseFormat::X10 => (row, col),
    }
}

/// Encode a mouse button press or release event for xterm / SGR / SGR-Pixels mouse protocols.
/// `row` and `col` are 0-based grid coordinates for `X10`/`Sgr`, or 0-based physical pixel
/// coordinates `(px_y, px_x)` relative to pane content origin for `SgrPixels` (`?1016`).
pub fn encode_mouse_button(
    button: MouseButton,
    pressed: bool,
    row: usize,
    col: usize,
    modifiers: ModifiersState,
    protocol: MouseProtocol,
    format: MouseFormat,
) -> Option<Vec<u8>> {
    if protocol == MouseProtocol::None {
        return None;
    }
    if protocol == MouseProtocol::X10 && !pressed {
        return None;
    }
    let base = mouse_button_code(button)?;
    let mods = if protocol == MouseProtocol::X10 {
        0
    } else {
        mouse_modifier_bits(modifiers)
    };
    let col_1 = col + 1;
    let row_1 = row + 1;

    match format {
        MouseFormat::Sgr | MouseFormat::SgrPixels => {
            let cb = base | mods;
            let suffix = if pressed { 'M' } else { 'm' };
            Some(format!("\x1b[<{cb};{col_1};{row_1}{suffix}").into_bytes())
        }
        MouseFormat::X10 => {
            if base >= 128 {
                return None;
            }
            let code = if pressed { base | mods } else { 3 | mods };
            let cb = code.saturating_add(32);
            let cx = (col_1.min(223) + 32) as u8;
            let cy = (row_1.min(223) + 32) as u8;
            Some(vec![0x1b, b'[', b'M', cb, cx, cy])
        }
    }
}

/// Encode a mouse motion event (`?1002` ButtonEvent drag or `?1003` AnyEvent drag/hover).
/// `button` is `Some(btn)` when dragging with a button held, or `None` for hover motion under `?1003`.
/// `row` and `col` are 0-based grid coordinates for `X10`/`Sgr`, or 0-based physical pixel
/// coordinates `(px_y, px_x)` relative to pane content origin for `SgrPixels` (`?1016`).
pub fn encode_mouse_motion(
    button: Option<MouseButton>,
    row: usize,
    col: usize,
    modifiers: ModifiersState,
    protocol: MouseProtocol,
    format: MouseFormat,
) -> Option<Vec<u8>> {
    match protocol {
        MouseProtocol::AnyEvent => {}
        MouseProtocol::ButtonEvent if button.is_some() => {}
        _ => return None,
    }
    let base = match button {
        Some(btn) => mouse_button_code(btn)?,
        None => 3,
    };
    let cb = base | 32 | mouse_modifier_bits(modifiers);
    let col_1 = col + 1;
    let row_1 = row + 1;

    match format {
        MouseFormat::Sgr | MouseFormat::SgrPixels => {
            Some(format!("\x1b[<{cb};{col_1};{row_1}M").into_bytes())
        }
        MouseFormat::X10 => {
            if base >= 128 {
                return None;
            }
            let cb_byte = cb.saturating_add(32);
            let cx = (col_1.min(223) + 32) as u8;
            let cy = (row_1.min(223) + 32) as u8;
            Some(vec![0x1b, b'[', b'M', cb_byte, cx, cy])
        }
    }
}

/// Encode a single mouse wheel step (`64` = up, `65` = down, `66` = left, `67` = right)
/// with xterm modifier bits (Shift +4, Alt +8, Ctrl +16). `row` and `col` are 0-based grid
/// coordinates for `X10`/`Sgr`, or 0-based physical pixel coordinates `(px_y, px_x)` for `SgrPixels`.
pub fn encode_mouse_wheel_step(
    wheel_button: u8,
    row: usize,
    col: usize,
    modifiers: ModifiersState,
    format: MouseFormat,
) -> Vec<u8> {
    let cb = wheel_button | mouse_modifier_bits(modifiers);
    let col_1 = col + 1;
    let row_1 = row + 1;
    match format {
        MouseFormat::Sgr | MouseFormat::SgrPixels => {
            format!("\x1b[<{cb};{col_1};{row_1}M").into_bytes()
        }
        MouseFormat::X10 => {
            let cb_byte = cb.saturating_add(32);
            let cx = (col_1.min(223) + 32) as u8;
            let cy = (row_1.min(223) + 32) as u8;
            vec![0x1b, b'[', b'M', cb_byte, cx, cy]
        }
    }
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

/// Normalize a key event into the character used for application shortcut matching.
///
/// Holding Shift changes the reported logical key (`[` becomes `{`, `]` becomes `}`),
/// which would otherwise break shortcuts such as `Cmd/Ctrl+Shift+[`. Shifted brackets are
/// therefore folded back onto their unshifted form, and non-ASCII layouts (such as macOS
/// Zhuyin/Bopomofo, Cangjie, Pinyin, etc.) fall back to the physical key code so shortcuts
/// like `Cmd+C`, `Cmd+V`, `Cmd+T`, `Cmd+W` always work regardless of active IME.
pub fn normalize_shortcut_char(logical_key: &Key, physical_key: PhysicalKey) -> Option<String> {
    if let Key::Character(ref s) = logical_key {
        let lower = s.to_lowercase();
        match lower.as_str() {
            "{" => return Some("[".to_string()),
            "}" => return Some("]".to_string()),
            "<" | "，" => return Some(",".to_string()),
            // Non-ASCII layouts, and control characters produced by Ctrl (some
            // platforms report Ctrl+[ as ESC), fall through to the physical key below.
            _ if lower.is_ascii() && !lower.chars().any(|c| c.is_control()) => return Some(lower),
            _ => {}
        }
    }

    let code = match physical_key {
        PhysicalKey::Code(KeyCode::KeyA) => "a",
        PhysicalKey::Code(KeyCode::KeyB) => "b",
        PhysicalKey::Code(KeyCode::KeyC) => "c",
        PhysicalKey::Code(KeyCode::KeyD) => "d",
        PhysicalKey::Code(KeyCode::KeyE) => "e",
        PhysicalKey::Code(KeyCode::KeyF) => "f",
        PhysicalKey::Code(KeyCode::KeyG) => "g",
        PhysicalKey::Code(KeyCode::KeyH) => "h",
        PhysicalKey::Code(KeyCode::KeyI) => "i",
        PhysicalKey::Code(KeyCode::KeyJ) => "j",
        PhysicalKey::Code(KeyCode::KeyK) => "k",
        PhysicalKey::Code(KeyCode::KeyL) => "l",
        PhysicalKey::Code(KeyCode::KeyM) => "m",
        PhysicalKey::Code(KeyCode::KeyN) => "n",
        PhysicalKey::Code(KeyCode::KeyO) => "o",
        PhysicalKey::Code(KeyCode::KeyP) => "p",
        PhysicalKey::Code(KeyCode::KeyQ) => "q",
        PhysicalKey::Code(KeyCode::KeyR) => "r",
        PhysicalKey::Code(KeyCode::KeyS) => "s",
        PhysicalKey::Code(KeyCode::KeyT) => "t",
        PhysicalKey::Code(KeyCode::KeyU) => "u",
        PhysicalKey::Code(KeyCode::KeyV) => "v",
        PhysicalKey::Code(KeyCode::KeyW) => "w",
        PhysicalKey::Code(KeyCode::KeyX) => "x",
        PhysicalKey::Code(KeyCode::KeyY) => "y",
        PhysicalKey::Code(KeyCode::KeyZ) => "z",
        PhysicalKey::Code(KeyCode::Digit0 | KeyCode::Numpad0) => "0",
        PhysicalKey::Code(KeyCode::Digit1 | KeyCode::Numpad1) => "1",
        PhysicalKey::Code(KeyCode::Digit2 | KeyCode::Numpad2) => "2",
        PhysicalKey::Code(KeyCode::Digit3 | KeyCode::Numpad3) => "3",
        PhysicalKey::Code(KeyCode::Digit4 | KeyCode::Numpad4) => "4",
        PhysicalKey::Code(KeyCode::Digit5 | KeyCode::Numpad5) => "5",
        PhysicalKey::Code(KeyCode::Digit6 | KeyCode::Numpad6) => "6",
        PhysicalKey::Code(KeyCode::Digit7 | KeyCode::Numpad7) => "7",
        PhysicalKey::Code(KeyCode::Digit8 | KeyCode::Numpad8) => "8",
        PhysicalKey::Code(KeyCode::Digit9 | KeyCode::Numpad9) => "9",
        PhysicalKey::Code(KeyCode::BracketLeft) => "[",
        PhysicalKey::Code(KeyCode::BracketRight) => "]",
        PhysicalKey::Code(KeyCode::Comma) => ",",
        PhysicalKey::Code(KeyCode::Period) => ".",
        PhysicalKey::Code(KeyCode::Equal | KeyCode::NumpadAdd) => "=",
        PhysicalKey::Code(KeyCode::Minus | KeyCode::NumpadSubtract) => "-",
        PhysicalKey::Code(KeyCode::Slash) => "/",
        PhysicalKey::Code(KeyCode::Backslash) => "\\",
        PhysicalKey::Code(KeyCode::Semicolon) => ";",
        PhysicalKey::Code(KeyCode::Quote) => "'",
        PhysicalKey::Code(KeyCode::Backquote) => "`",
        _ => return None,
    };
    Some(code.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain_input<'a>(logical_key: &'a Key, text: Option<&'a str>) -> KeyInput<'a> {
        KeyInput {
            logical_key,
            key_without_modifiers: None,
            physical_key: PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
            location: KeyLocation::Standard,
            text,
            state: ElementState::Pressed,
            repeat: false,
            modifiers: KeyModifierFlags::default(),
        }
    }

    #[test]
    fn kkp_flag2_without_flag8_suppresses_plain_text_release_and_repeat_csi() {
        let key_a = Key::Character("a".into());
        let flags = KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_EVENTS; // 3

        // Plain 'a' press -> raw UTF-8 "a"
        let press = plain_input(&key_a, Some("a"));
        assert_eq!(
            encode_key_input(&press, false, flags, 0).as_deref(),
            Some(b"a".as_slice())
        );

        // Plain 'a' repeat without Flag 8 -> raw UTF-8 "a" (NOT CSI 97;1:2u)
        let mut repeat = plain_input(&key_a, Some("a"));
        repeat.repeat = true;
        assert_eq!(
            encode_key_input(&repeat, false, flags, 0).as_deref(),
            Some(b"a".as_slice())
        );

        // Plain 'a' release without Flag 8 -> None (NOT CSI 97;1:3u)
        let mut release = plain_input(&key_a, None);
        release.state = ElementState::Released;
        assert_eq!(encode_key_input(&release, false, flags, 0), None);

        // Plain Enter / Tab / Backspace / Space release without Flag 8 -> None
        for named in [
            NamedKey::Enter,
            NamedKey::Tab,
            NamedKey::Backspace,
            NamedKey::Space,
        ] {
            let k = Key::Named(named);
            let mut rel = plain_input(&k, None);
            rel.state = ElementState::Released;
            assert_eq!(
                encode_key_input(&rel, false, flags, 0),
                None,
                "unmodified {named:?} release should not emit CSI u without Flag 8"
            );
        }

        // Shift+A release without Flag 8 -> None
        let key_upper_a = Key::Character("A".into());
        let mut shift_rel = plain_input(&key_upper_a, None);
        shift_rel.state = ElementState::Released;
        shift_rel.modifiers.shift = true;
        assert_eq!(encode_key_input(&shift_rel, false, flags, 0), None);
    }

    #[test]
    fn kkp_flag2_emits_repeat_and_release_for_all_csi_keys() {
        let flags = KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_EVENTS; // 3

        // Escape repeat & release
        let esc = Key::Named(NamedKey::Escape);
        let mut esc_rep = plain_input(&esc, None);
        esc_rep.repeat = true;
        assert_eq!(
            encode_key_input(&esc_rep, false, flags, 0).as_deref(),
            Some(b"\x1b[27;1:2u".as_slice())
        );
        let mut esc_rel = plain_input(&esc, None);
        esc_rel.state = ElementState::Released;
        assert_eq!(
            encode_key_input(&esc_rel, false, flags, 0).as_deref(),
            Some(b"\x1b[27;1:3u".as_slice())
        );

        // ArrowUp repeat & release
        let up = Key::Named(NamedKey::ArrowUp);
        let mut up_rep = plain_input(&up, None);
        up_rep.repeat = true;
        assert_eq!(
            encode_key_input(&up_rep, false, flags, 0).as_deref(),
            Some(b"\x1b[1;1:2A".as_slice())
        );
        let mut up_rel = plain_input(&up, None);
        up_rel.state = ElementState::Released;
        assert_eq!(
            encode_key_input(&up_rel, false, flags, 0).as_deref(),
            Some(b"\x1b[1;1:3A".as_slice())
        );

        // Shift+PageUp release
        let pgup = Key::Named(NamedKey::PageUp);
        let mut pgup_rel = plain_input(&pgup, None);
        pgup_rel.state = ElementState::Released;
        pgup_rel.modifiers.shift = true;
        assert_eq!(
            encode_key_input(&pgup_rel, false, flags, 0).as_deref(),
            Some(b"\x1b[5;2:3~".as_slice())
        );

        // F1 release -> CSI 1;1:3P, F5 repeat -> CSI 15;1:2~
        let f1 = Key::Named(NamedKey::F1);
        let mut f1_rel = plain_input(&f1, None);
        f1_rel.state = ElementState::Released;
        assert_eq!(
            encode_key_input(&f1_rel, false, flags, 0).as_deref(),
            Some(b"\x1b[1;1:3P".as_slice())
        );
        let f5 = Key::Named(NamedKey::F5);
        let mut f5_rep = plain_input(&f5, None);
        f5_rep.repeat = true;
        assert_eq!(
            encode_key_input(&f5_rep, false, flags, 0).as_deref(),
            Some(b"\x1b[15;1:2~".as_slice())
        );

        // Shift+Enter repeat & release
        let enter = Key::Named(NamedKey::Enter);
        let mut se_rep = plain_input(&enter, None);
        se_rep.repeat = true;
        se_rep.modifiers.shift = true;
        assert_eq!(
            encode_key_input(&se_rep, false, flags, 0).as_deref(),
            Some(b"\x1b[13;2:2u".as_slice())
        );
        let mut se_rel = plain_input(&enter, None);
        se_rel.state = ElementState::Released;
        se_rel.modifiers.shift = true;
        assert_eq!(
            encode_key_input(&se_rel, false, flags, 0).as_deref(),
            Some(b"\x1b[13;2:3u".as_slice())
        );
    }

    #[test]
    fn kkp_f3_uses_csi_13_tilde_instead_of_cpr_collision() {
        let f3 = Key::Named(NamedKey::F3);

        // Legacy mode: unmodified ESC O R, Shift+F3 CSI 1;2R
        let legacy_plain = plain_input(&f3, None);
        assert_eq!(
            encode_key_input(&legacy_plain, false, 0, 0).as_deref(),
            Some(b"\x1bOR".as_slice())
        );
        let mut legacy_shift = plain_input(&f3, None);
        legacy_shift.modifiers.shift = true;
        assert_eq!(
            encode_key_input(&legacy_shift, false, 0, 0).as_deref(),
            Some(b"\x1b[1;2R".as_slice())
        );

        // KKP mode: unmodified CSI 13~, Shift+F3 CSI 13;2~, release CSI 13;2:3~
        assert_eq!(
            encode_key_input(&legacy_plain, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[13~".as_slice())
        );
        assert_eq!(
            encode_key_input(&legacy_shift, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[13;2~".as_slice())
        );
        let mut kkp_rel = legacy_shift;
        kkp_rel.state = ElementState::Released;
        assert_eq!(
            encode_key_input(
                &kkp_rel,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_EVENTS,
                0
            )
            .as_deref(),
            Some(b"\x1b[13;2:3~".as_slice())
        );
    }

    #[test]
    fn kkp_flag4_alternate_keys_shifted_and_base_layout() {
        // Ctrl+Shift+/ on US keyboard -> base '/' (47), shifted '?' (63)
        let qmark = Key::Character("?".into());
        let slash = Key::Character("/".into());
        let mut ctrl_shift_slash = plain_input(&qmark, None);
        ctrl_shift_slash.key_without_modifiers = Some(&slash);
        ctrl_shift_slash.physical_key = PhysicalKey::Code(KeyCode::Slash);
        ctrl_shift_slash.modifiers.ctrl = true;
        ctrl_shift_slash.modifiers.shift = true;

        // Flag 1 only: uses unshifted base '/' (47) instead of '?' (63)
        assert_eq!(
            encode_key_input(&ctrl_shift_slash, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[47;6u".as_slice())
        );
        // Flag 1 + 4: appends shifted '?' (63), omits identical base layout 47
        assert_eq!(
            encode_key_input(
                &ctrl_shift_slash,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_ALTERNATE,
                0
            )
            .as_deref(),
            Some(b"\x1b[47:63;6u".as_slice())
        );

        // Russian Cyrillic 'с' (1089) on physical KeyC ('c' = 99)
        let cyr_c = Key::Character("с".into());
        let cyr_upper_c = Key::Character("С".into());
        let mut ru_ctrl = plain_input(&cyr_c, None);
        ru_ctrl.key_without_modifiers = Some(&cyr_c);
        ru_ctrl.physical_key = PhysicalKey::Code(KeyCode::KeyC);
        ru_ctrl.modifiers.ctrl = true;
        assert_eq!(
            encode_key_input(
                &ru_ctrl,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_ALTERNATE,
                0
            )
            .as_deref(),
            Some(b"\x1b[1089::99;5u".as_slice())
        );

        let mut ru_ctrl_shift = plain_input(&cyr_upper_c, None);
        ru_ctrl_shift.key_without_modifiers = Some(&cyr_c);
        ru_ctrl_shift.physical_key = PhysicalKey::Code(KeyCode::KeyC);
        ru_ctrl_shift.modifiers.ctrl = true;
        ru_ctrl_shift.modifiers.shift = true;
        assert_eq!(
            encode_key_input(
                &ru_ctrl_shift,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_ALTERNATE,
                0
            )
            .as_deref(),
            Some(b"\x1b[1089:1057:99;6u".as_slice())
        );
    }

    #[test]
    fn kkp_flag8_and_flag16_report_all_keys_modifiers_and_associated_text() {
        let key_a = Key::Character("a".into());
        let key_upper_a = Key::Character("A".into());

        // Plain 'a' under Flag 8 -> CSI 97u
        let press_a = plain_input(&key_a, Some("a"));
        assert_eq!(
            encode_key_input(&press_a, false, KITTY_FLAG_REPORT_ALL_KEYS, 0).as_deref(),
            Some(b"\x1b[97u".as_slice())
        );

        // Plain 'a' under Flag 8 + 16 -> CSI 97;;97u
        assert_eq!(
            encode_key_input(
                &press_a,
                false,
                KITTY_FLAG_REPORT_ALL_KEYS | KITTY_FLAG_REPORT_TEXT,
                0
            )
            .as_deref(),
            Some(b"\x1b[97;;97u".as_slice())
        );

        // Shift+A under Flags 2|4|8|16 -> press: CSI 97:65;2;65u, repeat: CSI 97:65;2:2;65u, release: CSI 97:65;2:3u (no text!)
        let all_flags = KITTY_FLAG_DISAMBIGUATE
            | KITTY_FLAG_REPORT_EVENTS
            | KITTY_FLAG_REPORT_ALTERNATE
            | KITTY_FLAG_REPORT_ALL_KEYS
            | KITTY_FLAG_REPORT_TEXT;
        let mut shift_a = plain_input(&key_upper_a, Some("A"));
        shift_a.key_without_modifiers = Some(&key_a);
        shift_a.physical_key = PhysicalKey::Code(KeyCode::KeyA);
        shift_a.modifiers.shift = true;
        assert_eq!(
            encode_key_input(&shift_a, false, all_flags, 0).as_deref(),
            Some(b"\x1b[97:65;2;65u".as_slice())
        );
        shift_a.repeat = true;
        assert_eq!(
            encode_key_input(&shift_a, false, all_flags, 0).as_deref(),
            Some(b"\x1b[97:65;2:2;65u".as_slice())
        );
        shift_a.repeat = false;
        shift_a.state = ElementState::Released;
        assert_eq!(
            encode_key_input(&shift_a, false, all_flags, 0).as_deref(),
            Some(b"\x1b[97:65;2:3u".as_slice())
        );

        // Plain Enter/Tab/Backspace under Flag 8
        for (named, expected) in [
            (NamedKey::Enter, b"\x1b[13u".as_slice()),
            (NamedKey::Tab, b"\x1b[9u".as_slice()),
            (NamedKey::Backspace, b"\x1b[127u".as_slice()),
        ] {
            let k = Key::Named(named);
            let inp = plain_input(&k, None);
            assert_eq!(
                encode_key_input(&inp, false, KITTY_FLAG_REPORT_ALL_KEYS, 0).as_deref(),
                Some(expected)
            );
        }

        // Standalone ShiftLeft & SuperRight under Flag 8 + 2
        let shift_k = Key::Named(NamedKey::Shift);
        let mut lshift = plain_input(&shift_k, None);
        lshift.physical_key = PhysicalKey::Code(KeyCode::ShiftLeft);
        lshift.location = KeyLocation::Left;
        assert_eq!(
            encode_key_input(&lshift, false, KITTY_FLAG_DISAMBIGUATE, 0),
            None
        );
        assert_eq!(
            encode_key_input(&lshift, false, KITTY_FLAG_REPORT_ALL_KEYS, 0).as_deref(),
            Some(b"\x1b[57441u".as_slice())
        );
        lshift.state = ElementState::Released;
        assert_eq!(
            encode_key_input(
                &lshift,
                false,
                KITTY_FLAG_REPORT_ALL_KEYS | KITTY_FLAG_REPORT_EVENTS,
                0
            )
            .as_deref(),
            Some(b"\x1b[57441;1:3u".as_slice())
        );

        let super_k = Key::Named(NamedKey::Super);
        let mut rsuper = plain_input(&super_k, None);
        rsuper.physical_key = PhysicalKey::Code(KeyCode::SuperRight);
        rsuper.location = KeyLocation::Right;
        assert_eq!(
            encode_key_input(&rsuper, false, KITTY_FLAG_REPORT_ALL_KEYS, 0).as_deref(),
            Some(b"\x1b[57450u".as_slice())
        );
    }

    #[test]
    fn kkp_numpad_extended_fkeys_and_super_capslock_numlock_modifiers() {
        // Numpad1 with Flag 1 + 16 -> CSI 57400;;49u
        let one = Key::Character("1".into());
        let mut kp1 = plain_input(&one, Some("1"));
        kp1.physical_key = PhysicalKey::Code(KeyCode::Numpad1);
        kp1.location = KeyLocation::Numpad;
        assert_eq!(
            encode_key_input(
                &kp1,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_TEXT,
                0
            )
            .as_deref(),
            Some(b"\x1b[57400;;49u".as_slice())
        );

        // NumpadEnter with Flag 1 -> CSI 57414u
        let enter = Key::Named(NamedKey::Enter);
        let mut kp_enter = plain_input(&enter, None);
        kp_enter.physical_key = PhysicalKey::Code(KeyCode::NumpadEnter);
        kp_enter.location = KeyLocation::Numpad;
        assert_eq!(
            encode_key_input(&kp_enter, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[57414u".as_slice())
        );

        // F13, PrintScreen, Pause, ContextMenu
        let f13 = Key::Named(NamedKey::F13);
        assert_eq!(
            encode_key_input(&plain_input(&f13, None), false, KITTY_FLAG_DISAMBIGUATE, 0)
                .as_deref(),
            Some(b"\x1b[57376u".as_slice())
        );
        let prtsc = Key::Named(NamedKey::PrintScreen);
        assert_eq!(
            encode_key_input(
                &plain_input(&prtsc, None),
                false,
                KITTY_FLAG_DISAMBIGUATE,
                0
            )
            .as_deref(),
            Some(b"\x1b[57361u".as_slice())
        );

        // Super (+8) + CapsLock (+64) + NumLock (+128) modifier bits
        let key_s = Key::Character("s".into());
        let mut super_s = plain_input(&key_s, Some("s"));
        super_s.modifiers.super_key = true;
        // Legacy blocks Super
        assert_eq!(encode_key_input(&super_s, false, 0, 0), None);
        // KKP encodes Super as 1 + 8 = 9
        assert_eq!(
            encode_key_input(&super_s, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[115;9u".as_slice())
        );

        super_s.modifiers.caps_lock = true;
        super_s.modifiers.num_lock = true;
        // 1 + 8 + 64 + 128 = 201
        assert_eq!(
            encode_key_input(&super_s, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[115;201u".as_slice())
        );
    }

    #[test]
    fn legacy_alt_special_keys_prepend_esc() {
        let mut alt = ModifiersState::empty();
        alt.insert(ModifiersState::ALT);

        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Enter),
                None,
                ElementState::Pressed,
                alt,
                false
            ),
            Some(vec![0x1b, b'\r'])
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Backspace),
                None,
                ElementState::Pressed,
                alt,
                false
            ),
            Some(vec![0x1b, 0x7f])
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Tab),
                None,
                ElementState::Pressed,
                alt,
                false
            ),
            Some(vec![0x1b, b'\t'])
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Escape),
                None,
                ElementState::Pressed,
                alt,
                false
            ),
            Some(vec![0x1b, 0x1b])
        );
    }

    #[test]
    fn mouse_encoding_sgr_and_x10_with_modifiers_drag_hover_and_wheel() {
        let none = ModifiersState::empty();
        let mut ctrl_alt = ModifiersState::empty();
        ctrl_alt.insert(ModifiersState::CONTROL | ModifiersState::ALT);
        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        // SGR press & release (0-based row=4, col=9 -> 1-based col=10, row=5)
        assert_eq!(
            encode_mouse_button(
                MouseButton::Left,
                true,
                4,
                9,
                none,
                MouseProtocol::Normal,
                MouseFormat::Sgr
            )
            .as_deref(),
            Some(b"\x1b[<0;10;5M".as_slice())
        );
        assert_eq!(
            encode_mouse_button(
                MouseButton::Left,
                false,
                4,
                9,
                none,
                MouseProtocol::Normal,
                MouseFormat::Sgr
            )
            .as_deref(),
            Some(b"\x1b[<0;10;5m".as_slice())
        );
        // Right button with Ctrl+Alt (2 | 8 | 16 = 26)
        assert_eq!(
            encode_mouse_button(
                MouseButton::Right,
                true,
                0,
                0,
                ctrl_alt,
                MouseProtocol::Normal,
                MouseFormat::Sgr
            )
            .as_deref(),
            Some(b"\x1b[<26;1;1M".as_slice())
        );

        // X10 press & release
        assert_eq!(
            encode_mouse_button(
                MouseButton::Middle,
                true,
                1,
                2,
                none,
                MouseProtocol::Normal,
                MouseFormat::X10
            ),
            Some(vec![0x1b, b'[', b'M', 33, 35, 34])
        );
        assert_eq!(
            encode_mouse_button(
                MouseButton::Middle,
                false,
                1,
                2,
                none,
                MouseProtocol::Normal,
                MouseFormat::X10
            ),
            Some(vec![0x1b, b'[', b'M', 35, 35, 34])
        );
        // X10 protocol (?9) ignores release
        assert_eq!(
            encode_mouse_button(
                MouseButton::Left,
                false,
                1,
                2,
                none,
                MouseProtocol::X10,
                MouseFormat::X10
            ),
            None
        );

        // ButtonEvent (?1002): drag allowed, hover suppressed
        assert_eq!(
            encode_mouse_motion(
                Some(MouseButton::Left),
                2,
                3,
                none,
                MouseProtocol::ButtonEvent,
                MouseFormat::Sgr
            )
            .as_deref(),
            Some(b"\x1b[<32;4;3M".as_slice())
        );
        assert_eq!(
            encode_mouse_motion(
                None,
                2,
                3,
                none,
                MouseProtocol::ButtonEvent,
                MouseFormat::Sgr
            ),
            None
        );

        // AnyEvent (?1003): hover allowed (3 | 32 = 35; with Shift: 35 | 4 = 39)
        assert_eq!(
            encode_mouse_motion(None, 2, 3, shift, MouseProtocol::AnyEvent, MouseFormat::Sgr)
                .as_deref(),
            Some(b"\x1b[<39;4;3M".as_slice())
        );

        // Wheel vertical (64/65) and horizontal (66/67) with Ctrl+Alt (+24)
        assert_eq!(
            encode_mouse_wheel_step(64, 0, 0, ctrl_alt, MouseFormat::Sgr),
            b"\x1b[<88;1;1M"
        );
        assert_eq!(
            encode_mouse_wheel_step(66, 3, 5, none, MouseFormat::Sgr),
            b"\x1b[<66;6;4M"
        );
        assert_eq!(
            encode_mouse_wheel_step(67, 3, 5, shift, MouseFormat::Sgr),
            b"\x1b[<71;6;4M"
        );

        // Back (128) / Forward (129) in SGR vs X10
        assert_eq!(
            encode_mouse_button(
                MouseButton::Back,
                true,
                0,
                0,
                none,
                MouseProtocol::Normal,
                MouseFormat::Sgr
            )
            .as_deref(),
            Some(b"\x1b[<128;1;1M".as_slice())
        );
        assert_eq!(
            encode_mouse_button(
                MouseButton::Forward,
                false,
                0,
                0,
                none,
                MouseProtocol::Normal,
                MouseFormat::Sgr
            )
            .as_deref(),
            Some(b"\x1b[<129;1;1m".as_slice())
        );
        assert_eq!(
            encode_mouse_button(
                MouseButton::Back,
                true,
                0,
                0,
                none,
                MouseProtocol::Normal,
                MouseFormat::X10
            ),
            None
        );
    }

    #[test]
    fn kkp_disambiguate_ctrl_i_m_bracket_space_and_numpad_nav_locks() {
        // Ctrl+i, Ctrl+m, Ctrl+[ under Flag 1
        for (ch, expected) in [
            ("i", b"\x1b[105;5u".as_slice()),
            ("m", b"\x1b[109;5u".as_slice()),
            ("[", b"\x1b[91;5u".as_slice()),
        ] {
            let k = Key::Character(ch.into());
            let mut inp = plain_input(&k, None);
            inp.modifiers.ctrl = true;
            assert_eq!(
                encode_key_input(&inp, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
                Some(expected)
            );
        }

        // Shift+Space: plain space under Flag 1+2 (no release), CSI u under Flag 8+16
        let sp = Key::Named(NamedKey::Space);
        let mut shift_sp = plain_input(&sp, Some(" "));
        shift_sp.modifiers.shift = true;
        assert_eq!(
            encode_key_input(
                &shift_sp,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_EVENTS,
                0
            )
            .as_deref(),
            Some(b" ".as_slice())
        );
        assert_eq!(
            encode_key_input(
                &shift_sp,
                false,
                KITTY_FLAG_REPORT_ALL_KEYS | KITTY_FLAG_REPORT_TEXT,
                0
            )
            .as_deref(),
            Some(b"\x1b[32;2;32u".as_slice())
        );
        shift_sp.state = ElementState::Released;
        assert_eq!(
            encode_key_input(
                &shift_sp,
                false,
                KITTY_FLAG_DISAMBIGUATE | KITTY_FLAG_REPORT_EVENTS,
                0
            ),
            None
        );
        assert_eq!(
            encode_key_input(
                &shift_sp,
                false,
                KITTY_FLAG_REPORT_ALL_KEYS | KITTY_FLAG_REPORT_EVENTS | KITTY_FLAG_REPORT_TEXT,
                0
            )
            .as_deref(),
            Some(b"\x1b[32;2:3u".as_slice())
        );

        // Numpad nav (NumLock off) KP_UP -> 57419, plus Pause/ContextMenu/F35 and CapsLock/NumLock
        let up = Key::Named(NamedKey::ArrowUp);
        let mut kp_up = plain_input(&up, None);
        kp_up.location = KeyLocation::Numpad;
        assert_eq!(
            encode_key_input(&kp_up, false, KITTY_FLAG_DISAMBIGUATE, 0).as_deref(),
            Some(b"\x1b[57419u".as_slice())
        );

        let pause = Key::Named(NamedKey::Pause);
        assert_eq!(
            encode_key_input(
                &plain_input(&pause, None),
                false,
                KITTY_FLAG_DISAMBIGUATE,
                0
            )
            .as_deref(),
            Some(b"\x1b[57362u".as_slice())
        );
        let menu = Key::Named(NamedKey::ContextMenu);
        assert_eq!(
            encode_key_input(&plain_input(&menu, None), false, KITTY_FLAG_DISAMBIGUATE, 0)
                .as_deref(),
            Some(b"\x1b[57363u".as_slice())
        );
        let f35 = Key::Named(NamedKey::F35);
        assert_eq!(
            encode_key_input(&plain_input(&f35, None), false, KITTY_FLAG_DISAMBIGUATE, 0)
                .as_deref(),
            Some(b"\x1b[57398u".as_slice())
        );
        let caps = Key::Named(NamedKey::CapsLock);
        assert_eq!(
            encode_key_input(
                &plain_input(&caps, None),
                false,
                KITTY_FLAG_REPORT_ALL_KEYS,
                0
            )
            .as_deref(),
            Some(b"\x1b[57358u".as_slice())
        );
        let numl = Key::Named(NamedKey::NumLock);
        assert_eq!(
            encode_key_input(
                &plain_input(&numl, None),
                false,
                KITTY_FLAG_REPORT_ALL_KEYS,
                0
            )
            .as_deref(),
            Some(b"\x1b[57360u".as_slice())
        );
    }

    #[test]
    fn sgr_pixels_mouse_encoding_press_release_motion_and_wheel() {
        // Coordinate resolver picks (px_y, px_x) for SgrPixels and (row, col) for Sgr/X10
        assert_eq!(
            resolve_mouse_report_coords(MouseFormat::SgrPixels, 2, 5, 1919, 1079),
            (1079, 1919)
        );
        assert_eq!(
            resolve_mouse_report_coords(MouseFormat::Sgr, 2, 5, 1919, 1079),
            (2, 5)
        );
        assert_eq!(
            resolve_mouse_report_coords(MouseFormat::X10, 2, 5, 1919, 1079),
            (2, 5)
        );

        // Press at 0-based pixel (0, 0) -> 1-based (1, 1)
        let (ry, rx) = resolve_mouse_report_coords(MouseFormat::SgrPixels, 0, 0, 0, 0);
        assert_eq!(
            encode_mouse_button(
                MouseButton::Left,
                true,
                ry,
                rx,
                ModifiersState::empty(),
                MouseProtocol::Normal,
                MouseFormat::SgrPixels,
            )
            .as_deref(),
            Some(b"\x1b[<0;1;1M".as_slice())
        );

        // Release at large physical pixel (1439, 899) -> 1-based (1440, 900)
        let (ry, rx) = resolve_mouse_report_coords(MouseFormat::SgrPixels, 10, 20, 1439, 899);
        assert_eq!(
            encode_mouse_button(
                MouseButton::Right,
                false,
                ry,
                rx,
                ModifiersState::CONTROL,
                MouseProtocol::Normal,
                MouseFormat::SgrPixels,
            )
            .as_deref(),
            Some(b"\x1b[<18;1440;900m".as_slice())
        );

        // Drag motion (?1002) with Left button at pixel (312, 405) -> 1-based (313, 406)
        assert_eq!(
            encode_mouse_motion(
                Some(MouseButton::Left),
                405,
                312,
                ModifiersState::ALT,
                MouseProtocol::ButtonEvent,
                MouseFormat::SgrPixels,
            )
            .as_deref(),
            Some(b"\x1b[<40;313;406M".as_slice())
        );

        // Hover motion (?1003) with no button at pixel (99, 200) -> cb=35, 1-based (100, 201)
        assert_eq!(
            encode_mouse_motion(
                None,
                200,
                99,
                ModifiersState::empty(),
                MouseProtocol::AnyEvent,
                MouseFormat::SgrPixels,
            )
            .as_deref(),
            Some(b"\x1b[<35;100;201M".as_slice())
        );

        // Wheel up (64) + Shift (+4) at pixel (500, 250) -> cb=68, 1-based (501, 251)
        assert_eq!(
            encode_mouse_wheel_step(64, 250, 500, ModifiersState::SHIFT, MouseFormat::SgrPixels),
            b"\x1b[<68;501;251M"
        );
    }
}
