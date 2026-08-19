//! Main Application Event Loop, Window Handler, Tabs, Split Panes, Clipboard, CJK IME, and Native Preferences Window.

use crate::config::{parse_hex_color, Config, ConfigManager, Theme};
use crate::input::encode_key_event;
use crate::layout::{PaneId, SplitDirection, Workspace};
use crate::preferences_window::PreferencesWindow;
use arboard::Clipboard;
use skterm_core::{PtySpawnOptions, Selection, SelectionMode};
use skterm_render::{PaneRenderItem, RenderTheme, TabItem, TermRenderer};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, Position, Size};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Fullscreen, Window, WindowAttributes, WindowId};

/// skTerm GUI Application instance.
pub struct App {
    pub config_manager: ConfigManager,
    pub preferences_window: Option<PreferencesWindow>,
    pub window: Option<Arc<Window>>,
    pub renderer: Option<TermRenderer>,
    pub workspace: Option<Workspace>,
    pub clipboard: Option<Arc<Mutex<Clipboard>>>,
    pub modifiers: ModifiersState,
    pub mouse_pos: (f32, f32),
    pub is_mouse_down: bool,
    pub last_click_time: Instant,
    pub click_count: usize,
    pub pending_geometry_save: bool,
    pub last_resize_time: Option<Instant>,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let config_manager = ConfigManager::load_or_create();

        let clipboard = match Clipboard::new() {
            Ok(cb) => Some(Arc::new(Mutex::new(cb))),
            Err(e) => {
                warn!("Failed to initialize clipboard: {:?}", e);
                None
            }
        };

