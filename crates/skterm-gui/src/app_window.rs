//! Main Application Event Loop, Multi-Window Session Handler, Native macOS Tabbing,
//! Split Panes, Clipboard, CJK IME, and Native Preferences Window.

#![allow(unexpected_cfgs)]

use crate::config::{parse_hex_color, Config, ConfigManager, Theme};
use crate::input::encode_key_event;
use crate::layout::{PaneId, Rect, SplitDirection, Workspace};
use crate::preferences_window::PreferencesWindow;
use arboard::Clipboard;
use skterm_core::{PtySpawnOptions, Selection, SelectionMode};
use skterm_render::{PaneRenderItem, RenderTheme, TabItem, TermRenderer};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, Position, Size};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Fullscreen, Window, WindowAttributes, WindowId};

#[cfg(target_os = "macos")]
fn get_nswindow(window: &Window) -> *mut objc::runtime::Object {
    use objc::{msg_send, sel, sel_impl};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = window.window_handle() {
        if let RawWindowHandle::AppKit(appkit) = handle.as_raw() {
            let view_ptr = appkit.ns_view.as_ptr();
            let win_ptr: *mut objc::runtime::Object =
                unsafe { msg_send![view_ptr as *mut objc::runtime::Object, window] };
            return win_ptr;
        }
    }
    std::ptr::null_mut()
}

/// Open a URL using the default system browser in a cross-platform manner.
pub fn open_url_in_browser(url: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open").arg(url).spawn()?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()?;
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::process::Command::new("xdg-open").arg(url).spawn()?;
    }
    Ok(())
}

/// Terminal Window Session representing an active window (or native macOS tab).
pub struct TerminalSession {
    pub window: Arc<Window>,
    pub renderer: TermRenderer,
    pub workspace: Workspace,
    pub process_cache: crate::layout::ProcessTreeCache,
    pub mouse_pos: (f32, f32),
    pub is_mouse_down: bool,
    pub last_click_time: Instant,
    pub click_count: usize,
    pub dragging_splitter: Option<(u32, SplitDirection, Rect)>,
    pub hovered_url: Option<String>,
    pub scroll_pixel_accumulator: f64,
}

impl TerminalSession {
    pub fn update_ime_position(&self, tab_bar_h: f32) {
        if let Some(active_tab) = self.workspace.active_tab() {
            if let Some(terminal) = active_tab.active_terminal() {
                let pane_rects = self.workspace.compute_active_pane_rects_with_scale(
                    self.renderer.config.width as f32,
                    self.renderer.config.height as f32,
                    tab_bar_h,
                    self.renderer.scale_factor,
                );

                for (id, rect) in &pane_rects {
                    if *id == active_tab.active_pane_id {
                        let cursor = &terminal.grid().active().cursor;
                        let cell_w = self.renderer.cell_width();
                        let cell_h = self.renderer.cell_height();

                        let cursor_px =
                            rect.x + self.renderer.padding_x + (cursor.col as f32 * cell_w);
                        let cursor_py =
                            rect.y + self.renderer.padding_y + (cursor.row as f32 * cell_h);

                        let scale = self.renderer.scale_factor as f64;
                        let logical_pos = LogicalPosition::new(
                            cursor_px as f64 / scale,
                            cursor_py as f64 / scale,
                        );
                        let logical_size =
                            LogicalSize::new(cell_w as f64 / scale, cell_h as f64 / scale);

                        self.window.set_ime_cursor_area(
                            Position::Logical(logical_pos),
                            Size::Logical(logical_size),
                        );
                        break;
                    }
                }
            }
        }
    }

    pub fn copy_to_clipboard(&self, clipboard: Option<&Arc<Mutex<Clipboard>>>) {
        if let Some(active_tab) = self.workspace.active_tab() {
            if let Some(terminal) = active_tab.active_terminal() {
                let text = terminal.grid().active().extract_selected_text();
                if !text.is_empty() {
                    if let Some(cb) = clipboard {
                        if let Ok(mut clip) = cb.lock() {
                            let _ = clip.set_text(text);
                            info!("Copied selected text to system clipboard");
                        }
                    }
                }
            }
        }
    }

