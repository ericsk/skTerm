//! Binary Space Partitioning (BSP) tree for split panes, tabs, and multi-terminal management.

use skterm_core::{CursorShape, PtySpawnOptions, Terminal};

/// Unique identifier for a split pane.
pub type PaneId = u32;
/// Unique identifier for a tab.
pub type TabId = u32;

/// Rectangle in physical pixels representing a pane or layout region.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= (self.x + self.width) && py >= self.y && py <= (self.y + self.height)
    }
}

/// Split Direction: Horizontal (Left/Right) or Vertical (Top/Bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal, // Split into Left and Right
    Vertical,   // Split into Top and Bottom
}

/// Node in the Binary Space Partitioning (BSP) pane tree.
pub enum PaneNode {
    Leaf {
        id: PaneId,
        terminal: Terminal,
    },
    Split {
        direction: SplitDirection,
        ratio: f32,
        first: Box<PaneNode>,
        second: Box<PaneNode>,
    },
}

impl PaneNode {
    pub fn find_terminal(&self, target_id: PaneId) -> Option<&Terminal> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    Some(terminal)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.find_terminal(target_id).or_else(|| second.find_terminal(target_id))
            }
        }
    }

    pub fn find_terminal_mut(&mut self, target_id: PaneId) -> Option<&mut Terminal> {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    Some(terminal)
                } else {
                    None
                }
            }
            PaneNode::Split { first, second, .. } => {
                if let Some(t) = first.find_terminal_mut(target_id) {
                    Some(t)
                } else {
                    second.find_terminal_mut(target_id)
                }
            }
        }
    }

    pub fn all_leaf_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        self.collect_leaf_ids(&mut ids);
        ids
    }

    fn collect_leaf_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            PaneNode::Leaf { id, .. } => out.push(*id),
            PaneNode::Split { first, second, .. } => {
                first.collect_leaf_ids(out);
                second.collect_leaf_ids(out);
            }
        }
    }

    pub fn find_dead_panes(&self) -> Vec<PaneId> {
        let mut dead = Vec::new();
        self.collect_dead_panes(&mut dead);
        dead
    }

    fn collect_dead_panes(&self, out: &mut Vec<PaneId>) {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if !terminal.is_child_alive() {
                    out.push(*id);
                }
            }
            PaneNode::Split { first, second, .. } => {
                first.collect_dead_panes(out);
                second.collect_dead_panes(out);
            }
        }
    }

    pub fn split_pane(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_id: PaneId,
        new_terminal: Terminal,
    ) -> bool {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    let old_id = *id;
                    let old_terminal = std::mem::replace(
                        terminal,
                        Terminal::new_headless(1, 1, 0),
                    );

                    *self = PaneNode::Split {
                        direction,
                        ratio: 0.5,
                        first: Box::new(PaneNode::Leaf {
                            id: old_id,
                            terminal: old_terminal,
                        }),
                        second: Box::new(PaneNode::Leaf {
                            id: new_id,
                            terminal: new_terminal,
                        }),
                    };
                    true
                } else {
                    false
                }
            }
            PaneNode::Split { first, second, .. } => {
                if first.split_pane(target_id, direction, new_id, new_terminal.clone_dummy()) {
                    true
                } else {
                    second.split_pane(target_id, direction, new_id, new_terminal)
                }
            }
        }
    }

    pub fn close_pane(self, target_id: PaneId) -> (Option<Box<PaneNode>>, bool) {
        match self {
            PaneNode::Leaf { id, .. } => {
                if id == target_id {
                    (None, true)
                } else {
                    (Some(Box::new(self)), false)
                }
            }
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let (new_first, closed_first) = first.close_pane(target_id);
                if closed_first {
                    if let Some(f) = new_first {
                        (
                            Some(Box::new(PaneNode::Split {
                                direction,
                                ratio,
                                first: f,
                                second,
                            })),
                            true,
                        )
                    } else {
                        (Some(second), true)
                    }
                } else {
                    let (new_second, closed_second) = second.close_pane(target_id);
                    if closed_second {
                        if let Some(s) = new_second {
                            (
                                Some(Box::new(PaneNode::Split {
                                    direction,
                                    ratio,
                                    first: new_first.unwrap(),
                                    second: s,
                                })),
                                true,
                            )
                        } else {
                            (Some(new_first.unwrap()), true)
                        }
                    } else {
                        (
                            Some(Box::new(PaneNode::Split {
                                direction,
                                ratio,
                                first: new_first.unwrap(),
                                second: new_second.unwrap(),
                            })),
                            false,
                        )
                    }
                }
            }
        }
    }

    pub fn compute_layout(&self, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            PaneNode::Leaf { id, .. } => {
                out.push((*id, rect));
            }
            PaneNode::Split {
                direction,
                ratio,
                first,
                second,
            } => {
                let sep_size = 2.0;
                match direction {
                    SplitDirection::Horizontal => {
                        let w1 = ((rect.width - sep_size) * ratio).floor().max(10.0);
                        let w2 = (rect.width - w1 - sep_size).max(10.0);
                        let r1 = Rect::new(rect.x, rect.y, w1, rect.height);
                        let r2 = Rect::new(rect.x + w1 + sep_size, rect.y, w2, rect.height);
                        first.compute_layout(r1, out);
                        second.compute_layout(r2, out);
                    }
                    SplitDirection::Vertical => {
                        let h1 = ((rect.height - sep_size) * ratio).floor().max(10.0);
                        let h2 = (rect.height - h1 - sep_size).max(10.0);
                        let r1 = Rect::new(rect.x, rect.y, rect.width, h1);
                        let r2 = Rect::new(rect.x, rect.y + h1 + sep_size, rect.width, h2);
                        first.compute_layout(r1, out);
                        second.compute_layout(r2, out);
                    }
                }
            }
        }
    }

    pub fn poll_all_ptys(&mut self) -> bool {
        match self {
            PaneNode::Leaf { terminal, .. } => terminal.poll_pty(),
            PaneNode::Split { first, second, .. } => {
                let p1 = first.poll_all_ptys();
                let p2 = second.poll_all_ptys();
                p1 || p2
            }
        }
    }

    pub fn update_cursor_shape(&mut self, shape: CursorShape) {
        match self {
            PaneNode::Leaf { terminal, .. } => {
                terminal.set_cursor_shape(shape);
            }
            PaneNode::Split { first, second, .. } => {
                first.update_cursor_shape(shape);
                second.update_cursor_shape(shape);
            }
        }
    }
}

