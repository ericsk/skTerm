//! Screen Buffer, Character Grid, Damage Tracking, Dual Buffer, Selection, and Terminal Graphics.

use crate::graphics::{
    kitty_diacritic_index, AnimationFrame, AnimationState, GraphicImage, PlaceholderCellMeta,
    StoredImage, VirtualPlacement, KITTY_PLACEHOLDER_CHAR, MAX_IMAGE_STORAGE_BYTES,
    PLACEHOLDER_META_TAG,
};
use bitflags::bitflags;
use std::collections::{HashMap, VecDeque};
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::Arc;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Upper bound on UTF-8 bytes stored for a single cell's grapheme cluster (Zalgo DoS guard).
const MAX_GRAPHEME_BYTES: usize = 128;

#[inline]
fn is_skin_tone_modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

#[inline]
fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Compute the terminal cell display width (`1` or `2`) of a Unicode grapheme cluster,
/// following Ghostty / DEC Mode `?2027` rules for combining marks, VS15/VS16 variation selectors,
/// ZWJ emoji sequences, skin-tone modifiers, keycap sequences, and regional indicator flags.
pub fn grapheme_display_width(g: &str) -> usize {
    let mut chars = g.chars();
    let Some(first) = chars.next() else {
        return 0;
    };
    let Some(second) = chars.next() else {
        return UnicodeWidthChar::width(first).unwrap_or(1).min(2);
    };

    // Regional indicator pair (e.g. "🇺🇸") -> always width 2.
    if is_regional_indicator(first) && is_regional_indicator(second) {
        return 2;
    }

    // Combining enclosing keycap (e.g. "1️⃣", "#️⃣", "*️⃣") -> width 2.
    if g.contains('\u{20E3}') {
        return 2;
    }

    // Emoji Fitzpatrick skin-tone modifier (e.g. "👍🏽", "☝🏽") -> width 2.
    if g.chars().any(is_skin_tone_modifier) {
        return 2;
    }

    // ZWJ sequences: emoji ZWJ sequences (e.g. "👨‍👩‍👧‍👦", "👩‍💻", "🏳️‍🌈", "❤️‍🔥") are width 2;
    // non-emoji script ZWJ sequences (e.g. Indic conjuncts) follow their base width.
    if g.contains('\u{200D}') {
        if g.contains('\u{FE0F}')
            || g.chars()
                .any(|ch| UnicodeWidthChar::width(ch).unwrap_or(0) >= 2)
        {
            return 2;
        }
        return UnicodeWidthChar::width(first).unwrap_or(1).clamp(1, 2);
    }

    // Text presentation selector VS15 (U+FE0E) narrows emoji presentation to width 1.
    if g.contains('\u{FE0E}') {
        return 1;
    }

    // Emoji presentation selector VS16 (U+FE0F) widens emoji bases (e.g. "❤️", "⚙️", "☀️") to width 2.
    if g.contains('\u{FE0F}') {
        if UnicodeWidthStr::width(g) >= 2 {
            return 2;
        }
        let cp = first as u32;
        if matches!(
            cp,
            0x203C
                | 0x2049
                | 0x2122
                | 0x2139
                | 0x2194..=0x2199
                | 0x21A9..=0x21AA
                | 0x231A..=0x231B
                | 0x2328
                | 0x23CF
                | 0x23E9..=0x23F3
                | 0x23F8..=0x23FA
                | 0x24C2
                | 0x25AA..=0x25AB
                | 0x25B6
                | 0x25C0
                | 0x25FB..=0x25FE
                | 0x2600..=0x27BF
                | 0x2934..=0x2935
                | 0x2B05..=0x2B07
                | 0x2B1B..=0x2B1C
                | 0x2B50
                | 0x2B55
                | 0x3030
                | 0x303D
                | 0x3297
                | 0x3299
                | 0x1F000..=0x1FAFF
        ) {
            return 2;
        }
        return 1;
    }

    UnicodeWidthStr::width(g).clamp(1, 2)
}

bitflags! {
    /// Visual cell attributes (SGR styling) and character properties.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub struct CellFlags: u16 {
        const BOLD              = 1 << 0;
        const DIM               = 1 << 1;
        const ITALIC            = 1 << 2;
        const UNDERLINE         = 1 << 3;
        const BLINK             = 1 << 4;
        const INVERSE           = 1 << 5;
        const HIDDEN            = 1 << 6;
        const STRIKETHROUGH     = 1 << 7;
        const WIDE_CHAR         = 1 << 8;
        const WIDE_CHAR_SPACER  = 1 << 9;
        const WRAP_SPACER       = 1 << 10;
        const DOUBLE_UNDERLINE  = 1 << 11;
        const CURLY_UNDERLINE   = 1 << 12;
        const DOTTED_UNDERLINE  = 1 << 13;
        const DASHED_UNDERLINE  = 1 << 14;
        const OVERLINE          = 1 << 15;
        const ANY_UNDERLINE     = Self::UNDERLINE.bits()
                                | Self::DOUBLE_UNDERLINE.bits()
                                | Self::CURLY_UNDERLINE.bits()
                                | Self::DOTTED_UNDERLINE.bits()
                                | Self::DASHED_UNDERLINE.bits();
    }
}

/// Standard 16 ANSI colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedColor {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    BrightBlack,
    BrightRed,
    BrightGreen,
    BrightYellow,
    BrightBlue,
    BrightMagenta,
    BrightCyan,
    BrightWhite,
}

/// Cell foreground / background color representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Color {
    #[default]
    DefaultFg,
    DefaultBg,
    Named(NamedColor),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

/// A single terminal character cell.
///
/// Memory here is multiplied by `(rows + scrollback) * cols`, so the layout matters:
/// at 200 columns with 10k lines of scrollback, every byte costs ~2 MB per pane.
///
/// `hyperlink_id` uses `NonZeroU32` so that `Option` can exploit the null niche
/// instead of spending 4 extra bytes on a discriminant (`Option<u32>` is 8 bytes,
/// `Option<NonZeroU32>` is 4). `underline_color_id` uses `Option<NonZeroU16>` (2 bytes)
/// to occupy the 2 bytes of alignment padding next to `flags` (`u16`), keeping `Cell`
/// at 20 bytes total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    pub underline_color_id: Option<NonZeroU16>,
    pub hyperlink_id: Option<NonZeroU32>,
}

// Lock in the layout: 4 (char) + 4 (fg) + 4 (bg) + 2 (flags) + 2 (underline_color_id) + 4 (link) = 20.
// If this ever fails, check what was added to `Cell`.
const _: () = assert!(std::mem::size_of::<Cell>() == 20);

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            flags: CellFlags::empty(),
            underline_color_id: None,
            hyperlink_id: None,
        }
    }
}

impl Cell {
    pub fn new(c: char, fg: Color, bg: Color, flags: CellFlags) -> Self {
        Self {
            c,
            fg,
            bg,
            flags,
            underline_color_id: None,
            hyperlink_id: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// An erased cell carrying the current background colour. This is what every
    /// clear/erase/scroll path fills with, so it exists as a single definition.
    #[inline]
    pub const fn blank(bg: Color) -> Self {
        Self {
            c: ' ',
            fg: Color::DefaultFg,
            bg,
            flags: CellFlags::empty(),
            underline_color_id: None,
            hyperlink_id: None,
        }
    }
}

/// A row of terminal cells with dirty tracking and sparse multi-codepoint grapheme storage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub is_dirty: bool,
    pub is_wrapped: bool,
    /// Sparse storage for multi-codepoint grapheme clusters indexed by column.
    /// `None` on pure single-codepoint rows (zero heap overhead in the common case).
    pub graphemes: Option<Box<HashMap<u16, String>>>,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            is_dirty: true,
            is_wrapped: false,
            graphemes: None,
        }
    }

    /// Return the full multi-codepoint grapheme cluster stored at `col`, if any.
    #[inline]
    pub fn cell_grapheme(&self, col: usize) -> Option<&str> {
        let map = self.graphemes.as_deref()?;
        u16::try_from(col)
            .ok()
            .and_then(|c| map.get(&c).map(String::as_str))
    }

    /// Store a multi-codepoint grapheme cluster at `col`.
    pub fn set_cell_grapheme(&mut self, col: usize, text: String) {
        let Ok(col_u16) = u16::try_from(col) else {
            return;
        };
        let map = self
            .graphemes
            .get_or_insert_with(|| Box::new(HashMap::new()));
        map.insert(col_u16, text);
    }

    /// Remove any multi-codepoint grapheme cluster at `col`.
    #[inline]
    pub fn clear_cell_grapheme(&mut self, col: usize) {
        if let Some(map) = self.graphemes.as_deref_mut() {
            if let Ok(col_u16) = u16::try_from(col) {
                map.remove(&col_u16);
                if map.is_empty() {
                    self.graphemes = None;
                }
            }
        }
    }

    /// Remove all grapheme entries whose column lies in `start..end`.
    pub fn clear_graphemes_in_range(&mut self, start: usize, end: usize) {
        if let Some(map) = self.graphemes.as_deref_mut() {
            map.retain(|&col, _| {
                let c = col as usize;
                c < start || c >= end
            });
            if map.is_empty() {
                self.graphemes = None;
            }
        }
    }

    /// Shift grapheme column indices left after `delete_chars(from_col, count)`.
    pub fn shift_graphemes_left(&mut self, from_col: usize, count: usize, cols: usize) {
        let Some(old) = self.graphemes.take() else {
            return;
        };
        let mut next = HashMap::with_capacity(old.len());
        for (col_u16, g) in *old {
            let c = col_u16 as usize;
            if c < from_col {
                next.insert(col_u16, g);
            } else if c >= from_col + count && c < cols {
                next.insert((c - count) as u16, g);
            }
        }
        if !next.is_empty() {
            self.graphemes = Some(Box::new(next));
        }
    }

    /// Shift grapheme column indices right after `insert_chars(from_col, count)`.
    pub fn shift_graphemes_right(&mut self, from_col: usize, count: usize, cols: usize) {
        let Some(old) = self.graphemes.take() else {
            return;
        };
        let mut next = HashMap::with_capacity(old.len());
        for (col_u16, g) in *old {
            let c = col_u16 as usize;
            if c < from_col {
                next.insert(col_u16, g);
            } else if c + count < cols {
                next.insert((c + count) as u16, g);
            }
        }
        if !next.is_empty() {
            self.graphemes = Some(Box::new(next));
        }
    }

    pub fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::default());
        if let Some(map) = self.graphemes.as_deref_mut() {
            map.retain(|&col, _| (col as usize) < cols);
            if map.is_empty() {
                self.graphemes = None;
            }
        }
        self.is_dirty = true;
    }

    /// Returns the number of cells up to the last non-default cell.
    /// If the row is marked as wrapped, it conceptually occupies all cells.
    pub fn content_len(&self) -> usize {
        if self.is_wrapped {
            self.cells.len()
        } else {
            self.cells
                .iter()
                .rposition(|c| {
                    c.c != ' '
                        || c.bg != Color::DefaultBg
                        || c.flags != CellFlags::empty()
                        || c.underline_color_id.is_some()
                        || c.hyperlink_id.is_some()
                })
                .map(|idx| idx + 1)
                .unwrap_or(0)
        }
    }

    pub fn clear(&mut self, bg: Color) {
        self.cells.fill(Cell::blank(bg));
        self.graphemes = None;
        self.is_dirty = true;
        self.is_wrapped = false;
    }

    /// Recycle this row's existing allocation as a fresh blank line of `cols`
    /// columns. Scrolling a full screen happens once per output line, so reusing
    /// the buffer here is what keeps the steady state allocation-free.
    pub fn reuse_as_blank(&mut self, cols: usize, bg: Color) {
        let blank = Cell::blank(bg);
        if self.cells.len() == cols {
            self.cells.fill(blank);
        } else {
            self.cells.clear();
            self.cells.resize(cols, blank);
        }
        self.graphemes = None;
        self.is_dirty = true;
        self.is_wrapped = false;
    }
}

/// Cursor visual shape
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
}

/// Mouse pointer cursor shape controlled via `OSC 22` (CSS / X11 cursor names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PointerShape {
    #[default]
    Default,
    Text,
    VerticalText,
    Pointer,
    Crosshair,
    Wait,
    Progress,
    Help,
    Grab,
    Grabbing,
    NotAllowed,
    NoDrop,
    ContextMenu,
    Cell,
    Move,
    Alias,
    Copy,
    ZoomIn,
    ZoomOut,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    AllScroll,
    None,
}

impl PointerShape {
    /// Parse a single CSS / XCursor pointer name into [`PointerShape`].
    pub fn from_css_name(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "default" | "arrow" | "left_ptr" | "top_left_arrow" => Some(Self::Default),
            "text" | "ibeam" | "xterm" => Some(Self::Text),
            "vertical-text" => Some(Self::VerticalText),
            "pointer" | "hand" | "hand1" | "hand2" | "pointing_hand" => Some(Self::Pointer),
            "crosshair" | "cross" | "tcross" => Some(Self::Crosshair),
            "wait" | "watch" => Some(Self::Wait),
            "progress" | "left_ptr_watch" | "half-busy" => Some(Self::Progress),
            "help" | "question_arrow" | "whats_this" => Some(Self::Help),
            "grab" | "openhand" => Some(Self::Grab),
            "grabbing" | "closedhand" | "dnd-move" => Some(Self::Grabbing),
            "not-allowed" | "crossed_circle" | "forbidden" => Some(Self::NotAllowed),
            "no-drop" | "dnd-no-drop" => Some(Self::NoDrop),
            "context-menu" => Some(Self::ContextMenu),
            "cell" | "plus" => Some(Self::Cell),
            "move" | "fleur" | "size_all" => Some(Self::Move),
            "alias" | "dnd-link" | "link" => Some(Self::Alias),
            "copy" | "dnd-copy" => Some(Self::Copy),
            "zoom-in" => Some(Self::ZoomIn),
            "zoom-out" => Some(Self::ZoomOut),
            "ew-resize" | "e-resize" | "w-resize" | "sb_h_double_arrow" | "size_hor" => {
                Some(Self::EwResize)
            }
            "ns-resize" | "n-resize" | "s-resize" | "sb_v_double_arrow" | "size_ver" => {
                Some(Self::NsResize)
            }
            "nesw-resize" | "ne-resize" | "sw-resize" | "size_bdiag" => Some(Self::NeswResize),
            "nwse-resize" | "nw-resize" | "se-resize" | "size_fdiag" => Some(Self::NwseResize),
            "col-resize" | "split_h" => Some(Self::ColResize),
            "row-resize" | "split_v" => Some(Self::RowResize),
            "all-scroll" => Some(Self::AllScroll),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    /// Canonical CSS cursor name reported by `OSC 22 ; ?`.
    pub fn as_css_name(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Text => "text",
            Self::VerticalText => "vertical-text",
            Self::Pointer => "pointer",
            Self::Crosshair => "crosshair",
            Self::Wait => "wait",
            Self::Progress => "progress",
            Self::Help => "help",
            Self::Grab => "grab",
            Self::Grabbing => "grabbing",
            Self::NotAllowed => "not-allowed",
            Self::NoDrop => "no-drop",
            Self::ContextMenu => "context-menu",
            Self::Cell => "cell",
            Self::Move => "move",
            Self::Alias => "alias",
            Self::Copy => "copy",
            Self::ZoomIn => "zoom-in",
            Self::ZoomOut => "zoom-out",
            Self::EwResize => "ew-resize",
            Self::NsResize => "ns-resize",
            Self::NeswResize => "nesw-resize",
            Self::NwseResize => "nwse-resize",
            Self::ColResize => "col-resize",
            Self::RowResize => "row-resize",
            Self::AllScroll => "all-scroll",
            Self::None => "none",
        }
    }
}

