//! VT100, xterm, 24-bit TrueColor ANSI, Sixel, and Kitty Graphics escape sequence handler.

use crate::graphics::{GraphicImage, KittyGraphicsParser, SixelDecoder};
use crate::grid::{CellFlags, Color, CursorShape, NamedColor, SavedCursor, ScreenGrid};
use std::sync::Arc;
use tracing::info;
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
    pub current_hyperlink_id: Option<u32>,
    pub title: String,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub mouse_protocol: MouseProtocol,
    pub mouse_format: MouseFormat,
    pub in_sixel: bool,
    pub sixel_buffer: Vec<u8>,
    pub kitty_parser: KittyGraphicsParser,
    pub insert_mode: bool,
}

impl TerminalState {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            grid: ScreenGrid::new(rows, cols, max_scrollback),
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            flags: CellFlags::empty(),
            current_hyperlink: None,
            current_hyperlink_id: None,
            title: String::from("skTerm"),
            application_cursor_keys: false,
            bracketed_paste: false,
            mouse_protocol: MouseProtocol::None,
            mouse_format: MouseFormat::X10,
            in_sixel: false,
            sixel_buffer: Vec::new(),
            kitty_parser: KittyGraphicsParser::new(),
            insert_mode: false,
        }
    }

    pub fn reset_sgr(&mut self) {
        self.fg = Color::DefaultFg;
        self.bg = Color::DefaultBg;
        self.flags = CellFlags::empty();
        self.current_hyperlink = None;
        self.current_hyperlink_id = None;
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

        let mut iter = flat_params.into_iter();
        while let Some(code) = iter.next() {
            match code {
                0 => self.reset_sgr(),
                1 => self.flags.insert(CellFlags::BOLD),
                2 => self.flags.insert(CellFlags::DIM),
                3 => self.flags.insert(CellFlags::ITALIC),
                4 => self.flags.insert(CellFlags::UNDERLINE),
                5 => self.flags.insert(CellFlags::BLINK),
                7 => self.flags.insert(CellFlags::INVERSE),
                8 => self.flags.insert(CellFlags::HIDDEN),
                9 => self.flags.insert(CellFlags::STRIKETHROUGH),
                22 => self.flags.remove(CellFlags::BOLD | CellFlags::DIM),
                23 => self.flags.remove(CellFlags::ITALIC),
                24 => self.flags.remove(CellFlags::UNDERLINE),
                25 => self.flags.remove(CellFlags::BLINK),
                27 => self.flags.remove(CellFlags::INVERSE),
                28 => self.flags.remove(CellFlags::HIDDEN),
                29 => self.flags.remove(CellFlags::STRIKETHROUGH),

                30..=37 => self.fg = Color::Named(named_from_code(code - 30, false)),
                38 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.fg = color;
                    }
                }
                39 => self.fg = Color::DefaultFg,

                40..=47 => self.bg = Color::Named(named_from_code(code - 40, false)),
                48 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.bg = color;
                    }
                }
                49 => self.bg = Color::DefaultBg,

                90..=97 => self.fg = Color::Named(named_from_code(code - 90, true)),
                100..=107 => self.bg = Color::Named(named_from_code(code - 100, true)),

                _ => {}
            }
        }
    }
}

fn named_from_code(offset: u16, bright: bool) -> NamedColor {
    match (offset, bright) {
        (0, false) => NamedColor::Black,
        (1, false) => NamedColor::Red,
        (2, false) => NamedColor::Green,
        (3, false) => NamedColor::Yellow,
        (4, false) => NamedColor::Blue,
        (5, false) => NamedColor::Magenta,
        (6, false) => NamedColor::Cyan,
        (7, false) => NamedColor::White,
        (0, true) => NamedColor::BrightBlack,
        (1, true) => NamedColor::BrightRed,
        (2, true) => NamedColor::BrightGreen,
        (3, true) => NamedColor::BrightYellow,
        (4, true) => NamedColor::BrightBlue,
        (5, true) => NamedColor::BrightMagenta,
        (6, true) => NamedColor::BrightCyan,
        (7, true) => NamedColor::BrightWhite,
        _ => NamedColor::White,
    }
}

fn parse_extended_color(iter: &mut impl Iterator<Item = u16>) -> Option<Color> {
    match iter.next()? {
        2 => {
            let r = iter.next()? as u8;
            let g = iter.next()? as u8;
            let b = iter.next()? as u8;
            Some(Color::Rgb(r, g, b))
        }
        5 => {
            let idx = iter.next()? as u8;
            Some(Color::Indexed(idx))
        }
        _ => None,
    }
}

