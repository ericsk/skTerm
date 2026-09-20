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
        Self {
            x,
            y,
            width,
            height,
        }
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

/// Information about a draggable splitter bar between two pane regions.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SplitterBar {
    pub split_id: u32,
    pub direction: SplitDirection,
    pub bar_rect: Rect,
    pub parent_rect: Rect,
    pub ratio: f32,
}

/// Node in the Binary Space Partitioning (BSP) pane tree.
#[allow(clippy::large_enum_variant)]
pub enum PaneNode {
    Leaf {
        id: PaneId,
        terminal: Terminal,
    },
    Split {
        id: u32,
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
            PaneNode::Split { first, second, .. } => first
                .find_terminal(target_id)
                .or_else(|| second.find_terminal(target_id)),
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

    pub fn collect_leaves<'a>(&'a self, out: &mut Vec<(PaneId, &'a Terminal)>) {
        match self {
            PaneNode::Leaf { id, terminal } => out.push((*id, terminal)),
            PaneNode::Split { first, second, .. } => {
                first.collect_leaves(out);
                second.collect_leaves(out);
            }
        }
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

    /// Whether any leaf in this subtree has a dead child process.
    ///
    /// Short-circuits and allocates nothing, unlike `find_dead_panes().is_empty()`.
    /// This is polled frequently, so the "nothing died" answer must be cheap.
    pub fn any_dead(&self) -> bool {
        match self {
            PaneNode::Leaf { terminal, .. } => !terminal.is_child_alive(),
            PaneNode::Split { first, second, .. } => first.any_dead() || second.any_dead(),
        }
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

    /// Splits the pane identified by `target_id`.
    ///
    /// `new_terminal` is passed as `&mut Option<Terminal>` so that ownership of the
    /// (PTY-backed) terminal is transferred exactly once, at the matching leaf. Passing
    /// it by value would require cloning it down every recursion branch, and `Terminal`
    /// cannot be meaningfully cloned because it owns a PTY.
    pub fn split_pane(
        &mut self,
        target_id: PaneId,
        direction: SplitDirection,
        new_id: PaneId,
        new_terminal: &mut Option<Terminal>,
        split_id: u32,
    ) -> bool {
        match self {
            PaneNode::Leaf { id, terminal } => {
                if *id == target_id {
                    let Some(new_terminal) = new_terminal.take() else {
                        // Already consumed by an earlier branch; nothing left to install.
                        return false;
                    };
                    let old_id = *id;
                    let old_terminal = std::mem::replace(terminal, Terminal::new_headless(1, 1, 0));

                    *self = PaneNode::Split {
                        id: split_id,
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
                first.split_pane(target_id, direction, new_id, new_terminal, split_id)
                    || second.split_pane(target_id, direction, new_id, new_terminal, split_id)
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
                id,
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
                                id,
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
                                    id,
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
                                id,
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
                ..
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

    pub fn compute_splitters(&self, rect: Rect, out: &mut Vec<SplitterBar>) {
        if let PaneNode::Split {
            id,
            direction,
            ratio,
            first,
            second,
        } = self
        {
            let sep_size = 2.0;
            match direction {
                SplitDirection::Horizontal => {
                    let w1 = ((rect.width - sep_size) * ratio).floor().max(10.0);
                    let w2 = (rect.width - w1 - sep_size).max(10.0);
                    let bar_rect = Rect::new(rect.x + w1, rect.y, sep_size, rect.height);
                    out.push(SplitterBar {
                        split_id: *id,
                        direction: *direction,
                        bar_rect,
                        parent_rect: rect,
                        ratio: *ratio,
                    });
                    let r1 = Rect::new(rect.x, rect.y, w1, rect.height);
                    let r2 = Rect::new(rect.x + w1 + sep_size, rect.y, w2, rect.height);
                    first.compute_splitters(r1, out);
                    second.compute_splitters(r2, out);
                }
                SplitDirection::Vertical => {
                    let h1 = ((rect.height - sep_size) * ratio).floor().max(10.0);
                    let h2 = (rect.height - h1 - sep_size).max(10.0);
                    let bar_rect = Rect::new(rect.x, rect.y + h1, rect.width, sep_size);
                    out.push(SplitterBar {
                        split_id: *id,
                        direction: *direction,
                        bar_rect,
                        parent_rect: rect,
                        ratio: *ratio,
                    });
                    let r1 = Rect::new(rect.x, rect.y, rect.width, h1);
                    let r2 = Rect::new(rect.x, rect.y + h1 + sep_size, rect.width, h2);
                    first.compute_splitters(r1, out);
                    second.compute_splitters(r2, out);
                }
            }
        }
    }

    pub fn set_split_ratio(&mut self, target_id: u32, new_ratio: f32) -> bool {
        match self {
            PaneNode::Leaf { .. } => false,
            PaneNode::Split {
                id,
                ratio,
                first,
                second,
                ..
            } => {
                if *id == target_id {
                    *ratio = new_ratio.clamp(0.05, 0.95);
                    true
                } else {
                    first.set_split_ratio(target_id, new_ratio)
                        || second.set_split_ratio(target_id, new_ratio)
                }
            }
        }
    }

    pub fn equalize_ratios(&mut self) {
        if let PaneNode::Split {
            ratio,
            first,
            second,
            ..
        } = self
        {
            *ratio = 0.5;
            first.equalize_ratios();
            second.equalize_ratios();
        }
    }

    pub fn adjust_pane_ratio(&mut self, target_pane_id: PaneId, delta: f32) -> bool {
        match self {
            PaneNode::Leaf { .. } => false,
            PaneNode::Split {
                ratio,
                first,
                second,
                ..
            } => {
                let in_first = first.all_leaf_ids().contains(&target_pane_id);
                let in_second = second.all_leaf_ids().contains(&target_pane_id);

                if in_first && in_second {
                    first.adjust_pane_ratio(target_pane_id, delta)
                        || second.adjust_pane_ratio(target_pane_id, delta)
                } else if in_first {
                    if first.adjust_pane_ratio(target_pane_id, delta) {
                        true
                    } else {
                        *ratio = (*ratio + delta).clamp(0.05, 0.95);
                        true
                    }
                } else if in_second {
                    if second.adjust_pane_ratio(target_pane_id, delta) {
                        true
                    } else {
                        *ratio = (*ratio - delta).clamp(0.05, 0.95);
                        true
                    }
                } else {
                    false
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

/// A single tab containing a BSP tree of panes.
pub struct Tab {
    pub id: TabId,
    pub title: String,
    pub root: PaneNode,
    pub active_pane_id: PaneId,
    pub zoomed_pane_id: Option<PaneId>,
    pub next_split_id: u32,
    pub has_unseen_output: bool,
}

impl Tab {
    pub fn new(
        id: TabId,
        title: String,
        rows: usize,
        cols: usize,
        pane_id: PaneId,
    ) -> anyhow::Result<Self> {
        Self::new_with_options(
            id,
            title,
            rows,
            cols,
            pane_id,
            10000,
            PtySpawnOptions::default(),
        )
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
            root: PaneNode::Leaf {
                id: pane_id,
                terminal,
            },
            active_pane_id: pane_id,
            zoomed_pane_id: None,
            next_split_id: 1,
            has_unseen_output: false,
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
        self.split_active_with_options(
            direction,
            next_pane_id,
            rows,
            cols,
            10000,
            PtySpawnOptions::default(),
        )
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

        let split_id = self.next_split_id;
        self.next_split_id += 1;

        let new_terminal = Terminal::new_with_options(rows, cols, scrollback_lines, options)?;
        let mut new_terminal = Some(new_terminal);
        let installed = self.root.split_pane(
            self.active_pane_id,
            direction,
            new_id,
            &mut new_terminal,
            split_id,
        );
        if !installed {
            // Target pane vanished; don't leave a phantom pane id behind.
            anyhow::bail!("split target pane {} not found", self.active_pane_id);
        }
        self.active_pane_id = new_id;
        self.zoomed_pane_id = None;
        Ok(new_id)
    }

    pub fn close_pane_by_id(&mut self, target: PaneId) -> bool {
        if self.zoomed_pane_id == Some(target) {
            self.zoomed_pane_id = None;
        }

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

    pub fn toggle_zoom(&mut self) -> bool {
        if self.zoomed_pane_id.is_some() {
            self.zoomed_pane_id = None;
            false
        } else {
            self.zoomed_pane_id = Some(self.active_pane_id);
            true
        }
    }

    pub fn is_zoomed(&self) -> bool {
        self.zoomed_pane_id.is_some()
    }

    pub fn find_splitter_at(&self, bounds: Rect, px: f32, py: f32) -> Option<SplitterBar> {
        if self.is_zoomed() {
            return None;
        }
        let mut splitters = Vec::new();
        self.root.compute_splitters(bounds, &mut splitters);
        let hit_margin = 5.0; // 5px margin on each side of the splitter line
        for s in splitters {
            match s.direction {
                SplitDirection::Horizontal => {
                    if px >= (s.bar_rect.x - hit_margin)
                        && px <= (s.bar_rect.x + s.bar_rect.width + hit_margin)
                        && py >= s.bar_rect.y
                        && py <= (s.bar_rect.y + s.bar_rect.height)
                    {
                        return Some(s);
                    }
                }
                SplitDirection::Vertical => {
                    if py >= (s.bar_rect.y - hit_margin)
                        && py <= (s.bar_rect.y + s.bar_rect.height + hit_margin)
                        && px >= s.bar_rect.x
                        && px <= (s.bar_rect.x + s.bar_rect.width)
                    {
                        return Some(s);
                    }
                }
            }
        }
        None
    }

    pub fn set_split_ratio(&mut self, split_id: u32, ratio: f32) -> bool {
        self.root.set_split_ratio(split_id, ratio)
    }

    pub fn adjust_pane_ratio(&mut self, pane_id: PaneId, delta: f32) -> bool {
        self.root.adjust_pane_ratio(pane_id, delta)
    }

    pub fn equalize_panes(&mut self) {
        self.root.equalize_ratios();
    }
}

/// What a `Workspace::poll_all_ptys` sweep actually changed.
///
/// Separating "some pane produced bytes" from "something the user can see
/// changed" is what lets the event loop skip redraws for background tabs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PollOutcome {
    /// The visible tab produced output, so terminal content changed.
    pub active_tab_dirty: bool,
    /// A background tab's activity dot just turned on, so the tab bar changed.
    pub tab_bar_dirty: bool,
    /// Any pane at all produced output, visible or not. Used for window-level
    /// activity tracking, not for deciding whether to repaint.
    pub any_output: bool,
}

impl PollOutcome {
    /// Whether anything currently on screen changed and a repaint is warranted.
    pub fn needs_redraw(&self) -> bool {
        self.active_tab_dirty || self.tab_bar_dirty
    }
}

/// Workspace holding multiple tabs and managing global layouts.
pub struct Workspace {
    pub tabs: Vec<Tab>,
    pub active_tab_idx: usize,
    pub sidebar_open: bool,
    pub sidebar_width: f32,
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
            sidebar_open: false,
            sidebar_width: 220.0,
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
            tab.split_active_with_options(
                direction,
                &mut next_id,
                rows,
                cols,
                scrollback_lines,
                options,
            )
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
        self.tabs.iter().any(|tab| tab.root.any_dead())
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
            self.tabs[idx].has_unseen_output = false;
        }
    }

    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab_idx = (self.active_tab_idx + 1) % self.tabs.len();
            self.tabs[self.active_tab_idx].has_unseen_output = false;
        }
    }

    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab_idx = if self.active_tab_idx == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab_idx - 1
            };
            self.tabs[self.active_tab_idx].has_unseen_output = false;
        }
    }

    pub fn toggle_zoom_active_pane(&mut self) -> bool {
        if let Some(tab) = self.active_tab_mut() {
            tab.toggle_zoom()
        } else {
            false
        }
    }

    pub fn is_active_tab_zoomed(&self) -> bool {
        self.active_tab().is_some_and(|t| t.is_zoomed())
    }

    pub fn set_split_ratio(&mut self, split_id: u32, ratio: f32) -> bool {
        if let Some(tab) = self.active_tab_mut() {
            tab.set_split_ratio(split_id, ratio)
        } else {
            false
        }
    }

    pub fn adjust_active_pane_ratio(&mut self, delta: f32) -> bool {
        if let Some(tab) = self.active_tab_mut() {
            let active_id = tab.active_pane_id;
            tab.adjust_pane_ratio(active_id, delta)
        } else {
            false
        }
    }

    pub fn equalize_active_panes(&mut self) {
        if let Some(tab) = self.active_tab_mut() {
            tab.equalize_panes();
        }
    }

    pub fn move_tab_left(&mut self) -> bool {
        if self.active_tab_idx > 0 && self.tabs.len() > 1 {
            self.tabs.swap(self.active_tab_idx, self.active_tab_idx - 1);
            self.active_tab_idx -= 1;
            true
        } else {
            false
        }
    }

    pub fn move_tab_right(&mut self) -> bool {
        if self.active_tab_idx + 1 < self.tabs.len() {
            self.tabs.swap(self.active_tab_idx, self.active_tab_idx + 1);
            self.active_tab_idx += 1;
            true
        } else {
            false
        }
    }

    pub fn rename_active_tab(&mut self, new_title: String) {
        if let Some(tab) = self.active_tab_mut() {
            tab.title = new_title;
        }
    }

    /// Drain every pane's PTY in this workspace and report what actually changed
    /// *on screen*.
    ///
    /// The distinction matters for idle power: output in a background tab is not
    /// visible, so it must not drive a full GPU re-render of the tab the user is
    /// actually looking at. A `tail -f` or `cargo watch` in a background tab used
    /// to pin the render path continuously.
    pub fn poll_all_ptys(&mut self) -> PollOutcome {
        let mut outcome = PollOutcome::default();
        let active_idx = self.active_tab_idx;
        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            if tab.root.poll_all_ptys() {
                outcome.any_output = true;
                if idx == active_idx {
                    outcome.active_tab_dirty = true;
                } else if !tab.has_unseen_output {
                    // The activity dot is turning on for the first time, which is
                    // itself a visible change to the tab bar. Further output while
                    // the dot is already lit changes nothing on screen.
                    tab.has_unseen_output = true;
                    outcome.tab_bar_dirty = true;
                }
            }
        }
        outcome
    }

    pub fn content_bounds(
        &self,
        screen_w: f32,
        screen_h: f32,
        tab_bar_h: f32,
        scale_factor: f32,
    ) -> Rect {
        // The agents panel is docked to the right as a floating card, so the
        // terminal keeps the left edge and gives up the card plus the gutters on
        // either side of it.
        let reserved = if self.sidebar_open {
            skterm_render::AgentPanelLayout::reserved_width(
                self.sidebar_width * scale_factor,
                scale_factor,
            )
        } else {
            0.0
        };
        Rect::new(
            0.0,
            tab_bar_h,
            (screen_w - reserved).max(0.0),
            (screen_h - tab_bar_h).max(0.0),
        )
    }

    pub fn compute_active_pane_rects(
        &self,
        screen_w: f32,
        screen_h: f32,
        tab_bar_h: f32,
    ) -> Vec<(PaneId, Rect)> {
        self.compute_active_pane_rects_with_scale(screen_w, screen_h, tab_bar_h, 1.0)
    }

    pub fn compute_active_pane_rects_with_scale(
        &self,
        screen_w: f32,
        screen_h: f32,
        tab_bar_h: f32,
        scale_factor: f32,
    ) -> Vec<(PaneId, Rect)> {
        let mut rects = Vec::new();
        if let Some(tab) = self.active_tab() {
            let bounds = self.content_bounds(screen_w, screen_h, tab_bar_h, scale_factor);
            if let Some(zid) = tab.zoomed_pane_id {
                if tab.root.find_terminal(zid).is_some() {
                    rects.push((zid, bounds));
                    return rects;
                }
            }
            tab.root.compute_layout(bounds, &mut rects);
        }
        rects
    }

    #[allow(clippy::too_many_arguments)]
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
        self.sync_pane_sizes_with_scale(
            screen_w, screen_h, cell_w, cell_h, pad_x, pad_y, tab_bar_h, 1.0,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn sync_pane_sizes_with_scale(
        &mut self,
        screen_w: f32,
        screen_h: f32,
        cell_w: f32,
        cell_h: f32,
        pad_x: f32,
        pad_y: f32,
        tab_bar_h: f32,
        scale_factor: f32,
    ) {
        let rects =
            self.compute_active_pane_rects_with_scale(screen_w, screen_h, tab_bar_h, scale_factor);
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

    /// Scan all tabs and panes to find running AI Agent CLI instances.
    pub fn scan_ai_agents(
        &self,
        cache: &mut ProcessTreeCache,
    ) -> Vec<skterm_render::AgentSidebarItem> {
        cache.refresh_if_needed();
        let mut items = Vec::new();

        for (tab_idx, tab) in self.tabs.iter().enumerate() {
            let mut leaves = Vec::new();
            tab.root.collect_leaves(&mut leaves);

            for (pane_id, terminal) in leaves {
                let mut found_agent: Option<(u32, String)> = None;

                if let Some(shell_pid) = terminal.process_id() {
                    if let Some((agent_pid, name)) = cache.find_agent_descendant(shell_pid) {
                        found_agent = Some((agent_pid, name.to_string()));
                    }
                }

                if found_agent.is_none() {
                    let title_lower = terminal.state.title.to_lowercase();
                    if title_lower.contains("jetski") {
                        let pid = terminal.process_id().unwrap_or(0);
                        found_agent = Some((pid, "Jetski".to_string()));
                    } else if title_lower.contains("antigravity") || title_lower.contains("agy") {
                        let pid = terminal.process_id().unwrap_or(0);
                        found_agent = Some((pid, "Antigravity CLI".to_string()));
                    } else if title_lower.contains("claude") {
                        let pid = terminal.process_id().unwrap_or(0);
                        found_agent = Some((pid, "Claude Code".to_string()));
                    } else if title_lower.contains("gemini") {
                        let pid = terminal.process_id().unwrap_or(0);
                        found_agent = Some((pid, "Gemini CLI".to_string()));
                    }
                }

                if let Some((agent_pid, agent_name)) = found_agent {
                    let project_name = if agent_pid > 0 {
                        cache.get_project_name(agent_pid)
                    } else if let Some(spid) = terminal.process_id() {
                        cache.get_project_name(spid)
                    } else {
                        "workspace".to_string()
                    };

                    let location_label = format!("Tab {} · #{}", tab_idx + 1, pane_id);
                    let title_summary = if let Some(ref custom) = terminal.custom_label {
                        if !custom.trim().is_empty() {
                            custom.clone()
                        } else if terminal.state.title != "skTerm"
                            && !terminal.state.title.is_empty()
                        {
                            terminal.state.title.clone()
                        } else {
                            "Double-click to name...".to_string()
                        }
                    } else if terminal.state.title != "skTerm" && !terminal.state.title.is_empty() {
                        terminal.state.title.clone()
                    } else {
                        "Double-click to name...".to_string()
                    };

                    let is_focused =
                        tab_idx == self.active_tab_idx && tab.active_pane_id == pane_id;

                    items.push(skterm_render::AgentSidebarItem {
                        tab_id: tab.id,
                        pane_id,
                        agent_name,
                        project_name,
                        location_label,
                        title_summary,
                        is_active_output: terminal.is_active_output(),
                        is_focused,
                    });
                }
            }
        }

        items
    }
}

/// Parent PID -> its child `(pid, process name)` pairs, shared with the
/// background refresh thread that repopulates the tree off the UI thread.
type ChildrenMap =
    std::sync::Arc<std::sync::Mutex<std::collections::HashMap<u32, Vec<(u32, String)>>>>;

/// Cached system process tree and CWD lookups for low-overhead AI Agent discovery.
pub struct ProcessTreeCache {
    last_refresh: Option<std::time::Instant>,
    children_map: ChildrenMap,
    is_refreshing: std::sync::Arc<std::sync::atomic::AtomicBool>,
    cwd_cache: std::collections::HashMap<u32, String>,
}

impl Default for ProcessTreeCache {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessTreeCache {
    pub fn new() -> Self {
        Self {
            // Defer first process tree scan so cold startup frame renders immediately with zero subprocess delay
            last_refresh: Some(std::time::Instant::now()),
            children_map: std::sync::Arc::new(std::sync::Mutex::new(
                std::collections::HashMap::new(),
            )),
            is_refreshing: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            cwd_cache: std::collections::HashMap::new(),
        }
    }

    pub fn refresh_if_needed(&mut self) {
        if let Some(last) = self.last_refresh {
            if last.elapsed() < std::time::Duration::from_millis(1200) {
                return;
            }
        }
        if self
            .is_refreshing
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::SeqCst,
                std::sync::atomic::Ordering::SeqCst,
            )
            .is_err()
        {
            return;
        }

        self.last_refresh = Some(std::time::Instant::now());
        let map_arc = self.children_map.clone();
        let flag_arc = self.is_refreshing.clone();

        std::thread::spawn(move || {
            let mut new_map: std::collections::HashMap<u32, Vec<(u32, String)>> =
                std::collections::HashMap::new();

            #[cfg(unix)]
            {
                if let Ok(output) = std::process::Command::new("ps")
                    .args(["-ax", "-o", "pid=,ppid=,args="])
                    .output()
                {
                    if let Ok(text) = String::from_utf8(output.stdout) {
                        for line in text.lines() {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if parts.len() >= 3 {
                                if let (Ok(pid), Ok(ppid)) =
                                    (parts[0].parse::<u32>(), parts[1].parse::<u32>())
                                {
                                    let cmd_line = parts[2..].join(" ").to_lowercase();
                                    new_map.entry(ppid).or_default().push((pid, cmd_line));
                                }
                            }
                        }
                    }
                }
            }

            if let Ok(mut guard) = map_arc.lock() {
                *guard = new_map;
            }
            flag_arc.store(false, std::sync::atomic::Ordering::SeqCst);
        });
    }

    fn match_agent_command(cmd_line: &str) -> Option<&'static str> {
        for token in cmd_line.split_whitespace() {
            let base = std::path::Path::new(token)
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(token);
            if base == "jetski" {
                return Some("Jetski");
            } else if base == "agy" || base == "antigravity" {
                return Some("Antigravity CLI");
            } else if base == "claude" {
                return Some("Claude Code");
            } else if base == "gemini" {
                return Some("Gemini CLI");
            } else if base == "aider" {
                return Some("Aider");
            } else if base == "codex" {
                return Some("Codex CLI");
            }
        }
        None
    }

    /// Search descendant processes of `root_pid` for a known AI Agent CLI.
    pub fn find_agent_descendant(&self, root_pid: u32) -> Option<(u32, &'static str)> {
        let guard = self.children_map.lock().ok()?;
        let mut queue = std::collections::VecDeque::new();
        queue.push_back(root_pid);
        let mut visited = std::collections::HashSet::new();
        visited.insert(root_pid);

        while let Some(curr_pid) = queue.pop_front() {
            if let Some(children) = guard.get(&curr_pid) {
                for (child_pid, cmd_line) in children {
                    if !visited.insert(*child_pid) {
                        continue;
                    }
                    if let Some(agent_name) = Self::match_agent_command(cmd_line) {
                        return Some((*child_pid, agent_name));
                    }
                    queue.push_back(*child_pid);
                }
            }
        }
        None
    }

    pub fn get_project_name(&mut self, pid: u32) -> String {
        if pid == 0 {
            return "workspace".to_string();
        }
        if let Some(cached) = self.cwd_cache.get(&pid) {
            return cached.clone();
        }
        #[cfg(target_os = "linux")]
        {
            if let Ok(path) = std::fs::read_link(format!("/proc/{}/cwd", pid)) {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    let name_str = name.to_string();
                    self.cwd_cache.insert(pid, name_str.clone());
                    return name_str;
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("lsof")
                .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
                .output()
            {
                if let Ok(text) = String::from_utf8(output.stdout) {
                    for line in text.lines() {
                        if let Some(path_str) = line.strip_prefix('n') {
                            let name = std::path::Path::new(path_str)
                                .file_name()
                                .and_then(|s| s.to_str())
                                .unwrap_or(path_str)
                                .to_string();
                            self.cwd_cache.insert(pid, name.clone());
                            return name;
                        }
                    }
                }
            }
        }
        // Cache fallback so we never repeatedly run failing lookups on every frame
        self.cwd_cache.insert(pid, "workspace".to_string());
        "workspace".to_string()
    }
}
