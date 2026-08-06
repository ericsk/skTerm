//! VT100, xterm, and 24-bit TrueColor ANSI escape sequence parser and handler.

use crate::grid::{CellFlags, Color, CursorShape, NamedColor, SavedCursor, ScreenGrid};
use std::sync::Arc;
use vte::{Params, Perform};

/// Mouse reporting modes supported by xterm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseProtocol {
    #[default]
    None,
    X10,
    Normal,
    ButtonEvent,
    AnyEvent,
}

/// Mouse coordinate format
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseFormat {
    #[default]
    X10,
    Sgr,
}

/// Terminal state engine combining ScreenGrid and VTE event handlers.
#[derive(Debug, Clone)]
pub struct TerminalState {
    pub grid: ScreenGrid,
    pub fg: Color,
    pub bg: Color,
    pub flags: CellFlags,
    pub current_hyperlink: Option<Arc<String>>,
    pub title: String,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub mouse_protocol: MouseProtocol,
    pub mouse_format: MouseFormat,
}

impl TerminalState {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            grid: ScreenGrid::new(rows, cols, max_scrollback),
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            flags: CellFlags::empty(),
            current_hyperlink: None,
            title: String::from("skTerm"),
            application_cursor_keys: false,
            bracketed_paste: false,
            mouse_protocol: MouseProtocol::None,
            mouse_format: MouseFormat::X10,
        }
    }

    pub fn reset_sgr(&mut self) {
        self.fg = Color::DefaultFg;
        self.bg = Color::DefaultBg;
        self.flags = CellFlags::empty();
        self.current_hyperlink = None;
    }

    fn save_cursor(&mut self) {
        let active = self.grid.active_mut();
        active.saved_cursor = Some(SavedCursor {
            row: active.cursor.row,
            col: active.cursor.col,
            fg: self.fg,
            bg: self.bg,
            flags: self.flags,
            shape: active.cursor.shape,
            visible: active.cursor.visible,
            pending_wrap: active.cursor.pending_wrap,
        });
    }

    fn restore_cursor(&mut self) {
        let active = self.grid.active_mut();
        if let Some(saved) = active.saved_cursor.take() {
            active.cursor.row = saved.row.min(active.rows.saturating_sub(1));
            active.cursor.col = saved.col.min(active.cols.saturating_sub(1));
            active.cursor.shape = saved.shape;
            active.cursor.visible = saved.visible;
            active.cursor.pending_wrap = saved.pending_wrap;
            self.fg = saved.fg;
            self.bg = saved.bg;
            self.flags = saved.flags;
        }
    }

    fn parse_sgr(&mut self, params: &Params) {
        let mut flat_params = Vec::new();
        for param in params.iter() {
            for &val in param {
                flat_params.push(val);
            }
        }

        if flat_params.is_empty() {
            self.reset_sgr();
            return;
        }

        let mut i = 0;
        while i < flat_params.len() {
            match flat_params[i] {
                0 => self.reset_sgr(),
                1 => self.flags.insert(CellFlags::BOLD),
                2 => self.flags.insert(CellFlags::DIM),
                3 => self.flags.insert(CellFlags::ITALIC),
                4 => self.flags.insert(CellFlags::UNDERLINE),
                5 => self.flags.insert(CellFlags::BLINK),
                7 => self.flags.insert(CellFlags::INVERSE),
                8 => self.flags.insert(CellFlags::HIDDEN),
                9 => self.flags.insert(CellFlags::STRIKETHROUGH),
                22 => {
                    self.flags.remove(CellFlags::BOLD);
                    self.flags.remove(CellFlags::DIM);
                }
                23 => self.flags.remove(CellFlags::ITALIC),
                24 => self.flags.remove(CellFlags::UNDERLINE),
                25 => self.flags.remove(CellFlags::BLINK),
                27 => self.flags.remove(CellFlags::INVERSE),
                28 => self.flags.remove(CellFlags::HIDDEN),
                29 => self.flags.remove(CellFlags::STRIKETHROUGH),
                // 30-37 Standard FG
                30 => self.fg = Color::Named(NamedColor::Black),
                31 => self.fg = Color::Named(NamedColor::Red),
                32 => self.fg = Color::Named(NamedColor::Green),
                33 => self.fg = Color::Named(NamedColor::Yellow),
                34 => self.fg = Color::Named(NamedColor::Blue),
                35 => self.fg = Color::Named(NamedColor::Magenta),
                36 => self.fg = Color::Named(NamedColor::Cyan),
                37 => self.fg = Color::Named(NamedColor::White),
                // 38 Extended FG
                38 => {
                    if i + 1 < flat_params.len() {
                        match flat_params[i + 1] {
                            5 => {
                                // 256 Color: 38;5;n
                                if i + 2 < flat_params.len() {
                                    self.fg = Color::Indexed(flat_params[i + 2] as u8);
                                    i += 2;
                                }
                            }
                            2 => {
                                // TrueColor RGB: 38;2;r;g;b
                                if i + 4 < flat_params.len() {
                                    let r = flat_params[i + 2] as u8;
                                    let g = flat_params[i + 3] as u8;
                                    let b = flat_params[i + 4] as u8;
                                    self.fg = Color::Rgb(r, g, b);
                                    i += 4;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                39 => self.fg = Color::DefaultFg,
                // 40-47 Standard BG
                40 => self.bg = Color::Named(NamedColor::Black),
                41 => self.bg = Color::Named(NamedColor::Red),
                42 => self.bg = Color::Named(NamedColor::Green),
                43 => self.bg = Color::Named(NamedColor::Yellow),
                44 => self.bg = Color::Named(NamedColor::Blue),
                45 => self.bg = Color::Named(NamedColor::Magenta),
                46 => self.bg = Color::Named(NamedColor::Cyan),
                47 => self.bg = Color::Named(NamedColor::White),
                // 48 Extended BG
                48 => {
                    if i + 1 < flat_params.len() {
                        match flat_params[i + 1] {
                            5 => {
                                // 256 Color: 48;5;n
                                if i + 2 < flat_params.len() {
                                    self.bg = Color::Indexed(flat_params[i + 2] as u8);
                                    i += 2;
                                }
                            }
                            2 => {
                                // TrueColor RGB: 48;2;r;g;b
                                if i + 4 < flat_params.len() {
                                    let r = flat_params[i + 2] as u8;
                                    let g = flat_params[i + 3] as u8;
                                    let b = flat_params[i + 4] as u8;
                                    self.bg = Color::Rgb(r, g, b);
                                    i += 4;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                49 => self.bg = Color::DefaultBg,
                // 90-97 Bright FG
                90 => self.fg = Color::Named(NamedColor::BrightBlack),
                91 => self.fg = Color::Named(NamedColor::BrightRed),
                92 => self.fg = Color::Named(NamedColor::BrightGreen),
                93 => self.fg = Color::Named(NamedColor::BrightYellow),
                94 => self.fg = Color::Named(NamedColor::BrightBlue),
                95 => self.fg = Color::Named(NamedColor::BrightMagenta),
                96 => self.fg = Color::Named(NamedColor::BrightCyan),
                97 => self.fg = Color::Named(NamedColor::BrightWhite),
                // 100-107 Bright BG
                100 => self.bg = Color::Named(NamedColor::BrightBlack),
                101 => self.bg = Color::Named(NamedColor::BrightRed),
                102 => self.bg = Color::Named(NamedColor::BrightGreen),
                103 => self.bg = Color::Named(NamedColor::BrightYellow),
                104 => self.bg = Color::Named(NamedColor::BrightBlue),
                105 => self.bg = Color::Named(NamedColor::BrightMagenta),
                106 => self.bg = Color::Named(NamedColor::BrightCyan),
                107 => self.bg = Color::Named(NamedColor::BrightWhite),
                _ => {}
            }
            i += 1;
        }
    }
}

impl Perform for TerminalState {
    fn print(&mut self, c: char) {
        self.grid.active_mut().write_char(
            c,
            self.fg,
            self.bg,
            self.flags,
            self.current_hyperlink.clone(),
        );
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' => self.grid.active_mut().linefeed(),
            b'\r' => self.grid.active_mut().carriage_return(),
            b'\t' => self.grid.active_mut().tab(),
            0x08 => self.grid.active_mut().backspace(), // BS
            0x07 => {}                                  // Bell
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        if let Ok(cmd_str) = std::str::from_utf8(params[0]) {
            match cmd_str {
                "0" | "2" => {
                    // Set window title
                    if params.len() > 1 {
                        if let Ok(title) = std::str::from_utf8(params[1]) {
                            self.title = title.to_string();
                        }
                    }
                }
                "8" => {
                    // OSC 8 Hyperlink: \x1b]8;params;url\x1b\
                    if params.len() >= 3 {
                        if let Ok(url) = std::str::from_utf8(params[2]) {
                            if url.is_empty() {
                                self.current_hyperlink = None;
                            } else {
                                self.current_hyperlink = Some(Arc::new(url.to_string()));
                            }
                        }
                    } else {
                        self.current_hyperlink = None;
                    }
                }
                _ => {}
            }
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        let first_param = params.iter().next().and_then(|p| p.first().copied()).unwrap_or(0);
        let n = if first_param == 0 { 1 } else { first_param as usize };
        let is_private = intermediates.first() == Some(&b'?');

        match action {
            // Cursor Movement
            'A' => self.grid.active_mut().move_up(n),
            'B' => self.grid.active_mut().move_down(n),
            'C' => self.grid.active_mut().move_forward(n),
            'D' => self.grid.active_mut().move_backward(n),
            'E' => {
                let active = self.grid.active_mut();
                active.move_down(n);
                active.carriage_return();
            }
            'F' => {
                let active = self.grid.active_mut();
                active.move_up(n);
                active.carriage_return();
            }
            'G' | '`' => {
                let col = n.saturating_sub(1);
                self.grid.active_mut().cursor.col = col.min(self.grid.active().cols.saturating_sub(1));
                self.grid.active_mut().cursor.pending_wrap = false;
            }
            'H' | 'f' => {
                let mut iter = params.iter();
                let r = iter
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize;
                let c = iter
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize;
                self.grid.active_mut().goto(r - 1, c - 1);
            }
            'd' => {
                let row = n.saturating_sub(1);
                self.grid.active_mut().cursor.row = row.min(self.grid.active().rows.saturating_sub(1));
                self.grid.active_mut().cursor.pending_wrap = false;
            }

            // Erase / Delete / Insert
            'J' => self.grid.active_mut().erase_in_display(first_param),
            'K' => self.grid.active_mut().erase_in_line(first_param),
            '@' => self.grid.active_mut().insert_chars(n),
            'P' => self.grid.active_mut().delete_chars(n),
            'L' => self.grid.active_mut().insert_lines(n),
            'M' => self.grid.active_mut().delete_lines(n),
            'X' => self.grid.active_mut().erase_chars(n),

            // Scroll
            'S' => self.grid.active_mut().scroll_up(n),
            'T' => self.grid.active_mut().scroll_down(n),

            // Margins / Scroll Region
            'r' => {
                let mut iter = params.iter();
                let top = iter
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(1)
                    .max(1) as usize;
                let bottom = iter
                    .next()
                    .and_then(|p| p.first().copied())
                    .unwrap_or(self.grid.active().rows as u16) as usize;
                self.grid
                    .active_mut()
                    .set_scroll_region(top - 1, bottom.saturating_sub(1));
            }

            // SGR (Select Graphic Rendition)
            'm' => self.parse_sgr(params),

            // Mode Set (SM / DECSET)
            'h' => {
                for param in params.iter() {
                    for &mode in param {
                        if is_private {
                            match mode {
                                1 => self.application_cursor_keys = true,
                                25 => self.grid.active_mut().cursor.visible = true,
                                47 => self.grid.set_alternate(true),
                                1049 => {
                                    self.save_cursor();
                                    self.grid.set_alternate(true);
                                }
                                2004 => self.bracketed_paste = true,
                                1000 => self.mouse_protocol = MouseProtocol::Normal,
                                1002 => self.mouse_protocol = MouseProtocol::ButtonEvent,
                                1003 => self.mouse_protocol = MouseProtocol::AnyEvent,
                                1006 => self.mouse_format = MouseFormat::Sgr,
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Mode Reset (RM / DECRST)
            'l' => {
                for param in params.iter() {
                    for &mode in param {
                        if is_private {
                            match mode {
                                1 => self.application_cursor_keys = false,
                                25 => self.grid.active_mut().cursor.visible = false,
                                47 => self.grid.set_alternate(false),
                                1049 => {
                                    self.grid.set_alternate(false);
                                    self.restore_cursor();
                                }
                                2004 => self.bracketed_paste = false,
                                1000 | 1002 | 1003 => self.mouse_protocol = MouseProtocol::None,
                                1006 => self.mouse_format = MouseFormat::X10,
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Cursor Shape (DECSCUSR)
            'q' if intermediates.first() == Some(&b' ') => match first_param {
                1 | 2 => self.grid.active_mut().cursor.shape = CursorShape::Block,
                3 | 4 => self.grid.active_mut().cursor.shape = CursorShape::Underline,
                5 | 6 => self.grid.active_mut().cursor.shape = CursorShape::Beam,
                _ => {}
            },

            _ => {}
        }
    }

    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, byte: u8) {
        match byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'M' => {
                // Reverse Index (RI)
                let active = self.grid.active_mut();
                if active.cursor.row == active.scroll_region.0 {
                    active.scroll_down(1);
                } else {
                    active.move_up(1);
                }
            }
            b'D' => self.grid.active_mut().linefeed(), // Index (IND)
            b'E' => self.grid.active_mut().newline(),  // Next Line (NEL)
            b'c' => {
                // Reset to Initial State (RIS)
                let rows = self.grid.active().rows;
                let cols = self.grid.active().cols;
                let max_scroll = self.grid.primary.max_scrollback;
                *self = Self::new(rows, cols, max_scroll);
            }
            _ => {}
        }
    }
}
