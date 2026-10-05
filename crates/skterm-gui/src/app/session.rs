//! Per-window terminal session state: tabs, panes, selection, search, and IME.

use super::events::TAB_LABEL_REFRESH_INTERVAL;
use super::pacer::{frame_interval_from_millihertz, FramePacer, PacerPoll};
use crate::layout::{PaneId, Rect, SplitDirection, Workspace};
use arboard::Clipboard;
use skterm_render::{AgentPanelLayout, TermRenderer, TopRightButtons};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::info;
use winit::dpi::{LogicalPosition, LogicalSize, Position, Size};
use winit::window::Window;

/// Active drag state when user clicks and drags a pane's scrollbar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarDragState {
    pub pane_id: PaneId,
    pub track_y: f32,
    pub travel: f32,
    pub drag_offset_y: f32,
}

/// Terminal Window Session representing an active window (or native macOS tab).
pub struct TerminalSession {
    pub window: Arc<Window>,
    pub renderer: TermRenderer,
    pub workspace: Workspace,
    pub process_cache: crate::layout::ProcessTreeCache,
    pub mouse_pos: (f32, f32),
    pub is_mouse_down: bool,
    pub selection_anchor: Option<(usize, usize)>,
    pub has_dragged_selection: bool,
    pub last_click_time: Instant,
    pub click_count: usize,
    pub dragging_splitter: Option<(u32, SplitDirection, Rect)>,
    /// When the panes were last reflowed during the current splitter drag.
    ///
    /// See [`RESIZE_REFLOW_DEBOUNCE`](super::events::RESIZE_REFLOW_DEBOUNCE). Reset on mouse-down so the first move of a
    /// new drag is never swallowed by the tail of a previous one.
    pub last_drag_reflow: Instant,
    pub dragging_sidebar: bool,
    pub dragging_scrollbar: Option<ScrollbarDragState>,
    pub hovered_scrollbar_pane: Option<PaneId>,
    pub last_scroll_time: Option<Instant>,
    pub hovered_url: Option<String>,
    pub scroll_pixel_accumulator: f64,
    pub search_open: bool,
    pub search_query: String,
    pub search_matches: Vec<skterm_core::SearchMatch>,
    pub active_search_idx: Option<usize>,
    pub last_ime_cursor_area: Option<(i32, i32, i32, i32)>,
    /// Last time the native tab label was recomputed.
    ///
    /// Building the label costs a foreground-process lookup plus a cwd lookup
    /// per session. Cheap individually, but not worth paying every frame for a
    /// string that changes only when the user runs something or cds.
    pub last_tab_label_refresh: Option<Instant>,
    pub cached_tab_label: String,
    pub cached_tab_tooltip: String,
    /// PTY output arrived while this window was not the focused one.
    pub has_unseen_output: bool,
    /// Keeps PTY-driven repaints down to what this window's monitor can show.
    pub pacer: FramePacer,
    /// Whether the top dropdown or right-click context menu is currently open.
    pub menu_open: bool,
    /// True when `menu_open` is a right-click context menu rather than the top bar dropdown.
    pub menu_is_context: bool,
    /// Anchor position `(x, y)` in physical pixels when `menu_is_context` is true (or custom anchored).
    pub menu_anchor: (f32, f32),
    /// Currently hovered menu item index in the open menu.
    pub menu_hovered_idx: Option<usize>,
    /// Whether the pointer is in the top-right hover zone (reveals floating `[AI] [⚙]` pill when tab bar is hidden).
    pub top_right_hover: bool,
    /// Whether the pointer is hovering over the `[AI]` button.
    pub ai_btn_hovered: bool,
    /// Whether the pointer is hovering over the `[⚙]` Preferences button.
    pub prefs_btn_hovered: bool,
}

/// Refresh period of the monitor `window` currently sits on.
pub fn monitor_frame_interval(window: &Window) -> Duration {
    frame_interval_from_millihertz(
        window
            .current_monitor()
            .and_then(|m| m.refresh_rate_millihertz()),
    )
}

