//! Screen Grid buffer, Cell attributes, Rows, Cursor, Selection, and Scrollback management.

use bitflags::bitflags;
use std::collections::VecDeque;
use std::sync::Arc;
use unicode_width::UnicodeWidthChar;

/// Named ANSI 16 colors
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NamedColor {
    Black = 0,
    Red = 1,
    Green = 2,
    Yellow = 3,
    Blue = 4,
    Magenta = 5,
    Cyan = 6,
    White = 7,
    BrightBlack = 8,
    BrightRed = 9,
    BrightGreen = 10,
    BrightYellow = 11,
    BrightBlue = 12,
    BrightMagenta = 13,
    BrightCyan = 14,
    BrightWhite = 15,
}

/// Color representation supporting default, 16 named, 256 indexed, and 24-bit TrueColor RGB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Color {
    DefaultFg,
    DefaultBg,
    Named(NamedColor),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

bitflags! {
    /// Bitflags representing text styles and cell attributes.
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
        const WRAPPED           = 1 << 10;
    }
}

/// A single cell on the terminal grid.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub c: char,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    pub hyperlink: Option<Arc<String>>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            c: ' ',
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            flags: CellFlags::empty(),
            hyperlink: None,
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
            hyperlink: None,
        }
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

/// A single row containing a fixed number of cells and metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub cells: Vec<Cell>,
    pub is_wrapped: bool,
    pub is_dirty: bool,
}

impl Row {
    pub fn new(cols: usize) -> Self {
        Self {
            cells: vec![Cell::default(); cols],
            is_wrapped: false,
            is_dirty: true,
        }
    }

    pub fn resize(&mut self, cols: usize) {
        if cols > self.cells.len() {
            self.cells.resize(cols, Cell::default());
        } else if cols < self.cells.len() {
            self.cells.truncate(cols);
        }
        self.is_dirty = true;
    }

    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            cell.reset();
        }
        self.is_wrapped = false;
        self.is_dirty = true;
    }

    pub fn mark_dirty(&mut self) {
        self.is_dirty = true;
    }

    pub fn reset_dirty(&mut self) {
        self.is_dirty = false;
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

/// State of the cursor
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

/// Saved cursor state for ESC 7 / ESC 8 / CSI s / CSI u
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

/// A single screen buffer containing grid rows, scrollback history, and cursor state.
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
    pub scroll_offset: usize, // 0 = at bottom, > 0 = viewing history
}

