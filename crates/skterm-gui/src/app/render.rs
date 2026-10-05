//! Frame rendering: window title upkeep, pane collection, and agent-panel state.

use super::events::SCROLLBAR_FADE_AFTER;
#[cfg(target_os = "macos")]
use super::macos::{
    center_macos_window_title, get_nswindow, set_macos_tab_label, style_macos_tab_bar,
};
use super::menu::{build_context_menu, build_top_dropdown_menu};
use super::App;
use skterm_render::{AgentPanelState, PaneRenderItem, TabItem};
use std::time::Duration;
use tracing::warn;
use winit::window::WindowId;

impl App {
    /// Handles `WindowEvent::RedrawRequested` for a terminal window.
    pub(crate) fn handle_redraw_requested(&mut self, window_id: WindowId, tab_bar_h: f32) {
        let edited_agents;
        // The panel is always drawn from the cached list, never from a fresh
        // scan. Two reasons. Correctness: cards are clicked by index, so drawing
        // one list and hit-testing another is how a click ended up renaming the
        // wrong pane. Cost: the old fallback fired whenever the cache was empty,
        // which is the normal state with no agents running, so an open sidebar
        // walked the entire process tree on *every frame*.
        //
        // That leaves one obligation: whoever opens the sidebar fills the cache.
        // `toggle_sidebar` is the only path, and it does.
        debug_assert!(
            !self.sidebar_open || self.last_ai_agent_scan.is_some(),
            "sidebar is open but the agent cache was never filled -- see toggle_sidebar"
        );
        let agents: &[skterm_render::AgentSidebarItem] = if !self.sidebar_open {
            &[]
        } else if self.editing_agent_pane.is_some() {
            // A rename has to track each keystroke, which the 1.5 s cache cadence
            // cannot. Decorating a copy costs one clone of a short list per frame
            // and only while the editor is open -- the scan it replaces cost a
            // process-tree walk plus an `lsof`.
            let mut items = self.cached_ai_agents.items().to_vec();
            self.decorate_editing_card(&mut items, self.cached_ai_agents.owners());
            edited_agents = items;
            &edited_agents
        } else {
            self.cached_ai_agents.items()
        }
        ;

        // The panel header states how many tabs the listing spans, which
        // is what stops it reading as a feature of the selected tab. On
        // macOS each session is one native tab, so sum across sessions.
        let total_tab_count: usize = self.sessions.values().map(|s| s.workspace.tabs.len()).sum();

        if let Some(session) = self.sessions.get_mut(&window_id) {
            session.mark_frame_start();

            if self.editing_agent_pane.is_none()
                && (!self.ime_preedit.is_empty()
                    || self
                        .last_ime_commit_time
                        .is_some_and(|t| t.elapsed() < Duration::from_millis(200)))
            {
                session.update_ime_position(tab_bar_h);
            }

            if let Some(active_tab) = session.workspace.active_tab() {
                let raw_title = {
                    #[cfg(target_os = "macos")]
                    {
                        let clean = session
                            .cached_tab_label
                            .strip_prefix("• ")
                            .unwrap_or(&session.cached_tab_label);
                        if !clean.is_empty() {
                            clean.to_string()
                        } else if active_tab.title == "Tab 1" {
                            "skTerm".to_string()
                        } else {
                            active_tab.title.clone()
                        }
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        active_tab.title.clone()
                    }
                };
                let win_title = if raw_title.is_empty() || raw_title == "skTerm" {
                    "skTerm".to_string()
                } else {
                    format!("skTerm — {}", raw_title)
                };
                if session.window.title() != win_title {
                    session.window.set_title(&win_title);
                    #[cfg(target_os = "macos")]
                    {
                        let win_ptr = get_nswindow(&session.window);
                        if !session.cached_tab_label.is_empty() {
                            unsafe {
                                set_macos_tab_label(
                                    win_ptr,
                                    &session.cached_tab_label,
                                    &session.cached_tab_tooltip,
                                );
                            }
                        }
                        unsafe {
                            center_macos_window_title(win_ptr);
                            style_macos_tab_bar(win_ptr);
                        }
                    }
                }
            }

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
                        let is_focused = *id == active_tab.active_pane_id;
                        let is_scrollbar_hovered = session.hovered_scrollbar_pane == Some(*id);
                        let is_scrollbar_dragging = session
                            .dragging_scrollbar
                            .as_ref()
                            .is_some_and(|d| d.pane_id == *id);
                        let is_recently_scrolled = session
                            .last_scroll_time
                            .is_some_and(|t| t.elapsed() < SCROLLBAR_FADE_AFTER);

                        render_panes.push(PaneRenderItem {
                            grid: t.grid(),
                            rect_x: rect.x,
                            rect_y: rect.y,
                            rect_w: rect.width,
                            rect_h: rect.height,
                            is_focused,
                            search_matches: if is_focused {
                                &session.search_matches
                            } else {
                                &[]
                            },
                            active_search_idx: if is_focused {
                                session.active_search_idx
                            } else {
                                None
                            },
                            search_query: if is_focused && session.search_open {
                                Some(&session.search_query)
                            } else {
                                None
                            },
                            is_scrollbar_hovered,
                            is_scrollbar_dragging,
                            is_recently_scrolled,
                            dynamic_palette: Some(&t.state.dynamic_palette),
                            dynamic_fg: t.state.dynamic_fg,
                            dynamic_bg: t.state.dynamic_bg,
                            dynamic_cursor: t.state.dynamic_cursor,
                        });
                    }
                }
            }

            let panel_state = AgentPanelState {
                open: session.workspace.sidebar_open,
                width_px: if session.workspace.sidebar_open {
                    session.workspace.sidebar_width * session.renderer.scale_factor
                } else {
                    0.0
                },
                agents,
                tab_count: total_tab_count,
                show_floating_top_right: session.top_right_hover
                    || (session.menu_open && !session.menu_is_context),
                ai_btn_hovered: session.ai_btn_hovered,
                prefs_btn_hovered: session.prefs_btn_hovered,
            };

            let (top_menu, _actions) = if session.menu_open {
                let scale = session.renderer.scale_factor;
                let screen_w = session.renderer.config.width as f32;
                let screen_h = session.renderer.config.height as f32;
                if session.menu_is_context {
                    let (m, acts) = build_context_menu(
                        scale,
                        session.menu_anchor.0,
                        session.menu_anchor.1,
                        screen_w,
                        screen_h,
                        session.menu_hovered_idx,
                    );
                    (Some(m), acts)
                } else {
                    let menu_w = 240.0 * scale;
                    let menu_x = session.get_dropdown_menu_x(tab_bar_h, menu_w);
                    let (mut m, acts) = build_top_dropdown_menu(
                        scale,
                        menu_x,
                        tab_bar_h,
                        session.menu_hovered_idx,
                    );
                    m.clamp_to_screen(screen_w, screen_h, scale);
                    (Some(m), acts)
                }
            } else {
                (None, Vec::new())
            };

            if let Err(e) = session.renderer.render_workspace_with_agent_panel(
                &tabs,
                &render_panes,
                tab_bar_h,
                panel_state,
                top_menu.as_ref(),
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
}
