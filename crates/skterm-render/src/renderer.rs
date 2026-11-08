//! Main Terminal Renderer combining wgpu Device, Queue, Surface, Atlas, Graphics Images, and Pipelines.

use crate::agent_panel::AgentPanelLayout;
use crate::atlas::GlyphAtlas;
use crate::box_drawing::draw_custom_block_or_box_char;
use crate::pipeline::{color_to_rgba, BgInstance, GlyphInstance, ImageInstance, TerminalPipeline};
use skterm_core::{CellFlags, Color, CursorShape, ScreenGrid};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;
use wgpu::{
    Backends, CompositeAlphaMode, Device, DeviceDescriptor, Extent3d, Features, Instance,
    InstanceDescriptor, Limits, Origin3d, PowerPreference, PresentMode, Queue,
    RequestAdapterOptions, Surface, SurfaceConfiguration, TexelCopyBufferLayout,
    TexelCopyTextureInfo, TextureAspect, TextureDimension, TextureFormat, TextureUsages,
};
use winit::window::Window;

/// Theme color overrides for custom themes and ANSI palettes
#[derive(Debug, Clone, Copy)]
pub struct RenderTheme {
    pub default_fg: [f32; 4],
    pub default_bg: [f32; 4],
    pub cursor_color: [f32; 4],
    pub selection_color: [f32; 4],
    pub tab_bar_bg: [f32; 4],
    pub tab_active_bg: [f32; 4],
    pub tab_active_fg: [f32; 4],
    pub tab_inactive_fg: [f32; 4],
    pub border_color: [f32; 4],
    pub focused_border_color: [f32; 4],
    pub ansi_palette: Option<[[f32; 4]; 16]>,
}

impl Default for RenderTheme {
    fn default() -> Self {
        Self {
            default_fg: [0.96, 0.96, 0.97, 1.0],
            default_bg: [0.00, 0.00, 0.00, 1.0], // OLED Pure Black (#000000)
            cursor_color: [0.039, 0.518, 1.00, 0.85],
            selection_color: [0.18, 0.28, 0.45, 0.5],
            tab_bar_bg: [0.00, 0.00, 0.00, 1.0],
            tab_active_bg: [0.00, 0.00, 0.00, 1.0],
            tab_active_fg: [0.96, 0.96, 0.97, 1.0],
            tab_inactive_fg: [0.55, 0.58, 0.65, 1.0],
            border_color: [0.20, 0.22, 0.28, 1.0],
            focused_border_color: [0.039, 0.518, 1.00, 1.0],
            ansi_palette: None,
        }
    }
}

/// Layout metrics and geometry for a pane's vertical scrollbar.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarMetrics {
    pub track_x: f32,
    pub track_y: f32,
    pub track_w: f32,
    pub track_h: f32,
    pub thumb_x: f32,
    pub thumb_y: f32,
    pub thumb_w: f32,
    pub thumb_h: f32,
    pub travel: f32,
    pub hit_x: f32,
    pub hit_y: f32,
    pub hit_w: f32,
    pub hit_h: f32,
}

impl ScrollbarMetrics {
    // The scrollbar geometry is derived from the pane rect, the scale, and the
    // scroll state, all of which are plain scalars supplied by the caller at the
    // call site. Wrapping them in a struct would just relocate the same list.
    #[allow(clippy::too_many_arguments)]
    pub fn compute(
        rect_x: f32,
        rect_y: f32,
        rect_w: f32,
        rect_h: f32,
        scale: f32,
        scrollback_len: usize,
        screen_rows: usize,
        scroll_offset: usize,
        is_hovered: bool,
        is_dragging: bool,
    ) -> Option<Self> {
        if scrollback_len == 0 {
            return None;
        }

        let track_w = 8.0 * scale;
        let track_x = rect_x + rect_w - track_w - 2.0 * scale;
        let track_y = rect_y + 2.0 * scale;
        let track_h = (rect_h - 4.0 * scale).max(10.0 * scale);

        let total_rows = scrollback_len + screen_rows;
        let visible_ratio = (screen_rows as f32 / total_rows as f32).clamp(0.02, 1.0);
        let thumb_h = (track_h * visible_ratio).clamp(20.0 * scale, track_h);
        let travel = (track_h - thumb_h).max(0.0);

        let progress = 1.0 - (scroll_offset as f32 / scrollback_len as f32).clamp(0.0, 1.0);
        let thumb_y = track_y + progress * travel;

        let is_active = is_hovered || is_dragging;
        let thumb_w = if is_active { 7.0 * scale } else { 5.0 * scale };
        let thumb_x = rect_x + rect_w - thumb_w - if is_active { 2.5 * scale } else { 3.5 * scale };

        let hit_w = 10.0 * scale;
        let hit_x = rect_x + rect_w - hit_w;
        let hit_y = rect_y;
        let hit_h = rect_h;

        Some(Self {
            track_x,
            track_y,
            track_w,
            track_h,
            thumb_x,
            thumb_y,
            thumb_w,
            thumb_h,
            travel,
            hit_x,
            hit_y,
            hit_w,
            hit_h,
        })
    }
}

/// Display width in terminal grid cells for a single character (`1` for narrow/ASCII,
/// `2` for wide CJK/Emoji).
#[inline]
pub fn char_display_cells(c: char) -> usize {
    unicode_width::UnicodeWidthChar::width(c)
        .unwrap_or(1)
        .max(1)
}

/// Number of terminal grid cells occupied by `text` when rendered inline.
pub fn text_display_cells(text: &str) -> usize {
    text.chars().map(char_display_cells).sum()
}

/// Horizontal pixel offset to center a multi-cell wide glyph's typographic advance box
/// inside its allocated `cells * cell_w` terminal grid span (e.g. centering a `1.0em` CJK
/// glyph inside a `2 * cell_w` = `1.25em` monospace double-cell), while leaving all
/// single-cell glyphs (ASCII, Nerd Font icons, Powerline triangles) untouched at `0.0`.
#[inline]
pub fn wide_cell_center_offset(c: char, glyph_advance: f32, cell_w: f32) -> f32 {
    let cells = char_display_cells(c);
    if cells > 1 && glyph_advance > 0.0 {
        let span_w = cells as f32 * cell_w;
        if glyph_advance < span_w {
            return ((span_w - glyph_advance) * 0.5).round();
        }
    }
    0.0
}

/// Lay out `text` inside `[x, x + max_w]` using per-glyph `(advance_px, payload)` from
/// `lookup(c)`. Advancing by each glyph's true typographic advance width ensures uniform
/// visual spacing across mixed ASCII/digits and full-width CJK characters in UI text
/// (such as tab capsules, menus, and sidebars). When `text` exceeds `max_w`, truncates on
/// a character boundary and appends `'…'` (if `'…'` itself fits).
pub fn for_each_ellipsized_glyph<T>(
    text: &str,
    x: f32,
    max_w: f32,
    mut lookup: impl FnMut(char) -> (f32, T),
    mut emit: impl FnMut(char, f32, T),
) {
    if text.is_empty() || max_w <= 0.0 {
        return;
    }

    let mut total_w = 0.0_f32;
    let mut overflows = false;
    for c in text.chars() {
        let (adv, _) = lookup(c);
        total_w += adv.max(0.0);
        if total_w > max_w + 1e-3 {
            overflows = true;
            break;
        }
    }

    if !overflows {
        let mut cur_x = x;
        for c in text.chars() {
            let (adv, payload) = lookup(c);
            emit(c, cur_x, payload);
            cur_x += adv.max(0.0);
        }
        return;
    }

    let ellipsis = '…';
    let (ellipsis_adv_raw, ellipsis_payload) = lookup(ellipsis);
    let ellipsis_adv = ellipsis_adv_raw.max(0.0);
    if ellipsis_adv <= 0.0 || ellipsis_adv > max_w + 1e-3 {
        return;
    }

    let prefix_budget = (max_w - ellipsis_adv).max(0.0);
    let mut used = 0.0_f32;
    for c in text.chars() {
        let (adv_raw, payload) = lookup(c);
        let adv = adv_raw.max(0.0);
        if used + adv > prefix_budget + 1e-3 {
            break;
        }
        emit(c, x + used, payload);
        used += adv;
    }
    emit(ellipsis, x + used, ellipsis_payload);
}

/// Computed horizontal geometry for an inline IME preedit overlay inside a pane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PreeditOverlayLayout {
    pub start_x: f32,
    pub box_w: f32,
    pub avail_w: f32,
    pub selected_clause: Option<(f32, f32)>,
    pub caret_x: f32,
}

impl PreeditOverlayLayout {
    /// Compute the `[start_col, end_col)` range of terminal grid columns on the cursor row
    /// occupied by inline IME preedit text, matching Ghostty/cmux's `rebuildCells` behavior.
    pub fn compute_grid_cols(
        preedit: &str,
        cursor_col: usize,
        cols: usize,
    ) -> Option<(usize, usize)> {
        if preedit.is_empty() || cols == 0 {
            return None;
        }
        let total_cells = text_display_cells(preedit);
        if total_cells == 0 {
            return None;
        }
        let start_col = if cursor_col + total_cells > cols {
            cols.saturating_sub(total_cells).min(cursor_col)
        } else {
            cursor_col
        };
        let mut end_col = start_col;
        for c in preedit.chars() {
            let w = unicode_width::UnicodeWidthChar::width(c)
                .unwrap_or(1)
                .max(1);
            if end_col + w > cols {
                break;
            }
            end_col += w;
        }
        if end_col > start_col {
            Some((start_col, end_col))
        } else {
            None
        }
    }

    pub fn compute(
        preedit: &str,
        cursor_range: Option<(usize, usize)>,
        cur_x: f32,
        pane_x: f32,
        pane_w: f32,
        padding_x: f32,
        cell_w: f32,
    ) -> Option<Self> {
        if preedit.is_empty() || cell_w <= 0.0 || pane_w <= 0.0 {
            return None;
        }
        let total_cells = text_display_cells(preedit).max(1);
        let preedit_w = total_cells as f32 * cell_w;

        let min_left = pane_x + padding_x;
        let max_right = (pane_x + pane_w - padding_x).max(min_left + cell_w);
        let start_x = if cur_x + preedit_w > max_right {
            (max_right - preedit_w).max(min_left)
        } else {
            cur_x.max(min_left)
        };
        let avail_w = (pane_x + pane_w - start_x).max(0.0);
        let box_w = preedit_w.min(avail_w);
        if box_w <= 0.0 {
            return None;
        }

        let byte_offset_to_px = |byte_idx: usize| -> f32 {
            let clamped = byte_idx.min(preedit.len());
            let cells: usize = preedit
                .char_indices()
                .take_while(|&(idx, _)| idx < clamped)
                .map(|(_, c)| {
                    unicode_width::UnicodeWidthChar::width(c)
                        .unwrap_or(1)
                        .max(1)
                })
                .sum();
            (start_x + cells as f32 * cell_w).min(start_x + box_w)
        };

        let (selected_clause, caret_x) = match cursor_range {
            Some((start_b, end_b)) => {
                let sx = byte_offset_to_px(start_b);
                let ex = byte_offset_to_px(end_b);
                let clause = if ex > sx { Some((sx, ex - sx)) } else { None };
                (clause, ex)
            }
            None => (None, start_x + box_w),
        };

        Some(Self {
            start_x,
            box_w,
            avail_w,
            selected_clause,
            caret_x,
        })
    }
}

/// Render descriptor for an individual pane on screen
pub struct PaneRenderItem<'a> {
    pub grid: &'a ScreenGrid,
    pub rect_x: f32,
    pub rect_y: f32,
    pub rect_w: f32,
    pub rect_h: f32,
    pub is_focused: bool,
    pub search_matches: &'a [skterm_core::SearchMatch],
    pub active_search_idx: Option<usize>,
    pub search_query: Option<&'a str>,
    pub ime_preedit: Option<&'a str>,
    pub ime_preedit_cursor: Option<(usize, usize)>,
    pub is_scrollbar_hovered: bool,
    pub is_scrollbar_dragging: bool,
    pub is_recently_scrolled: bool,
    pub dynamic_palette: Option<&'a HashMap<u8, (u8, u8, u8)>>,
    pub dynamic_fg: Option<(u8, u8, u8)>,
    pub dynamic_bg: Option<(u8, u8, u8)>,
    pub dynamic_cursor: Option<(u8, u8, u8)>,
}

/// Render descriptor for a tab item in the tab bar
pub struct TabItem {
    pub title: String,
    pub is_active: bool,
}