impl ScreenBuffer {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        let grid = (0..rows).map(|_| Row::new(cols)).collect();
        Self {
            rows,
            cols,
            grid,
            scrollback: VecDeque::with_capacity(max_scrollback),
            max_scrollback,
            cursor: Cursor::default(),
            saved_cursor: None,
            scroll_region: (0, rows.saturating_sub(1)),
            selection: None,
            scroll_offset: 0,
        }
    }

    pub fn resize(&mut self, new_rows: usize, new_cols: usize) {
        if new_rows == 0 || new_cols == 0 {
            return;
        }

        for row in &mut self.grid {
            row.resize(new_cols);
        }
        for row in &mut self.scrollback {
            row.resize(new_cols);
        }

        if new_rows > self.rows {
            for _ in self.rows..new_rows {
                self.grid.push(Row::new(new_cols));
            }
        } else if new_rows < self.rows {
            let diff = self.rows - new_rows;
            for _ in 0..diff {
                if !self.grid.is_empty() {
                    let removed = self.grid.remove(0);
                    if self.max_scrollback > 0 {
                        if self.scrollback.len() >= self.max_scrollback {
                            self.scrollback.pop_front();
                        }
                        self.scrollback.push_back(removed);
                    }
                }
            }
        }

        self.rows = new_rows;
        self.cols = new_cols;
        self.scroll_region = (0, self.rows.saturating_sub(1));

        self.cursor.row = self.cursor.row.min(self.rows.saturating_sub(1));
        self.cursor.col = self.cursor.col.min(self.cols.saturating_sub(1));
        self.cursor.pending_wrap = false;
        self.selection = None;
    }

    pub fn scroll_display(&mut self, delta: isize) {
        if delta < 0 {
            // Scroll down towards bottom
            let abs_delta = (-delta) as usize;
            self.scroll_offset = self.scroll_offset.saturating_sub(abs_delta);
        } else {
            // Scroll up into history
            let max_offset = self.scrollback.len();
            self.scroll_offset = (self.scroll_offset + delta as usize).min(max_offset);
        }
    }

    /// Retrieve the row that should be displayed at visible line index `r` (0..rows).
    pub fn get_visible_row(&self, r: usize) -> Option<&Row> {
        if self.scroll_offset == 0 {
            self.grid.get(r)
        } else {
            let total_sb = self.scrollback.len();
            let start_sb_idx = total_sb.saturating_sub(self.scroll_offset);
            if start_sb_idx + r < total_sb {
                self.scrollback.get(start_sb_idx + r)
            } else {
                let grid_idx = (start_sb_idx + r) - total_sb;
                self.grid.get(grid_idx)
            }
        }
    }

    /// Extract text from active selection.
    pub fn extract_selected_text(&self) -> String {
        let Some(sel) = &self.selection else {
            return String::new();
        };

        let (start, end) = sel.normalized();
        let mut result = String::new();

        for r in start.0..=end.0 {
            if let Some(row) = self.grid.get(r) {
                let start_col = if r == start.0 { start.1 } else { 0 };
                let end_col = if r == end.0 { end.1 } else { self.cols.saturating_sub(1) };

                let mut row_str = String::new();
                for c in start_col..=end_col.min(row.cells.len().saturating_sub(1)) {
                    let cell = &row.cells[c];
                    if !cell.flags.contains(CellFlags::WIDE_CHAR_SPACER) {
                        row_str.push(cell.c);
                    }
                }

                if r < end.0 && !row.is_wrapped {
                    result.push_str(row_str.trim_end());
                    result.push('\n');
                } else {
                    result.push_str(&row_str);
                }
            }
        }

        result
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let top = top.min(self.rows.saturating_sub(1));
        let bottom = bottom.min(self.rows.saturating_sub(1));
        if top < bottom {
            self.scroll_region = (top, bottom);
        } else {
            self.scroll_region = (0, self.rows.saturating_sub(1));
        }
    }

    pub fn scroll_up(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region;
        if top >= bottom || top >= self.rows || bottom >= self.rows {
            return;
        }

        for _ in 0..count {
            if top == 0 && bottom == self.rows.saturating_sub(1) && self.max_scrollback > 0 {
                let removed = self.grid.remove(0);
                if self.scrollback.len() >= self.max_scrollback {
                    self.scrollback.pop_front();
                }
                self.scrollback.push_back(removed);
                self.grid.insert(bottom, Row::new(self.cols));
            } else {
                self.grid.remove(top);
                self.grid.insert(bottom, Row::new(self.cols));
            }
        }

        for r in top..=bottom {
            if r < self.grid.len() {
                self.grid[r].mark_dirty();
            }
        }
    }

    pub fn scroll_down(&mut self, count: usize) {
        let (top, bottom) = self.scroll_region;
        if top >= bottom || top >= self.rows || bottom >= self.rows {
            return;
        }

        for _ in 0..count {
            self.grid.remove(bottom);
            self.grid.insert(top, Row::new(self.cols));
        }

        for r in top..=bottom {
            if r < self.grid.len() {
                self.grid[r].mark_dirty();
            }
        }
    }

    pub fn insert_lines(&mut self, count: usize) {
        let cur_row = self.cursor.row;
        let (top, bottom) = self.scroll_region;
        if cur_row < top || cur_row > bottom {
            return;
        }

        for _ in 0..count {
            self.grid.remove(bottom);
            self.grid.insert(cur_row, Row::new(self.cols));
        }

        for r in cur_row..=bottom {
            if r < self.grid.len() {
                self.grid[r].mark_dirty();
            }
        }
    }

    pub fn delete_lines(&mut self, count: usize) {
        let cur_row = self.cursor.row;
        let (top, bottom) = self.scroll_region;
        if cur_row < top || cur_row > bottom {
            return;
        }

        for _ in 0..count {
            self.grid.remove(cur_row);
            self.grid.insert(bottom, Row::new(self.cols));
        }

        for r in cur_row..=bottom {
            if r < self.grid.len() {
                self.grid[r].mark_dirty();
            }
        }
    }

    pub fn insert_chars(&mut self, count: usize) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        if r < self.grid.len() {
            let row = &mut self.grid[r];
            for _ in 0..count {
                if c < row.cells.len() {
                    row.cells.pop();
                    row.cells.insert(c, Cell::default());
                }
            }
            row.mark_dirty();
        }
    }

    pub fn delete_chars(&mut self, count: usize) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        if r < self.grid.len() {
            let row = &mut self.grid[r];
            for _ in 0..count {
                if c < row.cells.len() {
                    row.cells.remove(c);
                    row.cells.push(Cell::default());
                }
            }
            row.mark_dirty();
        }
    }

    pub fn erase_chars(&mut self, count: usize) {
        let r = self.cursor.row;
        let start_col = self.cursor.col;
        if r < self.grid.len() {
            let row = &mut self.grid[r];
            let end_col = (start_col + count).min(row.cells.len());
            for c in start_col..end_col {
                row.cells[c].reset();
            }
            row.mark_dirty();
        }
    }

    pub fn erase_in_line(&mut self, mode: u16) {
        let r = self.cursor.row;
        let c = self.cursor.col;
        if r >= self.grid.len() {
            return;
        }

        let row = &mut self.grid[r];
        match mode {
            0 => {
                for col in c..row.cells.len() {
                    row.cells[col].reset();
                }
            }
            1 => {
                let end = (c + 1).min(row.cells.len());
                for col in 0..end {
                    row.cells[col].reset();
                }
            }
            2 => {
                row.clear();
            }
            _ => {}
        }
        row.mark_dirty();
    }

    pub fn erase_in_display(&mut self, mode: u16) {
        match mode {
            0 => {
                self.erase_in_line(0);
                for r in (self.cursor.row + 1)..self.rows {
                    if r < self.grid.len() {
                        self.grid[r].clear();
                    }
                }
            }
            1 => {
                for r in 0..self.cursor.row {
                    if r < self.grid.len() {
                        self.grid[r].clear();
                    }
                }
                self.erase_in_line(1);
            }
            2 => {
                for row in &mut self.grid {
                    row.clear();
                }
            }
            3 => {
                self.scrollback.clear();
            }
            _ => {}
        }
    }

    pub fn write_char(
        &mut self,
        c: char,
        fg: Color,
        bg: Color,
        flags: CellFlags,
        hyperlink: Option<Arc<String>>,
    ) {
        let char_width = UnicodeWidthChar::width(c).unwrap_or(1);
        if char_width == 0 {
            return;
        }

        if self.cursor.pending_wrap {
            self.cursor.col = 0;
            if self.cursor.row >= self.scroll_region.1 {
                self.scroll_up(1);
            } else {
                self.cursor.row += 1;
            }
            if self.cursor.row < self.grid.len() {
                self.grid[self.cursor.row].is_wrapped = true;
            }
            self.cursor.pending_wrap = false;
        }

        if char_width == 2 && self.cursor.col >= self.cols.saturating_sub(1) {
            if self.cursor.row < self.grid.len() && self.cursor.col < self.cols {
                self.grid[self.cursor.row].cells[self.cursor.col].reset();
                self.grid[self.cursor.row].mark_dirty();
            }
            self.cursor.col = 0;
            if self.cursor.row >= self.scroll_region.1 {
                self.scroll_up(1);
            } else {
                self.cursor.row += 1;
            }
            if self.cursor.row < self.grid.len() {
                self.grid[self.cursor.row].is_wrapped = true;
            }
        }

        let r = self.cursor.row;
        let c_col = self.cursor.col;

        if r < self.grid.len() && c_col < self.cols {
            let row = &mut self.grid[r];
            let mut cell_flags = flags;

            if char_width == 2 {
                cell_flags |= CellFlags::WIDE_CHAR;
                row.cells[c_col] = Cell {
                    c,
                    fg,
                    bg,
                    flags: cell_flags,
                    hyperlink: hyperlink.clone(),
                };
                if c_col + 1 < self.cols {
                    row.cells[c_col + 1] = Cell {
                        c: ' ',
                        fg,
                        bg,
                        flags: CellFlags::WIDE_CHAR_SPACER,
                        hyperlink,
                    };
                }
            } else {
                row.cells[c_col] = Cell {
                    c,
                    fg,
                    bg,
                    flags: cell_flags,
                    hyperlink,
                };
            }
            row.mark_dirty();
        }

        if self.cursor.col + char_width >= self.cols {
            self.cursor.col = self.cols.saturating_sub(1);
            self.cursor.pending_wrap = true;
        } else {
            self.cursor.col += char_width;
        }
    }

    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.cursor.pending_wrap = false;
    }

    pub fn linefeed(&mut self) {
        self.cursor.pending_wrap = false;
        if self.cursor.row >= self.scroll_region.1 {
            self.scroll_up(1);
        } else {
            self.cursor.row += 1;
        }
    }

    pub fn newline(&mut self) {
        self.carriage_return();
        self.linefeed();
    }

    pub fn backspace(&mut self) {
        self.cursor.pending_wrap = false;
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
        }
    }

    pub fn tab(&mut self) {
        self.cursor.pending_wrap = false;
        let next_tab = (self.cursor.col / 8 + 1) * 8;
        self.cursor.col = next_tab.min(self.cols.saturating_sub(1));
    }

    pub fn goto(&mut self, row: usize, col: usize) {
        self.cursor.pending_wrap = false;
        self.cursor.row = row.min(self.rows.saturating_sub(1));
        self.cursor.col = col.min(self.cols.saturating_sub(1));
    }

    pub fn move_up(&mut self, count: usize) {
        self.cursor.pending_wrap = false;
        let (top, _) = self.scroll_region;
        if self.cursor.row >= top {
            self.cursor.row = self.cursor.row.saturating_sub(count).max(top);
        } else {
            self.cursor.row = self.cursor.row.saturating_sub(count);
        }
    }

    pub fn move_down(&mut self, count: usize) {
        self.cursor.pending_wrap = false;
        let (_, bottom) = self.scroll_region;
        if self.cursor.row <= bottom {
            self.cursor.row = (self.cursor.row + count).min(bottom);
        } else {
            self.cursor.row = (self.cursor.row + count).min(self.rows.saturating_sub(1));
        }
    }

    pub fn move_forward(&mut self, count: usize) {
        self.cursor.pending_wrap = false;
        self.cursor.col = (self.cursor.col + count).min(self.cols.saturating_sub(1));
    }

    pub fn move_backward(&mut self, count: usize) {
        self.cursor.pending_wrap = false;
        self.cursor.col = self.cursor.col.saturating_sub(count);
    }
}