        Self {
            config_manager,
            preferences_window: None,
            window: None,
            renderer: None,
            workspace: None,
            clipboard,
            modifiers: ModifiersState::default(),
            mouse_pos: (0.0, 0.0),
            is_mouse_down: false,
            last_click_time: Instant::now(),
            click_count: 0,
            pending_geometry_save: false,
            last_resize_time: None,
        }
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
        event_loop.run_app(&mut self)?;
        Ok(())
    }

    pub fn open_preferences_window(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pref) = &self.preferences_window {
            pref.window.focus_window();
            pref.window.request_redraw();
            return;
        }

        match pollster::block_on(PreferencesWindow::create(event_loop, &self.config_manager.current_config)) {
            Ok(pref) => {
                info!("Opened dedicated Preferences Window (ID: {:?})", pref.id());
                self.preferences_window = Some(pref);
            }
            Err(e) => {
                error!("Failed to open Preferences Window: {:?}", e);
            }
        }
    }

    pub fn apply_config_live(&mut self, new_cfg: &Config) {
        info!("Applying live configuration update to terminal...");
        let old_font = self.config_manager.current_config.font.clone();
        let font_changed = old_font.family != new_cfg.font.family
            || (old_font.size - new_cfg.font.size).abs() > 0.01
            || old_font.fallback_families != new_cfg.font.fallback_families;

        self.config_manager.current_config = new_cfg.clone();

        // 1. Update font (only if changed), padding, and opacity
        if let Some(renderer) = &mut self.renderer {
            if font_changed {
                info!("Font settings changed, rebuilding WGPU glyph atlas...");
                let _ = renderer.update_font(
                    &new_cfg.font.family,
                    new_cfg.font.size,
                    &new_cfg.font.fallback_families,
                );
            }
            renderer.update_padding(new_cfg.window.padding_x, new_cfg.window.padding_y);
            renderer.update_opacity(new_cfg.window.opacity);
        }

        // 2. Update theme
        self.apply_theme_to_renderer();

        // 3. Update cursor shape
        let cursor_shape = match new_cfg.cursor.shape.as_str() {
            "beam" => skterm_core::CursorShape::Beam,
            "underline" => skterm_core::CursorShape::Underline,
            _ => skterm_core::CursorShape::Block,
        };
        if let Some(workspace) = &mut self.workspace {
            workspace.update_cursor_shape(cursor_shape);
        }

        // 4. Sync workspace pane sizes to updated cell size & padding
        self.sync_all_pane_sizes();

        // 5. Request redraw on main window
        if let Some(w) = &self.window {
            w.request_redraw();
        }

        // 6. Auto-save to config.toml in background thread
        if let Ok(toml_str) = toml::to_string_pretty(new_cfg) {
            let path = self.config_manager.path.clone();
            std::thread::spawn(move || {
                let _ = std::fs::write(&path, toml_str);
            });
        }
    }

    fn build_pty_options(&self) -> PtySpawnOptions {
        let term_cfg = &self.config_manager.current_config.terminal;
        let (shell, args) = if let Some(s) = &term_cfg.shell {
            (Some(s.program.clone()), s.args.clone())
        } else {
            (None, Vec::new())
        };

        PtySpawnOptions {
            shell,
            args,
            cwd: term_cfg.working_directory.clone(),
            term: Some(term_cfg.term.clone()),
            env: term_cfg.env.clone(),
        }
    }

    pub fn apply_theme_to_renderer(&mut self) {
        let config = &self.config_manager.current_config;
        let preset = Theme::get(&config.colors.theme);
        let mut render_theme = RenderTheme {
            default_fg: preset.foreground,
            default_bg: preset.background,
            cursor_color: preset.cursor,
            selection_color: preset.selection,
            tab_bar_bg: [preset.background[0] * 0.85, preset.background[1] * 0.85, preset.background[2] * 0.85, 1.0],
            tab_active_bg: preset.background,
            tab_active_fg: preset.foreground,
            tab_inactive_fg: [preset.foreground[0] * 0.6, preset.foreground[1] * 0.6, preset.foreground[2] * 0.6, 1.0],
            border_color: [0.3, 0.3, 0.35, 1.0],
            focused_border_color: [0.5, 0.7, 1.0, 1.0],
            ansi_palette: Some(preset.palette),
        };

        if let Some(fg_hex) = &config.colors.foreground {
            if let Some(c) = parse_hex_color(fg_hex) {
                render_theme.default_fg = c;
                render_theme.tab_active_fg = c;
            }
        }
        if let Some(bg_hex) = &config.colors.background {
            if let Some(c) = parse_hex_color(bg_hex) {
                render_theme.default_bg = c;
                render_theme.tab_active_bg = c;
            }
        }
        if let Some(cur_hex) = &config.colors.cursor {
            if let Some(c) = parse_hex_color(cur_hex) {
                render_theme.cursor_color = c;
            }
        }
        if let Some(sel_hex) = &config.colors.selection {
            if let Some(c) = parse_hex_color(sel_hex) {
                render_theme.selection_color = c;
            }
        }

        if let Some(palette_cfg) = &config.colors.palette {
            let mut pal = preset.palette;

            if let Some(c) = palette_cfg.black.as_ref().and_then(|h| parse_hex_color(h)) { pal[0] = c; }
            if let Some(c) = palette_cfg.red.as_ref().and_then(|h| parse_hex_color(h)) { pal[1] = c; }
            if let Some(c) = palette_cfg.green.as_ref().and_then(|h| parse_hex_color(h)) { pal[2] = c; }
            if let Some(c) = palette_cfg.yellow.as_ref().and_then(|h| parse_hex_color(h)) { pal[3] = c; }
            if let Some(c) = palette_cfg.blue.as_ref().and_then(|h| parse_hex_color(h)) { pal[4] = c; }
            if let Some(c) = palette_cfg.magenta.as_ref().and_then(|h| parse_hex_color(h)) { pal[5] = c; }
            if let Some(c) = palette_cfg.cyan.as_ref().and_then(|h| parse_hex_color(h)) { pal[6] = c; }
            if let Some(c) = palette_cfg.white.as_ref().and_then(|h| parse_hex_color(h)) { pal[7] = c; }
            if let Some(c) = palette_cfg.bright_black.as_ref().and_then(|h| parse_hex_color(h)) { pal[8] = c; }
            if let Some(c) = palette_cfg.bright_red.as_ref().and_then(|h| parse_hex_color(h)) { pal[9] = c; }
            if let Some(c) = palette_cfg.bright_green.as_ref().and_then(|h| parse_hex_color(h)) { pal[10] = c; }
            if let Some(c) = palette_cfg.bright_yellow.as_ref().and_then(|h| parse_hex_color(h)) { pal[11] = c; }
            if let Some(c) = palette_cfg.bright_blue.as_ref().and_then(|h| parse_hex_color(h)) { pal[12] = c; }
            if let Some(c) = palette_cfg.bright_magenta.as_ref().and_then(|h| parse_hex_color(h)) { pal[13] = c; }
            if let Some(c) = palette_cfg.bright_cyan.as_ref().and_then(|h| parse_hex_color(h)) { pal[14] = c; }
            if let Some(c) = palette_cfg.bright_white.as_ref().and_then(|h| parse_hex_color(h)) { pal[15] = c; }

            render_theme.ansi_palette = Some(pal);
        }

        if let Some(renderer) = &mut self.renderer {
            renderer.set_theme(render_theme);
        }
    }

    pub fn sync_all_pane_sizes(&mut self) {
        let tab_bar_h = self.get_tab_bar_height();
        if let (Some(renderer), Some(workspace)) = (&self.renderer, &mut self.workspace) {
            workspace.sync_pane_sizes(
                renderer.config.width as f32,
                renderer.config.height as f32,
                renderer.cell_width(),
                renderer.cell_height(),
                renderer.padding_x,
                renderer.padding_y,
                tab_bar_h,
            );
        }
    }

    pub fn get_tab_bar_height(&self) -> f32 {
        let tab_bar_style = self.config_manager.current_config.window.tab_bar_style.as_str();
        let tab_count = self.workspace.as_ref().map(|w| w.tabs.len()).unwrap_or(0);
        let scale = self.renderer.as_ref().map(|r| r.scale_factor).unwrap_or(1.0);

        match tab_bar_style {
            "never" => 0.0,
            "always" => 28.0 * scale,
            "auto" | _ => {
                if tab_count > 1 {
                    28.0 * scale
                } else {
                    0.0
                }
            }
        }
    }

    fn update_ime_position(&self) {
        if let (Some(window), Some(renderer), Some(workspace)) = (&self.window, &self.renderer, &self.workspace) {
            if let Some(active_tab) = workspace.active_tab() {
                if let Some(terminal) = active_tab.active_terminal() {
                    let tab_bar_h = self.get_tab_bar_height();
                    let pane_rects = workspace.compute_active_pane_rects(
                        renderer.config.width as f32,
                        renderer.config.height as f32,
                        tab_bar_h,
                    );

                    for (id, rect) in &pane_rects {
                        if *id == active_tab.active_pane_id {
                            let cursor = &terminal.grid().active().cursor;
                            let cell_w = renderer.cell_width();
                            let cell_h = renderer.cell_height();

                            let cursor_px = rect.x + renderer.padding_x + (cursor.col as f32 * cell_w);
                            let cursor_py = rect.y + renderer.padding_y + (cursor.row as f32 * cell_h);

                            let scale = renderer.scale_factor as f64;
                            let logical_pos = LogicalPosition::new(cursor_px as f64 / scale, cursor_py as f64 / scale);
                            let logical_size = LogicalSize::new(cell_w as f64 / scale, cell_h as f64 / scale);

                            window.set_ime_cursor_area(
                                Position::Logical(logical_pos),
                                Size::Logical(logical_size),
                            );
                            break;
                        }
                    }
                }
            }
        }
    }

    fn copy_to_clipboard(&self) {
        if let Some(workspace) = &self.workspace {
            if let Some(active_tab) = workspace.active_tab() {
                if let Some(terminal) = active_tab.active_terminal() {
                    let text = terminal.grid().active().extract_selected_text();
                    if !text.is_empty() {
                        if let Some(cb) = &self.clipboard {
                            if let Ok(mut clipboard) = cb.lock() {
                                let _ = clipboard.set_text(text);
                                info!("Copied selected text to system clipboard");
                            }
                        }
                    }
                }
            }
        }
    }

    fn paste_from_clipboard(&self) {
        if let Some(cb) = &self.clipboard {
            if let Ok(mut clipboard) = cb.lock() {
                if let Ok(text) = clipboard.get_text() {
                    if !text.is_empty() {
                        if let Some(workspace) = &self.workspace {
                            if let Some(active_tab) = workspace.active_tab() {
                                if let Some(terminal) = active_tab.active_terminal() {
                                    let bracketed = terminal.state.bracketed_paste;
                                    let payload = if bracketed {
                                        format!("\x1b[200~{}\x1b[201~", text)
                                    } else {
                                        text
                                    };
                                    let _ = terminal.write_to_pty(payload.as_bytes());
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    fn get_pane_and_cell_at(&self, mouse_x: f32, mouse_y: f32) -> Option<(PaneId, usize, usize)> {
        let (renderer, workspace) = (self.renderer.as_ref()?, self.workspace.as_ref()?);
        let tab_bar_h = self.get_tab_bar_height();
        let pane_rects = workspace.compute_active_pane_rects(
            renderer.config.width as f32,
            renderer.config.height as f32,
            tab_bar_h,
        );

        for (id, rect) in pane_rects {
            if rect.contains(mouse_x, mouse_y) {
                let cell_w = renderer.cell_width();
                let cell_h = renderer.cell_height();
                let pad_x = renderer.padding_x;
                let pad_y = renderer.padding_y;

                let rel_x = (mouse_x - rect.x - pad_x).max(0.0);
                let rel_y = (mouse_y - rect.y - pad_y).max(0.0);

                let col = (rel_x / cell_w).floor() as usize;
                let row = (rel_y / cell_h).floor() as usize;

                return Some((id, row, col));
            }
        }
        None
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let init_cols = self.config_manager.current_config.window.columns as f32;
        let init_rows = self.config_manager.current_config.window.rows as f32;
        let font_size_pt = self.config_manager.current_config.font.size;
        let pad_x = self.config_manager.current_config.window.padding_x;
        let pad_y = self.config_manager.current_config.window.padding_y;

        let mut window_attrs = WindowAttributes::default()
            .with_title("skTerm")
            .with_inner_size(LogicalSize::new(800.0, 500.0));

        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attrs = window_attrs.with_title_hidden(true).with_titlebar_transparent(true);
        }

        match self.config_manager.current_config.window.decorations.as_str() {
            "none" => { window_attrs = window_attrs.with_decorations(false); }
            "transparent" => { window_attrs = window_attrs.with_transparent(true); }
            _ => {}
        }

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => {
                w.set_ime_allowed(true);
                match self.config_manager.current_config.window.startup_mode.as_str() {
                    "maximized" => w.set_maximized(true),
                    "fullscreen" => w.set_fullscreen(Some(Fullscreen::Borderless(None))),
                    _ => {}
                }
                Arc::new(w)
            }
            Err(e) => {
                error!("Failed to create window: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        let font_cfg = &self.config_manager.current_config.font;
        let renderer = match pollster::block_on(TermRenderer::new_with_font(
            window.clone(),
            font_size_pt,
            &font_cfg.family,
            &font_cfg.fallback_families,
        )) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to initialize wgpu renderer: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        let scale = renderer.scale_factor;
        let cell_w_logical = renderer.cell_width() / scale;
        let cell_h_logical = renderer.cell_height() / scale;
        let target_logical_w = (init_cols * cell_w_logical + pad_x * 2.0).ceil();
        let target_logical_h = (init_rows * cell_h_logical + pad_y * 2.0).ceil();
        let _ = window.request_inner_size(LogicalSize::new(target_logical_w as f64, target_logical_h as f64));

        let pty_opts = self.build_pty_options();
        let scrollback = self.config_manager.current_config.terminal.scrollback_lines;

        let workspace = match Workspace::new_with_options(
            init_rows as usize,
            init_cols as usize,
            scrollback,
            pty_opts,
        ) {
            Ok(ws) => ws,
            Err(e) => {
                error!("Failed to initialize workspace: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.workspace = Some(workspace);

        self.apply_theme_to_renderer();
        self.sync_all_pane_sizes();
        info!("skTerm Main Window, Renderer, and Workspace initialized successfully!");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        // 1. Route to Preferences Window if event belongs to it
        if let Some(pref) = &mut self.preferences_window {
            if pref.id() == window_id {
                match event {
                    WindowEvent::CloseRequested => {
                        info!("Preferences Window closed.");
                        self.preferences_window = None;
                        return;
                    }
                    WindowEvent::Resized(new_size) => {
                        pref.resize(new_size);
                        pref.window.request_redraw();
                        return;
                    }
                    WindowEvent::RedrawRequested => {
                        let maybe_new_cfg = pref.render_frame(&self.config_manager);
                        if let Some(new_cfg) = maybe_new_cfg {
                            self.apply_config_live(&new_cfg);
                        }
                        return;
                    }
                    _ => {
                        let res = pref.egui_state.on_window_event(&pref.window, &event);
                        if res.repaint {
                            pref.window.request_redraw();
                        }
                        if let Some(new_cfg) = pref.check_changed(&self.config_manager) {
                            self.apply_config_live(&new_cfg);
                        }
                        return;
                    }
                }
            }
        }

        // 2. Handle Main Terminal Window Events
        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting application.");
                if self.pending_geometry_save {
                    if let Ok(toml_str) = toml::to_string_pretty(&self.config_manager.current_config) {
                        let _ = std::fs::write(&self.config_manager.path, toml_str);
                    }
                }
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods.state();
            }

            WindowEvent::Ime(ime_event) => {
                match ime_event {
                    Ime::Commit(text) => {
                        if let Some(workspace) = &self.workspace {
                            if let Some(tab) = workspace.active_tab() {
                                if let Some(terminal) = tab.active_terminal() {
                                    let _ = terminal.write_to_pty(text.as_bytes());
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                #[cfg(target_os = "macos")]
                let is_cmd = self.modifiers.super_key();

                #[cfg(not(target_os = "macos"))]
                let is_cmd = self.modifiers.control_key() && self.modifiers.shift_key();

                let is_shift = self.modifiers.shift_key();

                // Handle Global skTerm Shortcuts
                if is_cmd {
                    if let Key::Character(ch) = &event.logical_key {
                        let lower = ch.to_lowercase();
                        match lower.as_str() {
                            "," => {
                                // Cmd+, / Ctrl+Shift+, -> Open Dedicated Preferences Window
                                info!("Opening Native Preferences Window...");
                                self.open_preferences_window(event_loop);
                                return;
                            }
                            "=" | "+" => {
                                // Zoom In Font Size
                                if let Some(renderer) = &mut self.renderer {
                                    let new_pt = renderer.font_size_pt + 1.0;
                                    let _ = renderer.update_font_size(new_pt);
                                }
                                self.apply_theme_to_renderer();
                                self.sync_all_pane_sizes();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            "-" | "_" => {
                                // Zoom Out Font Size
                                if let Some(renderer) = &mut self.renderer {
                                    let new_pt = (renderer.font_size_pt - 1.0).max(8.0);
                                    let _ = renderer.update_font_size(new_pt);
                                }
                                self.apply_theme_to_renderer();
                                self.sync_all_pane_sizes();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            "0" => {
                                // Reset Font Size
                                let def_pt = self.config_manager.current_config.font.size;
                                if let Some(renderer) = &mut self.renderer {
                                    let _ = renderer.update_font_size(def_pt);
                                }
                                self.apply_theme_to_renderer();
                                self.sync_all_pane_sizes();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            "t" => {
                                // New Tab: Cmd+T
                                let pty_opts = self.build_pty_options();
                                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
                                if let Some(workspace) = &mut self.workspace {
                                    let _ = workspace.new_tab_with_options(24, 80, scrollback, pty_opts);
                                }
                                self.sync_all_pane_sizes();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            "w" => {
                                // Close Tab/Pane: Cmd+W
                                if let Some(workspace) = &mut self.workspace {
                                    let all_closed = workspace.close_active_pane();
                                    if all_closed {
                                        info!("All tabs closed via shortcut, exiting.");
                                        event_loop.exit();
                                        return;
                                    }
                                }
                                self.sync_all_pane_sizes();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            "d" => {
                                // Split Pane: Cmd+D (Left/Right) or Cmd+Shift+D (Top/Bottom)
                                let dir = if is_shift { SplitDirection::Vertical } else { SplitDirection::Horizontal };
                                let pty_opts = self.build_pty_options();
                                let scrollback = self.config_manager.current_config.terminal.scrollback_lines;
                                if let Some(workspace) = &mut self.workspace {
                                    let _ = workspace.split_active_pane(dir, 24, 80, scrollback, pty_opts);
                                }
                                self.sync_all_pane_sizes();
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                                return;
                            }
                            "c" => {
                                self.copy_to_clipboard();
                                return;
                            }
                            "v" => {
                                self.paste_from_clipboard();
                                return;
                            }
                            "]" => {
                                // Next Tab / Next Pane
                                if let Some(workspace) = &mut self.workspace {
                                    if is_shift {
                                        workspace.next_tab();
                                    } else {
                                        if let Some(tab) = workspace.active_tab_mut() {
                                            tab.focus_next();
                                        }
                                    }
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                }
                                return;
                            }
                            "[" => {
                                // Prev Tab / Prev Pane
                                if let Some(workspace) = &mut self.workspace {
                                    if is_shift {
                                        workspace.prev_tab();
                                    } else {
                                        if let Some(tab) = workspace.active_tab_mut() {
                                            tab.focus_prev();
                                        }
                                    }
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                }
                                return;
                            }
                            _ => {
                                // Tab Direct Jump (Cmd+1..9)
                                if let Ok(digit) = lower.parse::<usize>() {
                                    if digit >= 1 && digit <= 9 {
                                        if let Some(workspace) = &mut self.workspace {
                                            workspace.select_tab(digit - 1);
                                            if let Some(window) = &self.window {
                                                window.request_redraw();
                                            }
                                        }
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }

                // Send terminal bytes to active PTY
                if let Some(workspace) = &self.workspace {
                    if let Some(active_tab) = workspace.active_tab() {
                        if let Some(terminal) = active_tab.active_terminal() {
                            let app_cursor = terminal.state.application_cursor_keys;
                            if let Some(bytes) = encode_key_event(&event, self.modifiers, app_cursor) {
                                let _ = terminal.write_to_pty(&bytes);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                        }
                    }
                }
            }

            WindowEvent::Resized(physical_size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(physical_size.width, physical_size.height);
                    self.sync_all_pane_sizes();
                    if let Some(workspace) = &self.workspace {
                        if let Some(tab) = workspace.active_tab() {
                            if let Some(term) = tab.active_terminal() {
                                let new_cols = term.grid().active().cols as u16;
                                let new_rows = term.grid().active().rows as u16;
                                let cfg = &mut self.config_manager.current_config;
                                if cfg.window.columns != new_cols || cfg.window.rows != new_rows {
                                    cfg.window.columns = new_cols;
                                    cfg.window.rows = new_rows;
                                    self.pending_geometry_save = true;
                                    self.last_resize_time = Some(Instant::now());
                                }
                            }
                        }
                    }
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(renderer) = &mut self.renderer {
                    let _ = renderer.update_scale_factor(scale_factor as f32);
                    self.sync_all_pane_sizes();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);
                let (mx, my) = self.mouse_pos;

                // Dragging Selection
                if self.is_mouse_down {
                    if let Some((_pane_id, row, col)) = self.get_pane_and_cell_at(mx, my) {
                        if let Some(workspace) = &mut self.workspace {
                            if let Some(tab) = workspace.active_tab_mut() {
                                if let Some(terminal) = tab.active_terminal_mut() {
                                    if let Some(sel) = &mut terminal.grid_mut().active_mut().selection {
                                        sel.end = (row, col);
                                        if let Some(window) = &self.window {
                                            window.request_redraw();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                let (mx, my) = self.mouse_pos;
                let tab_bar_h = self.get_tab_bar_height();

                if button == MouseButton::Left {
                    if state == ElementState::Pressed {
                        self.is_mouse_down = true;

                        // Click on Tab Bar
                        if tab_bar_h > 0.0 && my <= tab_bar_h {
                            if let Some(renderer) = &self.renderer {
                                let tab_w = 140.0 * renderer.scale_factor;
                                let clicked_idx = (mx / tab_w).floor() as usize;
                                if let Some(workspace) = &mut self.workspace {
                                    if clicked_idx < workspace.tabs.len() {
                                        workspace.select_tab(clicked_idx);
                                        if let Some(window) = &self.window {
                                            window.request_redraw();
                                        }
                                        return;
                                    }
                                }
                            }
                        }

                        // Click inside Terminal Pane
                        if let Some((pane_id, row, col)) = self.get_pane_and_cell_at(mx, my) {
                            if let Some(workspace) = &mut self.workspace {
                                if let Some(tab) = workspace.active_tab_mut() {
                                    tab.active_pane_id = pane_id;

                                    let now = Instant::now();
                                    if now.duration_since(self.last_click_time) < Duration::from_millis(400) {
                                        self.click_count += 1;
                                    } else {
                                        self.click_count = 1;
                                    }
                                    self.last_click_time = now;

                                    let mode = match self.click_count {
                                        2 => SelectionMode::Word,
                                        3 => SelectionMode::Line,
                                        _ => SelectionMode::Simple,
                                    };

                                    if let Some(terminal) = tab.active_terminal_mut() {
                                        terminal.grid_mut().active_mut().selection = Some(Selection::new((row, col), (row, col), mode));
                                    }
                                    if let Some(window) = &self.window {
                                        window.request_redraw();
                                    }
                                }
                            }
                        }
                    } else {
                        self.is_mouse_down = false;
                    }
                } else if button == MouseButton::Right && state == ElementState::Pressed {
                    self.paste_from_clipboard();
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y as i32,
                    MouseScrollDelta::PixelDelta(pos) => {
                        let scale = self.renderer.as_ref().map(|r| r.cell_height()).unwrap_or(20.0);
                        (pos.y / scale as f64) as i32
                    }
                };

                if lines != 0 {
                    if let Some(workspace) = &mut self.workspace {
                        if let Some(tab) = workspace.active_tab_mut() {
                            if let Some(terminal) = tab.active_terminal_mut() {
                                terminal.grid_mut().active_mut().scroll_display(lines as isize);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let tab_bar_h = self.get_tab_bar_height();
                if let (Some(renderer), Some(workspace), Some(_window)) = (&mut self.renderer, &mut self.workspace, &self.window) {
                    let _ = workspace.poll_all_ptys();

                    let tabs: Vec<TabItem> = workspace
                        .tabs
                        .iter()
                        .enumerate()
                        .map(|(i, t)| TabItem {
                            title: t.title.clone(),
                            is_active: i == workspace.active_tab_idx,
                        })
                        .collect();

                    let pane_rects = workspace.compute_active_pane_rects(
                        renderer.config.width as f32,
                        renderer.config.height as f32,
                        tab_bar_h,
                    );

                    let mut render_panes = Vec::new();
                    if let Some(active_tab) = workspace.active_tab() {
                        for (id, rect) in &pane_rects {
                            if let Some(t) = active_tab.root.find_terminal(*id) {
                                render_panes.push(PaneRenderItem {
                                    grid: t.grid(),
                                    rect_x: rect.x,
                                    rect_y: rect.y,
                                    rect_w: rect.width,
                                    rect_h: rect.height,
                                    is_focused: *id == active_tab.active_pane_id,
                                });
                            }
                        }
                    }

                    if let Err(e) = renderer.render_workspace(&tabs, &render_panes, tab_bar_h) {
                        error!("Render error: {:?}", e);
                    }

                    if let Some(active_tab) = workspace.active_tab_mut() {
                        for (id, _) in &pane_rects {
                            if let Some(t) = active_tab.root.find_terminal_mut(*id) {
                                t.grid_mut().clear_dirty();
                            }
                        }
                    }

                    self.update_ime_position();
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let old_cfg = self.config_manager.current_config.clone();
        if let Some(reloaded_cfg) = self.config_manager.check_reload() {
            if old_cfg != *reloaded_cfg {
                info!("Applying hot-reloaded configuration to running app...");
                let font_changed = old_cfg.font.family != reloaded_cfg.font.family
                    || (old_cfg.font.size - reloaded_cfg.font.size).abs() > 0.01
                    || old_cfg.font.fallback_families != reloaded_cfg.font.fallback_families;

                if font_changed {
                    if let Some(renderer) = &mut self.renderer {
                        let _ = renderer.update_font(
                            &reloaded_cfg.font.family,
                            reloaded_cfg.font.size,
                            &reloaded_cfg.font.fallback_families,
                        );
                    }
                }
                self.apply_theme_to_renderer();
                self.sync_all_pane_sizes();
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        }

        let tab_bar_h = self.get_tab_bar_height();
        if let Some(workspace) = &mut self.workspace {
            // Check for Terminated Shell Processes (e.g. exit / EOF)
            if workspace.has_dead_sessions() {
                let all_dead = workspace.cleanup_dead_sessions();
                if all_dead {
                    info!("All terminal sessions closed. Exiting skTerm.");
                    event_loop.exit();
                    return;
                }
                if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
                    workspace.sync_pane_sizes(
                        renderer.config.width as f32,
                        renderer.config.height as f32,
                        renderer.cell_width(),
                        renderer.cell_height(),
                        renderer.padding_x,
                        renderer.padding_y,
                        tab_bar_h,
                    );
                    window.request_redraw();
                }
            }

            let has_output = workspace.poll_all_ptys();
            if has_output {
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
        } // close workspace block

        if self.pending_geometry_save {
            if let Some(last) = self.last_resize_time {
                if last.elapsed() > Duration::from_millis(400) {
                    self.pending_geometry_save = false;
                    if let Ok(toml_str) = toml::to_string_pretty(&self.config_manager.current_config) {
                        let path = self.config_manager.path.clone();
                        std::thread::spawn(move || {
                            let _ = std::fs::write(&path, toml_str);
                        });
                    }
                }
            }
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}
