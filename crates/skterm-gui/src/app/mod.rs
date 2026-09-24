//! Main Application Event Loop, Multi-Window Session Handler, Native macOS Tabbing,
//! Split Panes, Clipboard, CJK IME, and Native Preferences Window.

#![allow(unexpected_cfgs)]

mod agents;
mod events;
mod input_keyboard;
mod input_mouse;
#[cfg(target_os = "macos")]
mod macos;
mod pacer;
mod render;
mod session;
mod theme;
mod tick;
mod window;

pub use agents::{AgentList, AgentRow};
pub use events::AppEvent;
pub use input_mouse::open_url_in_browser;
#[cfg(target_os = "macos")]
pub use macos::{center_macos_window_title, set_macos_tab_label, style_macos_tab_bar};
pub use session::{ScrollbarDragState, TerminalSession};

use crate::config::ConfigManager;
use crate::preferences_window::PreferencesWindow;
use arboard::Clipboard;
#[cfg(target_os = "macos")]
use macos::get_nswindow;
use skterm_core::PtySpawnOptions;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::WindowId;

/// skTerm GUI Application instance managing multiple native window sessions or native tabs.
pub struct App {
    pub config_manager: ConfigManager,
    pub preferences_window: Option<PreferencesWindow>,
    pub sessions: HashMap<WindowId, TerminalSession>,
    pub session_order: Vec<WindowId>,
    pub active_session_id: Option<WindowId>,
    pub sidebar_open: bool,
    /// Last value of the macOS titlebar toggle's click counter that we acted on.
    /// AppKit runs the button's action outside our event loop, so we observe it
    /// by counter rather than by flag -- a flag would drop a second click that
    /// landed before the first was polled.
    #[cfg(target_os = "macos")]
    pub last_agent_toggle_click: u64,
    pub clipboard: Option<Arc<Mutex<Clipboard>>>,
    pub modifiers: ModifiersState,
    /// Set whenever `current_config` differs from what is on disk. Both window
    /// geometry changes and live preference edits funnel through here so there is
    /// only ever one writer, debounced by `last_config_change`.
    pub pending_config_save: bool,
    pub last_config_change: Option<Instant>,
    pub editing_agent_pane: Option<AgentRow>,
    pub editing_agent_text: String,
    pub editing_agent_cursor: usize,
    pub ime_preedit: String,
    pub last_ime_commit_time: Option<Instant>,
    pub event_proxy: Option<winit::event_loop::EventLoopProxy<AppEvent>>,
    /// Set by PTY reader threads when they have queued output, cleared by the event
    /// loop when it drains. Ensures at most one `PtyWakeup` is in flight at a time
    /// instead of one per 64 KiB read.
    pub pty_wakeup_pending: Arc<AtomicBool>,
    /// The agent list the sidebar is drawn from *and* hit-tested against.
    pub cached_ai_agents: AgentList,
    pub last_ai_agent_scan: Option<Instant>,
    /// The agent card last clicked, and when, for the rename double-click.
    ///
    /// Deliberately not `TerminalSession::last_click_time`. Sharing that one made
    /// the two gestures interfere: clicking a card stamped it without touching
    /// `click_count`, so a following click in the terminal within the double-click
    /// window saw a "recent" click, bumped the count to 2 and selected a word from
    /// what the user meant as a single click.
    pub last_agent_card_click: Option<(AgentRow, Instant)>,
    #[cfg(target_os = "macos")]
    pub macos_editor: crate::macos_textfield::NativeSidebarEditor,
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
            sessions: HashMap::new(),
            session_order: Vec::new(),
            active_session_id: None,
            sidebar_open: false,
            #[cfg(target_os = "macos")]
            last_agent_toggle_click: crate::macos_titlebar::click_count(),
            clipboard,
            modifiers: ModifiersState::default(),
            pending_config_save: false,
            last_config_change: None,
            editing_agent_pane: None,
            editing_agent_text: String::new(),
            editing_agent_cursor: 0,
            ime_preedit: String::new(),
            last_ime_commit_time: None,
            event_proxy: None,
            pty_wakeup_pending: Arc::new(AtomicBool::new(false)),
            cached_ai_agents: AgentList::default(),
            last_agent_card_click: None,
            last_ai_agent_scan: None,
            #[cfg(target_os = "macos")]
            macos_editor: crate::macos_textfield::NativeSidebarEditor::new(),
        }
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
        let proxy = event_loop.create_proxy();
        self.event_proxy = Some(proxy.clone());
        self.config_manager.set_wakeup(Arc::new(move || {
            let _ = proxy.send_event(AppEvent::ConfigChanged);
        }));
        // Sleep until something actually happens. `about_to_wait` re-arms a timer
        // whenever there is pending timed work (scrollbar fade, agent rescan,
        // debounced autosave, dead-pane reaping).
        event_loop.set_control_flow(ControlFlow::Wait);
        event_loop.run_app(&mut self)?;
        Ok(())
    }

    pub fn open_preferences_window(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(pref) = &self.preferences_window {
            pref.window.focus_window();
            pref.window.request_redraw();
            return;
        }

        match pollster::block_on(PreferencesWindow::create(
            event_loop,
            &self.config_manager.current_config,
        )) {
            Ok(pref) => {
                info!("Opened dedicated Preferences Window (ID: {:?})", pref.id());
                self.preferences_window = Some(pref);
            }
            Err(e) => {
                error!("Failed to open Preferences Window: {:?}", e);
            }
        }
    }

    pub fn build_pty_options(&self) -> PtySpawnOptions {
        let term_cfg = &self.config_manager.current_config.terminal;
        let (shell, args) = if let Some(s) = &term_cfg.shell {
            (Some(s.program.clone()), s.args.clone())
        } else {
            #[cfg(target_os = "macos")]
            let default_args = vec!["-l".to_string()];
            #[cfg(not(target_os = "macos"))]
            let default_args = Vec::new();
            (None, default_args)
        };

        let inherited_cwd = self
            .active_session_id
            .and_then(|wid| self.sessions.get(&wid))
            .and_then(|s| s.workspace.active_tab())
            .and_then(|t| t.active_terminal())
            .and_then(|term| term.current_working_directory());

        let cwd = inherited_cwd.or_else(|| term_cfg.working_directory.clone());

        let wakeup_cb = self.event_proxy.as_ref().map(|p| {
            let p = p.clone();
            let pending = Arc::clone(&self.pty_wakeup_pending);
            Arc::new(move || {
                // Coalesce: only send on a false->true transition. The event loop clears
                // the flag before draining, so at most one wakeup is ever in flight even
                // when several reader threads are producing output at full speed.
                if !pending.swap(true, Ordering::AcqRel) {
                    let _ = p.send_event(AppEvent::PtyWakeup);
                }
            }) as Arc<dyn Fn() + Send + Sync>
        });

        PtySpawnOptions {
            shell,
            args,
            cwd,
            term: Some(term_cfg.term.clone()),
            env: term_cfg.env.clone(),
            wakeup_cb,
        }
    }

    /// On macOS, native tabs are hosted in the system titlebar, so the internal canvas tab bar height is 0.
    /// On Windows/Linux, return the inline tab bar height if multiple tabs exist.
    pub fn get_tab_bar_height(&self) -> f32 {
        let tab_bar_style = self
            .config_manager
            .current_config
            .window
            .tab_bar_style
            .as_str();
        let tab_count = self
            .active_session()
            .map(|s| s.workspace.tabs.len())
            .unwrap_or(0);
        let scale = self
            .active_session()
            .map(|s| s.renderer.scale_factor)
            .unwrap_or(1.0);

        match tab_bar_style {
            "never" => 0.0,
            "always" => 36.0 * scale,
            _ => {
                if tab_count > 1 {
                    36.0 * scale
                } else {
                    0.0
                }
            }
        }
    }

    pub fn active_session(&self) -> Option<&TerminalSession> {
        self.active_session_id.and_then(|id| self.sessions.get(&id))
    }

    pub fn active_session_mut(&mut self) -> Option<&mut TerminalSession> {
        if let Some(id) = self.active_session_id {
            self.sessions.get_mut(&id)
        } else {
            None
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::PtyWakeup => {
                // Clear before draining: any output that arrives while we are polling
                // will flip the flag again and schedule another wakeup, so nothing is lost.
                self.pty_wakeup_pending.store(false, Ordering::Release);
                let active_session = self.active_session_id;
                for (id, session) in self.sessions.iter_mut() {
                    let outcome = session.workspace.poll_all_ptys();
                    if outcome.any_output {
                        session.flush_osc52_clipboard(self.clipboard.as_ref());
                    }
                    if outcome.any_output && active_session != Some(*id) {
                        session.mark_unseen_output();
                    }
                    // Only repaint if something visible changed. Output in a
                    // background tab updates that tab's buffer and activity dot
                    // without touching what is currently on screen.
                    //
                    // Paced rather than immediate: this arm fires every few
                    // milliseconds under load. `about_to_wait` runs straight after
                    // `user_event` and arms the deadline for anything held back.
                    if outcome.needs_redraw() {
                        session.request_output_redraw();
                    }
                }
            }
            // Nothing to do here: waking the loop is the whole point, and
            // `about_to_wait` drains the config channel right after this returns.
            AppEvent::ConfigChanged => {}
        }
    }

    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.sessions.is_empty() {
            if let Err(e) = self.create_terminal_session(event_loop) {
                error!("Failed to create initial terminal window: {:?}", e);
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::ModifiersChanged(ref new_modifiers) = event {
            self.modifiers = new_modifiers.state();
        }

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
                            // Originates from the user editing the prefs UI -> persist.
                            self.apply_config_live(&new_cfg, true);
                        }
                        return;
                    }
                    _ => {
                        let res = pref.egui_state.on_window_event(&pref.window, &event);
                        if res.repaint {
                            pref.window.request_redraw();
                        }
                        // No `check_changed` here on purpose. egui is immediate-mode:
                        // the form fields can only be mutated while the UI closure
                        // runs, which happens inside `render_frame` on the
                        // `RedrawRequested` arm above -- and that already ends with a
                        // `check_changed`. Calling it here as well compared state that
                        // provably had not changed since the last frame, at the cost of
                        // a full `Config` deep clone (15+ Strings plus the env map) on
                        // every event, mouse motion included.
                        return;
                    }
                }
            }
        }

        // 2. Track Focused Window
        if let WindowEvent::Focused(true) = event {
            self.active_session_id = Some(window_id);
            // Selecting a tab is the user seeing it, so retire its activity dot.
            if let Some(session) = self.sessions.get_mut(&window_id) {
                session.clear_unseen_output();
                #[cfg(target_os = "macos")]
                {
                    let win_ptr = get_nswindow(&session.window);
                    unsafe {
                        center_macos_window_title(win_ptr);
                        style_macos_tab_bar(win_ptr);
                    }
                }
            }
        }

        // 3. Handle Terminal Window Session Events
        let tab_bar_h = self.get_tab_bar_height();

        match event {
            WindowEvent::CloseRequested => self.handle_close_requested(event_loop, window_id),

            WindowEvent::Resized(physical_size) => {
                self.handle_resized(window_id, tab_bar_h, physical_size)
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.handle_scale_factor_changed(window_id, tab_bar_h, scale_factor)
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.handle_modifiers_changed(window_id, new_modifiers)
            }

            WindowEvent::Ime(ime_event) => self.handle_ime(window_id, ime_event),

            WindowEvent::KeyboardInput { ref event, .. } => {
                self.handle_keyboard_input(event_loop, window_id, tab_bar_h, event)
            }

            WindowEvent::CursorMoved { position, .. } => {
                self.handle_cursor_moved(window_id, tab_bar_h, position)
            }

            WindowEvent::MouseInput { state, button, .. } => {
                self.handle_mouse_input(event_loop, window_id, tab_bar_h, state, button)
            }

            WindowEvent::MouseWheel { delta, .. } => {
                self.handle_mouse_wheel(window_id, tab_bar_h, delta)
            }

            WindowEvent::RedrawRequested => self.handle_redraw_requested(window_id, tab_bar_h),

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.on_about_to_wait(event_loop);
    }

    fn exiting(&mut self, _event_loop: &ActiveEventLoop) {
        self.on_exiting();
    }
}