trait TerminalDummy {
    fn clone_dummy(&self) -> Terminal;
}

impl TerminalDummy for Terminal {
    fn clone_dummy(&self) -> Terminal {
        Terminal::new_headless(self.grid().active().rows, self.grid().active().cols, 100)
    }
}

/// A single tab containing a BSP tree of panes.
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub root: PaneNode,
    pub active_pane_id: PaneId,
}

impl Tab {
    pub fn new(id: TabId, title: String, rows: usize, cols: usize, pane_id: PaneId) -> anyhow::Result<Self> {
        Self::new_with_options(id, title, rows, cols, pane_id, 10000, PtySpawnOptions::default())
    }

    pub fn new_with_options(
        id: TabId,
        title: String,
        rows: usize,
        cols: usize,
        pane_id: PaneId,
        scrollback_lines: usize,
        options: PtySpawnOptions,
    ) -> anyhow::Result<Self> {
        let terminal = Terminal::new_with_options(rows, cols, scrollback_lines, options)?;
        Ok(Self {
            id,
            title,
            root: PaneNode::Leaf { id: pane_id, terminal },
            active_pane_id: pane_id,
        })
    }

    pub fn active_terminal(&self) -> Option<&Terminal> {
        self.root.find_terminal(self.active_pane_id)
    }

    pub fn active_terminal_mut(&mut self) -> Option<&mut Terminal> {
        self.root.find_terminal_mut(self.active_pane_id)
    }

    pub fn split_active(
        &mut self,
        direction: SplitDirection,
        next_pane_id: &mut u32,
        rows: usize,
        cols: usize,
    ) -> anyhow::Result<PaneId> {
        self.split_active_with_options(direction, next_pane_id, rows, cols, 10000, PtySpawnOptions::default())
    }

