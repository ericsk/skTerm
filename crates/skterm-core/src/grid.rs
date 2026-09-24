//! Screen Buffer, Character Grid, Damage Tracking, Dual Buffer, Selection, and Terminal Graphics.

use crate::graphics::GraphicImage;
use bitflags::bitflags;
use std::collections::{HashMap, VecDeque};
use std::num::NonZeroU32;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

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
    }
}

/// Standard 16 ANSI colors
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
/// `Option<NonZeroU32>` is 4). Hyperlink ids are handed out starting from 1, so zero
/// is never a valid id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    pub hyperlink_id: Option<NonZeroU32>,
}

// Lock in the layout: 4 (char) + 4 (fg) + 4 (bg) + 2 (flags) + 4 (link) = 18, padded
// to 20 by the alignment-4 fields. If this ever fails, check what was added to `Cell`.
const _: () = assert!(std::mem::size_of::<Cell>() == 20);

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            flags: CellFlags::empty(),
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
            hyperlink_id: None,
        }
    }
}

/// A row of terminal cells with dirty tracking for rendering efficiency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub is_dirty: bool,
    pub is_wrapped: bool,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            is_dirty: true,
            is_wrapped: false,
        }
    }

    pub fn resize(&mut self, cols: usize) {
        self.cells.resize(cols, Cell::default());
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
                        || c.hyperlink_id.is_some()
                })
                .map(|idx| idx + 1)
                .unwrap_or(0)
        }
    }

    pub fn clear(&mut self, bg: Color) {
        self.cells.fill(Cell::blank(bg));
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

/// Terminal cursor state
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cursor {
    pub row: usize,
    pub col: usize,
    pub shape: CursorShape,
    pub visible: bool,
    pub pending_wrap: bool,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            row: 0,
            col: 0,
            shape: CursorShape::Block,
            visible: true,
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
    pub flags: CellFlags,
    pub shape: CursorShape,
    pub visible: bool,
    pub pending_wrap: bool,
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
}

impl ScreenBuffer {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        let grid = (0..rows).map(|_| Row::new(cols)).collect();
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
        }
    }

    pub fn add_image(&mut self, img: GraphicImage) {
        self.images.push(img);
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == 0 || new_cols == 0 {
            return;
        }

        if new_cols != self.cols {
            self.reflow(new_rows, new_cols);
        } else {
            self.resize_height_only(new_rows);
        }
    }

    /// Resize when only the number of rows changes (cols remains the same).
    pub fn resize_height_only(&mut self, new_rows: usize) {
        if new_rows > self.rows {
            let diff = new_rows - self.rows;
            for _ in 0..diff {
                if let Some(old_row) = self.scrollback.pop_back() {
                    self.grid.push_front(old_row);
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
                if self.max_scrollback > 0 {
                    if self.scrollback.len() >= self.max_scrollback {
                        self.scrollback.pop_front();
                    }
                    self.scrollback.push_back(removed);
                }
            }
        }

        self.rows = new_rows;
        self.scroll_region = (0, new_rows.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
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

        if new_rows > self.rows {
            let diff = new_rows - self.rows;
            for _ in 0..diff {
                if let Some(mut old_row) = self.scrollback.pop_back() {
                    old_row.resize(new_cols);
                    self.grid.push_front(old_row);
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
                if self.max_scrollback > 0 {
                    if self.scrollback.len() >= self.max_scrollback {
                        self.scrollback.pop_front();
                    }
                    self.scrollback.push_back(removed);
                }
            }
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.scroll_region = (0, new_rows.saturating_sub(1));
        self.cursor.row = self.cursor.row.min(new_rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(new_cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
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
            is_wrapped: bool,
            cursor_offset: Option<usize>,
            was_soft_wrapped: bool,
        }

        let mut logical_lines: Vec<LogicalLine> = Vec::new();
        let mut current_line: Option<LogicalLine> = None;
        let mut grid_start_logical: Option<(usize, usize)> = None;

        for abs_row in 0..=last_needed_row {
            if abs_row == total_scrollback {
                let line_idx = logical_lines.len();
                let cell_idx = current_line.as_ref().map(|l| l.cells.len()).unwrap_or(0);
                grid_start_logical = Some((line_idx, cell_idx));
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
            for (c_idx, cell) in row.cells[..len].iter().enumerate() {
                if has_cursor && c_idx == cursor_col {
                    cursor_offset_in_row = Some(row_cells.len());
                }
                if !cell.flags.contains(CellFlags::WRAP_SPACER) {
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
                cur.cells.extend(row_cells);
                cur.is_wrapped = row.is_wrapped;
                cur.was_soft_wrapped = true;

                if row.is_wrapped {
                    current_line = Some(cur);
                } else {
                    logical_lines.push(cur);
                }
            } else {
                let line = LogicalLine {
                    cells: row_cells,
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
                is_wrapped: false,
                cursor_offset: Some(0),
                was_soft_wrapped: false,
            });
        }

        let mut new_physical_rows: Vec<Row> = Vec::new();
        let mut new_cursor: Option<(usize, usize)> = None;
        let mut first_new_grid_row: Option<usize> = None;
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

            if total_cells == 0 {
                let row_idx = new_physical_rows.len();
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
                continue;
            }

            let in_grid = if let Some((g_line, _)) = grid_start_logical {
                line_idx >= g_line
            } else {
                false
            };

            // Fast path: a line that already fits in the new width keeps its own
            // single physical row.
            //
            // Lines that do *not* fit fall through to the wrapping loop below. That
            // used to truncate here instead, to stop the extra continuation rows from
            // pushing the cursor down the screen -- a shell repainting relative to the
            // cursor after SIGWINCH (`ESC[1A`) would then write to the wrong line.
            // The cursor is now anchored by pushing the overflow into scrollback
            // during the split at the end of this function, so wrapping is safe and
            // nothing is destroyed. See `anchor` there, and
            // `test_vt_prompt_line_preservation_on_resize`.
            if in_grid && !line.was_soft_wrapped && total_cells <= new_cols {
                let row_idx = new_physical_rows.len();
                if first_new_grid_row.is_none() {
                    first_new_grid_row = Some(row_idx);
                }

                let mut row = Row::new(new_cols);
                let copy_len = total_cells.min(new_cols);
                for c in 0..copy_len {
                    row.cells[c] = line.cells[c];
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
                // Count only the continuation rows this line inserted *above* the
                // cursor; rows below it do not move the cursor.
                let extra = match new_cursor {
                    // The cursor landed on this line: everything before its chunk.
                    Some((cursor_row, _)) if cursor_row >= rows_before_line => {
                        cursor_row - rows_before_line
                    }
                    // The cursor is still ahead of us, so this whole line sits above
                    // it. One row is the line's own; the rest are new.
                    _ => new_physical_rows.len() - rows_before_line - 1,
                };
                anchor_debt += extra;
            }
        }

        let total_new = new_physical_rows.len();
        let (cursor_row_abs, cursor_col_final) =
            new_cursor.unwrap_or((total_new.saturating_sub(1), 0));
        let natural_first_grid_row = first_new_grid_row.unwrap_or(0).min(cursor_row_abs);

        // Cursor anchoring.
        //
        // Re-wrapping a line that no longer fits creates extra physical rows. Where
        // those rows land above the cursor, the cursor slides down the screen -- and a
        // shell that repaints relative to the cursor after SIGWINCH (`ESC[1A`) then
        // writes to the wrong line and corrupts the display. That is why this function
        // used to truncate such lines outright.
        //
        // Instead of dropping the overflow, scroll it off the top: start the grid
        // `anchor_debt` rows further down, so the cursor keeps the row it had and the
        // displaced rows become scrollback. Nothing is lost -- the user can scroll up
        // to see it -- and the cursor stays where the shell expects.
        //
        // `anchor_debt` counts only rows introduced by re-wrapping a previously
        // unwrapped visible line. Ordinary re-wrapping of already-wrapped lines is
        // untouched, so the cursor still tracks its logical line the way it always did.
        let first_grid_row = (natural_first_grid_row + anchor_debt).min(cursor_row_abs);

        if first_grid_row > natural_first_grid_row {
            // Sever the soft-wrap link across the new scrollback/grid boundary.
            //
            // The row we just pushed off the top may be flagged as continuing into the
            // row that is now the top of the grid. Left alone, a later widening would
            // coalesce the two back into one logical line -- but by then the shell has
            // repainted the visible row in response to SIGWINCH, so the join would fuse
            // the stale prefix onto unrelated new content. Declaring the split line
            // hard keeps the resize idempotent.
            //
            // This only affects rows displaced by the anchoring above. Rows that reach
            // scrollback by ordinary scrolling keep their flag and still re-join
            // correctly.
            new_physical_rows[first_grid_row - 1].is_wrapped = false;
        }

        let grid_content_count = total_new.saturating_sub(first_grid_row);

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

        self.rows = new_rows;
        self.cols = new_cols;
        self.scroll_region = (0, new_rows.saturating_sub(1));
        self.cursor.pending_wrap = false;
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
                // If selection starts on the trailing spacer cell of a wide CJK character,
                // include the leading character cell so the first character isn't dropped.
                if col_start > 0
                    && col_start < row.cells.len()
                    && row.cells[col_start]
                        .flags
                        .contains(CellFlags::WIDE_CHAR_SPACER)
                {
                    let prev = &row.cells[col_start - 1];
                    if !prev
                        .flags
                        .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::WRAP_SPACER)
                    {
                        line_str.push(prev.c);
                    }
                }

                for c_idx in col_start..=col_end {
                    if let Some(cell) = row.cells.get(c_idx) {
                        if !cell
                            .flags
                            .intersects(CellFlags::WIDE_CHAR_SPACER | CellFlags::WRAP_SPACER)
                        {
                            line_str.push(cell.c);
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
    }

    pub fn move_up(&mut self, n: usize) {
        let (top, _) = self.scroll_region;
        if self.cursor.row >= top {
            self.cursor.row = self.cursor.row.saturating_sub(n).max(top);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(n);
        }
        self.cursor.pending_wrap = false;
    }

    pub fn move_down(&mut self, n: usize) {
        let (_, bottom) = self.scroll_region;
        if self.cursor.row <= bottom {
            self.cursor.row = (self.cursor.row + n).min(bottom);
        } else {
            self.cursor.row = (self.cursor.row + n).min(self.rows.saturating_sub(1));
        }
        self.cursor.pending_wrap = false;
    }

    pub fn move_forward(&mut self, n: usize) {
        self.cursor.col = (self.cursor.col + n).min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
    }

    pub fn move_backward(&mut self, n: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(n);
        self.cursor.pending_wrap = false;
    }

    pub fn linefeed(&mut self) {
        let (_, bottom) = self.scroll_region;
        if self.cursor.row == bottom {
            self.scroll_up(1);
        } else if self.cursor.row < self.rows.saturating_sub(1) {
            self.cursor.row += 1;
        }
        self.cursor.pending_wrap = false;
    }

    pub fn newline(&mut self) {
        self.linefeed();
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    pub fn scroll_up(&mut self, n: usize) {
        let (top, bottom) = self.scroll_region;
        let full_screen = top == 0 && bottom == self.rows.saturating_sub(1);
        let cols = self.cols;

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
                blank.reuse_as_blank(cols, Color::DefaultBg);
                self.grid.push_back(blank);
            } else {
                let Some(mut row) = self.grid.remove(top) else {
                    break;
                };
                row.reuse_as_blank(cols, Color::DefaultBg);
                self.grid.insert(bottom, row);
            }
        }

        for img in &mut self.images {
            img.row = img.row.saturating_sub(n);
        }
        self.images.retain(|img| img.row < self.rows + 100);
    }

    pub fn scroll_down(&mut self, n: usize) {
        let (top, bottom) = self.scroll_region;
        let cols = self.cols;
        for _ in 0..n {
            let Some(mut row) = self.grid.remove(bottom) else {
                break;
            };
            row.reuse_as_blank(cols, Color::DefaultBg);
            self.grid.insert(top, row);
        }

        for img in &mut self.images {
            img.row = img.row.saturating_add(n);
        }
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
            }
            self.grid[row].cells[start_col]
                .flags
                .remove(CellFlags::WIDE_CHAR_SPACER);
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
            }
            self.grid[row].cells[end_col].c = ' ';
            self.grid[row].cells[end_col]
                .flags
                .remove(CellFlags::WIDE_CHAR_SPACER);
        }
    }

    pub fn write_char(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        hyperlink_id: Option<NonZeroU32>,
    ) {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(1);
        if char_width == 0 {
            return;
        }

        if self.cursor.pending_wrap {
            self.grid[self.cursor.row].is_wrapped = true;
            self.newline();
        }

        if char_width == 2 && self.cursor.col + 1 >= self.cols {
            self.clear_split_wide_chars(self.cursor.row, self.cursor.col, self.cols);
            self.grid[self.cursor.row].cells[self.cursor.col].reset();
            self.grid[self.cursor.row].cells[self.cursor.col]
                .flags
                .insert(CellFlags::WRAP_SPACER);
            self.grid[self.cursor.row].is_wrapped = true;
            self.newline();
        }

        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;
        let cols = self.cols;

        self.clear_split_wide_chars(cur_row, cur_col, cur_col + char_width);

        let mut actual_flags = flags;
        if char_width == 2 {
            actual_flags.insert(CellFlags::WIDE_CHAR);
        }

        // Resolve the row once: this runs per printed character, and every
        // `self.grid[..]` is an index computation on the deque.
        let row = &mut self.grid[cur_row];
        let cell = &mut row.cells[cur_col];
        cell.c = c;
        cell.fg = fg;
        cell.bg = bg;
        cell.flags = actual_flags;
        cell.hyperlink_id = hyperlink_id;
        row.is_dirty = true;

        if char_width == 2 && cur_col + 1 < cols {
            let spacer_cell = &mut row.cells[cur_col + 1];
            spacer_cell.c = ' ';
            spacer_cell.fg = fg;
            spacer_cell.bg = bg;
            spacer_cell.flags = flags | CellFlags::WIDE_CHAR_SPACER;
            spacer_cell.hyperlink_id = hyperlink_id;
            self.cursor.col += 1;
        }

        if self.cursor.col + 1 >= self.cols {
            self.cursor.pending_wrap = true;
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
        row.is_dirty = true;
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
        // A wide char pushed half-off the right edge leaves a dangling lead cell.
        if row.cells[cols - 1].flags.contains(CellFlags::WIDE_CHAR) {
            row.cells[cols - 1] = Cell::blank(bg);
        }
        row.is_dirty = true;
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
        row.is_dirty = true;
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
        for _ in 0..count {
            // The row pushed out of the bottom of the region becomes the blank row
            // inserted at the top, so no allocation is needed.
            let Some(mut row) = self.grid.remove(bottom) else {
                break;
            };
            row.reuse_as_blank(cols, bg);
            self.grid.insert(cur_row, row);
        }
        self.cursor.pending_wrap = false;
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
        for _ in 0..count {
            let Some(mut row) = self.grid.remove(cur_row) else {
                break;
            };
            row.reuse_as_blank(cols, bg);
            self.grid.insert(bottom, row);
        }
        self.cursor.pending_wrap = false;
    }

    pub fn erase_in_display(&mut self, mode: usize, bg: Color) {
        match mode {
            0 => {
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                self.clear_split_wide_chars(cur_row, cur_col, self.cols);
                let row = &mut self.grid[cur_row];
                row.cells[cur_col..].fill(Cell::blank(bg));
                row.is_dirty = true;
                for r in (cur_row + 1)..self.rows {
                    self.grid[r].clear(bg);
                }
            }
            1 => {
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                for r in 0..cur_row {
                    self.grid[r].clear(bg);
                }
                let end = cur_col.min(self.cols.saturating_sub(1));
                self.clear_split_wide_chars(cur_row, 0, end + 1);
                let row = &mut self.grid[cur_row];
                row.cells[..=end].fill(Cell::blank(bg));
                row.is_dirty = true;
            }
            2 | 3 => {
                for r in 0..self.rows {
                    self.grid[r].clear(bg);
                }
                if mode == 3 {
                    self.scrollback.clear();
                    self.images.clear();
                }
                self.cursor.pending_wrap = false;
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
                row.is_wrapped = false;
                row.is_dirty = true;
                self.cursor.pending_wrap = false;
            }
            1 => {
                let end = cur_col.min(self.cols.saturating_sub(1));
                self.clear_split_wide_chars(cur_row, 0, end + 1);
                let row = &mut self.grid[cur_row];
                row.cells[..=end].fill(Cell::blank(bg));
                row.is_dirty = true;
            }
            2 => {
                self.grid[cur_row].clear(bg);
                self.cursor.pending_wrap = false;
            }
            _ => {}
        }
    }

    /// Clear both visible screen rows and scrollback history (`Cmd+K` behavior).
    pub fn clear_scrollback_and_screen(&mut self) {
        self.scrollback.clear();
        self.images.clear();
        self.scroll_offset = 0;
        self.selection = None;
        for r in 0..self.rows {
            self.grid[r].clear(Color::DefaultBg);
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
    pub next_image_id: u32,
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
            next_image_id: 1,
        }
    }

    pub fn register_hyperlink(&mut self, uri: &str) -> NonZeroU32 {
        let id = self.next_hyperlink_id;
        // Saturate rather than wrap. Wrapping would eventually hand out 0, which is
        // reserved as the `None` niche in `Cell::hyperlink_id`. Note that ids are not
        // interned today, so a shell emitting the same URI repeatedly grows this table;
        // saturating merely makes the degenerate case safe, not cheap.
        self.next_hyperlink_id = self.next_hyperlink_id.checked_add(1).unwrap_or(id);
        self.hyperlinks.insert(id, Arc::new(uri.to_string()));
        id
    }

    pub fn get_hyperlink(&self, id: NonZeroU32) -> Option<&str> {
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

    pub fn set_alternate(&mut self, alt: bool) {
        self.is_alternate = alt;
    }

    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.primary.cursor.shape = shape;
        self.alternate.cursor.shape = shape;
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.primary.resize(rows, cols);
        self.alternate.resize_no_reflow(rows, cols);
    }

    pub fn is_dirty(&self) -> bool {
        self.active().grid.iter().any(|r| r.is_dirty)
    }

    pub fn clear_dirty(&mut self) {
        for row in &mut self.active_mut().grid {
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
