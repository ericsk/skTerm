//! Geometry for the global AI agents panel.
//!
//! The panel lists every agent running in the application, across all tabs and
//! windows. To make that scope legible it is drawn as a floating card docked to
//! the **right** edge with a gap on every side, rather than as a column butted
//! up against the tab strip -- a panel that starts immediately below the tabs
//! reads as belonging to the selected tab, which is exactly the wrong message.
//!
//! Both the renderer and the GUI hit-testing derive their coordinates from
//! [`AgentPanelLayout`], so the drawn pixels and the clickable regions cannot
//! drift apart.

/// Gap between the panel card and the window edges, in logical points.
pub const PANEL_MARGIN: f32 = 10.0;
/// Grab tolerance either side of the panel's left edge, in logical points.
pub const RESIZE_HANDLE: f32 = 5.0;
/// Header height: a title row plus a scope subtitle row.
pub const HEADER_H: f32 = 52.0;
/// Size of the panel's close button, in logical points.
pub const CLOSE_BTN: f32 = 18.0;
/// Width of the "+ New" button, in logical points.
pub const NEW_BTN_W: f32 = 56.0;
/// Height of the "+ New" button, in logical points.
pub const NEW_BTN_H: f32 = 22.0;
/// Height of one agent card, in logical points.
pub const CARD_H: f32 = 70.0;
/// Vertical gap between agent cards, in logical points.
pub const CARD_GAP: f32 = 6.0;
/// Horizontal inset of the cards inside the panel, in logical points.
pub const CARD_INSET: f32 = 8.0;
/// Gap between the header divider and the first card, in logical points.
pub const LIST_TOP_GAP: f32 = 8.0;
/// Narrowest the panel may be dragged, in logical points.
pub const MIN_PANEL_W: f32 = 160.0;
/// Widest the panel may be dragged, in logical points.
pub const MAX_PANEL_W: f32 = 480.0;

/// An axis-aligned rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl PanelRect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.right() && y >= self.y && y <= self.bottom()
    }

    /// This rectangle as a `[x, y, w, h]` array for the quad instance buffers.
    pub fn as_array(&self) -> [f32; 4] {
        [self.x, self.y, self.w, self.h]
    }
}

/// What the pointer is over inside (or outside) the panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentPanelHit {
    /// The drag handle on the panel's left edge.
    ResizeHandle,
    /// The header's close button.
    CloseButton,
    /// The header's "+ New" button.
    NewButton,
    /// The agent card at this index.
    Card(usize),
    /// Panel chrome that is not interactive, but still swallows the click.
    Background,
    /// Not the panel at all -- the click belongs to the terminal.
    Outside,
}

/// Resolved pixel geometry of the agents panel for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AgentPanelLayout {
    pub scale: f32,
    /// The floating card itself, inset from the window edges.
    pub panel: PanelRect,
    pub header: PanelRect,
    pub close_btn: PanelRect,
    pub new_btn: PanelRect,
    /// Top of the scrolling card list.
    pub list_top: f32,
    pub card_x: f32,
    pub card_w: f32,
    pub card_h: f32,
    pub card_gap: f32,
}

impl AgentPanelLayout {
    /// Builds the layout for a panel of `panel_w` physical pixels docked to the
    /// right of a `screen_w` x `screen_h` window.
    ///
    /// The panel spans the full window height from top (`PANEL_MARGIN`) to bottom
    /// (`screen_h - PANEL_MARGIN`), so its top edge is aligned with the top of
    /// the tab list rather than pushed below `tab_bar_h`.
    pub fn new(screen_w: f32, screen_h: f32, _tab_bar_h: f32, panel_w: f32, scale: f32) -> Self {
        let margin = PANEL_MARGIN * scale;
        let panel = PanelRect::new(
            screen_w - panel_w - margin,
            margin,
            panel_w,
            (screen_h - margin * 2.0).max(0.0),
        );
        let header = PanelRect::new(panel.x, panel.y, panel.w, HEADER_H * scale);

        let close = CLOSE_BTN * scale;
        let close_btn = PanelRect::new(
            panel.right() - CARD_INSET * scale - close,
            panel.y + 5.0 * scale,
            close,
            close,
        );
        let new_btn = PanelRect::new(
            panel.right() - CARD_INSET * scale - NEW_BTN_W * scale,
            panel.y + 26.0 * scale,
            NEW_BTN_W * scale,
            NEW_BTN_H * scale,
        );

        Self {
            scale,
            panel,
            header,
            close_btn,
            new_btn,
            list_top: header.bottom() + LIST_TOP_GAP * scale,
            card_x: panel.x + CARD_INSET * scale,
            card_w: (panel.w - CARD_INSET * 2.0 * scale).max(20.0),
            card_h: CARD_H * scale,
            card_gap: CARD_GAP * scale,
        }
    }