    pub fn paste_from_clipboard(&self, clipboard: Option<&Arc<Mutex<Clipboard>>>) {
        if let Some(cb) = clipboard {
            if let Ok(mut clip) = cb.lock() {
                if let Ok(text) = clip.get_text() {
                    if !text.is_empty() {
                        if let Some(active_tab) = self.workspace.active_tab() {
                            if let Some(terminal) = active_tab.active_terminal() {
                                let bracketed = terminal.state.bracketed_paste;
                                let payload = if bracketed {
                                    format!("\x1b[200~{}\x1b[201~", text)
                                } else {
                                    text
                                };
                                let _ = terminal.write_to_pty(payload.as_bytes());
                                self.window.request_redraw();
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn get_pane_and_cell_at(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        tab_bar_h: f32,
    ) -> Option<(PaneId, usize, usize)> {
        let pane_rects = self.workspace.compute_active_pane_rects_with_scale(
            self.renderer.config.width as f32,
            self.renderer.config.height as f32,
            tab_bar_h,
            self.renderer.scale_factor,
        );

        for (id, rect) in pane_rects {
            if rect.contains(mouse_x, mouse_y) {
                let cell_w = self.renderer.cell_width();
                let cell_h = self.renderer.cell_height();
                let pad_x = self.renderer.padding_x;
                let pad_y = self.renderer.padding_y;

                let rel_x = (mouse_x - rect.x - pad_x).max(0.0);
                let rel_y = (mouse_y - rect.y - pad_y).max(0.0);

                let col = (rel_x / cell_w).floor() as usize;
                let row = (rel_y / cell_h).floor() as usize;

                return Some((id, row, col));
            }
        }
        None
    }

    pub fn sync_all_pane_sizes(&mut self, tab_bar_h: f32) {
        self.workspace.sync_pane_sizes_with_scale(
            self.renderer.config.width as f32,
            self.renderer.config.height as f32,
            self.renderer.cell_width(),
            self.renderer.cell_height(),
            self.renderer.padding_x,
            self.renderer.padding_y,
            tab_bar_h,
            self.renderer.scale_factor,
        );
    }
}

/// skTerm GUI Application instance managing multiple native window sessions or native tabs.
pub struct App {
    pub config_manager: ConfigManager,
    pub preferences_window: Option<PreferencesWindow>,
    pub sessions: HashMap<WindowId, TerminalSession>,
    pub session_order: Vec<WindowId>,
    pub active_session_id: Option<WindowId>,
    pub sidebar_open: bool,
    pub clipboard: Option<Arc<Mutex<Clipboard>>>,
    pub modifiers: ModifiersState,
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
            sessions: HashMap::new(),
            session_order: Vec::new(),
            active_session_id: None,
            sidebar_open: false,
            clipboard,
            modifiers: ModifiersState::default(),
            pending_geometry_save: false,
            last_resize_time: None,
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_open = !self.sidebar_open;
        let tab_bar_h = self.get_tab_bar_height();
        for session in self.sessions.values_mut() {
            session.workspace.sidebar_open = self.sidebar_open;
            session.sync_all_pane_sizes(tab_bar_h);
            session.window.request_redraw();
        }
    }

    pub fn scan_all_ai_agents(&mut self) -> Vec<(WindowId, skterm_render::AgentSidebarItem)> {
        let mut all_agents = Vec::new();
        let active_wid = self.active_session_id;
        let session_ids = self.session_order.clone();

        for (s_idx, wid) in session_ids.iter().enumerate() {
            if let Some(session) = self.sessions.get_mut(wid) {
                let mut agents = session
                    .workspace
                    .scan_ai_agents(&mut session.process_cache);
                for agent in &mut agents {
                    if session_ids.len() > 1 {
                        agent.location_label = format!("T{}•#{}", s_idx + 1, agent.pane_id);
                    }
                    if Some(*wid) != active_wid {
                        agent.is_focused = false;
                    }
                    all_agents.push((*wid, agent.clone()));
                }
            }
        }

        all_agents
    }

    pub fn run(mut self) -> anyhow::Result<()> {
        let event_loop = EventLoop::new()?;
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
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

    pub fn build_render_theme(&self) -> RenderTheme {
        let config = &self.config_manager.current_config;
        let preset = Theme::get(&config.colors.theme);
        let mut render_theme = RenderTheme {
            default_fg: preset.foreground,
            default_bg: preset.background,
            cursor_color: preset.cursor,
            selection_color: preset.selection,
            tab_bar_bg: [
                preset.background[0] * 0.85,
                preset.background[1] * 0.85,
                preset.background[2] * 0.85,
                1.0,
            ],
            tab_active_bg: preset.background,
            tab_active_fg: preset.foreground,
            tab_inactive_fg: [
                preset.foreground[0] * 0.6,
                preset.foreground[1] * 0.6,
                preset.foreground[2] * 0.6,
                1.0,
            ],
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

            if let Some(c) = palette_cfg.black.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[0] = c;
            }
            if let Some(c) = palette_cfg.red.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[1] = c;
            }
            if let Some(c) = palette_cfg.green.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[2] = c;
            }
            if let Some(c) = palette_cfg.yellow.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[3] = c;
            }
            if let Some(c) = palette_cfg.blue.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[4] = c;
            }
            if let Some(c) = palette_cfg
                .magenta
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[5] = c;
            }
            if let Some(c) = palette_cfg.cyan.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[6] = c;
            }
            if let Some(c) = palette_cfg.white.as_ref().and_then(|h| parse_hex_color(h)) {
                pal[7] = c;
            }
            if let Some(c) = palette_cfg
                .bright_black
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[8] = c;
            }
            if let Some(c) = palette_cfg
                .bright_red
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[9] = c;
            }
            if let Some(c) = palette_cfg
                .bright_green
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[10] = c;
            }
            if let Some(c) = palette_cfg
                .bright_yellow
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[11] = c;
            }
            if let Some(c) = palette_cfg
                .bright_blue
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[12] = c;
            }
            if let Some(c) = palette_cfg
                .bright_magenta
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[13] = c;
            }
            if let Some(c) = palette_cfg
                .bright_cyan
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[14] = c;
            }
            if let Some(c) = palette_cfg
                .bright_white
                .as_ref()
                .and_then(|h| parse_hex_color(h))
            {
                pal[15] = c;
            }

            render_theme.ansi_palette = Some(pal);
        }

        render_theme
    }

    pub fn apply_config_live(&mut self, new_cfg: &Config) {
        info!("Applying live configuration update to all terminal sessions...");
        let old_font = self.config_manager.current_config.font.clone();
        let font_changed = old_font.family != new_cfg.font.family
            || (old_font.size - new_cfg.font.size).abs() > 0.01
            || old_font.fallback_families != new_cfg.font.fallback_families;

        self.config_manager.current_config = new_cfg.clone();
        let render_theme = self.build_render_theme();
        let tab_bar_h = self.get_tab_bar_height();

        let cursor_shape = match new_cfg.cursor.shape.as_str() {
            "beam" => skterm_core::CursorShape::Beam,
            "underline" => skterm_core::CursorShape::Underline,
            _ => skterm_core::CursorShape::Block,
        };

        for session in self.sessions.values_mut() {
            if font_changed {
                let _ = session.renderer.update_font(
                    &new_cfg.font.family,
                    new_cfg.font.size,
                    &new_cfg.font.fallback_families,
                );
            }
            session
                .renderer
                .update_padding(new_cfg.window.padding_x, new_cfg.window.padding_y);
            session.renderer.update_opacity(new_cfg.window.opacity);
            session.renderer.set_theme(render_theme);
            session.workspace.update_cursor_shape(cursor_shape);
            session.sync_all_pane_sizes(tab_bar_h);
            session.window.request_redraw();
        }

        // Auto-save to config.toml in background thread
        if let Ok(toml_str) = toml::to_string_pretty(new_cfg) {
            let path = self.config_manager.path.clone();
            std::thread::spawn(move || {
                let _ = std::fs::write(&path, toml_str);
            });
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

        PtySpawnOptions {
            shell,
            args,
            cwd: term_cfg.working_directory.clone(),
            term: Some(term_cfg.term.clone()),
            env: term_cfg.env.clone(),
        }
    }

    /// On macOS, native tabs are hosted in the system titlebar, so the internal canvas tab bar height is 0.
    /// On Windows/Linux, return the inline tab bar height if multiple tabs exist.
    pub fn get_tab_bar_height(&self) -> f32 {
        #[cfg(target_os = "macos")]
        {
            // 100% native macOS NSWindow Tabs! Terminal Grid keeps full window height.
            0.0
        }
        #[cfg(not(target_os = "macos"))]
        {
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
                "auto" | _ => {
                    if tab_count > 1 {
                        36.0 * scale
                    } else {
                        0.0
                    }
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

    /// Create a new Terminal Session. On macOS, this window attaches as a native NSWindow tab!
    pub fn create_terminal_session(
        &mut self,
        event_loop: &ActiveEventLoop,
    ) -> anyhow::Result<WindowId> {
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
            window_attrs = window_attrs
                .with_tabbing_identifier("skterm.tabs")
                .with_title_hidden(true)
                .with_titlebar_transparent(true);
        }

        match self
            .config_manager
            .current_config
            .window
            .decorations
            .as_str()
        {
            "none" => {
                window_attrs = window_attrs.with_decorations(false);
            }
            "transparent" => {
                window_attrs = window_attrs.with_transparent(true);
            }
            _ => {}
        }

        let window = Arc::new(event_loop.create_window(window_attrs)?);
        window.set_ime_allowed(true);

        match self
            .config_manager
            .current_config
            .window
            .startup_mode
            .as_str()
        {
            "maximized" => window.set_maximized(true),
            "fullscreen" => window.set_fullscreen(Some(Fullscreen::Borderless(None))),
            _ => {}
        }

        // On macOS: Enable NSWindowTabbingModePreferred and merge with parent tabbed window
        #[cfg(target_os = "macos")]
        {
            use objc::{msg_send, sel, sel_impl};

            let new_ptr = get_nswindow(&window);
            if !new_ptr.is_null() {
                // NSWindowTabbingModePreferred = 2
                let _: () = unsafe { msg_send![new_ptr, setTabbingMode: 2isize] };
            }

            if let Some(active_id) = self.active_session_id {
                if let Some(active_sess) = self.sessions.get(&active_id) {
                    let parent_ptr = get_nswindow(&active_sess.window);
                    if !parent_ptr.is_null() && !new_ptr.is_null() {
                        // NSWindowAbove = 1
                        let _: () = unsafe {
                            msg_send![
                                parent_ptr,
                                addTabbedWindow: new_ptr
                                ordered: 1isize
                            ]
                        };
                    }
                }
            }
        }

        let font_cfg = &self.config_manager.current_config.font;
        let mut renderer = pollster::block_on(TermRenderer::new_with_font(
            window.clone(),
            font_size_pt,
            &font_cfg.family,
            &font_cfg.fallback_families,
        ))?;

        let scale = renderer.scale_factor;
        let cell_w_logical = renderer.cell_width() / scale;
        let cell_h_logical = renderer.cell_height() / scale;
        let target_logical_w = (init_cols * cell_w_logical + pad_x * 2.0).ceil();
        let target_logical_h = (init_rows * cell_h_logical + pad_y * 2.0).ceil();
        let _ = window.request_inner_size(LogicalSize::new(
            target_logical_w as f64,
            target_logical_h as f64,
        ));

        let pty_opts = self.build_pty_options();
        let scrollback = self.config_manager.current_config.terminal.scrollback_lines;

        let mut workspace = Workspace::new_with_options(
            init_rows as usize,
            init_cols as usize,
            scrollback,
            pty_opts,
        )?;
        workspace.sidebar_open = self.sidebar_open;

        let render_theme = self.build_render_theme();
        renderer.set_theme(render_theme);

        let tab_bar_h = self.get_tab_bar_height();
        workspace.sync_pane_sizes(
            renderer.config.width as f32,
            renderer.config.height as f32,
            renderer.cell_width(),
            renderer.cell_height(),
            renderer.padding_x,
            renderer.padding_y,
            tab_bar_h,
        );

        let window_id = window.id();
        let session = TerminalSession {
            window: window.clone(),
            renderer,
            workspace,
            process_cache: crate::layout::ProcessTreeCache::new(),
            mouse_pos: (0.0, 0.0),
            is_mouse_down: false,
            last_click_time: Instant::now(),
            click_count: 0,
            dragging_splitter: None,
            hovered_url: None,
            scroll_pixel_accumulator: 0.0,
        };

        self.sessions.insert(window_id, session);
        self.session_order.push(window_id);
        self.active_session_id = Some(window_id);
        info!("Initialized Terminal Window Session (ID: {:?})", window_id);
        Ok(window_id)
    }
}

impl ApplicationHandler for App {
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

        // 2. Track Focused Window
        if let WindowEvent::Focused(true) = event {
            self.active_session_id = Some(window_id);
        }

        // 3. Handle Terminal Window Session Events
        let tab_bar_h = self.get_tab_bar_height();

        match event {
            WindowEvent::CloseRequested => {
                info!("Terminal window close requested for {:?}", window_id);
                self.sessions.remove(&window_id);
                self.session_order.retain(|&id| id != window_id);
                if self.sessions.is_empty() {
                    info!("All terminal windows closed, exiting.");
                    event_loop.exit();
                } else if self.active_session_id == Some(window_id) {
                    self.active_session_id = self.sessions.keys().next().copied();
                }
            }

            WindowEvent::Resized(physical_size) => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    session
                        .renderer
                        .resize(physical_size.width, physical_size.height);
                    session.sync_all_pane_sizes(tab_bar_h);

                    if let Some(active_tab) = session.workspace.active_tab() {
                        if let Some(term) = active_tab.active_terminal() {
                            let new_cols = term.grid().active().cols as u16;
                            let new_rows = term.grid().active().rows as u16;
                            let current_cols = self.config_manager.current_config.window.columns;
                            let current_rows = self.config_manager.current_config.window.rows;
                            if new_cols != current_cols || new_rows != current_rows {
                                self.config_manager.current_config.window.columns = new_cols;
                                self.config_manager.current_config.window.rows = new_rows;
                                self.pending_geometry_save = true;
                                self.last_resize_time = Some(Instant::now());
                            }
                        }
                    }
                    session.window.request_redraw();
                }
            }

            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    let _ = session.renderer.update_scale_factor(scale_factor as f32);
                    session.sync_all_pane_sizes(tab_bar_h);
                    session.window.request_redraw();
                }
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                self.modifiers = new_modifiers.state();

                #[cfg(target_os = "macos")]
                let is_link_modifier = self.modifiers.super_key();
                #[cfg(not(target_os = "macos"))]
                let is_link_modifier = self.modifiers.control_key();

                if let Some(session) = self.sessions.get_mut(&window_id) {
                    if session.dragging_splitter.is_none() {
                        if is_link_modifier && session.hovered_url.is_some() {
                            session.window.set_cursor(CursorIcon::Pointer);
                        } else {
                            session.window.set_cursor(CursorIcon::Default);
                        }
                    }
                }
            }

            WindowEvent::Ime(ime_event) => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    match ime_event {
                        Ime::Commit(text) => {
                            if let Some(tab) = session.workspace.active_tab_mut() {
                                if let Some(term) = tab.active_terminal_mut() {
                                    let _ = term.write_to_pty(text.as_bytes());
                                    session.window.request_redraw();
                                }
                            }
                        }
                        Ime::Preedit(_text, _cursor) => {
                            session.window.request_redraw();
                        }
                        _ => {}
                    }
                }
            }

            WindowEvent::KeyboardInput { ref event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                #[cfg(target_os = "macos")]
                let is_cmd = self.modifiers.super_key();

                #[cfg(not(target_os = "macos"))]
                let is_cmd = self.modifiers.control_key() && self.modifiers.shift_key();

                let is_shift = self.modifiers.shift_key();

                if is_cmd {
                    // Check NamedKey first (Enter, Arrow keys)
                    match event.logical_key {
                        Key::Named(NamedKey::Enter) if is_shift => {
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.workspace.toggle_zoom_active_pane();
                                session.sync_all_pane_sizes(tab_bar_h);
                                session.window.request_redraw();
                                return;
                            }
                        }
                        Key::Named(NamedKey::ArrowLeft) if self.modifiers.alt_key() => {
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.workspace.adjust_active_pane_ratio(-0.05);
                                session.sync_all_pane_sizes(tab_bar_h);
                                session.window.request_redraw();
                                return;
                            }
                        }
                        Key::Named(NamedKey::ArrowRight) if self.modifiers.alt_key() => {
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.workspace.adjust_active_pane_ratio(0.05);
                                session.sync_all_pane_sizes(tab_bar_h);
                                session.window.request_redraw();
                                return;
                            }
                        }
                        Key::Named(NamedKey::ArrowUp) if self.modifiers.alt_key() => {
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.workspace.adjust_active_pane_ratio(-0.05);
                                session.sync_all_pane_sizes(tab_bar_h);
                                session.window.request_redraw();
                                return;
                            }
                        }
                        Key::Named(NamedKey::ArrowDown) if self.modifiers.alt_key() => {
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.workspace.adjust_active_pane_ratio(0.05);
                                session.sync_all_pane_sizes(tab_bar_h);
                                session.window.request_redraw();
                                return;
                            }
                        }
                        _ => {}
                    }

                    if let Key::Character(ref s) = event.logical_key {
                        let lower = s.to_lowercase();
                        match lower.as_str() {
                            "," => {
                                self.open_preferences_window(event_loop);
                                return;
                            }
                            "z" if is_shift => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    session.workspace.toggle_zoom_active_pane();
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                    return;
                                }
                            }
                            "a" if is_shift => {
                                self.toggle_sidebar();
                                return;
                            }
                            "t" => {
                                // macOS Native Tab (or new window)
                                #[cfg(target_os = "macos")]
                                {
                                    let _ = self.create_terminal_session(event_loop);
                                    return;
                                }
                                #[cfg(not(target_os = "macos"))]
                                {
                                    let pty_opts = self.build_pty_options();
                                    let scrollback = self
                                        .config_manager
                                        .current_config
                                        .terminal
                                        .scrollback_lines;
                                    if let Some(session) = self.sessions.get_mut(&window_id) {
                                        let _ = session
                                            .workspace
                                            .new_tab_with_options(24, 80, scrollback, pty_opts);
                                        session.sync_all_pane_sizes(tab_bar_h);
                                        session.window.request_redraw();
                                    }
                                    return;
                                }
                            }
                            "w" => {
                                // Close Pane or Window Session
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    let all_panes_closed = session.workspace.close_active_pane();
                                    if all_panes_closed {
                                        self.sessions.remove(&window_id);
                                        self.session_order.retain(|&id| id != window_id);
                                        if self.sessions.is_empty() {
                                            event_loop.exit();
                                        }
                                    } else {
                                        session.sync_all_pane_sizes(tab_bar_h);
                                        session.window.request_redraw();
                                    }
                                }
                                return;
                            }
                            "d" => {
                                // Split Panes: Cmd+D = Left/Right, Cmd+Shift+D = Top/Bottom
                                let dir = if is_shift {
                                    SplitDirection::Vertical
                                } else {
                                    SplitDirection::Horizontal
                                };
                                let pty_opts = self.build_pty_options();
                                let scrollback =
                                    self.config_manager.current_config.terminal.scrollback_lines;
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    let _ = session
                                        .workspace
                                        .split_active_pane(dir, 24, 80, scrollback, pty_opts);
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                                return;
                            }
                            "[" => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    if self.modifiers.alt_key() {
                                        session.workspace.move_tab_left();
                                    } else if is_shift {
                                        session.workspace.prev_tab();
                                    } else if let Some(tab) = session.workspace.active_tab_mut() {
                                        tab.focus_prev();
                                    }
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                                return;
                            }
                            "]" => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    if self.modifiers.alt_key() {
                                        session.workspace.move_tab_right();
                                    } else if is_shift {
                                        session.workspace.next_tab();
                                    } else if let Some(tab) = session.workspace.active_tab_mut() {
                                        tab.focus_next();
                                    }
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                                return;
                            }
                            "c" => {
                                if let Some(session) = self.sessions.get(&window_id) {
                                    session.copy_to_clipboard(self.clipboard.as_ref());
                                }
                                return;
                            }
                            "v" => {
                                if let Some(session) = self.sessions.get(&window_id) {
                                    session.paste_from_clipboard(self.clipboard.as_ref());
                                }
                                return;
                            }
                            "+" | "=" => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    if self.modifiers.alt_key() {
                                        session.workspace.equalize_active_panes();
                                    } else {
                                        let cur_pt = session.renderer.font_size_pt;
                                        let _ = session.renderer.update_font_size(cur_pt + 1.0);
                                    }
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                                return;
                            }
                            "-" => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    let cur_pt = session.renderer.font_size_pt;
                                    let _ =
                                        session.renderer.update_font_size((cur_pt - 1.0).max(8.0));
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                                return;
                            }
                            "0" => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    let def_pt = self.config_manager.current_config.font.size;
                                    let _ = session.renderer.update_font_size(def_pt);
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                                return;
                            }
                            _ => {
                                if let Ok(digit) = lower.parse::<usize>() {
                                    if (1..=9).contains(&digit) {
                                        if let Some(session) = self.sessions.get_mut(&window_id) {
                                            session.workspace.select_tab(digit - 1);
                                            session.window.request_redraw();
                                        }
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }

                // Send Normal Key to PTY
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    if let Some(tab) = session.workspace.active_tab_mut() {
                        if let Some(term) = tab.active_terminal_mut() {
                            let app_cursor = term.state.application_cursor_keys;
                            if let Some(bytes) = encode_key_event(event, self.modifiers, app_cursor)
                            {
                                let _ = term.write_to_pty(&bytes);
                                session.window.request_redraw();
                            }
                        }
                    }
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    session.mouse_pos = (position.x as f32, position.y as f32);
                    let (mx, my) = session.mouse_pos;

                    // 1. Handling Splitter Dragging
                    if let Some((split_id, direction, parent_rect)) = session.dragging_splitter {
                        let new_ratio = match direction {
                            SplitDirection::Horizontal => {
                                ((mx - parent_rect.x) / parent_rect.width).clamp(0.05, 0.95)
                            }
                            SplitDirection::Vertical => {
                                ((my - parent_rect.y) / parent_rect.height).clamp(0.05, 0.95)
                            }
                        };
                        session.workspace.set_split_ratio(split_id, new_ratio);
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                        return;
                    }

                    // 2. Handling Text Selection Dragging
                    if session.is_mouse_down {
                        if let Some((_pane_id, row, col)) =
                            session.get_pane_and_cell_at(mx, my, tab_bar_h)
                        {
                            if let Some(tab) = session.workspace.active_tab_mut() {
                                if let Some(term) = tab.active_terminal_mut() {
                                    if let Some(sel) = &mut term.grid_mut().active_mut().selection {
                                        sel.end = (row, col);
                                        session.window.request_redraw();
                                    }
                                }
                            }
                        }
                    } else {
                        // 3. Hover testing for Splitters and Hyperlinks to update cursor icon
                        let width = session.renderer.config.width as f32;
                        let height = session.renderer.config.height as f32;
                        let bounds = Rect::new(0.0, tab_bar_h, width, height - tab_bar_h);

                        #[cfg(target_os = "macos")]
                        let is_link_modifier = self.modifiers.super_key();
                        #[cfg(not(target_os = "macos"))]
                        let is_link_modifier = self.modifiers.control_key();

                        let mut is_over_splitter = false;
                        if let Some(tab) = session.workspace.active_tab() {
                            if let Some(splitter) = tab.find_splitter_at(bounds, mx, my) {
                                is_over_splitter = true;
                                let icon = match splitter.direction {
                                    SplitDirection::Horizontal => CursorIcon::ColResize,
                                    SplitDirection::Vertical => CursorIcon::RowResize,
                                };
                                session.window.set_cursor(icon);
                            }
                        }

                        if !is_over_splitter {
                            // Check hyperlink under cursor
                            let detected_url = if let Some((pane_id, row, col)) =
                                session.get_pane_and_cell_at(mx, my, tab_bar_h)
                            {
                                if let Some(tab) = session.workspace.active_tab() {
                                    tab.root
                                        .find_terminal(pane_id)
                                        .and_then(|t| t.url_at(row, col))
                                } else {
                                    None
                                }
                            } else {
                                None
                            };

                            session.hovered_url = detected_url;

                            if is_link_modifier && session.hovered_url.is_some() {
                                session.window.set_cursor(CursorIcon::Pointer);
                            } else {
                                session.window.set_cursor(CursorIcon::Default);
                            }
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                // Inline Tab Bar handling (Windows/Linux only)
                #[cfg(not(target_os = "macos"))]
                if button == MouseButton::Left && state == ElementState::Pressed && tab_bar_h > 0.0
                {
                    enum TabAction {
                        NewTab,
                        CloseTab(usize),
                        SelectTab(usize),
                    }

                    let mut action = None;
                    if let Some(session) = self.sessions.get(&window_id) {
                        let (mx, my) = session.mouse_pos;
                        if my <= tab_bar_h {
                            let scale = session.renderer.scale_factor;
                            let tabs_len = session.workspace.tabs.len();
                            let start_x = 8.0 * scale;
                            let plus_btn_w = 24.0 * scale;
                            let tab_gap = 4.0 * scale;
                            let avail_w = (session.renderer.config.width as f32
                                - start_x
                                - plus_btn_w
                                - 16.0 * scale)
                                .max(80.0);
                            let total_gaps = (tabs_len.saturating_sub(1) as f32) * tab_gap;
                            let tab_w = ((avail_w - total_gaps) / tabs_len as f32)
                                .clamp(75.0 * scale, 190.0 * scale);
                            let tab_h = (tab_bar_h - 8.0 * scale).max(18.0);
                            let tab_y = (tab_bar_h - tab_h) / 2.0;

                            let last_tab_right = start_x + (tabs_len as f32 * (tab_w + tab_gap));
                            let plus_btn_x = last_tab_right + 2.0 * scale;
                            if mx >= plus_btn_x - 4.0 * scale
                                && mx <= plus_btn_x + plus_btn_w + 6.0 * scale
                                && my >= tab_y
                                && my <= tab_y + tab_h
                            {
                                action = Some(TabAction::NewTab);
                            } else {
                                for idx in 0..tabs_len {
                                    let tab_x = start_x + (idx as f32 * (tab_w + tab_gap));
                                    if mx >= tab_x
                                        && mx <= tab_x + tab_w
                                        && my >= tab_y
                                        && my <= tab_y + tab_h
                                    {
                                        let close_size = 14.0 * scale;
                                        let close_x = tab_x + tab_w - close_size - 6.0 * scale;
                                        if mx >= close_x - 4.0 * scale {
                                            action = Some(TabAction::CloseTab(idx));
                                        } else {
                                            action = Some(TabAction::SelectTab(idx));
                                        }
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    if let Some(act) = action {
                        match act {
                            TabAction::NewTab => {
                                let pty_opts = self.build_pty_options();
                                let scrollback =
                                    self.config_manager.current_config.terminal.scrollback_lines;
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    let _ = session
                                        .workspace
                                        .new_tab_with_options(24, 80, scrollback, pty_opts);
                                    session.sync_all_pane_sizes(tab_bar_h);
                                    session.window.request_redraw();
                                }
                            }
                            TabAction::CloseTab(idx) => {
                                let mut all_closed = false;
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    all_closed = session.workspace.close_tab_at(idx);
                                    if !all_closed {
                                        session.sync_all_pane_sizes(tab_bar_h);
                                        session.window.request_redraw();
                                    }
                                }
                                if all_closed {
                                    self.sessions.remove(&window_id);
                                    self.session_order.retain(|&id| id != window_id);
                                    if self.sessions.is_empty() {
                                        event_loop.exit();
                                    }
                                }
                            }
                            TabAction::SelectTab(idx) => {
                                if let Some(session) = self.sessions.get_mut(&window_id) {
                                    session.workspace.select_tab(idx);
                                    session.window.request_redraw();
                                }
                            }
                        }
                        return;
                    }
                }

                let pty_opts = self.build_pty_options();
                let scrollback = self
                    .config_manager
                    .current_config
                    .terminal
                    .scrollback_lines;

                let mut toggle_sidebar_clicked = false;
                let mut clicked_agent_idx: Option<usize> = None;

                if let Some(session) = self.sessions.get_mut(&window_id) {
                    let (mx, my) = session.mouse_pos;

                    if button == MouseButton::Left {
                        if state == ElementState::Pressed {
                            let scale = session.renderer.scale_factor;

                            // 0. Check AI Sidebar Toggle Button click (in Tab Bar)
                            if tab_bar_h > 0.0 && my <= tab_bar_h {
                                let base_x = if cfg!(target_os = "macos") {
                                    78.0 * scale
                                } else {
                                    8.0 * scale
                                };
                                let ai_btn_w = 34.0 * scale;
                                if mx >= base_x && mx <= base_x + ai_btn_w {
                                    toggle_sidebar_clicked = true;
                                }
                            }

                            // 0.5 Check AI Sidebar Panel click when open
                            if !toggle_sidebar_clicked && session.workspace.sidebar_open {
                                let sidebar_px = session.workspace.sidebar_width * scale;
                                if mx <= sidebar_px && my >= tab_bar_h {
                                    let header_top = tab_bar_h;
                                    let header_h = 38.0 * scale;

                                    // Check "+ New" button click
                                    let new_btn_w = 56.0 * scale;
                                    let new_btn_h = 22.0 * scale;
                                    let new_btn_x = sidebar_px - new_btn_w - 10.0 * scale;
                                    let new_btn_y = header_top + (header_h - new_btn_h) / 2.0;
                                    if mx >= new_btn_x
                                        && mx <= new_btn_x + new_btn_w
                                        && my >= new_btn_y
                                        && my <= new_btn_y + new_btn_h
                                    {
                                        let _ = session.workspace.split_active_pane(
                                            SplitDirection::Horizontal,
                                            24,
                                            80,
                                            scrollback,
                                            pty_opts,
                                        );
                                        session.sync_all_pane_sizes(tab_bar_h);
                                        if let Some(tab) = session.workspace.active_tab_mut() {
                                            if let Some(term) = tab.active_terminal_mut() {
                                                let _ = term.write_to_pty(b"agy\r");
                                            }
                                        }
                                        session.window.request_redraw();
                                        return;
                                    }

                                    // Check Agent Card click
                                    let card_h = 66.0 * scale;
                                    let card_gap = 6.0 * scale;
                                    let list_start_y = header_top + header_h + 8.0 * scale;
                                    if my >= list_start_y {
                                        let idx = ((my - list_start_y) / (card_h + card_gap))
                                            .floor()
                                            as usize;
                                        clicked_agent_idx = Some(idx);
                                    } else {
                                        return;
                                    }
                                }
                            }
                        }
                    }
                }

                if toggle_sidebar_clicked {
                    self.toggle_sidebar();
                    return;
                }

                if let Some(idx) = clicked_agent_idx {
                    let all_agents = self.scan_all_ai_agents();
                    if let Some((target_wid, agent)) = all_agents.get(idx).cloned() {
                        self.active_session_id = Some(target_wid);
                        if let Some(target_session) = self.sessions.get_mut(&target_wid) {
                            if target_wid != window_id {
                                target_session.window.focus_window();
                                #[cfg(target_os = "macos")]
                                {
                                    let ns_win = get_nswindow(&target_session.window);
                                    if !ns_win.is_null() {
                                        use objc::{msg_send, sel, sel_impl};
                                        let _: () = unsafe {
                                            msg_send![
                                                ns_win,
                                                makeKeyAndOrderFront: std::ptr::null::<objc::runtime::Object>()
                                            ]
                                        };
                                    }
                                }
                            }
                            if let Some(t_pos) = target_session
                                .workspace
                                .tabs
                                .iter()
                                .position(|t| t.id == agent.tab_id)
                            {
                                target_session.workspace.select_tab(t_pos);
                            }
                            if let Some(tab) = target_session.workspace.active_tab_mut() {
                                tab.active_pane_id = agent.pane_id;
                                if tab.zoomed_pane_id.is_some()
                                    && tab.zoomed_pane_id != Some(agent.pane_id)
                                {
                                    tab.zoomed_pane_id = Some(agent.pane_id);
                                }
                            }
                            target_session.window.request_redraw();
                        }
                    }
                    return;
                }

                if let Some(session) = self.sessions.get_mut(&window_id) {
                    let (mx, my) = session.mouse_pos;

                    if button == MouseButton::Left {
                        if state == ElementState::Pressed {
                            let scale = session.renderer.scale_factor;

                            // 1. Check Splitter Hit Test First
                            let width = session.renderer.config.width as f32;
                            let height = session.renderer.config.height as f32;
                            let bounds = session
                                .workspace
                                .content_bounds(width, height, tab_bar_h, scale);
                            if let Some(tab) = session.workspace.active_tab() {
                                if let Some(splitter) = tab.find_splitter_at(bounds, mx, my) {
                                    session.dragging_splitter = Some((
                                        splitter.split_id,
                                        splitter.direction,
                                        splitter.parent_rect,
                                    ));
                                    return;
                                }
                            }

                            // 2. Check Hyperlink Click (Cmd+Click on macOS, Ctrl+Click on Linux/Windows)
                            #[cfg(target_os = "macos")]
                            let is_link_modifier = self.modifiers.super_key();
                            #[cfg(not(target_os = "macos"))]
                            let is_link_modifier = self.modifiers.control_key();

                            if is_link_modifier {
                                if let Some((pane_id, row, col)) =
                                    session.get_pane_and_cell_at(mx, my, tab_bar_h)
                                {
                                    if let Some(tab) = session.workspace.active_tab() {
                                        if let Some(term) = tab.root.find_terminal(pane_id) {
                                            if let Some(url) = term.url_at(row, col) {
                                                info!("Clicked hyperlink, opening in default browser: {}", url);
                                                if let Err(e) = open_url_in_browser(&url) {
                                                    error!("Failed to open hyperlink: {:?}", e);
                                                }
                                                // Prevent text selection
                                                return;
                                            }
                                        }
                                    }
                                }
                            }

                            session.is_mouse_down = true;

                            // Click inside Terminal Pane
                            if let Some((pane_id, row, col)) =
                                session.get_pane_and_cell_at(mx, my, tab_bar_h)
                            {
                                if let Some(tab) = session.workspace.active_tab_mut() {
                                    tab.active_pane_id = pane_id;

                                    let now = Instant::now();
                                    if now.duration_since(session.last_click_time)
                                        < Duration::from_millis(400)
                                    {
                                        session.click_count += 1;
                                    } else {
                                        session.click_count = 1;
                                    }
                                    session.last_click_time = now;

                                    let mode = match session.click_count {
                                        2 => SelectionMode::Word,
                                        3 => SelectionMode::Line,
                                        _ => SelectionMode::Simple,
                                    };

                                    if let Some(terminal) = tab.active_terminal_mut() {
                                        terminal.grid_mut().active_mut().selection =
                                            Some(Selection::new((row, col), (row, col), mode));
                                        session.window.request_redraw();
                                    }
                                }
                            }
                        } else {
                            session.is_mouse_down = false;
                            if session.dragging_splitter.is_some() {
                                session.dragging_splitter = None;
                                session.sync_all_pane_sizes(tab_bar_h);
                                session.window.request_redraw();
                            }
                        }
                    } else if button == MouseButton::Right && state == ElementState::Pressed {
                        session.paste_from_clipboard(self.clipboard.as_ref());
                    }
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(session) = self.sessions.get_mut(&window_id) {
                    let lines = match delta {
                        MouseScrollDelta::LineDelta(_, y) => {
                            session.scroll_pixel_accumulator = 0.0;
                            y as i32
                        }
                        MouseScrollDelta::PixelDelta(pos) => {
                            let scale = session.renderer.cell_height().max(1.0) as f64;
                            session.scroll_pixel_accumulator += pos.y;
                            let lines = (session.scroll_pixel_accumulator / scale) as i32;
                            session.scroll_pixel_accumulator -= lines as f64 * scale;
                            lines
                        }
                    };

                    if lines != 0 {
                        let (mx, my) = session.mouse_pos;
                        let cell_pos = session
                            .get_pane_and_cell_at(mx, my, tab_bar_h)
                            .map(|(_, row, col)| (row + 1, col + 1))
                            .unwrap_or((1, 1));

                        if let Some(tab) = session.workspace.active_tab_mut() {
                            if let Some(terminal) = tab.active_terminal_mut() {
                                if terminal.state.mouse_protocol
                                    != skterm_core::vt::MouseProtocol::None
                                {
                                    let button = if lines > 0 { 64 } else { 65 };
                                    let (row, col) = cell_pos;
                                    let mut payload = Vec::new();
                                    for _ in 0..lines.unsigned_abs() {
                                        if terminal.state.mouse_format
                                            == skterm_core::vt::MouseFormat::Sgr
                                        {
                                            payload.extend_from_slice(
                                                format!("\x1b[<{};{};{}M", button, col, row)
                                                    .as_bytes(),
                                            );
                                        } else {
                                            let cb = (button + 32) as u8;
                                            let cx = (col.min(223) + 32) as u8;
                                            let cy = (row.min(223) + 32) as u8;
                                            payload.extend_from_slice(&[
                                                0x1b, b'[', b'M', cb, cx, cy,
                                            ]);
                                        }
                                    }
                                    let _ = terminal.write_to_pty(&payload);
                                } else if terminal.grid().is_alternate {
                                    let seq: &[u8] = if lines > 0 {
                                        if terminal.state.application_cursor_keys {
                                            b"\x1bOA"
                                        } else {
                                            b"\x1b[A"
                                        }
                                    } else if terminal.state.application_cursor_keys {
                                        b"\x1bOB"
                                    } else {
                                        b"\x1b[B"
                                    };
                                    let mut payload = Vec::new();
                                    for _ in 0..lines.unsigned_abs() {
                                        payload.extend_from_slice(seq);
                                    }
                                    let _ = terminal.write_to_pty(&payload);
                                } else {
                                    terminal
                                        .grid_mut()
                                        .active_mut()
                                        .scroll_display(lines as isize);
                                    session.window.request_redraw();
                                }
                            }
                        }
                    }
                }
            }

            WindowEvent::RedrawRequested => {
                let agents: Vec<skterm_render::AgentSidebarItem> = self
                    .scan_all_ai_agents()
                    .into_iter()
                    .map(|(_, a)| a)
                    .collect();

                if let Some(session) = self.sessions.get_mut(&window_id) {
                    session.update_ime_position(tab_bar_h);

                    let tabs: Vec<TabItem> = session
                        .workspace
                        .tabs
                        .iter()
                        .enumerate()
                        .map(|(idx, tab)| {
                            let title = if tab.is_zoomed() {
                                format!("{} [Z]", tab.title)
                            } else {
                                tab.title.clone()
                            };
                            TabItem {
                                title,
                                is_active: idx == session.workspace.active_tab_idx,
                            }
                        })
                        .collect();

                    let pane_rects = session.workspace.compute_active_pane_rects_with_scale(
                        session.renderer.config.width as f32,
                        session.renderer.config.height as f32,
                        tab_bar_h,
                        session.renderer.scale_factor,
                    );

                    let mut render_panes = Vec::new();
                    if let Some(active_tab) = session.workspace.active_tab() {
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

                    let sidebar_px = if session.workspace.sidebar_open {
                        session.workspace.sidebar_width * session.renderer.scale_factor
                    } else {
                        0.0
                    };

                    if let Err(e) = session.renderer.render_workspace_with_sidebar(
                        &tabs,
                        &render_panes,
                        tab_bar_h,
                        session.workspace.sidebar_open,
                        sidebar_px,
                        &agents,
                        None::<
                            fn(
                                &wgpu::Device,
                                &wgpu::Queue,
                                &wgpu::TextureView,
                                &wgpu::SurfaceConfiguration,
                            ),
                        >,
                    ) {
                        warn!("Render error on session {:?}: {:?}", window_id, e);
                    }

                    if let Some(active_tab) = session.workspace.active_tab_mut() {
                        for (id, _) in &pane_rects {
                            if let Some(t) = active_tab.root.find_terminal_mut(*id) {
                                t.grid_mut().clear_dirty();
                            }
                        }
                    }
                }
            }

            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // 1. Hot Reload Configuration
        let old_cfg = self.config_manager.current_config.clone();
        let reloaded = self.config_manager.check_reload().cloned();
        if let Some(reloaded_cfg) = reloaded {
            if old_cfg != reloaded_cfg {
                info!("Config file modified on disk, applying live changes to all sessions...");
                self.apply_config_live(&reloaded_cfg);
            }
        }

        // 2. Poll all sessions for PTY output and dead sessions
        let tab_bar_h = self.get_tab_bar_height();
        let mut dead_sessions = Vec::new();

        for (id, session) in self.sessions.iter_mut() {
            if session.workspace.has_dead_sessions() {
                let all_dead = session.workspace.cleanup_dead_sessions();
                if all_dead {
                    dead_sessions.push(*id);
                } else {
                    session.sync_all_pane_sizes(tab_bar_h);
                    session.window.request_redraw();
                }
            } else {
                let has_output = session.workspace.poll_all_ptys();
                if has_output {
                    session.window.request_redraw();
                }
            }
        }

        for id in dead_sessions {
            self.sessions.remove(&id);
            self.session_order.retain(|&wid| wid != id);
        }

        if self.sessions.is_empty() {
            info!("All terminal sessions closed. Exiting skTerm.");
            event_loop.exit();
            return;
        }

        // 3. Debounced Auto-Save
        if self.pending_geometry_save {
            if let Some(last) = self.last_resize_time {
                if last.elapsed() > Duration::from_millis(400) {
                    self.pending_geometry_save = false;
                    if let Ok(toml_str) =
                        toml::to_string_pretty(&self.config_manager.current_config)
                    {
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