/// Lifecycle status of an AI Agent displayed in the global agents sidebar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentStatus {
    /// Idle / waiting for user prompt (Warm Yellow indicator).
    #[default]
    Idle,
    /// Actively working / generating output (Emerald Green indicator).
    Working,
    /// Task finished (`OSC 7501` `state=done`, Sky Cyan indicator).
    Done,
    /// Blocked waiting for user input (`OSC 7501` `state=blocked`, Amber indicator).
    NeedsInput,
    /// Blocked waiting for tool/action permission (`OSC 7501` `state=blocked:kind=permission`, Amber indicator).
    NeedsPermission,
    /// Blocked waiting for user to answer a question (`OSC 7501` `state=blocked:kind=question`, Amber indicator).
    NeedsAnswer,
    /// Blocked waiting for authentication (`OSC 7501` `state=blocked:kind=auth`, Amber indicator).
    NeedsSignIn,
    /// Stopped with an error (`OSC 7501` `state=error`, Crimson Red indicator).
    Error,
}

impl AgentStatus {
    /// Map an [`skterm_core::ProgramStatusRecord`] to its sidebar [`AgentStatus`].
    pub fn from_program_status(rec: &skterm_core::ProgramStatusRecord) -> Self {
        use skterm_core::{ProgramStatusKind, ProgramStatusState};
        match rec.state {
            ProgramStatusState::Working => Self::Working,
            ProgramStatusState::Done => Self::Done,
            ProgramStatusState::Blocked => match rec.kind {
                ProgramStatusKind::Permission => Self::NeedsPermission,
                ProgramStatusKind::Question => Self::NeedsAnswer,
                ProgramStatusKind::Auth => Self::NeedsSignIn,
                ProgramStatusKind::None => Self::NeedsInput,
            },
            ProgramStatusState::Error => Self::Error,
            ProgramStatusState::Idle | ProgramStatusState::Clear => Self::Idle,
        }
    }

    /// Short human-readable status label when no custom `status_message` is provided.
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "Working",
            Self::Done => "Done",
            Self::NeedsPermission => "Needs permission",
            Self::NeedsAnswer => "Needs answer",
            Self::NeedsSignIn => "Needs sign-in",
            Self::NeedsInput => "Needs input",
            Self::Error => "Error",
            Self::Idle => "Idle",
        }
    }

    /// True if the agent is blocked waiting on user interaction.
    pub fn is_blocked(self) -> bool {
        matches!(
            self,
            Self::NeedsInput | Self::NeedsPermission | Self::NeedsAnswer | Self::NeedsSignIn
        )
    }

    /// Status indicator dot RGBA color.
    pub fn dot_color(self) -> [f32; 4] {
        match self {
            Self::Working => [0.18, 0.88, 0.46, 1.0],
            Self::Done => [0.28, 0.78, 0.96, 1.0],
            Self::NeedsInput | Self::NeedsPermission | Self::NeedsAnswer | Self::NeedsSignIn => {
                [1.0, 0.62, 0.18, 1.0]
            }
            Self::Error => [0.96, 0.32, 0.34, 1.0],
            Self::Idle => [0.96, 0.76, 0.22, 1.0],
        }
    }
}

/// Render descriptor for an AI Agent item in the global agents panel
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentSidebarItem {
    pub tab_id: u32,
    pub pane_id: u32,
    pub agent_name: String,
    pub project_name: String,
    pub location_label: String,
    pub title_summary: String,
    pub is_active_output: bool,
    pub is_focused: bool,
    pub status: AgentStatus,
    pub status_message: Option<String>,
}

impl AgentSidebarItem {
    /// Indicator dot color for this agent card, taking both `status` and `is_active_output` into account.
    pub fn dot_color(&self) -> [f32; 4] {
        if self.status != AgentStatus::Idle {
            self.status.dot_color()
        } else if self.is_active_output {
            AgentStatus::Working.dot_color()
        } else {
            AgentStatus::Idle.dot_color()
        }
    }

    /// Formatted status string for the bottom subtitle line of the sidebar card.
    pub fn status_display(&self) -> String {
        if let Some(ref msg) = self.status_message {
            let trimmed = msg.trim();
            if !trimmed.is_empty() {
                if self.status == AgentStatus::Error
                    && !trimmed.to_ascii_lowercase().contains("error")
                {
                    return format!("Error: {trimmed}");
                }
                return trimmed.to_string();
            }
        }
        if self.status != AgentStatus::Idle {
            self.status.label().to_string()
        } else if self.is_active_output {
            "Running".to_string()
        } else {
            "Idle".to_string()
        }
    }
}

/// Everything the renderer needs to draw the global AI agents panel.
///
/// Bundled into one struct rather than passed as loose arguments so the panel's
/// scope (`tab_count`) travels with its contents -- the header advertises that
/// the listing spans every tab, which is what keeps the panel from reading as a
/// feature of the selected tab.
#[derive(Debug, Clone, Copy)]
pub struct AgentPanelState<'a> {
    pub open: bool,
    /// Panel card width in physical pixels.
    pub width_px: f32,
    pub agents: &'a [AgentSidebarItem],
    /// Total number of tabs the listing covers, across every window.
    pub tab_count: usize,
    /// Whether to draw the floating top-right `[AI] [⚙]` pill when the tab bar is hidden.
    pub show_floating_top_right: bool,
    /// Whether the pointer is hovering over the `[AI]` toggle button.
    pub ai_btn_hovered: bool,
    /// Whether the pointer is hovering over the `[⚙]` preferences button.
    pub prefs_btn_hovered: bool,
}

impl AgentPanelState<'_> {
    /// A closed panel, for render paths that have no agents UI at all.
    pub fn closed() -> Self {
        Self {
            open: false,
            width_px: 0.0,
            agents: &[],
            tab_count: 0,
            show_floating_top_right: false,
            ai_btn_hovered: false,
            prefs_btn_hovered: false,
        }
    }
}

/// Geometry and hit-test helper for the right-aligned `[AI]` and `[⚙]` buttons
/// shown on the right end of the tab bar (when `tab_bar_h > 0`) or as a floating
/// hover pill in the top-right corner of the terminal (when `tab_bar_h == 0`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TopRightButtons {
    /// Pill container rect `[x, y, w, h]`
    pub pill_rect: [f32; 4],
    /// `[x, y, w, h]` of the `[AI]` sidebar toggle button
    pub ai_btn: [f32; 4],
    /// `[x, y, w, h]` of the `[⚙]` preferences button
    pub prefs_btn: [f32; 4],
}

impl TopRightButtons {
    /// Width reserved on the right end of the tab bar so tab capsules and `[+][v]`
    /// never overlap the `[AI]` and `[⚙]` buttons.
    pub fn reserved_width(scale: f32) -> f32 {
        76.0 * scale
    }

    pub fn compute(main_w: f32, tab_bar_h: f32, scale: f32) -> Self {
        let btn_h = 24.0 * scale;
        let prefs_w = 26.0 * scale;
        let ai_w = 32.0 * scale;
        let gap = 4.0 * scale;
        let right_margin = 8.0 * scale;

        let prefs_x = (main_w - right_margin - prefs_w).max(4.0 * scale);
        let ai_x = (prefs_x - gap - ai_w).max(4.0 * scale);
        let btn_y = if tab_bar_h > 0.0 {
            (tab_bar_h - btn_h) / 2.0
        } else {
            6.0 * scale
        };

        let pad = 4.0 * scale;
        let pill_rect = [
            (ai_x - pad).max(0.0),
            (btn_y - pad * 0.75).max(0.0),
            ai_w + gap + prefs_w + pad * 2.0,
            btn_h + pad * 1.5,
        ];

        Self {
            pill_rect,
            ai_btn: [ai_x, btn_y, ai_w, btn_h],
            prefs_btn: [prefs_x, btn_y, prefs_w, btn_h],
        }
    }

    /// Returns `true` when `(mx, my)` is inside the top-right hover zone that reveals
    /// the floating `[AI] [⚙]` pill when the tab bar is hidden (`tab_bar_h == 0`).
    pub fn in_hover_zone(&self, mx: f32, my: f32, scale: f32) -> bool {
        let [px, py, pw, ph] = self.pill_rect;
        let margin_x = 28.0 * scale;
        let margin_y = 16.0 * scale;
        mx >= (px - margin_x).max(0.0)
            && mx <= px + pw + 8.0 * scale
            && my >= 0.0
            && my <= py + ph + margin_y
    }

    pub fn hit_ai(&self, mx: f32, my: f32, scale: f32) -> bool {
        let [x, y, w, h] = self.ai_btn;
        let pad = 2.0 * scale;
        mx >= x - pad && mx <= x + w + pad && my >= y - pad && my <= y + h + pad
    }

    pub fn hit_prefs(&self, mx: f32, my: f32, scale: f32) -> bool {
        let [x, y, w, h] = self.prefs_btn;
        let pad = 2.0 * scale;
        mx >= x - pad && mx <= x + w + pad && my >= y - pad && my <= y + h + pad
    }
}

/// Descriptor for an item in the top dropdown menu.
#[derive(Debug, Clone)]
pub struct TopDropdownMenuItem {
    pub label: String,
    pub shortcut: Option<String>,
    pub is_separator: bool,
}

/// Descriptor for the floating top dropdown / context menu.
#[derive(Debug, Clone)]
pub struct TopDropdownMenu {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub items: Vec<TopDropdownMenuItem>,
    pub hovered_idx: Option<usize>,
}

impl TopDropdownMenu {
    pub fn total_height(&self, scale: f32) -> f32 {
        let row_h = 24.0 * scale;
        let sep_h = 6.0 * scale;
        let mut total_h = 6.0 * scale;
        for item in &self.items {
            if item.is_separator {
                total_h += sep_h;
            } else {
                total_h += row_h;
            }
        }
        total_h
    }

    pub fn clamp_to_screen(&mut self, screen_w: f32, screen_h: f32, scale: f32) {
        let total_h = self.total_height(scale);
        let margin = 4.0 * scale;
        let max_x = (screen_w - self.width - margin).max(margin);
        let max_y = (screen_h - total_h - margin).max(margin);
        self.x = self.x.clamp(margin, max_x);
        self.y = self.y.clamp(margin, max_y);
    }
}

pub struct TermRenderer {
    pub window: Arc<Window>,
    pub surface: Surface<'static>,
    pub device: Device,
    pub queue: Queue,
    pub config: SurfaceConfiguration,
    pub atlas: GlyphAtlas,
    pub pipeline: TerminalPipeline,
    pub padding_x: f32,
    pub padding_y: f32,
    pub theme: RenderTheme,
    pub scale_factor: f32,
    pub font_size_pt: f32,
    pub line_height: f32,
    pub font_family: String,
    pub fallback_families: Vec<String>,
    pub opacity: f32,
    pub image_sampler: wgpu::Sampler,
    pub image_cache: HashMap<u32, (wgpu::Texture, wgpu::TextureView, wgpu::BindGroup)>,

    // Per-frame instance scratch buffers.
    //
    // These are rebuilt from scratch every frame, but they are kept here rather than
    // being locals so the allocation survives across frames: `clear()` retains
    // capacity, so after the first few frames the draw path performs no allocation
    // and no realloc-memcpy at all. A dense 200x50 pane pushes ~10k glyph instances
    // (48 B each), which as a local `Vec` meant reallocating 4096 -> 8192 -> 16384
    // and memcpying ~600 KB per frame purely to grow.
    //
    // They are `mem::take`n at the top of the draw call and put back at the end, so
    // that building them doesn't hold a borrow on `self`.
    bg_instances: Vec<BgInstance>,
    glyph_instances: Vec<GlyphInstance>,
    image_draw_calls: Vec<(u32, ImageInstance)>,
    image_instances: Vec<ImageInstance>,

    perf: FrameProfiler,
}

/// Opt-in per-frame timing, enabled by setting the `SKTERM_PERF` environment
/// variable to the number of frames to average over (or to `1` for the default).
///
/// This exists to answer one specific question with data instead of intuition: how
/// much of a frame is actually spent building the instance buffers on the CPU? That
/// is the only part that per-row dirty tracking could remove, so it sets the ceiling
/// on what incremental rendering could ever buy. Everything else in a frame -- the
/// surface acquire, the buffer uploads, the render pass and the present -- has to
/// happen whether or not any cell changed.
///
/// Disabled by default and costs nothing when off: the timing calls are behind
/// `enabled`, so a normal build never reads the clock.
#[derive(Default)]
struct FrameProfiler {
    enabled: bool,
    window: u32,
    frames: u32,
    acquire_ns: u128,
    build_ns: u128,
    upload_and_submit_ns: u128,
    present_ns: u128,
    total_ns: u128,
    glyph_instances: u64,
    bg_instances: u64,
}

