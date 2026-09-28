//! `skterm-gui`: Application window loop, input handling, config, tabs, and split panes multiplexer.

pub mod app;
pub mod config;
pub mod egui_fonts;
pub mod icon;
pub mod input;
pub mod layout;
#[cfg(target_os = "macos")]
pub mod macos_textfield;
#[cfg(target_os = "macos")]
pub mod macos_titlebar;
pub mod preferences_window;
pub mod tab_title;

pub use app::{build_top_dropdown_menu, hit_test_dropdown_menu, App, TopMenuAction};
pub use config::{default_config_template, parse_hex_color, Config, ConfigManager, Theme};
pub use icon::{apply_window_icon, configure_window_attributes, get_app_icon};
pub use input::{
    check_tab_switch, check_tab_switch_for_os, encode_key, encode_key_event,
    encode_key_event_with_modes, encode_key_with_modes, is_tab_switch_modifier,
    is_tab_switch_modifier_for_os, parse_tab_switch_digit,
};
pub use layout::{
    PaneId, PaneNode, PollOutcome, Rect, SplitDirection, SplitterBar, Tab, TabId, Workspace,
};
pub use preferences_window::{PreferencesCategory, PreferencesWindow};

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
        let enter = encode_key(
            &Key::Named(NamedKey::Enter),
            None,
            ElementState::Pressed,
            ModifiersState::empty(),
            false,
        );
        assert_eq!(enter, Some(vec![b'\r']));

        let backspace = encode_key(
            &Key::Named(NamedKey::Backspace),
            None,
            ElementState::Pressed,
            ModifiersState::empty(),
            false,
        );
        assert_eq!(backspace, Some(vec![0x7f]));
    }

    #[test]
    fn test_key_encoding_ctrl_c() {
        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        let ctrl_c = encode_key(
            &Key::Character("c".into()),
            None,
            ElementState::Pressed,
            ctrl,
            false,
        );
        assert_eq!(ctrl_c, Some(vec![0x03]));
    }

    #[test]
    fn test_key_encoding_shift_tab() {
        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        // Plain Tab -> 0x09
        let tab = encode_key(
            &Key::Named(NamedKey::Tab),
            None,
            ElementState::Pressed,
            ModifiersState::empty(),
            false,
        );
        assert_eq!(tab, Some(vec![b'\t']));

        // Shift + Tab -> \x1b[Z (Backtab)
        let backtab = encode_key(
            &Key::Named(NamedKey::Tab),
            None,
            ElementState::Pressed,
            shift,
            false,
        );
        assert_eq!(backtab, Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn test_key_encoding_modified_keys() {
        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        // Shift + Up -> \x1b[1;2A
        let shift_up = encode_key(
            &Key::Named(NamedKey::ArrowUp),
            None,
            ElementState::Pressed,
            shift,
            false,
        );
        assert_eq!(shift_up, Some(b"\x1b[1;2A".to_vec()));

        // Ctrl + Left -> \x1b[1;5D
        let ctrl_left = encode_key(
            &Key::Named(NamedKey::ArrowLeft),
            None,
            ElementState::Pressed,
            ctrl,
            false,
        );
        assert_eq!(ctrl_left, Some(b"\x1b[1;5D".to_vec()));
    }

    #[test]
    fn test_full_config_parsing() {
        let toml_str = r##"
            [font]
            family = "JetBrains Mono"
            size = 18.0
            line_height = 1.3
            fallback_families = ["Hiragino Sans GB", "PingFang TC"]

            [window]
            columns = 100
            rows = 30
            opacity = 0.90
            padding_x = 12.0
            padding_y = 12.0
            startup_mode = "maximized"
            decorations = "transparent"
            tab_bar_style = "always"

            [terminal]
            term = "xterm-direct"
            scrollback_lines = 20000
            option_as_alt = "only_left"
            bell = "sound"
            working_directory = "~"

            [terminal.env]
            COLORTERM = "truecolor"
            MY_ENV = "test123"

            [colors]
            theme = "dracula"
            foreground = "#f8f8f2"
            background = "#282a36"
            cursor = "#f1fa8c"
            selection = "#44475a"

            [colors.palette]
            black = "#21222c"
            red = "#ff5555"
            green = "#50fa7b"

            [cursor]
            shape = "underline"
            blink = true
            blink_interval_ms = 400

            [keybindings]
            "cmd+t" = "new_tab"
        "##;

        let cfg: Config = toml::from_str(toml_str).expect("Failed to parse full config");
        assert_eq!(cfg.font.family, "JetBrains Mono");
        assert_eq!(cfg.font.size, 18.0);
        assert_eq!(cfg.font.fallback_families.len(), 2);
        assert_eq!(cfg.window.columns, 100);
        assert_eq!(cfg.window.rows, 30);
        assert_eq!(cfg.window.opacity, 0.90);
        assert_eq!(cfg.window.startup_mode, "maximized");
        assert_eq!(cfg.window.decorations, "transparent");
        assert_eq!(cfg.window.tab_bar_style, "always");

        assert_eq!(cfg.terminal.term, "xterm-direct");
        assert_eq!(cfg.terminal.scrollback_lines, 20000);
        assert_eq!(cfg.terminal.option_as_alt, "only_left");
        assert_eq!(cfg.terminal.bell, "sound");
        assert_eq!(cfg.terminal.env.get("MY_ENV").unwrap(), "test123");

        assert_eq!(cfg.colors.theme, "dracula");
        assert_eq!(cfg.colors.foreground.as_deref(), Some("#f8f8f2"));
        assert_eq!(
            cfg.colors.palette.as_ref().unwrap().red.as_deref(),
            Some("#ff5555")
        );

        assert_eq!(cfg.cursor.shape, "underline");
        assert_eq!(cfg.cursor.blink_interval_ms, 400);
    }

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#ffffff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_hex_color("000000"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(
            parse_hex_color("#ff000080"),
            Some([1.0, 0.0, 0.0, 128.0 / 255.0])
        );
        assert_eq!(parse_hex_color("invalid"), None);
    }

    #[test]
    fn test_default_config_template_validity() {
        let template = default_config_template();
        let cfg: Result<Config, _> = toml::from_str(template);
        assert!(
            cfg.is_ok(),
            "default_config_template should parse as a valid Config"
        );
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
        let split_res = ws.split_active_pane(
            SplitDirection::Horizontal,
            24,
            80,
            1000,
            skterm_core::PtySpawnOptions::default(),
        );
        assert!(split_res.is_ok());

        let rects = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert_eq!(rects.len(), 2);
        assert!(rects[0].1.width > 0.0);
        assert!(rects[1].1.width > 0.0);
        assert_eq!(rects[0].1.y, 32.0);

        // Test Pane Closing
        let closed_tab = ws.close_active_pane();
        assert!(!closed_tab);
        assert_eq!(ws.tabs.len(), 2);

        // Close second tab
        let closed_second = ws.close_active_pane();
        assert!(!closed_second);
        assert_eq!(ws.tabs.len(), 1);
    }

    /// Regression test: splitting a pane that lives in the `first` subtree of an
    /// existing split used to hand the new pane a `Terminal::new_headless(..)` dummy
    /// (via a `clone_dummy()` call made to satisfy the by-value recursion), silently
    /// dropping the real PTY-backed terminal. The result was an empty, permanently
    /// dead pane. Every leaf must own a live PTY.
    #[test]
    fn test_split_in_first_subtree_keeps_real_pty() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");

        // First split: root becomes Split { first: Leaf(0), second: Leaf(1) }.
        ws.split_active_pane(
            SplitDirection::Horizontal,
            24,
            80,
            1000,
            skterm_core::PtySpawnOptions::default(),
        )
        .expect("first split failed");

        // Focus the pane in the `first` subtree -- this is what triggered the bug.
        let tab = ws.active_tab_mut().expect("no active tab");
        let leaves = tab.root.all_leaf_ids();
        assert_eq!(leaves.len(), 2);
        tab.active_pane_id = leaves[0];

        // Second split, now nested inside `first`.
        ws.split_active_pane(
            SplitDirection::Vertical,
            24,
            80,
            1000,
            skterm_core::PtySpawnOptions::default(),
        )
        .expect("second split failed");

        let tab = ws.active_tab().expect("no active tab");
        let leaves = tab.root.all_leaf_ids();
        assert_eq!(leaves.len(), 3, "expected three panes after two splits");

        for id in leaves {
            let term = tab
                .root
                .find_terminal(id)
                .unwrap_or_else(|| panic!("pane {} missing from tree", id));
            assert!(
                term.process_id().is_some(),
                "pane {} has no PTY -- a headless dummy terminal was installed",
                id
            );
        }
    }

    #[test]
    fn test_installed_font_families_are_deduplicated() {
        let families = skterm_render::atlas::installed_font_families();
        // A minimal CI container may have no fonts at all, so only assert the
        // invariant that matters: one entry per family, not one per file.
        // Must match the case-insensitive key the implementation dedups on,
        // otherwise "Foo" and "FOO" would look distinct here and the assertion
        // would pass vacuously.
        let mut cleaned: Vec<String> = families
            .iter()
            .map(|f| {
                f.chars()
                    .filter(|c| c.is_alphanumeric())
                    .collect::<String>()
                    .to_lowercase()
            })
            .collect();
        let before = cleaned.len();
        cleaned.sort();
        cleaned.dedup();
        assert_eq!(before, cleaned.len(), "family list contains duplicates");
    }

    #[test]
    fn test_pane_zoom_and_restore() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");
        ws.split_active_pane(
            SplitDirection::Horizontal,
            24,
            80,
            1000,
            skterm_core::PtySpawnOptions::default(),
        )
        .expect("Failed to split pane");

        let rects_split = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert_eq!(rects_split.len(), 2);

        // Toggle Zoom on
        let is_zoomed = ws.toggle_zoom_active_pane();
        assert!(is_zoomed);
        assert!(ws.is_active_tab_zoomed());

        let rects_zoomed = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert_eq!(rects_zoomed.len(), 1);
        assert_eq!(rects_zoomed[0].1.width, 800.0);
        assert_eq!(rects_zoomed[0].1.height, 568.0); // 600 - 32

        // Toggle Zoom off
        let unzoomed = ws.toggle_zoom_active_pane();
        assert!(!unzoomed);
        assert!(!ws.is_active_tab_zoomed());

        let rects_restored = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert_eq!(rects_restored.len(), 2);
    }

    #[test]
    fn test_splitter_drag_and_resize() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");
        ws.split_active_pane(
            SplitDirection::Horizontal,
            24,
            80,
            1000,
            skterm_core::PtySpawnOptions::default(),
        )
        .expect("Failed to split pane");

        let tab = ws.active_tab().unwrap();
        let bounds = Rect::new(0.0, 32.0, 800.0, 568.0);
        let splitter = tab.find_splitter_at(bounds, 399.0, 100.0);
        assert!(splitter.is_some());
        let s = splitter.unwrap();
        assert_eq!(s.direction, SplitDirection::Horizontal);

        // Resize via set_split_ratio
        let set_res = ws.set_split_ratio(s.split_id, 0.7);
        assert!(set_res);

        let rects = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert!(rects[0].1.width > rects[1].1.width);

        // Resize via keyboard adjust
        let adj_res = ws.adjust_active_pane_ratio(-0.1);
        assert!(adj_res);

        // Equalize panes
        ws.equalize_active_panes();
        let rects_eq = ws.compute_active_pane_rects(800.0, 600.0, 32.0);
        assert!((rects_eq[0].1.width - rects_eq[1].1.width).abs() < 2.0);
    }

    #[test]
    fn test_tab_reordering_and_rename() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");
        ws.new_tab(24, 80).expect("Failed to create tab 2");
        ws.rename_active_tab("Development".to_string());
        assert_eq!(ws.active_tab().unwrap().title, "Development");

        assert_eq!(ws.active_tab_idx, 1);
        assert!(ws.move_tab_left());
        assert_eq!(ws.active_tab_idx, 0);
        assert_eq!(ws.active_tab().unwrap().title, "Development");

        assert!(ws.move_tab_right());
        assert_eq!(ws.active_tab_idx, 1);
        assert_eq!(ws.active_tab().unwrap().title, "Development");
    }

    #[test]
    fn test_scrollbar_drag_and_scroll_on_input() {
        use skterm_core::Terminal;
        use skterm_render::ScrollbarMetrics;

        let mut term = Terminal::new_headless(10, 80, 1000);
        // Feed 50 lines so scrollback has lines
        for i in 0..50 {
            term.feed_bytes(format!("Line {}\r\n", i).as_bytes());
        }

        let scrollback_len = term.grid().active().scrollback.len();
        assert!(scrollback_len >= 40);

        // Initially at bottom (scroll_offset == 0)
        assert_eq!(term.grid().active().scroll_offset, 0);

        // Scroll up by 20 lines
        term.grid_mut().active_mut().scroll_display(20);
        assert_eq!(term.grid().active().scroll_offset, 20);

        // Verify ScrollbarMetrics
        let metrics = ScrollbarMetrics::compute(
            0.0,
            0.0,
            800.0,
            600.0,
            1.0,
            scrollback_len,
            10,
            20,
            false,
            false,
        )
        .expect("ScrollbarMetrics should be computed");

        assert!(metrics.travel > 0.0);
        assert!(metrics.thumb_y > metrics.track_y);
        assert!(metrics.thumb_y < metrics.track_y + metrics.travel);

        // Simulate dragging to bottom
        let progress_bottom = 1.0;
        let new_offset_bottom = ((1.0 - progress_bottom) * scrollback_len as f32).round() as usize;
        assert_eq!(new_offset_bottom, 0);

        // Simulate user typing text when scrolled up:
        // When user types or pastes, scroll_offset must reset to 0 (scroll back to cursor)
        assert_eq!(term.grid().active().scroll_offset, 20);
        term.grid_mut().active_mut().scroll_offset = 0;
        assert_eq!(term.grid().active().scroll_offset, 0);
    }

    #[test]
    fn test_tab_switch_modifiers_windows_linux_vs_macos() {
        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        let mut super_key = ModifiersState::empty();
        super_key.insert(ModifiersState::SUPER);

        let mut alt = ModifiersState::empty();
        alt.insert(ModifiersState::ALT);

        let mut ctrl_shift = ModifiersState::empty();
        ctrl_shift.insert(ModifiersState::CONTROL);
        ctrl_shift.insert(ModifiersState::SHIFT);

        let mut super_shift = ModifiersState::empty();
        super_shift.insert(ModifiersState::SUPER);
        super_shift.insert(ModifiersState::SHIFT);

        let mut ctrl_alt = ModifiersState::empty();
        ctrl_alt.insert(ModifiersState::CONTROL);
        ctrl_alt.insert(ModifiersState::ALT);

        let mut super_alt = ModifiersState::empty();
        super_alt.insert(ModifiersState::SUPER);
        super_alt.insert(ModifiersState::ALT);

        let mut ctrl_super = ModifiersState::empty();
        ctrl_super.insert(ModifiersState::CONTROL);
        ctrl_super.insert(ModifiersState::SUPER);

        // On Windows and Linux (is_macos = false):
        // Tab switching must trigger on CTRL (with or without Shift)
        // Must NOT trigger with Alt or Super
        assert!(is_tab_switch_modifier_for_os(ctrl, false));
        assert!(is_tab_switch_modifier_for_os(ctrl_shift, false));
        assert!(!is_tab_switch_modifier_for_os(ctrl_alt, false));
        assert!(!is_tab_switch_modifier_for_os(ctrl_super, false));
        assert!(!is_tab_switch_modifier_for_os(super_key, false));
        assert!(!is_tab_switch_modifier_for_os(alt, false));
        assert!(!is_tab_switch_modifier_for_os(
            ModifiersState::empty(),
            false
        ));

        // On macOS (is_macos = true):
        // Tab switching must trigger on CMD / Super (with or without Shift)
        // Must NOT trigger with Ctrl or Alt
        assert!(is_tab_switch_modifier_for_os(super_key, true));
        assert!(is_tab_switch_modifier_for_os(super_shift, true));
        assert!(!is_tab_switch_modifier_for_os(super_alt, true));
        assert!(!is_tab_switch_modifier_for_os(ctrl_super, true));
        assert!(!is_tab_switch_modifier_for_os(ctrl, true));
        assert!(!is_tab_switch_modifier_for_os(alt, true));
        assert!(!is_tab_switch_modifier_for_os(
            ModifiersState::empty(),
            true
        ));
    }

    #[test]
    fn test_normalize_shortcut_char_shifted_brackets() {
        use crate::input::normalize_shortcut_char;
        use winit::keyboard::{Key, KeyCode, NativeKeyCode, PhysicalKey};

        let unknown = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);

        // Shift+[ / Shift+] report "{" / "}" and must fold back to the unshifted form
        // so that Cmd/Ctrl+Shift+[ and ] switch tabs.
        assert_eq!(
            normalize_shortcut_char(&Key::Character("{".into()), unknown).as_deref(),
            Some("[")
        );
        assert_eq!(
            normalize_shortcut_char(&Key::Character("}".into()), unknown).as_deref(),
            Some("]")
        );

        // Unshifted brackets stay untouched.
        assert_eq!(
            normalize_shortcut_char(&Key::Character("[".into()), unknown).as_deref(),
            Some("[")
        );

        // Letters are lowercased, so shortcuts match regardless of Shift / Caps.
        assert_eq!(
            normalize_shortcut_char(&Key::Character("D".into()), unknown).as_deref(),
            Some("d")
        );

        // Non-ASCII layouts (e.g. QWERTZ "ü" on the US "[" position) fall back to the
        // physical key, as do control characters such as Ctrl+[ reported as ESC.
        assert_eq!(
            normalize_shortcut_char(
                &Key::Character("ü".into()),
                PhysicalKey::Code(KeyCode::BracketLeft)
            )
            .as_deref(),
            Some("[")
        );
        assert_eq!(
            normalize_shortcut_char(
                &Key::Character("\u{1b}".into()),
                PhysicalKey::Code(KeyCode::BracketRight)
            )
            .as_deref(),
            Some("]")
        );

        // Named keys without a bracket physical code yield nothing.
        assert_eq!(
            normalize_shortcut_char(&Key::Named(winit::keyboard::NamedKey::Enter), unknown),
            None
        );

        // Non-ASCII IME layouts (e.g. macOS Zhuyin/Bopomofo where physical KeyC reports "ㄏ")
        // must normalize to Some("c") so Cmd+C triggers copy instead of falling through to PTY.
        assert_eq!(
            normalize_shortcut_char(
                &Key::Character("ㄏ".into()),
                PhysicalKey::Code(KeyCode::KeyC)
            )
            .as_deref(),
            Some("c")
        );
        assert_eq!(
            normalize_shortcut_char(
                &Key::Character("ㄒ".into()),
                PhysicalKey::Code(KeyCode::KeyV)
            )
            .as_deref(),
            Some("v")
        );
    }

    #[test]
    fn test_cmd_c_never_encodes_to_pty() {
        use winit::keyboard::{Key, KeyCode, PhysicalKey};

        let mut cmd = ModifiersState::empty();
        cmd.insert(ModifiersState::SUPER);

        // Even if Cmd+C reaches encode_key (e.g. when no text is selected or in vim),
        // it must NEVER encode `b"c"` (`0x63`) to the PTY.
        let encoded_ascii = encode_key(
            &Key::Character("c".into()),
            Some("c"),
            ElementState::Pressed,
            cmd,
            false,
        );
        assert_eq!(encoded_ascii, None);

        // And under Zhuyin/CJK IME where logical_key is "ㄏ" and event.text is Some("c"),
        // it must also return None when Command (SUPER) is held.
        let encoded_ime = encode_key(
            &Key::Character("ㄏ".into()),
            Some("c"),
            ElementState::Pressed,
            cmd,
            false,
        );
        assert_eq!(encoded_ime, None);

        // Ctrl+C under Zhuyin/CJK IME (logical_key = "ㄏ", physical_key = KeyC)
        // normalizes via physical key to "c" and encodes SIGINT (0x03).
        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);
        let norm = crate::input::normalize_shortcut_char(
            &Key::Character("ㄏ".into()),
            PhysicalKey::Code(KeyCode::KeyC),
        )
        .unwrap();
        let ctrl_c_ime = encode_key(
            &Key::Character(norm.into()),
            None,
            ElementState::Pressed,
            ctrl,
            false,
        );
        assert_eq!(ctrl_c_ime, Some(vec![0x03]));
    }

    #[test]
    fn test_parse_tab_switch_digit() {
        use winit::keyboard::{Key, KeyCode, PhysicalKey};

        // 1. Logical key digits 1..=9
        for d in 1..=9 {
            let logical = Key::Character(d.to_string().into());
            let physical = PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified);
            assert_eq!(parse_tab_switch_digit(&logical, physical), Some(d));
        }

        // 0 and non-digits should return None
        let logical_0 = Key::Character("0".into());
        let dummy_physical =
            PhysicalKey::Unidentified(winit::keyboard::NativeKeyCode::Unidentified);
        assert_eq!(parse_tab_switch_digit(&logical_0, dummy_physical), None);

        let logical_a = Key::Character("a".into());
        let physical_a = PhysicalKey::Code(KeyCode::KeyA);
        assert_eq!(parse_tab_switch_digit(&logical_a, physical_a), None);

        // 2. Physical key digits (e.g. AZERTY top row where logical key might be shifted symbols, or numpad)
        let keycodes = [
            (KeyCode::Digit1, 1),
            (KeyCode::Digit2, 2),
            (KeyCode::Digit3, 3),
            (KeyCode::Digit4, 4),
            (KeyCode::Digit5, 5),
            (KeyCode::Digit6, 6),
            (KeyCode::Digit7, 7),
            (KeyCode::Digit8, 8),
            (KeyCode::Digit9, 9),
            (KeyCode::Numpad1, 1),
            (KeyCode::Numpad2, 2),
            (KeyCode::Numpad3, 3),
            (KeyCode::Numpad4, 4),
            (KeyCode::Numpad5, 5),
            (KeyCode::Numpad6, 6),
            (KeyCode::Numpad7, 7),
            (KeyCode::Numpad8, 8),
            (KeyCode::Numpad9, 9),
        ];

        let dummy_logical = Key::Character("&".into()); // e.g. unshifted top row symbol on AZERTY
        for (code, expected_digit) in keycodes {
            assert_eq!(
                parse_tab_switch_digit(&dummy_logical, PhysicalKey::Code(code)),
                Some(expected_digit)
            );
        }

        // Digit0 and Numpad0 should return None
        assert_eq!(
            parse_tab_switch_digit(&dummy_logical, PhysicalKey::Code(KeyCode::Digit0)),
            None
        );
        assert_eq!(
            parse_tab_switch_digit(&dummy_logical, PhysicalKey::Code(KeyCode::Numpad0)),
            None
        );
    }

    #[test]
    fn test_check_tab_switch_cross_platform() {
        use winit::keyboard::{Key, KeyCode, PhysicalKey};

        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        let mut cmd = ModifiersState::empty();
        cmd.insert(ModifiersState::SUPER);

        let logical_3 = Key::Character("3".into());
        let physical_3 = PhysicalKey::Code(KeyCode::Digit3);

        // Windows & Linux:
        // Ctrl+3 Pressed -> Some(3)
        assert_eq!(
            check_tab_switch_for_os(&logical_3, physical_3, ElementState::Pressed, ctrl, false),
            Some(3)
        );
        // Cmd+3 on Linux/Win -> None
        assert_eq!(
            check_tab_switch_for_os(&logical_3, physical_3, ElementState::Pressed, cmd, false),
            None
        );
        // Released key -> None
        assert_eq!(
            check_tab_switch_for_os(&logical_3, physical_3, ElementState::Released, ctrl, false),
            None
        );

        // macOS:
        // Cmd+3 Pressed -> Some(3)
        assert_eq!(
            check_tab_switch_for_os(&logical_3, physical_3, ElementState::Pressed, cmd, true),
            Some(3)
        );
        // Ctrl+3 on Mac -> None
        assert_eq!(
            check_tab_switch_for_os(&logical_3, physical_3, ElementState::Pressed, ctrl, true),
            None
        );
        // Released key -> None
        assert_eq!(
            check_tab_switch_for_os(&logical_3, physical_3, ElementState::Released, cmd, true),
            None
        );
    }

    #[test]
    fn test_workspace_select_tab_direct_index() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");
        ws.new_tab(24, 80).expect("Failed to create tab 2");
        ws.new_tab(24, 80).expect("Failed to create tab 3");
        assert_eq!(ws.tabs.len(), 3);
        assert_eq!(ws.active_tab_idx, 2);

        // Switch to Tab 1 (index 0)
        ws.select_tab(0);
        assert_eq!(ws.active_tab_idx, 0);

        // Switch to Tab 2 (index 1)
        ws.select_tab(1);
        assert_eq!(ws.active_tab_idx, 1);

        // Switch to Tab 3 (index 2)
        ws.select_tab(2);
        assert_eq!(ws.active_tab_idx, 2);

        // Attempt to switch to out-of-range Tab (e.g. index 8 for Tab 9) -> index remains unchanged
        ws.select_tab(8);
        assert_eq!(ws.active_tab_idx, 2);
    }
    /// The bug these cover: `fs::write` truncates in place and then streams the
    /// body, so the preference autosave and the geometry autosave could interleave
    /// inside `config.toml` and leave invalid TOML behind -- after which startup
    /// silently fell back to defaults and the user's font setting looked ignored.
    ///
    /// Note the payload sizes below. A ~1 KB config is usually written in a single
    /// syscall, which hides the bug; these use a config big enough that the write
    /// is necessarily split, which is what makes the tests actually fail against
    /// the old `fs::write` implementation.
    mod config_persistence {
        use super::*;
        use crate::config::write_config_atomically;

        fn temp_dir(tag: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "skterm-cfg-test-{}-{}-{}",
                tag,
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// A config whose serialized form is `filler_len` bytes larger than default.
        fn bulky_config(family: &str, filler_len: usize) -> Config {
            let mut cfg = Config::default();
            cfg.font.family = family.to_string();
            cfg.terminal
                .env
                .insert("SKTERM_TEST_FILLER".to_string(), "a".repeat(filler_len));
            cfg
        }

        #[test]
        fn concurrent_writes_never_leave_invalid_toml() {
            let dir = temp_dir("concurrent");
            let path = dir.join("config.toml");

            // Different families AND different lengths: a torn file shows up either
            // as unparseable TOML or as a blend of the two documents.
            let small = bulky_config("Menlo", 64 * 1024);
            let large = bulky_config("JetBrains Mono", 192 * 1024);
            let small_toml = toml::to_string_pretty(&small).unwrap();
            let large_toml = toml::to_string_pretty(&large).unwrap();

            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let path = path.clone();
                    let body = if i % 2 == 0 {
                        small_toml.clone()
                    } else {
                        large_toml.clone()
                    };
                    std::thread::spawn(move || {
                        for _ in 0..40 {
                            write_config_atomically(&path, &body).unwrap();
                        }
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }

            let content = std::fs::read_to_string(&path).unwrap();
            let parsed = toml::from_str::<Config>(&content)
                .expect("config.toml must always be valid TOML after concurrent writes");
            assert!(
                parsed == small || parsed == large,
                "file contains a blend of two different writes"
            );

            // The temp file must be cleaned up, not left next to the config.
            let leftovers: Vec<_> = std::fs::read_dir(&dir)
                .unwrap()
                .flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|n| n != "config.toml")
                .collect();
            assert!(leftovers.is_empty(), "stray temp files: {:?}", leftovers);

            let _ = std::fs::remove_dir_all(&dir);
        }

        #[test]
        fn reader_never_observes_a_partial_file() {
            // This is what the file watcher does: read the config whenever it
            // changes. It must never parse a half-written document.
            let dir = temp_dir("reader");
            let path = dir.join("config.toml");
            let cfg = bulky_config("Menlo", 256 * 1024);
            let body = toml::to_string_pretty(&cfg).unwrap();
            write_config_atomically(&path, &body).unwrap();

            let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let writer_done = done.clone();
            let writer_path = path.clone();
            let writer_body = body.clone();
            let writer = std::thread::spawn(move || {
                for _ in 0..100 {
                    write_config_atomically(&writer_path, &writer_body).unwrap();
                }
                writer_done.store(true, std::sync::atomic::Ordering::Release);
            });

            let mut reads = 0usize;
            while !done.load(std::sync::atomic::Ordering::Acquire) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    assert_eq!(
                        content.len(),
                        body.len(),
                        "reader saw a partially written config ({} of {} bytes)",
                        content.len(),
                        body.len()
                    );
                    reads += 1;
                }
            }
            writer.join().unwrap();
            assert!(reads > 0, "test did not actually read anything");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn test_encode_key_kitty_and_modify_other_keys() {
        use winit::keyboard::{Key, NamedKey};

        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        let mut ctrl_shift = ModifiersState::empty();
        ctrl_shift.insert(ModifiersState::CONTROL | ModifiersState::SHIFT);

        // Shift+Enter with Kitty flags = 1 -> CSI 13;2u
        let shift_enter_kitty = encode_key_with_modes(
            &Key::Named(NamedKey::Enter),
            None,
            ElementState::Pressed,
            shift,
            false,
            1,
            0,
        );
        assert_eq!(shift_enter_kitty.as_deref(), Some(b"\x1b[13;2u".as_slice()));

        // Shift+Enter with modifyOtherKeys = 2 -> CSI 27;2;13~
        let shift_enter_mok = encode_key_with_modes(
            &Key::Named(NamedKey::Enter),
            None,
            ElementState::Pressed,
            shift,
            false,
            0,
            2,
        );
        assert_eq!(
            shift_enter_mok.as_deref(),
            Some(b"\x1b[27;2;13~".as_slice())
        );

        // Ctrl+Shift+A with Kitty flags = 1 -> CSI 97;6u (base lowercase codepoint 97, mod 1+1+4=6)
        let ctrl_shift_a_kitty = encode_key_with_modes(
            &Key::Character("A".into()),
            Some("A"),
            ElementState::Pressed,
            ctrl_shift,
            false,
            1,
            0,
        );
        assert_eq!(
            ctrl_shift_a_kitty.as_deref(),
            Some(b"\x1b[97;6u".as_slice())
        );

        // Key release with Kitty flags = 3 (disambiguate + report event types) -> CSI 97;6:3u
        let ctrl_shift_a_release = encode_key_with_modes(
            &Key::Character("A".into()),
            None,
            ElementState::Released,
            ctrl_shift,
            false,
            3,
            0,
        );
        assert_eq!(
            ctrl_shift_a_release.as_deref(),
            Some(b"\x1b[97;6:3u".as_slice())
        );
    }

    #[test]
    fn test_normalize_shortcut_char_comma() {
        use crate::input::normalize_shortcut_char;
        use winit::keyboard::{Key, KeyCode, NativeKeyCode, PhysicalKey};

        let unknown = PhysicalKey::Unidentified(NativeKeyCode::Unidentified);

        // Standard comma
        assert_eq!(
            normalize_shortcut_char(&Key::Character(",".into()), unknown).as_deref(),
            Some(",")
        );

        // Shift+, reports "<"
        assert_eq!(
            normalize_shortcut_char(&Key::Character("<".into()), unknown).as_deref(),
            Some(",")
        );

        // CJK IME full-width comma
        assert_eq!(
            normalize_shortcut_char(&Key::Character("，".into()), unknown).as_deref(),
            Some(",")
        );

        // Physical comma fallback
        assert_eq!(
            normalize_shortcut_char(
                &Key::Character("，".into()),
                PhysicalKey::Code(KeyCode::Comma)
            )
            .as_deref(),
            Some(",")
        );
    }

    #[test]
    fn test_xterm_key_encoding_app_home_end_modified_fkeys_and_ctrl_symbols() {
        use winit::keyboard::{Key, NamedKey};

        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        // Home / End in normal vs app_cursor_keys (?1h) mode
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Home),
                None,
                ElementState::Pressed,
                ModifiersState::empty(),
                false
            )
            .as_deref(),
            Some(b"\x1b[H".as_slice())
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Home),
                None,
                ElementState::Pressed,
                ModifiersState::empty(),
                true
            )
            .as_deref(),
            Some(b"\x1bOH".as_slice())
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::End),
                None,
                ElementState::Pressed,
                ModifiersState::empty(),
                true
            )
            .as_deref(),
            Some(b"\x1bOF".as_slice())
        );

        // Modified F1..F12: Shift+F1 -> \x1b[1;2P, Ctrl+F5 -> \x1b[15;5~, Shift+F12 -> \x1b[24;2~
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::F1),
                None,
                ElementState::Pressed,
                shift,
                false
            )
            .as_deref(),
            Some(b"\x1b[1;2P".as_slice())
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::F5),
                None,
                ElementState::Pressed,
                ctrl,
                false
            )
            .as_deref(),
            Some(b"\x1b[15;5~".as_slice())
        );
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::F12),
                None,
                ElementState::Pressed,
                shift,
                false
            )
            .as_deref(),
            Some(b"\x1b[24;2~".as_slice())
        );

        // Ctrl+Space (NamedKey::Space or Character(" ")) -> 0x00 (NUL)
        assert_eq!(
            encode_key(
                &Key::Named(NamedKey::Space),
                Some(" "),
                ElementState::Pressed,
                ctrl,
                false
            ),
            Some(vec![0x00])
        );
        // Ctrl+/ -> 0x1f (US / Undo in emacs/zsh/bash)
        assert_eq!(
            encode_key(
                &Key::Character("/".into()),
                None,
                ElementState::Pressed,
                ctrl,
                false
            ),
            Some(vec![0x1f])
        );
        // Ctrl+\ -> 0x1c (SIGQUIT)
        assert_eq!(
            encode_key(
                &Key::Character("\\".into()),
                None,
                ElementState::Pressed,
                ctrl,
                false
            ),
            Some(vec![0x1c])
        );
    }

    #[test]
    fn test_build_and_hit_test_top_dropdown_menu() {
        let (menu, actions) = build_top_dropdown_menu(1.0, 100.0, 36.0, None);
        assert!(!menu.items.is_empty());
        assert_eq!(menu.items.len(), actions.len());

        // First item is Preferences
        assert_eq!(actions[0], Some(TopMenuAction::OpenPreferences));
        assert!(menu.items[0].label.contains("偏好設定"));

        // Second item is separator
        assert!(menu.items[1].is_separator);
        assert_eq!(actions[1], None);

        // Hit test inside the menu bounds
        let row_h = 24.0;
        let item0_hit = hit_test_dropdown_menu(&menu, 1.0, 120.0, 36.0 + 3.0 + 5.0);
        assert_eq!(item0_hit, Some(0));

        // Hit test on separator should return None
        let sep_hit = hit_test_dropdown_menu(&menu, 1.0, 120.0, 36.0 + 3.0 + row_h + 2.0);
        assert_eq!(sep_hit, None);

        // Hit test outside menu width
        let outside_hit = hit_test_dropdown_menu(&menu, 1.0, 50.0, 45.0);
        assert_eq!(outside_hit, None);
    }
}