/// Progress indicator state reported via `OSC 9 ; 4 ; <state> ; <percent>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressState {
    /// Normal active progress (`state = 1`).
    Normal,
    /// Error progress (`state = 2`).
    Error,
    /// Indeterminate / busy activity (`state = 3`).
    Indeterminate,
    /// Paused / warning progress (`state = 4`).
    Paused,
}

/// Active `OSC 9 ; 4` progress bar state for a terminal pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalProgress {
    pub state: ProgressState,
    /// Percentage clamped to `0..=100`.
    pub percent: u8,
}

/// Current semantic shell zone (`OSC 133` / `OSC 633`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SemanticZone {
    #[default]
    Idle,
    /// Between `133;A` (prompt start) and `133;B`.
    Prompt,
    /// Between `133;B` (command input start) and `133;C`.
    CommandInput,
    /// Between `133;C` (command execution start) and `133;D`.
    CommandOutput,
}

/// A semantic prompt marker (`OSC 133;A` / `OSC 633;A` / `OSC 1337;SetMark`) anchored to a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PromptMark {
    /// Row index (`>= 0` indexes live `grid[row]`; `< 0` indexes `-row` lines into `scrollback` from newest).
    pub row: isize,
    /// Exit status reported by `OSC 133;D;<exit>` / `OSC 633;D;<exit>` when the command completed.
    pub exit_code: Option<i32>,
}

/// Terminal cursor state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub visible: bool,
    pub blink: bool,
    pub pending_wrap: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: true,
            blink: true,
            pending_wrap: false,
        }
    }
}

/// Saved cursor snapshot for DECSC / DECRC
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedCursor {
    pub row: usize,
    pub col: usize,
    pub fg: Color,
    pub bg: Color,
    pub underline_color: Option<Color>,
    pub underline_color_id: Option<NonZeroU16>,
    pub flags: CellFlags,
    pub shape: CursorShape,
    pub visible: bool,
    pub blink: bool,
    pub pending_wrap: bool,
    pub origin_mode: bool,
    pub auto_wrap: bool,
}

/// Text Selection Mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SelectionMode {
    #[default]
    Simple,
    Word,
    Line,
    Block,
}

/// Active text selection range
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub start: (usize, usize), // (row, col)
    pub end: (usize, usize),   // (row, col)
    pub mode: SelectionMode,
}

impl Selection {
    pub fn new(start: (usize, usize), end: (usize, usize), mode: SelectionMode) -> Self {
        Self { start, end, mode }
    }

    pub fn normalized(&self) -> ((usize, usize), (usize, usize)) {
        if self.start <= self.end {
            (self.start, self.end)
        } else {
            (self.end, self.start)
        }
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (start, end) = self.normalized();
        match self.mode {
            SelectionMode::Block => {
                let min_col = start.1.min(end.1);
                let max_col = start.1.max(end.1);
                row >= start.0 && row <= end.0 && col >= min_col && col <= max_col
            }
            SelectionMode::Line => row >= start.0 && row <= end.0,
            SelectionMode::Simple | SelectionMode::Word => {
                if row < start.0 || row > end.0 {
                    false
                } else if row == start.0 && row == end.0 {
                    col >= start.1 && col <= end.1
                } else if row == start.0 {
                    col >= start.1
                } else if row == end.0 {
                    col <= end.1
                } else {
                    true
                }
            }
        }
    }
}

/// A single screen buffer containing grid rows, scrollback history, cursor state, and images.
#[derive(Debug, Clone)]
pub struct ScreenBuffer {
    pub rows: usize,
    pub cols: usize,
    /// Visible rows, newest at the back. A deque rather than a `Vec` because the
    /// single hottest operation in the whole emulator is "scroll the screen up by
    /// one line", which is a `pop_front` + `push_back` here instead of shifting
    /// every row down by one slot.
    pub grid: VecDeque<Row>,
    pub scrollback: VecDeque<Row>,
    pub max_scrollback: usize,
    pub cursor: Cursor,
    pub saved_cursor: Option<SavedCursor>,
    pub scroll_region: (usize, usize), // (top, bottom) inclusive, 0-indexed
    pub selection: Option<Selection>,
    pub scroll_offset: usize,
    pub images: Vec<GraphicImage>,
    pub images_dirty: bool,
    pub last_placeholder_pos: Option<(usize, usize)>,
    pub last_grapheme_pos: Option<(usize, usize)>,
    pub tab_stops: Vec<bool>,
    pub prompt_marks: VecDeque<PromptMark>,
    pub semantic_zone: SemanticZone,
}

