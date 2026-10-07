//! Helpers backing the global AI-agents sidebar and its inline label editor.

use super::events::DOUBLE_CLICK_INTERVAL;
#[cfg(target_os = "macos")]
use super::macos::get_nswindow;
use super::App;
use skterm_render::AgentSidebarItem;
use std::time::Instant;
use winit::window::WindowId;

/// Identifies the pane behind an agent card: window, tab, pane.
pub type AgentRow = (WindowId, u32, u32);

/// The agent list the sidebar is drawn from.
///
/// Cards are addressed by index -- the hit-test returns `AgentPanelHit::Card(i)`
/// -- so the list the user clicked has to be the same list that produced the
/// pixels. It previously was not: the panel was drawn from a cache refreshed
/// every 1.5 s while the click handler re-resolved the index against a *fresh*
/// scan, so an agent starting or exiting in between shifted the indices and the
/// click landed on a different pane. With a rename in flight that meant renaming
/// the wrong one.
///
/// Keeping the drawn items and their owners in one value, populated only through
/// [`AgentList::set`], makes that class of bug unrepresentable: the two vectors
/// cannot be updated independently.
#[derive(Default)]
pub struct AgentList {
    /// What the renderer draws. The index is the card index.
    items: Vec<AgentSidebarItem>,
    /// The pane behind each card, indexed identically to `items`.
    owners: Vec<AgentRow>,
}

impl AgentList {
    /// Replace the list wholesale. The only way to populate it.
    /// Returns `true` if the new list differs from the previous one.
    pub fn set(&mut self, scanned: Vec<(WindowId, AgentSidebarItem)>) -> bool {
        let unchanged = self.items.len() == scanned.len()
            && self
                .owners
                .iter()
                .zip(self.items.iter())
                .zip(scanned.iter())
                .all(|((&owner, item), (wid, new_item))| {
                    owner == (*wid, new_item.tab_id, new_item.pane_id) && item == new_item
                });
        if unchanged {
            return false;
        }
        self.items.clear();
        self.owners.clear();
        self.items.reserve(scanned.len());
        self.owners.reserve(scanned.len());
        for (wid, item) in scanned {
            self.owners.push((wid, item.tab_id, item.pane_id));
            self.items.push(item);
        }
        true
    }

    /// The cards, in draw order.
    pub fn items(&self) -> &[AgentSidebarItem] {
        &self.items
    }

    /// The pane behind each card, in the same order as [`AgentList::items`].
    pub fn owners(&self) -> &[AgentRow] {
        &self.owners
    }

    /// The pane behind the card at `idx`, or `None` if the list has shrunk.
    pub fn owner(&self, idx: usize) -> Option<AgentRow> {
        self.owners.get(idx).copied()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.owners.clear();
    }
}

