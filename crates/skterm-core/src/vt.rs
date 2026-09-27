//! VT100, xterm, 24-bit TrueColor ANSI, Sixel, and Kitty Graphics escape sequence handler.

use crate::graphics::{GraphicImage, KittyGraphicsParser, SixelDecoder};
use crate::grid::{CellFlags, Color, CursorShape, NamedColor, SavedCursor, ScreenGrid};
use std::collections::HashMap;
use std::num::{NonZeroU16, NonZeroU32};
use std::sync::Arc;
use tracing::info;
use vte::{Params, Perform};

const ALL_UNDERLINE_FLAGS: CellFlags = CellFlags::UNDERLINE
    .union(CellFlags::DOUBLE_UNDERLINE)
    .union(CellFlags::CURLY_UNDERLINE)
    .union(CellFlags::DOTTED_UNDERLINE)
    .union(CellFlags::DASHED_UNDERLINE);

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

/// Character set designation for G0 / G1 slots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Charset {
    #[default]
    Ascii,
    DecSpecialGraphics,
}

/// Terminal state engine combining ScreenGrid and VTE event handlers.
#[derive(Debug, Clone)]
pub struct TerminalState {
    pub grid: ScreenGrid,
    pub fg: Color,
    pub bg: Color,
    pub underline_color: Option<Color>,
    pub current_underline_color_id: Option<NonZeroU16>,
    pub flags: CellFlags,
    pub current_hyperlink: Option<Arc<String>>,
    pub current_hyperlink_id: Option<NonZeroU32>,
    pub title: String,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub auto_wrap: bool,
    pub origin_mode: bool,
    pub focus_events: bool,
    pub synchronized_output: bool,
    pub modify_other_keys: u8,
    pub kitty_keyboard_stack: Vec<u16>,
    pub dynamic_palette: HashMap<u8, (u8, u8, u8)>,
    pub dynamic_fg: Option<(u8, u8, u8)>,
    pub dynamic_bg: Option<(u8, u8, u8)>,
    pub dynamic_cursor: Option<(u8, u8, u8)>,
    pub mouse_protocol: MouseProtocol,
    pub mouse_format: MouseFormat,
    pub in_sixel: bool,
    pub sixel_buffer: Vec<u8>,
    pub kitty_parser: KittyGraphicsParser,
    pub insert_mode: bool,
    pub current_working_directory: Option<String>,
    pub pending_clipboard: Option<String>,
    pub pending_responses: Vec<u8>,
    pub last_printed_char: Option<char>,
    pub g0_charset: Charset,
    pub g1_charset: Charset,
    pub active_charset: u8,
}

