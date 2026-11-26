//! VT100, xterm, 24-bit TrueColor ANSI, Sixel, Kitty Graphics, and OSC 1337 escape sequence handler.

use crate::graphics::{
    composite_rgba_patch, decode_b64_flexible, decode_kitty_pixels, load_transmission_payload,
    AnimationFrame, AnimationState, GraphicImage, KittyControlParams, KittyGraphicsParser,
    KittyParseStep, Osc1337Action, Osc1337FileParams, Osc1337Parser, SixelDecoder,
    VirtualPlacement,
};
use crate::grid::{
    CellFlags, Color, CursorShape, NamedColor, PointerShape, ProgressState, SavedCursor,
    ScreenGrid, SemanticZone, TerminalProgress,
};
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

/// Mouse coordinate format / encoding (`?1006` SGR, `?1016` SGR-Pixels, or legacy X10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MouseFormat {
    #[default]
    X10,
    Sgr,
    SgrPixels,
}

/// Alias for [`MouseFormat`] matching terminal specification naming (`MouseEncoding::SgrPixels`).
pub type MouseEncoding = MouseFormat;

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

/// Clipboard operation requested by `OSC 52` or `OSC 1337;Copy=...`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardRequest {
    /// Set clipboard contents to decoded UTF-8 string.
    Write(String),
    /// Clear clipboard contents (`OSC 52 ; c ; `).
    Clear,
    /// Query current clipboard contents (`OSC 52 ; <selection> ; ?`).
    Query {
        selection: String,
        bell_terminated: bool,
    },
}

/// Desktop / Toast notification emitted via `OSC 9`, `OSC 99`, or `OSC 777`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalNotification {
    pub id: Option<String>,
    pub title: String,
    pub body: String,
    /// `true` when `OSC 99` explicitly specifies `o=always` (display even when focused).
    pub force_when_focused: bool,
    /// `true` when `OSC 99` specifies `o=invisible` (display only when occluded or background tab).
    pub only_when_invisible: bool,
    /// `true` when urgency is critical (`u=2`).
    pub urgent: bool,
}

