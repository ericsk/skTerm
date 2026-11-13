//! VT100, xterm, 24-bit TrueColor ANSI, Sixel, and Kitty Graphics escape sequence handler.

use crate::graphics::{
    composite_rgba_patch, decode_kitty_pixels, load_transmission_payload, AnimationFrame,
    GraphicImage, KittyControlParams, KittyGraphicsParser, KittyParseStep, SixelDecoder,
    VirtualPlacement,
};
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

/// Active Device Control String (DCS) sub-protocol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DcsMode {
    #[default]
    None,
    Sixel,
    XtGetTcap,
    DecRqss,
}

/// Program lifecycle state reported via `OSC 7501` (`ESC ] 7501 ; ... ST/BEL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProgramStatusState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    Clear,
}

impl ProgramStatusState {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "idle" => Some(Self::Idle),
            "working" => Some(Self::Working),
            "done" => Some(Self::Done),
            "blocked" => Some(Self::Blocked),
            "error" => Some(Self::Error),
            "clear" => Some(Self::Clear),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Done => "done",
            Self::Blocked => "blocked",
            Self::Error => "error",
            Self::Clear => "clear",
        }
    }

    /// Selection priority when multiple status records exist in the same pane.
    /// Higher value wins (`Blocked > Error > Working > Done > Idle`).
    pub fn priority(&self) -> u8 {
        match self {
            Self::Blocked => 4,
            Self::Error => 3,
            Self::Working => 2,
            Self::Done => 1,
            Self::Idle | Self::Clear => 0,
        }
    }
}

/// Optional sub-kind reported via `OSC 7501` (`kind=...`), refining [`ProgramStatusState::Blocked`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ProgramStatusKind {
    #[default]
    None,
    Permission,
    Question,
    Auth,
}

impl ProgramStatusKind {
    pub fn parse(s: &str) -> Self {
        match s {
            "permission" => Self::Permission,
            "question" => Self::Question,
            "auth" => Self::Auth,
            _ => Self::None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Permission => "permission",
            Self::Question => "question",
            Self::Auth => "auth",
        }
    }
}