/// Dual-buffer Screen Grid: Primary (with Scrollback) and Alternate (fullscreen TUI).
#[derive(Debug, Clone)]
pub struct ScreenGrid {
    pub primary: ScreenBuffer,
    pub alternate: ScreenBuffer,
    pub is_alternate: bool,
}

impl ScreenGrid {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            primary: ScreenBuffer::new(rows, cols, max_scrollback),
            alternate: ScreenBuffer::new(rows, cols, 0),
            is_alternate: false,
        }
    }

    #[inline]
    pub fn active(&self) -> &ScreenBuffer {
        if self.is_alternate {
            &self.alternate
        } else {
            &self.primary
        }
    }

    #[inline]
    pub fn active_mut(&mut self) -> &mut ScreenBuffer {
        if self.is_alternate {
            &mut self.alternate
        } else {
            &mut self.primary
        }
    }

    pub fn set_alternate(&mut self, alt: bool) {
        if self.is_alternate != alt {
            self.is_alternate = alt;
            if alt {
                self.alternate.erase_in_display(2);
                self.alternate.cursor = Cursor::default();
            }
        }
    }

    pub fn resize(&mut self, rows: usize, cols: usize) {
        self.primary.resize(rows, cols);
        self.alternate.resize(rows, cols);
    }

    pub fn is_dirty(&self) -> bool {
        self.active().grid.iter().any(|row| row.is_dirty)
    }

    pub fn clear_dirty(&mut self) {
        for row in &mut self.active_mut().grid {
            row.reset_dirty();
        }
    }
}