impl TerminalSession {
    /// Repaint because PTY output changed something visible, at most once per
    /// refresh period. See [`FramePacer`] for why this is a leading-edge
    /// throttle and what that has to do with typing latency.
    ///
    /// If the active terminal is mid-batch in DEC Mode `?2026` (Synchronized Output),
    /// hold the repaint until `?2026l` arrives or the 100ms safety timeout expires.
    pub fn request_output_redraw(&mut self) {
        if self
            .workspace
            .active_tab()
            .and_then(|t| t.active_terminal())
            .is_some_and(|term| term.state.is_synchronized_output_active())
        {
            return;
        }
        if self.pacer.on_output(Instant::now()) {
            self.window.request_redraw();
        }
    }

    /// Release a frame the pacer deferred, or report when it comes due so the
    /// caller can arm the event loop's timer. Also enforces the 100ms safety
    /// timeout for DEC Mode `?2026` (Synchronized Output).
    pub fn flush_paced_redraw(&mut self) -> Option<Instant> {
        let now = Instant::now();
        if let Some(term) = self
            .workspace
            .active_tab_mut()
            .and_then(|t| t.active_terminal_mut())
        {
            if term.state.synchronized_output {
                if let Some(since) = term.state.synchronized_output_since {
                    let timeout_at = since + Duration::from_millis(100);
                    if now < timeout_at {
                        return Some(timeout_at);
                    }
                }
                term.state.synchronized_output = false;
                term.state.synchronized_output_since = None;
                self.window.request_redraw();
                return None;
            }
        }
        match self.pacer.poll(now) {
            PacerPoll::Idle => None,
            PacerPoll::RedrawNow => {
                self.window.request_redraw();
                None
            }
            PacerPoll::WakeAt(due) => Some(due),
        }
    }

    /// Record that a frame is starting, whatever caused it.
    pub fn mark_frame_start(&mut self) {
        self.pacer.on_frame_start(Instant::now());
    }

    /// Re-read the monitor's refresh rate if it is due, so a window dragged to a
    /// faster display starts pacing to it.
    pub fn refresh_frame_interval(&mut self) {
        let now = Instant::now();
        if self.pacer.interval_refresh_due(now) {
            let interval = monitor_frame_interval(&self.window);
            self.pacer.set_interval(interval, now);
        }
    }

    /// Note that output arrived while this session was not the visible one.
    ///
    /// Resetting the throttle makes the indicator show up on the next pass of
    /// the event loop instead of up to [`TAB_LABEL_REFRESH_INTERVAL`] later.
    pub fn mark_unseen_output(&mut self) {
        if !self.has_unseen_output {
            self.has_unseen_output = true;
            self.last_tab_label_refresh = None;
        }
    }

    /// The user is now looking at this session, so its output is no longer unseen.
    pub fn clear_unseen_output(&mut self) {
        if self.has_unseen_output {
            self.has_unseen_output = false;
            self.last_tab_label_refresh = None;
        }
    }