impl TerminalState {
    pub fn new(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            grid: ScreenGrid::new(rows, cols, max_scrollback),
            fg: Color::DefaultFg,
            bg: Color::DefaultBg,
            underline_color: None,
            current_underline_color_id: None,
            flags: CellFlags::empty(),
            current_hyperlink: None,
            current_hyperlink_id: None,
            title: String::from("skTerm"),
            application_cursor_keys: false,
            bracketed_paste: false,
            auto_wrap: true,
            origin_mode: false,
            focus_events: false,
            synchronized_output: false,
            modify_other_keys: 0,
            kitty_keyboard_stack: Vec::new(),
            dynamic_palette: HashMap::new(),
            dynamic_fg: None,
            dynamic_bg: None,
            dynamic_cursor: None,
            mouse_protocol: MouseProtocol::None,
            mouse_format: MouseFormat::X10,
            in_sixel: false,
            sixel_buffer: Vec::new(),
            kitty_parser: KittyGraphicsParser::new(),
            insert_mode: false,
            current_working_directory: None,
            pending_clipboard: None,
            pending_responses: Vec::new(),
            last_printed_char: None,
            g0_charset: Charset::Ascii,
            g1_charset: Charset::Ascii,
            active_charset: 0,
        }
    }

    pub fn reset_sgr(&mut self) {
        self.fg = Color::DefaultFg;
        self.bg = Color::DefaultBg;
        self.underline_color = None;
        self.current_underline_color_id = None;
        self.flags = CellFlags::empty();
    }

    pub fn kitty_keyboard_flags(&self) -> u16 {
        self.kitty_keyboard_stack.last().copied().unwrap_or(0)
    }

    fn set_underline_color(&mut self, color: Option<Color>) {
        self.underline_color = color;
        self.current_underline_color_id = color.and_then(|c| self.grid.register_underline_color(c));
    }

    fn active_charset_mode(&self) -> Charset {
        if self.active_charset == 1 {
            self.g1_charset
        } else {
            self.g0_charset
        }
    }

    fn write_graphic_char(&mut self, c: char) {
        let fg = self.fg;
        let bg = self.bg;
        let flags = self.flags;
        let ul_id = self.current_underline_color_id;
        let hid = self.current_hyperlink_id;
        let auto_wrap = self.auto_wrap;
        if self.insert_mode {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if w > 0 {
                self.grid.active_mut().insert_chars(w, bg);
            }
        }
        self.grid
            .active_mut()
            .write_char_with_style(c, fg, bg, flags, ul_id, hid, auto_wrap);
    }

    fn save_cursor(&mut self) {
        let origin_mode = self.origin_mode;
        let active = self.grid.active_mut();
        active.saved_cursor = Some(SavedCursor {
            row: active.cursor.row,
            col: active.cursor.col,
            fg: self.fg,
            bg: self.bg,
            underline_color: self.underline_color,
            underline_color_id: self.current_underline_color_id,
            flags: self.flags,
            shape: active.cursor.shape,
            visible: active.cursor.visible,
            pending_wrap: active.cursor.pending_wrap,
            origin_mode,
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
            self.underline_color = saved.underline_color;
            self.current_underline_color_id = saved.underline_color_id;
            self.flags = saved.flags;
            self.origin_mode = saved.origin_mode;
        }
    }

    fn parse_sgr(&mut self, params: &Params) {
        if params.is_empty() {
            self.reset_sgr();
            return;
        }

        // Walk `Params` directly without heap allocation while keeping colon-separated
        // subparameters (`ESC[4:3m`, `ESC[38:2::r:g:bm`, `ESC[58:2::r:g:bm`) grouped
        // with their owning SGR code instead of leaking them as standalone SGR commands.
        let mut iter = params.iter();
        while let Some(param) = iter.next() {
            let code = param.first().copied().unwrap_or(0);
            if param.len() > 1 {
                match code {
                    4 => {
                        self.flags.remove(ALL_UNDERLINE_FLAGS);
                        match param[1] {
                            0 => {}
                            1 => self.flags.insert(CellFlags::UNDERLINE),
                            2 => self
                                .flags
                                .insert(CellFlags::UNDERLINE | CellFlags::DOUBLE_UNDERLINE),
                            3 => self
                                .flags
                                .insert(CellFlags::UNDERLINE | CellFlags::CURLY_UNDERLINE),
                            4 => self
                                .flags
                                .insert(CellFlags::UNDERLINE | CellFlags::DOTTED_UNDERLINE),
                            5 => self
                                .flags
                                .insert(CellFlags::UNDERLINE | CellFlags::DASHED_UNDERLINE),
                            _ => self.flags.insert(CellFlags::UNDERLINE),
                        }
                    }
                    38 => {
                        if let Some(color) = parse_colon_color(&param[1..]) {
                            self.fg = color;
                        }
                    }
                    48 => {
                        if let Some(color) = parse_colon_color(&param[1..]) {
                            self.bg = color;
                        }
                    }
                    58 => {
                        if let Some(color) = parse_colon_color(&param[1..]) {
                            self.set_underline_color(Some(color));
                        }
                    }
                    _ => {}
                }
                continue;
            }

            match code {
                0 => self.reset_sgr(),
                1 => self.flags.insert(CellFlags::BOLD),
                2 => self.flags.insert(CellFlags::DIM),
                3 => self.flags.insert(CellFlags::ITALIC),
                4 => {
                    self.flags.remove(ALL_UNDERLINE_FLAGS);
                    self.flags.insert(CellFlags::UNDERLINE);
                }
                5 => self.flags.insert(CellFlags::BLINK),
                7 => self.flags.insert(CellFlags::INVERSE),
                8 => self.flags.insert(CellFlags::HIDDEN),
                9 => self.flags.insert(CellFlags::STRIKETHROUGH),
                21 => {
                    self.flags.remove(ALL_UNDERLINE_FLAGS);
                    self.flags
                        .insert(CellFlags::UNDERLINE | CellFlags::DOUBLE_UNDERLINE);
                }
                22 => self.flags.remove(CellFlags::BOLD | CellFlags::DIM),
                23 => self.flags.remove(CellFlags::ITALIC),
                24 => self.flags.remove(ALL_UNDERLINE_FLAGS),
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

                53 => self.flags.insert(CellFlags::OVERLINE),
                55 => self.flags.remove(CellFlags::OVERLINE),

                58 => {
                    if let Some(color) = parse_extended_color(&mut iter) {
                        self.set_underline_color(Some(color));
                    }
                }
                59 => self.set_underline_color(None),

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

fn parse_colon_color(sub: &[u16]) -> Option<Color> {
    match sub.first().copied()? {
        2 => {
            // ITU T.416 allows both `2:r:g:b` (4 items) and `2:colorspace:r:g:b` (5+ items)
            if sub.len() >= 5 {
                Some(Color::Rgb(sub[2] as u8, sub[3] as u8, sub[4] as u8))
            } else if sub.len() == 4 {
                Some(Color::Rgb(sub[1] as u8, sub[2] as u8, sub[3] as u8))
            } else {
                None
            }
        }
        5 => sub.get(1).map(|&idx| Color::Indexed(idx as u8)),
        _ => None,
    }
}

fn parse_extended_color<'a>(iter: &mut impl Iterator<Item = &'a [u16]>) -> Option<Color> {
    let mode_param = iter.next()?;
    if mode_param.len() > 1 {
        return parse_colon_color(mode_param);
    }
    match mode_param.first().copied()? {
        2 => {
            let r = iter.next()?.first().copied()? as u8;
            let g = iter.next()?.first().copied()? as u8;
            let b = iter.next()?.first().copied()? as u8;
            Some(Color::Rgb(r, g, b))
        }
        5 => {
            let idx = iter.next()?.first().copied()? as u8;
            Some(Color::Indexed(idx))
        }
        _ => None,
    }
}

impl Perform for TerminalState {
    fn print(&mut self, c: char) {
        let mapped = if self.active_charset_mode() == Charset::DecSpecialGraphics {
            map_dec_special_graphics(c)
        } else {
            c
        };
        self.last_printed_char = Some(mapped);
        self.write_graphic_char(mapped);
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0B | 0x0C => self.grid.active_mut().linefeed_with_bg(self.bg),
            b'\r' => {
                let active = self.grid.active_mut();
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
            }
            b'\t' => {
                self.grid.active_mut().advance_tab(1);
            }
            0x08 => self.grid.active_mut().move_backward(1),
            0x0E => self.active_charset = 1, // SO (Shift Out -> G1)
            0x0F => self.active_charset = 0, // SI (Shift In -> G0)
            0x07 => {}                       // Bell
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

    fn osc_dispatch(&mut self, params: &[&[u8]], bell_terminated: bool) {
        if params.is_empty() {
            return;
        }

        let terminator = if bell_terminated { "\x07" } else { "\x1b\\" };

        match params[0] {
            b"0" | b"2" => {
                if params.len() > 1 {
                    if let Ok(title) = std::str::from_utf8(params[1]) {
                        self.title = title.to_string();
                    }
                }
            }
            b"4" => {
                // OSC 4 ; index ; spec [; index ; spec ...]
                for pair in params[1..].chunks(2) {
                    if pair.len() < 2 {
                        break;
                    }
                    let Ok(idx_str) = std::str::from_utf8(pair[0]) else {
                        continue;
                    };
                    let Ok(idx) = idx_str.trim().parse::<u8>() else {
                        continue;
                    };
                    let Ok(spec) = std::str::from_utf8(pair[1]) else {
                        continue;
                    };
                    if spec == "?" {
                        let (r, g, b) = self
                            .dynamic_palette
                            .get(&idx)
                            .copied()
                            .unwrap_or_else(|| default_indexed_rgb(idx));
                        let rgb_str = format_osc_rgb(r, g, b);
                        let resp = format!("\x1b]4;{idx};{rgb_str}{terminator}");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    } else if let Some(rgb) = parse_osc_color_spec(spec) {
                        self.dynamic_palette.insert(idx, rgb);
                    }
                }
            }
            b"7" => {
                if params.len() > 1 {
                    if let Ok(uri) = std::str::from_utf8(params[1]) {
                        if let Some(path) = parse_osc7_uri(uri) {
                            self.current_working_directory = Some(path);
                        }
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
            b"10" | b"11" | b"12" => {
                // Dynamic color query or set (used by neovim, fish, zsh, lipgloss/termenv)
                if let Some(&arg) = params.get(1) {
                    let code = std::str::from_utf8(params[0]).unwrap_or("10");
                    if arg == b"?" {
                        let (r, g, b) = match params[0] {
                            b"10" => self.dynamic_fg.unwrap_or((0xf5, 0xf5, 0xf7)),
                            b"11" => self.dynamic_bg.unwrap_or((0x1a, 0x1b, 0x1e)),
                            _ => self.dynamic_cursor.unwrap_or((0xf5, 0xf5, 0xf7)),
                        };
                        let rgb_str = format_osc_rgb(r, g, b);
                        let resp = format!("\x1b]{code};{rgb_str}{terminator}");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    } else if let Ok(spec) = std::str::from_utf8(arg) {
                        if let Some(rgb) = parse_osc_color_spec(spec) {
                            match params[0] {
                                b"10" => self.dynamic_fg = Some(rgb),
                                b"11" => self.dynamic_bg = Some(rgb),
                                _ => self.dynamic_cursor = Some(rgb),
                            }
                        }
                    }
                }
            }
            b"104" => {
                // Reset dynamic palette colors (specific indices or all if none specified)
                if params.len() <= 1 || (params.len() == 2 && params[1].is_empty()) {
                    self.dynamic_palette.clear();
                } else {
                    for &p in &params[1..] {
                        if let Ok(s) = std::str::from_utf8(p) {
                            if let Ok(idx) = s.trim().parse::<u8>() {
                                self.dynamic_palette.remove(&idx);
                            }
                        }
                    }
                }
            }
            b"110" => self.dynamic_fg = None,
            b"111" => self.dynamic_bg = None,
            b"112" => self.dynamic_cursor = None,
            b"52" => {
                if params.len() >= 3 && params[2] != b"?" {
                    use base64::Engine;
                    let cleaned: Vec<u8> = params[2]
                        .iter()
                        .copied()
                        .filter(|b| !b.is_ascii_whitespace())
                        .collect();
                    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(&cleaned)
                    {
                        if let Ok(text) = String::from_utf8(decoded) {
                            self.pending_clipboard = Some(text);
                        }
                    }
                }
            }
            _ => {
                // Check Kitty Graphics via OSC or APC
                if let Ok(cmd) = std::str::from_utf8(params[0]) {
                    if let Some(cmd_args) = cmd.strip_prefix('G') {
                        let header = cmd_args.split(';').next().unwrap_or("");
                        let is_query = header.split(',').any(|kv| kv == "a=q");
                        if is_query {
                            let quiet = header
                                .split(',')
                                .find_map(|kv| kv.strip_prefix("q="))
                                .unwrap_or("0");
                            if quiet != "2" {
                                if let Some(id_val) =
                                    header.split(',').find_map(|kv| kv.strip_prefix("i="))
                                {
                                    let resp = format!("\x1b_Gi={id_val};OK\x1b\\");
                                    self.pending_responses.extend_from_slice(resp.as_bytes());
                                } else {
                                    self.pending_responses.extend_from_slice(b"\x1b_G;OK\x1b\\");
                                }
                            }
                        } else if let Some((w, h, rgba, z)) =
                            self.kitty_parser.parse_command(cmd_args)
                        {
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
            'm' if intermediates.is_empty() => self.parse_sgr(params),
            'm' if intermediates == b">" => {
                // xterm modifyOtherKeys: CSI > 4 ; Pv m (or CSI > 4 m to reset)
                let mut iter = params.iter();
                let res = iter.next().and_then(|p| p.first()).copied().unwrap_or(0);
                let val = iter.next().and_then(|p| p.first()).copied().unwrap_or(0);
                if res == 4 {
                    self.modify_other_keys = (val as u8).min(2);
                } else if res == 0 {
                    self.modify_other_keys = 0;
                }
            }
            'H' | 'f' if intermediates.is_empty() => {
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
                let target_row = if self.origin_mode {
                    let (top, bottom) = self.grid.active().scroll_region;
                    (top + (row as usize - 1)).min(bottom)
                } else {
                    (row as usize) - 1
                };
                self.grid
                    .active_mut()
                    .move_cursor(target_row, (col as usize) - 1);
            }
            'A' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .move_up((first_param as usize).max(1)),
            'B' | 'e' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .move_down((first_param as usize).max(1)),
            'C' | 'a' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .move_forward((first_param as usize).max(1)),
            'D' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .move_backward((first_param as usize).max(1)),
            'E' if intermediates.is_empty() => {
                let active = self.grid.active_mut();
                active.move_down((first_param as usize).max(1));
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
            }
            'F' if intermediates.is_empty() => {
                let active = self.grid.active_mut();
                active.move_up((first_param as usize).max(1));
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
            }
            'G' | '`' if intermediates.is_empty() => {
                let col = (first_param as usize).max(1) - 1;
                let active = self.grid.active_mut();
                active.cursor.col = col.min(active.cols.saturating_sub(1));
                active.cursor.pending_wrap = false;
            }
            'd' if intermediates.is_empty() => {
                let rel_row = (first_param as usize).max(1) - 1;
                let origin_mode = self.origin_mode;
                let active = self.grid.active_mut();
                active.cursor.row = if origin_mode {
                    (active.scroll_region.0 + rel_row).min(active.scroll_region.1)
                } else {
                    rel_row.min(active.rows.saturating_sub(1))
                };
                active.cursor.pending_wrap = false;
            }
            'I' if intermediates.is_empty() => {
                // CHT: Cursor Forward Tabulation Ps tab stops
                let count = (first_param as usize).max(1);
                self.grid.active_mut().advance_tab(count);
            }
            'Z' if intermediates.is_empty() => {
                // CBT: Cursor Backward Tabulation Ps tab stops
                let count = (first_param as usize).max(1);
                self.grid.active_mut().backward_tab(count);
            }
            'g' if intermediates.is_empty() => {
                // TBC: Tabulation Clear (0 = current column, 3 = all tab stops)
                self.grid.active_mut().clear_tab_stop(first_param as usize);
            }
            'b' if intermediates.is_empty() => {
                // REP: Repeat preceding graphic character Ps times
                if let Some(c) = self.last_printed_char {
                    let max_repeat = self.grid.active().rows * self.grid.active().cols;
                    let count = (first_param as usize).max(1).min(max_repeat);
                    for _ in 0..count {
                        self.write_graphic_char(c);
                    }
                }
            }
            '@' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .insert_chars((first_param as usize).max(1), self.bg),
            'P' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .delete_chars((first_param as usize).max(1), self.bg),
            'X' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .erase_chars((first_param as usize).max(1), self.bg),
            'L' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .insert_lines((first_param as usize).max(1), self.bg),
            'M' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .delete_lines((first_param as usize).max(1), self.bg),
            'J' if intermediates.is_empty() || (is_private && intermediates.len() == 1) => self
                .grid
                .active_mut()
                .erase_in_display(first_param as usize, self.bg),
            'K' if intermediates.is_empty() || (is_private && intermediates.len() == 1) => self
                .grid
                .active_mut()
                .erase_in_line(first_param as usize, self.bg),
            'S' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .scroll_up_with_bg((first_param as usize).max(1), self.bg),
            'T' if intermediates.is_empty() => self
                .grid
                .active_mut()
                .scroll_down_with_bg((first_param as usize).max(1), self.bg),
            's' if intermediates.is_empty() => self.save_cursor(),
            'u' if intermediates.is_empty() => self.restore_cursor(),
            'u' if intermediates == b">" => {
                // Kitty Keyboard Protocol: push flags onto stack (CSI > flags u)
                if self.kitty_keyboard_stack.len() >= 16 {
                    self.kitty_keyboard_stack.remove(0);
                }
                self.kitty_keyboard_stack.push(first_param);
            }
            'u' if intermediates == b"<" => {
                // Kitty Keyboard Protocol: pop count entries from stack (CSI < count u)
                let count = (first_param as usize).max(1);
                let new_len = self.kitty_keyboard_stack.len().saturating_sub(count);
                self.kitty_keyboard_stack.truncate(new_len);
            }
            'u' if intermediates == b"=" => {
                // Kitty Keyboard Protocol: set/modify current flags (CSI = flags ; mode u)
                let mut iter = params.iter();
                let flags = iter.next().and_then(|p| p.first()).copied().unwrap_or(0);
                let mode = iter.next().and_then(|p| p.first()).copied().unwrap_or(1);
                let cur = self.kitty_keyboard_flags();
                let next = match mode {
                    2 => cur | flags,
                    3 => cur & !flags,
                    _ => flags,
                };
                if let Some(last) = self.kitty_keyboard_stack.last_mut() {
                    *last = next;
                } else {
                    self.kitty_keyboard_stack.push(next);
                }
            }
            'u' if intermediates == b"?" => {
                // Kitty Keyboard Protocol: query current flags (CSI ? u -> CSI ? flags u)
                let flags = self.kitty_keyboard_flags();
                let resp = format!("\x1b[?{flags}u");
                self.pending_responses.extend_from_slice(resp.as_bytes());
            }
            'r' if intermediates.is_empty() => {
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
                    let active = self.grid.active_mut();
                    active.scroll_region = (top - 1, bottom - 1);
                    active.cursor.row = top - 1;
                    active.cursor.col = 0;
                    active.cursor.pending_wrap = false;
                }
            }
            'c' if intermediates.is_empty() => {
                // DA1: Primary Device Attributes (VT220 Level 2 + Sixel (4) + ANSI Color (22))
                if first_param == 0 {
                    self.pending_responses.extend_from_slice(b"\x1b[?62;4;22c");
                }
            }
            'c' if intermediates == b">" => {
                // DA2: Secondary Device Attributes (xterm version 350)
                if first_param == 0 {
                    self.pending_responses.extend_from_slice(b"\x1b[>41;350;0c");
                }
            }
            'n' if intermediates.is_empty() => match first_param {
                5 => {
                    // DSR: Operating Status Report -> Terminal OK
                    self.pending_responses.extend_from_slice(b"\x1b[0n");
                }
                6 => {
                    // CPR: Cursor Position Report (1-indexed, relative to scroll_region when DECOM is active)
                    let active = self.grid.active();
                    let row = if self.origin_mode {
                        active.cursor.row.saturating_sub(active.scroll_region.0) + 1
                    } else {
                        active.cursor.row + 1
                    };
                    let col = active.cursor.col + 1;
                    let resp = format!("\x1b[{row};{col}R");
                    self.pending_responses.extend_from_slice(resp.as_bytes());
                }
                _ => {}
            },
            'n' if is_private && intermediates.len() == 1 => {
                if first_param == 6 {
                    // DECXCPR: Extended Cursor Position Report (1-indexed)
                    let active = self.grid.active();
                    let row = if self.origin_mode {
                        active.cursor.row.saturating_sub(active.scroll_region.0) + 1
                    } else {
                        active.cursor.row + 1
                    };
                    let col = active.cursor.col + 1;
                    let resp = format!("\x1b[?{row};{col}R");
                    self.pending_responses.extend_from_slice(resp.as_bytes());
                }
            }
            'p' if intermediates == b"?$" => {
                // DECRQM: Request DEC Private Mode
                let pm = match first_param {
                    1 => {
                        if self.application_cursor_keys {
                            1
                        } else {
                            2
                        }
                    }
                    6 => {
                        if self.origin_mode {
                            1
                        } else {
                            2
                        }
                    }
                    7 => {
                        if self.auto_wrap {
                            1
                        } else {
                            2
                        }
                    }
                    25 => {
                        if self.grid.active().cursor.visible {
                            1
                        } else {
                            2
                        }
                    }
                    47 | 1047 | 1049 => {
                        if self.grid.is_alternate {
                            1
                        } else {
                            2
                        }
                    }
                    1000 => {
                        if self.mouse_protocol == MouseProtocol::Normal {
                            1
                        } else {
                            2
                        }
                    }
                    1002 => {
                        if self.mouse_protocol == MouseProtocol::ButtonEvent {
                            1
                        } else {
                            2
                        }
                    }
                    1003 => {
                        if self.mouse_protocol == MouseProtocol::AnyEvent {
                            1
                        } else {
                            2
                        }
                    }
                    1004 => {
                        if self.focus_events {
                            1
                        } else {
                            2
                        }
                    }
                    1006 => {
                        if self.mouse_format == MouseFormat::Sgr {
                            1
                        } else {
                            2
                        }
                    }
                    2004 => {
                        if self.bracketed_paste {
                            1
                        } else {
                            2
                        }
                    }
                    2026 => {
                        if self.synchronized_output {
                            1
                        } else {
                            2
                        }
                    }
                    _ => 0,
                };
                let resp = format!("\x1b[?{first_param};{pm}$y");
                self.pending_responses.extend_from_slice(resp.as_bytes());
            }
            'p' if intermediates == b"$" => {
                // DECRQM: Request ANSI Mode
                let pm = match first_param {
                    4 => {
                        if self.insert_mode {
                            1
                        } else {
                            2
                        }
                    }
                    _ => 0,
                };
                let resp = format!("\x1b[{first_param};{pm}$y");
                self.pending_responses.extend_from_slice(resp.as_bytes());
            }
            'h' => {
                for param in params.iter() {
                    for &mode in param {
                        if is_private && intermediates.len() == 1 {
                            match mode {
                                1 => self.application_cursor_keys = true,
                                6 => {
                                    self.origin_mode = true;
                                    let active = self.grid.active_mut();
                                    active.cursor.row = active.scroll_region.0;
                                    active.cursor.col = 0;
                                    active.cursor.pending_wrap = false;
                                }
                                7 => self.auto_wrap = true,
                                25 => self.grid.active_mut().cursor.visible = true,
                                47 | 1047 | 1049 => {
                                    if mode == 1049 {
                                        self.save_cursor();
                                    }
                                    self.grid.set_alternate(true);
                                }
                                2004 => self.bracketed_paste = true,
                                2026 => self.synchronized_output = true,
                                1000 => self.mouse_protocol = MouseProtocol::Normal,
                                1002 => self.mouse_protocol = MouseProtocol::ButtonEvent,
                                1003 => self.mouse_protocol = MouseProtocol::AnyEvent,
                                1004 => self.focus_events = true,
                                1006 => self.mouse_format = MouseFormat::Sgr,
                                _ => {}
                            }
                        } else if intermediates.is_empty() && mode == 4 {
                            self.insert_mode = true;
                        }
                    }
                }
            }
            'l' => {
                for param in params.iter() {
                    for &mode in param {
                        if is_private && intermediates.len() == 1 {
                            match mode {
                                1 => self.application_cursor_keys = false,
                                6 => {
                                    self.origin_mode = false;
                                    let active = self.grid.active_mut();
                                    active.cursor.row = 0;
                                    active.cursor.col = 0;
                                    active.cursor.pending_wrap = false;
                                }
                                7 => {
                                    self.auto_wrap = false;
                                    self.grid.active_mut().cursor.pending_wrap = false;
                                }
                                25 => self.grid.active_mut().cursor.visible = false,
                                47 | 1047 => {
                                    self.grid.set_alternate(false);
                                    self.kitty_keyboard_stack.clear();
                                    self.modify_other_keys = 0;
                                }
                                1049 => {
                                    self.grid.set_alternate(false);
                                    self.restore_cursor();
                                    self.kitty_keyboard_stack.clear();
                                    self.modify_other_keys = 0;
                                }
                                2004 => self.bracketed_paste = false,
                                2026 => self.synchronized_output = false,
                                1000 | 1002 | 1003 => self.mouse_protocol = MouseProtocol::None,
                                1004 => self.focus_events = false,
                                1006 => self.mouse_format = MouseFormat::X10,
                                _ => {}
                            }
                        } else if intermediates.is_empty() && mode == 4 {
                            self.insert_mode = false;
                        }
                    }
                }
            }
            'q' if intermediates == b" " => match first_param {
                0..=2 => self.grid.active_mut().cursor.shape = CursorShape::Block,
                3 | 4 => self.grid.active_mut().cursor.shape = CursorShape::Underline,
                5 | 6 => self.grid.active_mut().cursor.shape = CursorShape::Beam,
                _ => {}
            },
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, byte: u8) {
        if intermediates == b"(" {
            match byte {
                b'0' => self.g0_charset = Charset::DecSpecialGraphics,
                b'B' => self.g0_charset = Charset::Ascii,
                _ => {}
            }
            return;
        }
        if intermediates == b")" {
            match byte {
                b'0' => self.g1_charset = Charset::DecSpecialGraphics,
                b'B' => self.g1_charset = Charset::Ascii,
                _ => {}
            }
            return;
        }
        if !intermediates.is_empty() {
            return;
        }

        match byte {
            b'7' => self.save_cursor(),
            b'8' => self.restore_cursor(),
            b'H' => self.grid.active_mut().set_tab_stop(),
            b'M' => {
                let bg = self.bg;
                let active = self.grid.active_mut();
                if active.cursor.row == active.scroll_region.0 {
                    active.scroll_down_with_bg(1, bg);
                } else {
                    active.move_up(1);
                }
            }
            b'D' => self.grid.active_mut().linefeed_with_bg(self.bg),
            b'E' => self.grid.active_mut().newline_with_bg(self.bg),
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

fn format_osc_rgb(r: u8, g: u8, b: u8) -> String {
    format!("rgb:{r:02x}{r:02x}/{g:02x}{g:02x}/{b:02x}{b:02x}")
}

fn parse_osc_color_spec(spec: &str) -> Option<(u8, u8, u8)> {
    let s = spec.trim();
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            6 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
                let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
                Some((r, g, b))
            }
            12 => {
                let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
                let g = u8::from_str_radix(&hex[4..6], 16).ok()?;
                let b = u8::from_str_radix(&hex[8..10], 16).ok()?;
                Some((r, g, b))
            }
            _ => None,
        }
    } else if let Some(rgb_part) = s.strip_prefix("rgb:") {
        let mut parts = rgb_part.split('/');
        let r = parse_xparsecolor_channel(parts.next()?)?;
        let g = parse_xparsecolor_channel(parts.next()?)?;
        let b = parse_xparsecolor_channel(parts.next()?)?;
        Some((r, g, b))
    } else {
        None
    }
}

fn parse_xparsecolor_channel(ch: &str) -> Option<u8> {
    let val = u16::from_str_radix(ch, 16).ok()?;
    match ch.len() {
        1 => Some((val as u8) * 17),
        2 => Some(val as u8),
        3 => Some((val >> 4) as u8),
        4 => Some((val >> 8) as u8),
        _ => None,
    }
}

fn default_indexed_rgb(idx: u8) -> (u8, u8, u8) {
    match idx {
        0 => (0x1a, 0x1b, 0x1e),
        1 => (0xff, 0x55, 0x55),
        2 => (0x50, 0xfa, 0x7b),
        3 => (0xf1, 0xfa, 0x8c),
        4 => (0xbd, 0x93, 0xf9),
        5 => (0xff, 0x79, 0xc6),
        6 => (0x8b, 0xe9, 0xfd),
        7 => (0xbf, 0xbf, 0xbf),
        8 => (0x4d, 0x4d, 0x4d),
        9 => (0xff, 0x6e, 0x67),
        10 => (0x5a, 0xf7, 0x8e),
        11 => (0xf4, 0xf9, 0x9d),
        12 => (0xca, 0xa9, 0xfa),
        13 => (0xff, 0x92, 0xd0),
        14 => (0x9a, 0xed, 0xfe),
        15 => (0xe6, 0xe6, 0xe6),
        16..=231 => {
            let n = idx - 16;
            let r_idx = n / 36;
            let g_idx = (n % 36) / 6;
            let b_idx = n % 6;
            let to_u8 = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (to_u8(r_idx), to_u8(g_idx), to_u8(b_idx))
        }
        232..=255 => {
            let v = 8 + (idx - 232) * 10;
            (v, v, v)
        }
    }
}

/// Map VT100 DEC Special Graphics character set (`ESC ( 0`) bytes `0x60..=0x7E`
/// to their corresponding Unicode box-drawing and symbol characters.
fn map_dec_special_graphics(c: char) -> char {
    match c {
        '`' => '◆',
        'a' => '▒',
        'b' => '␉',
        'c' => '␌',
        'd' => '␍',
        'e' => '␊',
        'f' => '°',
        'g' => '±',
        'h' => '␤',
        'i' => '␋',
        'j' => '┘',
        'k' => '┐',
        'l' => '┌',
        'm' => '└',
        'n' => '┼',
        'o' => '⎺',
        'p' => '⎻',
        'q' => '─',
        'r' => '⎼',
        's' => '⎽',
        't' => '├',
        'u' => '┤',
        'v' => '┴',
        'w' => '┬',
        'x' => '│',
        'y' => '≤',
        'z' => '≥',
        '{' => 'π',
        '|' => '≠',
        '}' => '£',
        '~' => '·',
        _ => c,
    }
}

/// Parse an OSC 7 file URI (e.g., `file://hostname/Users/name/path%20with%20spaces`)
/// into a decoded local filesystem path string.
fn parse_osc7_uri(uri: &str) -> Option<String> {
    let raw_path = if let Some(after_scheme) = uri.strip_prefix("file://") {
        // Skip hostname up to first '/'
        let idx = after_scheme.find('/')?;
        &after_scheme[idx..]
    } else if uri.starts_with('/') {
        uri
    } else {
        return None;
    };

    let mut bytes = Vec::with_capacity(raw_path.len());
    let mut chars = raw_path.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let h1 = chars.next()?;
            let h2 = chars.next()?;
            let hex = [h1, h2];
            if let Ok(s) = std::str::from_utf8(&hex) {
                if let Ok(val) = u8::from_str_radix(s, 16) {
                    bytes.push(val);
                    continue;
                }
            }
            bytes.push(b);
            bytes.push(h1);
            bytes.push(h2);
        } else {
            bytes.push(b);
        }
    }

    String::from_utf8(bytes).ok()
}