    /// Horizontal space the open panel takes away from the terminal content,
    /// which is the card plus the gap on *both* of its sides.
    pub fn reserved_width(panel_w: f32, scale: f32) -> f32 {
        panel_w + PANEL_MARGIN * 2.0 * scale
    }

    /// Logical panel width implied by dragging the resize handle to `pointer_x`.
    pub fn width_for_pointer(screen_w: f32, pointer_x: f32, scale: f32) -> f32 {
        (((screen_w - pointer_x) / scale) - PANEL_MARGIN).clamp(MIN_PANEL_W, MAX_PANEL_W)
    }

    pub fn card_rect(&self, idx: usize) -> PanelRect {
        PanelRect::new(
            self.card_x,
            self.list_top + idx as f32 * (self.card_h + self.card_gap),
            self.card_w,
            self.card_h,
        )
    }

    /// How many cards fit before running past the bottom of the panel.
    pub fn visible_cards(&self) -> usize {
        let span = self.card_h + self.card_gap;
        if span <= 0.0 {
            return 0;
        }
        let room = self.panel.bottom() - self.list_top + self.card_gap;
        (room / span).floor().max(0.0) as usize
    }

    /// Index of the card at `y`, or `None` above the list, inside a gap, or past
    /// the last card that actually fits on screen.
    pub fn card_index_at(&self, y: f32) -> Option<usize> {
        if y < self.list_top {
            return None;
        }
        let span = self.card_h + self.card_gap;
        let idx = ((y - self.list_top) / span).floor() as usize;
        // Land in the gap between two cards and no card was hit.
        if y - (self.list_top + idx as f32 * span) > self.card_h {
            return None;
        }
        // Rows past the bottom of the card are never drawn, so they must not be
        // clickable either.
        if idx >= self.visible_cards() {
            return None;
        }
        Some(idx)
    }

