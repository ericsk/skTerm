//! Helpers backing the global AI-agents sidebar and its inline label editor.

#[cfg(target_os = "macos")]
use super::macos::get_nswindow;
use super::App;
use winit::window::WindowId;

impl App {
    #[cfg(target_os = "macos")]
    pub fn poll_macos_editor(&mut self) {
        while let Ok(evt) = self.macos_editor.receiver.try_recv() {
            match evt {
                crate::macos_textfield::TextFieldEvent::Committed {
                    target: (wid, tab_id, pane_id),
                    text,
                } => {
                    let label = text.trim().to_string();
                    if let Some(target_s) = self.sessions.get_mut(&wid) {
                        if let Some(t_pos) =
                            target_s.workspace.tabs.iter().position(|t| t.id == tab_id)
                        {
                            if let Some(term) = target_s.workspace.tabs[t_pos]
                                .root
                                .find_terminal_mut(pane_id)
                            {
                                term.custom_label =
                                    if label.is_empty() { None } else { Some(label) };
                            }
                        }
                        target_s.window.request_redraw();
                    }
                    self.editing_agent_pane = None;
                    for s in self.sessions.values() {
                        s.window.request_redraw();
                    }
                }
                crate::macos_textfield::TextFieldEvent::Cancelled => {
                    self.editing_agent_pane = None;
                    for s in self.sessions.values() {
                        s.window.request_redraw();
                    }
                }
            }
        }
    }

    pub fn insert_editing_text(&mut self, s: &str) {
        let mut chars: Vec<char> = self.editing_agent_text.chars().collect();
        let cur = self.editing_agent_cursor.min(chars.len());
        let insert_chars: Vec<char> = s.chars().collect();
        let insert_len = insert_chars.len();
        chars.splice(cur..cur, insert_chars);
        self.editing_agent_text = chars.into_iter().collect();
        self.editing_agent_cursor = cur + insert_len;
    }

    pub fn backspace_editing_text(&mut self) {
        let mut chars: Vec<char> = self.editing_agent_text.chars().collect();
        let cur = self.editing_agent_cursor.min(chars.len());
        if cur > 0 {
            chars.remove(cur - 1);
            self.editing_agent_text = chars.into_iter().collect();
            self.editing_agent_cursor = cur - 1;
        }
    }

    pub fn delete_editing_text(&mut self) {
        let mut chars: Vec<char> = self.editing_agent_text.chars().collect();
        let cur = self.editing_agent_cursor.min(chars.len());
        if cur < chars.len() {
            chars.remove(cur);
            self.editing_agent_text = chars.into_iter().collect();
        }
    }

    pub fn commit_agent_label_edit(&mut self) {
        #[cfg(target_os = "macos")]
        if self.macos_editor.is_active() {
            self.macos_editor.close_active(true);
            self.poll_macos_editor();
            return;
        }
        if let Some((edit_wid, edit_tab_id, edit_pane_id)) = self.editing_agent_pane.take() {
            let label = self.editing_agent_text.trim().to_string();
            if let Some(target_s) = self.sessions.get_mut(&edit_wid) {
                if let Some(t_pos) = target_s
                    .workspace
                    .tabs
                    .iter()
                    .position(|t| t.id == edit_tab_id)
                {
                    if let Some(term) = target_s.workspace.tabs[t_pos]
                        .root
                        .find_terminal_mut(edit_pane_id)
                    {
                        term.custom_label = if label.is_empty() { None } else { Some(label) };
                    }
                }
                target_s.window.request_redraw();
            }
            self.ime_preedit.clear();
            for s in self.sessions.values() {
                s.window.request_redraw();
            }
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_open = !self.sidebar_open;
        if self.sidebar_open {
            // The periodic scan is gated on the sidebar being visible, so its cached
            // result is stale (or empty) by now. Force a fresh scan on the next tick.
            self.last_ai_agent_scan = None;
        } else {
            self.cached_ai_agents.clear();
        }
        let tab_bar_h = self.get_tab_bar_height();
        let open = self.sidebar_open;
        for session in self.sessions.values_mut() {
            session.workspace.sidebar_open = open;
            session.sync_all_pane_sizes(tab_bar_h);
            // The panel is global, so every window's titlebar toggle has to
            // reflect it -- not just the one that was clicked.
            #[cfg(target_os = "macos")]
            unsafe {
                crate::macos_titlebar::set_agent_toggle_state(get_nswindow(&session.window), open);
            }
            session.window.request_redraw();
        }
    }

    pub fn scan_all_ai_agents(&mut self) -> Vec<(WindowId, skterm_render::AgentSidebarItem)> {
        let mut all_agents = Vec::new();
        let active_wid = self.active_session_id;
        let session_ids = self.session_order.clone();

        for (s_idx, wid) in session_ids.iter().enumerate() {
            if let Some(session) = self.sessions.get_mut(wid) {
                let mut agents = session.workspace.scan_ai_agents(&mut session.process_cache);
                for agent in &mut agents {
                    if session_ids.len() > 1 {
                        agent.location_label =
                            format!("Win {} · {}", s_idx + 1, agent.location_label);
                    }
                    if Some(*wid) != active_wid {
                        agent.is_focused = false;
                    }
                    if self.editing_agent_pane == Some((*wid, agent.tab_id, agent.pane_id)) {
                        #[cfg(target_os = "macos")]
                        let hide_underlying = self.macos_editor.is_active();
                        #[cfg(not(target_os = "macos"))]
                        let hide_underlying = false;

                        if hide_underlying {
                            agent.title_summary = String::new();
                        } else {
                            let chars: Vec<char> = self.editing_agent_text.chars().collect();
                            let cur = self.editing_agent_cursor.min(chars.len());
                            let before: String = chars[..cur].iter().collect();
                            let after: String = chars[cur..].iter().collect();
                            if !self.ime_preedit.is_empty() {
                                agent.title_summary =
                                    format!("✎ {}[{}]▏{}", before, self.ime_preedit, after);
                            } else {
                                agent.title_summary = format!("✎ {}▏{}", before, after);
                            }
                        }
                    }
                    all_agents.push((*wid, agent.clone()));
                }
            }
        }

        all_agents
    }
}