impl FrameProfiler {
    fn from_env() -> Self {
        match std::env::var("SKTERM_PERF") {
            Ok(v) => {
                let window = v.trim().parse::<u32>().unwrap_or(1).max(1);
                // `1` is the natural thing to type for "just turn it on", but a
                // per-frame log line would be unreadable, so treat it as "use a
                // sensible default window".
                let window = if window == 1 { 120 } else { window };
                info!("SKTERM_PERF enabled: reporting every {window} frames");
                Self {
                    enabled: true,
                    window,
                    ..Default::default()
                }
            }
            Err(_) => Self::default(),
        }
    }

    #[inline]
    fn now(&self) -> Option<std::time::Instant> {
        self.enabled.then(std::time::Instant::now)
    }

    #[inline]
    fn since(&self, t: Option<std::time::Instant>) -> u128 {
        t.map(|t| t.elapsed().as_nanos()).unwrap_or(0)
    }

    #[allow(clippy::too_many_arguments)]
    fn record(
        &mut self,
        acquire_ns: u128,
        build_ns: u128,
        upload_and_submit_ns: u128,
        present_ns: u128,
        total_ns: u128,
        glyphs: usize,
        bgs: usize,
    ) {
        if !self.enabled {
            return;
        }
        self.frames += 1;
        self.acquire_ns += acquire_ns;
        self.build_ns += build_ns;
        self.upload_and_submit_ns += upload_and_submit_ns;
        self.present_ns += present_ns;
        self.total_ns += total_ns;
        self.glyph_instances += glyphs as u64;
        self.bg_instances += bgs as u64;

        if self.frames < self.window {
            return;
        }

        let n = self.frames as f64;
        let us = |ns: u128| ns as f64 / n / 1000.0;
        let total_us = us(self.total_ns);
        let pct = |v: f64| {
            if total_us > 0.0 {
                v / total_us * 100.0
            } else {
                0.0
            }
        };
        let build_us = us(self.build_ns);
        info!(
            "perf: {:.0} fps | frame {:.0}us = acquire {:.0}us ({:.0}%) + build {:.0}us ({:.0}%) \
             + upload/submit {:.0}us ({:.0}%) + present {:.0}us ({:.0}%) | {:.0} glyphs, {:.0} bg",
            1_000_000.0 / total_us.max(0.001),
            total_us,
            us(self.acquire_ns),
            pct(us(self.acquire_ns)),
            build_us,
            pct(build_us),
            us(self.upload_and_submit_ns),
            pct(us(self.upload_and_submit_ns)),
            us(self.present_ns),
            pct(us(self.present_ns)),
            self.glyph_instances as f64 / n,
            self.bg_instances as f64 / n,
        );

        self.frames = 0;
        self.acquire_ns = 0;
        self.build_ns = 0;
        self.upload_and_submit_ns = 0;
        self.present_ns = 0;
        self.total_ns = 0;
        self.glyph_instances = 0;
        self.bg_instances = 0;
    }
}

impl TermRenderer {
    pub async fn new(window: Arc<Window>, font_size_pt: f32) -> anyhow::Result<Self> {
        Self::new_with_font(window, font_size_pt, "Menlo", &[]).await
    }