    /// Classifies a pointer position. The resize handle wins over the panel
    /// body so the edge stays grabbable even where it overlaps the card list.
    pub fn hit(&self, x: f32, y: f32) -> AgentPanelHit {
        if y >= self.panel.y
            && y <= self.panel.bottom()
            && (x - self.panel.x).abs() <= RESIZE_HANDLE * self.scale
        {
            return AgentPanelHit::ResizeHandle;
        }
        // The margin around the card belongs to the panel too: clicks there must
        // not fall through and move the terminal cursor.
        let margin = PANEL_MARGIN * self.scale;
        if x < self.panel.x - margin {
            return AgentPanelHit::Outside;
        }
        if self.close_btn.contains(x, y) {
            return AgentPanelHit::CloseButton;
        }
        if self.new_btn.contains(x, y) {
            return AgentPanelHit::NewButton;
        }
        if self.panel.contains(x, y) {
            if let Some(idx) = self.card_index_at(y) {
                return AgentPanelHit::Card(idx);
            }
        }
        AgentPanelHit::Background
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1000x600 window, no tab bar, 200px panel, scale 1.
    fn layout() -> AgentPanelLayout {
        AgentPanelLayout::new(1000.0, 600.0, 0.0, 200.0, 1.0)
    }

    #[test]
    fn panel_floats_inset_from_the_right_edge() {
        let l = layout();
        assert_eq!(l.panel.x, 1000.0 - 200.0 - PANEL_MARGIN);
        assert_eq!(l.panel.y, PANEL_MARGIN);
        assert_eq!(l.panel.right(), 1000.0 - PANEL_MARGIN);
        assert_eq!(l.panel.bottom(), 600.0 - PANEL_MARGIN);
    }

    #[test]
    fn panel_is_top_aligned_with_the_tab_bar_even_when_tabs_are_shown() {
        let l = AgentPanelLayout::new(1000.0, 600.0, 40.0, 200.0, 1.0);
        assert_eq!(l.panel.y, PANEL_MARGIN);
        assert_eq!(l.panel.bottom(), 600.0 - PANEL_MARGIN);
    }

    #[test]
    fn reserved_width_accounts_for_the_gap_on_both_sides() {
        assert_eq!(
            AgentPanelLayout::reserved_width(200.0, 1.0),
            200.0 + PANEL_MARGIN * 2.0
        );
        // Physical width is already scaled; only the margins scale here.
        assert_eq!(
            AgentPanelLayout::reserved_width(400.0, 2.0),
            400.0 + PANEL_MARGIN * 4.0
        );
    }

    #[test]
    fn dragging_the_handle_widens_the_panel_towards_the_left() {
        // Pointer further left => wider panel.
        let narrow = AgentPanelLayout::width_for_pointer(1000.0, 800.0, 1.0);
        let wide = AgentPanelLayout::width_for_pointer(1000.0, 600.0, 1.0);
        assert!(wide > narrow);
        assert_eq!(narrow, 200.0 - PANEL_MARGIN);
    }

    #[test]
    fn drag_width_is_clamped_to_the_allowed_range() {
        assert_eq!(
            AgentPanelLayout::width_for_pointer(1000.0, 995.0, 1.0),
            MIN_PANEL_W
        );
        assert_eq!(
            AgentPanelLayout::width_for_pointer(1000.0, 0.0, 1.0),
            MAX_PANEL_W
        );
    }

    #[test]
    fn resize_handle_straddles_the_left_edge() {
        let l = layout();
        let edge = l.panel.x;
        assert_eq!(l.hit(edge, 300.0), AgentPanelHit::ResizeHandle);
        assert_eq!(
            l.hit(edge - RESIZE_HANDLE, 300.0),
            AgentPanelHit::ResizeHandle
        );
        assert_eq!(
            l.hit(edge + RESIZE_HANDLE, 300.0),
            AgentPanelHit::ResizeHandle
        );
    }

    #[test]
    fn resize_handle_does_not_swallow_the_first_card() {
        let l = layout();
        let card = l.card_rect(0);
        // Cards are inset far enough that their left edge clears the handle.
        assert!(card.x > l.panel.x + RESIZE_HANDLE);
        assert_eq!(l.hit(card.x + 1.0, card.y + 1.0), AgentPanelHit::Card(0));
    }

    #[test]
    fn header_buttons_are_hit_before_the_card_list() {
        let l = layout();
        assert_eq!(
            l.hit(l.close_btn.x + 1.0, l.close_btn.y + 1.0),
            AgentPanelHit::CloseButton
        );
        assert_eq!(
            l.hit(l.new_btn.x + 1.0, l.new_btn.y + 1.0),
            AgentPanelHit::NewButton
        );
    }

    #[test]
    fn close_and_new_buttons_do_not_overlap() {
        let l = layout();
        assert!(l.new_btn.y >= l.close_btn.bottom());
        assert!(l.close_btn.bottom() <= l.header.bottom());
        assert!(l.new_btn.bottom() <= l.header.bottom());
    }

    #[test]
    fn card_index_maps_positions_to_rows() {
        let l = layout();
        assert_eq!(l.card_index_at(l.list_top - 1.0), None);
        assert_eq!(l.card_index_at(l.list_top), Some(0));
        assert_eq!(l.card_index_at(l.list_top + CARD_H - 1.0), Some(0));
        // The gap between cards belongs to neither.
        assert_eq!(l.card_index_at(l.list_top + CARD_H + 2.0), None);
        assert_eq!(l.card_index_at(l.list_top + CARD_H + CARD_GAP), Some(1));
    }

    #[test]
    fn card_rects_follow_the_index() {
        let l = layout();
        assert_eq!(l.card_rect(0).y, l.list_top);
        assert_eq!(l.card_rect(2).y, l.list_top + 2.0 * (CARD_H + CARD_GAP));
        assert_eq!(l.card_rect(0).w, 200.0 - CARD_INSET * 2.0);
    }

    #[test]
    fn terminal_clicks_to_the_left_of_the_panel_pass_through() {
        let l = layout();
        assert_eq!(l.hit(100.0, 300.0), AgentPanelHit::Outside);
        // The gutter beside the card still belongs to the panel.
        assert_eq!(
            l.hit(l.panel.x - PANEL_MARGIN + 1.0, 300.0),
            AgentPanelHit::Background
        );
    }

    #[test]
    fn empty_space_below_the_last_card_is_panel_background() {
        let l = layout();
        assert_eq!(
            l.hit(l.card_x + 1.0, l.panel.bottom() - 1.0),
            AgentPanelHit::Background
        );
    }

    #[test]
    fn scaling_multiplies_every_logical_metric() {
        let l = AgentPanelLayout::new(2000.0, 1200.0, 0.0, 400.0, 2.0);
        assert_eq!(l.panel.x, 2000.0 - 400.0 - PANEL_MARGIN * 2.0);
        assert_eq!(l.card_h, CARD_H * 2.0);
        assert_eq!(l.card_x, l.panel.x + CARD_INSET * 2.0);
        assert_eq!(l.header.h, HEADER_H * 2.0);
    }

    #[test]
    fn visible_cards_fit_inside_the_panel() {
        let l = layout();
        let n = l.visible_cards();
        assert!(n > 0);
        assert!(l.card_rect(n - 1).bottom() <= l.panel.bottom());
        assert!(l.card_rect(n).bottom() > l.panel.bottom());
    }

    #[test]
    fn cards_past_the_bottom_of_the_panel_are_not_clickable() {
        let l = layout();
        let n = l.visible_cards();
        assert_eq!(l.card_index_at(l.card_rect(n - 1).y + 1.0), Some(n - 1));
        assert_eq!(l.card_index_at(l.card_rect(n).y + 1.0), None);
    }
}
