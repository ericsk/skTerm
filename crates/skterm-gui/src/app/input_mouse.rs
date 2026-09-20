//! Pointer event handling: selection, splitters, scrollbars, tab bar, and hyperlinks.

use super::events::RESIZE_REFLOW_DEBOUNCE;
#[cfg(target_os = "macos")]
use super::macos::get_nswindow;
use super::session::ScrollbarDragState;
use super::App;
use crate::layout::SplitDirection;
use skterm_core::{Selection, SelectionMode};
use skterm_render::{AgentPanelHit, AgentPanelLayout, ScrollbarMetrics};
use std::time::{Duration, Instant};
use tracing::{error, info};
use winit::dpi::PhysicalPosition;
#[cfg(not(target_os = "macos"))]
use winit::dpi::{LogicalPosition, LogicalSize, Position, Size};
use winit::event::{ElementState, MouseButton, MouseScrollDelta};
use winit::event_loop::ActiveEventLoop;
use winit::window::{CursorIcon, WindowId};

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

impl App {
    /// Handles `WindowEvent::CursorMoved` for a terminal window.
    pub(crate) fn handle_cursor_moved(
        &mut self,
        window_id: WindowId,
        tab_bar_h: f32,
        position: PhysicalPosition<f64>,
    ) {
        if let Some(session) = self.sessions.get_mut(&window_id) {
            session.mouse_pos = (position.x as f32, position.y as f32);
            let (mx, my) = session.mouse_pos;

            // 0.4 Handling Scrollbar Dragging
            if let Some(drag) = session.dragging_scrollbar {
                if let Some(tab) = session.workspace.active_tab_mut() {
                    if let Some(term) = tab.root.find_terminal_mut(drag.pane_id) {
                        let active_buf = term.grid_mut().active_mut();
                        let scrollback_len = active_buf.scrollback.len();
                        if scrollback_len > 0 && drag.travel > 0.0 {
                            let target_thumb_y = my - drag.drag_offset_y;
                            let clamped_thumb_y =
                                target_thumb_y.clamp(drag.track_y, drag.track_y + drag.travel);
                            let progress = (clamped_thumb_y - drag.track_y) / drag.travel;
                            let new_offset =
                                ((1.0 - progress) * scrollback_len as f32).round() as usize;
                            active_buf.scroll_offset = new_offset.min(scrollback_len);
                            session.last_scroll_time = Some(Instant::now());
                            session.window.request_redraw();
                        }
                    }
                }
                session.window.set_cursor(CursorIcon::Default);
                return;
            }

            // 0.5 Handling Agents Panel Resizing Dragging
            if session.dragging_sidebar {
                let scale = session.renderer.scale_factor;
                // The panel is docked right, so its width grows as the
                // pointer moves *away* from the right edge.
                session.workspace.sidebar_width = AgentPanelLayout::width_for_pointer(
                    session.renderer.config.width as f32,
                    mx,
                    scale,
                );
                // Same throttle as the splitter drag below: the panel edge
                // follows the pointer every event, the pane reflow does not.
                // The release arm performs the exact final sync.
                if session.last_drag_reflow.elapsed() >= RESIZE_REFLOW_DEBOUNCE {
                    session.last_drag_reflow = Instant::now();
                    session.sync_all_pane_sizes(tab_bar_h);
                }
                session.window.request_redraw();
                return;
            }

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
                // Cheap: just moves the divider, so the splitter tracks the
                // mouse with no lag.
                session.workspace.set_split_ratio(split_id, new_ratio);
                // Expensive: reflows every pane and signals every child.
                // Throttled; the release arm does the exact final sync.
                if session.last_drag_reflow.elapsed() >= RESIZE_REFLOW_DEBOUNCE {
                    session.last_drag_reflow = Instant::now();
                    session.sync_all_pane_sizes(tab_bar_h);
                }
                session.window.request_redraw();
                return;
            }

            // 2. Handling Text Selection Dragging
            if session.is_mouse_down {
                if let Some((_pane_id, row, col)) = session.get_pane_and_cell_at(mx, my, tab_bar_h)
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
                // 3. Hover testing for Scrollbar, Splitters, and Hyperlinks to update cursor icon
                let width = session.renderer.config.width as f32;
                let height = session.renderer.config.height as f32;

                // Must match the press handler: splitters live inside the
                // content area, which shrinks while the agents panel is open.
                let bounds = session.workspace.content_bounds(
                    width,
                    height,
                    tab_bar_h,
                    session.renderer.scale_factor,
                );

                // 3.0 Check Scrollbar Hover
                let scale = session.renderer.scale_factor;
                let pane_rects = session
                    .workspace
                    .compute_active_pane_rects_with_scale(width, height, tab_bar_h, scale);

                let mut over_scrollbar_pane = None;
                if let Some(active_tab) = session.workspace.active_tab() {
                    for (id, rect) in &pane_rects {
                        if let Some(term) = active_tab.root.find_terminal(*id) {
                            let grid = term.grid();
                            if !grid.is_alternate {
                                let active_buf = grid.active();
                                if let Some(metrics) = ScrollbarMetrics::compute(
                                    rect.x,
                                    rect.y,
                                    rect.width,
                                    rect.height,
                                    scale,
                                    active_buf.scrollback.len(),
                                    active_buf.rows,
                                    active_buf.scroll_offset,
                                    false,
                                    false,
                                ) {
                                    if mx >= metrics.hit_x
                                        && mx <= rect.x + rect.width
                                        && my >= metrics.hit_y
                                        && my <= metrics.hit_y + metrics.hit_h
                                    {
                                        over_scrollbar_pane = Some(*id);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }

                if over_scrollbar_pane != session.hovered_scrollbar_pane {
                    session.hovered_scrollbar_pane = over_scrollbar_pane;
                    session.window.request_redraw();
                }

                if session.hovered_scrollbar_pane.is_some() {
                    session.window.set_cursor(CursorIcon::Default);
                    return;
                }

                #[cfg(target_os = "macos")]
                let is_link_modifier = self.modifiers.super_key();
                #[cfg(not(target_os = "macos"))]
                let is_link_modifier = self.modifiers.control_key();

                let mut is_over_splitter = false;
                if let Some(panel) = session.agent_panel_layout(tab_bar_h) {
                    if panel.hit(mx, my) == AgentPanelHit::ResizeHandle {
                        is_over_splitter = true;
                        session.window.set_cursor(CursorIcon::ColResize);
                    }
                }
                if !is_over_splitter {
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

    /// Handles `WindowEvent::MouseInput` for a terminal window.
    pub(crate) fn handle_mouse_input(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        tab_bar_h: f32,
        state: ElementState,
        button: MouseButton,
    ) {
        // Inline Tab Bar handling (main window area only, excluding right sidebar)
        if button == MouseButton::Left && state == ElementState::Pressed && tab_bar_h > 0.0 {
            #[allow(clippy::enum_variant_names)]
            enum TabAction {
                NewTab,
                CloseTab(usize),
                SelectTab(usize),
            }

            let mut action = None;
            if let Some(session) = self.sessions.get(&window_id) {
                let (mx, my) = session.mouse_pos;
                let scale = session.renderer.scale_factor;
                let reserved_right = if session.workspace.sidebar_open {
                    AgentPanelLayout::reserved_width(session.workspace.sidebar_width * scale, scale)
                } else {
                    0.0
                };
                let main_w = (session.renderer.config.width as f32 - reserved_right).max(0.0);

                if my <= tab_bar_h && mx < main_w {
                    let tabs_len = session.workspace.tabs.len();
                    let start_x = 8.0 * scale;
                    let plus_btn_w = 24.0 * scale;
                    let tab_gap = 4.0 * scale;
                    let avail_w = (main_w - start_x - plus_btn_w - 16.0 * scale).max(80.0);
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
                            session.last_tab_label_refresh = None;
                        }
                        let new_tab_bar_h = self.get_tab_bar_height();
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.sync_all_pane_sizes(new_tab_bar_h);
                            session.window.request_redraw();
                        }
                    }
                    TabAction::CloseTab(idx) => {
                        let mut all_closed = false;
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            all_closed = session.workspace.close_tab_at(idx);
                            session.last_tab_label_refresh = None;
                        }
                        if all_closed {
                            self.sessions.remove(&window_id);
                            self.session_order.retain(|&id| id != window_id);
                            if self.sessions.is_empty() {
                                event_loop.exit();
                            }
                        } else {
                            let new_tab_bar_h = self.get_tab_bar_height();
                            if let Some(session) = self.sessions.get_mut(&window_id) {
                                session.sync_all_pane_sizes(new_tab_bar_h);
                                session.window.request_redraw();
                            }
                        }
                    }
                    TabAction::SelectTab(idx) => {
                        if let Some(session) = self.sessions.get_mut(&window_id) {
                            session.workspace.select_tab(idx);
                            session.last_tab_label_refresh = None;
                            session.window.request_redraw();
                        }
                    }
                }
                return;
            }
        }

        let mut close_panel_clicked = false;
        let mut clicked_agent_idx: Option<usize> = None;
        let mut spawn_agent_pane = false;
        let mut swallowed_by_panel = false;

        if let Some(session) = self.sessions.get_mut(&window_id) {
            let (mx, my) = session.mouse_pos;

            if button == MouseButton::Left && state == ElementState::Pressed {
                // 0. Check the global agents panel, whose geometry
                // comes from the same layout the renderer drew with.
                if let Some(panel) = session.agent_panel_layout(tab_bar_h) {
                    match panel.hit(mx, my) {
                        AgentPanelHit::ResizeHandle => {
                            session.dragging_sidebar = true;
                            // See the splitter drag-start: make the first
                            // move of this drag reflow immediately.
                            session.last_drag_reflow = Instant::now() - RESIZE_REFLOW_DEBOUNCE;
                            return;
                        }
                        AgentPanelHit::CloseButton => {
                            close_panel_clicked = true;
                        }
                        // Defer the spawn until the `session`
                        // borrow ends below: `build_pty_options`
                        // reads `self.sessions`.
                        AgentPanelHit::NewButton => spawn_agent_pane = true,
                        AgentPanelHit::Card(idx) => clicked_agent_idx = Some(idx),
                        AgentPanelHit::Background => swallowed_by_panel = true,
                        AgentPanelHit::Outside => {}
                    }
                }
            }
        }

        // Clicks on inert panel chrome must not fall through to the
        // terminal and move the cursor or start a selection.
        if swallowed_by_panel {
            if self.editing_agent_pane.is_some() {
                self.commit_agent_label_edit();
            }
            return;
        }

        if close_panel_clicked {
            if self.editing_agent_pane.is_some() {
                self.commit_agent_label_edit();
            }
            self.toggle_sidebar();
            return;
        }

        if spawn_agent_pane {
            // Built here rather than at the top of the handler: this clones the
            // shell args, TERM and the whole env map, so doing it for every
            // mouse press and release was pure waste.
            let pty_opts = self.build_pty_options();
            let scrollback = self.config_manager.current_config.terminal.scrollback_lines;

            if let Some(session) = self.sessions.get_mut(&window_id) {
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
            }
            if self.editing_agent_pane.is_some() {
                self.commit_agent_label_edit();
            }
            return;
        }

        if let Some(idx) = clicked_agent_idx {
            let all_agents = self.scan_all_ai_agents();
            if let Some((target_wid, agent)) = all_agents.get(idx).cloned() {
                let is_same_editing =
                    self.editing_agent_pane == Some((target_wid, agent.tab_id, agent.pane_id));

                let is_double_or_right_click = if button == MouseButton::Right {
                    true
                } else if let Some(s) = self.sessions.get(&window_id) {
                    Instant::now().duration_since(s.last_click_time) < Duration::from_millis(400)
                } else {
                    false
                };

                if let Some(s) = self.sessions.get_mut(&window_id) {
                    s.last_click_time = Instant::now();
                }

                if is_same_editing && !is_double_or_right_click {
                    return;
                }

                if self.editing_agent_pane.is_some() {
                    self.commit_agent_label_edit();
                }

                if is_double_or_right_click {
                    // Start inline task rename on this agent card
                    let current = self
                        .sessions
                        .get(&target_wid)
                        .and_then(|s| s.workspace.tabs.iter().find(|t| t.id == agent.tab_id))
                        .and_then(|t| t.root.find_terminal(agent.pane_id))
                        .and_then(|term| term.custom_label.clone())
                        .unwrap_or_default();

                    self.editing_agent_pane = Some((target_wid, agent.tab_id, agent.pane_id));
                    self.editing_agent_text = current;
                    self.editing_agent_cursor = self.editing_agent_text.chars().count();
                    if let Some(s) = self.sessions.get(&window_id) {
                        #[cfg(target_os = "macos")]
                        {
                            // Position the native field over the card's
                            // title row using the same layout the panel
                            // was drawn with, converted back to points.
                            let scale = s.renderer.scale_factor as f64;
                            let (logical_x, logical_y, logical_w) =
                                match s.agent_panel_layout(tab_bar_h) {
                                    Some(panel) => {
                                        let card = panel.card_rect(idx);
                                        (
                                            (card.x as f64) / scale + 24.0,
                                            (card.y as f64) / scale + 7.0,
                                            (((card.w as f64) / scale) - 34.0).max(90.0),
                                        )
                                    }
                                    None => (24.0, 24.0, 90.0),
                                };
                            let logical_h = 22.0;
                            let win = s.window.clone();
                            let _ = s;
                            self.macos_editor.open_for_card(
                                &win,
                                (target_wid, agent.tab_id, agent.pane_id),
                                &self.editing_agent_text,
                                logical_x,
                                logical_y,
                                logical_w,
                                logical_h,
                            );
                            win.request_redraw();
                        }
                        #[cfg(not(target_os = "macos"))]
                        {
                            let scale = s.renderer.scale_factor as f64;
                            let (mx, my) = s.mouse_pos;
                            let logical_pos = LogicalPosition::new(
                                (mx as f64 / scale).max(16.0),
                                (my as f64 / scale) + 12.0,
                            );
                            let logical_size = LogicalSize::new(180.0, 22.0);
                            s.window.set_ime_cursor_area(
                                Position::Logical(logical_pos),
                                Size::Logical(logical_size),
                            );
                            s.window.request_redraw();
                        }
                    }
                    return;
                }

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
                        target_session.last_tab_label_refresh = None;
                    }
                    if let Some(tab) = target_session.workspace.active_tab_mut() {
                        tab.active_pane_id = agent.pane_id;
                        if tab.zoomed_pane_id.is_some() && tab.zoomed_pane_id != Some(agent.pane_id)
                        {
                            tab.zoomed_pane_id = Some(agent.pane_id);
                        }
                    }
                }
                let new_tab_bar_h = self.get_tab_bar_height();
                if let Some(target_session) = self.sessions.get_mut(&target_wid) {
                    target_session.sync_all_pane_sizes(new_tab_bar_h);
                    target_session.window.request_redraw();
                }
            }
            return;
        }

        if button == MouseButton::Left
            && state == ElementState::Pressed
            && self.editing_agent_pane.is_some()
        {
            self.commit_agent_label_edit();
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
                            session.dragging_splitter =
                                Some((splitter.split_id, splitter.direction, splitter.parent_rect));
                            // Backdate the clock so the very first move of
                            // this drag reflows immediately instead of
                            // inheriting the tail of an earlier drag.
                            session.last_drag_reflow = Instant::now() - RESIZE_REFLOW_DEBOUNCE;
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
                                        info!(
                                            "Clicked hyperlink, opening in default browser: {}",
                                            url
                                        );
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

                    // 2.5 Check Scrollbar Click / Drag Start
                    let mut scrollbar_clicked = false;
                    let pane_rects = session
                        .workspace
                        .compute_active_pane_rects_with_scale(width, height, tab_bar_h, scale);
                    if let Some(active_tab) = session.workspace.active_tab_mut() {
                        for (id, rect) in &pane_rects {
                            if let Some(term) = active_tab.root.find_terminal_mut(*id) {
                                let (is_alt, scrollback_len, rows, scroll_offset) = {
                                    let grid = term.grid();
                                    let buf = grid.active();
                                    (
                                        grid.is_alternate,
                                        buf.scrollback.len(),
                                        buf.rows,
                                        buf.scroll_offset,
                                    )
                                };
                                if !is_alt && scrollback_len > 0 {
                                    if let Some(metrics) = ScrollbarMetrics::compute(
                                        rect.x,
                                        rect.y,
                                        rect.width,
                                        rect.height,
                                        scale,
                                        scrollback_len,
                                        rows,
                                        scroll_offset,
                                        true,
                                        false,
                                    ) {
                                        if mx >= metrics.hit_x
                                            && mx <= rect.x + rect.width
                                            && my >= metrics.hit_y
                                            && my <= metrics.hit_y + metrics.hit_h
                                        {
                                            scrollbar_clicked = true;
                                            active_tab.active_pane_id = *id;

                                            let (target_thumb_y, drag_offset_y) = if my
                                                >= metrics.thumb_y
                                                && my <= metrics.thumb_y + metrics.thumb_h
                                            {
                                                (metrics.thumb_y, my - metrics.thumb_y)
                                            } else {
                                                let target_y = (my - metrics.thumb_h / 2.0).clamp(
                                                    metrics.track_y,
                                                    metrics.track_y + metrics.travel,
                                                );
                                                (target_y, metrics.thumb_h / 2.0)
                                            };

                                            if metrics.travel > 0.0 {
                                                let progress = (target_thumb_y - metrics.track_y)
                                                    / metrics.travel;
                                                let new_offset = ((1.0 - progress)
                                                    * scrollback_len as f32)
                                                    .round()
                                                    as usize;
                                                term.grid_mut().active_mut().scroll_offset =
                                                    new_offset.min(scrollback_len);
                                            }

                                            session.dragging_scrollbar = Some(ScrollbarDragState {
                                                pane_id: *id,
                                                track_y: metrics.track_y,
                                                travel: metrics.travel,
                                                drag_offset_y,
                                            });
                                            session.last_scroll_time = Some(Instant::now());
                                            session.window.request_redraw();
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if scrollbar_clicked {
                        return;
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

                            if let Some(terminal) = tab.active_terminal_mut() {
                                match session.click_count {
                                    2 => terminal.grid_mut().active_mut().select_word_at(row, col),
                                    3 => terminal.grid_mut().active_mut().select_line_at(row),
                                    _ => {
                                        terminal.grid_mut().active_mut().selection =
                                            Some(Selection::new(
                                                (row, col),
                                                (row, col),
                                                SelectionMode::Simple,
                                            ));
                                    }
                                }
                                session.window.request_redraw();
                            }
                        }
                    }
                } else {
                    session.is_mouse_down = false;
                    if session.dragging_scrollbar.is_some() {
                        session.dragging_scrollbar = None;
                        session.window.request_redraw();
                    }
                    // Both drags throttle their reflow while the button is
                    // held (see `RESIZE_REFLOW_DEBOUNCE`), so these two
                    // syncs are unconditional: they are what guarantees the
                    // panes match the final divider position exactly.
                    if session.dragging_sidebar {
                        session.dragging_sidebar = false;
                        session.sync_all_pane_sizes(tab_bar_h);
                        session.window.request_redraw();
                    }
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

    /// Handles `WindowEvent::MouseWheel` for a terminal window.
    pub(crate) fn handle_mouse_wheel(
        &mut self,
        window_id: WindowId,
        tab_bar_h: f32,
        delta: MouseScrollDelta,
    ) {
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
                        if terminal.state.mouse_protocol != skterm_core::vt::MouseProtocol::None {
                            let button = if lines > 0 { 64 } else { 65 };
                            let (row, col) = cell_pos;
                            let mut payload = Vec::new();
                            for _ in 0..lines.unsigned_abs() {
                                if terminal.state.mouse_format == skterm_core::vt::MouseFormat::Sgr
                                {
                                    payload.extend_from_slice(
                                        format!("\x1b[<{};{};{}M", button, col, row).as_bytes(),
                                    );
                                } else {
                                    let cb = (button + 32) as u8;
                                    let cx = (col.min(223) + 32) as u8;
                                    let cy = (row.min(223) + 32) as u8;
                                    payload.extend_from_slice(&[0x1b, b'[', b'M', cb, cx, cy]);
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
                            session.last_scroll_time = Some(Instant::now());
                            session.window.request_redraw();
                        }
                    }
                }
            }
        }
    }
}