    pub async fn new_with_font(
        window: Arc<Window>,
        font_size_pt: f32,
        font_family: &str,
        fallback_families: &[String],
    ) -> anyhow::Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);
        let scale_factor = (window.scale_factor() as f32).max(1.0);

        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            ..InstanceDescriptor::new_without_display_handle()
        });

        let surface = instance.create_surface(window.clone())?;

        let adapter = instance
            .request_adapter(&RequestAdapterOptions {
                power_preference: PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await?;

        info!("Selected GPU Adapter: {:?}", adapter.get_info());

        let (device, queue) = adapter
            .request_device(&DeviceDescriptor {
                label: Some("skterm_render_device"),
                required_features: Features::empty(),
                required_limits: Limits::default(),
                memory_hints: Default::default(),
                ..Default::default()
            })
            .await?;

        let surface_caps = surface.get_capabilities(&adapter);
        // Terminal colors (TrueColor RGB, 256-color palette, theme hex values, and
        // Sixel/Kitty Rgba8Unorm textures) are already in sRGB [0.0..1.0]. Selecting an
        // `*Srgb` surface format would apply hardware linear-to-sRGB gamma encoding a
        // second time and wash out midtones.
        let surface_format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        // Prioritize non-blocking, low-latency presentation modes for maximum typing smoothness
        let present_mode = if surface_caps.present_modes.contains(&PresentMode::Mailbox) {
            PresentMode::Mailbox
        } else if surface_caps
            .present_modes
            .contains(&PresentMode::AutoNoVsync)
        {
            PresentMode::AutoNoVsync
        } else if surface_caps.present_modes.contains(&PresentMode::Immediate) {
            PresentMode::Immediate
        } else {
            PresentMode::AutoVsync
        };

        info!(
            "Surface present mode selected: {:?} (supported: {:?})",
            present_mode, surface_caps.present_modes
        );

        let config = SurfaceConfiguration {
            usage: TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            color_space: wgpu::SurfaceColorSpace::Auto,
            width,
            height,
            present_mode,
            alpha_mode: CompositeAlphaMode::Auto,
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };

        surface.configure(&device, &config);

        let physical_font_size = font_size_pt * scale_factor;
        let padding_x = 8.0 * scale_factor;
        let padding_y = 8.0 * scale_factor;

        let atlas = GlyphAtlas::new_with_family(
            &device,
            &queue,
            physical_font_size,
            font_family,
            fallback_families,
            None,
        )?;
        let pipeline = TerminalPipeline::new(&device, &config, &atlas)?;

        let image_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skterm_image_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });

        Ok(Self {
            window,
            surface,
            device,
            queue,
            config,
            atlas,
            pipeline,
            padding_x,
            padding_y,
            theme: RenderTheme::default(),
            scale_factor,
            font_size_pt,
            line_height: 1.2,
            font_family: font_family.to_string(),
            fallback_families: fallback_families.to_vec(),
            opacity: 1.0,
            image_sampler,
            image_cache: HashMap::new(),
            // Pre-sized for a dense ~200x50 pane so even the first frames avoid
            // the realloc chain. These only ever grow to the high-water mark.
            bg_instances: Vec::with_capacity(4096),
            glyph_instances: Vec::with_capacity(12288),
            image_draw_calls: Vec::new(),
            image_instances: Vec::new(),
            perf: FrameProfiler::from_env(),
        })
    }

    pub fn update_padding(&mut self, pad_x: f32, pad_y: f32) {
        self.padding_x = pad_x * self.scale_factor;
        self.padding_y = pad_y * self.scale_factor;
    }

    pub fn update_opacity(&mut self, opacity: f32) {
        self.opacity = opacity.clamp(0.05, 1.0);
    }

    pub fn update_line_height(&mut self, line_height: f32) {
        self.line_height = line_height.clamp(0.8, 2.5);
        self.atlas.set_line_height(self.line_height);
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
            self.pipeline
                .update_screen_size(&self.queue, width as f32, height as f32);
        }
    }

    pub fn update_scale_factor(&mut self, new_scale: f32) -> anyhow::Result<()> {
        if (self.scale_factor - new_scale).abs() > 0.01 {
            self.scale_factor = new_scale;
            let physical_font_size = self.font_size_pt * new_scale;
            self.padding_x = 8.0 * new_scale;
            self.padding_y = 8.0 * new_scale;
            self.atlas = GlyphAtlas::new_with_family(
                &self.device,
                &self.queue,
                physical_font_size,
                &self.font_family,
                &self.fallback_families,
                None,
            )?;
            self.atlas.set_line_height(self.line_height);
            self.pipeline = TerminalPipeline::new(&self.device, &self.config, &self.atlas)?;
        }
        Ok(())
    }

    pub fn update_font(
        &mut self,
        font_family: &str,
        font_size_pt: f32,
        fallback_families: &[String],
    ) -> anyhow::Result<()> {
        self.font_family = font_family.to_string();
        self.font_size_pt = font_size_pt.clamp(8.0, 72.0);
        self.fallback_families = fallback_families.to_vec();

        let physical_font_size = self.font_size_pt * self.scale_factor;
        self.atlas = GlyphAtlas::new_with_family(
            &self.device,
            &self.queue,
            physical_font_size,
            &self.font_family,
            &self.fallback_families,
            None,
        )?;
        self.atlas.set_line_height(self.line_height);
        self.pipeline = TerminalPipeline::new(&self.device, &self.config, &self.atlas)?;
        Ok(())
    }

    pub fn update_font_size(&mut self, new_pt: f32) -> anyhow::Result<()> {
        let fam = self.font_family.clone();
        let fallbacks = self.fallback_families.clone();
        self.update_font(&fam, new_pt, &fallbacks)
    }

    pub fn set_theme(&mut self, theme: RenderTheme) {
        self.theme = theme;
    }

    pub fn cell_width(&self) -> f32 {
        self.atlas.cell_width
    }

    pub fn cell_height(&self) -> f32 {
        self.atlas.cell_height
    }

    /// Measure the typographic advance width in physical pixels of `text` for UI layout.
    pub fn measure_text_width(&mut self, text: &str, is_bold: bool) -> f32 {
        self.atlas.measure_text_width(&self.queue, text, is_bold)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_line(
        &mut self,
        glyph_instances: &mut Vec<GlyphInstance>,
        text: &str,
        x: f32,
        y: f32,
        max_w: f32,
        color: [f32; 4],
        is_bold: bool,
    ) {
        let cell_w = self.atlas.cell_width;
        let baseline = self.atlas.baseline_offset;
        let y_snapped = y.round();
        let mut cur_x = x;
        for c in text.chars() {
            let glyph_info = self
                .atlas
                .get_or_insert_glyph(&self.queue, c, is_bold, false);
            let adv = glyph_info
                .map(|g| g.advance)
                .unwrap_or_else(|| char_display_cells(c) as f32 * cell_w)
                .max(0.0);
            if cur_x - x + adv > max_w + 1e-3 {
                break;
            }
            if let Some(glyph_info) = glyph_info {
                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                    glyph_instances.push(GlyphInstance {
                        rect: [
                            cur_x.round() + glyph_info.offset[0],
                            y_snapped + baseline - glyph_info.offset[1] - glyph_info.size[1],
                            glyph_info.size[0],
                            glyph_info.size[1],
                        ],
                        uv: [
                            glyph_info.uv_min[0],
                            glyph_info.uv_min[1],
                            glyph_info.uv_max[0],
                            glyph_info.uv_max[1],
                        ],
                        color,
                    });
                }
            }
            cur_x += adv;
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_text_line_ellipsized(
        &mut self,
        glyph_instances: &mut Vec<GlyphInstance>,
        text: &str,
        x: f32,
        y: f32,
        max_w: f32,
        color: [f32; 4],
        is_bold: bool,
    ) {
        let cell_w = self.atlas.cell_width;
        let baseline = self.atlas.baseline_offset;
        let y_snapped = y.round();
        let atlas = &mut self.atlas;
        let queue = &self.queue;
        for_each_ellipsized_glyph(
            text,
            x,
            max_w,
            |c| {
                let info = atlas.get_or_insert_glyph(queue, c, is_bold, false);
                let adv = info
                    .map(|g| g.advance)
                    .unwrap_or_else(|| char_display_cells(c) as f32 * cell_w);
                (adv, info)
            },
            |_c, cur_x, glyph_info| {
                if let Some(glyph_info) = glyph_info {
                    if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                        glyph_instances.push(GlyphInstance {
                            rect: [
                                cur_x.round() + glyph_info.offset[0],
                                y_snapped + baseline - glyph_info.offset[1] - glyph_info.size[1],
                                glyph_info.size[0],
                                glyph_info.size[1],
                            ],
                            uv: [
                                glyph_info.uv_min[0],
                                glyph_info.uv_min[1],
                                glyph_info.uv_max[0],
                                glyph_info.uv_max[1],
                            ],
                            color,
                        });
                    }
                }
            },
        );
    }

    /// Draws the right-aligned `[AI]` and `[⚙]` action buttons (either embedded in
    /// the tab bar or inside a floating top-right pill when the tab bar is hidden).
    fn draw_top_right_buttons(
        &mut self,
        bg: &mut Vec<BgInstance>,
        glyphs: &mut Vec<GlyphInstance>,
        btns: &TopRightButtons,
        state: &AgentPanelState<'_>,
        floating_pill: bool,
        scale: f32,
    ) {
        if floating_pill {
            let [px, py, pw, ph] = btns.pill_rect;
            // Subtle shadow under floating pill
            bg.push(BgInstance {
                rect: [
                    px - 2.0 * scale,
                    py - 1.0 * scale,
                    pw + 4.0 * scale,
                    ph + 3.0 * scale,
                ],
                color: [0.0, 0.0, 0.0, 0.40],
            });
            // Pill container background + 1px border
            bg.push(BgInstance {
                rect: [px - 1.0, py - 1.0, pw + 2.0, ph + 2.0],
                color: [0.26, 0.30, 0.40, 0.95],
            });
            bg.push(BgInstance {
                rect: btns.pill_rect,
                color: [0.08, 0.09, 0.13, 0.96],
            });
        }

        // 1. [AI] Sidebar Toggle Button
        let [ax, ay_btn, aw, ah] = btns.ai_btn;
        let ai_bg = if state.open {
            [0.12, 0.34, 0.68, 0.95]
        } else if state.ai_btn_hovered {
            [0.20, 0.24, 0.34, 0.95]
        } else {
            [0.12, 0.14, 0.18, 0.78]
        };
        let ai_border = if state.open {
            [0.24, 0.60, 1.0, 0.95]
        } else if state.ai_btn_hovered {
            [0.32, 0.38, 0.52, 0.85]
        } else {
            [0.22, 0.25, 0.33, 0.6]
        };
        bg.push(BgInstance {
            rect: btns.ai_btn,
            color: ai_bg,
        });
        bg.push(BgInstance {
            rect: [ax, ay_btn, aw, 1.0],
            color: ai_border,
        });

        let ai_fg = if state.open || state.ai_btn_hovered {
            [0.98, 0.99, 1.0, 1.0]
        } else {
            [0.80, 0.84, 0.94, 0.92]
        };

        let glyph_a = self
            .atlas
            .get_or_insert_glyph(&self.queue, 'A', true, false);
        let glyph_i = self
            .atlas
            .get_or_insert_glyph(&self.queue, 'I', true, false);
        if let (Some(ga), Some(gi)) = (glyph_a, glyph_i) {
            if ga.size[0] > 0.0 && gi.size[0] > 0.0 {
                let gap = 1.5 * scale;
                let total_w = ga.size[0] + gap + gi.size[0];
                let start_x = ax + (aw - total_w) / 2.0;
                let ay = ay_btn + (ah - ga.size[1]) / 2.0;
                let iy = ay_btn + (ah - gi.size[1]) / 2.0;
                glyphs.push(GlyphInstance {
                    rect: [start_x, ay, ga.size[0], ga.size[1]],
                    uv: [ga.uv_min[0], ga.uv_min[1], ga.uv_max[0], ga.uv_max[1]],
                    color: ai_fg,
                });
                glyphs.push(GlyphInstance {
                    rect: [start_x + ga.size[0] + gap, iy, gi.size[0], gi.size[1]],
                    uv: [gi.uv_min[0], gi.uv_min[1], gi.uv_max[0], gi.uv_max[1]],
                    color: ai_fg,
                });
            }
        }
        // Small active-agent indicator dot in top-right corner of [AI] button
        if !state.agents.is_empty() {
            let dot = (4.5 * scale).round().max(3.0);
            let dot_color = if state.agents.iter().any(|a| a.status.is_blocked()) {
                AgentStatus::NeedsInput.dot_color()
            } else if state.agents.iter().any(|a| a.status == AgentStatus::Error) {
                AgentStatus::Error.dot_color()
            } else if state
                .agents
                .iter()
                .any(|a| a.status == AgentStatus::Working || a.is_active_output)
            {
                AgentStatus::Working.dot_color()
            } else if state.agents.iter().any(|a| a.status == AgentStatus::Done) {
                AgentStatus::Done.dot_color()
            } else {
                AgentStatus::Idle.dot_color()
            };
            bg.push(BgInstance {
                rect: [ax + aw - dot - 2.0 * scale, ay_btn + 2.0 * scale, dot, dot],
                color: dot_color,
            });
        }

        // 2. [⚙] Preferences Button
        let [prx, pry, prw, prh] = btns.prefs_btn;
        let prefs_bg = if state.prefs_btn_hovered {
            [0.20, 0.24, 0.34, 0.95]
        } else {
            [0.12, 0.14, 0.18, 0.78]
        };
        let prefs_border = if state.prefs_btn_hovered {
            [0.32, 0.38, 0.52, 0.85]
        } else {
            [0.22, 0.25, 0.33, 0.6]
        };
        bg.push(BgInstance {
            rect: btns.prefs_btn,
            color: prefs_bg,
        });
        bg.push(BgInstance {
            rect: [prx, pry, prw, 1.0],
            color: prefs_border,
        });

        let prefs_fg = if state.prefs_btn_hovered {
            [0.98, 0.99, 1.0, 1.0]
        } else {
            [0.80, 0.84, 0.94, 0.92]
        };

        let mut drew_gear = false;
        if let Some(g) = self
            .atlas
            .get_or_insert_glyph(&self.queue, '⚙', false, false)
        {
            if g.size[0] > 0.0 && g.size[1] > 0.0 {
                let max_icon = prw - 6.0 * scale;
                let scale_down = if g.size[0] > max_icon || g.size[1] > max_icon {
                    (max_icon / g.size[0].max(g.size[1])).clamp(0.4, 1.0)
                } else {
                    1.0
                };
                let gw = g.size[0] * scale_down;
                let gh = g.size[1] * scale_down;
                let gx = prx + (prw - gw) / 2.0;
                let gy = pry + (prh - gh) / 2.0;
                glyphs.push(GlyphInstance {
                    rect: [gx, gy, gw, gh],
                    uv: [g.uv_min[0], g.uv_min[1], g.uv_max[0], g.uv_max[1]],
                    color: prefs_fg,
                });
                drew_gear = true;
            }
        }
        if !drew_gear {
            // Procedural 3-slider settings icon fallback if no system font has U+2699
            let bar_w = 12.0 * scale;
            let bar_h = (1.5 * scale).round().max(1.0);
            let bx = prx + (prw - bar_w) / 2.0;
            for (i, knob_frac) in [0.25_f32, 0.65, 0.35].iter().enumerate() {
                let by = pry + 6.5 * scale + (i as f32 * 5.0 * scale);
                bg.push(BgInstance {
                    rect: [bx, by, bar_w, bar_h],
                    color: prefs_fg,
                });
                let knob_s = (3.0 * scale).round().max(2.0);
                bg.push(BgInstance {
                    rect: [
                        bx + bar_w * knob_frac - knob_s * 0.5,
                        by - (knob_s - bar_h) * 0.5,
                        knob_s,
                        knob_s,
                    ],
                    color: prefs_fg,
                });
            }
        }
    }

    /// Draws the global AI agents panel: a floating card docked to the right
    /// edge, deliberately detached from the tab strip so it reads as an
    /// application-wide listing rather than something owned by the active tab.
    fn draw_agent_panel(
        &mut self,
        bg: &mut Vec<BgInstance>,
        glyphs: &mut Vec<GlyphInstance>,
        layout: &AgentPanelLayout,
        state: &AgentPanelState<'_>,
    ) {
        let scale = layout.scale;
        let cell_h = self.atlas.cell_height;
        let panel = layout.panel;
        let accent = [0.039, 0.518, 1.00, 1.0];

        // Drop shadow: concentric translucent rings that lift the card off the
        // terminal underneath it. Without this the card just looks like another
        // region of the tab's content.
        for i in 1..=3 {
            let spread = i as f32 * 2.0 * scale;
            bg.push(BgInstance {
                rect: [
                    panel.x - spread,
                    panel.y - spread,
                    panel.w + spread * 2.0,
                    panel.h + spread * 2.0,
                ],
                color: [0.0, 0.0, 0.0, 0.10],
            });
        }

        // Card body + 1px outline on all four sides.
        bg.push(BgInstance {
            rect: panel.as_array(),
            color: [0.055, 0.065, 0.085, 0.985],
        });
        let outline = [0.22, 0.25, 0.33, 0.95];
        bg.push(BgInstance {
            rect: [panel.x, panel.y, panel.w, 1.0],
            color: outline,
        });
        bg.push(BgInstance {
            rect: [panel.x, panel.bottom() - 1.0, panel.w, 1.0],
            color: outline,
        });
        bg.push(BgInstance {
            rect: [panel.x, panel.y, 1.0, panel.h],
            color: outline,
        });
        bg.push(BgInstance {
            rect: [panel.right() - 1.0, panel.y, 1.0, panel.h],
            color: outline,
        });

        // Accent strip across the top of the card -- a window-level affordance,
        // matching nothing in the per-tab chrome.
        bg.push(BgInstance {
            rect: [panel.x, panel.y, panel.w, 2.0 * scale],
            color: accent,
        });

        // Header row 1: title, plus the close button on the right.
        let title = format!("ALL AGENTS ({})", state.agents.len());
        self.draw_text_line(
            glyphs,
            &title,
            panel.x + 12.0 * scale,
            panel.y + 7.0 * scale,
            (layout.close_btn.x - panel.x - 16.0 * scale).max(20.0),
            [0.92, 0.95, 1.0, 1.0],
            true,
        );

        let close = layout.close_btn;
        bg.push(BgInstance {
            rect: close.as_array(),
            color: [0.14, 0.16, 0.21, 0.9],
        });
        let close_label_w = self.measure_text_width("×", false);
        self.draw_text_line(
            glyphs,
            "×",
            close.x + (close.w - close_label_w) / 2.0,
            close.y + (close.h - cell_h) / 2.0,
            close.w,
            [0.72, 0.76, 0.85, 0.95],
            false,
        );

        // Header row 2: the scope line. This is the sentence that tells the user
        // the panel is not about the current tab.
        let scope = if state.tab_count > 1 {
            format!("Global · across {} tabs", state.tab_count)
        } else {
            "Global · all tabs".to_string()
        };
        self.draw_text_line_ellipsized(
            glyphs,
            &scope,
            panel.x + 12.0 * scale,
            layout.new_btn.y + (layout.new_btn.h - cell_h) / 2.0,
            (layout.new_btn.x - panel.x - 16.0 * scale).max(20.0),
            [0.52, 0.58, 0.70, 0.95],
            false,
        );

        let new_btn = layout.new_btn;
        bg.push(BgInstance {
            rect: new_btn.as_array(),
            color: [0.10, 0.36, 0.72, 0.9],
        });
        let new_label = "+ New";
        let new_label_w = self.measure_text_width(new_label, true);
        self.draw_text_line(
            glyphs,
            new_label,
            new_btn.x + (new_btn.w - new_label_w) / 2.0,
            new_btn.y + (new_btn.h - cell_h) / 2.0,
            new_btn.w,
            [0.98, 0.98, 1.0, 1.0],
            true,
        );

        // Divider under the header.
        bg.push(BgInstance {
            rect: [
                panel.x + 1.0,
                layout.header.bottom() - 1.0,
                panel.w - 2.0,
                1.0,
            ],
            color: [0.18, 0.21, 0.28, 0.9],
        });

        if state.agents.is_empty() {
            self.draw_text_line(
                glyphs,
                "No active CLI agents",
                panel.x + 12.0 * scale,
                layout.list_top + 8.0 * scale,
                layout.card_w,
                [0.50, 0.54, 0.62, 0.9],
                false,
            );
            self.draw_text_line(
                glyphs,
                "Click '+ New' to spawn",
                panel.x + 12.0 * scale,
                layout.list_top + 8.0 * scale + cell_h + 4.0 * scale,
                layout.card_w,
                [0.42, 0.46, 0.54, 0.85],
                false,
            );
            return;
        }

        let max_cards = layout.visible_cards();
        for (idx, item) in state.agents.iter().enumerate() {
            if idx >= max_cards {
                break;
            }
            let card = layout.card_rect(idx);
            let cy = card.y;

            bg.push(BgInstance {
                rect: card.as_array(),
                color: if item.is_focused {
                    [0.13, 0.21, 0.34, 0.95]
                } else {
                    [0.09, 0.11, 0.15, 0.9]
                },
            });
            let border_col = if item.is_focused {
                accent
            } else {
                [0.20, 0.23, 0.30, 0.6]
            };
            bg.push(BgInstance {
                rect: [card.x, cy, card.w, 1.0],
                color: border_col,
            });
            bg.push(BgInstance {
                rect: [card.x, card.bottom() - 1.0, card.w, 1.0],
                color: border_col,
            });
            if item.is_focused {
                bg.push(BgInstance {
                    rect: [card.x, cy, 3.0 * scale, card.h],
                    color: accent,
                });
            }

            // Status dot (Green = Working/Running, Amber = Blocked/Needs Action, Red = Error, Cyan = Done, Yellow = Idle)
            let dot_size = 7.0 * scale;
            let dot_x = card.x + 10.0 * scale;
            let dot_y = cy + 9.0 * scale + (cell_h - dot_size) / 2.0;
            bg.push(BgInstance {
                rect: [dot_x, dot_y, dot_size, dot_size],
                color: item.dot_color(),
            });

            // Location pill ("Tab 2 · #3"): says which tab owns this agent, so a
            // listing spanning several tabs is self-explanatory.
            let pill_text_w = self.measure_text_width(&item.location_label, false);
            let pill_w = pill_text_w + 10.0 * scale;
            let pill_h = cell_h + 4.0 * scale;
            let pill_x = card.right() - pill_w - 8.0 * scale;
            let pill_y = cy + 6.0 * scale;
            bg.push(BgInstance {
                rect: [pill_x, pill_y, pill_w, pill_h],
                color: if item.is_focused {
                    [0.16, 0.30, 0.50, 0.95]
                } else {
                    [0.14, 0.16, 0.22, 0.95]
                },
            });
            self.draw_text_line(
                glyphs,
                &item.location_label,
                pill_x + 5.0 * scale,
                pill_y + (pill_h - cell_h) / 2.0,
                pill_text_w + 1.0,
                [0.68, 0.78, 0.94, 1.0],
                false,
            );

            let has_custom_task =
                !item.title_summary.is_empty() && item.title_summary != "Double-click to name...";
            let status_str = item.status_display();
            let (top_title, bottom_subtitle) = if has_custom_task {
                (
                    item.title_summary.as_str(),
                    format!("{} • {}", item.agent_name, status_str),
                )
            } else {
                (
                    item.agent_name.as_str(),
                    format!("{} • Double-click to name...", status_str),
                )
            };

            // Line 1: task name (or agent name when unnamed)
            let text_x = dot_x + dot_size + 6.0 * scale;
            self.draw_text_line_ellipsized(
                glyphs,
                top_title,
                text_x,
                cy + 8.0 * scale,
                (pill_x - text_x - 6.0 * scale).max(20.0),
                [0.98, 0.99, 1.0, 1.0],
                true,
            );
            // Line 2: project / directory
            self.draw_text_line_ellipsized(
                glyphs,
                &format!("▸ {}", item.project_name),
                card.x + 10.0 * scale,
                cy + 27.0 * scale,
                card.w - 16.0 * scale,
                [0.45, 0.82, 0.96, 0.95],
                false,
            );
            // Line 3: agent type • status
            let subtitle_color = if item.status.is_blocked() {
                [0.98, 0.78, 0.42, 0.95]
            } else if item.status == AgentStatus::Error {
                [0.98, 0.52, 0.54, 0.95]
            } else {
                [0.58, 0.63, 0.72, 0.88]
            };
            self.draw_text_line_ellipsized(
                glyphs,
                &bottom_subtitle,
                card.x + 10.0 * scale,
                cy + 46.0 * scale,
                card.w - 16.0 * scale,
                subtitle_color,
                false,
            );
        }

        // Tell the user when the list is clipped rather than silently hiding rows.
        if state.agents.len() > max_cards {
            self.draw_text_line(
                glyphs,
                &format!("+{} more", state.agents.len() - max_cards),
                layout.card_x + 4.0 * scale,
                (panel.bottom() - cell_h - 6.0 * scale).max(layout.list_top),
                layout.card_w,
                [0.55, 0.60, 0.70, 0.9],
                false,
            );
        }
    }

    /// Render tabs, split panes, graphic images (Sixel/Kitty), separators, and text grids
    pub fn render_workspace(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
    ) -> anyhow::Result<()> {
        self.render_workspace_with_agent_panel(
            tabs,
            panes,
            tab_bar_h,
            AgentPanelState::closed(),
            None,
            None::<fn(&Device, &Queue, &wgpu::TextureView, &SurfaceConfiguration)>,
        )
        .map(|_| ())
    }

    /// Render workspace with optional custom overlay (e.g. egui UI).
    pub fn render_workspace_with_overlay<F>(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
        overlay_fn: Option<F>,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(&Device, &Queue, &wgpu::TextureView, &SurfaceConfiguration),
    {
        self.render_workspace_with_agent_panel(
            tabs,
            panes,
            tab_bar_h,
            AgentPanelState::closed(),
            None,
            overlay_fn,
        )
        .map(|_| ())
    }

    /// Render workspace with the global AI agents panel, floating top dropdown menu, and an optional overlay.
    pub fn render_workspace_with_agent_panel<F>(
        &mut self,
        tabs: &[TabItem],
        panes: &[PaneRenderItem],
        tab_bar_h: f32,
        panel_state: AgentPanelState<'_>,
        top_menu: Option<&TopDropdownMenu>,
        overlay_fn: Option<F>,
    ) -> anyhow::Result<RenderSurfaceOutcome>
    where
        F: FnOnce(&Device, &Queue, &wgpu::TextureView, &SurfaceConfiguration),
    {
        let frame_start = self.perf.now();

        let (output, suboptimal) = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(texture) => (texture, false),
            wgpu::CurrentSurfaceTexture::Suboptimal(texture) => (texture, true),
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.resize(self.config.width, self.config.height);
                return Ok(RenderSurfaceOutcome::Reconfigured);
            }
            wgpu::CurrentSurfaceTexture::Timeout => {
                return Ok(RenderSurfaceOutcome::Timeout);
            }
            wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(RenderSurfaceOutcome::Occluded);
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                return Err(anyhow::anyhow!("Surface validation error"));
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // `get_current_texture` is where a GPU-bound frame actually blocks: it waits
        // for the compositor to hand back a free swapchain image.
        let acquire_ns = self.perf.since(frame_start);

        // Borrow the persistent scratch buffers for the duration of this frame.
        // `take` leaves empty Vecs behind (no allocation) and we hand the buffers
        // back at the end, so the capacity built up by previous frames is reused.
        let mut bg_instances = std::mem::take(&mut self.bg_instances);
        let mut glyph_instances = std::mem::take(&mut self.glyph_instances);
        let mut image_draw_calls = std::mem::take(&mut self.image_draw_calls);
        bg_instances.clear();
        glyph_instances.clear();
        image_draw_calls.clear();

        // Everything from here to the "Upload instances to GPU buffers" marker is the
        // CPU-side instance build -- the only part dirty tracking could skip.
        let build_start = self.perf.now();

        let cell_w = self.atlas.cell_width;
        let cell_h = self.atlas.cell_height;
        let baseline = self.atlas.baseline_offset;
        let scale = self.scale_factor;
        let screen_w = self.config.width as f32;
        let screen_h = self.config.height as f32;
        let sidebar_open = panel_state.open;

        // 1. Full Screen Background
        let mut full_bg = self.theme.default_bg;
        full_bg[3] *= self.opacity;

        bg_instances.push(BgInstance {
            rect: [0.0, 0.0, screen_w, screen_h],
            color: full_bg,
        });

        // 2. Tab Bar (splits only the main window area, excluding the right sidebar)
        if tab_bar_h > 0.0 && !tabs.is_empty() {
            let reserved_right = if sidebar_open && panel_state.width_px > 0.0 {
                AgentPanelLayout::reserved_width(panel_state.width_px, scale)
            } else {
                0.0
            };
            let main_w = (screen_w - reserved_right).max(0.0);

            // 2.1 Tab Bar Header Background (Deep translucent frosted bar)
            bg_instances.push(BgInstance {
                rect: [0.0, 0.0, main_w, tab_bar_h],
                color: [0.06, 0.07, 0.09, 1.0],
            });

            // Bottom 1px subtle divider
            bg_instances.push(BgInstance {
                rect: [0.0, tab_bar_h - 1.0, main_w, 1.0],
                color: [0.18, 0.20, 0.26, 0.8],
            });

            // 2.1.5 Tab capsules start at a clean left margin inside the main area.
            let base_x = 8.0 * scale;

            // 2.2 Calculate Tab Capsule Geometry
            let start_x = base_x;
            let plus_btn_w = 24.0 * scale;
            let chevron_w = 20.0 * scale;
            let right_btns = TopRightButtons::compute(main_w, tab_bar_h, scale);
            let right_btns_w = TopRightButtons::reserved_width(scale);
            let tab_gap = 4.0 * scale;
            let avail_w =
                (main_w - start_x - plus_btn_w - chevron_w - right_btns_w - 16.0 * scale).max(80.0);
            let total_gaps = (tabs.len().saturating_sub(1) as f32) * tab_gap;
            let tab_w =
                ((avail_w - total_gaps) / tabs.len() as f32).clamp(75.0 * scale, 190.0 * scale);
            let tab_h = (tab_bar_h - 8.0 * scale).max(18.0);
            let tab_y = (tab_bar_h - tab_h) / 2.0;

            for (idx, tab) in tabs.iter().enumerate() {
                let tab_x = start_x + (idx as f32 * (tab_w + tab_gap));

                if tab.is_active {
                    // Active Tab: Floating Card Capsule with subtle border and Active Blue Indicator
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, tab_w, tab_h],
                        color: [0.15, 0.17, 0.23, 1.0],
                    });
                    // Top / Left / Right subtle border line
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, tab_w, 1.0],
                        color: [0.28, 0.32, 0.42, 0.8],
                    });
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, 1.0, tab_h],
                        color: [0.28, 0.32, 0.42, 0.5],
                    });
                    bg_instances.push(BgInstance {
                        rect: [tab_x + tab_w - 1.0, tab_y, 1.0, tab_h],
                        color: [0.28, 0.32, 0.42, 0.5],
                    });
                    // Active Blue Indicator line at bottom of capsule (#0A84FF)
                    bg_instances.push(BgInstance {
                        rect: [
                            tab_x + 3.0 * scale,
                            tab_y + tab_h - 2.0,
                            tab_w - 6.0 * scale,
                            2.0,
                        ],
                        color: [0.039, 0.518, 1.00, 1.0],
                    });
                } else {
                    // Inactive Tab: Subtle dark capsule
                    bg_instances.push(BgInstance {
                        rect: [tab_x, tab_y, tab_w, tab_h],
                        color: [0.09, 0.10, 0.13, 0.6],
                    });
                }

                // Close "x" button on right side of tab
                let close_size = 12.0 * scale;
                let close_x = tab_x + tab_w - close_size - 6.0 * scale;
                let close_y = tab_y + (tab_h - close_size) / 2.0;

                if let Some(glyph_info) =
                    self.atlas
                        .get_or_insert_glyph(&self.queue, '×', false, false)
                {
                    if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                        let draw_x = close_x + (close_size - glyph_info.size[0]) / 2.0;
                        let draw_y = close_y + (close_size - glyph_info.size[1]) / 2.0;
                        let close_color = if tab.is_active {
                            [0.72, 0.76, 0.85, 0.9]
                        } else {
                            [0.45, 0.48, 0.55, 0.6]
                        };
                        glyph_instances.push(GlyphInstance {
                            rect: [draw_x, draw_y, glyph_info.size[0], glyph_info.size[1]],
                            uv: [
                                glyph_info.uv_min[0],
                                glyph_info.uv_min[1],
                                glyph_info.uv_max[0],
                                glyph_info.uv_max[1],
                            ],
                            color: close_color,
                        });
                    }
                }

                // Tab Title Text (Centered vertically, CJK wide-character aware + ellipsis)
                let text_start_x = tab_x + 10.0 * scale;
                let max_text_w = close_x - text_start_x - 4.0 * scale;
                let text_y = (tab_bar_h - cell_h) / 2.0;
                let text_color = if tab.is_active {
                    [0.98, 0.98, 1.00, 1.0]
                } else {
                    [0.62, 0.65, 0.73, 1.0]
                };

                self.draw_text_line_ellipsized(
                    &mut glyph_instances,
                    &tab.title,
                    text_start_x,
                    text_y,
                    max_text_w,
                    text_color,
                    false,
                );
            }

            // 2.3 New Tab "+" Button
            let last_tab_right = start_x + (tabs.len() as f32 * (tab_w + tab_gap));
            let plus_btn_x = last_tab_right + 2.0 * scale;
            let plus_btn_y = tab_y + (tab_h - plus_btn_w) / 2.0;

            bg_instances.push(BgInstance {
                rect: [plus_btn_x, plus_btn_y, plus_btn_w, plus_btn_w],
                color: [0.12, 0.14, 0.18, 0.7],
            });
            bg_instances.push(BgInstance {
                rect: [plus_btn_x, plus_btn_y, plus_btn_w, 1.0],
                color: [0.22, 0.25, 0.33, 0.6],
            });

            if let Some(glyph_info) = self
                .atlas
                .get_or_insert_glyph(&self.queue, '+', false, false)
            {
                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                    let draw_x = plus_btn_x + (plus_btn_w - glyph_info.size[0]) / 2.0;
                    let draw_y = plus_btn_y + (plus_btn_w - glyph_info.size[1]) / 2.0;
                    glyph_instances.push(GlyphInstance {
                        rect: [draw_x, draw_y, glyph_info.size[0], glyph_info.size[1]],
                        uv: [
                            glyph_info.uv_min[0],
                            glyph_info.uv_min[1],
                            glyph_info.uv_max[0],
                            glyph_info.uv_max[1],
                        ],
                        color: [0.80, 0.84, 0.94, 0.9],
                    });
                }
            }

            // Dropdown Menu chevron "v" Button
            let chevron_x = plus_btn_x + plus_btn_w + 2.0 * scale;
            bg_instances.push(BgInstance {
                rect: [chevron_x, plus_btn_y, chevron_w, plus_btn_w],
                color: [0.12, 0.14, 0.18, 0.7],
            });
            bg_instances.push(BgInstance {
                rect: [chevron_x, plus_btn_y, chevron_w, 1.0],
                color: [0.22, 0.25, 0.33, 0.6],
            });
            let chevron_label_w = self.measure_text_width("v", false);
            self.draw_text_line(
                &mut glyph_instances,
                "v",
                chevron_x + (chevron_w - chevron_label_w) / 2.0,
                plus_btn_y + (plus_btn_w - cell_h) / 2.0,
                chevron_w,
                [0.7, 0.75, 0.85, 1.0],
                false,
            );

            // 2.4 Right-aligned [AI] and [⚙] Action Buttons
            self.draw_top_right_buttons(
                &mut bg_instances,
                &mut glyph_instances,
                &right_btns,
                &panel_state,
                false,
                scale,
            );
        }

        // 2.5 Global AI Agents Panel
        if sidebar_open && panel_state.width_px > 0.0 {
            let layout =
                AgentPanelLayout::new(screen_w, screen_h, tab_bar_h, panel_state.width_px, scale);
            self.draw_agent_panel(
                &mut bg_instances,
                &mut glyph_instances,
                &layout,
                &panel_state,
            );
        }

        // 3. Panes and Images
        for pane in panes {
            let px = pane.rect_x;
            let py = pane.rect_y;
            let pw = pane.rect_w;
            let ph = pane.rect_h;

            let mut pane_theme = self.theme;
            if let Some((r, g, b)) = pane.dynamic_fg {
                pane_theme.default_fg = [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0];
            }
            if let Some((r, g, b)) = pane.dynamic_bg {
                pane_theme.default_bg = [
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                    self.theme.default_bg[3],
                ];
            }
            if let Some((r, g, b)) = pane.dynamic_cursor {
                pane_theme.cursor_color = [
                    r as f32 / 255.0,
                    g as f32 / 255.0,
                    b as f32 / 255.0,
                    self.theme.cursor_color[3],
                ];
            }

            bg_instances.push(BgInstance {
                rect: [px, py, pw, ph],
                color: pane_theme.default_bg,
            });

            let border_col = if pane.is_focused && panes.len() > 1 {
                self.theme.focused_border_color
            } else {
                self.theme.border_color
            };

            if panes.len() > 1 {
                bg_instances.push(BgInstance {
                    rect: [px, py, pw, 1.0],
                    color: border_col,
                });
                bg_instances.push(BgInstance {
                    rect: [px, py + ph - 1.0, pw, 1.0],
                    color: border_col,
                });
                bg_instances.push(BgInstance {
                    rect: [px, py, 1.0, ph],
                    color: border_col,
                });
                bg_instances.push(BgInstance {
                    rect: [px + pw - 1.0, py, 1.0, ph],
                    color: border_col,
                });
            }

            let active_buf = pane.grid.active();

            // Sixel / Kitty Images in this pane
            for img in &active_buf.images {
                if !self.image_cache.contains_key(&img.id) && img.width_px > 0 && img.height_px > 0
                {
                    let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                        label: Some(&format!("skterm_image_{}", img.id)),
                        size: Extent3d {
                            width: img.width_px,
                            height: img.height_px,
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: TextureDimension::D2,
                        format: TextureFormat::Rgba8Unorm,
                        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                        view_formats: &[],
                    });

                    self.queue.write_texture(
                        TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: 0,
                            origin: Origin3d::ZERO,
                            aspect: TextureAspect::All,
                        },
                        &img.rgba_data,
                        TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(img.width_px * 4),
                            rows_per_image: Some(img.height_px),
                        },
                        Extent3d {
                            width: img.width_px,
                            height: img.height_px,
                            depth_or_array_layers: 1,
                        },
                    );

                    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                    let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&format!("skterm_image_bg_{}", img.id)),
                        layout: &self.pipeline.image_bind_group_layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(&self.image_sampler),
                            },
                        ],
                    });

                    self.image_cache.insert(img.id, (texture, view, bind_group));
                }

                let ix = px + self.padding_x + (img.col as f32 * cell_w);
                let iy = py + self.padding_y + (img.row as f32 * cell_h);
                let iw = (img.width_cells as f32 * cell_w).min(pw);
                let ih = (img.height_cells as f32 * cell_h).min(ph);

                if ix < px + pw && iy < py + ph {
                    image_draw_calls.push((
                        img.id,
                        ImageInstance {
                            rect: [ix, iy, iw, ih],
                        },
                    ));
                }
            }

            // Grid Cells (with Ghostty/cmux-style inline inverse-video IME preedit cells on cursor.row)
            let preedit_grid_span = if pane.is_focused
                && pane.search_query.is_none()
                && active_buf.scroll_offset == 0
            {
                pane.ime_preedit
                    .and_then(|s| {
                        PreeditOverlayLayout::compute_grid_cols(
                            s,
                            active_buf.cursor.col,
                            active_buf.cols,
                        )
                    })
                    .map(|(start_col, end_col)| (active_buf.cursor.row, start_col, end_col))
            } else {
                None
            };

            let top_global_row = active_buf
                .scrollback
                .len()
                .saturating_sub(active_buf.scroll_offset);

            for r_idx in 0..active_buf.rows {
                let y = py + self.padding_y + (r_idx as f32 * cell_h);
                if y + cell_h > py + ph {
                    break;
                }
                let global_row = top_global_row + r_idx;

                if let Some(row) = active_buf.get_visible_row(r_idx) {
                    // `ScreenBuffer::search` emits matches in ascending `global_row`
                    // order, so this row's matches are a contiguous subslice. Finding
                    // it once per row turns the per-cell highlight test from
                    // O(cells * total_matches) into O(cells * matches_on_this_row),
                    // which is 0 or 1 for almost every row. Searching a common word
                    // in a 10k-line scrollback used to make every frame scan
                    // thousands of matches for each of ~30k visible cells.
                    let row_match_start = pane
                        .search_matches
                        .partition_point(|m| m.global_row < global_row);
                    let row_match_end = pane.search_matches[row_match_start..]
                        .partition_point(|m| m.global_row == global_row)
                        + row_match_start;
                    let row_matches = &pane.search_matches[row_match_start..row_match_end];

                    for (c_idx, cell) in row.cells.iter().enumerate() {
                        if let Some((preedit_row, start_col, end_col)) = preedit_grid_span {
                            if r_idx == preedit_row {
                                if c_idx >= start_col && c_idx < end_col {
                                    continue;
                                }
                                // If a wide character starts at `start_col - 1`, its right half overlaps
                                // `start_col`, so skip it cleanly while preedit occupies `start_col`.
                                if c_idx + 1 == start_col
                                    && row.cells.get(start_col).is_some_and(|next| {
                                        next.flags.contains(CellFlags::WIDE_CHAR_SPACER)
                                    })
                                {
                                    continue;
                                }
                            }
                        }

                        let x = px + self.padding_x + (c_idx as f32 * cell_w);
                        if x + cell_w > px + pw {
                            break;
                        }

                        let is_selected = active_buf
                            .selection
                            .as_ref()
                            .map(|s| s.contains(r_idx, c_idx))
                            .unwrap_or(false);

                        // Fast-path: Skip default empty blank cells (typically 80-90% of screen).
                        // Gated on this row's matches, not the whole pane's: an active
                        // search used to disable this fast path for the entire screen.
                        if cell.c == ' '
                            && cell.bg == Color::DefaultBg
                            && !cell.flags.intersects(
                                CellFlags::INVERSE
                                    | CellFlags::UNDERLINE
                                    | CellFlags::STRIKETHROUGH
                                    | CellFlags::OVERLINE,
                            )
                            && !is_selected
                            && row_matches.is_empty()
                        {
                            continue;
                        }

                        let is_inverse = cell.flags.contains(CellFlags::INVERSE);
                        let is_bold_flag = cell.flags.contains(CellFlags::BOLD);
                        let (mut fg_rgba, bg_rgba) = if is_inverse {
                            (
                                color_to_rgba_with_dynamic_palette(
                                    cell.bg,
                                    false,
                                    is_bold_flag,
                                    &pane_theme,
                                    pane.dynamic_palette,
                                ),
                                color_to_rgba_with_dynamic_palette(
                                    cell.fg,
                                    true,
                                    false,
                                    &pane_theme,
                                    pane.dynamic_palette,
                                ),
                            )
                        } else {
                            (
                                color_to_rgba_with_dynamic_palette(
                                    cell.fg,
                                    false,
                                    is_bold_flag,
                                    &pane_theme,
                                    pane.dynamic_palette,
                                ),
                                color_to_rgba_with_dynamic_palette(
                                    cell.bg,
                                    true,
                                    false,
                                    &pane_theme,
                                    pane.dynamic_palette,
                                ),
                            )
                        };

                        if cell.flags.contains(CellFlags::DIM) {
                            fg_rgba[0] *= 0.66;
                            fg_rgba[1] *= 0.66;
                            fg_rgba[2] *= 0.66;
                        }

                        if bg_rgba != pane_theme.default_bg {
                            bg_instances.push(BgInstance {
                                rect: [x, y, cell_w, cell_h],
                                color: bg_rgba,
                            });
                        }

                        // Highlight search matches
                        if !row_matches.is_empty() {
                            let mut match_highlight = None;
                            for (i, m) in row_matches.iter().enumerate() {
                                if c_idx >= m.start_col && c_idx <= m.end_col {
                                    // `active_search_idx` indexes the full match list,
                                    // so map this row-local index back.
                                    if pane.active_search_idx == Some(row_match_start + i) {
                                        match_highlight = Some([0.98, 0.68, 0.12, 0.90]);
                                        break;
                                    } else {
                                        match_highlight = Some([0.85, 0.65, 0.15, 0.42]);
                                    }
                                }
                            }
                            if let Some(hl_col) = match_highlight {
                                bg_instances.push(BgInstance {
                                    rect: [x, y, cell_w, cell_h],
                                    color: hl_col,
                                });
                            }
                        }

                        if is_selected {
                            bg_instances.push(BgInstance {
                                rect: [x, y, cell_w, cell_h],
                                color: pane_theme.selection_color,
                            });
                        }

                        if !cell.flags.contains(CellFlags::HIDDEN) {
                            let line_h = scale.round().max(1.0);
                            if cell.flags.contains(CellFlags::UNDERLINE) {
                                let ul_rgba = cell
                                    .underline_color_id
                                    .and_then(|id| pane.grid.get_underline_color(id))
                                    .map(|col| {
                                        color_to_rgba_with_dynamic_palette(
                                            col,
                                            false,
                                            false,
                                            &pane_theme,
                                            pane.dynamic_palette,
                                        )
                                    })
                                    .unwrap_or(fg_rgba);
                                let ul_y =
                                    (y + baseline + (2.0 * scale).round()).min(y + cell_h - line_h);

                                if cell.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
                                    let top_ul_y = (ul_y - 2.0 * line_h).max(y);
                                    bg_instances.push(BgInstance {
                                        rect: [x, top_ul_y, cell_w, line_h],
                                        color: ul_rgba,
                                    });
                                    bg_instances.push(BgInstance {
                                        rect: [x, ul_y, cell_w, line_h],
                                        color: ul_rgba,
                                    });
                                } else if cell.flags.contains(CellFlags::CURLY_UNDERLINE) {
                                    let seg_w = (cell_w / 4.0).max(1.0);
                                    for step in 0..4 {
                                        let sy = if step % 2 == 0 {
                                            (ul_y - line_h).max(y)
                                        } else {
                                            ul_y
                                        };
                                        bg_instances.push(BgInstance {
                                            rect: [x + step as f32 * seg_w, sy, seg_w, line_h],
                                            color: ul_rgba,
                                        });
                                    }
                                } else if cell.flags.contains(CellFlags::DOTTED_UNDERLINE) {
                                    let dot_w = (cell_w / 4.0).max(1.0);
                                    bg_instances.push(BgInstance {
                                        rect: [x, ul_y, dot_w, line_h],
                                        color: ul_rgba,
                                    });
                                    bg_instances.push(BgInstance {
                                        rect: [x + 2.0 * dot_w, ul_y, dot_w, line_h],
                                        color: ul_rgba,
                                    });
                                } else if cell.flags.contains(CellFlags::DASHED_UNDERLINE) {
                                    let dash_w = (cell_w * 0.65).max(1.0);
                                    let dash_x = x + (cell_w - dash_w) * 0.5;
                                    bg_instances.push(BgInstance {
                                        rect: [dash_x, ul_y, dash_w, line_h],
                                        color: ul_rgba,
                                    });
                                } else {
                                    bg_instances.push(BgInstance {
                                        rect: [x, ul_y, cell_w, line_h],
                                        color: ul_rgba,
                                    });
                                }
                            }
                            if cell.flags.contains(CellFlags::OVERLINE) {
                                bg_instances.push(BgInstance {
                                    rect: [x, y, cell_w, line_h],
                                    color: fg_rgba,
                                });
                            }
                            if cell.flags.contains(CellFlags::STRIKETHROUGH) {
                                let st_y = (y + (cell_h * 0.5).round()).min(y + cell_h - line_h);
                                bg_instances.push(BgInstance {
                                    rect: [x, st_y, cell_w, line_h],
                                    color: fg_rgba,
                                });
                            }
                        }

                        if cell.c != ' '
                            && cell.c != '\0'
                            && !cell
                                .flags
                                .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::HIDDEN)
                        {
                            // Procedural gap-free rendering for Unicode Block Elements and Box Drawing
                            if draw_custom_block_or_box_char(
                                cell.c,
                                x,
                                y,
                                cell_w,
                                cell_h,
                                fg_rgba,
                                &mut bg_instances,
                            ) {
                                continue;
                            }

                            let is_bold = cell.flags.contains(CellFlags::BOLD);
                            let is_italic = cell.flags.contains(CellFlags::ITALIC);

                            if let Some(glyph_info) = self.atlas.get_or_insert_glyph(
                                &self.queue,
                                cell.c,
                                is_bold,
                                is_italic,
                            ) {
                                if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0 {
                                    let wide_pad_x =
                                        wide_cell_center_offset(cell.c, glyph_info.advance, cell_w);
                                    let glyph_x = x + wide_pad_x + glyph_info.offset[0];
                                    let glyph_y =
                                        y + baseline - glyph_info.offset[1] - glyph_info.size[1];

                                    glyph_instances.push(GlyphInstance {
                                        rect: [
                                            glyph_x,
                                            glyph_y,
                                            glyph_info.size[0],
                                            glyph_info.size[1],
                                        ],
                                        uv: [
                                            glyph_info.uv_min[0],
                                            glyph_info.uv_min[1],
                                            glyph_info.uv_max[0],
                                            glyph_info.uv_max[1],
                                        ],
                                        color: fg_rgba,
                                    });
                                }
                            }
                        }
                    }

                    // Render inline IME preedit cells on `cursor.row` in standard terminal
                    // inverse-video style (matching Ghostty and cmux's `rebuildCells`).
                    if let Some((preedit_row, start_col, _end_col)) = preedit_grid_span {
                        if r_idx == preedit_row {
                            if let Some(preedit) = pane.ime_preedit {
                                let preedit_bg = [
                                    pane_theme.default_fg[0],
                                    pane_theme.default_fg[1],
                                    pane_theme.default_fg[2],
                                    1.0,
                                ];
                                let preedit_fg = [
                                    pane_theme.default_bg[0],
                                    pane_theme.default_bg[1],
                                    pane_theme.default_bg[2],
                                    1.0,
                                ];
                                let mut col = start_col;
                                for c in preedit.chars() {
                                    let w_cells = unicode_width::UnicodeWidthChar::width(c)
                                        .unwrap_or(1)
                                        .max(1);
                                    if col + w_cells > active_buf.cols {
                                        break;
                                    }
                                    let x = px + self.padding_x + (col as f32 * cell_w);
                                    let w_px = w_cells as f32 * cell_w;
                                    if x + w_px > px + pw {
                                        break;
                                    }
                                    bg_instances.push(BgInstance {
                                        rect: [x, y, w_px, cell_h],
                                        color: preedit_bg,
                                    });
                                    if c != ' ' && c != '\0' {
                                        if let Some(glyph_info) = self.atlas.get_or_insert_glyph(
                                            &self.queue,
                                            c,
                                            false,
                                            false,
                                        ) {
                                            if glyph_info.size[0] > 0.0 && glyph_info.size[1] > 0.0
                                            {
                                                let wide_pad_x = wide_cell_center_offset(
                                                    c,
                                                    glyph_info.advance,
                                                    cell_w,
                                                );
                                                let glyph_x = x + wide_pad_x + glyph_info.offset[0];
                                                let glyph_y = y + baseline
                                                    - glyph_info.offset[1]
                                                    - glyph_info.size[1];
                                                glyph_instances.push(GlyphInstance {
                                                    rect: [
                                                        glyph_x,
                                                        glyph_y,
                                                        glyph_info.size[0],
                                                        glyph_info.size[1],
                                                    ],
                                                    uv: [
                                                        glyph_info.uv_min[0],
                                                        glyph_info.uv_min[1],
                                                        glyph_info.uv_max[0],
                                                        glyph_info.uv_max[1],
                                                    ],
                                                    color: preedit_fg,
                                                });
                                            }
                                        }
                                    }
                                    col += w_cells;
                                }
                            }
                        }
                    }
                }
            }

            // Cursor (suppressed while inline IME preedit cells occupy the cursor position)
            if pane.is_focused
                && active_buf.cursor.visible
                && active_buf.scroll_offset == 0
                && preedit_grid_span.is_none()
            {
                let cur_x = px + self.padding_x + (active_buf.cursor.col as f32 * cell_w);
                let cur_y = py + self.padding_y + (active_buf.cursor.row as f32 * cell_h);

                if cur_x + cell_w <= px + pw && cur_y + cell_h <= py + ph {
                    let (cx, cy, cw, ch) = match active_buf.cursor.shape {
                        CursorShape::Block => (cur_x, cur_y, cell_w, cell_h),
                        CursorShape::Beam => (cur_x, cur_y, 2.0 * self.scale_factor, cell_h),
                        CursorShape::Underline => (
                            cur_x,
                            cur_y + baseline,
                            cell_w,
                            (2.0 * self.scale_factor).max(1.0),
                        ),
                    };

                    bg_instances.push(BgInstance {
                        rect: [cx, cy, cw, ch],
                        color: pane_theme.cursor_color,
                    });
                }
            }

            // Vertical Scrollbar (Overlay on right side of pane)
            if !pane.grid.is_alternate {
                if let Some(metrics) = ScrollbarMetrics::compute(
                    px,
                    py,
                    pw,
                    ph,
                    scale,
                    active_buf.scrollback.len(),
                    active_buf.rows,
                    active_buf.scroll_offset,
                    pane.is_scrollbar_hovered,
                    pane.is_scrollbar_dragging,
                ) {
                    let is_active = pane.is_scrollbar_dragging || pane.is_scrollbar_hovered;

                    // Track background
                    if is_active || pane.is_recently_scrolled {
                        let track_alpha = if pane.is_scrollbar_dragging {
                            0.08
                        } else if pane.is_scrollbar_hovered {
                            0.05
                        } else {
                            0.03
                        };
                        bg_instances.push(BgInstance {
                            rect: [
                                metrics.track_x,
                                metrics.track_y,
                                metrics.track_w,
                                metrics.track_h,
                            ],
                            color: [1.0, 1.0, 1.0, track_alpha],
                        });
                    }

                    // Thumb
                    let thumb_alpha = if pane.is_scrollbar_dragging {
                        0.75
                    } else if pane.is_scrollbar_hovered {
                        0.60
                    } else if pane.is_recently_scrolled {
                        0.50
                    } else if active_buf.scroll_offset > 0 {
                        0.40
                    } else {
                        0.22
                    };

                    let thumb_color = [1.0, 1.0, 1.0, thumb_alpha];
                    let corner = 1.0 * scale;
                    if metrics.thumb_h > 4.0 * scale && metrics.thumb_w > 2.0 * scale {
                        bg_instances.push(BgInstance {
                            rect: [
                                metrics.thumb_x + corner,
                                metrics.thumb_y,
                                metrics.thumb_w - corner * 2.0,
                                corner,
                            ],
                            color: thumb_color,
                        });
                        bg_instances.push(BgInstance {
                            rect: [
                                metrics.thumb_x,
                                metrics.thumb_y + corner,
                                metrics.thumb_w,
                                metrics.thumb_h - corner * 2.0,
                            ],
                            color: thumb_color,
                        });
                        bg_instances.push(BgInstance {
                            rect: [
                                metrics.thumb_x + corner,
                                metrics.thumb_y + metrics.thumb_h - corner,
                                metrics.thumb_w - corner * 2.0,
                                corner,
                            ],
                            color: thumb_color,
                        });
                    } else {
                        bg_instances.push(BgInstance {
                            rect: [
                                metrics.thumb_x,
                                metrics.thumb_y,
                                metrics.thumb_w,
                                metrics.thumb_h,
                            ],
                            color: thumb_color,
                        });
                    }
                }
            }
        }

        let normal_bg_len = bg_instances.len();
        let normal_glyph_len = glyph_instances.len();

        // Foreground Pane Overlays (Floating Cmd+F Find Bar)
        // Drawn in the overlay pass so their opaque backgrounds cleanly cover any
        // underlying terminal glyphs on the same cells.
        for pane in panes {
            let px = pane.rect_x;
            let py = pane.rect_y;
            let pw = pane.rect_w;

            // Floating Find Bar overlay (Cmd+F)
            if let Some(query) = pane.search_query {
                let count_str = if pane.search_matches.is_empty() {
                    "0/0".to_string()
                } else {
                    format!(
                        "{}/{}",
                        pane.active_search_idx.unwrap_or(0) + 1,
                        pane.search_matches.len()
                    )
                };
                let label = match pane.ime_preedit.filter(|s| !s.is_empty()) {
                    Some(preedit) => format!("Find: {}[{}]▏  ({})", query, preedit, count_str),
                    None => format!("Find: {}▏  ({})", query, count_str),
                };
                let bar_w = (self.measure_text_width(&label, false) + 24.0 * scale)
                    .clamp(220.0 * scale, (pw - 24.0 * scale).max(120.0));
                let bar_h = cell_h + 12.0 * scale;
                let bar_x = (px + pw - bar_w - 12.0 * scale).max(px + 4.0);
                let bar_y = py + 8.0 * scale;

                // Capsule background
                bg_instances.push(BgInstance {
                    rect: [bar_x, bar_y, bar_w, bar_h],
                    color: [0.10, 0.12, 0.17, 0.96],
                });
                // Top accent line
                bg_instances.push(BgInstance {
                    rect: [bar_x, bar_y, bar_w, 2.0 * scale],
                    color: [0.039, 0.518, 1.00, 0.95],
                });

                self.draw_text_line(
                    &mut glyph_instances,
                    &label,
                    bar_x + 12.0 * scale,
                    bar_y + 6.0 * scale,
                    bar_w - 16.0 * scale,
                    [0.96, 0.97, 1.0, 1.0],
                    false,
                );
            }
        }

        // Floating Top-Right [AI] [⚙] Pill Overlay when Tab Bar is hidden (auto 1-tab or never)
        if tab_bar_h <= 0.0 && panel_state.show_floating_top_right {
            let main_w = if sidebar_open {
                (screen_w - panel_state.width_px).max(0.0)
            } else {
                screen_w
            };
            let right_btns = TopRightButtons::compute(main_w, 0.0, scale);
            self.draw_top_right_buttons(
                &mut bg_instances,
                &mut glyph_instances,
                &right_btns,
                &panel_state,
                true,
                scale,
            );
        }

        // Floating Top Dropdown / Context Menu Overlay
        if let Some(menu) = top_menu {
            let row_h = 24.0 * scale;
            let sep_h = 6.0 * scale;
            let mut total_h = 6.0 * scale;
            for item in &menu.items {
                if item.is_separator {
                    total_h += sep_h;
                } else {
                    total_h += row_h;
                }
            }

            // Layer 1: Ambient deep drop shadow
            bg_instances.push(BgInstance {
                rect: [
                    menu.x - 4.0 * scale,
                    menu.y - 1.0,
                    menu.width + 8.0 * scale,
                    total_h + 8.0 * scale,
                ],
                color: [0.0, 0.0, 0.0, 0.35],
            });
            // Layer 2: Soft direct shadow
            bg_instances.push(BgInstance {
                rect: [
                    menu.x - 2.0 * scale,
                    menu.y - 0.5,
                    menu.width + 4.0 * scale,
                    total_h + 4.0 * scale,
                ],
                color: [0.0, 0.0, 0.0, 0.45],
            });

            // Layer 3: Crisp 1px border outline
            bg_instances.push(BgInstance {
                rect: [menu.x - 1.0, menu.y - 1.0, menu.width + 2.0, total_h + 2.0],
                color: [0.28, 0.31, 0.40, 1.0],
            });
            // Layer 4: 100% Solid opaque dark background (no transparency, zero bleed-through)
            bg_instances.push(BgInstance {
                rect: [menu.x, menu.y, menu.width, total_h],
                color: [0.11, 0.12, 0.16, 1.0],
            });

            let mut cur_y = menu.y + 3.0 * scale;
            for (idx, item) in menu.items.iter().enumerate() {
                if item.is_separator {
                    bg_instances.push(BgInstance {
                        rect: [
                            menu.x + 8.0 * scale,
                            cur_y + sep_h / 2.0,
                            menu.width - 16.0 * scale,
                            1.0,
                        ],
                        color: [0.22, 0.25, 0.33, 1.0],
                    });
                    cur_y += sep_h;
                } else {
                    let is_hovered = menu.hovered_idx == Some(idx);
                    if is_hovered {
                        bg_instances.push(BgInstance {
                            rect: [menu.x + 3.0 * scale, cur_y, menu.width - 6.0 * scale, row_h],
                            color: [0.22, 0.32, 0.52, 1.0],
                        });
                    }

                    let text_color = if is_hovered {
                        [1.0, 1.0, 1.0, 1.0]
                    } else {
                        [0.90, 0.92, 0.96, 1.0]
                    };

                    let sc_info = item
                        .shortcut
                        .as_deref()
                        .map(|sc| (sc, self.measure_text_width(sc, false)));
                    let label_max_w = match sc_info {
                        Some((_, sc_w)) => (menu.width - 28.0 * scale - sc_w).max(20.0),
                        None => (menu.width - 20.0 * scale).max(20.0),
                    };

                    self.draw_text_line_ellipsized(
                        &mut glyph_instances,
                        &item.label,
                        menu.x + 10.0 * scale,
                        cur_y + (row_h - cell_h) / 2.0,
                        label_max_w,
                        text_color,
                        is_hovered,
                    );

                    if let Some((sc, sc_w)) = sc_info {
                        self.draw_text_line(
                            &mut glyph_instances,
                            sc,
                            menu.x + menu.width - sc_w - 10.0 * scale,
                            cur_y + (row_h - cell_h) / 2.0,
                            sc_w + 4.0,
                            if is_hovered {
                                [0.85, 0.90, 1.0, 1.0]
                            } else {
                                [0.60, 0.65, 0.78, 1.0]
                            },
                            false,
                        );
                    }

                    cur_y += row_h;
                }
            }
        }

        // Upload instances to GPU buffers
        let build_ns = self.perf.since(build_start);
        let gpu_start = self.perf.now();

        let required_capacity = bg_instances.len().max(glyph_instances.len());
        self.pipeline
            .ensure_bg_capacity(&self.device, required_capacity);

        if !bg_instances.is_empty() {
            self.queue.write_buffer(
                &self.pipeline.bg_instance_buffer,
                0,
                bytemuck::cast_slice(&bg_instances),
            );
        }

        if !glyph_instances.is_empty() {
            self.queue.write_buffer(
                &self.pipeline.glyph_instance_buffer,
                0,
                bytemuck::cast_slice(&glyph_instances),
            );
        }

        // Images must have ALL their instances uploaded *before* the render pass.
        //
        // `Queue::write_buffer` is not ordered with respect to commands recorded into a
        // command encoder: every queued write is flushed before the submitted command
        // buffer executes. Writing one instance at a time inside the pass would therefore
        // leave only the last write visible, and every image would be drawn at the last
        // image's rect. Instead we upload the whole array once and select per draw via
        // the instance index.
        image_draw_calls.retain(|(id, _)| self.image_cache.contains_key(id));
        image_draw_calls.truncate(TerminalPipeline::MAX_IMAGE_INSTANCES);
        if !image_draw_calls.is_empty() {
            // De-interleave into a reused buffer rather than `collect()`ing a fresh
            // Vec, which allocated once per frame whenever any image was on screen.
            let mut img_instances = std::mem::take(&mut self.image_instances);
            img_instances.clear();
            img_instances.extend(image_draw_calls.iter().map(|(_, inst)| *inst));
            self.queue.write_buffer(
                &self.pipeline.image_instance_buffer,
                0,
                bytemuck::cast_slice(&img_instances),
            );
            self.image_instances = img_instances;
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("skterm_render_encoder"),
            });

        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("skterm_render_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.theme.default_bg[0] as f64,
                            g: self.theme.default_bg[1] as f64,
                            b: self.theme.default_bg[2] as f64,
                            a: self.theme.default_bg[3] as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            render_pass.set_bind_group(0, &self.pipeline.bind_group, &[]);

            // 1. Draw Base Background Quads (cells, selections, scrollbars, tab bar, agent panel)
            if normal_bg_len > 0 {
                render_pass.set_pipeline(&self.pipeline.bg_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.bg_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..normal_bg_len as u32);
            }

            // 2. Draw Graphic Images (Sixel / Kitty)
            // Each image owns its own bind group, so we still need one draw per image,
            // but the pipeline and vertex buffer are bound once and each draw selects
            // its instance by index into the array uploaded above.
            if !image_draw_calls.is_empty() {
                render_pass.set_pipeline(&self.pipeline.image_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.image_instance_buffer.slice(..));
                for (idx, (id, _)) in image_draw_calls.iter().enumerate() {
                    if let Some((_, _, img_bg)) = self.image_cache.get(id) {
                        render_pass.set_bind_group(1, img_bg, &[]);
                        let idx = idx as u32;
                        render_pass.draw(0..6, idx..idx + 1);
                    }
                }
            }

            // 3. Draw Base Text Glyphs (terminal text, tab bar text, agent panel text)
            if normal_glyph_len > 0 {
                render_pass.set_pipeline(&self.pipeline.glyph_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.glyph_instance_buffer.slice(..));
                render_pass.draw(0..6, 0..normal_glyph_len as u32);
            }

            // 4. Floating Top Dropdown Menu Overlay
            // Drawn strictly AFTER all base backgrounds, images, and text glyphs so that
            // terminal characters behind the menu can never show through or occlude the menu.
            if bg_instances.len() > normal_bg_len {
                render_pass.set_pipeline(&self.pipeline.bg_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.bg_instance_buffer.slice(..));
                render_pass.draw(0..6, normal_bg_len as u32..bg_instances.len() as u32);
            }
            if glyph_instances.len() > normal_glyph_len {
                render_pass.set_pipeline(&self.pipeline.glyph_pipeline);
                render_pass.set_vertex_buffer(0, self.pipeline.glyph_instance_buffer.slice(..));
                render_pass.draw(0..6, normal_glyph_len as u32..glyph_instances.len() as u32);
            }
        }

        self.queue.submit(std::iter::once(encoder.finish()));

        if let Some(overlay) = overlay_fn {
            overlay(&self.device, &self.queue, &view, &self.config);
        }

        let upload_and_submit_ns = self.perf.since(gpu_start);
        let present_start = self.perf.now();

        self.queue.present(output);
        if suboptimal {
            self.resize(self.config.width, self.config.height);
        }

        self.perf.record(
            acquire_ns,
            build_ns,
            upload_and_submit_ns,
            self.perf.since(present_start),
            self.perf.since(frame_start),
            glyph_instances.len(),
            bg_instances.len(),
        );

        // Hand the buffers back so the next frame reuses this frame's capacity.
        self.bg_instances = bg_instances;
        self.glyph_instances = glyph_instances;
        self.image_draw_calls = image_draw_calls;

        Ok(RenderSurfaceOutcome::Presented)
    }
}

