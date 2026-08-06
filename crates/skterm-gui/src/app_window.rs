//! Main Application Event Loop and Window Handler using winit 0.30.

use crate::input::encode_key_event;
use skterm_core::Terminal;
use skterm_render::TermRenderer;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{error, info};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

/// skTerm GUI Application instance.
pub struct App {
    pub font_size: f32,
    pub window: Option<Arc<Window>>,
    pub renderer: Option<TermRenderer>,
    pub terminal: Option<Terminal>,
    pub modifiers: ModifiersState,
    pub last_frame_time: Instant,
}

impl App {
    pub fn new(font_size: f32) -> Self {
        Self {
            font_size,
            window: None,
            renderer: None,
            terminal: None,
            modifiers: ModifiersState::default(),
            last_frame_time: Instant::now(),
        }
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(16)));
        event_loop.run_app(&mut self)?;
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        info!("Creating main window and initializing GPU renderer...");
        let window_attrs = WindowAttributes::default()
            .with_title("skTerm")
            .with_inner_size(LogicalSize::new(960.0, 640.0));

        let window = match event_loop.create_window(window_attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                error!("Failed to create window: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        let renderer = match pollster::block_on(TermRenderer::new(window.clone(), self.font_size)) {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to initialize wgpu renderer: {:?}", e);
                event_loop.exit();
                return;
            }
        };

        let size = window.inner_size();
        let cell_w = renderer.cell_width();
        let cell_h = renderer.cell_height();
        let cols = ((size.width as f32 - renderer.padding_x * 2.0) / cell_w).floor().max(10.0) as usize;
        let rows = ((size.height as f32 - renderer.padding_y * 2.0) / cell_h).floor().max(4.0) as usize;

        info!(
            "Spawning terminal shell with Grid dimensions: cols={}, rows={}",
            cols, rows
        );

        let terminal = match Terminal::new(rows, cols, 10000, None, None) {
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

            WindowEvent::KeyboardInput { event, .. } => {
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
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
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

        // Schedule next check (60 FPS pacing / 16ms)
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}
