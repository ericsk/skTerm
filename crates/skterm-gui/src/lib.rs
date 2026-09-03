//! `skterm-gui`: Application window loop, input handling, config, tabs, and split panes multiplexer.

pub mod app_window;
pub mod config;
pub mod input;
pub mod layout;
pub mod preferences_gui;
pub mod preferences_window;

pub use app_window::App;
pub use config::{default_config_template, parse_hex_color, Config, ConfigManager, Theme};
pub use input::{encode_key, encode_key_event};
pub use layout::{PaneId, PaneNode, Rect, SplitDirection, SplitterBar, Tab, TabId, Workspace};
pub use preferences_gui::{PreferencesGui, PreferencesTab};
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
    fn test_key_encoding_shift_tab() {
        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        // Plain Tab -> 0x09
        let tab = encode_key(&Key::Named(NamedKey::Tab), None, ElementState::Pressed, ModifiersState::empty(), false);
        assert_eq!(tab, Some(vec![b'\t']));

        // Shift + Tab -> \x1b[Z (Backtab)
        let backtab = encode_key(&Key::Named(NamedKey::Tab), None, ElementState::Pressed, shift, false);
        assert_eq!(backtab, Some(b"\x1b[Z".to_vec()));
    }

    #[test]
    fn test_key_encoding_modified_keys() {
        let mut shift = ModifiersState::empty();
        shift.insert(ModifiersState::SHIFT);

        let mut ctrl = ModifiersState::empty();
        ctrl.insert(ModifiersState::CONTROL);

        // Shift + Up -> \x1b[1;2A
        let shift_up = encode_key(&Key::Named(NamedKey::ArrowUp), None, ElementState::Pressed, shift, false);
        assert_eq!(shift_up, Some(b"\x1b[1;2A".to_vec()));

        // Ctrl + Left -> \x1b[1;5D
        let ctrl_left = encode_key(&Key::Named(NamedKey::ArrowLeft), None, ElementState::Pressed, ctrl, false);
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
        assert_eq!(cfg.colors.palette.as_ref().unwrap().red.as_deref(), Some("#ff5555"));

        assert_eq!(cfg.cursor.shape, "underline");
        assert_eq!(cfg.cursor.blink_interval_ms, 400);
    }

    #[test]
    fn test_parse_hex_color() {
        assert_eq!(parse_hex_color("#ffffff"), Some([1.0, 1.0, 1.0, 1.0]));
        assert_eq!(parse_hex_color("000000"), Some([0.0, 0.0, 0.0, 1.0]));
        assert_eq!(parse_hex_color("#ff000080"), Some([1.0, 0.0, 0.0, 128.0 / 255.0]));
        assert_eq!(parse_hex_color("invalid"), None);
    }

    #[test]
    fn test_default_config_template_validity() {
        let template = default_config_template();
        let cfg: Result<Config, _> = toml::from_str(template);
        assert!(cfg.is_ok(), "default_config_template should parse as a valid Config");
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
        let split_res = ws.split_active_pane(SplitDirection::Horizontal, 24, 80, 1000, skterm_core::PtySpawnOptions::default());
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

    #[test]
    fn test_preferences_gui_state() {
        let mut gui = PreferencesGui::new();
        assert!(!gui.is_open);

        let default_cfg = Config::default();
        gui.toggle(&default_cfg);
        assert!(gui.is_open);
        assert_eq!(gui.font_family, "Menlo");
        assert_eq!(gui.columns, 80);
        assert_eq!(gui.rows, 24);

        gui.font_family = "Fira Code".to_string();
        gui.font_size = 20.0;
        gui.theme = "tokyo-night".to_string();
        gui.columns = 120;

        let modified_cfg = gui.to_config(&default_cfg);
        assert_eq!(modified_cfg.font.family, "Fira Code");
        assert_eq!(modified_cfg.font.size, 20.0);
        assert_eq!(modified_cfg.colors.theme, "tokyo-night");
        assert_eq!(modified_cfg.window.columns, 120);

        gui.toggle(&default_cfg);
        assert!(!gui.is_open);
    }

    #[test]
    fn test_preferences_window_font_scan() {
        let fonts = preferences_window::scan_available_monospace_fonts();
        // Even if empty on minimal CI container, it should safely return a Vec<String>
        assert!(fonts.len() <= 30);
    }

    #[test]
    fn test_pane_zoom_and_restore() {
        let mut ws = Workspace::new(24, 80).expect("Failed to create workspace");
        ws.split_active_pane(SplitDirection::Horizontal, 24, 80, 1000, skterm_core::PtySpawnOptions::default())
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
        ws.split_active_pane(SplitDirection::Horizontal, 24, 80, 1000, skterm_core::PtySpawnOptions::default())
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
}