/// Outcome of attempting to acquire and present a frame on the window's GPU surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderSurfaceOutcome {
    /// A swapchain texture was acquired, rendered, and presented to the compositor.
    Presented,
    /// The surface was lost or outdated and has been reconfigured; the caller should
    /// schedule another redraw so the frame is actually presented.
    Reconfigured,
    /// `get_current_texture()` timed out waiting for a drawable; the caller should
    /// retry on the next frame tick.
    Timeout,
    /// The window's surface is currently occluded (e.g. covered or on another Space);
    /// no frame was drawn, and dirty state should be preserved until un-occluded.
    Occluded,
}

/// Convert skterm Color to RGBA considering active theme, custom palette, and bold_is_bright.
pub fn color_to_rgba_themed(
    color: Color,
    is_bg: bool,
    is_bold: bool,
    theme: &RenderTheme,
) -> [f32; 4] {
    match color {
        Color::DefaultFg => {
            if is_bold && !is_bg {
                if let Some(palette) = theme.ansi_palette {
                    return palette[15];
                }
            }
            theme.default_fg
        }
        Color::DefaultBg => theme.default_bg,
        Color::Named(named) => {
            let mut idx = named as usize;
            if is_bold && !is_bg && idx < 8 {
                idx += 8;
            }
            if let Some(palette) = theme.ansi_palette {
                if idx < 16 {
                    return palette[idx];
                }
            }
            color_to_rgba(color, is_bg)
        }
        Color::Indexed(idx) if idx < 16 => {
            let mut target_idx = idx as usize;
            if is_bold && !is_bg && target_idx < 8 {
                target_idx += 8;
            }
            if let Some(palette) = theme.ansi_palette {
                return palette[target_idx];
            }
            color_to_rgba(color, is_bg)
        }
        _ => color_to_rgba(color, is_bg),
    }
}