    /// Recompute this window's native tab label, at most once per
    /// [`TAB_LABEL_REFRESH_INTERVAL`].
    ///
    /// Returns `Some((label, tooltip))` **only when the text actually changed**.
    /// In the steady state a terminal sits in one directory running one program,
    /// so this returns `None` and the caller skips the AppKit round-trip
    /// entirely.
    pub fn refresh_tab_label(
        &mut self,
        configured_shell: Option<&str>,
    ) -> Option<(String, String)> {
        let due = match self.last_tab_label_refresh {
            Some(t) => t.elapsed() >= TAB_LABEL_REFRESH_INTERVAL,
            None => true,
        };
        if !due {
            return None;
        }
        self.last_tab_label_refresh = Some(Instant::now());

        let home = std::env::var("HOME").ok();
        let active_idx = self.workspace.active_tab_idx;
        let mut active_label = None;
        let mut active_tooltip = None;
        let mut any_tab_changed = false;

        for (idx, tab) in self.workspace.tabs.iter_mut().enumerate() {
            if let Some(terminal) = tab.active_terminal() {
                let process = terminal.foreground_process_name();
                let cwd = terminal.current_working_directory();
                let fallback = format!("Tab {}", idx + 1);
                let mut label = crate::tab_title::build_tab_label(
                    process.as_deref(),
                    cwd.as_deref(),
                    home.as_deref(),
                    configured_shell,
                    &fallback,
                );
                if tab.has_unseen_output {
                    label.insert_str(0, "• ");
                }
                if tab.title != label {
                    tab.title = label.clone();
                    any_tab_changed = true;
                }
                if idx == active_idx {
                    let mut win_label = label;
                    if self.has_unseen_output && !win_label.starts_with("• ") {
                        win_label.insert_str(0, "• ");
                    }
                    let tooltip = cwd.unwrap_or_else(|| win_label.clone());
                    active_label = Some(win_label);
                    active_tooltip = Some(tooltip);
                }
            }
        }

        if any_tab_changed {
            self.window.request_redraw();
        }

        let label = active_label?;
        let tooltip = active_tooltip?;

        if label == self.cached_tab_label && tooltip == self.cached_tab_tooltip {
            return None;
        }
        self.cached_tab_label = label.clone();
        self.cached_tab_tooltip = tooltip.clone();
        Some((label, tooltip))
    }

    pub fn update_search_matches(&mut self) {
        self.search_matches = self
            .workspace
            .active_tab()
            .and_then(|t| t.active_terminal())
            .map(|term| term.grid().active().search(&self.search_query))
            .unwrap_or_default();

        if self.search_matches.is_empty() {
            self.active_search_idx = None;
        } else {
            let idx = self.search_matches.len() - 1;
            self.active_search_idx = Some(idx);
            let target_row = self.search_matches[idx].global_row;
            if let Some(tab) = self.workspace.active_tab_mut() {
                if let Some(term) = tab.active_terminal_mut() {
                    term.grid_mut()
                        .active_mut()
                        .scroll_to_global_row(target_row);
                }
            }
        }
    }

