//! Window lifecycle: session creation and the window-management event arms.

#[cfg(target_os = "macos")]
use super::macos::{center_macos_window_title, get_nswindow, style_macos_tab_bar};
use super::pacer::FramePacer;
use super::session::{monitor_frame_interval, TerminalSession};
use super::App;
use crate::layout::Workspace;
use skterm_render::TermRenderer;
use std::sync::Arc;
use std::time::Instant;
use tracing::info;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::Modifiers;
use winit::event_loop::ActiveEventLoop;
use winit::window::{CursorIcon, Fullscreen, WindowAttributes, WindowId};

impl App {
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

        let existing_geometry = self
            .active_session_id
            .and_then(|id| self.sessions.get(&id))
            .map(|s| (s.window.inner_size(), s.workspace.sidebar_width));

        let mut window_attrs = WindowAttributes::default().with_title("skTerm");
        if let Some((phys_size, _)) = existing_geometry {
            window_attrs = window_attrs.with_inner_size(phys_size);
        } else {
            window_attrs = window_attrs.with_inner_size(LogicalSize::new(800.0, 500.0));
        }

        #[cfg(target_os = "macos")]
        {
            use winit::platform::macos::WindowAttributesExtMacOS;
            window_attrs = window_attrs.with_title_hidden(false);
            if self.config_manager.current_config.window.decorations == "transparent" {
                window_attrs = window_attrs.with_titlebar_transparent(true);
            }
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

        window_attrs = crate::icon::configure_window_attributes(window_attrs);

        let window = Arc::new(event_loop.create_window(window_attrs)?);
        crate::icon::apply_window_icon(&window);
        window.set_ime_allowed(true);

        if existing_geometry.is_none() {
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
        }

        // On macOS: Disallow native NSWindow tabbing so AppKit never inserts a
        // full-width NSTabBar above the right-hand Agents sidebar. Tabs are
        // managed inside the workspace and drawn in the left main area.
        #[cfg(target_os = "macos")]
        {
            use objc::{msg_send, sel, sel_impl};

            let new_ptr = get_nswindow(&window);
            if !new_ptr.is_null() {
                // NSWindowTabbingModeDisallowed = 2
                let _: () = unsafe { msg_send![new_ptr, setTabbingMode: 2isize] };
                // NSWindowToolbarStyleExpanded = 1 (forces centered window title on macOS 11+)
                let _: () = unsafe { msg_send![new_ptr, setToolbarStyle: 1isize] };
                unsafe {
                    center_macos_window_title(new_ptr);
                    crate::macos_titlebar::install_agent_toggle(new_ptr);
                    crate::macos_titlebar::set_agent_toggle_state(new_ptr, self.sidebar_open);
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

        if existing_geometry.is_none() {
            let scale = renderer.scale_factor;
            let cell_w_logical = renderer.cell_width() / scale;
            let cell_h_logical = renderer.cell_height() / scale;
            let target_logical_w = (init_cols * cell_w_logical + pad_x * 2.0).ceil();
            let target_logical_h = (init_rows * cell_h_logical + pad_y * 2.0).ceil();
            let _ = window.request_inner_size(LogicalSize::new(
                target_logical_w as f64,
                target_logical_h as f64,
            ));
        }

        let pty_opts = self.build_pty_options();
        let scrollback = self.config_manager.current_config.terminal.scrollback_lines;

        let mut workspace = Workspace::new_with_options(
            init_rows as usize,
            init_cols as usize,
            scrollback,
            pty_opts,
        )?;
        workspace.sidebar_open = self.sidebar_open;
        if let Some((_, sidebar_w)) = existing_geometry {
            workspace.sidebar_width = sidebar_w;
        }

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
            last_drag_reflow: Instant::now(),
            dragging_sidebar: false,
            dragging_scrollbar: None,
            hovered_scrollbar_pane: None,
            last_scroll_time: None,
            hovered_url: None,
            scroll_pixel_accumulator: 0.0,
            search_open: false,
            search_query: String::new(),
            search_matches: Vec::new(),
            active_search_idx: None,
            last_ime_cursor_area: None,
            last_tab_label_refresh: None,
            cached_tab_label: String::new(),
            cached_tab_tooltip: String::new(),
            has_unseen_output: false,
            pacer: FramePacer::new(monitor_frame_interval(&window), Instant::now()),
            menu_open: false,
            menu_hovered_idx: None,
        };

        self.sessions.insert(window_id, session);
        self.session_order.push(window_id);
        self.active_session_id = Some(window_id);
        info!("Initialized Terminal Window Session (ID: {:?})", window_id);
        Ok(window_id)
    }

    /// Handles `WindowEvent::CloseRequested` for a terminal window.
    pub(crate) fn handle_close_requested(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
    ) {
        info!("Terminal window close requested for {:?}", window_id);
        // Read the NSWindow pointer while the session still owns the
        // winit window -- it is unobtainable after the remove below.
        #[cfg(target_os = "macos")]
        if let Some(session) = self.sessions.get(&window_id) {
            crate::macos_titlebar::forget_window(get_nswindow(&session.window));
        }
        self.sessions.remove(&window_id);
        self.session_order.retain(|&id| id != window_id);
        if self.sessions.is_empty() {
            info!("All terminal windows closed, exiting.");
            event_loop.exit();
        } else if self.active_session_id == Some(window_id) {
            self.active_session_id = self.sessions.keys().next().copied();
        }
    }

    /// Handles `WindowEvent::Resized` for a terminal window.
    pub(crate) fn handle_resized(
        &mut self,
        window_id: WindowId,
        tab_bar_h: f32,
        physical_size: PhysicalSize<u32>,
    ) {
        if let Some(session) = self.sessions.get_mut(&window_id) {
            session
                .renderer
                .resize(physical_size.width, physical_size.height);
            session.sync_all_pane_sizes(tab_bar_h);

            let bounds = session.workspace.content_bounds(
                physical_size.width as f32,
                physical_size.height as f32,
                tab_bar_h,
                session.renderer.scale_factor,
            );
            let cell_w = session.renderer.cell_width();
            let cell_h = session.renderer.cell_height();
            let pad_x = session.renderer.padding_x;
            let pad_y = session.renderer.padding_y;
            let avail_w = (bounds.width - pad_x * 2.0).max(cell_w);
            let avail_h = (bounds.height - pad_y * 2.0).max(cell_h);
            let new_cols = (avail_w / cell_w).floor().max(1.0) as u16;
            let new_rows = (avail_h / cell_h).floor().max(1.0) as u16;
            let current_cols = self.config_manager.current_config.window.columns;
            let current_rows = self.config_manager.current_config.window.rows;
            if new_cols != current_cols || new_rows != current_rows {
                self.config_manager.current_config.window.columns = new_cols;
                self.config_manager.current_config.window.rows = new_rows;
                // Keep an open preferences form in step with the geometry we
                // just derived. Without this its stale copy would be written
                // straight back over the resize on the next event.
                if let Some(pref) = self.preferences_window.as_mut() {
                    pref.sync_window_geometry(new_cols, new_rows);
                }
                self.pending_config_save = true;
                self.last_config_change = Some(Instant::now());
            }
            #[cfg(target_os = "macos")]
            {
                let win_ptr = get_nswindow(&session.window);
                unsafe {
                    center_macos_window_title(win_ptr);
                    style_macos_tab_bar(win_ptr);
                }
            }
            session.window.request_redraw();
        }
    }

    /// Handles `WindowEvent::ScaleFactorChanged` for a terminal window.
    pub(crate) fn handle_scale_factor_changed(
        &mut self,
        window_id: WindowId,
        tab_bar_h: f32,
        scale_factor: f64,
    ) {
        if let Some(session) = self.sessions.get_mut(&window_id) {
            let _ = session.renderer.update_scale_factor(scale_factor as f32);
            session.sync_all_pane_sizes(tab_bar_h);
            session.window.request_redraw();
        }
    }

    /// Handles `WindowEvent::ModifiersChanged` for a terminal window.
    pub(crate) fn handle_modifiers_changed(
        &mut self,
        window_id: WindowId,
        new_modifiers: Modifiers,
    ) {
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
}