/// A single program status entry stored per `id` in [`TerminalState`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramStatusRecord {
    /// Program/session status identifier (`None` for the default unnamed slot).
    pub id: Option<String>,
    /// Current lifecycle state (`Idle`, `Working`, `Done`, `Blocked`, `Error`).
    pub state: ProgramStatusState,
    /// Sub-kind (`Permission`, `Question`, `Auth`, `None`).
    pub kind: ProgramStatusKind,
    /// Application identifier (e.g. `"agy"`, `"jetski"`, `"claude"`, `"gemini"`).
    pub app: Option<String>,
    /// Decoded UTF-8 session/task title (from Base64 `title=`).
    pub title: Option<String>,
    /// Decoded UTF-8 status detail message (from Base64 `msg=`).
    pub msg: Option<String>,
    /// Optional progress percentage (`0..=100`, from `progress=`).
    pub progress: Option<u8>,
    /// Monotonic sequence number for tie-breaking between records of equal priority.
    pub seq: u64,
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
    pub title_stack: Vec<String>,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub auto_wrap: bool,
    pub origin_mode: bool,
    pub focus_events: bool,
    pub synchronized_output: bool,
    pub synchronized_output_since: Option<std::time::Instant>,
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
    pub dcs_mode: DcsMode,
    pub dcs_buffer: Vec<u8>,
    pub kitty_parser: KittyGraphicsParser,
    pub insert_mode: bool,
    pub current_working_directory: Option<String>,
    pub pending_clipboard: Option<String>,
    pub pending_responses: Vec<u8>,
    pub last_printed_char: Option<char>,
    pub g0_charset: Charset,
    pub g1_charset: Charset,
    pub active_charset: u8,
    pub program_statuses: HashMap<String, ProgramStatusRecord>,
    pub program_status_seq: u64,
    pub program_status_dirty: bool,
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
            title_stack: Vec::new(),
            application_cursor_keys: false,
            bracketed_paste: false,
            auto_wrap: true,
            origin_mode: false,
            focus_events: false,
            synchronized_output: false,
            synchronized_output_since: None,
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
            dcs_mode: DcsMode::None,
            dcs_buffer: Vec::new(),
            kitty_parser: KittyGraphicsParser::new(),
            insert_mode: false,
            current_working_directory: None,
            pending_clipboard: None,
            pending_responses: Vec::new(),
            last_printed_char: None,
            g0_charset: Charset::Ascii,
            g1_charset: Charset::Ascii,
            active_charset: 0,
            program_statuses: HashMap::new(),
            program_status_seq: 0,
            program_status_dirty: false,
        }
    }

    /// Return the highest-priority active OSC 7501 program status record in this terminal.
    ///
    /// Priority order: `Blocked > Error > Working > Done > Idle`, with ties broken by the
    /// most recently updated sequence number (`seq`).
    pub fn active_program_status(&self) -> Option<&ProgramStatusRecord> {
        self.program_statuses
            .values()
            .max_by_key(|rec| (rec.state.priority(), rec.seq))
    }

    pub fn reset_sgr(&mut self) {
        self.fg = Color::DefaultFg;
        self.bg = Color::DefaultBg;
        self.underline_color = None;
        self.current_underline_color_id = None;
        self.flags = CellFlags::empty();
    }

    /// Perform DECSTR (`CSI ! p`) Soft Terminal Reset without clearing the screen.
    pub fn soft_reset(&mut self) {
        self.reset_sgr();
        self.application_cursor_keys = false;
        self.origin_mode = false;
        self.auto_wrap = true;
        self.insert_mode = false;
        self.synchronized_output = false;
        self.synchronized_output_since = None;
        self.g0_charset = Charset::Ascii;
        self.g1_charset = Charset::Ascii;
        self.active_charset = 0;
        let active = self.grid.active_mut();
        active.cursor.visible = true;
        active.cursor.pending_wrap = false;
        active.scroll_region = (0, active.rows.saturating_sub(1));
        active.saved_cursor = None;
    }

    /// Returns true if DEC Private Mode `?2026` (Synchronized Output) is currently active
    /// and within its 100ms safety timeout window.
    pub fn is_synchronized_output_active(&self) -> bool {
        self.synchronized_output
            && self
                .synchronized_output_since
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(100))
    }

    pub fn kitty_keyboard_flags(&self) -> u16 {
        self.kitty_keyboard_stack.last().copied().unwrap_or(0)
    }

    fn format_decrqss_sgr(&self) -> String {
        let mut parts: Vec<String> = vec!["0".to_string()];
        if self.flags.contains(CellFlags::BOLD) {
            parts.push("1".to_string());
        }
        if self.flags.contains(CellFlags::DIM) {
            parts.push("2".to_string());
        }
        if self.flags.contains(CellFlags::ITALIC) {
            parts.push("3".to_string());
        }
        if self.flags.contains(CellFlags::CURLY_UNDERLINE) {
            parts.push("4:3".to_string());
        } else if self.flags.contains(CellFlags::DOUBLE_UNDERLINE) {
            parts.push("4:2".to_string());
        } else if self.flags.contains(CellFlags::DOTTED_UNDERLINE) {
            parts.push("4:4".to_string());
        } else if self.flags.contains(CellFlags::DASHED_UNDERLINE) {
            parts.push("4:5".to_string());
        } else if self.flags.contains(CellFlags::UNDERLINE) {
            parts.push("4".to_string());
        }
        if self.flags.contains(CellFlags::BLINK) {
            parts.push("5".to_string());
        }
        if self.flags.contains(CellFlags::INVERSE) {
            parts.push("7".to_string());
        }
        if self.flags.contains(CellFlags::HIDDEN) {
            parts.push("8".to_string());
        }
        if self.flags.contains(CellFlags::STRIKETHROUGH) {
            parts.push("9".to_string());
        }
        if self.flags.contains(CellFlags::OVERLINE) {
            parts.push("53".to_string());
        }
        match self.fg {
            Color::DefaultFg | Color::DefaultBg => {}
            Color::Named(n) => parts.push(named_color_to_sgr(n, false).to_string()),
            Color::Indexed(idx) => parts.push(format!("38;5;{idx}")),
            Color::Rgb(r, g, b) => parts.push(format!("38;2;{r};{g};{b}")),
        }
        match self.bg {
            Color::DefaultFg | Color::DefaultBg => {}
            Color::Named(n) => parts.push(named_color_to_sgr(n, true).to_string()),
            Color::Indexed(idx) => parts.push(format!("48;5;{idx}")),
            Color::Rgb(r, g, b) => parts.push(format!("48;2;{r};{g};{b}")),
        }
        if let Some(ul) = self.underline_color {
            match ul {
                Color::Indexed(idx) => parts.push(format!("58;5;{idx}")),
                Color::Rgb(r, g, b) => parts.push(format!("58;2;{r};{g};{b}")),
                _ => {}
            }
        }
        parts.join(";")
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

fn named_color_to_sgr(color: NamedColor, is_bg: bool) -> u16 {
    let base = match color {
        NamedColor::Black => 30,
        NamedColor::Red => 31,
        NamedColor::Green => 32,
        NamedColor::Yellow => 33,
        NamedColor::Blue => 34,
        NamedColor::Magenta => 35,
        NamedColor::Cyan => 36,
        NamedColor::White => 37,
        NamedColor::BrightBlack => 90,
        NamedColor::BrightRed => 91,
        NamedColor::BrightGreen => 92,
        NamedColor::BrightYellow => 93,
        NamedColor::BrightBlue => 94,
        NamedColor::BrightMagenta => 95,
        NamedColor::BrightCyan => 96,
        NamedColor::BrightWhite => 97,
    };
    if is_bg {
        base + 10
    } else {
        base
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

    fn hook(&mut self, _params: &Params, intermediates: &[u8], _ignore: bool, action: char) {
        self.in_sixel = false;
        self.dcs_mode = DcsMode::None;
        self.dcs_buffer.clear();
        if action == 'q' {
            if intermediates.is_empty() {
                // Sixel Graphics DCS entry
                self.in_sixel = true;
                self.dcs_mode = DcsMode::Sixel;
                self.sixel_buffer.clear();
            } else if intermediates == b"+" {
                self.dcs_mode = DcsMode::XtGetTcap;
            } else if intermediates == b"$" {
                self.dcs_mode = DcsMode::DecRqss;
            }
        }
    }

    fn put(&mut self, byte: u8) {
        match self.dcs_mode {
            DcsMode::Sixel => self.sixel_buffer.push(byte),
            DcsMode::XtGetTcap | DcsMode::DecRqss => {
                if self.dcs_buffer.len() < 4096 {
                    self.dcs_buffer.push(byte);
                }
            }
            DcsMode::None => {}
        }
    }

    fn unhook(&mut self) {
        let mode = self.dcs_mode;
        self.dcs_mode = DcsMode::None;
        self.in_sixel = false;

        match mode {
            DcsMode::Sixel => {
                let mut decoder = SixelDecoder::new();
                if let Some((w_px, h_px, rgba)) = decoder.decode(&self.sixel_buffer) {
                    let cell_w = self.grid.cell_width_px.max(1.0);
                    let cell_h = self.grid.cell_height_px.max(1.0);
                    let w_cells = ((w_px as f32 / cell_w).ceil() as usize).max(1);
                    let h_cells = ((h_px as f32 / cell_h).ceil() as usize).max(1);

                    let id = self.grid.alloc_texture_id();

                    let cur_row = self.grid.active().cursor.row;
                    let cur_col = self.grid.active().cursor.col;

                    let img = GraphicImage {
                        id,
                        image_id: 0,
                        image_number: 0,
                        placement_id: 0,
                        row: cur_row as isize,
                        col: cur_col,
                        width_px: w_px,
                        height_px: h_px,
                        src_rect: [0, 0, w_px, h_px],
                        cell_offset: [0, 0],
                        display_cols: None,
                        display_rows: None,
                        width_cells: w_cells,
                        height_cells: h_cells,
                        rgba_data: Arc::new(rgba),
                        z_index: 0,
                    };

                    info!(
                        "Decoded Sixel image: {}x{} px (cells: {}x{})",
                        w_px, h_px, w_cells, h_cells
                    );
                    self.grid.active_mut().add_image(img);

                    // Advance cursor down across the image rows (scrolling if needed)
                    let bg = self.bg;
                    for _ in 0..h_cells {
                        self.grid.active_mut().linefeed_with_bg(bg);
                    }
                    self.grid.active_mut().cursor.col = 0;
                }
                self.sixel_buffer.clear();
            }
            DcsMode::XtGetTcap => {
                if let Ok(payload) = std::str::from_utf8(&self.dcs_buffer) {
                    for hex_key in payload.split(';') {
                        let hex_key = hex_key.trim();
                        if hex_key.is_empty() {
                            continue;
                        }
                        if let Some(cap_bytes) = decode_hex_bytes(hex_key) {
                            if let Ok(cap_name) = std::str::from_utf8(&cap_bytes) {
                                if let Some(val_bytes) = lookup_xtgettcap(cap_name) {
                                    let resp = if val_bytes.is_empty() {
                                        format!("\x1bP1+r{hex_key}\x1b\\")
                                    } else {
                                        let hex_val = encode_hex_bytes(val_bytes);
                                        format!("\x1bP1+r{hex_key}={hex_val}\x1b\\")
                                    };
                                    self.pending_responses.extend_from_slice(resp.as_bytes());
                                    continue;
                                }
                            }
                        }
                        let resp = format!("\x1bP0+r{hex_key}\x1b\\");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    }
                }
                self.dcs_buffer.clear();
            }
            DcsMode::DecRqss => {
                if let Ok(req) = std::str::from_utf8(&self.dcs_buffer) {
                    let resp = match req {
                        "m" => format!("\x1bP1$r{}m\x1b\\", self.format_decrqss_sgr()),
                        "r" => {
                            let (top, bottom) = self.grid.active().scroll_region;
                            format!("\x1bP1$r{};{}r\x1b\\", top + 1, bottom + 1)
                        }
                        " q" => {
                            let code = match self.grid.active().cursor.shape {
                                CursorShape::Block => 1,
                                CursorShape::Underline => 3,
                                CursorShape::Beam => 5,
                            };
                            format!("\x1bP1$r{code} q\x1b\\")
                        }
                        _ => String::from("\x1bP0$r\x1b\\"),
                    };
                    self.pending_responses.extend_from_slice(resp.as_bytes());
                }
                self.dcs_buffer.clear();
            }
            DcsMode::None => {}
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
            b"7501" => {
                if params.len() > 1 {
                    let payload_cow: std::borrow::Cow<'_, [u8]> = if params.len() == 2 {
                        std::borrow::Cow::Borrowed(params[1])
                    } else {
                        std::borrow::Cow::Owned(params[1..].join(&b';'))
                    };
                    let trimmed = trim_ascii_bytes(&payload_cow);
                    if trimmed == b"?" {
                        let resp = format!("\x1b]7501;?{terminator}");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    } else if let Ok(payload_str) = std::str::from_utf8(trimmed) {
                        if let Some(parsed) = parse_osc7501_payload(payload_str) {
                            self.apply_osc7501_update(parsed);
                        }
                    }
                }
            }
            _ => {
                // Fallback support for Kitty Graphics sent via OSC (`\x1b]G<keys>;<payload>\x1b\\`).
                // Note that `vte` splits OSC parameters on `;`, so rejoin `params` before parsing.
                if let Some(first_rest) = params[0].strip_prefix(b"G") {
                    if params.len() == 1 {
                        self.handle_kitty_graphics(first_rest);
                    } else {
                        let extra_len: usize = params[1..].iter().map(|p| 1 + p.len()).sum();
                        let mut joined = Vec::with_capacity(first_rest.len() + extra_len);
                        joined.extend_from_slice(first_rest);
                        for seg in &params[1..] {
                            joined.push(b';');
                            joined.extend_from_slice(seg);
                        }
                        self.handle_kitty_graphics(&joined);
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
                                2026 => {
                                    self.synchronized_output = true;
                                    self.synchronized_output_since =
                                        Some(std::time::Instant::now());
                                }
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
                                2026 => {
                                    self.synchronized_output = false;
                                    self.synchronized_output_since = None;
                                }
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
            'p' if intermediates == b"!" => {
                // DECSTR (Soft Terminal Reset)
                self.soft_reset();
            }
            'q' if intermediates == b" " => match first_param {
                0..=2 => self.grid.active_mut().cursor.shape = CursorShape::Block,
                3 | 4 => self.grid.active_mut().cursor.shape = CursorShape::Underline,
                5 | 6 => self.grid.active_mut().cursor.shape = CursorShape::Beam,
                _ => {}
            },
            'q' if intermediates == b">" => {
                // XTVERSION: CSI > q / CSI > 0 q -> DCS > | skTerm(<version>) ST
                if first_param == 0 {
                    self.pending_responses.extend_from_slice(
                        concat!("\x1bP>|skTerm(", env!("CARGO_PKG_VERSION"), ")\x1b\\").as_bytes(),
                    );
                }
            }
            't' if intermediates.is_empty() => {
                // XTWINOPS: Window manipulation queries and title stack
                match first_param {
                    14 => {
                        let rows = self.grid.active().rows;
                        let cols = self.grid.active().cols;
                        let ch = (self.grid.cell_height_px.round() as usize).max(1);
                        let cw = (self.grid.cell_width_px.round() as usize).max(1);
                        let resp = format!("\x1b[4;{};{}t", rows * ch, cols * cw);
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    }
                    16 => {
                        let ch = (self.grid.cell_height_px.round() as usize).max(1);
                        let cw = (self.grid.cell_width_px.round() as usize).max(1);
                        let resp = format!("\x1b[6;{ch};{cw}t");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    }
                    18 => {
                        let rows = self.grid.active().rows;
                        let cols = self.grid.active().cols;
                        let resp = format!("\x1b[8;{rows};{cols}t");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    }
                    22 => {
                        let sub = params
                            .iter()
                            .nth(1)
                            .and_then(|p| p.first())
                            .copied()
                            .unwrap_or(0);
                        if sub == 0 || sub == 2 {
                            if self.title_stack.len() >= 16 {
                                self.title_stack.remove(0);
                            }
                            self.title_stack.push(self.title.clone());
                        }
                    }
                    23 => {
                        let sub = params
                            .iter()
                            .nth(1)
                            .and_then(|p| p.first())
                            .copied()
                            .unwrap_or(0);
                        if sub == 0 || sub == 2 {
                            if let Some(prev) = self.title_stack.pop() {
                                self.title = prev;
                            }
                        }
                    }
                    _ => {}
                }
            }
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
                let had_status = !self.program_statuses.is_empty();
                *self = Self::new(rows, cols, max_scroll);
                self.program_status_dirty = had_status;
            }
            _ => {}
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedOsc7501 {
    id: Option<String>,
    state: ProgramStatusState,
    kind: ProgramStatusKind,
    app: Option<String>,
    title: Option<String>,
    msg: Option<String>,
    progress: Option<u8>,
}

impl TerminalState {
    fn apply_osc7501_update(&mut self, parsed: ParsedOsc7501) {
        const MAX_PROGRAM_STATUS_RECORDS: usize = 256;

        if parsed.state == ProgramStatusState::Clear {
            match parsed.id {
                None => {
                    if !self.program_statuses.is_empty() {
                        self.program_statuses.clear();
                        self.program_status_dirty = true;
                    }
                }
                Some(target_id) => {
                    let prev_len = self.program_statuses.len();
                    self.program_statuses.retain(|k, _| {
                        !(k == &target_id
                            || (k.starts_with(&target_id)
                                && k.as_bytes().get(target_id.len()) == Some(&b'/')))
                    });
                    if self.program_statuses.len() != prev_len {
                        self.program_status_dirty = true;
                    }
                }
            }
            return;
        }

        let key = parsed.id.clone().unwrap_or_default();
        self.program_status_seq = self.program_status_seq.wrapping_add(1);

        let inherited_app = parsed.app.or_else(|| self.resolve_inherited_app(&key));
        let inherited_title = parsed.title.or_else(|| {
            self.program_statuses
                .get(&key)
                .and_then(|r| r.title.clone())
        });

        let new_record = ProgramStatusRecord {
            id: parsed.id,
            state: parsed.state,
            kind: if parsed.state == ProgramStatusState::Blocked {
                parsed.kind
            } else {
                ProgramStatusKind::None
            },
            app: inherited_app,
            title: inherited_title,
            msg: parsed.msg,
            progress: parsed.progress,
            seq: self.program_status_seq,
        };

        let content_changed = match self.program_statuses.get(&key) {
            Some(existing) => {
                existing.state != new_record.state
                    || existing.kind != new_record.kind
                    || existing.app != new_record.app
                    || existing.title != new_record.title
                    || existing.msg != new_record.msg
                    || existing.progress != new_record.progress
            }
            None => true,
        };

        self.program_statuses.insert(key, new_record);
        if content_changed {
            self.program_status_dirty = true;
        }

        if self.program_statuses.len() > MAX_PROGRAM_STATUS_RECORDS {
            if let Some(oldest_key) = self
                .program_statuses
                .iter()
                .min_by_key(|(_, rec)| rec.seq)
                .map(|(k, _)| k.clone())
            {
                self.program_statuses.remove(&oldest_key);
                self.program_status_dirty = true;
            }
        }
    }

    fn resolve_inherited_app(&self, key: &str) -> Option<String> {
        if let Some(rec) = self.program_statuses.get(key) {
            if rec.app.is_some() {
                return rec.app.clone();
            }
        }
        let mut prefix = key;
        while let Some((parent, _)) = prefix.rsplit_once('/') {
            if let Some(rec) = self.program_statuses.get(parent) {
                if rec.app.is_some() {
                    return rec.app.clone();
                }
            }
            prefix = parent;
        }
        if !key.is_empty() {
            if let Some(rec) = self.program_statuses.get("") {
                if rec.app.is_some() {
                    return rec.app.clone();
                }
            }
        }
        None
    }

    /// Dispatch an Application Program Command (`ESC _ <apc_bytes> ST`).
    pub fn handle_apc(&mut self, apc_bytes: &[u8]) {
        if let Some(cmd_bytes) = apc_bytes.strip_prefix(b"G") {
            self.handle_kitty_graphics(cmd_bytes);
        }
    }

    /// Report an `ENOMEM` error when a single APC sequence exceeded `MAX_APC_BYTES`.
    pub fn handle_apc_overflow(&mut self, apc_prefix: &[u8]) {
        self.kitty_parser.reset();
        if let Some(cmd_prefix) = apc_prefix.strip_prefix(b"G") {
            let header_bytes = cmd_prefix.split(|&b| b == b';').next().unwrap_or(&[]);
            if let Ok(header) = std::str::from_utf8(header_bytes) {
                let mut params = KittyControlParams::default();
                params.apply_header(header);
                if let Some(resp) =
                    params.format_response(None, false, "ENOMEM:APC payload exceeds size limit")
                {
                    self.pending_responses.extend_from_slice(&resp);
                }
            }
        }
    }

    /// Parse and execute a Kitty Graphics Protocol command body (`<control>;<payload>`).
    pub fn handle_kitty_graphics(&mut self, cmd_bytes: &[u8]) {
        match self.kitty_parser.feed_chunk(cmd_bytes) {
            KittyParseStep::NeedMoreChunks => {}
            KittyParseStep::Error {
                params,
                code,
                message,
            } => {
                let err_msg = format!("{code}:{message}");
                if let Some(resp) = params.format_response(None, false, &err_msg) {
                    self.pending_responses.extend_from_slice(&resp);
                }
            }
            KittyParseStep::Complete { params, payload } => match params.action {
                'q' => {
                    let check_res = if payload.is_empty()
                        && params.medium == 'd'
                        && params.data_width == 0
                        && params.data_height == 0
                    {
                        Ok(())
                    } else {
                        load_transmission_payload(&params, payload)
                            .and_then(|raw| decode_kitty_pixels(&params, raw).map(|_| ()))
                    };
                    match check_res {
                        Ok(()) => {
                            if let Some(resp) = params.format_response(None, true, "OK") {
                                self.pending_responses.extend_from_slice(&resp);
                            }
                        }
                        Err((code, msg)) => {
                            let err_msg = format!("{code}:{msg}");
                            if let Some(resp) = params.format_response(None, false, &err_msg) {
                                self.pending_responses.extend_from_slice(&resp);
                            }
                        }
                    }
                }
                't' | 'T' => {
                    let res = load_transmission_payload(&params, payload)
                        .and_then(|raw| decode_kitty_pixels(&params, raw));
                    match res {
                        Ok((w, h, rgba)) => {
                            let image_id = if params.image_id != 0 {
                                params.image_id
                            } else {
                                self.grid.alloc_internal_kitty_id()
                            };
                            self.grid
                                .store_kitty_image(image_id, params.image_number, w, h, rgba);
                            if params.action == 'T' {
                                if let Err((code, msg)) = self.execute_kitty_put(&params, image_id)
                                {
                                    let err_msg = format!("{code}:{msg}");
                                    if let Some(resp) =
                                        params.format_response(Some(image_id), false, &err_msg)
                                    {
                                        self.pending_responses.extend_from_slice(&resp);
                                    }
                                    return;
                                }
                            }
                            if let Some(resp) = params.format_response(Some(image_id), true, "OK") {
                                self.pending_responses.extend_from_slice(&resp);
                            }
                        }
                        Err((code, msg)) => {
                            let err_msg = format!("{code}:{msg}");
                            if let Some(resp) = params.format_response(None, false, &err_msg) {
                                self.pending_responses.extend_from_slice(&resp);
                            }
                        }
                    }
                }
                'p' => {
                    let resolved_id = if params.image_id != 0 {
                        Some(params.image_id)
                    } else if params.image_number != 0 {
                        self.grid
                            .kitty_number_to_id
                            .get(&params.image_number)
                            .copied()
                    } else {
                        None
                    };
                    let Some(image_id) = resolved_id else {
                        let err = if params.image_id == 0 && params.image_number == 0 {
                            "EINVAL:Put command requires image ID or number"
                        } else {
                            "ENOENT:Put command refers to non-existent image number"
                        };
                        if let Some(resp) = params.format_response(None, false, err) {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                        return;
                    };

                    match self.execute_kitty_put(&params, image_id) {
                        Ok(()) => {
                            if let Some(resp) = params.format_response(Some(image_id), true, "OK") {
                                self.pending_responses.extend_from_slice(&resp);
                            }
                        }
                        Err((code, msg)) => {
                            let err_msg = format!("{code}:{msg}");
                            if let Some(resp) =
                                params.format_response(Some(image_id), false, &err_msg)
                            {
                                self.pending_responses.extend_from_slice(&resp);
                            }
                        }
                    }
                }
                'd' => {
                    self.execute_kitty_delete(&params);
                }
                'f' => match self.execute_kitty_frame(&params, payload) {
                    Ok(image_id) => {
                        if let Some(resp) = params.format_response(Some(image_id), true, "OK") {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                    }
                    Err((code, msg)) => {
                        let err_msg = format!("{code}:{msg}");
                        if let Some(resp) = params.format_response(None, false, &err_msg) {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                    }
                },
                'c' => match self.execute_kitty_compose(&params) {
                    Ok(image_id) => {
                        if let Some(resp) = params.format_response(Some(image_id), true, "OK") {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                    }
                    Err((code, msg)) => {
                        let err_msg = format!("{code}:{msg}");
                        if let Some(resp) = params.format_response(None, false, &err_msg) {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                    }
                },
                'a' => match self.execute_kitty_animate(&params) {
                    Ok(image_id) => {
                        if let Some(resp) = params.format_response(Some(image_id), true, "OK") {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                    }
                    Err((code, msg)) => {
                        let err_msg = format!("{code}:{msg}");
                        if let Some(resp) = params.format_response(None, false, &err_msg) {
                            self.pending_responses.extend_from_slice(&resp);
                        }
                    }
                },
                _ => {
                    if let Some(resp) =
                        params.format_response(None, false, "EINVAL:Unsupported Kitty action")
                    {
                        self.pending_responses.extend_from_slice(&resp);
                    }
                }
            },
        }
    }

    fn execute_kitty_put(
        &mut self,
        params: &KittyControlParams,
        image_id: u32,
    ) -> Result<(), (&'static str, &'static str)> {
        let Some(stored) = self.grid.kitty_images.get(&image_id).cloned() else {
            return Err(("ENOENT", "Put command refers to non-existent image"));
        };

        let sx = params.x.min(stored.width_px.saturating_sub(1));
        let sy = params.y.min(stored.height_px.saturating_sub(1));
        let sw = if params.w == 0 {
            stored.width_px.saturating_sub(sx)
        } else {
            params.w.min(stored.width_px.saturating_sub(sx))
        }
        .max(1);
        let sh = if params.h == 0 {
            stored.height_px.saturating_sub(sy)
        } else {
            params.h.min(stored.height_px.saturating_sub(sy))
        }
        .max(1);

        let display_cols = (params.cols > 0).then_some(params.cols);
        let display_rows = (params.rows > 0).then_some(params.rows);

        if params.unicode_placeholder {
            let vp = VirtualPlacement {
                image_id,
                placement_id: params.placement_id,
                cols: display_cols,
                rows: display_rows,
                src_rect: [sx, sy, sw, sh],
            };
            self.grid
                .virtual_placements
                .insert((image_id, params.placement_id), vp);
            self.grid.active_mut().images_dirty = true;
            return Ok(());
        }

        // Flush pending auto-wrap if placing directly at the cursor with cursor movement enabled.
        if params.parent_id == 0
            && params.cursor_policy == 0
            && self.grid.active().cursor.pending_wrap
        {
            if self.auto_wrap {
                let bg = self.bg;
                let active = self.grid.active_mut();
                let r = active.cursor.row;
                active.grid[r].is_wrapped = true;
                active.newline_with_bg(bg);
            } else {
                self.grid.active_mut().cursor.pending_wrap = false;
            }
        }

        let (anchor_row, anchor_col) = if params.parent_id != 0 {
            let parent = self
                .grid
                .active()
                .images
                .iter()
                .rev()
                .find(|p| {
                    p.image_id == params.parent_id
                        && (params.parent_placement_id == 0
                            || p.placement_id == params.parent_placement_id)
                })
                .ok_or(("ENOENT", "Parent placement not found"))?;
            let max_col = self.grid.active().cols.saturating_sub(1) as isize;
            let r = parent.row + params.parent_offset_v as isize;
            let c =
                (parent.col as isize + params.parent_offset_h as isize).clamp(0, max_col) as usize;
            (r, c)
        } else {
            let cur = &self.grid.active().cursor;
            (cur.row as isize, cur.col)
        };

        let cell_w = self.grid.cell_width_px.max(1.0);
        let cell_h = self.grid.cell_height_px.max(1.0);

        let mut img = GraphicImage {
            id: stored.texture_id,
            image_id,
            image_number: stored.image_number,
            placement_id: params.placement_id,
            row: anchor_row,
            col: anchor_col,
            width_px: stored.width_px,
            height_px: stored.height_px,
            src_rect: [sx, sy, sw, sh],
            cell_offset: [params.cell_x_offset, params.cell_y_offset],
            display_cols,
            display_rows,
            width_cells: 1,
            height_cells: 1,
            rgba_data: stored.rgba_data,
            z_index: params.z_index,
        };

        let [off_x, off_y, disp_w, disp_h] = img.compute_display_box(cell_w, cell_h);
        img.width_cells = display_cols
            .map(|c| c as usize)
            .unwrap_or_else(|| (((off_x + disp_w) / cell_w).ceil() as usize).max(1));
        img.height_cells = display_rows
            .map(|r| r as usize)
            .unwrap_or_else(|| (((off_y + disp_h) / cell_h).ceil() as usize).max(1));

        let width_cells = img.width_cells;
        let height_cells = img.height_cells;

        if params.placement_id != 0 {
            self.grid
                .active_mut()
                .images
                .retain(|p| !(p.image_id == image_id && p.placement_id == params.placement_id));
        }

        info!(
            "Placed Kitty Graphics image id={} ({}x{} px, {}x{} cells) at ({}, {})",
            image_id,
            img.width_px,
            img.height_px,
            width_cells,
            height_cells,
            anchor_row,
            anchor_col
        );
        self.grid.active_mut().add_image(img);

        if params.cursor_policy == 0 && params.parent_id == 0 {
            let bg = self.bg;
            let auto_wrap = self.auto_wrap;
            let active = self.grid.active_mut();
            for _ in 0..height_cells.saturating_sub(1) {
                active.linefeed_with_bg(bg);
            }
            let end_col = anchor_col + width_cells;
            if end_col < active.cols {
                active.cursor.col = end_col;
                active.cursor.pending_wrap = false;
            } else {
                active.cursor.col = active.cols.saturating_sub(1);
                active.cursor.pending_wrap = auto_wrap;
            }
            active.last_placeholder_pos = None;
        }

        Ok(())
    }

    fn execute_kitty_delete(&mut self, params: &KittyControlParams) {
        let sel = params.delete_target;
        let free_data = sel.is_ascii_uppercase();
        let kind = sel.to_ascii_lowercase();

        let cur_row = self.grid.active().cursor.row as isize;
        let cur_col = self.grid.active().cursor.col;

        let mut candidate_ids: Vec<u32> = Vec::new();
        let changed: bool;

        match kind {
            'a' => {
                // Delete all visible placements on the active screen.
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    let visible = p.row + (p.height_cells.max(1) as isize) > 0
                        && p.row < active.rows as isize;
                    if visible {
                        if p.image_id != 0 {
                            candidate_ids.push(p.image_id);
                        }
                        false
                    } else {
                        true
                    }
                });
                if active.images.len() != before {
                    active.images_dirty = true;
                }
                if free_data {
                    let all_ids: Vec<u32> = self.grid.kitty_images.keys().copied().collect();
                    self.grid.virtual_placements.clear();
                    for id in all_ids {
                        if !self.grid.is_kitty_image_referenced(id) {
                            self.grid.remove_stored_kitty_image(id);
                        }
                    }
                }
                return;
            }
            'i' => {
                let target_id = params.image_id;
                if target_id == 0 {
                    return;
                }
                candidate_ids.push(target_id);
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    !(p.image_id == target_id
                        && (params.placement_id == 0 || p.placement_id == params.placement_id))
                });
                changed = active.images.len() != before;
                if params.placement_id == 0 {
                    self.grid
                        .virtual_placements
                        .retain(|&(iid, _), _| iid != target_id);
                } else {
                    self.grid
                        .virtual_placements
                        .remove(&(target_id, params.placement_id));
                }
            }
            'n' => {
                let Some(&target_id) = self.grid.kitty_number_to_id.get(&params.image_number)
                else {
                    return;
                };
                candidate_ids.push(target_id);
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    !(p.image_id == target_id
                        && (params.placement_id == 0 || p.placement_id == params.placement_id))
                });
                changed = active.images.len() != before;
                if params.placement_id == 0 {
                    self.grid
                        .virtual_placements
                        .retain(|&(iid, _), _| iid != target_id);
                } else {
                    self.grid
                        .virtual_placements
                        .remove(&(target_id, params.placement_id));
                }
            }
            'c' => {
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    if p.intersects_cell(cur_row, cur_col) {
                        if p.image_id != 0 {
                            candidate_ids.push(p.image_id);
                        }
                        false
                    } else {
                        true
                    }
                });
                changed = active.images.len() != before;
            }
            'p' | 'q' => {
                // 1-based cell coordinates `(y, x)`, plus `z_index` match for `q`.
                if params.x == 0 || params.y == 0 {
                    return;
                }
                let target_row = (params.y - 1) as isize;
                let target_col = (params.x - 1) as usize;
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    let hit = p.intersects_cell(target_row, target_col)
                        && (kind != 'q' || p.z_index == params.z_index);
                    if hit {
                        if p.image_id != 0 {
                            candidate_ids.push(p.image_id);
                        }
                        false
                    } else {
                        true
                    }
                });
                changed = active.images.len() != before;
            }
            'r' => {
                let lo = params.x;
                let hi = params.y;
                if lo == 0 || hi < lo {
                    return;
                }
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    if p.image_id >= lo && p.image_id <= hi {
                        candidate_ids.push(p.image_id);
                        false
                    } else {
                        true
                    }
                });
                changed = active.images.len() != before;
                for &id in self.grid.kitty_images.keys() {
                    if id >= lo && id <= hi {
                        candidate_ids.push(id);
                    }
                }
                self.grid
                    .virtual_placements
                    .retain(|&(iid, _), _| !(iid >= lo && iid <= hi));
            }
            'x' => {
                if params.x == 0 {
                    return;
                }
                let target_col = (params.x - 1) as usize;
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    let hit = p.row + (p.height_cells.max(1) as isize) > 0
                        && p.row < active.rows as isize
                        && target_col >= p.col
                        && target_col < p.col + p.width_cells.max(1);
                    if hit {
                        if p.image_id != 0 {
                            candidate_ids.push(p.image_id);
                        }
                        false
                    } else {
                        true
                    }
                });
                changed = active.images.len() != before;
            }
            'y' => {
                if params.y == 0 {
                    return;
                }
                let target_row = (params.y - 1) as isize;
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    let hit = target_row >= p.row
                        && target_row < p.row + (p.height_cells.max(1) as isize);
                    if hit {
                        if p.image_id != 0 {
                            candidate_ids.push(p.image_id);
                        }
                        false
                    } else {
                        true
                    }
                });
                changed = active.images.len() != before;
            }
            'z' => {
                let active = self.grid.active_mut();
                let before = active.images.len();
                active.images.retain(|p| {
                    let visible = p.row + (p.height_cells.max(1) as isize) > 0
                        && p.row < active.rows as isize;
                    if visible && p.z_index == params.z_index {
                        if p.image_id != 0 {
                            candidate_ids.push(p.image_id);
                        }
                        false
                    } else {
                        true
                    }
                });
                changed = active.images.len() != before;
            }
            'f' => {
                let target_id = if params.image_id != 0 {
                    Some(params.image_id)
                } else if params.image_number != 0 {
                    self.grid
                        .kitty_number_to_id
                        .get(&params.image_number)
                        .copied()
                } else {
                    None
                };
                if let Some(id) = target_id {
                    if let Some(stored) = self.grid.kitty_images.get_mut(&id) {
                        if stored.frames.len() > 1 {
                            stored.frames.truncate(1);
                            stored.active_frame = 0;
                            stored.rgba_data = Arc::clone(&stored.frames[0].rgba_data);
                        }
                    }
                }
                return;
            }
            _ => return,
        }

        if changed {
            self.grid.active_mut().images_dirty = true;
        }

        if free_data {
            candidate_ids.sort_unstable();
            candidate_ids.dedup();
            for id in candidate_ids {
                if !self.grid.is_kitty_image_referenced(id) {
                    self.grid.remove_stored_kitty_image(id);
                }
            }
        }
    }

    fn resolve_target_kitty_id(
        &self,
        params: &KittyControlParams,
    ) -> Result<u32, (&'static str, &'static str)> {
        if params.image_id != 0 {
            if self.grid.kitty_images.contains_key(&params.image_id) {
                Ok(params.image_id)
            } else {
                Err(("ENOENT", "Image ID not found"))
            }
        } else if params.image_number != 0 {
            self.grid
                .kitty_number_to_id
                .get(&params.image_number)
                .copied()
                .ok_or(("ENOENT", "Image number not found"))
        } else {
            Err(("EINVAL", "Image ID or number is required"))
        }
    }

    fn refresh_kitty_texture_bindings(&mut self, image_id: u32) {
        let new_tex_id = self.grid.alloc_texture_id();
        let Some(stored) = self.grid.kitty_images.get_mut(&image_id) else {
            return;
        };
        stored.texture_id = new_tex_id;
        let rgba_arc = Arc::clone(&stored.rgba_data);

        for buf in [&mut self.grid.primary, &mut self.grid.alternate] {
            let mut touched = false;
            for p in &mut buf.images {
                if p.image_id == image_id {
                    p.id = new_tex_id;
                    p.rgba_data = Arc::clone(&rgba_arc);
                    touched = true;
                }
            }
            if touched {
                buf.images_dirty = true;
            }
        }
        self.grid.active_mut().images_dirty = true;
    }

    fn execute_kitty_frame(
        &mut self,
        params: &KittyControlParams,
        payload: Vec<u8>,
    ) -> Result<u32, (&'static str, &'static str)> {
        let image_id = self.resolve_target_kitty_id(params)?;
        let (img_w, img_h) = {
            let stored = self
                .grid
                .kitty_images
                .get(&image_id)
                .ok_or(("ENOENT", "Image not found"))?;
            (stored.width_px, stored.height_px)
        };

        let mut frame_params = params.clone();
        if frame_params.data_width == 0 {
            frame_params.data_width = img_w;
        }
        if frame_params.data_height == 0 {
            frame_params.data_height = img_h;
        }

        let (patch_w, patch_h, patch_rgba) = if payload.is_empty() && frame_params.medium == 'd' {
            (0, 0, Vec::new())
        } else {
            let raw = load_transmission_payload(&frame_params, payload)?;
            decode_kitty_pixels(&frame_params, raw)?
        };

        let overwrite = params.cell_x_offset == 1;
        let gap_ms = params.z_index.max(0) as u32;
        let fill = params.cell_y_offset.to_be_bytes();

        {
            let stored = self
                .grid
                .kitty_images
                .get_mut(&image_id)
                .ok_or(("ENOENT", "Image not found"))?;

            let edit_idx = if params.cols > 0 {
                let idx = (params.cols - 1) as usize;
                if idx < stored.frames.len() {
                    Some(idx)
                } else {
                    None
                }
            } else {
                None
            };

            if let Some(idx) = edit_idx {
                let mut canvas = (*stored.frames[idx].rgba_data).clone();
                if patch_w > 0 && patch_h > 0 {
                    composite_rgba_patch(
                        &mut canvas,
                        img_w,
                        img_h,
                        &patch_rgba,
                        patch_w,
                        patch_h,
                        params.x,
                        params.y,
                        overwrite,
                    );
                }
                let arc = Arc::new(canvas);
                stored.frames[idx].rgba_data = Arc::clone(&arc);
                if gap_ms > 0 {
                    stored.frames[idx].gap_ms = gap_ms;
                }
                if stored.active_frame == idx {
                    stored.rgba_data = arc;
                }
            } else {
                let mut canvas = if params.rows > 0 {
                    let base_idx = (params.rows - 1) as usize;
                    if let Some(base) = stored.frames.get(base_idx) {
                        (*base.rgba_data).clone()
                    } else {
                        fill.repeat((img_w as usize) * (img_h as usize))
                    }
                } else {
                    fill.repeat((img_w as usize) * (img_h as usize))
                };

                if patch_w > 0 && patch_h > 0 {
                    composite_rgba_patch(
                        &mut canvas,
                        img_w,
                        img_h,
                        &patch_rgba,
                        patch_w,
                        patch_h,
                        params.x,
                        params.y,
                        overwrite,
                    );
                }
                let frame_num = (stored.frames.len() + 1) as u32;
                let arc = Arc::new(canvas);
                stored.frames.push(AnimationFrame {
                    frame_number: frame_num,
                    rgba_data: arc,
                    gap_ms: if gap_ms > 0 { gap_ms } else { 40 },
                });
            }
        }

        self.refresh_kitty_texture_bindings(image_id);
        Ok(image_id)
    }

    fn execute_kitty_compose(
        &mut self,
        params: &KittyControlParams,
    ) -> Result<u32, (&'static str, &'static str)> {
        let image_id = self.resolve_target_kitty_id(params)?;
        let overwrite = params.cursor_policy == 1;

        {
            let stored = self
                .grid
                .kitty_images
                .get_mut(&image_id)
                .ok_or(("ENOENT", "Image not found"))?;

            let src_idx = params.rows.saturating_sub(1) as usize;
            let dst_idx = params.cols.saturating_sub(1) as usize;
            if src_idx >= stored.frames.len() || dst_idx >= stored.frames.len() {
                return Err(("ENOENT", "Animation frame index out of range"));
            }

            let img_w = stored.width_px;
            let img_h = stored.height_px;
            let sx = params.x.min(img_w);
            let sy = params.y.min(img_h);
            let copy_w = if params.w == 0 {
                img_w.saturating_sub(sx)
            } else {
                params.w.min(img_w.saturating_sub(sx))
            };
            let copy_h = if params.h == 0 {
                img_h.saturating_sub(sy)
            } else {
                params.h.min(img_h.saturating_sub(sy))
            };

            if copy_w > 0 && copy_h > 0 {
                let src_buf = &stored.frames[src_idx].rgba_data;
                let stride = img_w as usize * 4;
                let mut rect_bytes = Vec::with_capacity((copy_w * copy_h * 4) as usize);
                for r in 0..copy_h as usize {
                    let start = (sy as usize + r) * stride + (sx as usize) * 4;
                    rect_bytes.extend_from_slice(&src_buf[start..start + (copy_w as usize) * 4]);
                }

                let mut dst_canvas = (*stored.frames[dst_idx].rgba_data).clone();
                composite_rgba_patch(
                    &mut dst_canvas,
                    img_w,
                    img_h,
                    &rect_bytes,
                    copy_w,
                    copy_h,
                    params.cell_x_offset,
                    params.cell_y_offset,
                    overwrite,
                );
                let arc = Arc::new(dst_canvas);
                stored.frames[dst_idx].rgba_data = Arc::clone(&arc);
                if stored.active_frame == dst_idx {
                    stored.rgba_data = arc;
                }
            }
        }

        self.refresh_kitty_texture_bindings(image_id);
        Ok(image_id)
    }

    fn execute_kitty_animate(
        &mut self,
        params: &KittyControlParams,
    ) -> Result<u32, (&'static str, &'static str)> {
        let image_id = self.resolve_target_kitty_id(params)?;
        let switched = {
            let stored = self
                .grid
                .kitty_images
                .get_mut(&image_id)
                .ok_or(("ENOENT", "Image not found"))?;

            if params.rows > 0 {
                let idx = (params.rows - 1) as usize;
                if let Some(frame) = stored.frames.get_mut(idx) {
                    if params.z_index >= 0 {
                        frame.gap_ms = params.z_index as u32;
                    }
                }
            }

            if params.cols > 0 {
                let idx = (params.cols - 1) as usize;
                if let Some(frame) = stored.frames.get(idx) {
                    stored.active_frame = idx;
                    stored.rgba_data = Arc::clone(&frame.rgba_data);
                    true
                } else {
                    false
                }
            } else if params.data_width == 3 && !stored.frames.is_empty() {
                // `s=3` starts playback: advance to the latest available frame if still on frame 1.
                let last_idx = stored.frames.len() - 1;
                if stored.active_frame != last_idx {
                    stored.active_frame = last_idx;
                    stored.rgba_data = Arc::clone(&stored.frames[last_idx].rgba_data);
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };

        if switched {
            self.refresh_kitty_texture_bindings(image_id);
        }
        Ok(image_id)
    }
}

fn trim_ascii_bytes(bytes: &[u8]) -> &[u8] {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let end = bytes
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &bytes[start..end]
}

fn is_valid_osc7501_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'+' | b'-')
}