/// Convert skterm Color to RGBA considering per-pane dynamic OSC 4 palette overrides,
/// active theme, custom palette, and bold_is_bright.
pub fn color_to_rgba_with_dynamic_palette(
    color: Color,
    is_bg: bool,
    is_bold: bool,
    theme: &RenderTheme,
    dynamic_palette: Option<&HashMap<u8, (u8, u8, u8)>>,
) -> [f32; 4] {
    if let Some(dyn_pal) = dynamic_palette {
        if !dyn_pal.is_empty() {
            let target_idx = match color {
                Color::DefaultFg if is_bold && !is_bg => Some(15u8),
                Color::Named(named) => {
                    let mut idx = named as u8;
                    if is_bold && !is_bg && idx < 8 {
                        idx += 8;
                    }
                    Some(idx)
                }
                Color::Indexed(mut idx) => {
                    if is_bold && !is_bg && idx < 8 {
                        idx += 8;
                    }
                    Some(idx)
                }
                _ => None,
            };
            if let Some(idx) = target_idx {
                if let Some(&(r, g, b)) = dyn_pal.get(&idx) {
                    let alpha = if is_bg { theme.default_bg[3] } else { 1.0 };
                    return [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, alpha];
                }
            }
        }
    }
    color_to_rgba_themed(color, is_bg, is_bold, theme)
}