impl Perform for TerminalState {
    fn print(&mut self, c: char) {
        let fg = self.fg;
        let bg = self.bg;
        let flags = self.flags;
        let hid = self.current_hyperlink_id;
        if self.insert_mode {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if w > 0 {
                self.grid.active_mut().insert_chars(w, bg);
            }
        }
        self.grid.active_mut().write_char(c, fg, bg, flags, hid);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.grid.active_mut().linefeed(),
            b'\r' => {
                let active = self.grid.active_mut();
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
            }
            b'\t' => {
                let active = self.grid.active_mut();
                let next_tab = (active.cursor.col / 8 + 1) * 8;
                active.cursor.col = next_tab.min(active.cols.saturating_sub(1));
                active.cursor.pending_wrap = false;
            }
            0x08 => self.grid.active_mut().move_backward(1),
            0x07 => {} // Bell
            _ => {}
        }
    }

    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, action: char) {
        if action == 'q' {
            // Sixel Graphics DCS entry
            self.in_sixel = true;
            self.sixel_buffer.clear();
        }
    }

    fn put(&mut self, byte: u8) {
        if self.in_sixel {
            self.sixel_buffer.push(byte);
        }
    }

    fn unhook(&mut self) {
        if self.in_sixel {
            self.in_sixel = false;
            let mut decoder = SixelDecoder::new();
            if let Some((w_px, h_px, rgba)) = decoder.decode(&self.sixel_buffer) {
                let cell_w = 9.0;
                let cell_h = 18.0;
                let w_cells = ((w_px as f32 / cell_w).ceil() as usize).max(1);
                let h_cells = ((h_px as f32 / cell_h).ceil() as usize).max(1);

                let id = self.grid.next_image_id;
                self.grid.next_image_id += 1;

                let cur_row = self.grid.active().cursor.row;
                let cur_col = self.grid.active().cursor.col;

                let img = GraphicImage {
                    id,
                    row: cur_row,
                    col: cur_col,
                    width_px: w_px,
                    height_px: h_px,
                    width_cells: w_cells,
                    height_cells: h_cells,
                    rgba_data: rgba,
                    z_index: 0,
                };

                info!(
                    "Decoded Sixel image: {}x{} px (cells: {}x{})",
                    w_px, h_px, w_cells, h_cells
                );
                self.grid.active_mut().add_image(img);

                // Advance cursor down below the image
                self.grid.active_mut().move_down(h_cells);
            }
            self.sixel_buffer.clear();
        }
    }

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        match params[0] {
            b"0" | b"2" => {
                if params.len() > 1 {
                    if let Ok(title) = std::str::from_utf8(params[1]) {
                        self.title = title.to_string();
                    }
                }
            }
            b"8" => {
                if params.len() >= 3 {
                    if let Ok(uri) = std::str::from_utf8(params[2]) {
                        if !uri.is_empty() {
                            let hid = self.grid.register_hyperlink(uri);
                            self.current_hyperlink_id = Some(hid);
                            self.current_hyperlink = Some(Arc::new(uri.to_string()));
                        } else {
                            self.current_hyperlink_id = None;
                            self.current_hyperlink = None;
                        }
                    }
                }
            }
            _ => {
                // Check Kitty Graphics via OSC or APC
                if let Ok(cmd) = std::str::from_utf8(params[0]) {
                    if let Some(cmd_args) = cmd.strip_prefix('G') {
                        if let Some((w, h, rgba, z)) = self.kitty_parser.parse_command(cmd_args) {
                            let id = self.grid.next_image_id;
                            self.grid.next_image_id += 1;

                            let cur_row = self.grid.active().cursor.row;
                            let cur_col = self.grid.active().cursor.col;

                            let w_cells = ((w as f32 / 9.0).ceil() as usize).max(1);
                            let h_cells = ((h as f32 / 18.0).ceil() as usize).max(1);

                            let img = GraphicImage {
                                id,
                                row: cur_row,
                                col: cur_col,
                                width_px: w,
                                height_px: h,
                                width_cells: w_cells,
                                height_cells: h_cells,
                                rgba_data: rgba,
                                z_index: z,
                            };
                            info!("Decoded Kitty Graphics image: {}x{} px", w, h);
                            self.grid.active_mut().add_image(img);
                        }
                    }
                }
            }
        }
    }

    fn csi_dispatch(&mut self, params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        let is_private = intermediates.first() == Some(&b'?');
        let mut first_param = 0;
        if let Some(param) = params.iter().next() {
            if let Some(&val) = param.first() {
                first_param = val;
            }
        }

        match action {
            'm' => self.parse_sgr(params),
            'H' | 'f' => {
                let mut row = 1;
                let mut col = 1;
                let mut iter = params.iter();
                if let Some(p) = iter.next() {
                    if let Some(&r) = p.first() {
                        if r > 0 {
                            row = r;
                        }
                    }
                }
                if let Some(p) = iter.next() {
                    if let Some(&c) = p.first() {
                        if c > 0 {
                            col = c;
                        }
                    }
                }
                self.grid
                    .active_mut()
                    .move_cursor((row as usize) - 1, (col as usize) - 1);
            }
            'A' => self
                .grid
                .active_mut()
                .move_up((first_param as usize).max(1)),
            'B' => self
                .grid
                .active_mut()
                .move_down((first_param as usize).max(1)),
            'C' => self
                .grid
                .active_mut()
                .move_forward((first_param as usize).max(1)),
            'D' => self
                .grid
                .active_mut()
                .move_backward((first_param as usize).max(1)),
            'E' => {
                let active = self.grid.active_mut();
                active.move_down((first_param as usize).max(1));
                active.cursor.col = 0;
            }
            'F' => {
                let active = self.grid.active_mut();
                active.move_up((first_param as usize).max(1));
                active.cursor.col = 0;
            }
            'G' => {
                let col = (first_param as usize).max(1) - 1;
                self.grid.active_mut().cursor.col =
                    col.min(self.grid.active().cols.saturating_sub(1));
                self.grid.active_mut().cursor.pending_wrap = false;
            }
            'd' => {
                let row = (first_param as usize).max(1) - 1;
                self.grid.active_mut().cursor.row =
                    row.min(self.grid.active().rows.saturating_sub(1));
                self.grid.active_mut().cursor.pending_wrap = false;
            }
            '@' => self
                .grid
                .active_mut()
                .insert_chars((first_param as usize).max(1), self.bg),
            'P' => self
                .grid
                .active_mut()
                .delete_chars((first_param as usize).max(1), self.bg),
            'X' => self
                .grid
                .active_mut()
                .erase_chars((first_param as usize).max(1), self.bg),
            'L' => self
                .grid
                .active_mut()
                .insert_lines((first_param as usize).max(1), self.bg),
            'M' => self
                .grid
                .active_mut()
                .delete_lines((first_param as usize).max(1), self.bg),
            'J' => self
                .grid
                .active_mut()
                .erase_in_display(first_param as usize, self.bg),
            'K' => self
                .grid
                .active_mut()
                .erase_in_line(first_param as usize, self.bg),
            'S' => self
                .grid
                .active_mut()
                .scroll_up((first_param as usize).max(1)),
            'T' => self
                .grid
                .active_mut()
                .scroll_down((first_param as usize).max(1)),
            's' if !is_private && intermediates.is_empty() => self.save_cursor(),
            'u' if !is_private && intermediates.is_empty() => self.restore_cursor(),
            'r' => {
                let mut top = 1;
                let mut bottom = self.grid.active().rows;
                let mut iter = params.iter();
                if let Some(p) = iter.next() {
                    if let Some(&t) = p.first() {
                        if t > 0 {
                            top = t as usize;
                        }
                    }
                }
                if let Some(p) = iter.next() {
                    if let Some(&b) = p.first() {
                        if b > 0 {
                            bottom = b as usize;
                        }
                    }
                }
                if top < bottom && bottom <= self.grid.active().rows {
                    self.grid.active_mut().scroll_region = (top - 1, bottom - 1);
                    self.grid.active_mut().cursor.row = top - 1;
                    self.grid.active_mut().cursor.col = 0;
                }
            }
            'h' => {
                for param in params.iter() {
                    for &mode in param {
                        if is_private {
                            match mode {
                                1 => self.application_cursor_keys = true,
                                25 => self.grid.active_mut().cursor.visible = true,
                                47 | 1049 => {
                                    if mode == 1049 {
                                        self.save_cursor();
                                    }
                                    self.grid.set_alternate(true);
                                }
                                2004 => self.bracketed_paste = true,
                                1000 => self.mouse_protocol = MouseProtocol::Normal,
                                1002 => self.mouse_protocol = MouseProtocol::ButtonEvent,
                                1003 => self.mouse_protocol = MouseProtocol::AnyEvent,
                                1006 => self.mouse_format = MouseFormat::Sgr,
                                _ => {}
                            }
                        } else if mode == 4 {
                            self.insert_mode = true;
                        }
                    }
                }
            }
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
                        } else if mode == 4 {
                            self.insert_mode = false;
                        }
                    }
                }
            }
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
                let active = self.grid.active_mut();
                if active.cursor.row == active.scroll_region.0 {
                    active.scroll_down(1);
                } else {
                    active.move_up(1);
                }
            }
            b'D' => self.grid.active_mut().linefeed(),
            b'E' => self.grid.active_mut().newline(),
            b'c' => {
                let rows = self.grid.active().rows;
                let cols = self.grid.active().cols;
                let max_scroll = self.grid.primary.max_scrollback;
                *self = Self::new(rows, cols, max_scroll);
            }
            _ => {}
        }
    }
}