fn validate_osc7501_token(s: &str, max_len: usize) -> bool {
    !s.is_empty() && s.len() <= max_len && s.bytes().all(is_valid_osc7501_token_byte)
}

fn validate_osc7501_id(s: &str) -> bool {
    const MAX_ID_LEN: usize = 128;
    const MAX_SEGMENTS: usize = 8;
    const MAX_SEGMENT_LEN: usize = 32;

    if s.is_empty() || s.len() > MAX_ID_LEN {
        return false;
    }
    let mut count = 0;
    for seg in s.split('/') {
        count += 1;
        if count > MAX_SEGMENTS || !validate_osc7501_token(seg, MAX_SEGMENT_LEN) {
            return false;
        }
    }
    count > 0
}

fn decode_osc7501_base64_field(
    encoded: &str,
    max_encoded_len: usize,
    max_decoded_len: usize,
) -> Option<Option<String>> {
    if encoded.is_empty() {
        return Some(None);
    }
    if encoded.len() > max_encoded_len {
        return None;
    }

    // Normalize URL-safe Base64 (`-` -> `+`, `_` -> `/`) and validate alphabet.
    let mut normalized = Vec::with_capacity(encoded.len() + 2);
    for b in encoded.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' | b'=' => normalized.push(b),
            b'-' => normalized.push(b'+'),
            b'_' => normalized.push(b'/'),
            _ => return None,
        }
    }

    match normalized.len() % 4 {
        0 => {}
        1 => return None,
        2 => normalized.extend_from_slice(b"=="),
        3 => normalized.push(b'='),
        _ => unreachable!(),
    }

    use base64::Engine;
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(&normalized)
        .ok()?;
    if decoded.len() > max_decoded_len {
        return None;
    }

    let text = String::from_utf8(decoded).ok()?;
    // Reject C0 (< 0x20), DEL (0x7f), and C1 (0x80..=0x9f) control characters.
    if text
        .chars()
        .any(|c| (c as u32) < 0x20 || (0x7f..=0x9f).contains(&(c as u32)))
    {
        return None;
    }

    let trimmed = text.trim();
    if trimmed.is_empty() {
        Some(None)
    } else {
        Some(Some(trimmed.to_string()))
    }
}

