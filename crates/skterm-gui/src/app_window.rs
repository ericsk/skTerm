//! Main Application Event Loop, Window Handler, Tabs, Split Panes, Clipboard, CJK IME, and In-App Preferences GUI.

use crate::config::{parse_hex_color, ConfigManager, Theme};
use crate::input::encode_key_event;
use crate::layout::{PaneId, SplitDirection, Workspace};
use crate::preferences_gui::PreferencesGui;
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
    pub preferences_gui: PreferencesGui,
    pub egui_ctx: egui::Context,
    pub egui_state: Option<egui_winit::State>,
    pub egui_renderer: Option<egui_wgpu::Renderer>,
    pub window: Option<Arc<Window>>,
    pub renderer: Option<TermRenderer>,
    pub workspace: Option<Workspace>,
    pub clipboard: Option<Arc<Mutex<Clipboard>>>,
    pub modifiers: ModifiersState,
    pub mouse_pos: (f32, f32),
    pub is_mouse_down: bool,
    pub last_click_time: Instant,
    pub click_count: usize,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let config_manager = ConfigManager::load_or_create();
        let preferences_gui = PreferencesGui::new();
        let egui_ctx = egui::Context::default();
        crate::preferences_gui::setup_egui_cjk_fonts(&egui_ctx);

        let clipboard = match Clipboard::new() {
            Ok(cb) => Some(Arc::new(Mutex::new(cb))),
            Err(e) => {
                warn!("Failed to initialize clipboard: {:?}", e);
                None
            }
        };

        Self {
            config_manager,
            preferences_gui,
            egui_ctx,
            egui_state: None,
            egui_renderer: None,
            window: None,
            renderer: None,
            workspace: None,
            clipboard,
            modifiers: ModifiersState::default(),
            mouse_pos: (0.0, 0.0),
            is_mouse_down: false,
            last_click_time: Instant::now(),
            click_count: 0,
        }
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
        event_loop.run_app(&mut self)?;
        Ok(())
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

    fn apply_theme_to_renderer(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            let colors_cfg = &self.config_manager.current_config.colors;
            let window_cfg = &self.config_manager.current_config.window;
            let theme = Theme::get(&colors_cfg.theme);

            let fg = colors_cfg
                .foreground
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(theme.foreground);

            let mut bg = colors_cfg
                .background
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(theme.background);

            // Apply opacity to background
            bg[3] = window_cfg.opacity.clamp(0.1, 1.0);

            let cursor = colors_cfg
                .cursor
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(theme.cursor);

            let selection = colors_cfg
                .selection
                .as_deref()
                .and_then(parse_hex_color)
                .unwrap_or(theme.selection);

            // Custom ANSI Palette
            let ansi_palette = colors_cfg.palette.as_ref().map(|p| {
                let mut pal = [
                    [0.0, 0.0, 0.0, 1.0],       // 0: Black
                    [0.80, 0.20, 0.20, 1.0],    // 1: Red
                    [0.20, 0.80, 0.20, 1.0],    // 2: Green
                    [0.80, 0.80, 0.20, 1.0],    // 3: Yellow
                    [0.20, 0.40, 0.80, 1.0],    // 4: Blue
                    [0.80, 0.20, 0.80, 1.0],    // 5: Magenta
                    [0.20, 0.80, 0.80, 1.0],    // 6: Cyan
                    [0.80, 0.80, 0.80, 1.0],    // 7: White
                    [0.40, 0.40, 0.40, 1.0],    // 8: Bright Black
                    [1.0, 0.35, 0.35, 1.0],     // 9: Bright Red
                    [0.35, 1.0, 0.35, 1.0],     // 10: Bright Green
                    [1.0, 1.0, 0.35, 1.0],      // 11: Bright Yellow
                    [0.35, 0.65, 1.0, 1.0],     // 12: Bright Blue
                    [1.0, 0.35, 1.0, 1.0],      // 13: Bright Magenta
                    [0.35, 1.0, 1.0, 1.0],      // 14: Bright Cyan
                    [1.0, 1.0, 1.0, 1.0],       // 15: Bright White
                ];

                if let Some(c) = p.black.as_deref().and_then(parse_hex_color) { pal[0] = c; }
                if let Some(c) = p.red.as_deref().and_then(parse_hex_color) { pal[1] = c; }
                if let Some(c) = p.green.as_deref().and_then(parse_hex_color) { pal[2] = c; }
                if let Some(c) = p.yellow.as_deref().and_then(parse_hex_color) { pal[3] = c; }
                if let Some(c) = p.blue.as_deref().and_then(parse_hex_color) { pal[4] = c; }
                if let Some(c) = p.magenta.as_deref().and_then(parse_hex_color) { pal[5] = c; }
                if let Some(c) = p.cyan.as_deref().and_then(parse_hex_color) { pal[6] = c; }
                if let Some(c) = p.white.as_deref().and_then(parse_hex_color) { pal[7] = c; }
                if let Some(c) = p.bright_black.as_deref().and_then(parse_hex_color) { pal[8] = c; }
                if let Some(c) = p.bright_red.as_deref().and_then(parse_hex_color) { pal[9] = c; }
                if let Some(c) = p.bright_green.as_deref().and_then(parse_hex_color) { pal[10] = c; }
                if let Some(c) = p.bright_yellow.as_deref().and_then(parse_hex_color) { pal[11] = c; }
                if let Some(c) = p.bright_blue.as_deref().and_then(parse_hex_color) { pal[12] = c; }
                if let Some(c) = p.bright_magenta.as_deref().and_then(parse_hex_color) { pal[13] = c; }
                if let Some(c) = p.bright_cyan.as_deref().and_then(parse_hex_color) { pal[14] = c; }
                if let Some(c) = p.bright_white.as_deref().and_then(parse_hex_color) { pal[15] = c; }

                pal
            });

            renderer.set_theme(RenderTheme {
                default_fg: fg,
                default_bg: bg,
                cursor_color: cursor,
                selection_color: selection,
                ansi_palette,
                ..Default::default()
            });

            renderer.padding_x = window_cfg.padding_x * renderer.scale_factor;
            renderer.padding_y = window_cfg.padding_y * renderer.scale_factor;
        }
    }

    fn get_tab_bar_height(&self) -> f32 {
        if let (Some(workspace), Some(renderer)) = (&self.workspace, &self.renderer) {
            match self.config_manager.current_config.window.tab_bar_style.as_str() {
                "always" => 32.0 * renderer.scale_factor,
                "never" => 0.0,
                _ => {
                    if workspace.tabs.len() > 1 {
                        32.0 * renderer.scale_factor
                    } else {
                        0.0
                    }
                }
            }
        } else {
            0.0
        }
    }

    fn pixel_to_pane_grid(&self, px: f32, py: f32) -> Option<(PaneId, (usize, usize))> {
        if let (Some(renderer), Some(workspace)) = (&self.renderer, &self.workspace) {
            let scale = renderer.scale_factor;
            let phys_x = px * scale;
            let phys_y = py * scale;
            let tab_bar_h = self.get_tab_bar_height();

            let rects = workspace.compute_active_pane_rects(
                renderer.config.width as f32,
                renderer.config.height as f32,
                tab_bar_h,
            );

            for (id, rect) in rects {
                if rect.contains(phys_x, phys_y) {
                    let cell_w = renderer.cell_width();
                    let cell_h = renderer.cell_height();
                    let col = ((phys_x - rect.x - renderer.padding_x).max(0.0) / cell_w).floor() as usize;
                    let row = ((phys_y - rect.y - renderer.padding_y).max(0.0) / cell_h).floor() as usize;

                    if let Some(t) = workspace.active_tab().and_then(|tab| tab.root.find_terminal(id)) {
                        let max_col = t.grid().active().cols.saturating_sub(1);
                        let max_row = t.grid().active().rows.saturating_sub(1);
                        return Some((id, (row.min(max_row), col.min(max_col))));
                    }
                }
            }
        }
        None
    }

    fn update_ime_position(&self) {
        if self.preferences_gui.is_open {
            return;
        }

        if let (Some(window), Some(renderer), Some(workspace)) =
            (&self.window, &self.renderer, &self.workspace)
        {
            if let Some(tab) = workspace.active_tab() {
                if let Some(terminal) = tab.active_terminal() {
                    let active = terminal.grid().active();
                    let scale = renderer.scale_factor;
                    let cell_w_logical = renderer.cell_width() / scale;
                    let cell_h_logical = renderer.cell_height() / scale;
                    let pad_x_logical = renderer.padding_x / scale;
                    let pad_y_logical = renderer.padding_y / scale;

                    let tab_bar_h = self.get_tab_bar_height() / scale;
                    let rects = workspace.compute_active_pane_rects(
                        renderer.config.width as f32,
                        renderer.config.height as f32,
                        self.get_tab_bar_height(),
                    );

                    let active_pane_id = tab.active_pane_id;
                    let pane_offset = rects
                        .iter()
                        .find(|(id, _)| *id == active_pane_id)
                        .map(|(_, r)| (r.x / scale, r.y / scale))
                        .unwrap_or((0.0, tab_bar_h));

                    let cur_x = pane_offset.0 + pad_x_logical + (active.cursor.col as f32 * cell_w_logical);
                    let cur_y = pane_offset.1 + pad_y_logical + (active.cursor.row as f32 * cell_h_logical);

                    window.set_ime_cursor_area(
                        Position::Logical(LogicalPosition::new(cur_x as f64, (cur_y + cell_h_logical) as f64)),
                        Size::Logical(LogicalSize::new(cell_w_logical as f64, cell_h_logical as f64)),
                    );
                }
            }
        }
    }

    fn copy_to_clipboard(&self) {
        if let (Some(workspace), Some(clipboard)) = (&self.workspace, &self.clipboard) {
            if let Some(tab) = workspace.active_tab() {
                if let Some(terminal) = tab.active_terminal() {
                    let text = terminal.grid().active().extract_selected_text();
                    if !text.is_empty() {
                        if let Ok(mut cb) = clipboard.lock() {
                            let _ = cb.set_text(text);
                            info!("Copied selected text to clipboard");
                        }
                    }
                }
            }
        }
    }

    fn paste_from_clipboard(&self) {
        if let (Some(workspace), Some(clipboard)) = (&self.workspace, &self.clipboard) {
            if let Some(tab) = workspace.active_tab() {
                if let Some(terminal) = tab.active_terminal() {
                    if let Ok(mut cb) = clipboard.lock() {
                        if let Ok(text) = cb.get_text() {
                            let mut bytes = Vec::new();
                            if terminal.state.bracketed_paste {
                                bytes.extend_from_slice(b"\x1b[200~");
                                bytes.extend_from_slice(text.as_bytes());
                                bytes.extend_from_slice(b"\x1b[201~");
                            } else {
                                bytes.extend_from_slice(text.as_bytes());
                            }
                            let _ = terminal.write_to_pty(&bytes);
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }
        }
    }

    fn sync_all_pane_sizes(&mut self) {
        let tab_bar_h = self.get_tab_bar_height();
        if let (Some(workspace), Some(renderer)) = (&mut self.workspace, &self.renderer) {
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
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        info!("Creating main window and initializing GPU renderer with Workspace...");
        let font_size_pt = self.config_manager.current_config.font.size;
        let pad_x = self.config_manager.current_config.window.padding_x;
        let pad_y = self.config_manager.current_config.window.padding_y;
        let init_cols = self.config_manager.current_config.window.columns.max(20) as f32;
        let init_rows = self.config_manager.current_config.window.rows.max(5) as f32;

        let est_w = (init_cols * (font_size_pt * 0.6) + pad_x * 2.0).ceil();
        let est_h = (init_rows * (font_size_pt * 1.3) + pad_y * 2.0).ceil();

        let mut window_attrs = WindowAttributes::default()
            .with_title("skTerm")
            .with_inner_size(LogicalSize::new(est_w as f64, est_h as f64));

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

        let egui_state = egui_winit::State::new(
            self.egui_ctx.clone(),
            egui::viewport::ViewportId::ROOT,
            &window,
            Some(renderer.scale_factor),
            None,
            None,
        );

        let egui_renderer = egui_wgpu::Renderer::new(
            &renderer.device,
            renderer.config.format,
            None,
            1,
            false,
        );

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
        self.egui_state = Some(egui_state);
        self.egui_renderer = Some(egui_renderer);

        self.apply_theme_to_renderer();
        self.sync_all_pane_sizes();
        info!("skTerm Window, Renderer, Workspace, and Preferences GUI initialized successfully!");
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _window_id: WindowId, event: WindowEvent) {
        // Forward event to egui if preferences modal is open
        if self.preferences_gui.is_open {
            if let (Some(egui_state), Some(window)) = (&mut self.egui_state, &self.window) {
                let res = egui_state.on_window_event(window, &event);
                if res.repaint {
                    window.request_redraw();
                }
                if res.consumed {
                    return;
                }
            }
        }

        match event {
            WindowEvent::CloseRequested => {
                info!("Close requested, exiting application.");
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(new_mods) => {
                self.modifiers = new_mods.state();
            }

            WindowEvent::Ime(ime_event) => {
                if self.preferences_gui.is_open {
                    return;
                }
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
                                // Cmd+, / Ctrl+, -> Toggle In-App Preferences GUI Modal
                                info!("Toggling Preferences GUI via shortcut Cmd+, / Ctrl+,");
                                self.preferences_gui.toggle(&self.config_manager.current_config);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
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
                            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" => {
                                if let Ok(num) = lower.parse::<usize>() {
                                    if let Some(workspace) = &mut self.workspace {
                                        workspace.select_tab(num - 1);
                                        if let Some(window) = &self.window {
                                            window.request_redraw();
                                        }
                                    }
                                }
                                return;
                            }
                            _ => {}
                        }
                    }
                }

                // If Preferences is open, don't send keypresses to terminal
                if self.preferences_gui.is_open {
                    return;
                }

                // Send key to active terminal
                if let Some(workspace) = &self.workspace {
                    if let Some(tab) = workspace.active_tab() {
                        if let Some(terminal) = tab.active_terminal() {
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

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);
                if self.preferences_gui.is_open {
                    return;
                }
                if self.is_mouse_down {
                    if let Some((pane_id, grid_pos)) = self.pixel_to_pane_grid(self.mouse_pos.0, self.mouse_pos.1) {
                        if let Some(workspace) = &mut self.workspace {
                            if let Some(tab) = workspace.active_tab_mut() {
                                if let Some(t) = tab.root.find_terminal_mut(pane_id) {
                                    let active = t.grid_mut().active_mut();
                                    if let Some(sel) = &mut active.selection {
                                        sel.end = grid_pos;
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
                if self.preferences_gui.is_open {
                    return;
                }
                if button == MouseButton::Left {
                    if state == ElementState::Pressed {
                        self.is_mouse_down = true;

                        // Check Tab Bar Click
                        let scale = self.renderer.as_ref().map(|r| r.scale_factor).unwrap_or(1.0);
                        let phys_y = self.mouse_pos.1 * scale;
                        let tab_bar_h = self.get_tab_bar_height();

                        if tab_bar_h > 0.0 && phys_y < tab_bar_h {
                            let phys_x = self.mouse_pos.0 * scale;
                            let tab_w = 140.0 * scale;
                            let clicked_idx = (phys_x / tab_w).floor() as usize;
                            if let Some(workspace) = &mut self.workspace {
                                workspace.select_tab(clicked_idx);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                            return;
                        }

                        // Pane Click & Selection
                        let now = Instant::now();
                        if now.duration_since(self.last_click_time) < Duration::from_millis(400) {
                            self.click_count += 1;
                        } else {
                            self.click_count = 1;
                        }
                        self.last_click_time = now;

                        if let Some((pane_id, grid_pos)) = self.pixel_to_pane_grid(self.mouse_pos.0, self.mouse_pos.1) {
                            let mode = match self.click_count {
                                2 => SelectionMode::Word,
                                3 => SelectionMode::Line,
                                _ => SelectionMode::Simple,
                            };

                            if let Some(workspace) = &mut self.workspace {
                                if let Some(tab) = workspace.active_tab_mut() {
                                    tab.active_pane_id = pane_id;
                                    if let Some(t) = tab.root.find_terminal_mut(pane_id) {
                                        let active = t.grid_mut().active_mut();
                                        active.selection = Some(Selection::new(grid_pos, grid_pos, mode));
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
                if self.preferences_gui.is_open {
                    return;
                }
                let scroll_lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as isize,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as isize,
                };

                if let Some((pane_id, _)) = self.pixel_to_pane_grid(self.mouse_pos.0, self.mouse_pos.1) {
                    if let Some(workspace) = &mut self.workspace {
                        if let Some(tab) = workspace.active_tab_mut() {
                            if let Some(t) = tab.root.find_terminal_mut(pane_id) {
                                t.grid_mut().active_mut().scroll_display(scroll_lines);
                                if let Some(window) = &self.window {
                                    window.request_redraw();
                                }
                            }
                        }
                    }
                }
            }

            WindowEvent::Resized(physical_size) => {
                let tab_bar_h = self.get_tab_bar_height();
                if let (Some(renderer), Some(workspace)) = (&mut self.renderer, &mut self.workspace) {
                    renderer.resize(physical_size.width, physical_size.height);
                    workspace.sync_pane_sizes(
                        physical_size.width as f32,
                        physical_size.height as f32,
                        renderer.cell_width(),
                        renderer.cell_height(),
                        renderer.padding_x,
                        renderer.padding_y,
                        tab_bar_h,
                    );

                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let tab_bar_h = self.get_tab_bar_height();
                if let (Some(renderer), Some(workspace), Some(window)) = (&mut self.renderer, &mut self.workspace, &self.window) {
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

                    if self.preferences_gui.is_open {
                        if let (Some(egui_state), Some(egui_renderer)) = (&mut self.egui_state, &mut self.egui_renderer) {
                            let raw_input = egui_state.take_egui_input(window);
                            let full_output = self.egui_ctx.run(raw_input, |ctx| {
                                self.preferences_gui.render(ctx, &mut self.config_manager);
                            });
                            egui_state.handle_platform_output(window, full_output.platform_output);

                            let shapes = full_output.shapes;
                            let ppp = full_output.pixels_per_point;
                            let clipped_primitives = self.egui_ctx.tessellate(shapes, ppp);
                            let textures_delta = full_output.textures_delta;

                            let screen_descriptor = egui_wgpu::ScreenDescriptor {
                                size_in_pixels: [renderer.config.width, renderer.config.height],
                                pixels_per_point: ppp,
                            };

                            let egui_overlay_fn = |dev: &wgpu::Device, q: &wgpu::Queue, v: &wgpu::TextureView, _cfg: &wgpu::SurfaceConfiguration| {
                                crate::preferences_gui::render_egui_overlay(
                                    egui_renderer,
                                    dev,
                                    q,
                                    v,
                                    &textures_delta,
                                    &clipped_primitives,
                                    &screen_descriptor,
                                );
                            };

                            if let Err(e) = renderer.render_workspace_with_overlay(&tabs, &render_panes, tab_bar_h, Some(egui_overlay_fn)) {
                                error!("Render error: {:?}", e);
                            }
                        } else {
                            let _ = renderer.render_workspace(&tabs, &render_panes, tab_bar_h);
                        }
                    } else {
                        if let Err(e) = renderer.render_workspace(&tabs, &render_panes, tab_bar_h) {
                            error!("Render error: {:?}", e);
                        }
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
        if self.config_manager.check_reload().is_some() {
            info!("Applying hot-reloaded configuration to running app...");
            let font_cfg = &self.config_manager.current_config.font;
            if let Some(renderer) = &mut self.renderer {
                let _ = renderer.update_font(&font_cfg.family, font_cfg.size, &font_cfg.fallback_families);
            }
            self.apply_theme_to_renderer();
            self.sync_all_pane_sizes();
            if let Some(window) = &self.window {
                window.request_redraw();
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
        }

        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}