    pub fn split_active_with_options(
        &mut self,
        direction: SplitDirection,
        next_pane_id: &mut u32,
        rows: usize,
        cols: usize,
        scrollback_lines: usize,
        options: PtySpawnOptions,
    ) -> anyhow::Result<PaneId> {
        let new_id = *next_pane_id;
        *next_pane_id += 1;

        let new_terminal = Terminal::new_with_options(rows, cols, scrollback_lines, options)?;
        self.root.split_pane(self.active_pane_id, direction, new_id, new_terminal);
        self.active_pane_id = new_id;
        Ok(new_id)
    }

    pub fn close_pane_by_id(&mut self, target: PaneId) -> bool {
        let root = std::mem::replace(
            &mut self.root,
            PaneNode::Leaf {
                id: 0,
                terminal: Terminal::new_headless(1, 1, 0),
            },
        );

        let (new_root, _closed) = root.close_pane(target);
        if let Some(surviving_root) = new_root {
            self.root = *surviving_root;
            let leaves = self.root.all_leaf_ids();
            if !leaves.contains(&self.active_pane_id) {
                if let Some(&first_leaf) = leaves.first() {
                    self.active_pane_id = first_leaf;
                }
            }
            false
        } else {
            true
        }
    }

    pub fn close_active(&mut self) -> bool {
        self.close_pane_by_id(self.active_pane_id)
    }

    pub fn cleanup_dead_panes(&mut self) -> bool {
        let dead = self.root.find_dead_panes();
        for dead_id in dead {
            if self.close_pane_by_id(dead_id) {
                return true;
            }
        }
        self.root.all_leaf_ids().is_empty()
    }

    pub fn focus_next(&mut self) {
        let leaves = self.root.all_leaf_ids();
        if let Some(pos) = leaves.iter().position(|&id| id == self.active_pane_id) {
            let next_pos = (pos + 1) % leaves.len();
            self.active_pane_id = leaves[next_pos];
        }
    }

    pub fn focus_prev(&mut self) {
        let leaves = self.root.all_leaf_ids();
        if let Some(pos) = leaves.iter().position(|&id| id == self.active_pane_id) {
            let prev_pos = if pos == 0 { leaves.len() - 1 } else { pos - 1 };
            self.active_pane_id = leaves[prev_pos];
        }
    }
}

/// Workspace holding multiple tabs and managing global layouts.
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active_tab_idx: usize,
    next_tab_id: u32,
    next_pane_id: u32,
}

impl Workspace {
    pub fn new(rows: usize, cols: usize) -> anyhow::Result<Self> {
        Self::new_with_options(rows, cols, 10000, PtySpawnOptions::default())
    }

    pub fn new_with_options(
        rows: usize,
        cols: usize,
        scrollback_lines: usize,
        options: PtySpawnOptions,
    ) -> anyhow::Result<Self> {
        let mut next_tab_id = 1;
        let mut next_pane_id = 1;

        let initial_tab = Tab::new_with_options(
            next_tab_id,
            "Tab 1".to_string(),
            rows,
            cols,
            next_pane_id,
            scrollback_lines,
            options,
        )?;
        next_tab_id += 1;
        next_pane_id += 1;

        Ok(Self {
            tabs: vec![initial_tab],
            active_tab_idx: 0,
            next_tab_id,
            next_pane_id,
        })
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.tabs.get(self.active_tab_idx)
    }

    pub fn active_tab_mut(&mut self) -> Option<&mut Tab> {
        self.tabs.get_mut(self.active_tab_idx)
    }

    pub fn new_tab(&mut self, rows: usize, cols: usize) -> anyhow::Result<TabId> {
        self.new_tab_with_options(rows, cols, 10000, PtySpawnOptions::default())
    }

    pub fn new_tab_with_options(
        &mut self,
        rows: usize,
        cols: usize,
        scrollback_lines: usize,
        options: PtySpawnOptions,
    ) -> anyhow::Result<TabId> {
        let tab_id = self.next_tab_id;
        let pane_id = self.next_pane_id;
        self.next_tab_id += 1;
        self.next_pane_id += 1;

        let title = format!("Tab {}", self.tabs.len() + 1);
        let tab = Tab::new_with_options(
            tab_id,
            title,
            rows,
            cols,
            pane_id,
            scrollback_lines,
            options,
        )?;
        self.tabs.push(tab);
        self.active_tab_idx = self.tabs.len() - 1;
        Ok(tab_id)
    }