fn parse_osc7501_payload(payload: &str) -> Option<ParsedOsc7501> {
    const MAX_PAYLOAD_LEN: usize = 4087;
    const MAX_TITLE_ENCODED_LEN: usize = 256;
    const MAX_TITLE_DECODED_LEN: usize = 192;
    const MAX_MSG_ENCODED_LEN: usize = 2732;
    const MAX_MSG_DECODED_LEN: usize = 2048;

    let trimmed = payload.trim_ascii();
    if trimmed.is_empty() || trimmed.len() > MAX_PAYLOAD_LEN {
        return None;
    }

    let mut state: Option<ProgramStatusState> = None;
    let mut id: Option<String> = None;
    let mut kind = ProgramStatusKind::None;
    let mut app: Option<String> = None;
    let mut title: Option<String> = None;
    let mut msg: Option<String> = None;
    let mut progress: Option<u8> = None;

    for pair in trimmed.split(':') {
        let pair = pair.trim_ascii();
        if pair.is_empty() {
            return None;
        }
        let (raw_key, raw_val) = pair.split_once('=')?;
        let key = raw_key.trim_ascii();
        let val = raw_val.trim_ascii();
        if key.is_empty() {
            return None;
        }

        match key {
            "state" => {
                state = Some(ProgramStatusState::parse(val)?);
            }
            "id" => {
                if val.is_empty() {
                    id = None;
                } else if validate_osc7501_id(val) {
                    id = Some(val.to_string());
                } else {
                    return None;
                }
            }
            "app" => {
                if val.is_empty() {
                    app = None;
                } else if validate_osc7501_token(val, 32) {
                    app = Some(val.to_string());
                } else {
                    return None;
                }
            }
            "kind" => {
                if val.is_empty() {
                    kind = ProgramStatusKind::None;
                } else if validate_osc7501_token(val, 32) {
                    kind = ProgramStatusKind::parse(val);
                } else {
                    return None;
                }
            }
            "title" => {
                title =
                    decode_osc7501_base64_field(val, MAX_TITLE_ENCODED_LEN, MAX_TITLE_DECODED_LEN)?;
            }
            "msg" => {
                msg = decode_osc7501_base64_field(val, MAX_MSG_ENCODED_LEN, MAX_MSG_DECODED_LEN)?;
            }
            "progress" => {
                if val.is_empty() {
                    progress = None;
                } else {
                    let num_str = val.strip_suffix('%').unwrap_or(val);
                    let pct = num_str.parse::<u8>().ok()?;
                    if pct > 100 {
                        return None;
                    }
                    progress = Some(pct);
                }
            }
            _ => {
                // Ignore unknown keys for forward compatibility.
            }
        }
    }

    Some(ParsedOsc7501 {
        id,
        state: state?,
        kind,
        app,
        title,
        msg,
        progress,
    })
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

fn decode_hex_bytes(hex: &str) -> Option<Vec<u8>> {
    if !hex.len().is_multiple_of(2) {
        return None;
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for chunk in bytes.as_chunks::<2>().0 {
        let s = std::str::from_utf8(chunk).ok()?;
        let val = u8::from_str_radix(s, 16).ok()?;
        out.push(val);
    }
    Some(out)
}

fn encode_hex_bytes(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        use std::fmt::Write;
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn lookup_xtgettcap(cap_name: &str) -> Option<&'static [u8]> {
    match cap_name {
        "TN" | "name" => Some(b"xterm-256color"),
        "Co" | "colors" => Some(b"256"),
        "RGB" => Some(b"8/8/8"),
        "Tc" | "bce" | "am" | "xenl" | "msgr" | "Su" => Some(b""),
        "Smulx" => Some(b"\x1b[4:%p1%dm"),
        "Setulc" => Some(b"\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%dm"),
        "sync" => Some(b"\x1b[?2026%?%p1%{1}%-%tl%eh%;"),
        "Cs" => Some(b"\x1b]12;%p1%s\x07"),
        "Cr" => Some(b"\x1b]112\x07"),
        "Se" => Some(b"\x1b[2 q"),
        "Ss" => Some(b"\x1b[%p1%d q"),
        "Ms" => Some(b"\x1b]52;%p1%s;%p2%s\x07"),
        _ => None,
    }
}
