//! Main Application Event Loop, Window Handler, Tabs, Split Panes, Clipboard, and CJK IME using winit 0.30.

use crate::config::{ConfigManager, Theme};
use crate::input::encode_key_event;
use crate::layout::{PaneId, SplitDirection, Workspace};
use arboard::Clipboard;
use skterm_core::{Selection, SelectionMode};
use skterm_render::{PaneRenderItem, RenderTheme, TabItem, TermRenderer};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, Position, Size};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::window::{Window, WindowAttributes, WindowId};

/// skTerm GUI Application instance.
pub struct App {
    pub config_manager: ConfigManager,
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

    fn apply_theme_to_renderer(&mut self) {
        if let Some(renderer) = &mut self.renderer {
            let theme = Theme::get(&self.config_manager.current_config.colors.theme);
            renderer.set_theme(RenderTheme {
                default_fg: theme.foreground,
                default_bg: theme.background,
                cursor_color: theme.cursor,
                selection_color: theme.selection,
                ..Default::default()
            });
            renderer.padding_x = self.config_manager.current_config.window.padding_x * renderer.scale_factor;
            renderer.padding_y = self.config_manager.current_config.window.padding_y * renderer.scale_factor;
        }
    }

    fn get_tab_bar_height(&self) -> f32 {
        if let (Some(workspace), Some(renderer)) = (&self.workspace, &self.renderer) {
            if workspace.tabs.len() > 1 {
                32.0 * renderer.scale_factor
            } else {
                0.0
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

            for (id, rect, _) in rects {
                if rect.contains(phys_x, phys_y) {
                    let cell_w = renderer.cell_width();
                    let cell_h = renderer.cell_height();
                    let col = ((phys_x - rect.x - renderer.padding_x).max(0.0) / cell_w).floor() as usize;
                    let row = ((phys_y - rect.y - renderer.padding_y).max(0.0) / cell_h).floor() as usize;

                    if let Some(t) = workspace.active_tab().root.find_terminal(id) {
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
        if let (Some(window), Some(renderer), Some(workspace)) =
            (&self.window, &self.renderer, &self.workspace)
        {
            if let Some(terminal) = workspace.active_tab().active_terminal() {
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

                let active_pane_id = workspace.active_tab().active_pane_id;
                let pane_offset = rects
                    .iter()
                    .find(|(id, _, _)| *id == active_pane_id)
                    .map(|(_, r, _)| (r.x / scale, r.y / scale))
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

    fn copy_to_clipboard(&self) {
        if let (Some(workspace), Some(clipboard)) = (&self.workspace, &self.clipboard) {
            if let Some(terminal) = workspace.active_tab().active_terminal() {
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

    fn paste_from_clipboard(&self) {
        if let (Some(workspace), Some(clipboard)) = (&self.workspace, &self.clipboard) {
            if let Some(terminal) = workspace.active_tab().active_terminal() {
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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        info!("Creating main window and initializing GPU renderer with Workspace...");
        let font_size_pt = self.config_manager.current_config.font.size;
        let pad_x = self.config_manager.current_config.window.padding_x;
        let pad_y = self.config_manager.current_config.window.padding_y;

        let est_w = (80.0 * (font_size_pt * 0.6) + pad_x * 2.0).ceil();
        let est_h = (24.0 * (font_size_pt * 1.3) + pad_y * 2.0).ceil();

        let window_attrs = WindowAttributes::default()
            .with_title("skTerm")
            .with_inner_size(LogicalSize::new(est_w as f64, est_h as f64));

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => {
                w.set_ime_allowed(true);
                Arc::new(w)
            }
            Err(e) => {
                error!("Failed to create window: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        let renderer = match pollster::block_on(TermRenderer::new(window.clone(), font_size_pt)) {
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
        let target_logical_w = (80.0 * cell_w_logical + pad_x * 2.0).ceil();
        let target_logical_h = (24.0 * cell_h_logical + pad_y * 2.0).ceil();
        let _ = window.request_inner_size(LogicalSize::new(target_logical_w as f64, target_logical_h as f64));

        let workspace = match Workspace::new(24, 80) {
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
        self.update_ime_position();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                info!("Window close requested. Exiting skTerm.");
                event_loop.exit();
            }

            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(renderer) = &mut self.renderer {
                    let _ = renderer.update_scale_factor(scale_factor as f32);
                }
            }

            // Handle CJK IME Input
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(workspace) = &self.workspace {
                    if let Some(terminal) = workspace.active_tab().active_terminal() {
                        let _ = terminal.write_to_pty(text.as_bytes());
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                let is_super = self.modifiers.super_key();
                let is_ctrl = self.modifiers.control_key();
                let is_shift = self.modifiers.shift_key();

                let is_cmd_or_ctrl = if cfg!(target_os = "macos") { is_super } else { is_ctrl };

                if event.state == ElementState::Pressed {
                    // Check Global Multiplexing Shortcuts
                    if is_cmd_or_ctrl {
                        if let Key::Character(ref s) = event.logical_key {
                            let lower = s.to_ascii_lowercase();
                            match lower.as_str() {
                                "t" => {
                                    // New Tab: Cmd/Ctrl + T
                                    if let (Some(workspace), Some(renderer)) = (&mut self.workspace, &self.renderer) {
                                        let _ = workspace.new_tab(24, 80);
                                        let tab_bar_h = if workspace.tabs.len() > 1 { 32.0 * renderer.scale_factor } else { 0.0 };
                                        workspace.sync_pane_sizes(
                                            renderer.config.width as f32,
                                            renderer.config.height as f32,
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
                                    return;
                                }
                                "w" => {
                                    // Close Pane or Tab: Cmd/Ctrl + W
                                    if let (Some(workspace), Some(renderer)) = (&mut self.workspace, &self.renderer) {
                                        if !workspace.close_active_pane_or_tab() {
                                            event_loop.exit();
                                            return;
                                        }
                                        let tab_bar_h = if workspace.tabs.len() > 1 { 32.0 * renderer.scale_factor } else { 0.0 };
                                        workspace.sync_pane_sizes(
                                            renderer.config.width as f32,
                                            renderer.config.height as f32,
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
                                    return;
                                }
                                "d" => {
                                    // Split Pane: Cmd+D (Left/Right) or Cmd+Shift+D (Top/Bottom)
                                    let dir = if is_shift { SplitDirection::Vertical } else { SplitDirection::Horizontal };
                                    if let (Some(workspace), Some(renderer)) = (&mut self.workspace, &self.renderer) {
                                        let _ = workspace.split_active_pane(dir, 24, 80);
                                        let tab_bar_h = if workspace.tabs.len() > 1 { 32.0 * renderer.scale_factor } else { 0.0 };
                                        workspace.sync_pane_sizes(
                                            renderer.config.width as f32,
                                            renderer.config.height as f32,
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
                                            workspace.active_tab_mut().focus_next();
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
                                            workspace.active_tab_mut().focus_prev();
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
                }

                // Send key to active terminal
                if let Some(workspace) = &self.workspace {
                    if let Some(terminal) = workspace.active_tab().active_terminal() {
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

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);
                if self.is_mouse_down {
                    if let Some((pane_id, grid_pos)) = self.pixel_to_pane_grid(self.mouse_pos.0, self.mouse_pos.1) {
                        if let Some(workspace) = &mut self.workspace {
                            if let Some(t) = workspace.active_tab_mut().root.find_terminal_mut(pane_id) {
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

            WindowEvent::MouseInput { state, button, .. } => {
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
                                workspace.active_tab_mut().active_pane_id = pane_id;
                                if let Some(t) = workspace.active_tab_mut().root.find_terminal_mut(pane_id) {
                                    let active = t.grid_mut().active_mut();
                                    active.selection = Some(Selection::new(grid_pos, grid_pos, mode));
                                }
                                if let Some(window) = &self.window {
                                    window.request_redraw();
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
                let scroll_lines = match delta {
                    MouseScrollDelta::LineDelta(_, y) => (y * 3.0) as isize,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 20.0) as isize,
                };

                if let Some((pane_id, _)) = self.pixel_to_pane_grid(self.mouse_pos.0, self.mouse_pos.1) {
                    if let Some(workspace) = &mut self.workspace {
                        if let Some(t) = workspace.active_tab_mut().root.find_terminal_mut(pane_id) {
                            t.grid_mut().active_mut().scroll_display(scroll_lines);
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }

            WindowEvent::Resized(physical_size) => {
                if let (Some(renderer), Some(workspace)) = (&mut self.renderer, &mut self.workspace) {
                    renderer.resize(physical_size.width, physical_size.height);

                    let tab_bar_h = if workspace.tabs.len() > 1 { 32.0 * renderer.scale_factor } else { 0.0 };
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
                if let (Some(renderer), Some(workspace)) = (&mut self.renderer, &mut self.workspace) {
                    let _ = workspace.poll_all_ptys();

                    let tab_bar_h = if workspace.tabs.len() > 1 { 32.0 * renderer.scale_factor } else { 0.0 };
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
                    let active_tab = workspace.active_tab();

                    for (id, rect, is_focused) in &pane_rects {
                        if let Some(t) = active_tab.root.find_terminal(*id) {
                            render_panes.push(PaneRenderItem {
                                grid: t.grid(),
                                rect_x: rect.x,
                                rect_y: rect.y,
                                rect_w: rect.width,
                                rect_h: rect.height,
                                is_focused: *is_focused,
                            });
                        }
                    }

                    if let Err(e) = renderer.render_workspace(&tabs, &render_panes, tab_bar_h) {
                        error!("Render error: {:?}", e);
                    }

                    for (_, rect, _) in &pane_rects {
                        if let Some(t) = workspace.active_tab_mut().root.find_terminal_mut(rect.x as u32) {
                            t.grid_mut().clear_dirty();
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
            self.apply_theme_to_renderer();
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        }

        if let Some(workspace) = &mut self.workspace {
            // Check for Terminated Shell Processes (e.g. exit / EOF)
            if workspace.has_dead_sessions() {
                let still_alive = workspace.cleanup_dead_sessions();
                if !still_alive {
                    info!("All terminal sessions closed. Exiting skTerm.");
                    event_loop.exit();
                    return;
                }
                if let (Some(renderer), Some(window)) = (&self.renderer, &self.window) {
                    let tab_bar_h = if workspace.tabs.len() > 1 { 32.0 * renderer.scale_factor } else { 0.0 };
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