/// In-flight multi-chunk `OSC 99` (`d=0`) notification draft.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Osc99Draft {
    pub title: String,
    pub body: String,
    pub force_when_focused: bool,
    pub only_when_invisible: bool,
    pub urgent: bool,
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
    /// Explicit tab / icon title set via `OSC 1;<title>` (`None` when unset or cleared).
    pub icon_title: Option<String>,
    pub icon_title_stack: Vec<Option<String>>,
    pub application_cursor_keys: bool,
    pub bracketed_paste: bool,
    pub auto_wrap: bool,
    pub origin_mode: bool,
    pub focus_events: bool,
    pub synchronized_output: bool,
    pub synchronized_output_since: Option<std::time::Instant>,
    pub alternate_scroll: bool,
    pub grapheme_clustering: bool,
    pub color_scheme_updates: bool,
    pub color_scheme_dark: bool,
    pub in_band_resize: bool,
    pub saved_dec_modes: HashMap<u16, bool>,
    pub modify_other_keys: u8,
    pub primary_modify_other_keys: u8,
    pub alt_modify_other_keys: u8,
    pub kitty_keyboard_stack: Vec<u16>,
    pub alt_kitty_keyboard_stack: Vec<u16>,
    pub default_fg: [u8; 3],
    pub default_bg: [u8; 3],
    pub default_cursor: [u8; 3],
    pub default_palette: [[u8; 3]; 16],
    pub dynamic_palette: HashMap<u8, (u8, u8, u8)>,
    pub dynamic_fg: Option<(u8, u8, u8)>,
    pub dynamic_bg: Option<(u8, u8, u8)>,
    pub dynamic_cursor: Option<(u8, u8, u8)>,
    pub dynamic_selection_bg: Option<(u8, u8, u8)>,
    pub dynamic_selection_fg: Option<(u8, u8, u8)>,
    pub pointer_shape: PointerShape,
    pub pointer_shape_stack: Vec<PointerShape>,
    pub progress: Option<TerminalProgress>,
    pub progress_updated_at: Option<std::time::Instant>,
    pub mouse_protocol: MouseProtocol,
    pub mouse_format: MouseFormat,
    pub in_sixel: bool,
    pub sixel_buffer: Vec<u8>,
    pub dcs_mode: DcsMode,
    pub dcs_buffer: Vec<u8>,
    pub kitty_parser: KittyGraphicsParser,
    pub osc1337_parser: Osc1337Parser,
    pub insert_mode: bool,
    pub current_working_directory: Option<String>,
    pub current_document_url: Option<String>,
    pub pending_clipboard: Option<String>,
    pub pending_clipboard_requests: Vec<ClipboardRequest>,
    pub pending_notifications: Vec<TerminalNotification>,
    pub pending_user_attention: Option<bool>,
    pub osc99_drafts: HashMap<String, Osc99Draft>,
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
            icon_title: None,
            icon_title_stack: Vec::new(),
            application_cursor_keys: false,
            bracketed_paste: false,
            auto_wrap: true,
            origin_mode: false,
            focus_events: false,
            synchronized_output: false,
            synchronized_output_since: None,
            alternate_scroll: true,
            grapheme_clustering: true,
            color_scheme_updates: false,
            color_scheme_dark: true,
            in_band_resize: false,
            saved_dec_modes: HashMap::new(),
            modify_other_keys: 0,
            primary_modify_other_keys: 0,
            alt_modify_other_keys: 0,
            kitty_keyboard_stack: Vec::new(),
            alt_kitty_keyboard_stack: Vec::new(),
            default_fg: [0xf5, 0xf5, 0xf7],
            default_bg: [0x1a, 0x1b, 0x1e],
            default_cursor: [0xf5, 0xf5, 0xf7],
            default_palette: default_ansi_palette_16(),
            dynamic_palette: HashMap::new(),
            dynamic_fg: None,
            dynamic_bg: None,
            dynamic_cursor: None,
            dynamic_selection_bg: None,
            dynamic_selection_fg: None,
            pointer_shape: PointerShape::Default,
            pointer_shape_stack: Vec::new(),
            progress: None,
            progress_updated_at: None,
            mouse_protocol: MouseProtocol::None,
            mouse_format: MouseFormat::X10,
            in_sixel: false,
            sixel_buffer: Vec::new(),
            dcs_mode: DcsMode::None,
            dcs_buffer: Vec::new(),
            kitty_parser: KittyGraphicsParser::new(),
            osc1337_parser: Osc1337Parser::new(),
            insert_mode: false,
            current_working_directory: None,
            current_document_url: None,
            pending_clipboard: None,
            pending_clipboard_requests: Vec::new(),
            pending_notifications: Vec::new(),
            pending_user_attention: None,
            osc99_drafts: HashMap::new(),
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

    /// Return the currently active mouse pointer shape (`OSC 22`).
    pub fn active_pointer_shape(&self) -> PointerShape {
        self.pointer_shape_stack
            .last()
            .copied()
            .unwrap_or(self.pointer_shape)
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
        self.grapheme_clustering = true;
        self.pointer_shape = PointerShape::Default;
        self.pointer_shape_stack.clear();
        self.progress = None;
        self.progress_updated_at = None;
        self.g0_charset = Charset::Ascii;
        self.g1_charset = Charset::Ascii;
        self.active_charset = 0;
        let active = self.grid.active_mut();
        active.cursor.visible = true;
        active.cursor.blink = true;
        active.cursor.pending_wrap = false;
        active.last_placeholder_pos = None;
        active.last_grapheme_pos = None;
        active.scroll_region = (0, active.rows.saturating_sub(1));
        active.saved_cursor = None;
    }

    /// Returns true if DEC Private Mode `?2026` (Synchronized Output) is currently active
    /// and within its 1000ms safety timeout window.
    pub fn is_synchronized_output_active(&self) -> bool {
        self.synchronized_output
            && self
                .synchronized_output_since
                .is_some_and(|t| t.elapsed() < std::time::Duration::from_millis(1000))
    }

    #[inline]
    fn active_kitty_keyboard_stack(&self) -> &Vec<u16> {
        if self.grid.is_alternate {
            &self.alt_kitty_keyboard_stack
        } else {
            &self.kitty_keyboard_stack
        }
    }

    #[inline]
    fn active_kitty_keyboard_stack_mut(&mut self) -> &mut Vec<u16> {
        if self.grid.is_alternate {
            &mut self.alt_kitty_keyboard_stack
        } else {
            &mut self.kitty_keyboard_stack
        }
    }

    pub fn kitty_keyboard_flags(&self) -> u16 {
        self.active_kitty_keyboard_stack()
            .last()
            .copied()
            .unwrap_or(0)
    }

    #[inline]
    fn default_indexed_color(&self, idx: u8) -> (u8, u8, u8) {
        if idx < 16 {
            let c = self.default_palette[idx as usize];
            (c[0], c[1], c[2])
        } else {
            default_indexed_rgb(idx)
        }
    }

    /// Format a DEC Mode `?2048` in-band resize report (`CSI 48 ; rows ; cols ; h_px ; w_px t`).
    pub fn format_in_band_resize_report(&self) -> String {
        let rows = self.grid.active().rows;
        let cols = self.grid.active().cols;
        let w_px = (cols as f32 * self.grid.cell_width_px)
            .round()
            .clamp(0.0, u16::MAX as f32) as u16;
        let h_px = (rows as f32 * self.grid.cell_height_px)
            .round()
            .clamp(0.0, u16::MAX as f32) as u16;
        format!("\x1b[48;{rows};{cols};{h_px};{w_px}t")
    }

    /// Query a DEC private mode's boolean state for `DECRQM` (`CSI ? Ps $ p`) and `XTSAVE` (`CSI ? Ps s`).
    pub fn get_dec_private_mode(&self, mode: u16) -> Option<bool> {
        match mode {
            1 => Some(self.application_cursor_keys),
            6 => Some(self.origin_mode),
            7 => Some(self.auto_wrap),
            9 => Some(self.mouse_protocol == MouseProtocol::X10),
            12 => Some(self.grid.active().cursor.blink),
            25 => Some(self.grid.active().cursor.visible),
            47 | 1047 | 1049 => Some(self.grid.is_alternate),
            1000 => Some(self.mouse_protocol == MouseProtocol::Normal),
            1002 => Some(self.mouse_protocol == MouseProtocol::ButtonEvent),
            1003 => Some(self.mouse_protocol == MouseProtocol::AnyEvent),
            1004 => Some(self.focus_events),
            1006 => Some(self.mouse_format == MouseFormat::Sgr),
            1007 => Some(self.alternate_scroll),
            1016 => Some(self.mouse_format == MouseFormat::SgrPixels),
            1048 => Some(self.grid.active().saved_cursor.is_some()),
            2004 => Some(self.bracketed_paste),
            2026 => Some(self.synchronized_output),
            2027 => Some(self.grapheme_clustering),
            2031 => Some(self.color_scheme_updates),
            2048 => Some(self.in_band_resize),
            _ => None,
        }
    }

    /// Apply a DEC private mode set (`enable = true`) or reset (`enable = false`),
    /// shared across `CSI ? Ps h/l` and `XTRESTORE` (`CSI ? Ps r`).
    pub fn set_dec_private_mode(&mut self, mode: u16, enable: bool) {
        match (mode, enable) {
            (1, v) => self.application_cursor_keys = v,
            (6, true) => {
                self.origin_mode = true;
                let active = self.grid.active_mut();
                active.cursor.row = active.scroll_region.0;
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
            }
            (6, false) => {
                self.origin_mode = false;
                let active = self.grid.active_mut();
                active.cursor.row = 0;
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
            }
            (7, true) => self.auto_wrap = true,
            (7, false) => {
                self.auto_wrap = false;
                self.grid.active_mut().cursor.pending_wrap = false;
            }
            (9, true) => self.mouse_protocol = MouseProtocol::X10,
            (9, false) => {
                if self.mouse_protocol == MouseProtocol::X10 {
                    self.mouse_protocol = MouseProtocol::None;
                }
            }
            (12, v) => self.grid.active_mut().cursor.blink = v,
            (25, v) => self.grid.active_mut().cursor.visible = v,
            (47 | 1047 | 1049, true) => {
                let was_alt = self.grid.is_alternate;
                if mode == 1049 {
                    self.save_cursor();
                }
                if !was_alt {
                    self.primary_modify_other_keys = self.modify_other_keys;
                    self.alt_modify_other_keys = 0;
                    self.modify_other_keys = 0;
                    self.alt_kitty_keyboard_stack.clear();
                }
                self.grid.set_alternate(true);
                if mode == 1049 {
                    self.grid.alternate.clear_alternate();
                }
            }
            (47 | 1047 | 1049, false) => {
                let was_alt = self.grid.is_alternate;
                if mode == 1047 {
                    self.grid.alternate.clear_alternate();
                }
                self.grid.set_alternate(false);
                if mode == 1049 {
                    self.restore_cursor();
                }
                if was_alt {
                    self.alt_kitty_keyboard_stack.clear();
                    self.alt_modify_other_keys = 0;
                    self.modify_other_keys = self.primary_modify_other_keys;
                    self.pointer_shape = PointerShape::Default;
                    self.pointer_shape_stack.clear();
                }
            }
            (1000, true) => self.mouse_protocol = MouseProtocol::Normal,
            (1000, false) => {
                if self.mouse_protocol == MouseProtocol::Normal {
                    self.mouse_protocol = MouseProtocol::None;
                }
            }
            (1002, true) => self.mouse_protocol = MouseProtocol::ButtonEvent,
            (1002, false) => {
                if self.mouse_protocol == MouseProtocol::ButtonEvent {
                    self.mouse_protocol = MouseProtocol::None;
                }
            }
            (1003, true) => self.mouse_protocol = MouseProtocol::AnyEvent,
            (1003, false) => {
                if self.mouse_protocol == MouseProtocol::AnyEvent {
                    self.mouse_protocol = MouseProtocol::None;
                }
            }
            (1004, v) => self.focus_events = v,
            (1006, true) => self.mouse_format = MouseFormat::Sgr,
            (1006, false) => {
                if self.mouse_format == MouseFormat::Sgr {
                    self.mouse_format = MouseFormat::X10;
                }
            }
            (1007, v) => self.alternate_scroll = v,
            (1016, true) => self.mouse_format = MouseFormat::SgrPixels,
            (1016, false) => {
                if self.mouse_format == MouseFormat::SgrPixels {
                    self.mouse_format = MouseFormat::X10;
                }
            }
            (1048, true) => self.save_cursor(),
            (1048, false) => self.restore_cursor(),
            (2004, v) => self.bracketed_paste = v,
            (2026, true) => {
                self.synchronized_output = true;
                self.synchronized_output_since = Some(std::time::Instant::now());
            }
            (2026, false) => {
                self.synchronized_output = false;
                self.synchronized_output_since = None;
            }
            (2027, v) => self.grapheme_clustering = v,
            (2031, v) => self.color_scheme_updates = v,
            (2048, true) => {
                self.in_band_resize = true;
                let report = self.format_in_band_resize_report();
                self.pending_responses.extend_from_slice(report.as_bytes());
            }
            (2048, false) => self.in_band_resize = false,
            _ => {}
        }
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
        let grapheme_clustering = self.grapheme_clustering;
        if self.insert_mode
            && !self
                .grid
                .active()
                .will_extend_grapheme(c, grapheme_clustering)
        {
            let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(1);
            if w > 0 {
                self.grid.active_mut().insert_chars(w, bg);
            }
        }
        self.grid.active_mut().write_char_with_grapheme_mode(
            c,
            fg,
            bg,
            flags,
            ul_id,
            hid,
            auto_wrap,
            grapheme_clustering,
        );
    }

    fn save_cursor(&mut self) {
        let origin_mode = self.origin_mode;
        let auto_wrap = self.auto_wrap;
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
            blink: active.cursor.blink,
            pending_wrap: active.cursor.pending_wrap,
            origin_mode,
            auto_wrap,
        });
    }

    fn restore_cursor(&mut self) {
        let active = self.grid.active_mut();
        if let Some(saved) = active.saved_cursor.clone() {
            active.cursor.row = saved.row.min(active.rows.saturating_sub(1));
            active.cursor.col = saved.col.min(active.cols.saturating_sub(1));
            active.cursor.shape = saved.shape;
            active.cursor.visible = saved.visible;
            active.cursor.blink = saved.blink;
            active.cursor.pending_wrap = saved.pending_wrap;
            active.last_placeholder_pos = None;
            active.last_grapheme_pos = None;
            self.fg = saved.fg;
            self.bg = saved.bg;
            self.underline_color = saved.underline_color;
            self.current_underline_color_id = saved.underline_color_id;
            self.flags = saved.flags;
            self.origin_mode = saved.origin_mode;
            self.auto_wrap = saved.auto_wrap;
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
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
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
                        custom_display_cells: None,
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
                            let cur = &self.grid.active().cursor;
                            let code = match (cur.shape, cur.blink) {
                                (CursorShape::Block, true) => 1,
                                (CursorShape::Block, false) => 2,
                                (CursorShape::Underline, true) => 3,
                                (CursorShape::Underline, false) => 4,
                                (CursorShape::Beam, true) => 5,
                                (CursorShape::Beam, false) => 6,
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
            b"0" => {
                if params.len() > 1 {
                    let joined = params[1..].join(&b';');
                    if let Ok(title) = std::str::from_utf8(&joined) {
                        self.title = title.to_string();
                        if title.trim().is_empty() {
                            self.icon_title = None;
                        }
                    }
                }
            }
            b"1" => {
                if params.len() > 1 {
                    let joined = params[1..].join(&b';');
                    if let Ok(icon) = std::str::from_utf8(&joined) {
                        let trimmed = icon.trim();
                        self.icon_title = if trimmed.is_empty() {
                            None
                        } else {
                            Some(trimmed.to_string())
                        };
                    }
                } else {
                    self.icon_title = None;
                }
            }
            b"2" => {
                if params.len() > 1 {
                    let joined = params[1..].join(&b';');
                    if let Ok(title) = std::str::from_utf8(&joined) {
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
                            .unwrap_or_else(|| self.default_indexed_color(idx));
                        let rgb_str = format_osc_rgb(r, g, b);
                        let resp = format!("\x1b]4;{idx};{rgb_str}{terminator}");
                        self.pending_responses.extend_from_slice(resp.as_bytes());
                    } else if let Some(rgb) = parse_osc_color_spec(spec) {
                        self.dynamic_palette.insert(idx, rgb);
                    }
                }
            }
            b"6" => {
                // macOS Terminal.app Document URI (`OSC 6 ; file://host/path`)
                if params.len() > 1 {
                    if let Ok(uri) = std::str::from_utf8(params[1]) {
                        let uri = uri.trim();
                        if uri.is_empty() {
                            self.current_document_url = None;
                        } else if let Some(path) = parse_osc7_uri(uri) {
                            if self.current_working_directory.is_none() {
                                let derived_cwd = if path.ends_with('/') {
                                    Some(path.clone())
                                } else {
                                    std::path::Path::new(&path)
                                        .parent()
                                        .and_then(|p| p.to_str())
                                        .filter(|s| !s.is_empty())
                                        .map(str::to_string)
                                };
                                if let Some(cwd) = derived_cwd {
                                    self.current_working_directory = Some(cwd);
                                }
                            }
                            self.current_document_url = Some(path);
                        }
                    }
                } else {
                    self.current_document_url = None;
                }
            }
            b"7" => {
                if params.len() > 1 {
                    if let Ok(uri) = std::str::from_utf8(params[1]) {
                        let uri = uri.trim();
                        if uri.is_empty() {
                            self.current_working_directory = None;
                        } else if let Some(path) = parse_osc7_uri(uri) {
                            self.current_working_directory = Some(path);
                        }
                    }
                }
            }
            b"8" => {
                if params.len() >= 3 {
                    let uri_cow: std::borrow::Cow<'_, [u8]> = if params.len() == 3 {
                        std::borrow::Cow::Borrowed(params[2])
                    } else {
                        std::borrow::Cow::Owned(params[2..].join(&b';'))
                    };
                    if let Ok(uri) = std::str::from_utf8(&uri_cow) {
                        if !uri.is_empty() {
                            let explicit_id = std::str::from_utf8(params[1])
                                .ok()
                                .and_then(extract_osc8_id);
                            let hid = self.grid.register_hyperlink_with_id(explicit_id, uri);
                            self.current_hyperlink_id = Some(hid);
                            self.current_hyperlink = Some(Arc::new(uri.to_string()));
                        } else {
                            self.current_hyperlink_id = None;
                            self.current_hyperlink = None;
                        }
                    }
                }
            }
            b"9" => {
                self.handle_osc_9(&params[1..]);
            }
            b"10" | b"11" | b"12" | b"17" | b"19" => {
                // Dynamic color query or set (`10` fg, `11` bg, `12` cursor, `17` selection bg, `19` selection fg).
                // Supports xterm chained parameters (`OSC 10 ; spec1 ; spec2 ...`).
                if let Ok(start_code) = std::str::from_utf8(params[0]).unwrap_or("10").parse::<u8>()
                {
                    self.handle_osc_dynamic_color_series(start_code, &params[1..], terminator);
                }
            }
            b"21" => {
                // Kitty Color Control Protocol (`OSC 21 ; key=val ; ...`)
                self.handle_osc_21(&params[1..], terminator);
            }
            b"22" => {
                // Mouse Pointer Cursor Shape Protocol (`OSC 22 ; <op>`)
                self.handle_osc_22(&params[1..], terminator);
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
            b"117" => self.dynamic_selection_bg = None,
            b"119" => self.dynamic_selection_fg = None,
            b"52" => {
                self.handle_osc_52(&params[1..], bell_terminated);
            }
            b"99" => {
                self.handle_osc_99(&params[1..]);
            }
            b"133" | b"633" => {
                let is_633 = params[0] == b"633";
                self.handle_osc_semantic_prompt(&params[1..], is_633);
            }
            b"777" => {
                self.handle_osc_777(&params[1..]);
            }
            b"1337" => {
                if params.len() > 1 {
                    if params.len() == 2 {
                        self.handle_osc_1337(params[1], bell_terminated);
                    } else {
                        let body = params[1..].join(&b';');
                        self.handle_osc_1337(&body, bell_terminated);
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
                if self.grid.is_alternate {
                    self.alt_modify_other_keys = self.modify_other_keys;
                } else {
                    self.primary_modify_other_keys = self.modify_other_keys;
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
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
            }
            'F' if intermediates.is_empty() => {
                let active = self.grid.active_mut();
                active.move_up((first_param as usize).max(1));
                active.cursor.col = 0;
                active.cursor.pending_wrap = false;
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
            }
            'G' | '`' if intermediates.is_empty() => {
                let col = (first_param as usize).max(1) - 1;
                let active = self.grid.active_mut();
                active.cursor.col = col.min(active.cols.saturating_sub(1));
                active.cursor.pending_wrap = false;
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
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
                active.last_placeholder_pos = None;
                active.last_grapheme_pos = None;
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
            's' if is_private && intermediates.len() == 1 => {
                // XTSAVE: CSI ? Ps ; ... s — save current DEC private mode values.
                const ALL_SAVEABLE_DEC_MODES: &[u16] = &[
                    1, 6, 7, 9, 12, 25, 47, 1000, 1002, 1003, 1004, 1006, 1007, 1016, 1047, 1049,
                    2004, 2026, 2027, 2031, 2048,
                ];
                let mut had_explicit = false;
                for param in params.iter() {
                    for &mode in param {
                        if mode != 0 {
                            had_explicit = true;
                            if let Some(val) = self.get_dec_private_mode(mode) {
                                self.saved_dec_modes.insert(mode, val);
                            }
                        }
                    }
                }
                if !had_explicit {
                    for &mode in ALL_SAVEABLE_DEC_MODES {
                        if let Some(val) = self.get_dec_private_mode(mode) {
                            self.saved_dec_modes.insert(mode, val);
                        }
                    }
                }
            }
            'u' if intermediates.is_empty() => self.restore_cursor(),
            'u' if intermediates == b">" => {
                // Kitty Keyboard Protocol: push flags onto active buffer's stack (CSI > flags u)
                let stack = self.active_kitty_keyboard_stack_mut();
                if stack.len() >= 16 {
                    stack.remove(0);
                }
                stack.push(first_param);
            }
            'u' if intermediates == b"<" => {
                // Kitty Keyboard Protocol: pop count entries from active buffer's stack (CSI < count u)
                let count = (first_param as usize).max(1);
                let stack = self.active_kitty_keyboard_stack_mut();
                let new_len = stack.len().saturating_sub(count);
                stack.truncate(new_len);
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
                let stack = self.active_kitty_keyboard_stack_mut();
                if let Some(last) = stack.last_mut() {
                    *last = next;
                } else {
                    stack.push(next);
                }
            }
            'u' if intermediates == b"?" => {
                // Kitty Keyboard Protocol: query current flags (CSI ? u -> CSI ? flags u)
                let flags = self.kitty_keyboard_flags();
                let resp = format!("\x1b[?{flags}u");
                self.pending_responses.extend_from_slice(resp.as_bytes());
            }
            'r' if is_private && intermediates.len() == 1 => {
                // XTRESTORE: CSI ? Ps ; ... r — restore saved DEC private mode values.
                let mut requested: Vec<u16> = Vec::new();
                for param in params.iter() {
                    for &mode in param {
                        if mode != 0 {
                            requested.push(mode);
                        }
                    }
                }
                if requested.is_empty() {
                    let entries: Vec<(u16, bool)> =
                        self.saved_dec_modes.iter().map(|(&k, &v)| (k, v)).collect();
                    for (mode, val) in entries {
                        self.set_dec_private_mode(mode, val);
                    }
                } else {
                    for mode in requested {
                        if let Some(&val) = self.saved_dec_modes.get(&mode) {
                            self.set_dec_private_mode(mode, val);
                        }
                    }
                }
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
                    let origin_mode = self.origin_mode;
                    let active = self.grid.active_mut();
                    active.scroll_region = (top - 1, bottom - 1);
                    active.cursor.row = if origin_mode { top - 1 } else { 0 };
                    active.cursor.col = 0;
                    active.cursor.pending_wrap = false;
                    active.last_placeholder_pos = None;
                    active.last_grapheme_pos = None;
                }
            }
            'c' if intermediates.is_empty() => {
                // DA1: Primary Device Attributes (VT220 Level 2 + Sixel (4) + ANSI Color (22) + OSC 52 Clipboard (52))
                if first_param == 0 {
                    self.pending_responses
                        .extend_from_slice(b"\x1b[?62;4;22;52c");
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
            'n' if is_private && intermediates.len() == 1 => match first_param {
                6 => {
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
                996 => {
                    // Color Scheme DSR query: CSI ? 996 n -> CSI ? 997 ; 1 n (dark) or 2 n (light)
                    let seq: &[u8] = if self.color_scheme_dark {
                        b"\x1b[?997;1n"
                    } else {
                        b"\x1b[?997;2n"
                    };
                    self.pending_responses.extend_from_slice(seq);
                }
                _ => {}
            },
            'p' if intermediates == b"?$" => {
                // DECRQM: Request DEC Private Mode
                let pm = match self.get_dec_private_mode(first_param) {
                    Some(true) => 1,
                    Some(false) => 2,
                    None => 0,
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
                            self.set_dec_private_mode(mode, true);
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
                            self.set_dec_private_mode(mode, false);
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
            'q' if intermediates == b" " => {
                let cur = &mut self.grid.active_mut().cursor;
                match first_param {
                    0 | 1 => {
                        cur.shape = CursorShape::Block;
                        cur.blink = true;
                    }
                    2 => {
                        cur.shape = CursorShape::Block;
                        cur.blink = false;
                    }
                    3 => {
                        cur.shape = CursorShape::Underline;
                        cur.blink = true;
                    }
                    4 => {
                        cur.shape = CursorShape::Underline;
                        cur.blink = false;
                    }
                    5 => {
                        cur.shape = CursorShape::Beam;
                        cur.blink = true;
                    }
                    6 => {
                        cur.shape = CursorShape::Beam;
                        cur.blink = false;
                    }
                    _ => {}
                }
            }
            'q' if intermediates == b">" => {
                // XTVERSION: CSI > q / CSI > 0 q -> DCS > | ghostty 1.2.0 ST
                if first_param == 0 {
                    self.pending_responses
                        .extend_from_slice(b"\x1bP>|ghostty 1.2.0\x1b\\");
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
                        if sub == 0 || sub == 1 {
                            if self.icon_title_stack.len() >= 16 {
                                self.icon_title_stack.remove(0);
                            }
                            self.icon_title_stack.push(self.icon_title.clone());
                        }
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
                        if sub == 0 || sub == 1 {
                            if let Some(prev_icon) = self.icon_title_stack.pop() {
                                self.icon_title = prev_icon;
                            }
                        }
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
                let cell_w = self.grid.cell_width_px;
                let cell_h = self.grid.cell_height_px;
                let default_fg = self.default_fg;
                let default_bg = self.default_bg;
                let default_cursor = self.default_cursor;
                let default_palette = self.default_palette;
                let color_scheme_dark = self.color_scheme_dark;
                let had_status = !self.program_statuses.is_empty();
                *self = Self::new(rows, cols, max_scroll);
                self.grid.set_cell_pixel_size(cell_w, cell_h);
                self.default_fg = default_fg;
                self.default_bg = default_bg;
                self.default_cursor = default_cursor;
                self.default_palette = default_palette;
                self.color_scheme_dark = color_scheme_dark;
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

    /// Dispatch an Operating System Command (`ESC ] <osc_bytes> BEL/ST`) without `vte`'s
    /// 1024-byte `ArrayVec` truncation limit.
    pub fn handle_osc_raw(&mut self, osc_bytes: &[u8], bell_terminated: bool) {
        if let Some(body) = osc_bytes.strip_prefix(b"1337;") {
            self.handle_osc_1337(body, bell_terminated);
            return;
        }
        if let Some(cmd_bytes) = osc_bytes.strip_prefix(b"G") {
            self.handle_kitty_graphics(cmd_bytes);
            return;
        }
        let params: Vec<&[u8]> = osc_bytes.split(|&b| b == b';').collect();
        self.osc_dispatch(&params, bell_terminated);
    }

    /// Clean up state when a single OSC sequence exceeded `MAX_APC_BYTES`.
    pub fn handle_osc_overflow(&mut self, osc_prefix: &[u8]) {
        if osc_prefix.starts_with(b"1337;") {
            self.osc1337_parser.reset();
        } else if let Some(cmd_prefix) = osc_prefix.strip_prefix(b"G") {
            self.kitty_parser.reset();
            let header_bytes = cmd_prefix.split(|&b| b == b';').next().unwrap_or(&[]);
            if let Ok(header) = std::str::from_utf8(header_bytes) {
                let mut params = KittyControlParams::default();
                params.apply_header(header);
                if let Some(resp) =
                    params.format_response(None, false, "ENOMEM:OSC payload exceeds size limit")
                {
                    self.pending_responses.extend_from_slice(&resp);
                }
            }
        }
    }

    /// Parse and execute an iTerm2 `OSC 1337 ; <body>` sequence.
    pub fn handle_osc_1337(&mut self, body: &[u8], bell_terminated: bool) {
        match self.osc1337_parser.handle_command(body) {
            Osc1337Action::Ignored | Osc1337Action::NeedMoreParts => {}
            Osc1337Action::DisplayImage {
                params,
                width_px,
                height_px,
                rgba,
            } => {
                self.place_osc1337_image(&params, width_px, height_px, rgba);
            }
            Osc1337Action::SetCursorShape(code) => {
                let shape = match code {
                    1 => CursorShape::Beam,
                    2 => CursorShape::Underline,
                    _ => CursorShape::Block,
                };
                self.grid.set_cursor_shape(shape);
            }
            Osc1337Action::SetCurrentDir(dir) => {
                self.current_working_directory = Some(dir);
            }
            Osc1337Action::ReportCellSize => {
                let terminator = if bell_terminated { "\x07" } else { "\x1b\\" };
                let ch = self.grid.cell_height_px.max(1.0);
                let cw = self.grid.cell_width_px.max(1.0);
                let resp = format!("\x1b]1337;ReportCellSize={ch:.1};{cw:.1}{terminator}");
                self.pending_responses.extend_from_slice(resp.as_bytes());
            }
            Osc1337Action::ClearScrollback => {
                let active = self.grid.active_mut();
                active.scrollback.clear();
                active.scroll_offset = 0;
                active.prompt_marks.retain(|m| m.row >= 0);
                let before = active.images.len();
                active
                    .images
                    .retain(|img| img.row + (img.height_cells.max(1) as isize) > 0);
                if active.images.len() != before {
                    active.images_dirty = true;
                }
            }
            Osc1337Action::SetMark => {
                let active = self.grid.active_mut();
                active.record_prompt_mark();
                active.semantic_zone = SemanticZone::Prompt;
                self.progress = None;
                self.progress_updated_at = None;
            }
            Osc1337Action::CopyText(text) => {
                self.pending_clipboard = Some(text.clone());
                self.push_clipboard_request(ClipboardRequest::Write(text));
            }
            Osc1337Action::RequestAttention(active) => {
                self.pending_user_attention = Some(active);
            }
        }
    }

    fn push_clipboard_request(&mut self, req: ClipboardRequest) {
        if self.pending_clipboard_requests.len() >= 32 {
            self.pending_clipboard_requests.remove(0);
        }
        self.pending_clipboard_requests.push(req);
    }

    fn push_notification(&mut self, notif: TerminalNotification) {
        if notif.title.is_empty() && notif.body.is_empty() {
            return;
        }
        if self.pending_notifications.len() >= 16 {
            self.pending_notifications.remove(0);
        }
        self.pending_notifications.push(notif);
    }

    /// Handle `OSC 9`: disambiguates ConEmu `OSC 9 ; 4 ; <state> [; <pct>]` progress bar
    /// from iTerm2 `OSC 9 ; <message>` desktop notification.
    fn handle_osc_9(&mut self, args: &[&[u8]]) {
        if args.is_empty() {
            return;
        }
        if trim_ascii_bytes(args[0]) == b"4" {
            // Bare malformed `OSC 9;4` must be ignored rather than firing a `"4"` toast.
            if args.len() < 2 {
                return;
            }
            let state_code = std::str::from_utf8(trim_ascii_bytes(args[1]))
                .ok()
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(0);
            let pct_opt = args
                .get(2)
                .and_then(|b| std::str::from_utf8(trim_ascii_bytes(b)).ok())
                .and_then(|s| s.parse::<u16>().ok())
                .map(|v| v.min(100) as u8);

            match state_code {
                0 => {
                    self.progress = None;
                    self.progress_updated_at = None;
                }
                1 => {
                    self.progress = Some(TerminalProgress {
                        state: ProgressState::Normal,
                        percent: pct_opt.unwrap_or(0),
                    });
                    self.progress_updated_at = Some(std::time::Instant::now());
                }
                2 => {
                    self.progress = Some(TerminalProgress {
                        state: ProgressState::Error,
                        percent: pct_opt.unwrap_or(100),
                    });
                    self.progress_updated_at = Some(std::time::Instant::now());
                }
                3 => {
                    self.progress = Some(TerminalProgress {
                        state: ProgressState::Indeterminate,
                        percent: 0,
                    });
                    self.progress_updated_at = Some(std::time::Instant::now());
                }
                4 => {
                    self.progress = Some(TerminalProgress {
                        state: ProgressState::Paused,
                        percent: pct_opt.unwrap_or(100),
                    });
                    self.progress_updated_at = Some(std::time::Instant::now());
                }
                _ => {}
            }
            return;
        }

        let joined = args.join(&b';');
        if let Ok(msg) = std::str::from_utf8(trim_ascii_bytes(&joined)) {
            if !msg.is_empty() {
                self.push_notification(TerminalNotification {
                    id: None,
                    title: String::from("skTerm"),
                    body: msg.to_string(),
                    force_when_focused: false,
                    only_when_invisible: false,
                    urgent: false,
                });
            }
        }
    }

    /// Handle xterm dynamic color series (`OSC 10` fg, `11` bg, `12` cursor, `17` sel bg, `19` sel fg).
    fn handle_osc_dynamic_color_series(
        &mut self,
        start_code: u8,
        args: &[&[u8]],
        terminator: &str,
    ) {
        let mut code = start_code;
        for &arg in args {
            let arg = trim_ascii_bytes(arg);
            if arg == b"?" {
                let rgb_opt = match code {
                    10 => Some(self.dynamic_fg.unwrap_or((
                        self.default_fg[0],
                        self.default_fg[1],
                        self.default_fg[2],
                    ))),
                    11 => Some(self.dynamic_bg.unwrap_or((
                        self.default_bg[0],
                        self.default_bg[1],
                        self.default_bg[2],
                    ))),
                    12 => Some(self.dynamic_cursor.unwrap_or((
                        self.default_cursor[0],
                        self.default_cursor[1],
                        self.default_cursor[2],
                    ))),
                    17 => Some(self.dynamic_selection_bg.unwrap_or((0x48, 0x52, 0x70))),
                    19 => Some(self.dynamic_selection_fg.unwrap_or((
                        self.default_fg[0],
                        self.default_fg[1],
                        self.default_fg[2],
                    ))),
                    _ => None,
                };
                if let Some((r, g, b)) = rgb_opt {
                    let rgb_str = format_osc_rgb(r, g, b);
                    let resp = format!("\x1b]{code};{rgb_str}{terminator}");
                    self.pending_responses.extend_from_slice(resp.as_bytes());
                }
            } else if let Ok(spec) = std::str::from_utf8(arg) {
                if !spec.is_empty() {
                    if let Some(rgb) = parse_osc_color_spec(spec) {
                        match code {
                            10 => self.dynamic_fg = Some(rgb),
                            11 => self.dynamic_bg = Some(rgb),
                            12 => self.dynamic_cursor = Some(rgb),
                            17 => self.dynamic_selection_bg = Some(rgb),
                            19 => self.dynamic_selection_fg = Some(rgb),
                            _ => {}
                        }
                    }
                }
            }
            code = code.saturating_add(1);
        }
    }

    /// Handle Kitty Color Control Protocol (`OSC 21 ; key=value ; ...`).
    fn handle_osc_21(&mut self, args: &[&[u8]], terminator: &str) {
        let mut resp_parts: Vec<String> = Vec::new();
        for &raw in args {
            let Ok(entry) = std::str::from_utf8(trim_ascii_bytes(raw)) else {
                continue;
            };
            if entry.is_empty() {
                continue;
            }
            let Some((raw_key, raw_val)) = entry.split_once('=') else {
                continue;
            };
            let key = raw_key.trim();
            let val = raw_val.trim();

            enum ColorSlot {
                Fg,
                Bg,
                Cursor,
                SelBg,
                SelFg,
                Palette(u8),
            }

            let slot = match key {
                "foreground" | "fg" => Some(ColorSlot::Fg),
                "background" | "bg" => Some(ColorSlot::Bg),
                "cursor" => Some(ColorSlot::Cursor),
                "selection_background" | "selection_bg" => Some(ColorSlot::SelBg),
                "selection_foreground" | "selection_fg" => Some(ColorSlot::SelFg),
                _ => key
                    .strip_prefix("color")
                    .unwrap_or(key)
                    .parse::<u8>()
                    .ok()
                    .map(ColorSlot::Palette),
            };

            let Some(slot) = slot else {
                continue;
            };

            if val == "?" {
                let (r, g, b) = match slot {
                    ColorSlot::Fg => self.dynamic_fg.unwrap_or((
                        self.default_fg[0],
                        self.default_fg[1],
                        self.default_fg[2],
                    )),
                    ColorSlot::Bg => self.dynamic_bg.unwrap_or((
                        self.default_bg[0],
                        self.default_bg[1],
                        self.default_bg[2],
                    )),
                    ColorSlot::Cursor => self.dynamic_cursor.unwrap_or((
                        self.default_cursor[0],
                        self.default_cursor[1],
                        self.default_cursor[2],
                    )),
                    ColorSlot::SelBg => self.dynamic_selection_bg.unwrap_or((0x48, 0x52, 0x70)),
                    ColorSlot::SelFg => self.dynamic_selection_fg.unwrap_or((
                        self.default_fg[0],
                        self.default_fg[1],
                        self.default_fg[2],
                    )),
                    ColorSlot::Palette(idx) => self
                        .dynamic_palette
                        .get(&idx)
                        .copied()
                        .unwrap_or_else(|| self.default_indexed_color(idx)),
                };
                resp_parts.push(format!("{key}=rgb:{r:02x}/{g:02x}/{b:02x}"));
            } else if val.is_empty() {
                match slot {
                    ColorSlot::Fg => self.dynamic_fg = None,
                    ColorSlot::Bg => self.dynamic_bg = None,
                    ColorSlot::Cursor => self.dynamic_cursor = None,
                    ColorSlot::SelBg => self.dynamic_selection_bg = None,
                    ColorSlot::SelFg => self.dynamic_selection_fg = None,
                    ColorSlot::Palette(idx) => {
                        self.dynamic_palette.remove(&idx);
                    }
                }
            } else if let Some(rgb) = parse_osc_color_spec(val) {
                match slot {
                    ColorSlot::Fg => self.dynamic_fg = Some(rgb),
                    ColorSlot::Bg => self.dynamic_bg = Some(rgb),
                    ColorSlot::Cursor => self.dynamic_cursor = Some(rgb),
                    ColorSlot::SelBg => self.dynamic_selection_bg = Some(rgb),
                    ColorSlot::SelFg => self.dynamic_selection_fg = Some(rgb),
                    ColorSlot::Palette(idx) => {
                        self.dynamic_palette.insert(idx, rgb);
                    }
                }
            }
        }
        if !resp_parts.is_empty() {
            let resp = format!("\x1b]21;{}{terminator}", resp_parts.join(";"));
            self.pending_responses.extend_from_slice(resp.as_bytes());
        }
    }

    /// Handle `OSC 22` Mouse Pointer Cursor Shape set / stack push (`>`) / pop (`<`) / query (`?`).
    fn handle_osc_22(&mut self, args: &[&[u8]], terminator: &str) {
        if args.is_empty() {
            self.pointer_shape = PointerShape::Default;
            if let Some(top) = self.pointer_shape_stack.last_mut() {
                *top = PointerShape::Default;
            }
            return;
        }
        let joined = args.join(&b';');
        let Ok(spec) = std::str::from_utf8(trim_ascii_bytes(&joined)) else {
            return;
        };
        if spec.is_empty() {
            self.pointer_shape = PointerShape::Default;
            if let Some(top) = self.pointer_shape_stack.last_mut() {
                *top = PointerShape::Default;
            }
            return;
        }
        if spec == "<" {
            if let Some(popped) = self.pointer_shape_stack.pop() {
                if self.pointer_shape_stack.is_empty() {
                    let _ = popped;
                }
            } else {
                self.pointer_shape = PointerShape::Default;
            }
            return;
        }
        if let Some(query_rest) = spec.strip_prefix('?') {
            let q = query_rest.trim();
            if q.is_empty() || q == "__current__" {
                let name = self.active_pointer_shape().as_css_name();
                let resp = format!("\x1b]22;{name}{terminator}");
                self.pending_responses.extend_from_slice(resp.as_bytes());
            } else {
                let flags: Vec<&str> = q
                    .split(',')
                    .map(|n| {
                        if PointerShape::from_css_name(n).is_some() {
                            "1"
                        } else {
                            "0"
                        }
                    })
                    .collect();
                let resp = format!("\x1b]22;{}{terminator}", flags.join(","));
                self.pending_responses.extend_from_slice(resp.as_bytes());
            }
            return;
        }

        let (is_push, names_str) = if let Some(rest) = spec.strip_prefix('>') {
            (true, rest)
        } else if let Some(rest) = spec.strip_prefix('=') {
            (false, rest)
        } else {
            (false, spec)
        };

        let resolved = names_str
            .split(',')
            .find_map(PointerShape::from_css_name)
            .unwrap_or(PointerShape::Default);

        if is_push {
            if self.pointer_shape_stack.len() >= 16 {
                self.pointer_shape_stack.remove(0);
            }
            self.pointer_shape_stack.push(resolved);
        } else if let Some(top) = self.pointer_shape_stack.last_mut() {
            *top = resolved;
        } else {
            self.pointer_shape = resolved;
        }
    }

    /// Handle `OSC 52 ; <selection> ; <payload|?|empty>` clipboard write, clear, and query.
    fn handle_osc_52(&mut self, args: &[&[u8]], bell_terminated: bool) {
        if args.len() < 2 {
            return;
        }
        let sel_raw = std::str::from_utf8(trim_ascii_bytes(args[0])).unwrap_or("c");
        let selection = if sel_raw.is_empty() {
            String::from("c")
        } else {
            sel_raw.to_string()
        };
        let payload = trim_ascii_bytes(args[1]);

        if payload == b"?" {
            self.push_clipboard_request(ClipboardRequest::Query {
                selection,
                bell_terminated,
            });
        } else if payload.is_empty() {
            self.pending_clipboard = None;
            self.push_clipboard_request(ClipboardRequest::Clear);
        } else if let Ok(decoded) = decode_b64_flexible(payload) {
            if let Ok(text) = String::from_utf8(decoded) {
                self.pending_clipboard = Some(text.clone());
                self.push_clipboard_request(ClipboardRequest::Write(text));
            }
        }
    }

    /// Handle Kitty Desktop Notification Protocol (`OSC 99 ; <metadata> ; <payload>`).
    fn handle_osc_99(&mut self, args: &[&[u8]]) {
        if args.is_empty() {
            return;
        }
        let meta_str = std::str::from_utf8(trim_ascii_bytes(args[0])).unwrap_or("");
        let payload_bytes: std::borrow::Cow<'_, [u8]> = if args.len() <= 1 {
            std::borrow::Cow::Borrowed(b"")
        } else if args.len() == 2 {
            std::borrow::Cow::Borrowed(args[1])
        } else {
            std::borrow::Cow::Owned(args[1..].join(&b';'))
        };

        let mut id = String::new();
        let mut done = true;
        let mut part = "title";
        let mut base64_encoded = false;
        let mut occasion: Option<&str> = None;
        let mut urgency: Option<u8> = None;

        for kv in meta_str.split(':') {
            let Some((k, v)) = kv.split_once('=') else {
                continue;
            };
            match k.trim() {
                "i" => id = v.trim().to_string(),
                "d" => done = v.trim() != "0",
                "p" => part = v.trim(),
                "e" => base64_encoded = v.trim() == "1",
                "o" => occasion = Some(v.trim()),
                "u" => urgency = v.trim().parse::<u8>().ok(),
                _ => {}
            }
        }

        if part == "?" {
            let id_prefix = if id.is_empty() {
                String::new()
            } else {
                format!("i={id}:")
            };
            let resp = format!(
                "\x1b]99;{id_prefix}p=?;title,body,close:o=always,unfocused,invisible:u=0,1,2\x1b\\"
            );
            self.pending_responses.extend_from_slice(resp.as_bytes());
            return;
        }
        if part == "close" || part == "alive" {
            if !id.is_empty() {
                self.osc99_drafts.remove(&id);
            }
            return;
        }

        let decoded_text = if base64_encoded {
            decode_b64_flexible(&payload_bytes)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .unwrap_or_default()
        } else {
            String::from_utf8_lossy(&payload_bytes).into_owned()
        };

        let draft_key = id.clone();
        if self.osc99_drafts.len() >= 16 && !self.osc99_drafts.contains_key(&draft_key) {
            self.osc99_drafts.clear();
        }
        let draft = self.osc99_drafts.entry(draft_key.clone()).or_default();
        if let Some(occ) = occasion {
            draft.force_when_focused = occ == "always";
            draft.only_when_invisible = occ == "invisible";
        }
        if let Some(u) = urgency {
            draft.urgent = u >= 2;
        }
        match part {
            "body" => draft.body.push_str(&decoded_text),
            _ => draft.title.push_str(&decoded_text),
        }

        if done {
            if let Some(fin) = self.osc99_drafts.remove(&draft_key) {
                let (title, body) = if fin.body.is_empty() {
                    (String::from("skTerm"), fin.title)
                } else if fin.title.is_empty() {
                    (String::from("skTerm"), fin.body)
                } else {
                    (fin.title, fin.body)
                };
                self.push_notification(TerminalNotification {
                    id: (!id.is_empty()).then_some(id),
                    title,
                    body,
                    force_when_focused: fin.force_when_focused,
                    only_when_invisible: fin.only_when_invisible,
                    urgent: fin.urgent,
                });
            }
        }
    }

    /// Handle rxvt / VTE / Ghostty Desktop Notification (`OSC 777 ; notify ; <title> ; <body>`).
    fn handle_osc_777(&mut self, args: &[&[u8]]) {
        if args.len() < 2 {
            return;
        }
        let subcmd = std::str::from_utf8(trim_ascii_bytes(args[0])).unwrap_or("");
        if !subcmd.eq_ignore_ascii_case("notify") {
            return;
        }
        let title_str = std::str::from_utf8(trim_ascii_bytes(args[1])).unwrap_or("skTerm");
        let body_str = if args.len() >= 3 {
            let joined = args[2..].join(&b';');
            String::from_utf8_lossy(trim_ascii_bytes(&joined)).into_owned()
        } else {
            String::new()
        };
        let (title, body) = if body_str.is_empty() {
            (String::from("skTerm"), title_str.to_string())
        } else {
            (
                if title_str.is_empty() {
                    String::from("skTerm")
                } else {
                    title_str.to_string()
                },
                body_str,
            )
        };
        self.push_notification(TerminalNotification {
            id: None,
            title,
            body,
            force_when_focused: false,
            only_when_invisible: false,
            urgent: false,
        });
    }

    /// Handle FinalTerm `OSC 133` and VS Code `OSC 633` semantic shell integration sequences.
    fn handle_osc_semantic_prompt(&mut self, args: &[&[u8]], is_633: bool) {
        if args.is_empty() {
            return;
        }
        let kind = trim_ascii_bytes(args[0]);
        match kind {
            b"A" => {
                let active = self.grid.active_mut();
                active.record_prompt_mark();
                active.semantic_zone = SemanticZone::Prompt;
                self.progress = None;
                self.progress_updated_at = None;
            }
            b"B" => {
                self.grid.active_mut().semantic_zone = SemanticZone::CommandInput;
            }
            b"C" => {
                self.grid.active_mut().semantic_zone = SemanticZone::CommandOutput;
            }
            b"D" => {
                let exit_code = args
                    .get(1)
                    .and_then(|b| std::str::from_utf8(trim_ascii_bytes(b)).ok())
                    .and_then(|s| s.parse::<i32>().ok());
                let active = self.grid.active_mut();
                active.complete_last_prompt_mark(exit_code);
                active.semantic_zone = SemanticZone::Idle;
                self.progress = None;
                self.progress_updated_at = None;
            }
            b"P" if is_633 => {
                for &prop in &args[1..] {
                    if let Ok(s) = std::str::from_utf8(trim_ascii_bytes(prop)) {
                        if let Some(cwd_raw) = s.strip_prefix("Cwd=") {
                            let unescaped = unescape_osc633_value(cwd_raw);
                            if !unescaped.is_empty() {
                                self.current_working_directory = Some(unescaped);
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn place_osc1337_image(
        &mut self,
        params: &Osc1337FileParams,
        width_px: u32,
        height_px: u32,
        rgba: Vec<u8>,
    ) {
        if !params.do_not_move_cursor && self.grid.active().cursor.pending_wrap {
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

        let (anchor_row, anchor_col, pane_rows, pane_cols) = {
            let active = self.grid.active();
            (
                active.cursor.row as isize,
                active.cursor.col,
                active.rows,
                active.cols,
            )
        };
        let cell_w = self.grid.cell_width_px.max(1.0);
        let cell_h = self.grid.cell_height_px.max(1.0);
        let layout = params.compute_layout(
            width_px, height_px, cell_w, cell_h, pane_cols, pane_rows, anchor_col,
        );

        let id = self.grid.alloc_texture_id();
        let img = GraphicImage {
            id,
            image_id: 0,
            image_number: 0,
            placement_id: 0,
            row: anchor_row,
            col: anchor_col,
            width_px,
            height_px,
            src_rect: [0, 0, width_px, height_px],
            cell_offset: [0, 0],
            display_cols: None,
            display_rows: None,
            custom_display_cells: Some(layout.custom_display_cells),
            width_cells: layout.width_cells,
            height_cells: layout.height_cells,
            rgba_data: Arc::new(rgba),
            z_index: 0,
        };

        info!(
            "Placed OSC 1337 inline image id={} ({}x{} px, {}x{} cells) at ({}, {})",
            id,
            width_px,
            height_px,
            layout.width_cells,
            layout.height_cells,
            anchor_row,
            anchor_col
        );
        self.grid.active_mut().add_image(img);

        if !params.do_not_move_cursor {
            let bg = self.bg;
            let auto_wrap = self.auto_wrap;
            let active = self.grid.active_mut();
            for _ in 0..layout.height_cells.saturating_sub(1) {
                active.linefeed_with_bg(bg);
            }
            let end_col = anchor_col + layout.width_cells;
            if end_col < active.cols {
                active.cursor.col = end_col;
                active.cursor.pending_wrap = false;
            } else {
                active.cursor.col = active.cols.saturating_sub(1);
                active.cursor.pending_wrap = auto_wrap;
            }
            active.last_placeholder_pos = None;
        }
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
                    Ok((image_id, frame_num)) => {
                        if let Some(resp) =
                            params.format_frame_response(Some(image_id), frame_num, true, "OK")
                        {
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
            custom_display_cells: None,
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
                let target_ids: Vec<u32> = if params.image_id != 0 {
                    vec![params.image_id]
                } else if params.image_number != 0 {
                    self.grid
                        .kitty_number_to_id
                        .get(&params.image_number)
                        .copied()
                        .into_iter()
                        .collect()
                } else {
                    // `i=0` and `I=0`: apply to all stored Kitty images.
                    self.grid.kitty_images.keys().copied().collect()
                };

                let mut to_refresh = Vec::new();
                for id in target_ids {
                    if let Some(stored) = self.grid.kitty_images.get_mut(&id) {
                        let was_active_nonzero = stored.active_frame != 0;
                        stored.animation_state = AnimationState::Stopped;
                        stored.next_frame_at = None;
                        stored.loops_completed = 0;

                        if free_data {
                            // Uppercase `d=F`: drop all extra animation frames and reclaim storage bytes.
                            let old_bytes = stored.byte_len();
                            if stored.frames.len() > 1 {
                                stored.frames.truncate(1);
                                stored.frames.shrink_to_fit();
                            }
                            if !stored.frames.is_empty() {
                                stored.active_frame = 0;
                                stored.rgba_data = Arc::clone(&stored.frames[0].rgba_data);
                            }
                            let new_bytes = stored.byte_len();
                            self.grid.kitty_storage_bytes = self
                                .grid
                                .kitty_storage_bytes
                                .saturating_sub(old_bytes.saturating_sub(new_bytes));
                            if was_active_nonzero || old_bytes != new_bytes {
                                to_refresh.push(id);
                            }
                        } else if !stored.frames.is_empty() {
                            // Lowercase `d=f`: stop animation and reset display to base frame without freeing extra frames.
                            stored.active_frame = 0;
                            stored.rgba_data = Arc::clone(&stored.frames[0].rgba_data);
                            if was_active_nonzero {
                                to_refresh.push(id);
                            }
                        }
                    }
                }
                for id in to_refresh {
                    self.refresh_kitty_texture_bindings(id);
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
    ) -> Result<(u32, u32), (&'static str, &'static str)> {
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
        let explicit_gap_ms = if params.has_z {
            Some(if params.z_index < 0 {
                0
            } else {
                params.z_index as u32
            })
        } else {
            None
        };
        let fill = params.cell_y_offset.to_be_bytes();

        let (frame_num, old_bytes, new_bytes) = {
            let stored = self
                .grid
                .kitty_images
                .get_mut(&image_id)
                .ok_or(("ENOENT", "Image not found"))?;
            let old_bytes = stored.byte_len();

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

            let frame_num = if let Some(idx) = edit_idx {
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
                if let Some(gap) = explicit_gap_ms {
                    stored.frames[idx].gap_ms = gap;
                }
                if stored.active_frame == idx {
                    stored.rgba_data = arc;
                }
                (idx + 1) as u32
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
                let gap_ms = match explicit_gap_ms {
                    Some(0) if params.z_index < 0 => 0,
                    Some(g) if g > 0 => g,
                    _ => 40,
                };
                stored.frames.push(AnimationFrame {
                    frame_number: frame_num,
                    rgba_data: arc,
                    gap_ms,
                });
                // If an image in `Loading` mode was parked at the previous last frame waiting
                // for new frames, schedule advancing into the newly arrived frame.
                if stored.animation_state == AnimationState::Loading
                    && stored.next_frame_at.is_none()
                {
                    let cur_gap = stored
                        .frames
                        .get(stored.active_frame)
                        .map(|f| f.gap_ms.max(1))
                        .unwrap_or(40);
                    stored.next_frame_at = Some(
                        std::time::Instant::now()
                            + std::time::Duration::from_millis(cur_gap as u64),
                    );
                }
                frame_num
            };

            let new_bytes = stored.byte_len();
            (frame_num, old_bytes, new_bytes)
        };

        self.grid.kitty_storage_bytes = self
            .grid
            .kitty_storage_bytes
            .saturating_sub(old_bytes)
            .saturating_add(new_bytes);
        self.grid.enforce_kitty_storage_quota(image_id);
        self.refresh_kitty_texture_bindings(image_id);
        Ok((image_id, frame_num))
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
        let now = std::time::Instant::now();
        let switched = {
            let stored = self
                .grid
                .kitty_images
                .get_mut(&image_id)
                .ok_or(("ENOENT", "Image not found"))?;

            if params.rows > 0 && params.has_z {
                let idx = (params.rows - 1) as usize;
                if let Some(frame) = stored.frames.get_mut(idx) {
                    frame.gap_ms = params.z_index.max(0) as u32;
                }
            }

            if params.data_height > 0 {
                stored.loop_count = params.data_height;
                stored.loops_completed = 0;
            }

            let mut frame_switched = false;
            if params.cols > 0 {
                let idx = (params.cols - 1) as usize;
                if let Some(frame) = stored.frames.get(idx) {
                    stored.active_frame = idx;
                    stored.rgba_data = Arc::clone(&frame.rgba_data);
                    frame_switched = true;
                }
            }

            match params.data_width {
                1 => {
                    stored.animation_state = AnimationState::Stopped;
                    stored.next_frame_at = None;
                }
                2 => {
                    stored.animation_state = AnimationState::Loading;
                    stored.loops_completed = 0;
                    if stored.frames.len() > 1 && stored.active_frame + 1 < stored.frames.len() {
                        let gap = stored
                            .frames
                            .get(stored.active_frame)
                            .map(|f| f.gap_ms.max(1))
                            .unwrap_or(40);
                        stored.next_frame_at =
                            Some(now + std::time::Duration::from_millis(gap as u64));
                    } else {
                        stored.next_frame_at = None;
                    }
                }
                3 => {
                    stored.animation_state = AnimationState::Running;
                    stored.loops_completed = 0;
                    if stored.frames.len() > 1 {
                        let gap = stored
                            .frames
                            .get(stored.active_frame)
                            .map(|f| f.gap_ms.max(1))
                            .unwrap_or(40);
                        stored.next_frame_at =
                            Some(now + std::time::Duration::from_millis(gap as u64));
                    } else {
                        stored.next_frame_at = None;
                    }
                }
                _ => {
                    if frame_switched && stored.animation_state != AnimationState::Stopped {
                        let gap = stored
                            .frames
                            .get(stored.active_frame)
                            .map(|f| f.gap_ms.max(1))
                            .unwrap_or(40);
                        stored.next_frame_at =
                            Some(now + std::time::Duration::from_millis(gap as u64));
                    }
                }
            }

            frame_switched
        };

        if switched {
            self.refresh_kitty_texture_bindings(image_id);
        }
        Ok(image_id)
    }

    /// Advance active Kitty Graphics animations due at or before `now`.
    /// Returns `(any_frame_changed, earliest_next_deadline)`.
    pub fn tick_animations(
        &mut self,
        now: std::time::Instant,
    ) -> (bool, Option<std::time::Instant>) {
        let mut advanced_ids: Vec<u32> = Vec::new();
        let mut earliest_deadline: Option<std::time::Instant> = None;

        for stored in self.grid.kitty_images.values_mut() {
            if stored.animation_state == AnimationState::Stopped || stored.frames.len() <= 1 {
                stored.next_frame_at = None;
                continue;
            }
            if let Some(due) = stored.next_frame_at {
                if now >= due {
                    if stored.active_frame + 1 < stored.frames.len() {
                        stored.active_frame += 1;
                        stored.rgba_data =
                            Arc::clone(&stored.frames[stored.active_frame].rgba_data);
                        advanced_ids.push(stored.image_id);
                        let gap = stored.frames[stored.active_frame].gap_ms.max(1);
                        stored.next_frame_at =
                            Some(now + std::time::Duration::from_millis(gap as u64));
                    } else {
                        match stored.animation_state {
                            AnimationState::Loading => {
                                stored.next_frame_at = None;
                            }
                            AnimationState::Running => {
                                stored.loops_completed = stored.loops_completed.saturating_add(1);
                                let max_loops = if stored.loop_count <= 1 {
                                    None
                                } else {
                                    Some(stored.loop_count - 1)
                                };
                                if let Some(limit) = max_loops {
                                    if stored.loops_completed >= limit {
                                        stored.animation_state = AnimationState::Stopped;
                                        stored.next_frame_at = None;
                                        continue;
                                    }
                                }
                                stored.active_frame = 0;
                                stored.rgba_data = Arc::clone(&stored.frames[0].rgba_data);
                                advanced_ids.push(stored.image_id);
                                let gap = stored.frames[0].gap_ms.max(1);
                                stored.next_frame_at =
                                    Some(now + std::time::Duration::from_millis(gap as u64));
                            }
                            AnimationState::Stopped => {
                                stored.next_frame_at = None;
                            }
                        }
                    }
                }
            }
            if let Some(next) = stored.next_frame_at {
                earliest_deadline = Some(earliest_deadline.map_or(next, |cur| cur.min(next)));
            }
        }

        let dirty = !advanced_ids.is_empty();
        for id in advanced_ids {
            self.refresh_kitty_texture_bindings(id);
        }
        (dirty, earliest_deadline)
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

fn default_ansi_palette_16() -> [[u8; 3]; 16] {
    let mut out = [[0u8; 3]; 16];
    for (i, slot) in out.iter_mut().enumerate() {
        let (r, g, b) = default_indexed_rgb(i as u8);
        *slot = [r, g, b];
    }
    out
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

/// Extract explicit `id=<value>` from an `OSC 8 ; <params> ; <uri>` colon-separated parameter list.
fn extract_osc8_id(params_str: &str) -> Option<&str> {
    for kv in params_str.split(':') {
        if let Some(id) = kv.strip_prefix("id=") {
            if !id.is_empty() {
                return Some(id);
            }
        }
    }
    None
}

/// Unescape VS Code `OSC 633 ; P ; Cwd=...` values (`\xNN` hex escapes and `\\`).
fn unescape_osc633_value(raw: &str) -> String {
    let bytes = raw.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' && i + 1 < bytes.len() {
            if bytes[i + 1] == b'x' && i + 3 < bytes.len() {
                if let Ok(hex_str) = std::str::from_utf8(&bytes[i + 2..i + 4]) {
                    if let Ok(val) = u8::from_str_radix(hex_str, 16) {
                        out.push(val);
                        i += 4;
                        continue;
                    }
                }
            } else if bytes[i + 1] == b'\\' {
                out.push(b'\\');
                i += 2;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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
        "Tc" | "bce" | "am" | "xenl" | "msgr" | "Su" | "Hls" | "fullkbd" => Some(b""),
        "Smulx" => Some(b"\x1b[4:%p1%dm"),
        "Setulc" => Some(b"\x1b[58:2::%p1%{65536}%/%d:%p1%{256}%/%{255}%&%d:%p1%{255}%&%dm"),
        "sync" | "Sync" => Some(b"\x1b[?2026%?%p1%{1}%-%tl%eh%;"),
        "Smol" => Some(b"\x1b[53m"),
        "Rmol" => Some(b"\x1b[55m"),
        "BE" | "ED" => Some(b"\x1b[?2004h"),
        "BD" => Some(b"\x1b[?2004l"),
        "Cs" => Some(b"\x1b]12;%p1%s\x07"),
        "Cr" => Some(b"\x1b]112\x07"),
        "Se" => Some(b"\x1b[2 q"),
        "Ss" => Some(b"\x1b[%p1%d q"),
        "Ms" => Some(b"\x1b]52;%p1%s;%p2%s\x07"),
        _ => None,
    }
}