    pub fn next_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let next = match self.active_search_idx {
            Some(i) => (i + 1) % self.search_matches.len(),
            None => 0,
        };
        self.active_search_idx = Some(next);
        let target_row = self.search_matches[next].global_row;
        if let Some(tab) = self.workspace.active_tab_mut() {
            if let Some(term) = tab.active_terminal_mut() {
                term.grid_mut()
                    .active_mut()
                    .scroll_to_global_row(target_row);
            }
        }
    }

    pub fn prev_search_match(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        let prev = match self.active_search_idx {
            Some(i) => {
                if i == 0 {
                    self.search_matches.len() - 1
                } else {
                    i - 1
                }
            }
            None => self.search_matches.len() - 1,
        };
        self.active_search_idx = Some(prev);
        let target_row = self.search_matches[prev].global_row;
        if let Some(tab) = self.workspace.active_tab_mut() {
            if let Some(term) = tab.active_terminal_mut() {
                term.grid_mut()
                    .active_mut()
                    .scroll_to_global_row(target_row);
            }
        }
    }
    pub fn update_ime_position(&mut self, tab_bar_h: f32) {
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

                        let area_key = (
                            logical_pos.x.round() as i32,
                            logical_pos.y.round() as i32,
                            logical_size.width.round() as i32,
                            logical_size.height.round() as i32,
                        );
                        if self.last_ime_cursor_area == Some(area_key) {
                            break;
                        }
                        self.last_ime_cursor_area = Some(area_key);

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

    pub fn flush_osc52_clipboard(&mut self, clipboard: Option<&Arc<Mutex<Clipboard>>>) {
        for tab in &mut self.workspace.tabs {
            for id in tab.root.all_leaf_ids() {
                if let Some(terminal) = tab.root.find_terminal_mut(id) {
                    if let Some(text) = terminal.take_pending_clipboard() {
                        if let Some(cb) = clipboard {
                            if let Ok(mut clip) = cb.lock() {
                                let _ = clip.set_text(text);
                                info!("Copied OSC 52 text to system clipboard");
                            }
                        }
                    }
                }
            }
        }
    }

    pub fn paste_from_clipboard(&mut self, clipboard: Option<&Arc<Mutex<Clipboard>>>) {
        if let Some(cb) = clipboard {
            if let Ok(mut clip) = cb.lock() {
                let text_to_paste = if let Ok(text) = clip.get_text() {
                    if !text.is_empty() {
                        Some(text)
                    } else {
                        None
                    }
                } else {
                    None
                };

                let final_text = if let Some(text) = text_to_paste {
                    Some(text)
                } else if let Ok(img_data) = clip.get_image() {
                    // Save clipboard image to system temp directory as PNG and paste file path
                    if let Some(rgba_img) = image::RgbaImage::from_raw(
                        img_data.width as u32,
                        img_data.height as u32,
                        img_data.bytes.into_owned(),
                    ) {
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default();
                        let filename =
                            format!("clipboard-{}-{:08X}.png", now.as_secs(), now.subsec_nanos());
                        let temp_path = std::env::temp_dir().join(filename);
                        if rgba_img.save(&temp_path).is_ok() {
                            info!("Saved clipboard image to {:?}", temp_path);
                            Some(temp_path.to_string_lossy().into_owned())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };

                if let Some(text) = final_text {
                    if let Some(active_tab) = self.workspace.active_tab_mut() {
                        if let Some(terminal) = active_tab.active_terminal_mut() {
                            terminal.grid_mut().active_mut().scroll_offset = 0;
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

        let active_tab = self.workspace.active_tab();

        for (id, rect) in pane_rects {
            if rect.contains(mouse_x, mouse_y) {
                let cell_w = self.renderer.cell_width();
                let cell_h = self.renderer.cell_height();
                let pad_x = self.renderer.padding_x;
                let pad_y = self.renderer.padding_y;

                let rel_x = (mouse_x - rect.x - pad_x).max(0.0);
                let rel_y = (mouse_y - rect.y - pad_y).max(0.0);

                let mut col = (rel_x / cell_w).floor() as usize;
                let mut row = (rel_y / cell_h).floor() as usize;

                if let Some(term) = active_tab.and_then(|t| t.root.find_terminal(id)) {
                    let buf = term.grid().active();
                    col = col.min(buf.cols.saturating_sub(1));
                    row = row.min(buf.rows.saturating_sub(1));
                }

                return Some((id, row, col));
            }
        }
        None
    }

    /// Resolve `(mouse_x, mouse_y)` into `(row, col)` clamped to the active pane's bounds,
    /// used while dragging a text selection so dragging past the right/bottom edge still
    /// selects to the end of wrapped lines.
    pub fn get_active_pane_cell_clamped(
        &self,
        mouse_x: f32,
        mouse_y: f32,
        tab_bar_h: f32,
    ) -> Option<(usize, usize)> {
        let active_tab = self.workspace.active_tab()?;
        let active_pane_id = active_tab.active_pane_id;
        let term = active_tab.root.find_terminal(active_pane_id)?;
        let buf = term.grid().active();

        let pane_rects = self.workspace.compute_active_pane_rects_with_scale(
            self.renderer.config.width as f32,
            self.renderer.config.height as f32,
            tab_bar_h,
            self.renderer.scale_factor,
        );

        let (_, rect) = pane_rects
            .into_iter()
            .find(|(id, _)| *id == active_pane_id)?;
        let cell_w = self.renderer.cell_width().max(1.0);
        let cell_h = self.renderer.cell_height().max(1.0);
        let pad_x = self.renderer.padding_x;
        let pad_y = self.renderer.padding_y;

        let rel_x = (mouse_x - rect.x - pad_x).max(0.0);
        let rel_y = (mouse_y - rect.y - pad_y).max(0.0);

        let col = ((rel_x / cell_w).floor() as usize).min(buf.cols.saturating_sub(1));
        let row = ((rel_y / cell_h).floor() as usize).min(buf.rows.saturating_sub(1));

        Some((row, col))
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

    /// Geometry of the global agents panel for this window, or `None` when it is
    /// closed. Every hit-test goes through here so the clickable regions are the
    /// ones the renderer actually drew.
    pub fn agent_panel_layout(&self, tab_bar_h: f32) -> Option<AgentPanelLayout> {
        if !self.workspace.sidebar_open {
            return None;
        }
        let scale = self.renderer.scale_factor;
        Some(AgentPanelLayout::new(
            self.renderer.config.width as f32,
            self.renderer.config.height as f32,
            tab_bar_h,
            self.workspace.sidebar_width * scale,
            scale,
        ))
    }

    pub fn top_right_buttons(&self, tab_bar_h: f32) -> TopRightButtons {
        let scale = self.renderer.scale_factor;
        let reserved_right = if self.workspace.sidebar_open {
            AgentPanelLayout::reserved_width(self.workspace.sidebar_width * scale, scale)
        } else {
            0.0
        };
        let main_w = (self.renderer.config.width as f32 - reserved_right).max(0.0);
        TopRightButtons::compute(main_w, tab_bar_h, scale)
    }

    pub fn get_menu_button_rect(&self, tab_bar_h: f32) -> (f32, f32, f32, f32) {
        let scale = self.renderer.scale_factor;
        let tabs_len = self.workspace.tabs.len();
        let reserved_right = if self.workspace.sidebar_open {
            AgentPanelLayout::reserved_width(self.workspace.sidebar_width * scale, scale)
        } else {
            0.0
        };
        let main_w = (self.renderer.config.width as f32 - reserved_right).max(0.0);
        let base_x = 8.0 * scale;
        let start_x = base_x;
        let plus_btn_w = 24.0 * scale;
        let menu_btn_w = 20.0 * scale;
        let right_btns_w = TopRightButtons::reserved_width(scale);
        let tab_gap = 4.0 * scale;
        let avail_w = (main_w
            - start_x
            - plus_btn_w
            - menu_btn_w
            - right_btns_w
            - 16.0 * scale)
            .max(80.0);
        let total_gaps = (tabs_len.saturating_sub(1) as f32) * tab_gap;
        let tab_w = ((avail_w - total_gaps) / tabs_len as f32).clamp(75.0 * scale, 190.0 * scale);
        let tab_h = (tab_bar_h - 8.0 * scale).max(18.0);
        let tab_y = (tab_bar_h - tab_h) / 2.0;

        let last_tab_right = start_x + (tabs_len as f32 * (tab_w + tab_gap));
        let plus_btn_x = last_tab_right + 2.0 * scale;
        let plus_btn_y = tab_y + (tab_h - plus_btn_w) / 2.0;

        let menu_btn_x = plus_btn_x + plus_btn_w + 2.0 * scale;
        let menu_btn_y = plus_btn_y;
        (menu_btn_x, menu_btn_y, menu_btn_w, plus_btn_w)
    }

    pub fn get_dropdown_menu_x(&self, tab_bar_h: f32, menu_w: f32) -> f32 {
        let scale = self.renderer.scale_factor;
        if tab_bar_h <= 0.0 {
            let btns = self.top_right_buttons(0.0);
            let max_x = self.renderer.config.width as f32 - menu_w - 4.0 * scale;
            let prefs_right = btns.prefs_btn[0] + btns.prefs_btn[2];
            return (prefs_right - menu_w).clamp(4.0 * scale, max_x.max(4.0 * scale));
        }
        let (btn_x, _, btn_w, _) = self.get_menu_button_rect(tab_bar_h);
        let max_x = self.renderer.config.width as f32 - menu_w - 4.0 * scale;
        (btn_x + btn_w / 2.0 - menu_w / 2.0).clamp(4.0 * scale, max_x.max(4.0 * scale))
    }
}
