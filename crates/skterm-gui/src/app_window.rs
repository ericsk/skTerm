//! Main Application Event Loop, Window Handler, Clipboard, Mouse Selection, CJK IME, and Retina HiDPI.

use crate::config::{ConfigManager, Theme};
use crate::input::encode_key_event;
use arboard::Clipboard;
use skterm_core::{Selection, SelectionMode, Terminal};
use skterm_render::{RenderTheme, TermRenderer};
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
    pub terminal: Option<Terminal>,
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
            terminal: None,
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
            });
            renderer.padding_x = self.config_manager.current_config.window.padding_x * renderer.scale_factor;
            renderer.padding_y = self.config_manager.current_config.window.padding_y * renderer.scale_factor;
        }
    }

    fn pixel_to_grid(&self, px: f32, py: f32) -> (usize, usize) {
        if let (Some(renderer), Some(terminal)) = (&self.renderer, &self.terminal) {
            let scale = renderer.scale_factor;
            let phys_x = px * scale;
            let phys_y = py * scale;
            let cell_w = renderer.cell_width();
            let cell_h = renderer.cell_height();

            let col = ((phys_x - renderer.padding_x).max(0.0) / cell_w).floor() as usize;
            let row = ((phys_y - renderer.padding_y).max(0.0) / cell_h).floor() as usize;

            let max_col = terminal.grid().active().cols.saturating_sub(1);
            let max_row = terminal.grid().active().rows.saturating_sub(1);

            (row.min(max_row), col.min(max_col))
        } else {
            (0, 0)
        }
    }

    fn update_ime_position(&self) {
        if let (Some(window), Some(renderer), Some(terminal)) =
            (&self.window, &self.renderer, &self.terminal)
        {
            let active = terminal.grid().active();
            let scale = renderer.scale_factor;
            let cell_w_logical = renderer.cell_width() / scale;
            let cell_h_logical = renderer.cell_height() / scale;
            let pad_x_logical = renderer.padding_x / scale;
            let pad_y_logical = renderer.padding_y / scale;

            let cur_x = pad_x_logical + (active.cursor.col as f32 * cell_w_logical);
            let cur_y = pad_y_logical + (active.cursor.row as f32 * cell_h_logical);

            window.set_ime_cursor_area(
                Position::Logical(LogicalPosition::new(cur_x as f64, (cur_y + cell_h_logical) as f64)),
                Size::Logical(LogicalSize::new(cell_w_logical as f64, cell_h_logical as f64)),
            );
        }
    }

    fn copy_to_clipboard(&self) {
        if let (Some(terminal), Some(clipboard)) = (&self.terminal, &self.clipboard) {
            let text = terminal.grid().active().extract_selected_text();
            if !text.is_empty() {
                if let Ok(mut cb) = clipboard.lock() {
                    let _ = cb.set_text(text);
                    info!("Copied selected text to clipboard");
                }
            }
        }
    }

    fn paste_from_clipboard(&self) {
        if let (Some(terminal), Some(clipboard)) = (&self.terminal, &self.clipboard) {
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

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        info!("Creating main window and initializing GPU renderer...");
        let font_size_pt = self.config_manager.current_config.font.size;
        let pad_x = self.config_manager.current_config.window.padding_x;
        let pad_y = self.config_manager.current_config.window.padding_y;

        // Estimated initial logical size for 80x24
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

        // Resize window to exact 80x24 standard terminal size in logical units
        let scale = renderer.scale_factor;
        let cell_w_logical = renderer.cell_width() / scale;
        let cell_h_logical = renderer.cell_height() / scale;
        let target_logical_w = (80.0 * cell_w_logical + pad_x * 2.0).ceil();
        let target_logical_h = (24.0 * cell_h_logical + pad_y * 2.0).ceil();
        let _ = window.request_inner_size(LogicalSize::new(target_logical_w as f64, target_logical_h as f64));

        info!(
            "Spawning terminal shell with standard 80x24 dimensions (font: {}pt, scale: {}, logical_win: {}x{})",
            font_size_pt, scale, target_logical_w, target_logical_h
        );

        let terminal = match Terminal::new(24, 80, 10000, None, None) {
            Ok(t) => t,
            Err(e) => {
                error!("Failed to spawn PTY terminal: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.terminal = Some(terminal);
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
                if let Some(mut term) = self.terminal.take() {
                    let _ = term.resize(1, 1);
                }
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

            // Handle CJK IME Input (Pinyin, Zhuyin, Cangjie, etc.)
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(terminal) = &self.terminal {
                    let _ = terminal.write_to_pty(text.as_bytes());
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::KeyboardInput { event, .. } => {
                // Check Shortcuts for Copy / Paste
                let is_copy_cmd = (cfg!(target_os = "macos") && self.modifiers.super_key())
                    || (!cfg!(target_os = "macos") && self.modifiers.control_key() && self.modifiers.shift_key());

                let is_paste_cmd = (cfg!(target_os = "macos") && self.modifiers.super_key())
                    || (!cfg!(target_os = "macos") && self.modifiers.control_key() && self.modifiers.shift_key());

                if event.state == ElementState::Pressed {
                    if let Key::Character(ref s) = event.logical_key {
                        let lower = s.to_ascii_lowercase();
                        if is_copy_cmd && lower == "c" {
                            self.copy_to_clipboard();
                            return;
                        }
                        if is_paste_cmd && lower == "v" {
                            self.paste_from_clipboard();
                            return;
                        }
                    }
                }

                if let Some(terminal) = &self.terminal {
                    let app_cursor = terminal.state.application_cursor_keys;
                    if let Some(bytes) = encode_key_event(&event, self.modifiers, app_cursor) {
                        let _ = terminal.write_to_pty(&bytes);
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.mouse_pos = (position.x as f32, position.y as f32);
                if self.is_mouse_down {
                    let grid_pos = self.pixel_to_grid(self.mouse_pos.0, self.mouse_pos.1);
                    if let Some(terminal) = &mut self.terminal {
                        let active = terminal.grid_mut().active_mut();
                        if let Some(sel) = &mut active.selection {
                            sel.end = grid_pos;
                            if let Some(window) = &self.window {
                                window.request_redraw();
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if button == MouseButton::Left {
                    if state == ElementState::Pressed {
                        self.is_mouse_down = true;
                        let now = Instant::now();
                        if now.duration_since(self.last_click_time) < Duration::from_millis(400) {
                            self.click_count += 1;
                        } else {
                            self.click_count = 1;
                        }
                        self.last_click_time = now;

                        let grid_pos = self.pixel_to_grid(self.mouse_pos.0, self.mouse_pos.1);
                        let mode = match self.click_count {
                            2 => SelectionMode::Word,
                            3 => SelectionMode::Line,
                            _ => SelectionMode::Simple,
                        };

                        if let Some(terminal) = &mut self.terminal {
                            let active = terminal.grid_mut().active_mut();
                            active.selection = Some(Selection::new(grid_pos, grid_pos, mode));
                            if let Some(window) = &self.window {
                                window.request_redraw();
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

                if let Some(terminal) = &mut self.terminal {
                    terminal.grid_mut().active_mut().scroll_display(scroll_lines);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::Resized(physical_size) => {
                if let (Some(renderer), Some(terminal)) = (&mut self.renderer, &mut self.terminal) {
                    renderer.resize(physical_size.width, physical_size.height);

                    let cell_w = renderer.cell_width();
                    let cell_h = renderer.cell_height();
                    let cols = ((physical_size.width as f32 - renderer.padding_x * 2.0) / cell_w)
                        .floor()
                        .max(10.0) as usize;
                    let rows = ((physical_size.height as f32 - renderer.padding_y * 2.0) / cell_h)
                        .floor()
                        .max(4.0) as usize;

                    let _ = terminal.resize(rows, cols);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                if let (Some(renderer), Some(terminal)) = (&mut self.renderer, &mut self.terminal) {
                    let _ = terminal.poll_pty();
                    if let Err(e) = renderer.render(terminal.grid()) {
                        error!("Render error: {:?}", e);
                    }
                    terminal.grid_mut().clear_dirty();
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

        if let Some(terminal) = &mut self.terminal {
            if !terminal.is_alive() {
                info!("Child shell terminated. Closing application.");
                event_loop.exit();
                return;
            }

            let has_output = terminal.poll_pty();
            if has_output || terminal.grid().is_dirty() {
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