impl ScreenBuffer {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        let grid = (0..rows).map(|_| Row::new(cols)).collect();
        let tab_stops = (0..cols).map(|c| c > 0 && c % 8 == 0).collect();
        Self {
            rows,
            cols,
            grid,
            scrollback: VecDeque::new(),
            max_scrollback,
            cursor: Cursor::default(),
            saved_cursor: None,
            scroll_region: (0, rows.saturating_sub(1)),
            selection: None,
            scroll_offset: 0,
            images: Vec::new(),
            images_dirty: false,
            last_placeholder_pos: None,
            last_grapheme_pos: None,
            tab_stops,
            prompt_marks: VecDeque::new(),
            semantic_zone: SemanticZone::Idle,
        }
    }

    /// Return the multi-codepoint grapheme cluster stored at live grid `(row, col)`, if any.
    #[inline]
    pub fn cell_grapheme(&self, row: usize, col: usize) -> Option<&str> {
        self.grid.get(row)?.cell_grapheme(col)
    }

    /// Return the multi-codepoint grapheme cluster stored at visible viewport `(screen_row, col)`, if any.
    #[inline]
    pub fn visible_cell_grapheme(&self, screen_row: usize, col: usize) -> Option<&str> {
        self.get_visible_row(screen_row)?.cell_grapheme(col)
    }

    /// Reset an alternate screen buffer cleanly (rows, images, prompt marks, cursor, scroll region).
    pub fn clear_alternate(&mut self) {
        for row in &mut self.grid {
            row.clear(Color::DefaultBg);
        }
        if !self.images.is_empty() {
            self.images.clear();
            self.images_dirty = true;
        }
        self.prompt_marks.clear();
        self.selection = None;
        self.scroll_offset = 0;
        self.scroll_region = (0, self.rows.saturating_sub(1));
        self.cursor.row = 0;
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    fn resize_tab_stops(&mut self, new_cols: usize) {
        let old_len = self.tab_stops.len();
        if new_cols > old_len {
            self.tab_stops
                .extend((old_len..new_cols).map(|c| c > 0 && c % 8 == 0));
        } else {
            self.tab_stops.truncate(new_cols);
        }
    }

    pub fn set_tab_stop(&mut self) {
        if self.cursor.col < self.tab_stops.len() {
            self.tab_stops[self.cursor.col] = true;
        }
    }

    pub fn clear_tab_stop(&mut self, mode: usize) {
        match mode {
            0 => {
                if self.cursor.col < self.tab_stops.len() {
                    self.tab_stops[self.cursor.col] = false;
                }
            }
            3 => {
                self.tab_stops.fill(false);
            }
            _ => {}
        }
    }

    pub fn advance_tab(&mut self, count: usize) {
        let max_col = self.cols.saturating_sub(1);
        for _ in 0..count.max(1) {
            let next = ((self.cursor.col + 1)..self.cols)
                .find(|&c| self.tab_stops.get(c).copied().unwrap_or(false))
                .unwrap_or(max_col);
            self.cursor.col = next.min(max_col);
        }
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn backward_tab(&mut self, count: usize) {
        for _ in 0..count.max(1) {
            if self.cursor.col == 0 {
                break;
            }
            let prev = (0..self.cursor.col)
                .rev()
                .find(|&c| self.tab_stops.get(c).copied().unwrap_or(false))
                .unwrap_or(0);
            self.cursor.col = prev;
        }
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn add_image(&mut self, img: GraphicImage) {
        let is_standalone = img.image_id == 0;
        self.images.push(img);
        self.images_dirty = true;

        // Enforce `MAX_IMAGE_STORAGE_BYTES` across standalone (`image_id == 0`, Sixel / OSC 1337)
        // placements so repeated inline image dumps never grow heap memory without bound.
        if is_standalone && self.images.len() > 1 {
            let mut standalone_bytes: usize = self
                .images
                .iter()
                .filter(|p| p.image_id == 0)
                .map(|p| p.rgba_data.len())
                .fold(0usize, usize::saturating_add);
            while standalone_bytes > MAX_IMAGE_STORAGE_BYTES {
                // Keep at least the newest placement at `self.images.len() - 1`.
                let last_idx = self.images.len() - 1;
                if let Some(pos) = self.images[..last_idx].iter().position(|p| p.image_id == 0) {
                    standalone_bytes =
                        standalone_bytes.saturating_sub(self.images[pos].rgba_data.len());
                    self.images.remove(pos);
                } else {
                    break;
                }
            }
        }
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == 0 || new_cols == 0 {
            return;
        }
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;

        if new_cols != self.cols {
            self.reflow(new_rows, new_cols);
        } else {
            self.resize_height_only(new_rows);
        }
    }

    /// Resize when only the number of rows changes (cols remains the same).
    pub fn resize_height_only(&mut self, new_rows: usize) {
        let mut pulled_from_scrollback: isize = 0;
        let mut pushed_to_scrollback: isize = 0;

        if new_rows > self.rows {
            let diff = new_rows - self.rows;
            for _ in 0..diff {
                if let Some(old_row) = self.scrollback.pop_back() {
                    self.grid.push_front(old_row);
                    pulled_from_scrollback += 1;
                } else {
                    self.grid.push_back(Row::new(self.cols));
                }
            }
        } else if new_rows < self.rows {
            let diff = self.rows - new_rows;
            for _ in 0..diff {
                let Some(removed) = self.grid.pop_front() else {
                    break;
                };
                pushed_to_scrollback += 1;
                if self.max_scrollback > 0 {
                    if self.scrollback.len() >= self.max_scrollback {
                        self.scrollback.pop_front();
                    }
                    self.scrollback.push_back(removed);
                }
            }
        }

        let delta = pulled_from_scrollback - pushed_to_scrollback;
        if delta != 0 && !self.images.is_empty() {
            for img in &mut self.images {
                img.row += delta;
            }
            let min_row = -(self.scrollback.len() as isize);
            self.images
                .retain(|img| img.row + (img.height_cells.max(1) as isize) > min_row);
            self.images_dirty = true;
        }
        if delta != 0 && !self.prompt_marks.is_empty() {
            for mark in &mut self.prompt_marks {
                mark.row += delta;
            }
            let min_row = -(self.scrollback.len() as isize);
            self.prompt_marks
                .retain(|m| m.row >= min_row && m.row < new_rows as isize);
        }

        self.rows = new_rows;
        self.scroll_region = (0, new_rows.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
        self.selection = None;
    }

    /// Resize without reflow (truncate/pad cells), suitable for Alternate Screen Buffer.
    pub fn resize_no_reflow(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == 0 || new_cols == 0 {
            return;
        }

        for row in &mut self.grid {
            row.resize(new_cols);
        }

        let mut pulled_from_scrollback: isize = 0;
        let mut pushed_to_scrollback: isize = 0;

        if new_rows > self.rows {
            let diff = new_rows - self.rows;
            for _ in 0..diff {
                if let Some(mut old_row) = self.scrollback.pop_back() {
                    old_row.resize(new_cols);
                    self.grid.push_front(old_row);
                    pulled_from_scrollback += 1;
                } else {
                    self.grid.push_back(Row::new(new_cols));
                }
            }
        } else if new_rows < self.rows {
            let diff = self.rows - new_rows;
            for _ in 0..diff {
                let Some(removed) = self.grid.pop_front() else {
                    break;
                };
                pushed_to_scrollback += 1;
                if self.max_scrollback > 0 {
                    if self.scrollback.len() >= self.max_scrollback {
                        self.scrollback.pop_front();
                    }
                    self.scrollback.push_back(removed);
                }
            }
        }

        let delta = pulled_from_scrollback - pushed_to_scrollback;
        if delta != 0 && !self.images.is_empty() {
            for img in &mut self.images {
                img.row += delta;
            }
            let min_row = -(self.scrollback.len() as isize);
            self.images
                .retain(|img| img.row + (img.height_cells.max(1) as isize) > min_row);
            self.images_dirty = true;
        }
        if delta != 0 && !self.prompt_marks.is_empty() {
            for mark in &mut self.prompt_marks {
                mark.row += delta;
            }
            let min_row = -(self.scrollback.len() as isize);
            self.prompt_marks
                .retain(|m| m.row >= min_row && m.row < new_rows as isize);
        }

        self.resize_tab_stops(new_cols);
        self.rows = new_rows;
        self.cols = new_cols;
        self.scroll_region = (0, new_rows.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
        self.selection = None;
    }

    /// Full buffer reflow for column width changes (split panes, window resizing).
    pub fn reflow(&mut self, new_rows: usize, new_cols: usize) {
        let total_scrollback = self.scrollback.len();
        let total_rows = total_scrollback + self.grid.len();
        let cursor_abs_row =
            total_scrollback + self.cursor.row.min(self.grid.len().saturating_sub(1));
        let cursor_col = self.cursor.col;

        let mut last_needed_row = cursor_abs_row;
        for idx in (cursor_abs_row + 1..total_rows).rev() {
            let row = &self.grid[idx - total_scrollback];
            if row.content_len() > 0 || row.is_wrapped {
                last_needed_row = idx;
                break;
            }
        }

        struct LogicalLine {
            cells: Vec<Cell>,
            graphemes: HashMap<usize, String>,
            is_wrapped: bool,
            cursor_offset: Option<usize>,
            was_soft_wrapped: bool,
        }

        let mut logical_lines: Vec<LogicalLine> = Vec::new();
        let mut current_line: Option<LogicalLine> = None;
        let mut grid_start_logical: Option<(usize, usize)> = None;
        // Maps each old `abs_row` in `0..=last_needed_row` to `(logical_line_idx, cell_offset)`.
        let mut old_row_anchors: Vec<(usize, usize)> = Vec::with_capacity(last_needed_row + 1);

        for abs_row in 0..=last_needed_row {
            let line_idx = logical_lines.len();
            let base_cell_idx = current_line.as_ref().map(|l| l.cells.len()).unwrap_or(0);
            old_row_anchors.push((line_idx, base_cell_idx));

            if abs_row == total_scrollback {
                grid_start_logical = Some((line_idx, base_cell_idx));
            }

            let row = if abs_row < total_scrollback {
                &self.scrollback[abs_row]
            } else {
                &self.grid[abs_row - total_scrollback]
            };

            let has_cursor = abs_row == cursor_abs_row;
            let len = if row.is_wrapped {
                row.cells.len()
            } else if has_cursor {
                row.content_len().max(cursor_col).min(row.cells.len())
            } else {
                row.content_len()
            };

            let mut cursor_offset_in_row = None;
            let mut row_cells: Vec<Cell> = Vec::with_capacity(len);
            let mut row_graphemes: Vec<(usize, String)> = Vec::new();
            for (c_idx, cell) in row.cells[..len].iter().enumerate() {
                if has_cursor && c_idx == cursor_col {
                    cursor_offset_in_row = Some(row_cells.len());
                }
                if !cell.flags.contains(CellFlags::WRAP_SPACER) {
                    let out_idx = row_cells.len();
                    if let Some(g) = row.cell_grapheme(c_idx) {
                        row_graphemes.push((out_idx, g.to_string()));
                    }
                    row_cells.push(*cell);
                }
            }
            if has_cursor && cursor_offset_in_row.is_none() {
                cursor_offset_in_row = Some(row_cells.len());
            }

            if let Some(mut cur) = current_line.take() {
                let base_offset = cur.cells.len();
                if let Some(off) = cursor_offset_in_row {
                    if cur.cursor_offset.is_none() {
                        cur.cursor_offset = Some(base_offset + off);
                    }
                }
                for (rel_idx, g) in row_graphemes {
                    cur.graphemes.insert(base_offset + rel_idx, g);
                }
                cur.cells.extend(row_cells);
                cur.is_wrapped = row.is_wrapped;
                cur.was_soft_wrapped = true;

                if row.is_wrapped {
                    current_line = Some(cur);
                } else {
                    logical_lines.push(cur);
                }
            } else {
                let mut graphemes = HashMap::new();
                for (rel_idx, g) in row_graphemes {
                    graphemes.insert(rel_idx, g);
                }
                let line = LogicalLine {
                    cells: row_cells,
                    graphemes,
                    is_wrapped: row.is_wrapped,
                    cursor_offset: cursor_offset_in_row,
                    was_soft_wrapped: row.is_wrapped,
                };
                if row.is_wrapped {
                    current_line = Some(line);
                } else {
                    logical_lines.push(line);
                }
            }
        }

        if let Some(cur) = current_line {
            logical_lines.push(cur);
        }

        if logical_lines.is_empty() {
            logical_lines.push(LogicalLine {
                cells: Vec::new(),
                graphemes: HashMap::new(),
                is_wrapped: false,
                cursor_offset: Some(0),
                was_soft_wrapped: false,
            });
        }

        let mut new_physical_rows: Vec<Row> = Vec::new();
        let mut new_cursor: Option<(usize, usize)> = None;
        let mut first_new_grid_row: Option<usize> = None;
        // For each logical line: list of `(chunk_start_cell, new_physical_row_idx)` in ascending order.
        let mut line_row_spans: Vec<Vec<(usize, usize)>> = Vec::with_capacity(logical_lines.len());
        // Extra physical rows that appeared *above the cursor* purely because a
        // visible, previously-unwrapped line had to be re-wrapped to fit the new
        // width. These get scrolled off the top so the cursor keeps its row --
        // see the anchoring block after this loop.
        //
        // Deliberately counts *only* those lines. A line that was already
        // soft-wrapped changes its row count on every reflow by design, and the
        // cursor is supposed to move with it (`test_buffer_reflow_on_resize`).
        let mut anchor_debt: usize = 0;

        for (line_idx, line) in logical_lines.iter().enumerate() {
            let total_cells = line.cells.len();
            let mut spans: Vec<(usize, usize)> = Vec::new();

            if total_cells == 0 {
                let row_idx = new_physical_rows.len();
                spans.push((0, row_idx));
                if first_new_grid_row.is_none() {
                    if let Some((g_line, _)) = grid_start_logical {
                        if line_idx >= g_line {
                            first_new_grid_row = Some(row_idx);
                        }
                    }
                }
                if line.cursor_offset.is_some() && new_cursor.is_none() {
                    new_cursor = Some((row_idx, 0));
                }
                new_physical_rows.push(Row::new(new_cols));
                line_row_spans.push(spans);
                continue;
            }

            let in_grid = if let Some((g_line, _)) = grid_start_logical {
                line_idx >= g_line
            } else {
                false
            };

            // Fast path: a line that already fits in the new width keeps its own
            // single physical row.
            if in_grid && !line.was_soft_wrapped && total_cells <= new_cols {
                let row_idx = new_physical_rows.len();
                spans.push((0, row_idx));
                if first_new_grid_row.is_none() {
                    first_new_grid_row = Some(row_idx);
                }

                let mut row = Row::new(new_cols);
                let copy_len = total_cells.min(new_cols);
                for c in 0..copy_len {
                    row.cells[c] = line.cells[c];
                    if let Some(g) = line.graphemes.get(&c) {
                        row.set_cell_grapheme(c, g.clone());
                    }
                    if line.cursor_offset == Some(c) && new_cursor.is_none() {
                        new_cursor = Some((row_idx, c));
                    }
                }
                if let Some(cursor_offset) = line.cursor_offset {
                    if new_cursor.is_none() {
                        let cur_c = cursor_offset.min(new_cols.saturating_sub(1));
                        new_cursor = Some((row_idx, cur_c));
                    }
                }
                row.is_wrapped = false;
                new_physical_rows.push(row);
                line_row_spans.push(spans);
                continue;
            }

            // This line only owes anchor debt if it is one the fast path above would
            // have kept on a single row before, i.e. a visible line that simply got
            // too wide. Everything else re-wraps as it always has.
            let owes_anchor_debt = in_grid && !line.was_soft_wrapped;
            let rows_before_line = new_physical_rows.len();
            let cursor_placed_before_line = new_cursor.is_some();

            let mut i = 0;
            while i < total_cells {
                let row_idx = new_physical_rows.len();
                spans.push((i, row_idx));
                if first_new_grid_row.is_none() {
                    if let Some((g_line, g_cell)) = grid_start_logical {
                        if line_idx > g_line || (line_idx == g_line && i >= g_cell) {
                            first_new_grid_row = Some(row_idx);
                        }
                    }
                }

                let mut row = Row::new(new_cols);
                let mut col = 0;

                while col < new_cols && i < total_cells {
                    let cell = line.cells[i];

                    if line.cursor_offset == Some(i) && new_cursor.is_none() {
                        new_cursor = Some((row_idx, col));
                    }

                    if cell.flags.contains(CellFlags::WIDE_CHAR) && col + 1 >= new_cols {
                        row.cells[col].flags.insert(CellFlags::WRAP_SPACER);
                        break;
                    }

                    row.cells[col] = cell;
                    if let Some(g) = line.graphemes.get(&i) {
                        row.set_cell_grapheme(col, g.clone());
                    }
                    col += 1;
                    i += 1;

                    if cell.flags.contains(CellFlags::WIDE_CHAR)
                        && i < total_cells
                        && line.cells[i].flags.contains(CellFlags::WIDE_CHAR_SPACER)
                    {
                        if line.cursor_offset == Some(i) && new_cursor.is_none() {
                            new_cursor = Some((row_idx, col));
                        }
                        if col < new_cols {
                            row.cells[col] = line.cells[i];
                            col += 1;
                        }
                        i += 1;
                    }
                }

                if line.cursor_offset == Some(i) && new_cursor.is_none() {
                    new_cursor = Some((row_idx, col.min(new_cols.saturating_sub(1))));
                }

                let is_last_chunk = i >= total_cells;
                row.is_wrapped = if is_last_chunk { line.is_wrapped } else { true };
                new_physical_rows.push(row);
            }

            if line.cursor_offset.is_some() && new_cursor.is_none() {
                let last_idx = new_physical_rows.len().saturating_sub(1);
                let last_col = new_physical_rows[last_idx]
                    .content_len()
                    .min(new_cols.saturating_sub(1));
                new_cursor = Some((last_idx, last_col));
            }

            if owes_anchor_debt && !cursor_placed_before_line {
                let extra = match new_cursor {
                    Some((cursor_row, _)) if cursor_row >= rows_before_line => {
                        cursor_row - rows_before_line
                    }
                    _ => new_physical_rows.len() - rows_before_line - 1,
                };
                anchor_debt += extra;
            }
            line_row_spans.push(spans);
        }

        let total_new = new_physical_rows.len();
        let (cursor_row_abs, cursor_col_final) =
            new_cursor.unwrap_or((total_new.saturating_sub(1), 0));
        let natural_first_grid_row = first_new_grid_row.unwrap_or(0).min(cursor_row_abs);
        let first_grid_row = (natural_first_grid_row + anchor_debt).min(cursor_row_abs);

        if first_grid_row > natural_first_grid_row {
            new_physical_rows[first_grid_row - 1].is_wrapped = false;
        }

        let grid_content_count = total_new.saturating_sub(first_grid_row);
        let grid_origin_in_new = if grid_content_count <= new_rows {
            first_grid_row
        } else {
            total_new - new_rows
        };

        if grid_content_count <= new_rows {
            let mut scrollback_rows: VecDeque<Row> =
                new_physical_rows.drain(..first_grid_row).collect();
            // `Vec -> VecDeque` reuses the same allocation, so this is not a copy.
            self.grid = new_physical_rows.into();
            while self.grid.len() < new_rows {
                self.grid.push_back(Row::new(new_cols));
            }

            if self.max_scrollback > 0 {
                while scrollback_rows.len() > self.max_scrollback {
                    scrollback_rows.pop_front();
                }
            } else {
                scrollback_rows.clear();
            }
            self.scrollback = scrollback_rows;

            self.cursor.row = (cursor_row_abs - first_grid_row).min(new_rows.saturating_sub(1));
            self.cursor.col = cursor_col_final.min(new_cols.saturating_sub(1));
        } else {
            let split_point = total_new - new_rows;
            let mut scrollback_rows: VecDeque<Row> =
                new_physical_rows.drain(..split_point).collect();
            self.grid = new_physical_rows.into();

            if self.max_scrollback > 0 {
                while scrollback_rows.len() > self.max_scrollback {
                    scrollback_rows.pop_front();
                }
            } else {
                scrollback_rows.clear();
            }
            self.scrollback = scrollback_rows;

            if cursor_row_abs >= split_point {
                self.cursor.row = (cursor_row_abs - split_point).min(new_rows.saturating_sub(1));
            } else {
                self.cursor.row = 0;
            }
            self.cursor.col = cursor_col_final.min(new_cols.saturating_sub(1));
        }

        // Remap `images` and `prompt_marks` row coordinates to their post-reflow physical rows.
        if !self.images.is_empty() || !self.prompt_marks.is_empty() {
            let map_old_rel_row = |old_rel: isize| -> isize {
                let old_abs_i = total_scrollback as isize + old_rel;
                if old_abs_i < 0 {
                    return old_abs_i - grid_origin_in_new as isize;
                }
                let old_abs = old_abs_i as usize;
                let lookup_row = |abs_r: usize| -> usize {
                    let (l_idx, cell_off) = old_row_anchors[abs_r];
                    let spans = &line_row_spans[l_idx];
                    let pos = spans.partition_point(|&(start, _)| start <= cell_off);
                    spans[pos.saturating_sub(1)].1
                };
                let new_abs = if old_abs <= last_needed_row {
                    lookup_row(old_abs)
                } else {
                    let base = lookup_row(last_needed_row);
                    base + (old_abs - last_needed_row)
                };
                new_abs as isize - grid_origin_in_new as isize
            };

            let min_row = -(self.scrollback.len() as isize);
            if !self.images.is_empty() {
                for img in &mut self.images {
                    img.row = map_old_rel_row(img.row);
                }
                self.images
                    .retain(|img| img.row + (img.height_cells.max(1) as isize) > min_row);
                self.images_dirty = true;
            }
            if !self.prompt_marks.is_empty() {
                for mark in &mut self.prompt_marks {
                    mark.row = map_old_rel_row(mark.row);
                }
                self.prompt_marks
                    .retain(|m| m.row >= min_row && m.row < new_rows as isize);
                let mut sorted: Vec<PromptMark> = self.prompt_marks.drain(..).collect();
                sorted.sort_by_key(|m| m.row);
                sorted.dedup_by_key(|m| m.row);
                self.prompt_marks = sorted.into();
            }
        }

        self.resize_tab_stops(new_cols);
        self.rows = new_rows;
        self.cols = new_cols;
        self.scroll_region = (0, new_rows.saturating_sub(1));
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
        self.scroll_offset = 0;
        self.selection = None;
    }

    pub fn scroll_display(&mut self, delta: isize) {
        let max_scroll = self.scrollback.len();
        if delta > 0 {
            self.scroll_offset = self
                .scroll_offset
                .saturating_add(delta as usize)
                .min(max_scroll);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub((-delta) as usize);
        }
    }

    pub fn get_visible_row(&self, screen_row: usize) -> Option<&Row> {
        if self.scroll_offset == 0 {
            self.grid.get(screen_row)
        } else {
            let total_history = self.scrollback.len();
            if self.scroll_offset > total_history {
                return self.grid.get(screen_row);
            }
            let history_start_idx = total_history - self.scroll_offset;
            let virtual_idx = history_start_idx + screen_row;
            if virtual_idx < total_history {
                self.scrollback.get(virtual_idx)
            } else {
                let grid_idx = virtual_idx - total_history;
                self.grid.get(grid_idx)
            }
        }
    }

    pub fn extract_selected_text(&self) -> String {
        let sel = match &self.selection {
            Some(s) => s,
            None => return String::new(),
        };

        let (start, end) = sel.normalized();
        let mut result = String::new();

        for r_idx in start.0..=end.0 {
            if let Some(row) = self.get_visible_row(r_idx) {
                if row.cells.is_empty() {
                    continue;
                }
                let last_cell_idx = row.cells.len().saturating_sub(1);
                let (col_start, col_end) = match sel.mode {
                    SelectionMode::Line => (0, last_cell_idx),
                    SelectionMode::Block => (
                        start.1.min(end.1).min(last_cell_idx),
                        start.1.max(end.1).min(last_cell_idx),
                    ),
                    SelectionMode::Simple | SelectionMode::Word => {
                        let cs = if r_idx == start.0 { start.1 } else { 0 };
                        let ce = if r_idx == end.0 {
                            end.1.min(last_cell_idx)
                        } else {
                            last_cell_idx
                        };
                        (cs, ce)
                    }
                };

                if col_start > col_end {
                    if r_idx < end.0 && (!row.is_wrapped || sel.mode == SelectionMode::Block) {
                        result.push('\n');
                    }
                    continue;
                }

                let mut line_str = String::new();
                // If selection starts on the trailing spacer cell of a wide character,
                // include the leading character cell so the first character isn't dropped.
                if col_start > 0
                    && col_start < row.cells.len()
                    && row.cells[col_start]
                        .flags
                        .contains(CellFlags::WIDE_CHAR_SPACER)
                {
                    let prev_col = col_start - 1;
                    let prev = &row.cells[prev_col];
                    if !prev
                        .flags
                        .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::WRAP_SPACER)
                    {
                        if prev.c == KITTY_PLACEHOLDER_CHAR {
                            line_str.push(' ');
                        } else if let Some(g) = row.cell_grapheme(prev_col) {
                            line_str.push_str(g);
                        } else {
                            line_str.push(prev.c);
                        }
                    }
                }

                for c_idx in col_start..=col_end {
                    if let Some(cell) = row.cells.get(c_idx) {
                        if !cell
                            .flags
                            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::WRAP_SPACER)
                        {
                            if cell.c == KITTY_PLACEHOLDER_CHAR {
                                line_str.push(' ');
                            } else if let Some(g) = row.cell_grapheme(c_idx) {
                                line_str.push_str(g);
                            } else {
                                line_str.push(cell.c);
                            }
                        }
                    }
                }

                let continues_soft_wrap =
                    r_idx < end.0 && row.is_wrapped && sel.mode != SelectionMode::Block;
                if continues_soft_wrap {
                    result.push_str(&line_str);
                } else {
                    result.push_str(line_str.trim_end());
                    if r_idx < end.0 {
                        result.push('\n');
                    }
                }
            }
        }

        result
    }

    pub fn move_cursor(&mut self, row: usize, col: usize) {
        self.cursor.row = row.min(self.rows.saturating_sub(1));
        self.cursor.col = col.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn move_up(&mut self, n: usize) {
        let (top, _) = self.scroll_region;
        if self.cursor.row >= top {
            self.cursor.row = self.cursor.row.saturating_sub(n).max(top);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(n);
        }
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn move_down(&mut self, n: usize) {
        let (_, bottom) = self.scroll_region;
        if self.cursor.row <= bottom {
            self.cursor.row = (self.cursor.row + n).min(bottom);
        } else {
            self.cursor.row = (self.cursor.row + n).min(self.rows.saturating_sub(1));
        }
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn move_forward(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn move_backward(&mut self, n: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(n);
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn linefeed(&mut self) {
        self.linefeed_with_bg(Color::DefaultBg);
    }

    pub fn linefeed_with_bg(&mut self, bg: Color) {
        let (_, bottom) = self.scroll_region;
        if self.cursor.row == bottom {
            self.scroll_up_with_bg(1, bg);
        } else if self.cursor.row < self.rows.saturating_sub(1) {
            self.cursor.row += 1;
        }
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn newline(&mut self) {
        self.newline_with_bg(Color::DefaultBg);
    }

    pub fn newline_with_bg(&mut self, bg: Color) {
        self.linefeed_with_bg(bg);
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_up_with_bg(n, Color::DefaultBg);
    }

    pub fn scroll_up_with_bg(&mut self, n: usize, bg: Color) {
        let (top, bottom) = self.scroll_region;
        let full_screen = top == 0 && bottom == self.rows.saturating_sub(1);
        let cols = self.cols;
        let mut actual_scrolled: isize = 0;

        for _ in 0..n {
            if full_screen {
                let Some(evicted) = self.grid.pop_front() else {
                    break;
                };
                // The blank line appended at the bottom reuses an existing buffer:
                // either the row falling out of the scrollback ring, or -- when
                // scrollback is off -- the row we just removed.
                let mut blank = if self.max_scrollback > 0 {
                    let recycled = if self.scrollback.len() >= self.max_scrollback {
                        self.scrollback.pop_front()
                    } else {
                        None
                    };
                    self.scrollback.push_back(evicted);
                    match recycled {
                        Some(r) => r,
                        None => Row::new(cols),
                    }
                } else {
                    evicted
                };
                blank.reuse_as_blank(cols, bg);
                self.grid.push_back(blank);
                actual_scrolled += 1;
            } else {
                let Some(mut row) = self.grid.remove(top) else {
                    break;
                };
                row.reuse_as_blank(cols, bg);
                self.grid.insert(bottom, row);
                actual_scrolled += 1;
            }
        }

        if actual_scrolled > 0 && !self.images.is_empty() {
            if full_screen {
                for img in &mut self.images {
                    img.row -= actual_scrolled;
                }
                let min_row = -(self.scrollback.len() as isize);
                self.images
                    .retain(|img| img.row + (img.height_cells.max(1) as isize) > min_row);
            } else {
                let top_i = top as isize;
                let bottom_i = bottom as isize;
                self.images.retain_mut(|img| {
                    if img.row >= top_i && img.row <= bottom_i {
                        img.row -= actual_scrolled;
                        img.row + (img.height_cells.max(1) as isize) > top_i
                    } else {
                        true
                    }
                });
            }
            self.images_dirty = true;
        }
        if actual_scrolled > 0 && !self.prompt_marks.is_empty() {
            if full_screen {
                for mark in &mut self.prompt_marks {
                    mark.row -= actual_scrolled;
                }
                let min_row = -(self.scrollback.len() as isize);
                self.prompt_marks.retain(|m| m.row >= min_row);
            } else {
                let top_i = top as isize;
                let bottom_i = bottom as isize;
                self.prompt_marks.retain_mut(|m| {
                    if m.row >= top_i && m.row <= bottom_i {
                        m.row -= actual_scrolled;
                        m.row >= top_i
                    } else {
                        true
                    }
                });
            }
        }
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_down_with_bg(n, Color::DefaultBg);
    }

    pub fn scroll_down_with_bg(&mut self, n: usize, bg: Color) {
        let (top, bottom) = self.scroll_region;
        let cols = self.cols;
        let mut actual_scrolled: isize = 0;
        for _ in 0..n {
            let Some(mut row) = self.grid.remove(bottom) else {
                break;
            };
            row.reuse_as_blank(cols, bg);
            self.grid.insert(top, row);
            actual_scrolled += 1;
        }

        if actual_scrolled > 0 && !self.images.is_empty() {
            let top_i = top as isize;
            let bottom_i = bottom as isize;
            self.images.retain_mut(|img| {
                if img.row >= top_i && img.row <= bottom_i {
                    img.row += actual_scrolled;
                    img.row <= bottom_i
                } else {
                    true
                }
            });
            self.images_dirty = true;
        }
        if actual_scrolled > 0 && !self.prompt_marks.is_empty() {
            let top_i = top as isize;
            let bottom_i = bottom as isize;
            self.prompt_marks.retain_mut(|m| {
                if m.row >= top_i && m.row <= bottom_i {
                    m.row += actual_scrolled;
                    m.row <= bottom_i
                } else {
                    true
                }
            });
        }
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    /// Clean up any wide characters that would be split across the boundaries of `[start_col, end_col)`.
    /// - If `start_col` is the right half (`WIDE_CHAR_SPACER`) of a wide character at `start_col - 1`,
    ///   clear `start_col - 1`.
    /// - If `end_col` is the right half (`WIDE_CHAR_SPACER`) of a wide character at `end_col - 1`,
    ///   clear `end_col`.
    pub fn clear_split_wide_chars(&mut self, row: usize, start_col: usize, end_col: usize) {
        if row >= self.rows {
            return;
        }
        if start_col < self.cols
            && self.grid[row].cells[start_col]
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            if start_col > 0 {
                self.grid[row].cells[start_col - 1].c = ' ';
                self.grid[row].cells[start_col - 1]
                    .flags
                    .remove(CellFlags::WIDE_CHAR);
                self.grid[row].clear_cell_grapheme(start_col - 1);
            }
            self.grid[row].cells[start_col]
                .flags
                .remove(CellFlags::WIDE_CHAR_SPACER);
            self.grid[row].clear_cell_grapheme(start_col);
        }
        if end_col < self.cols
            && self.grid[row].cells[end_col]
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            if end_col > 0 {
                self.grid[row].cells[end_col - 1].c = ' ';
                self.grid[row].cells[end_col - 1]
                    .flags
                    .remove(CellFlags::WIDE_CHAR);
                self.grid[row].clear_cell_grapheme(end_col - 1);
            }
            self.grid[row].cells[end_col].c = ' ';
            self.grid[row].cells[end_col]
                .flags
                .remove(CellFlags::WIDE_CHAR_SPACER);
            self.grid[row].clear_cell_grapheme(end_col);
        }
    }

    /// Returns `true` if `c` would attach to the preceding cell (Kitty placeholder diacritic or
    /// grapheme cluster extension) rather than opening a new cell.
    pub fn will_extend_grapheme(&self, c: char, grapheme_clustering: bool) -> bool {
        if c == KITTY_PLACEHOLDER_CHAR {
            return false;
        }
        if self.last_placeholder_pos.is_some() && kitty_diacritic_index(c).is_some() {
            return true;
        }
        let Some((pr, pc)) = self.last_grapheme_pos else {
            return false;
        };
        let Some(row) = self.grid.get(pr) else {
            return false;
        };
        let Some(prev_cell) = row.cells.get(pc) else {
            return false;
        };
        if c.is_ascii() && prev_cell.c.is_ascii() && row.cell_grapheme(pc).is_none() {
            return false;
        }
        if !grapheme_clustering {
            return UnicodeWidthChar::width(c) == Some(0);
        }
        let mut candidate = String::with_capacity(16);
        if let Some(existing) = row.cell_grapheme(pc) {
            candidate.push_str(existing);
        } else {
            candidate.push(prev_cell.c);
        }
        candidate.push(c);
        UnicodeSegmentation::graphemes(candidate.as_str(), true).count() == 1
    }

    pub fn write_char(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        hyperlink_id: Option<NonZeroU32>,
    ) {
        self.write_char_with_wrap(c, fg, bg, flags, hyperlink_id, true);
    }

    pub fn write_char_with_wrap(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        hyperlink_id: Option<NonZeroU32>,
        auto_wrap: bool,
    ) {
        self.write_char_with_style(c, fg, bg, flags, None, hyperlink_id, auto_wrap);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_char_with_style(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        underline_color_id: Option<NonZeroU16>,
        hyperlink_id: Option<NonZeroU32>,
        auto_wrap: bool,
    ) {
        self.write_char_with_grapheme_mode(
            c,
            fg,
            bg,
            flags,
            underline_color_id,
            hyperlink_id,
            auto_wrap,
            true,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn write_char_with_grapheme_mode(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        underline_color_id: Option<NonZeroU16>,
        hyperlink_id: Option<NonZeroU32>,
        auto_wrap: bool,
        grapheme_clustering: bool,
    ) {
        let char_width = if c == KITTY_PLACEHOLDER_CHAR {
            1
        } else {
            UnicodeWidthChar::width(c).unwrap_or(1)
        };

        // 1. Kitty Unicode placeholder diacritics immediately following U+10EEEE.
        if char_width == 0 {
            if let Some((pr, pc)) = self.last_placeholder_pos {
                if let Some(diacritic_idx) = kitty_diacritic_index(c) {
                    if let Some(row) = self.grid.get_mut(pr) {
                        if let Some(cell) = row.cells.get_mut(pc) {
                            if cell.c == KITTY_PLACEHOLDER_CHAR {
                                let mut meta = cell
                                    .hyperlink_id
                                    .and_then(PlaceholderCellMeta::unpack)
                                    .unwrap_or_else(PlaceholderCellMeta::empty);
                                meta.push_diacritic(diacritic_idx);
                                cell.hyperlink_id = Some(meta.pack());
                                row.is_dirty = true;
                                return;
                            }
                        }
                    }
                }
            }
        }

        // 2. Grapheme cluster continuation onto the preceding base cell.
        if c != KITTY_PLACEHOLDER_CHAR {
            if let Some((pr, pc)) = self.last_grapheme_pos {
                let fast_skip = self
                    .grid
                    .get(pr)
                    .and_then(|row| {
                        row.cells.get(pc).map(|prev| {
                            c.is_ascii() && prev.c.is_ascii() && row.cell_grapheme(pc).is_none()
                        })
                    })
                    .unwrap_or(true);

                if !fast_skip {
                    if let Some(row) = self.grid.get(pr) {
                        if let Some(prev_cell) = row.cells.get(pc).copied() {
                            let existing = row.cell_grapheme(pc);
                            let mut candidate = String::with_capacity(
                                existing.map_or(prev_cell.c.len_utf8(), str::len) + c.len_utf8(),
                            );
                            if let Some(ex) = existing {
                                candidate.push_str(ex);
                            } else {
                                candidate.push(prev_cell.c);
                            }
                            candidate.push(c);

                            let attaches = if grapheme_clustering {
                                UnicodeSegmentation::graphemes(candidate.as_str(), true).count()
                                    == 1
                            } else {
                                char_width == 0
                            };

                            if attaches {
                                if candidate.len() > MAX_GRAPHEME_BYTES {
                                    return;
                                }
                                let prev_was_wide = prev_cell.flags.contains(CellFlags::WIDE_CHAR);
                                let new_width = if grapheme_clustering {
                                    grapheme_display_width(&candidate)
                                } else if prev_was_wide {
                                    2
                                } else {
                                    1
                                };

                                if !prev_was_wide && new_width == 2 {
                                    if pc + 1 < self.cols {
                                        self.clear_split_wide_chars(pr, pc + 1, pc + 2);
                                        let row = &mut self.grid[pr];
                                        row.cells[pc].flags.insert(CellFlags::WIDE_CHAR);
                                        row.set_cell_grapheme(pc, candidate);
                                        row.clear_cell_grapheme(pc + 1);
                                        let base = row.cells[pc];
                                        let spacer = &mut row.cells[pc + 1];
                                        spacer.c = ' ';
                                        spacer.fg = base.fg;
                                        spacer.bg = base.bg;
                                        spacer.flags = (base.flags - CellFlags::WIDE_CHAR)
                                            | CellFlags::WIDE_CHAR_SPACER;
                                        spacer.underline_color_id = base.underline_color_id;
                                        spacer.hyperlink_id = base.hyperlink_id;
                                        row.is_dirty = true;

                                        if pr == self.cursor.row {
                                            if pc + 2 >= self.cols {
                                                self.cursor.col = self.cols.saturating_sub(1);
                                                self.cursor.pending_wrap = auto_wrap;
                                            } else {
                                                self.cursor.col = pc + 2;
                                                self.cursor.pending_wrap = false;
                                            }
                                        }
                                        return;
                                    } else if auto_wrap && self.cols >= 2 && pr == self.cursor.row {
                                        // Base char sat at the last column and promoted to width 2:
                                        // wrap the whole grapheme cleanly onto the next line.
                                        let mut moved_cell = prev_cell;
                                        moved_cell.flags.insert(CellFlags::WIDE_CHAR);
                                        let wrap_bg = moved_cell.bg;
                                        {
                                            let old_row = &mut self.grid[pr];
                                            old_row.cells[pc] = Cell::blank(wrap_bg);
                                            old_row.cells[pc].flags.insert(CellFlags::WRAP_SPACER);
                                            old_row.clear_cell_grapheme(pc);
                                            old_row.is_wrapped = true;
                                            old_row.is_dirty = true;
                                        }
                                        self.newline_with_bg(wrap_bg);
                                        let new_row_idx = self.cursor.row;
                                        self.clear_split_wide_chars(new_row_idx, 0, 2);
                                        let new_row = &mut self.grid[new_row_idx];
                                        new_row.cells[0] = moved_cell;
                                        new_row.set_cell_grapheme(0, candidate);
                                        new_row.clear_cell_grapheme(1);
                                        let spacer = &mut new_row.cells[1];
                                        spacer.c = ' ';
                                        spacer.fg = moved_cell.fg;
                                        spacer.bg = moved_cell.bg;
                                        spacer.flags = (moved_cell.flags - CellFlags::WIDE_CHAR)
                                            | CellFlags::WIDE_CHAR_SPACER;
                                        spacer.underline_color_id = moved_cell.underline_color_id;
                                        spacer.hyperlink_id = moved_cell.hyperlink_id;
                                        new_row.is_dirty = true;
                                        self.last_grapheme_pos = Some((new_row_idx, 0));
                                        if 2 >= self.cols {
                                            self.cursor.col = self.cols.saturating_sub(1);
                                            self.cursor.pending_wrap = auto_wrap;
                                        } else {
                                            self.cursor.col = 2;
                                            self.cursor.pending_wrap = false;
                                        }
                                        return;
                                    }
                                } else if prev_was_wide && new_width == 1 && pc + 1 < self.cols {
                                    let row = &mut self.grid[pr];
                                    row.cells[pc].flags.remove(CellFlags::WIDE_CHAR);
                                    let cell_bg = row.cells[pc].bg;
                                    row.set_cell_grapheme(pc, candidate);
                                    if row.cells[pc + 1]
                                        .flags
                                        .contains(CellFlags::WIDE_CHAR_SPACER)
                                    {
                                        row.cells[pc + 1] = Cell::blank(cell_bg);
                                        row.clear_cell_grapheme(pc + 1);
                                    }
                                    row.is_dirty = true;
                                    if pr == self.cursor.row {
                                        self.cursor.col = pc + 1;
                                        self.cursor.pending_wrap = false;
                                    }
                                    return;
                                }

                                let row = &mut self.grid[pr];
                                row.set_cell_grapheme(pc, candidate);
                                row.is_dirty = true;
                                return;
                            }
                        }
                    }
                }
            }
        }

        // Orphan zero-width codepoint with no base cell to attach to.
        if char_width == 0 {
            return;
        }

        if self.cursor.pending_wrap {
            if auto_wrap {
                self.grid[self.cursor.row].is_wrapped = true;
                self.newline_with_bg(bg);
            } else {
                self.cursor.pending_wrap = false;
            }
        }

        if char_width == 2 && self.cursor.col + 1 >= self.cols {
            if !auto_wrap {
                return;
            }
            self.clear_split_wide_chars(self.cursor.row, self.cursor.col, self.cols);
            self.grid[self.cursor.row].cells[self.cursor.col] = Cell::blank(bg);
            self.grid[self.cursor.row].cells[self.cursor.col]
                .flags
                .insert(CellFlags::WRAP_SPACER);
            self.grid[self.cursor.row].clear_cell_grapheme(self.cursor.col);
            self.grid[self.cursor.row].is_wrapped = true;
            self.newline_with_bg(bg);
        }

        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;
        let cols = self.cols;

        self.clear_split_wide_chars(cur_row, cur_col, cur_col + char_width);

        let mut actual_flags = flags;
        if char_width == 2 {
            actual_flags.insert(CellFlags::WIDE_CHAR);
        }

        let cell_link_or_placeholder = if c == KITTY_PLACEHOLDER_CHAR {
            Some(PlaceholderCellMeta::empty().pack())
        } else {
            hyperlink_id
        };

        // Resolve the row once: this runs per printed character, and every
        // `self.grid[..]` is an index computation on the deque.
        let row = &mut self.grid[cur_row];
        row.clear_cell_grapheme(cur_col);
        let cell = &mut row.cells[cur_col];
        cell.c = c;
        cell.fg = fg;
        cell.bg = bg;
        cell.flags = actual_flags;
        cell.underline_color_id = underline_color_id;
        cell.hyperlink_id = cell_link_or_placeholder;
        row.is_dirty = true;

        if c == KITTY_PLACEHOLDER_CHAR {
            self.last_placeholder_pos = Some((cur_row, cur_col));
            self.last_grapheme_pos = None;
        } else {
            self.last_placeholder_pos = None;
            self.last_grapheme_pos = Some((cur_row, cur_col));
        }

        if char_width == 2 && cur_col + 1 < cols {
            row.clear_cell_grapheme(cur_col + 1);
            let spacer_cell = &mut row.cells[cur_col + 1];
            spacer_cell.c = ' ';
            spacer_cell.fg = fg;
            spacer_cell.bg = bg;
            spacer_cell.flags = flags | CellFlags::WIDE_CHAR_SPACER;
            spacer_cell.underline_color_id = underline_color_id;
            spacer_cell.hyperlink_id = hyperlink_id;
            self.cursor.col += 1;
        }

        if self.cursor.col + 1 >= self.cols {
            self.cursor.pending_wrap = auto_wrap;
        } else {
            self.cursor.col += 1;
        }
    }

    /// Delete `n` characters at the cursor position on the current line (DCH - CSI P).
    /// Characters to the right shift left by `n` columns, and blank cells fill from the right.
    pub fn delete_chars(&mut self, n: usize, bg: Color) {
        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;
        if cur_col >= self.cols || n == 0 {
            return;
        }
        let count = n.min(self.cols - cur_col);
        let cols = self.cols;

        self.clear_split_wide_chars(cur_row, cur_col, cur_col + count);

        let row = &mut self.grid[cur_row];
        // Shift the tail left, then blank the `count` columns it vacated.
        row.cells.copy_within(cur_col + count..cols, cur_col);
        row.cells[cols - count..].fill(Cell::blank(bg));
        row.shift_graphemes_left(cur_col, count, cols);
        row.is_dirty = true;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    /// Insert `n` blank characters at the cursor position on the current line (ICH - CSI @).
    /// Existing characters from the cursor position shift right by `n` columns.
    pub fn insert_chars(&mut self, n: usize, bg: Color) {
        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;
        if cur_col >= self.cols || n == 0 {
            return;
        }
        let count = n.min(self.cols - cur_col);
        let cols = self.cols;

        self.clear_split_wide_chars(cur_row, cur_col, cur_col);

        let row = &mut self.grid[cur_row];
        // `copy_within` handles the overlap correctly, so this replaces the
        // reverse per-cell loop.
        row.cells
            .copy_within(cur_col..cols - count, cur_col + count);
        row.cells[cur_col..cur_col + count].fill(Cell::blank(bg));
        row.shift_graphemes_right(cur_col, count, cols);
        // A wide char pushed half-off the right edge leaves a dangling lead cell.
        if row.cells[cols - 1].flags.contains(CellFlags::WIDE_CHAR) {
            row.cells[cols - 1] = Cell::blank(bg);
            row.clear_cell_grapheme(cols - 1);
        }
        row.is_dirty = true;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    /// Erase `n` characters starting at the cursor position without moving cursor (ECH - CSI X).
    pub fn erase_chars(&mut self, n: usize, bg: Color) {
        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;
        if cur_col >= self.cols || n == 0 {
            return;
        }
        let end_col = (cur_col + n).min(self.cols);

        self.clear_split_wide_chars(cur_row, cur_col, end_col);

        let row = &mut self.grid[cur_row];
        row.cells[cur_col..end_col].fill(Cell::blank(bg));
        row.clear_graphemes_in_range(cur_col, end_col);
        row.is_dirty = true;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    /// Insert `n` blank lines at the cursor row within the scroll region (IL - CSI L).
    pub fn insert_lines(&mut self, n: usize, bg: Color) {
        let (top, bottom) = self.scroll_region;
        let cur_row = self.cursor.row;
        if cur_row < top || cur_row > bottom {
            return;
        }
        let count = n.min(bottom - cur_row + 1);
        let cols = self.cols;
        let mut actual_inserted: isize = 0;
        for _ in 0..count {
            // The row pushed out of the bottom of the region becomes the blank row
            // inserted at the top, so no allocation is needed.
            let Some(mut row) = self.grid.remove(bottom) else {
                break;
            };
            row.reuse_as_blank(cols, bg);
            self.grid.insert(cur_row, row);
            actual_inserted += 1;
        }
        if actual_inserted > 0 && !self.images.is_empty() {
            let cur_i = cur_row as isize;
            let bottom_i = bottom as isize;
            self.images.retain_mut(|img| {
                if img.row >= cur_i && img.row <= bottom_i {
                    img.row += actual_inserted;
                    img.row <= bottom_i
                } else {
                    true
                }
            });
            self.images_dirty = true;
        }
        if actual_inserted > 0 && !self.prompt_marks.is_empty() {
            let cur_i = cur_row as isize;
            let bottom_i = bottom as isize;
            self.prompt_marks.retain_mut(|m| {
                if m.row >= cur_i && m.row <= bottom_i {
                    m.row += actual_inserted;
                    m.row <= bottom_i
                } else {
                    true
                }
            });
        }
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    /// Delete `n` lines at the cursor row within the scroll region (DL - CSI M).
    pub fn delete_lines(&mut self, n: usize, bg: Color) {
        let (top, bottom) = self.scroll_region;
        let cur_row = self.cursor.row;
        if cur_row < top || cur_row > bottom {
            return;
        }
        let count = n.min(bottom - cur_row + 1);
        let cols = self.cols;
        let mut actual_deleted: isize = 0;
        for _ in 0..count {
            let Some(mut row) = self.grid.remove(cur_row) else {
                break;
            };
            row.reuse_as_blank(cols, bg);
            self.grid.insert(bottom, row);
            actual_deleted += 1;
        }
        if actual_deleted > 0 && !self.images.is_empty() {
            let cur_i = cur_row as isize;
            let bottom_i = bottom as isize;
            self.images.retain_mut(|img| {
                if img.row >= cur_i && img.row <= bottom_i {
                    img.row -= actual_deleted;
                    img.row + (img.height_cells.max(1) as isize) > cur_i
                } else {
                    true
                }
            });
            self.images_dirty = true;
        }
        if actual_deleted > 0 && !self.prompt_marks.is_empty() {
            let cur_i = cur_row as isize;
            let bottom_i = bottom as isize;
            self.prompt_marks.retain_mut(|m| {
                if m.row >= cur_i && m.row <= bottom_i {
                    m.row -= actual_deleted;
                    m.row >= cur_i
                } else {
                    true
                }
            });
        }
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
    }

    pub fn erase_in_display(&mut self, mode: usize, bg: Color) {
        match mode {
            0 => {
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                self.clear_split_wide_chars(cur_row, cur_col, self.cols);
                let row = &mut self.grid[cur_row];
                row.cells[cur_col..].fill(Cell::blank(bg));
                row.clear_graphemes_in_range(cur_col, self.cols);
                row.is_dirty = true;
                for row in self.grid.iter_mut().skip(cur_row + 1) {
                    row.clear(bg);
                }
                if !self.images.is_empty() {
                    let cur_r = cur_row as isize;
                    let prev_len = self.images.len();
                    self.images.retain(|img| {
                        let img_end_r = img.row + (img.height_cells.max(1) as isize);
                        let img_end_c = img.col + img.width_cells.max(1);
                        let erased_below = img_end_r > cur_r + 1 && img.row < self.rows as isize;
                        let erased_on_cur =
                            img.row <= cur_r && img_end_r > cur_r && img_end_c > cur_col;
                        !(erased_below || erased_on_cur)
                    });
                    if self.images.len() != prev_len {
                        self.images_dirty = true;
                    }
                }
                let cur_r = cur_row as isize;
                self.prompt_marks
                    .retain(|m| m.row < cur_r || (cur_col > 0 && m.row == cur_r));
                self.cursor.pending_wrap = false;
                self.last_placeholder_pos = None;
                self.last_grapheme_pos = None;
            }
            1 => {
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                for row in self.grid.iter_mut().take(cur_row) {
                    row.clear(bg);
                }
                let end = cur_col.min(self.cols.saturating_sub(1));
                self.clear_split_wide_chars(cur_row, 0, end + 1);
                let row = &mut self.grid[cur_row];
                row.cells[..=end].fill(Cell::blank(bg));
                row.clear_graphemes_in_range(0, end + 1);
                row.is_dirty = true;
                if !self.images.is_empty() {
                    let cur_r = cur_row as isize;
                    let prev_len = self.images.len();
                    self.images.retain(|img| {
                        let img_end_r = img.row + (img.height_cells.max(1) as isize);
                        let in_scrollback_only = img_end_r <= 0;
                        let strictly_below = img.row > cur_r;
                        let strictly_right_on_cur = img.row == cur_r && img.col > end;
                        in_scrollback_only || strictly_below || strictly_right_on_cur
                    });
                    if self.images.len() != prev_len {
                        self.images_dirty = true;
                    }
                }
                let cur_r = cur_row as isize;
                self.prompt_marks.retain(|m| m.row < 0 || m.row >= cur_r);
                self.cursor.pending_wrap = false;
                self.last_placeholder_pos = None;
                self.last_grapheme_pos = None;
            }
            2 => {
                for row in &mut self.grid {
                    row.clear(bg);
                }
                let prev_len = self.images.len();
                self.images
                    .retain(|img| img.row + (img.height_cells.max(1) as isize) <= 0);
                if self.images.len() != prev_len {
                    self.images_dirty = true;
                }
                self.prompt_marks.retain(|m| m.row < 0);
                self.cursor.pending_wrap = false;
                self.last_placeholder_pos = None;
                self.last_grapheme_pos = None;
            }
            3 => {
                // Per xterm / Ghostty spec, CSI 3 J clears scrollback history ONLY,
                // leaving the visible screen intact.
                self.scrollback.clear();
                self.scroll_offset = 0;
                let prev_len = self.images.len();
                self.images
                    .retain(|img| img.row + (img.height_cells.max(1) as isize) > 0);
                if self.images.len() != prev_len {
                    self.images_dirty = true;
                }
                self.prompt_marks.retain(|m| m.row >= 0);
            }
            _ => {}
        }
    }

    pub fn erase_in_line(&mut self, mode: usize, bg: Color) {
        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;
        match mode {
            0 => {
                self.clear_split_wide_chars(cur_row, cur_col, self.cols);
                let row = &mut self.grid[cur_row];
                row.cells[cur_col..].fill(Cell::blank(bg));
                row.clear_graphemes_in_range(cur_col, self.cols);
                row.is_wrapped = false;
                row.is_dirty = true;
                self.cursor.pending_wrap = false;
                self.last_placeholder_pos = None;
                self.last_grapheme_pos = None;
            }
            1 => {
                let end = cur_col.min(self.cols.saturating_sub(1));
                self.clear_split_wide_chars(cur_row, 0, end + 1);
                let row = &mut self.grid[cur_row];
                row.cells[..=end].fill(Cell::blank(bg));
                row.clear_graphemes_in_range(0, end + 1);
                row.is_dirty = true;
                self.last_placeholder_pos = None;
                self.last_grapheme_pos = None;
            }
            2 => {
                self.grid[cur_row].clear(bg);
                self.cursor.pending_wrap = false;
                self.last_placeholder_pos = None;
                self.last_grapheme_pos = None;
            }
            _ => {}
        }
    }

    /// Clear both visible screen rows and scrollback history (`Cmd+K` behavior).
    pub fn clear_scrollback_and_screen(&mut self) {
        self.scrollback.clear();
        self.prompt_marks.clear();
        if !self.images.is_empty() {
            self.images.clear();
            self.images_dirty = true;
        }
        self.scroll_offset = 0;
        self.selection = None;
        self.last_placeholder_pos = None;
        self.last_grapheme_pos = None;
        for row in &mut self.grid {
            row.clear(Color::DefaultBg);
        }
        self.cursor.row = 0;
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    /// Compute the `(start, end)` cell coordinates of the word at `(row, col)`,
    /// expanding across soft-wrapped rows (`is_wrapped == true`).
    pub fn word_bounds_at(&self, row: usize, col: usize) -> ((usize, usize), (usize, usize)) {
        let is_word_char = |c: char| -> bool {
            c.is_alphanumeric()
                || matches!(
                    c,
                    '_' | '-' | '.' | '/' | '~' | '@' | ':' | '+' | '?' | '=' | '&' | '%' | '#'
                )
        };

        let Some(r) = self.get_visible_row(row) else {
            return ((row, col), (row, col));
        };
        if r.cells.is_empty() {
            return ((row, col), (row, col));
        }

        let mut base_col = col.min(r.cells.len().saturating_sub(1));
        if base_col > 0
            && r.cells[base_col]
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER)
        {
            base_col -= 1;
        }

        let target_cell = &r.cells[base_col];
        if target_cell.c == ' ' || target_cell.flags.contains(CellFlags::WRAP_SPACER) {
            return ((row, base_col), (row, base_col));
        }
        let target_is_word = is_word_char(target_cell.c);

        let mut start_row = row;
        let mut start_col = base_col;
        loop {
            if start_col > 0 {
                let prev_col = start_col - 1;
                let r_ref = self.get_visible_row(start_row).unwrap();
                let check_col = if prev_col > 0
                    && r_ref.cells[prev_col]
                        .flags
                        .contains(CellFlags::WIDE_CHAR_SPACER)
                {
                    prev_col - 1
                } else {
                    prev_col
                };
                let cell = &r_ref.cells[check_col];
                if !cell.flags.contains(CellFlags::WRAP_SPACER)
                    && cell.c != ' '
                    && is_word_char(cell.c) == target_is_word
                {
                    start_col = check_col;
                } else {
                    break;
                }
            } else if start_row > 0 {
                if let Some(prev_r) = self.get_visible_row(start_row - 1) {
                    if prev_r.is_wrapped && !prev_r.cells.is_empty() {
                        let mut prev_col = prev_r.cells.len() - 1;
                        if prev_col > 0
                            && prev_r.cells[prev_col]
                                .flags
                                .contains(CellFlags::WRAP_SPACER)
                        {
                            prev_col -= 1;
                        }
                        let check_col = if prev_col > 0
                            && prev_r.cells[prev_col]
                                .flags
                                .contains(CellFlags::WIDE_CHAR_SPACER)
                        {
                            prev_col - 1
                        } else {
                            prev_col
                        };
                        let cell = &prev_r.cells[check_col];
                        if !cell.flags.contains(CellFlags::WRAP_SPACER)
                            && cell.c != ' '
                            && is_word_char(cell.c) == target_is_word
                        {
                            start_row -= 1;
                            start_col = check_col;
                            continue;
                        }
                    }
                }
                break;
            } else {
                break;
            }
        }

        let mut end_row = row;
        let mut end_col =
            if target_cell.flags.contains(CellFlags::WIDE_CHAR) && base_col + 1 < r.cells.len() {
                base_col + 1
            } else {
                base_col
            };

        loop {
            let r_ref = self.get_visible_row(end_row).unwrap();
            if end_col + 1 < r_ref.cells.len() {
                let next_col = end_col + 1;
                if r_ref.cells[next_col].flags.contains(CellFlags::WRAP_SPACER) {
                    if r_ref.is_wrapped && end_row + 1 < self.rows {
                        if let Some(next_r) = self.get_visible_row(end_row + 1) {
                            if !next_r.cells.is_empty() {
                                let cell = &next_r.cells[0];
                                if !cell.flags.contains(CellFlags::WRAP_SPACER)
                                    && cell.c != ' '
                                    && is_word_char(cell.c) == target_is_word
                                {
                                    end_row += 1;
                                    end_col = if cell.flags.contains(CellFlags::WIDE_CHAR)
                                        && next_r.cells.len() > 1
                                    {
                                        1
                                    } else {
                                        0
                                    };
                                    continue;
                                }
                            }
                        }
                    }
                    break;
                } else {
                    let cell = &r_ref.cells[next_col];
                    if cell.c != ' ' && is_word_char(cell.c) == target_is_word {
                        end_col = if cell.flags.contains(CellFlags::WIDE_CHAR)
                            && next_col + 1 < r_ref.cells.len()
                        {
                            next_col + 1
                        } else {
                            next_col
                        };
                    } else {
                        break;
                    }
                }
            } else if r_ref.is_wrapped && end_row + 1 < self.rows {
                if let Some(next_r) = self.get_visible_row(end_row + 1) {
                    if !next_r.cells.is_empty() {
                        let cell = &next_r.cells[0];
                        if !cell.flags.contains(CellFlags::WRAP_SPACER)
                            && cell.c != ' '
                            && is_word_char(cell.c) == target_is_word
                        {
                            end_row += 1;
                            end_col = if cell.flags.contains(CellFlags::WIDE_CHAR)
                                && next_r.cells.len() > 1
                            {
                                1
                            } else {
                                0
                            };
                            continue;
                        }
                    }
                }
                break;
            } else {
                break;
            }
        }

        ((start_row, start_col), (end_row, end_col))
    }

    /// Compute the `(start, end)` cell coordinates of the full logical line at `row`,
    /// expanding across soft-wrapped rows (`is_wrapped == true`).
    pub fn line_bounds_at(&self, row: usize) -> ((usize, usize), (usize, usize)) {
        let mut start_row = row.min(self.rows.saturating_sub(1));
        while start_row > 0 {
            if self
                .get_visible_row(start_row - 1)
                .is_some_and(|r| r.is_wrapped)
            {
                start_row -= 1;
            } else {
                break;
            }
        }

        let mut end_row = row.min(self.rows.saturating_sub(1));
        while end_row + 1 < self.rows {
            if self.get_visible_row(end_row).is_some_and(|r| r.is_wrapped) {
                end_row += 1;
            } else {
                break;
            }
        }

        let last_col = self.cols.saturating_sub(1);
        ((start_row, 0), (end_row, last_col))
    }

    /// Select the word at visible row and column (Double-click behavior).
    pub fn select_word_at(&mut self, row: usize, col: usize) {
        let (start, end) = self.word_bounds_at(row, col);
        self.selection = Some(Selection::new(start, end, SelectionMode::Word));
    }

    /// Select the entire logical line spanning `row` (Triple-click behavior).
    pub fn select_line_at(&mut self, row: usize) {
        let (start, end) = self.line_bounds_at(row);
        self.selection = Some(Selection::new(start, end, SelectionMode::Line));
    }

    /// Select all content on the visible screen (`Cmd+A` behavior).
    pub fn select_all(&mut self) {
        if self.rows == 0 || self.cols == 0 {
            return;
        }
        let last_non_empty_row = (0..self.rows)
            .rev()
            .find(|&r| {
                self.get_visible_row(r)
                    .is_some_and(|row| row.content_len() > 0)
            })
            .unwrap_or(self.rows.saturating_sub(1));
        let last_col = self.cols.saturating_sub(1);
        self.selection = Some(Selection::new(
            (0, 0),
            (last_non_empty_row, last_col),
            SelectionMode::Line,
        ));
    }

    /// Update the active selection when dragging from `anchor` to `target`,
    /// preserving word and logical-line boundaries for `Word` and `Line` modes.
    pub fn extend_selection_to(&mut self, anchor: (usize, usize), target: (usize, usize)) {
        let Some(mode) = self.selection.as_ref().map(|s| s.mode) else {
            return;
        };
        let (start, end) = match mode {
            SelectionMode::Simple | SelectionMode::Block => (anchor, target),
            SelectionMode::Word => {
                let (a_start, a_end) = self.word_bounds_at(anchor.0, anchor.1);
                let (t_start, t_end) = self.word_bounds_at(target.0, target.1);
                (a_start.min(t_start), a_end.max(t_end))
            }
            SelectionMode::Line => {
                let (a_start, a_end) = self.line_bounds_at(anchor.0);
                let (t_start, t_end) = self.line_bounds_at(target.0);
                (a_start.min(t_start), a_end.max(t_end))
            }
        };
        self.selection = Some(Selection::new(start, end, mode));
    }

    /// Case-insensitive search across scrollback history and active screen buffer.
    /// Returns matches ordered from oldest scrollback line to bottom active row.
    ///
    /// The ordering is part of the contract, not an accident: results are sorted by
    /// `global_row` and then `start_col`, and the renderer relies on it to
    /// `partition_point` out just the matches on the row it is currently drawing.
    /// See `search_results_are_sorted_by_row_then_column`.
    ///
    /// This runs on every keystroke in the find bar, over the whole scrollback, so
    /// the per-row work matters: at 10k lines x 200 cols the previous version
    /// allocated a fresh `Vec<char>` per row (10k allocations) and called the full
    /// Unicode `to_lowercase()` on every cell (2M calls) for each character typed.
    pub fn search(&self, query: &str) -> Vec<SearchMatch> {
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let q_lower: Vec<char> = q.to_lowercase().chars().collect();
        let q_len = q_lower.len();
        let mut matches = Vec::new();

        let total_scrollback = self.scrollback.len();
        let total_lines = total_scrollback + self.rows;

        // One buffer reused for every row rather than one allocation per row.
        let mut row_chars: Vec<char> = Vec::with_capacity(self.cols);

        for global_row in 0..total_lines {
            let row_ref = if global_row < total_scrollback {
                self.scrollback.get(global_row)
            } else {
                self.grid.get(global_row - total_scrollback)
            };

            let Some(row) = row_ref else { continue };

            // `q` is trimmed, so its last character is not whitespace and a match can
            // never end inside the blank tail of a row. That makes `content_len` a
            // safe scan bound, and lets wholly-blank or too-short rows skip entirely
            // (the common case for most of a long scrollback).
            let scan_len = row.content_len().min(row.cells.len());
            if scan_len < q_len {
                continue;
            }

            row_chars.clear();
            row_chars.extend(row.cells[..scan_len].iter().map(|cell| {
                let c = cell.c;
                if c.is_ascii() {
                    // Avoids the full Unicode lowercase mapping for the overwhelming
                    // majority of terminal content.
                    c.to_ascii_lowercase()
                } else {
                    c.to_lowercase().next().unwrap_or(c)
                }
            }));

            let first = q_lower[0];
            let mut c_idx = 0;
            while c_idx + q_len <= row_chars.len() {
                // Cheap first-character reject before the full slice compare.
                if row_chars[c_idx] == first && row_chars[c_idx..c_idx + q_len] == q_lower[..] {
                    matches.push(SearchMatch {
                        global_row,
                        start_col: c_idx,
                        end_col: c_idx + q_len - 1,
                    });
                    c_idx += q_len;
                } else {
                    c_idx += 1;
                }
            }
        }

        matches
    }

    /// Adjust `scroll_offset` so that `global_row` is visible on screen.
    pub fn scroll_to_global_row(&mut self, global_row: usize) {
        let total_scrollback = self.scrollback.len();
        let desired_top = global_row.saturating_sub(self.rows / 2);
        let new_offset = total_scrollback
            .saturating_sub(desired_top)
            .min(total_scrollback);
        self.scroll_offset = new_offset;
    }

    /// Record a semantic prompt mark (`OSC 133;A` / `OSC 633;A` / `OSC 1337;SetMark`) at the
    /// current cursor row, deduplicating consecutive marks on the same row.
    pub fn record_prompt_mark(&mut self) {
        let cur_row = self.cursor.row as isize;
        if let Some(last) = self.prompt_marks.back_mut() {
            if last.row == cur_row {
                last.exit_code = None;
                return;
            }
        }
        // Keep `prompt_marks` sorted by `row` if the cursor jumped upward before emitting `133;A`.
        if let Some(pos) = self.prompt_marks.iter().position(|m| m.row == cur_row) {
            self.prompt_marks[pos].exit_code = None;
            return;
        }
        let max_marks = self
            .max_scrollback
            .saturating_add(self.rows)
            .saturating_add(64);
        if self.prompt_marks.len() >= max_marks {
            self.prompt_marks.pop_front();
        }
        let insert_idx = self.prompt_marks.partition_point(|m| m.row < cur_row);
        self.prompt_marks.insert(
            insert_idx,
            PromptMark {
                row: cur_row,
                exit_code: None,
            },
        );
    }

    /// Attach a command exit code (`OSC 133;D;<exit>` / `OSC 633;D;<exit>`) to the most recent
    /// prompt mark at or above the current cursor row.
    pub fn complete_last_prompt_mark(&mut self, exit_code: Option<i32>) {
        let cur_row = self.cursor.row as isize;
        if let Some(mark) = self
            .prompt_marks
            .iter_mut()
            .rev()
            .find(|m| m.row <= cur_row)
        {
            mark.exit_code = exit_code;
        } else if let Some(mark) = self.prompt_marks.back_mut() {
            mark.exit_code = exit_code;
        }
    }

    /// Return the [`PromptMark`] at visible viewport row `visible_row` (`0..self.rows`), if any.
    pub fn prompt_mark_at_visible_row(&self, visible_row: usize) -> Option<PromptMark> {
        if self.prompt_marks.is_empty() {
            return None;
        }
        let target_row = (visible_row as isize) - (self.scroll_offset as isize);
        let idx = self.prompt_marks.partition_point(|m| m.row < target_row);
        self.prompt_marks
            .get(idx)
            .copied()
            .filter(|m| m.row == target_row)
    }

    /// Scroll upward in history to the nearest semantic prompt above the current viewport top.
    /// Returns `true` if `scroll_offset` changed.
    pub fn scroll_to_prev_prompt(&mut self) -> bool {
        let max_scroll = self.scrollback.len();
        if max_scroll == 0 || self.prompt_marks.is_empty() {
            return false;
        }
        let mut best: Option<usize> = None;
        for mark in &self.prompt_marks {
            let desired = (-mark.row).clamp(0, max_scroll as isize) as usize;
            if desired > self.scroll_offset {
                best = Some(match best {
                    Some(cur) => cur.min(desired),
                    None => desired,
                });
            }
        }
        if let Some(target) = best {
            self.scroll_offset = target;
            true
        } else {
            false
        }
    }

    /// Scroll downward toward live output to the next semantic prompt (or `0` at the bottom).
    /// Returns `true` if `scroll_offset` changed.
    pub fn scroll_to_next_prompt(&mut self) -> bool {
        if self.scroll_offset == 0 {
            return false;
        }
        let max_scroll = self.scrollback.len();
        let mut best: Option<usize> = None;
        for mark in &self.prompt_marks {
            let desired = (-mark.row).clamp(0, max_scroll as isize) as usize;
            if desired < self.scroll_offset {
                best = Some(match best {
                    Some(cur) => cur.max(desired),
                    None => desired,
                });
            }
        }
        self.scroll_offset = best.unwrap_or(0);
        true
    }
}

/// A search match location in scrollback or active grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SearchMatch {
    pub global_row: usize,
    pub start_col: usize,
    pub end_col: usize,
}

/// Screen Grid containing dual buffers (Primary with Scrollback + Alternate Screen).
#[derive(Debug, Clone)]
pub struct ScreenGrid {
    pub primary: ScreenBuffer,
    pub alternate: ScreenBuffer,
    pub is_alternate: bool,
    pub next_hyperlink_id: NonZeroU32,
    pub hyperlinks: HashMap<NonZeroU32, Arc<String>>,
    /// Maps explicit `OSC 8 ; id=<key> ; <uri>` pairs to a shared `NonZeroU32` hyperlink ID.
    pub named_hyperlinks: HashMap<(String, String), NonZeroU32>,
    pub underline_colors: Vec<Color>,
    pub underline_color_map: HashMap<Color, NonZeroU16>,
    pub next_image_id: u32,
    pub cell_width_px: f32,
    pub cell_height_px: f32,
    pub kitty_images: HashMap<u32, StoredImage>,
    pub kitty_number_to_id: HashMap<u32, u32>,
    pub virtual_placements: HashMap<(u32, u32), VirtualPlacement>,
    pub kitty_storage_bytes: usize,
    pub kitty_seq: u64,
    pub next_internal_kitty_id: u32,
}

impl ScreenGrid {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            primary: ScreenBuffer::new(rows, cols, max_scrollback),
            alternate: ScreenBuffer::new(rows, cols, 0),
            is_alternate: false,
            // Starts at 1: zero is reserved as the `None` niche in `Cell::hyperlink_id`.
            next_hyperlink_id: NonZeroU32::new(1).unwrap(),
            hyperlinks: HashMap::new(),
            named_hyperlinks: HashMap::new(),
            underline_colors: Vec::new(),
            underline_color_map: HashMap::new(),
            next_image_id: 1,
            cell_width_px: 9.0,
            cell_height_px: 18.0,
            kitty_images: HashMap::new(),
            kitty_number_to_id: HashMap::new(),
            virtual_placements: HashMap::new(),
            kitty_storage_bytes: 0,
            kitty_seq: 0,
            next_internal_kitty_id: 0x8000_0001,
        }
    }

    pub fn alloc_texture_id(&mut self) -> u32 {
        let id = self.next_image_id;
        self.next_image_id = self.next_image_id.wrapping_add(1).max(1);
        id
    }

    pub fn alloc_internal_kitty_id(&mut self) -> u32 {
        loop {
            let id = self.next_internal_kitty_id;
            self.next_internal_kitty_id = self
                .next_internal_kitty_id
                .checked_add(1)
                .unwrap_or(0x8000_0001);
            if id != 0 && !self.kitty_images.contains_key(&id) {
                return id;
            }
        }
    }

    pub fn is_kitty_image_referenced(&self, image_id: u32) -> bool {
        self.primary.images.iter().any(|p| p.image_id == image_id)
            || self.alternate.images.iter().any(|p| p.image_id == image_id)
            || self
                .virtual_placements
                .keys()
                .any(|&(iid, _)| iid == image_id)
    }

    pub fn remove_stored_kitty_image(&mut self, image_id: u32) {
        if let Some(removed) = self.kitty_images.remove(&image_id) {
            self.kitty_storage_bytes = self.kitty_storage_bytes.saturating_sub(removed.byte_len());
            if removed.image_number != 0
                && self.kitty_number_to_id.get(&removed.image_number) == Some(&image_id)
            {
                self.kitty_number_to_id.remove(&removed.image_number);
            }
        }
        self.virtual_placements
            .retain(|&(iid, _), _| iid != image_id);
    }

    pub fn store_kitty_image(
        &mut self,
        image_id: u32,
        image_number: u32,
        width_px: u32,
        height_px: u32,
        rgba: Vec<u8>,
    ) -> u32 {
        if let Some(old) = self.kitty_images.remove(&image_id) {
            self.kitty_storage_bytes = self.kitty_storage_bytes.saturating_sub(old.byte_len());
        }

        let texture_id = self.alloc_texture_id();
        self.kitty_seq = self.kitty_seq.wrapping_add(1);
        let rgba_arc = Arc::new(rgba);
        let frame0 = AnimationFrame {
            frame_number: 1,
            rgba_data: Arc::clone(&rgba_arc),
            gap_ms: 40,
        };
        let stored = StoredImage {
            image_id,
            image_number,
            texture_id,
            width_px,
            height_px,
            rgba_data: Arc::clone(&rgba_arc),
            frames: vec![frame0],
            active_frame: 0,
            animation_state: AnimationState::Stopped,
            loop_count: 0,
            loops_completed: 0,
            next_frame_at: None,
            seq: self.kitty_seq,
        };
        let added_bytes = stored.byte_len();
        self.kitty_storage_bytes = self.kitty_storage_bytes.saturating_add(added_bytes);
        if image_number != 0 {
            self.kitty_number_to_id.insert(image_number, image_id);
        }
        self.kitty_images.insert(image_id, stored);

        // Refresh any live placements already referencing `image_id` so in-place image updates
        // immediately upload the new texture to the GPU.
        for buf in [&mut self.primary, &mut self.alternate] {
            let mut updated = false;
            for p in &mut buf.images {
                if p.image_id == image_id {
                    p.id = texture_id;
                    p.width_px = width_px;
                    p.height_px = height_px;
                    p.src_rect = [0, 0, width_px, height_px];
                    p.rgba_data = Arc::clone(&rgba_arc);
                    updated = true;
                }
            }
            if updated {
                buf.images_dirty = true;
            }
        }

        self.enforce_kitty_storage_quota(image_id);
        texture_id
    }

    pub fn enforce_kitty_storage_quota(&mut self, protected_id: u32) {
        if self.kitty_storage_bytes <= MAX_IMAGE_STORAGE_BYTES {
            return;
        }

        // Evict oldest unreferenced images first, then oldest overall (excluding `protected_id`).
        for pass in 0..2 {
            if self.kitty_storage_bytes <= MAX_IMAGE_STORAGE_BYTES {
                break;
            }
            let mut candidates: Vec<(u64, u32)> = self
                .kitty_images
                .values()
                .filter(|img| {
                    img.image_id != protected_id
                        && (pass == 1 || !self.is_kitty_image_referenced(img.image_id))
                })
                .map(|img| (img.seq, img.image_id))
                .collect();
            candidates.sort_unstable_by_key(|&(seq, _)| seq);

            for (_, victim_id) in candidates {
                if self.kitty_storage_bytes <= MAX_IMAGE_STORAGE_BYTES {
                    break;
                }
                self.primary.images.retain(|p| p.image_id != victim_id);
                self.alternate.images.retain(|p| p.image_id != victim_id);
                self.remove_stored_kitty_image(victim_id);
            }
        }
    }

    pub fn lookup_virtual_placement(
        &self,
        image_id: u32,
        placement_id: u32,
    ) -> Option<VirtualPlacement> {
        self.virtual_placements
            .get(&(image_id, placement_id))
            .or_else(|| {
                if placement_id != 0 {
                    self.virtual_placements.get(&(image_id, 0))
                } else {
                    None
                }
            })
            .copied()
    }

    pub fn register_underline_color(&mut self, color: Color) -> Option<NonZeroU16> {
        if let Some(&id) = self.underline_color_map.get(&color) {
            return Some(id);
        }
        let next_idx = self.underline_colors.len() + 1;
        let id = NonZeroU16::new(next_idx.min(u16::MAX as usize) as u16)?;
        if self.underline_colors.len() < u16::MAX as usize {
            self.underline_colors.push(color);
            self.underline_color_map.insert(color, id);
        }
        Some(id)
    }

    pub fn get_underline_color(&self, id: NonZeroU16) -> Option<Color> {
        self.underline_colors.get((id.get() - 1) as usize).copied()
    }

    pub fn register_hyperlink(&mut self, uri: &str) -> NonZeroU32 {
        self.register_hyperlink_with_id(None, uri)
    }

    pub fn register_hyperlink_with_id(
        &mut self,
        explicit_id: Option<&str>,
        uri: &str,
    ) -> NonZeroU32 {
        if let Some(eid) = explicit_id.filter(|s| !s.is_empty()) {
            let key = (eid.to_string(), uri.to_string());
            if let Some(&existing) = self.named_hyperlinks.get(&key) {
                return existing;
            }
            let id = self.alloc_hyperlink_slot(uri);
            if self.named_hyperlinks.len() >= 8192 {
                self.named_hyperlinks.clear();
            }
            self.named_hyperlinks.insert(key, id);
            return id;
        }
        self.alloc_hyperlink_slot(uri)
    }

    fn alloc_hyperlink_slot(&mut self, uri: &str) -> NonZeroU32 {
        let id = self.next_hyperlink_id;
        // Saturate below `PLACEHOLDER_META_TAG` (bit 31, reserved for Kitty Unicode placeholder
        // tile metadata) rather than wrapping into 0 or colliding with placeholder tags.
        let next_raw = (id.get() + 1).clamp(1, PLACEHOLDER_META_TAG - 1);
        self.next_hyperlink_id = NonZeroU32::new(next_raw).unwrap_or(id);
        self.hyperlinks.insert(id, Arc::new(uri.to_string()));
        id
    }

    pub fn get_hyperlink(&self, id: NonZeroU32) -> Option<&str> {
        if (id.get() & PLACEHOLDER_META_TAG) != 0 {
            return None;
        }
        self.hyperlinks.get(&id).map(|s| s.as_str())
    }

    pub fn active(&self) -> &ScreenBuffer {
        if self.is_alternate {
            &self.alternate
        } else {
            &self.primary
        }
    }

    pub fn active_mut(&mut self) -> &mut ScreenBuffer {
        if self.is_alternate {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    /// Return the multi-codepoint grapheme cluster stored at active grid `(row, col)`, if any.
    #[inline]
    pub fn cell_grapheme(&self, row: usize, col: usize) -> Option<&str> {
        self.active().cell_grapheme(row, col)
    }

    /// Return the multi-codepoint grapheme cluster stored at active viewport `(screen_row, col)`, if any.
    #[inline]
    pub fn visible_cell_grapheme(&self, screen_row: usize, col: usize) -> Option<&str> {
        self.active().visible_cell_grapheme(screen_row, col)
    }

    pub fn set_alternate(&mut self, alt: bool) {
        self.is_alternate = alt;
    }

    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.primary.cursor.shape = shape;
        self.alternate.cursor.shape = shape;
    }

    pub fn set_cell_pixel_size(&mut self, cell_w: f32, cell_h: f32) {
        if cell_w > 0.0 && cell_h > 0.0 {
            self.cell_width_px = cell_w;
            self.cell_height_px = cell_h;
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.primary.resize(rows, cols);
        self.alternate.resize_no_reflow(rows, cols);
    }

    pub fn is_dirty(&self) -> bool {
        let active = self.active();
        active.images_dirty || active.grid.iter().any(|r| r.is_dirty)
    }

    pub fn clear_dirty(&mut self) {
        let active = self.active_mut();
        active.images_dirty = false;
        for row in &mut active.grid {
            row.is_dirty = false;
        }
    }

    /// Retrieve the URL at the given screen cell position (row, col).
    /// Checks explicit OSC 8 hyperlinks first, then falls back to plain-text URL detection.
    pub fn url_at(&self, row: usize, col: usize) -> Option<String> {
        let display_row = self.active().get_visible_row(row)?;

        // 1. Check explicit OSC 8 hyperlink on cell
        if let Some(cell) = display_row.cells.get(col) {
            if let Some(hid) = cell.hyperlink_id {
                if let Some(uri) = self.get_hyperlink(hid) {
                    return Some(uri.to_string());
                }
            }
        }

        // 2. Plain text auto-detection
        detect_url_in_row(&display_row.cells, col)
    }
}

/// Helper to detect URLs in a row of terminal cells at a target column.
///
/// Runs on every `CursorMoved` event, so it avoids allocating until it has
/// actually found a URL to return.
///
/// Everything here is indexed in **characters**, matching `cells`. An earlier
/// version built a `String` for the row and used `str::find`, which returns a
/// *byte* offset; feeding that back into the char-indexed array silently compared
/// the wrong boundary character on any row containing non-ASCII text, and panicked
/// outright once the byte offset exceeded the row's character count (roughly 30+
/// CJK characters ahead of a URL on an 80-column row).
pub fn detect_url_in_row(cells: &[Cell], target_col: usize) -> Option<String> {
    if target_col >= cells.len() {
        return None;
    }

    let target_char = cells[target_col].c;
    if target_char.is_whitespace() || target_char == '\0' {
        return None;
    }

    const PREFIXES: [&str; 8] = [
        "https://", "http://", "ftp://", "file://", "ssh://", "git://", "mailto:", "www.",
    ];

    let total_len = cells.len();

    // Every prefix contains either ':' or is "www.", so a row with neither cannot
    // hold a URL. One cheap pass rejects the vast majority of hovered rows before
    // any real work.
    let mut has_colon = false;
    let mut has_w = false;
    for cell in cells {
        match cell.c {
            ':' => has_colon = true,
            'w' => has_w = true,
            _ => {}
        }
    }
    if !has_colon && !has_w {
        return None;
    }

    // Case-sensitive prefix compare at a char offset, matching the previous
    // `str::find` behaviour.
    let prefix_matches_at = |start: usize, prefix: &str| -> Option<usize> {
        let plen = prefix.chars().count();
        if start + plen > total_len {
            return None;
        }
        prefix
            .chars()
            .zip(cells[start..start + plen].iter())
            .all(|(pc, cell)| cell.c == pc)
            .then_some(plen)
    };

    for start in 0..total_len {
        // Word boundary before the prefix.
        if start > 0 {
            let prev_c = cells[start - 1].c;
            if prev_c.is_alphanumeric() || prev_c == '/' || prev_c == '.' {
                continue;
            }
        }

        let Some(prefix_len) = PREFIXES.iter().find_map(|p| prefix_matches_at(start, p)) else {
            continue;
        };

        // Find end of URL.
        let mut end_idx = start + prefix_len;
        while end_idx < total_len {
            let c = cells[end_idx].c;
            if c.is_whitespace()
                || c == '\0'
                || c == '"'
                || c == '\''
                || c == '`'
                || c == '<'
                || c == '>'
                || c == '^'
                || c == '{'
                || c == '}'
                || c == '|'
            {
                break;
            }
            end_idx += 1;
        }

        // Trim trailing punctuation marks like '.', ',', ';', '!', '?'.
        while end_idx > start {
            let last_c = cells[end_idx - 1].c;
            let unbalanced_close = |open: char| !cells[start..end_idx].iter().any(|c| c.c == open);
            if matches!(last_c, '.' | ',' | ';' | '!' | '?')
                || (last_c == ')' && unbalanced_close('('))
                || (last_c == ']' && unbalanced_close('['))
            {
                end_idx -= 1;
            } else {
                break;
            }
        }

        if start <= target_col && target_col < end_idx {
            let url: String = cells[start..end_idx].iter().map(|c| c.c).collect();
            if url.starts_with("www.") {
                return Some(format!("https://{}", url));
            }
            return Some(url);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a row of `cols` cells whose leading characters come from `s`.
    fn row_from(s: &str, cols: usize) -> Vec<Cell> {
        let mut cells = vec![Cell::default(); cols];
        for (i, c) in s.chars().take(cols).enumerate() {
            cells[i].c = c;
        }
        cells
    }

    /// Write `s` into the buffer starting at (row, 0), like a program printing a line.
    fn put_line(buf: &mut ScreenBuffer, row: usize, s: &str) {
        for (i, c) in s.chars().enumerate() {
            if i >= buf.cols {
                break;
            }
            buf.grid[row].cells[i].c = c;
        }
    }

    // ---------------------------------------------------------------- URL detection

    #[test]
    fn url_detected_at_hovered_column() {
        let cells = row_from("see https://example.com now", 80);
        // Hover somewhere inside the URL.
        assert_eq!(
            detect_url_in_row(&cells, 10).as_deref(),
            Some("https://example.com")
        );
        // Hovering the leading word must not produce a URL.
        assert_eq!(detect_url_in_row(&cells, 1), None);
    }

    #[test]
    fn url_www_prefix_gets_scheme() {
        let cells = row_from("go to www.example.com", 40);
        assert_eq!(
            detect_url_in_row(&cells, 8).as_deref(),
            Some("https://www.example.com")
        );
    }

    #[test]
    fn url_trailing_punctuation_is_trimmed() {
        let cells = row_from("see https://example.com.", 40);
        assert_eq!(
            detect_url_in_row(&cells, 10).as_deref(),
            Some("https://example.com")
        );

        // A closing paren is kept when the URL itself opened one.
        let balanced = row_from("https://en.wikipedia.org/wiki/Foo_(bar)", 60);
        assert_eq!(
            detect_url_in_row(&balanced, 5).as_deref(),
            Some("https://en.wikipedia.org/wiki/Foo_(bar)")
        );

        // ...and dropped when it did not.
        let unbalanced = row_from("(https://example.com)", 40);
        assert_eq!(
            detect_url_in_row(&unbalanced, 5).as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn url_requires_word_boundary_before_prefix() {
        // The prefix is glued to preceding alphanumerics, so it is not a URL start.
        let cells = row_from("xhttps://example.com", 40);
        assert_eq!(detect_url_in_row(&cells, 12), None);
    }

    #[test]
    fn url_blank_and_out_of_range_columns_are_rejected() {
        let cells = row_from("https://example.com", 40);
        // Past the text, inside the blank padding.
        assert_eq!(detect_url_in_row(&cells, 30), None);
        // Past the row entirely.
        assert_eq!(detect_url_in_row(&cells, 999), None);
    }

    /// Regression: `detect_url_in_row` used to build a `String` for the row and use
    /// `str::find`, whose *byte* offset was then used to index the *char* array. With
    /// enough CJK ahead of the URL the byte offset exceeds the row's char count and
    /// the lookup panicked -- reachable just by moving the mouse over such a row.
    #[test]
    fn url_after_many_cjk_chars_does_not_panic() {
        let text = format!("{} https://example.com", "中".repeat(30));
        let cells = row_from(&text, 80);

        // 30 CJK chars + 1 space => the URL starts at char index 31.
        let url_start = 31;
        assert_eq!(cells[url_start].c, 'h', "test fixture is misaligned");

        assert_eq!(
            detect_url_in_row(&cells, url_start + 4).as_deref(),
            Some("https://example.com")
        );
    }

    #[test]
    fn url_detection_handles_cjk_without_url() {
        let cells = row_from(&"中文測試".repeat(10), 80);
        for col in 0..40 {
            assert_eq!(detect_url_in_row(&cells, col), None);
        }
    }

    // ---------------------------------------------------------------- search

    #[test]
    fn search_is_case_insensitive_and_ordered() {
        let mut buf = ScreenBuffer::new(3, 20, 100);
        put_line(&mut buf, 0, "Hello World");
        put_line(&mut buf, 1, "nothing here");
        put_line(&mut buf, 2, "hello again");

        let matches = buf.search("HELLO");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].global_row, 0);
        assert_eq!(matches[0].start_col, 0);
        assert_eq!(matches[0].end_col, 4);
        assert_eq!(matches[1].global_row, 2);
    }

    #[test]
    fn search_finds_multiple_matches_per_row() {
        let mut buf = ScreenBuffer::new(1, 30, 100);
        put_line(&mut buf, 0, "ab ab ab");

        let matches = buf.search("ab");
        assert_eq!(matches.len(), 3);
        assert_eq!(
            matches.iter().map(|m| m.start_col).collect::<Vec<_>>(),
            vec![0, 3, 6]
        );
    }

    #[test]
    fn search_empty_query_returns_nothing() {
        let mut buf = ScreenBuffer::new(1, 20, 100);
        put_line(&mut buf, 0, "content");
        assert!(buf.search("").is_empty());
        assert!(buf.search("   ").is_empty());
    }

    #[test]
    fn search_spans_scrollback_and_grid() {
        let mut buf = ScreenBuffer::new(2, 20, 100);
        put_line(&mut buf, 0, "needle one");
        put_line(&mut buf, 1, "filler");
        // Push the first line into scrollback.
        buf.scroll_up(1);
        put_line(&mut buf, 1, "needle two");

        let matches = buf.search("needle");
        assert_eq!(
            matches.len(),
            2,
            "expected one hit in scrollback and one in grid"
        );
        // Scrollback rows sort before grid rows.
        assert!(matches[0].global_row < matches[1].global_row);
    }

    #[test]
    fn search_does_not_match_in_blank_padding() {
        let mut buf = ScreenBuffer::new(1, 40, 100);
        put_line(&mut buf, 0, "a");
        // The row is padded with spaces; a space query is trimmed to empty.
        assert!(buf.search(" ").is_empty());
        assert_eq!(buf.search("a").len(), 1);
    }

    #[test]
    fn search_matches_non_ascii() {
        let mut buf = ScreenBuffer::new(1, 20, 100);
        put_line(&mut buf, 0, "中文測試");
        let matches = buf.search("文測");
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].start_col, 1);
        assert_eq!(matches[0].end_col, 2);
    }

    /// The renderer binary-searches this list per row (`partition_point` on
    /// `global_row`) so it only scans the handful of matches on the row it is
    /// drawing instead of all of them. That is only correct if the list is sorted
    /// by `global_row`, then by `start_col`. Pin the invariant here so a future
    /// rewrite of `search` cannot silently break highlighting.
    #[test]
    fn search_results_are_sorted_by_row_then_column() {
        let mut buf = ScreenBuffer::new(4, 40, 100);
        put_line(&mut buf, 0, "hit and hit again");
        put_line(&mut buf, 1, "nothing here");
        put_line(&mut buf, 2, "hit hit hit");
        // Push the first two lines into scrollback so both halves of the scan
        // contribute. This also slides "hit hit hit" from row 2 up to row 0.
        buf.scroll_up(2);
        put_line(&mut buf, 2, "hit once more");

        let matches = buf.search("hit");
        // 2 in scrollback + 3 on grid row 0 + 1 on grid row 2.
        assert_eq!(matches.len(), 6, "unexpected fixture: {matches:?}");
        assert!(
            matches
                .windows(2)
                .all(|w| (w[0].global_row, w[0].start_col) < (w[1].global_row, w[1].start_col)),
            "search results are not sorted: {matches:?}"
        );
    }

    // ---------------------------------------------------------------- resize / reflow

    /// Collect the visible grid as trimmed strings, for readable assertions.
    fn grid_lines(buf: &ScreenBuffer) -> Vec<String> {
        buf.grid
            .iter()
            .map(|r| {
                r.cells
                    .iter()
                    .map(|c| c.c)
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    #[test]
    fn resize_wider_preserves_content() {
        let mut buf = ScreenBuffer::new(3, 10, 100);
        put_line(&mut buf, 0, "hello");
        put_line(&mut buf, 1, "world");

        buf.resize(3, 20);

        assert_eq!(buf.cols, 20);
        assert_eq!(buf.rows, 3);
        let lines = grid_lines(&buf);
        assert!(lines.contains(&"hello".to_string()), "got {lines:?}");
        assert!(lines.contains(&"world".to_string()), "got {lines:?}");
        // Every row must be physically resized, not just logically.
        assert!(buf.grid.iter().all(|r| r.cells.len() == 20));
    }

    /// Narrowing wraps a visible line across continuation rows instead of cutting
    /// it off at the new width.
    #[test]
    fn narrowing_wraps_visible_lines_across_rows() {
        let mut buf = ScreenBuffer::new(3, 20, 100);
        put_line(&mut buf, 0, "abcdefghij");

        buf.resize(6, 5);

        assert_eq!(buf.cols, 5);
        assert!(buf.grid.iter().all(|r| r.cells.len() == 5));

        let row0: String = buf.grid[0].cells.iter().map(|c| c.c).collect();
        let row1: String = buf.grid[1].cells.iter().map(|c| c.c).collect();
        assert_eq!(row0, "abcde");
        assert_eq!(row1, "fghij");
        assert!(
            buf.grid[0].is_wrapped,
            "the first chunk must be marked as continuing"
        );
        assert!(
            !buf.grid[1].is_wrapped,
            "the last chunk ends the logical line"
        );
    }

    /// A line that always fits survives a narrow-then-widen roundtrip intact.
    #[test]
    fn resize_roundtrip_preserves_a_fitting_line() {
        let mut buf = ScreenBuffer::new(4, 20, 100);
        put_line(&mut buf, 0, "short");

        buf.resize(4, 10);
        buf.resize(4, 20);

        let row0: String = buf.grid[0].cells.iter().map(|c| c.c).collect();
        assert_eq!(row0.trim_end(), "short");
    }

    #[test]
    fn resize_keeps_cursor_in_bounds() {
        let mut buf = ScreenBuffer::new(10, 40, 100);
        buf.cursor.row = 9;
        buf.cursor.col = 39;

        buf.resize(4, 10);

        assert!(
            buf.cursor.row < buf.rows,
            "cursor row {} escaped {} rows",
            buf.cursor.row,
            buf.rows
        );
        assert!(
            buf.cursor.col <= buf.cols,
            "cursor col {} escaped {} cols",
            buf.cursor.col,
            buf.cols
        );
    }

    #[test]
    fn resize_to_one_column_does_not_panic() {
        let mut buf = ScreenBuffer::new(3, 20, 100);
        put_line(&mut buf, 0, "some text here");
        buf.resize(3, 1);
        assert_eq!(buf.cols, 1);
        assert!(buf.grid.iter().all(|r| r.cells.len() == 1));
    }

    /// Dragging a splitter back and forth must never panic, corrupt the geometry
    /// invariants, or lose text, however many intermediate widths we pass through.
    #[test]
    fn repeated_one_column_resizes_are_stable() {
        const TEXT: &str = "drag me across the divider";
        let mut buf = ScreenBuffer::new(5, 40, 1000);
        put_line(&mut buf, 0, TEXT);

        for cols in (10..40).rev() {
            buf.resize(5, cols);
        }
        for cols in 10..40 {
            buf.resize(5, cols);
        }

        assert_eq!(buf.cols, 39);
        assert_eq!(buf.grid.len(), 5, "grid must stay exactly `rows` tall");
        assert!(buf.grid.iter().all(|r| r.cells.len() == 39));
        assert!(buf.cursor.row < buf.rows);
        assert!(buf.cursor.col <= buf.cols);

        let all: String = buf
            .scrollback
            .iter()
            .chain(buf.grid.iter())
            .flat_map(|r| r.cells.iter().map(|c| c.c))
            .collect();
        assert!(
            all.replace(' ', "").contains(&TEXT.replace(' ', "")),
            "content lost after repeated resizes"
        );
    }

    /// Narrowing re-wraps a long visible line instead of truncating it.
    ///
    /// This used to drop everything past the new width. The overflow rows are now
    /// pushed into scrollback so the cursor keeps its row (see the anchoring block
    /// in `reflow`), which means nothing is destroyed.
    #[test]
    fn narrowing_rewraps_long_visible_lines_losslessly() {
        let mut buf = ScreenBuffer::new(10, 80, 1000);
        put_line(&mut buf, 0, &"A".repeat(60));

        buf.resize(10, 40);

        let preserved = buf
            .scrollback
            .iter()
            .chain(buf.grid.iter())
            .flat_map(|r| r.cells.iter())
            .filter(|c| c.c == 'A')
            .count();

        assert_eq!(
            preserved,
            60,
            "narrowing dropped {} of 60 characters",
            60 - preserved
        );
    }

    /// The reason the truncation existed in the first place: re-wrapping must not
    /// slide the cursor down the screen, because shells repaint relative to it
    /// after SIGWINCH.
    ///
    /// Here row 0 holds a 60-character line that becomes two rows at 40 columns.
    /// The cursor sits on row 1 and must stay on row 1; the displaced row belongs
    /// in scrollback.
    #[test]
    fn narrowing_anchors_the_cursor_row_by_scrolling_overflow_off() {
        let mut buf = ScreenBuffer::new(10, 80, 1000);
        put_line(&mut buf, 0, &"A".repeat(60));
        put_line(&mut buf, 1, "> ");
        buf.cursor.row = 1;
        buf.cursor.col = 2;

        buf.resize(10, 40);

        assert_eq!(buf.cursor.row, 1, "cursor row must survive the reflow");
        assert_eq!(buf.cursor.col, 2);
        assert_eq!(
            buf.scrollback.len(),
            1,
            "the displaced wrap row belongs in scrollback"
        );
        // The severed prefix must be a hard line, so a later widening cannot fuse it
        // onto whatever the shell repaints into the top grid row.
        assert!(!buf.scrollback[0].is_wrapped);
        // Still lossless.
        let preserved = buf
            .scrollback
            .iter()
            .chain(buf.grid.iter())
            .flat_map(|r| r.cells.iter())
            .filter(|c| c.c == 'A')
            .count();
        assert_eq!(preserved, 60);
    }

    /// The counterpart to [`narrowing_rewraps_long_visible_lines_losslessly`]: a
    /// line that already lives in scrollback *is* re-wrapped losslessly, because no
    /// cursor is anchored to it.
    #[test]
    fn narrowing_rewraps_scrollback_lines_losslessly() {
        let mut buf = ScreenBuffer::new(3, 80, 1000);
        put_line(&mut buf, 0, &"A".repeat(60));
        // Push the line out of the grid and into scrollback.
        for _ in 0..3 {
            buf.scroll_up(1);
        }
        assert!(!buf.scrollback.is_empty(), "line should be in scrollback");

        buf.resize(3, 40);

        let preserved = buf
            .scrollback
            .iter()
            .chain(buf.grid.iter())
            .flat_map(|r| r.cells.iter())
            .filter(|c| c.c == 'A')
            .count();

        assert_eq!(preserved, 60, "scrollback re-wrap must not lose characters");
    }

    #[test]
    fn resize_no_reflow_keeps_dimensions() {
        let mut buf = ScreenBuffer::new(5, 20, 0);
        put_line(&mut buf, 0, "alt screen");

        buf.resize_no_reflow(8, 30);

        assert_eq!(buf.rows, 8);
        assert_eq!(buf.cols, 30);
        assert_eq!(buf.grid.len(), 8);
        assert!(buf.grid.iter().all(|r| r.cells.len() == 30));
    }

    #[test]
    fn scrollback_is_capped_at_max() {
        let mut buf = ScreenBuffer::new(2, 10, 5);
        for i in 0..20 {
            put_line(&mut buf, 0, &format!("line{i}"));
            buf.scroll_up(1);
        }
        assert!(
            buf.scrollback.len() <= 5,
            "scrollback grew to {}",
            buf.scrollback.len()
        );
    }

    #[test]
    fn scroll_up_recycles_rows_without_growing_grid() {
        let mut buf = ScreenBuffer::new(4, 10, 2);
        for _ in 0..50 {
            buf.scroll_up(1);
        }
        assert_eq!(buf.grid.len(), 4, "grid must stay exactly `rows` tall");
        assert!(buf.grid.iter().all(|r| r.cells.len() == 10));
    }
    // ------------------------------------------------------- reflow stress / invariants

    /// Deterministic xorshift so failures are reproducible without a dev-dependency.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn in_range(&mut self, lo: usize, hi: usize) -> usize {
            lo + (self.next() as usize) % (hi - lo + 1)
        }
    }

    /// Reflow is the most intricate function in this file and is driven directly by
    /// mouse movement (splitter drags), so it sees far stranger sequences of sizes
    /// than any hand-written case. Hammer it and assert the structural invariants
    /// every caller depends on.
    #[test]
    fn reflow_survives_random_resize_sequences() {
        let mut rng = Rng(0x5eed_1234_9876_abcd);

        for round in 0..200 {
            let rows0 = rng.in_range(1, 24);
            let cols0 = rng.in_range(1, 60);
            let mut buf = ScreenBuffer::new(rows0, cols0, rng.in_range(0, 200));

            for r in 0..rows0 {
                let len = rng.in_range(0, cols0);
                if len > 0 {
                    let text: String = (0..len)
                        .map(|i| char::from(b'a' + ((i + r) % 26) as u8))
                        .collect();
                    put_line(&mut buf, r, &text);
                }
            }
            buf.cursor.row = rng.in_range(0, rows0 - 1);
            buf.cursor.col = rng.in_range(0, cols0.saturating_sub(1));

            for step in 0..20 {
                let rows = rng.in_range(1, 24);
                let cols = rng.in_range(1, 60);
                buf.resize(rows, cols);

                let ctx = format!("round {round} step {step} -> {rows}x{cols}");
                assert_eq!(buf.rows, rows, "{ctx}: rows field");
                assert_eq!(buf.cols, cols, "{ctx}: cols field");
                assert_eq!(buf.grid.len(), rows, "{ctx}: grid height");
                assert!(
                    buf.grid.iter().all(|r| r.cells.len() == cols),
                    "{ctx}: a grid row has the wrong width"
                );
                assert!(
                    buf.scrollback.iter().all(|r| r.cells.len() == cols),
                    "{ctx}: a scrollback row has the wrong width"
                );
                assert!(buf.cursor.row < rows, "{ctx}: cursor row escaped");
                assert!(buf.cursor.col <= cols, "{ctx}: cursor col escaped");
                assert!(
                    buf.scrollback.len() <= buf.max_scrollback,
                    "{ctx}: scrollback over cap"
                );
                assert_eq!(buf.scroll_region, (0, rows - 1), "{ctx}: scroll region");
            }
        }
    }

    /// With scrollback large enough that nothing can be evicted, no sequence of
    /// resizes may destroy a character. This is the invariant the old truncating
    /// fast path violated.
    #[test]
    fn reflow_never_loses_characters_when_scrollback_is_ample() {
        let mut rng = Rng(0xabcd_ef01_2345_6789);

        for round in 0..100 {
            let cols0 = rng.in_range(10, 60);
            let mut buf = ScreenBuffer::new(8, cols0, 100_000);

            // `put_line` clips at one row, so spread the marker over several
            // full-width unwrapped rows. Narrowing then has to wrap every one of
            // them -- exactly the case the old fast path truncated.
            let mut planted = 0;
            for r in 0..4 {
                let len = rng.in_range(1, cols0);
                put_line(&mut buf, r, &"Z".repeat(len));
                planted += len;
            }

            let count = |b: &ScreenBuffer| {
                b.scrollback
                    .iter()
                    .chain(b.grid.iter())
                    .flat_map(|r| r.cells.iter())
                    .filter(|c| c.c == 'Z')
                    .count()
            };
            let expected = count(&buf);
            assert_eq!(expected, planted, "fixture did not land as expected");

            for step in 0..15 {
                let cols = rng.in_range(1, 60);
                buf.resize(rng.in_range(1, 20), cols);
                assert_eq!(
                    count(&buf),
                    expected,
                    "round {round} step {step}: resize to {cols} cols lost characters"
                );
            }
        }
    }
}