/// Is this click a repeat of the previous one, on the *same* card?
///
/// Keyed on the card's identity rather than its index, so the gesture survives
/// the list being reordered between clicks, and two single clicks on different
/// cards are never mistaken for one double click.
pub fn is_repeat_click(last: Option<(AgentRow, Instant)>, row: AgentRow, now: Instant) -> bool {
    match last {
        Some((prev, at)) => prev == row && now.duration_since(at) < DOUBLE_CLICK_INTERVAL,
        None => false,
    }
}

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
                        target_s.last_tab_label_refresh = None;
                        target_s.window.request_redraw();
                    }
                    self.editing_agent_pane = None;
                    // The new label reaches the panel through the cache, so
                    // without this the card would keep showing the old one until
                    // the next periodic scan -- up to AGENT_SCAN_INTERVAL of the
                    // rename appearing not to have taken.
                    if self.sidebar_open {
                        self.refresh_agent_cache();
                    }
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
                target_s.last_tab_label_refresh = None;
                target_s.window.request_redraw();
            }
            self.ime_preedit.clear();
            // See the native-editor path above: the card reads its label from
            // the cache, so it has to be rebuilt for the rename to show.
            if self.sidebar_open {
                self.refresh_agent_cache();
            }
            for s in self.sessions.values() {
                s.window.request_redraw();
            }
        }
    }

    pub fn toggle_sidebar(&mut self) {
        self.sidebar_open = !self.sidebar_open;
        if self.sidebar_open {
            // The periodic scan is gated on the sidebar being visible, so the
            // cache is stale or empty by now. Fill it here rather than leaving it
            // to the next tick: the panel is drawn before `about_to_wait` runs,
            // and clicks resolve against this exact list, so waiting would mean a
            // first frame that is both blank and unclickable.
            self.refresh_agent_cache();
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

    /// Rebuild the cached agent list.
    ///
    /// This is the only writer. Everything that draws or hit-tests the panel
    /// reads [`App::cached_ai_agents`], so the list on screen and the list a
    /// click resolves against are the same one by construction.
    pub fn refresh_agent_cache(&mut self) {
        self.last_ai_agent_scan = Some(Instant::now());
        let scanned = self.scan_all_ai_agents();
        if self.cached_ai_agents.set(scanned) {
            for s in self.sessions.values() {
                s.window.request_redraw();
            }
        }
    }

    /// Overlay the in-progress rename onto the card being edited.
    ///
    /// Split out of the scan because the two have completely different costs and
    /// cadences: scanning walks the whole process tree (plus an `lsof` on cache
    /// miss) and only needs doing a couple of times a second, while this is a
    /// little string formatting that has to track every keystroke. Keeping them
    /// together is what forced the render path to rescan on every frame while a
    /// rename was open.
    pub fn decorate_editing_card(&self, items: &mut [AgentSidebarItem], owners: &[AgentRow]) {
        let Some(editing) = self.editing_agent_pane else {
            return;
        };
        let Some(idx) = owners.iter().position(|row| *row == editing) else {
            return;
        };
        let Some(item) = items.get_mut(idx) else {
            return;
        };

        #[cfg(target_os = "macos")]
        let hide_underlying = self.macos_editor.is_active();
        #[cfg(not(target_os = "macos"))]
        let hide_underlying = false;

        if hide_underlying {
            item.title_summary = String::new();
            return;
        }

        let chars: Vec<char> = self.editing_agent_text.chars().collect();
        let cur = self.editing_agent_cursor.min(chars.len());
        let before: String = chars[..cur].iter().collect();
        let after: String = chars[cur..].iter().collect();
        item.title_summary = if self.ime_preedit.is_empty() {
            format!("✎ {}▏{}", before, after)
        } else {
            format!("✎ {}[{}]▏{}", before, self.ime_preedit, after)
        };
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
                    all_agents.push((*wid, agent.clone()));
                }
            }
        }

        all_agents
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn win() -> WindowId {
        WindowId::dummy()
    }

    fn item(tab: u32, pane: u32, name: &str) -> AgentSidebarItem {
        AgentSidebarItem {
            tab_id: tab,
            pane_id: pane,
            agent_name: name.to_string(),
            project_name: String::new(),
            location_label: String::new(),
            title_summary: String::new(),
            is_active_output: false,
            is_focused: false,
        }
    }

    #[test]
    fn owners_line_up_with_the_cards_they_were_built_from() {
        let mut list = AgentList::default();
        list.set(vec![
            (win(), item(1, 10, "a")),
            (win(), item(2, 20, "b")),
            (win(), item(3, 30, "c")),
        ]);

        for (idx, expected) in [(0, (1, 10)), (1, (2, 20)), (2, (3, 30))] {
            let drawn = &list.items()[idx];
            let owner = list.owner(idx).expect("owner for a drawn card");
            assert_eq!(
                (owner.1, owner.2),
                expected,
                "card {idx} points at a different pane than it was built from"
            );
            assert_eq!((drawn.tab_id, drawn.pane_id), expected);
        }
        assert_eq!(list.owners().len(), list.items().len());
    }

    #[test]
    fn a_click_resolves_against_the_drawn_list_not_a_later_scan() {
        // The bug this replaces: the panel was drawn from a cache while clicks
        // were resolved against a fresh scan. Here the first agent exits between
        // the frame and the click, so a re-scan would shift every index down by
        // one and card 1 would resolve to pane 30 instead of the pane 20 the user
        // actually clicked.
        let mut drawn = AgentList::default();
        drawn.set(vec![
            (win(), item(1, 10, "a")),
            (win(), item(2, 20, "b")),
            (win(), item(3, 30, "c")),
        ]);

        let rescanned = [(win(), item(2, 20, "b")), (win(), item(3, 30, "c"))];

        let clicked = 1;
        assert_eq!(drawn.owner(clicked).unwrap().2, 20, "what the user clicked");
        assert_eq!(
            rescanned[clicked].1.pane_id, 30,
            "what a re-scan would have given -- the wrong pane"
        );
    }

    #[test]
    fn a_click_past_the_end_of_a_shrunken_list_resolves_to_nothing() {
        // A card whose pane exited: better to do nothing than to act on whatever
        // slid into that index.
        let mut list = AgentList::default();
        list.set(vec![(win(), item(1, 10, "a"))]);
        assert!(list.owner(1).is_none());
        list.clear();
        assert!(list.owner(0).is_none());
        assert!(list.is_empty());
    }

    #[test]
    fn set_leaves_nothing_behind_from_the_previous_list() {
        let mut list = AgentList::default();
        list.set(vec![(win(), item(1, 10, "a")), (win(), item(2, 20, "b"))]);
        list.set(vec![(win(), item(9, 90, "z"))]);
        assert_eq!(list.items().len(), 1);
        assert_eq!(list.owners().len(), 1);
        assert_eq!(list.owner(0).unwrap().2, 90);
    }

    #[test]
    fn a_second_click_on_the_same_card_is_a_double_click() {
        let t0 = Instant::now();
        let card = (win(), 1, 10);
        let soon = t0 + Duration::from_millis(200);
        assert!(is_repeat_click(Some((card, t0)), card, soon));
    }

    #[test]
    fn a_slow_second_click_is_not_a_double_click() {
        let t0 = Instant::now();
        let card = (win(), 1, 10);
        let late = t0 + DOUBLE_CLICK_INTERVAL;
        assert!(!is_repeat_click(Some((card, t0)), card, late));
    }

    #[test]
    fn clicks_on_two_different_cards_are_not_a_double_click() {
        // Keyed on identity, not on "a click happened recently" -- otherwise
        // quickly picking one agent and then another would open a rename on the
        // second one.
        let t0 = Instant::now();
        let soon = t0 + Duration::from_millis(100);
        let first = (win(), 1, 10);
        let second = (win(), 2, 20);
        assert!(!is_repeat_click(Some((first, t0)), second, soon));
    }

    #[test]
    fn the_first_click_of_all_is_not_a_double_click() {
        assert!(!is_repeat_click(None, (win(), 1, 10), Instant::now()));
    }

    #[test]
    fn set_reports_whether_the_agent_list_changed() {
        let mut list = AgentList::default();
        assert!(!list.set(vec![]), "empty -> empty is unchanged");

        assert!(
            list.set(vec![(win(), item(1, 10, "Jetski"))]),
            "empty -> non-empty is a change"
        );
        assert!(
            !list.set(vec![(win(), item(1, 10, "Jetski"))]),
            "identical scan is unchanged"
        );

        let mut updated = item(1, 10, "Jetski");
        updated.is_active_output = true;
        assert!(
            list.set(vec![(win(), updated.clone())]),
            "activity status change is detected"
        );
        assert!(
            !list.set(vec![(win(), updated)]),
            "same activity status is unchanged"
        );
    }

    #[test]
    fn match_agent_command_detects_linux_and_glinux_invocations() {
        use crate::layout::ProcessTreeCache;

        // Direct binaries
        assert_eq!(
            ProcessTreeCache::match_agent_command("jetski"),
            Some("Jetski")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("agy"),
            Some("Antigravity CLI")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("claude"),
            Some("Claude Code")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("gemini"),
            Some("Gemini CLI")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("aider"),
            Some("Aider")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("codex"),
            Some("Codex CLI")
        );

        // Google Linux (gLinux) SAR extracted binaries and release wrappers
        assert_eq!(
            ProcessTreeCache::match_agent_command(
                "/tmp/sar.cli_internal.9mu5541l/cli_internal_impl.runfiles/google3/third_party/jetski/cmd/cli/cli --app_data_dir=jetski"
            ),
            Some("Jetski")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("/google/bin/releases/jetski-devs/tools/cli"),
            Some("Jetski")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("/bin/bash /tmp/sar.gemini.ab12cd34/gemini_impl"),
            Some("Gemini CLI")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("/google/bin/releases/gemini-cli/tools/gemini"),
            Some("Gemini CLI")
        );

        // Node / npm / Python wrappers on Linux
        assert_eq!(
            ProcessTreeCache::match_agent_command(
                "node /usr/local/lib/node_modules/@anthropic-ai/claude-code/cli.js"
            ),
            Some("Claude Code")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command(
                "env NODE_NO_WARNINGS=1 node /home/user/.nvm/versions/node/v22.0.0/bin/gemini"
            ),
            Some("Gemini CLI")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("/usr/bin/python3 -m aider --model sonnet"),
            Some("Aider")
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command(
                "node /usr/lib/node_modules/@openai/codex/bin/codex.js"
            ),
            Some("Codex CLI")
        );

        // Non-agents and background daemons must not false-positive
        assert_eq!(ProcessTreeCache::match_agent_command("/bin/bash"), None);
        assert_eq!(
            ProcessTreeCache::match_agent_command("grep jetski Cargo.toml"),
            None
        );
        assert_eq!(ProcessTreeCache::match_agent_command("vim claude.md"), None);
        assert_eq!(
            ProcessTreeCache::match_agent_command("git checkout gemini"),
            None
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command(
                "/tmp/sar.123/google3/third_party/jetski/cmd/hub/server/jetski-hub-server"
            ),
            None
        );
        assert_eq!(
            ProcessTreeCache::match_agent_command("jetski-sidecar --port 9000"),
            None
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_proc_tree_finds_spawned_agent_descendant() {
        use crate::layout::ProcessTreeCache;
        use std::os::unix::process::CommandExt;

        // Spawn a short-lived child process whose argv[0] mimics the Jetski SAR CLI binary.
        let mut child = std::process::Command::new("sleep")
            .arg0("/tmp/sar.cli_internal.test/cli_internal_impl.runfiles/google3/third_party/jetski/cmd/cli/cli")
            .arg("5")
            .spawn()
            .expect("spawn test child");

        let my_pid = std::process::id();
        let mut cache = ProcessTreeCache::new();
        let detected = cache.find_agent_descendant(my_pid);
        let project = cache.get_project_name(child.id());

        let _ = child.kill();
        let _ = child.wait();

        assert_eq!(detected, Some((child.id(), "Jetski")));
        assert!(!project.is_empty() && project != "workspace");
    }
}
