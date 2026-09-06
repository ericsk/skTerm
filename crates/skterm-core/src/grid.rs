//! Screen Buffer, Character Grid, Damage Tracking, Dual Buffer, Selection, and Terminal Graphics.

use crate::graphics::GraphicImage;
use bitflags::bitflags;
use std::collections::{HashMap, VecDeque};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    pub hyperlink_id: Option<u32>,
}

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
        for cell in &mut self.cells {
            cell.c = ' ';
            cell.fg = Color::DefaultFg;
            cell.bg = bg;
            cell.flags = CellFlags::empty();
            cell.hyperlink_id = None;
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
    pub grid: Vec<Row>,
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
                    self.grid.insert(0, old_row);
                } else {
                    self.grid.push(Row::new(self.cols));
                }
            }
        } else if new_rows < self.rows {
            let diff = self.rows - new_rows;
            for _ in 0..diff {
                let removed = self.grid.remove(0);
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
                    self.grid.insert(0, old_row);
                } else {
                    self.grid.push(Row::new(new_cols));
                }
            }
        } else if new_rows < self.rows {
            let diff = self.rows - new_rows;
            for _ in 0..diff {
                let removed = self.grid.remove(0);
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

            if in_grid && !line.was_soft_wrapped {
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
                if line.cursor_offset.is_some() && new_cursor.is_none() {
                    let cur_c = line.cursor_offset.unwrap().min(new_cols.saturating_sub(1));
                    new_cursor = Some((row_idx, cur_c));
                }
                row.is_wrapped = false;
                new_physical_rows.push(row);
                continue;
            }

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
        }

        let total_new = new_physical_rows.len();
        let (cursor_row_abs, cursor_col_final) =
            new_cursor.unwrap_or((total_new.saturating_sub(1), 0));
        let first_grid_row = first_new_grid_row.unwrap_or(0).min(cursor_row_abs);
        let grid_content_count = total_new.saturating_sub(first_grid_row);

        if grid_content_count <= new_rows {
            let mut scrollback_rows: VecDeque<Row> =
                new_physical_rows.drain(..first_grid_row).collect();
            self.grid = new_physical_rows;
            while self.grid.len() < new_rows {
                self.grid.push(Row::new(new_cols));
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
            self.grid = new_physical_rows;

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
        if delta < 0 {
            self.scroll_offset = self
                .scroll_offset
                .saturating_add((-delta) as usize)
                .min(max_scroll);
        } else {
            self.scroll_offset = self.scroll_offset.saturating_sub(delta as usize);
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
                let col_start = if r_idx == start.0 { start.1 } else { 0 };
                let col_end = if r_idx == end.0 {
                    end.1.min(row.cells.len().saturating_sub(1))
                } else {
                    row.cells.len().saturating_sub(1)
                };

                let mut line_str = String::new();
                for c_idx in col_start..=col_end {
                    if let Some(cell) = row.cells.get(c_idx) {
                        if !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                            line_str.push(cell.c);
                        }
                    }
                }

                result.push_str(line_str.trim_end());
                if r_idx < end.0 && !row.is_wrapped {
                    result.push('\n');
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
        for _ in 0..n {
            if top == 0 && bottom == self.rows.saturating_sub(1) {
                let removed = self.grid.remove(0);
                if self.max_scrollback > 0 {
                    if self.scrollback.len() >= self.max_scrollback {
                        self.scrollback.pop_front();
                    }
                    self.scrollback.push_back(removed);
                }
                self.grid.push(Row::new(self.cols));
            } else {
                self.grid.remove(top);
                self.grid.insert(bottom, Row::new(self.cols));
            }
        }

        for img in &mut self.images {
            img.row = img.row.saturating_sub(n);
        }
        self.images.retain(|img| img.row < self.rows + 100);
    }

    pub fn scroll_down(&mut self, n: usize) {
        let (top, bottom) = self.scroll_region;
        for _ in 0..n {
            self.grid.remove(bottom);
            self.grid.insert(top, Row::new(self.cols));
        }

        for img in &mut self.images {
            img.row = img.row.saturating_add(n);
        }
    }

    pub fn write_char(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        hyperlink_id: Option<u32>,
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
            self.grid[self.cursor.row].cells[self.cursor.col].reset();
            self.grid[self.cursor.row].cells[self.cursor.col]
                .flags
                .insert(CellFlags::WRAP_SPACER);
            self.grid[self.cursor.row].is_wrapped = true;
            self.newline();
        }

        let cur_row = self.cursor.row;
        let cur_col = self.cursor.col;

        let mut actual_flags = flags;
        if char_width == 2 {
            actual_flags.insert(CellFlags::WIDE_CHAR);
        }

        let cell = &mut self.grid[cur_row].cells[cur_col];
        cell.c = c;
        cell.fg = fg;
        cell.bg = bg;
        cell.flags = actual_flags;
        cell.hyperlink_id = hyperlink_id;
        self.grid[cur_row].is_dirty = true;

        if char_width == 2 && cur_col + 1 < self.cols {
            let spacer_cell = &mut self.grid[cur_row].cells[cur_col + 1];
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

    pub fn erase_in_display(&mut self, mode: usize, bg: Color) {
        match mode {
            0 => {
                let cur_row = self.cursor.row;
                let cur_col = self.cursor.col;
                for c in cur_col..self.cols {
                    self.grid[cur_row].cells[c].reset();
                    self.grid[cur_row].cells[c].bg = bg;
                }
                self.grid[cur_row].is_dirty = true;
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
                for c in 0..=cur_col.min(self.cols.saturating_sub(1)) {
                    self.grid[cur_row].cells[c].reset();
                    self.grid[cur_row].cells[c].bg = bg;
                }
                self.grid[cur_row].is_dirty = true;
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
                for c in cur_col..self.cols {
                    self.grid[cur_row].cells[c].reset();
                    self.grid[cur_row].cells[c].bg = bg;
                }
                self.grid[cur_row].is_dirty = true;
            }
            1 => {
                for c in 0..=cur_col.min(self.cols.saturating_sub(1)) {
                    self.grid[cur_row].cells[c].reset();
                    self.grid[cur_row].cells[c].bg = bg;
                }
                self.grid[cur_row].is_dirty = true;
            }
            2 => {
                self.grid[cur_row].clear(bg);
            }
            _ => {}
        }
    }
}

/// Screen Grid containing dual buffers (Primary with Scrollback + Alternate Screen).
#[derive(Debug, Clone)]
pub struct ScreenGrid {
    pub primary: ScreenBuffer,
    pub alternate: ScreenBuffer,
    pub is_alternate: bool,
    pub next_hyperlink_id: u32,
    pub hyperlinks: HashMap<u32, Arc<String>>,
    pub next_image_id: u32,
}

impl ScreenGrid {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            primary: ScreenBuffer::new(rows, cols, max_scrollback),
            alternate: ScreenBuffer::new(rows, cols, 0),
            is_alternate: false,
            next_hyperlink_id: 1,
            hyperlinks: HashMap::new(),
            next_image_id: 1,
        }
    }

    pub fn register_hyperlink(&mut self, uri: &str) -> u32 {
        let id = self.next_hyperlink_id;
        self.next_hyperlink_id += 1;
        self.hyperlinks.insert(id, Arc::new(uri.to_string()));
        id
    }

    pub fn get_hyperlink(&self, id: u32) -> Option<&str> {
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
pub fn detect_url_in_row(cells: &[Cell], target_col: usize) -> Option<String> {
    if target_col >= cells.len() {
        return None;
    }

    let target_char = cells[target_col].c;
    if target_char.is_whitespace() || target_char == '\0' {
        return None;
    }

    let chars: Vec<char> = cells.iter().map(|c| c.c).collect();
    let total_len = chars.len();
    let row_str: String = chars.iter().collect();

    let prefixes = [
        "https://", "http://", "ftp://", "file://", "ssh://", "git://", "mailto:", "www.",
    ];

    for prefix in &prefixes {
        let mut search_from = 0;
        while let Some(start_idx) = row_str[search_from..].find(prefix) {
            let actual_start = search_from + start_idx;
            // Check word boundary before the prefix
            if actual_start > 0 {
                let prev_c = chars[actual_start - 1];
                if prev_c.is_alphanumeric() || prev_c == '/' || prev_c == '.' {
                    search_from = actual_start + prefix.len();
                    continue;
                }
            }

            // Find end of URL
            let mut end_idx = actual_start + prefix.len();
            while end_idx < total_len {
                let c = chars[end_idx];
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

            // Trim trailing punctuation marks like '.', ',', ';', '!', '?'
            while end_idx > actual_start {
                let last_c = chars[end_idx - 1];
                if matches!(last_c, '.' | ',' | ';' | '!' | '?')
                    || (last_c == ')' && !chars[actual_start..end_idx].contains(&'('))
                    || (last_c == ']' && !chars[actual_start..end_idx].contains(&'['))
                {
                    end_idx -= 1;
                } else {
                    break;
                }
            }

            if actual_start <= target_col && target_col < end_idx {
                let mut url: String = chars[actual_start..end_idx].iter().collect();
                if url.starts_with("www.") {
                    url = format!("https://{}", url);
                }
                return Some(url);
            }

            search_from = actual_start + prefix.len();
        }
    }

    None
}