    pub fn split_active_pane(
        &mut self,
        direction: SplitDirection,
        rows: usize,
        cols: usize,
        scrollback_lines: usize,
        options: PtySpawnOptions,
    ) -> anyhow::Result<PaneId> {
        let mut next_id = self.next_pane_id;
        let res = if let Some(tab) = self.active_tab_mut() {
            tab.split_active_with_options(direction, &mut next_id, rows, cols, scrollback_lines, options)
        } else {
            Err(anyhow::anyhow!("No active tab in workspace"))
        };
        self.next_pane_id = next_id;
        res
    }

    pub fn close_active_pane(&mut self) -> bool {
        if self.tabs.is_empty() {
            return true;
        }

        let should_close_tab = self.tabs[self.active_tab_idx].close_active();
        if should_close_tab {
            self.tabs.remove(self.active_tab_idx);
            if self.tabs.is_empty() {
                return true;
            }
            if self.active_tab_idx >= self.tabs.len() {
                self.active_tab_idx = self.tabs.len() - 1;
            }
        }
        false
    }

    pub fn close_tab_at(&mut self, idx: usize) -> bool {
        if idx < self.tabs.len() {
            self.tabs.remove(idx);
            if self.tabs.is_empty() {
                return true;
            }
            if self.active_tab_idx >= self.tabs.len() {
                self.active_tab_idx = self.tabs.len() - 1;
            }
        }
        false
    }

    pub fn has_dead_sessions(&self) -> bool {
        for tab in &self.tabs {
            if !tab.root.find_dead_panes().is_empty() {
                return true;
            }
        }
        false
    }

    pub fn cleanup_dead_sessions(&mut self) -> bool {
        let mut i = 0;
        while i < self.tabs.len() {
            let tab_empty = self.tabs[i].cleanup_dead_panes();
            if tab_empty {
                self.tabs.remove(i);
                if self.active_tab_idx >= self.tabs.len() && !self.tabs.is_empty() {
                    self.active_tab_idx = self.tabs.len() - 1;
                }
            } else {
                i += 1;
            }
        }
        self.tabs.is_empty()
    }

    pub fn select_tab(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active_tab_idx = idx;
        }
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab_idx = (self.active_tab_idx + 1) % self.tabs.len();
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab_idx = if self.active_tab_idx == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab_idx - 1
            };
        }
    }

    pub fn poll_all_ptys(&mut self) -> bool {
        let mut any_data = false;
        for tab in &mut self.tabs {
            if tab.root.poll_all_ptys() {
                any_data = true;
            }
        }
        any_data
    }

    pub fn compute_active_pane_rects(&self, screen_w: f32, screen_h: f32, tab_bar_h: f32) -> Vec<(PaneId, Rect)> {
        let mut rects = Vec::new();
        if let Some(tab) = self.active_tab() {
            let bounds = Rect::new(0.0, tab_bar_h, screen_w, screen_h - tab_bar_h);
            tab.root.compute_layout(bounds, &mut rects);
        }
        rects
    }

    pub fn sync_pane_sizes(
        &mut self,
        screen_w: f32,
        screen_h: f32,
        cell_w: f32,
        cell_h: f32,
        pad_x: f32,
        pad_y: f32,
        tab_bar_h: f32,
    ) {
        let rects = self.compute_active_pane_rects(screen_w, screen_h, tab_bar_h);
        if let Some(tab) = self.active_tab_mut() {
            for (id, rect) in rects {
                if let Some(terminal) = tab.root.find_terminal_mut(id) {
                    let avail_w = (rect.width - pad_x * 2.0).max(cell_w);
                    let avail_h = (rect.height - pad_y * 2.0).max(cell_h);
                    let cols = (avail_w / cell_w).floor() as usize;
                    let rows = (avail_h / cell_h).floor() as usize;
                    let _ = terminal.resize(rows.max(1), cols.max(1));
                }
            }
        }
    }

    /// Update cursor shape across all tabs and panes.
    pub fn update_cursor_shape(&mut self, shape: CursorShape) {
        for tab in &mut self.tabs {
            tab.root.update_cursor_shape(shape);
        }
    }
}
