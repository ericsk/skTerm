//! High-level Terminal engine integrating PTY, VTE Parser, and ScreenGrid.

use crate::grid::{CursorShape, ScreenGrid};
use crate::pty::{PtyEvent, PtyProcess, PtySpawnOptions};
use crate::vt::TerminalState;
use vte::Parser;

/// Maximum bytes buffered for a single APC (`ESC _ ... ST`) sequence (64 MiB).
const MAX_APC_BYTES: usize = 64 * 1024 * 1024;

/// Streaming pre-parser state for intercepting Application Program Command (`ESC _ ... ST`)
/// sequences before `vte::Parser` discards them in `SosPmApcString`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ApcState {
    #[default]
    Normal,
    SawEsc,
    InApc,
    InApcEsc,
}

/// The complete terminal instance combining terminal state, parser, and child PTY process.
pub struct Terminal {
    pub state: TerminalState,
    parser: Parser,
    pty: Option<PtyProcess>,
    pub last_output_time: std::time::Instant,
    pub custom_label: Option<String>,
    /// Reused across `poll_pty` calls so steady-state reading allocates nothing.
    read_buf: Vec<u8>,
    /// Streaming APC (`\x1b_...\x1b\\`) state machine for Kitty Graphics Protocol.
    apc_state: ApcState,
    apc_buf: Vec<u8>,
    apc_overflowed: bool,
    /// Last `(pixel_width, pixel_height)` reported to the kernel PTY via `TIOCSWINSZ`.
    last_pty_pixels: (u16, u16),
    /// Set once `PtyEvent::Exit` has been observed. `Some(None)` means the child exited
    /// but no status could be retrieved.
    exit_code: Option<Option<u32>>,
    has_exited: bool,
}

impl Terminal {
    /// Create a new interactive Terminal running a child shell in a PTY.
    pub fn new(
        rows: usize,
        cols: usize,
        max_scrollback: usize,
        custom_shell: Option<String>,
        cwd: Option<String>,
    ) -> anyhow::Result<Self> {
        Self::new_with_options(
            rows,
            cols,
            max_scrollback,
            PtySpawnOptions {
                shell: custom_shell,
                cwd,
                ..Default::default()
            },
        )
    }

    /// Create a new interactive Terminal with custom PTY spawn options.
    pub fn new_with_options(
        rows: usize,
        cols: usize,
        max_scrollback: usize,
        options: PtySpawnOptions,
    ) -> anyhow::Result<Self> {
        let pty = PtyProcess::spawn_with_options(rows as u16, cols as u16, options)?;
        let state = TerminalState::new(rows, cols, max_scrollback);
        let parser = Parser::new();

        Ok(Self {
            state,
            parser,
            pty: Some(pty),
            last_output_time: std::time::Instant::now(),
            custom_label: None,
            read_buf: Vec::new(),
            apc_state: ApcState::Normal,
            apc_buf: Vec::new(),
            apc_overflowed: false,
            last_pty_pixels: (0, 0),
            exit_code: None,
            has_exited: false,
        })
    }

    /// Create a headless Terminal without an attached PTY process (ideal for unit tests).
    pub fn new_headless(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            state: TerminalState::new(rows, cols, max_scrollback),
            parser: Parser::new(),
            pty: None,
            last_output_time: std::time::Instant::now(),
            custom_label: None,
            read_buf: Vec::new(),
            apc_state: ApcState::Normal,
            apc_buf: Vec::new(),
            apc_overflowed: false,
            last_pty_pixels: (0, 0),
            exit_code: None,
            has_exited: false,
        }
    }

    /// Feed byte stream (from PTY or test) into the APC + VTE parser pipeline to update state.
    pub fn feed_bytes(&mut self, bytes: &[u8]) {
        // Zero-overhead fast path when not mid-APC and no ESC byte is present in `bytes`.
        if self.apc_state == ApcState::Normal && !bytes.contains(&0x1B) {
            self.parser.advance(&mut self.state, bytes);
            return;
        }

        let mut i = 0;
        while i < bytes.len() {
            match self.apc_state {
                ApcState::Normal => {
                    if let Some(rel) = bytes[i..].iter().position(|&b| b == 0x1B) {
                        if rel > 0 {
                            self.parser.advance(&mut self.state, &bytes[i..i + rel]);
                        }
                        i += rel + 1;
                        self.apc_state = ApcState::SawEsc;
                    } else {
                        self.parser.advance(&mut self.state, &bytes[i..]);
                        break;
                    }
                }
                ApcState::SawEsc => {
                    let b = bytes[i];
                    if b == b'_' {
                        i += 1;
                        self.parser.advance(&mut self.state, b"\x1b_");
                        self.apc_buf.clear();
                        self.apc_overflowed = false;
                        self.apc_state = ApcState::InApc;
                    } else if b == 0x1B {
                        self.parser.advance(&mut self.state, b"\x1b");
                        i += 1;
                    } else {
                        self.parser.advance(&mut self.state, b"\x1b");
                        self.apc_state = ApcState::Normal;
                    }
                }
                ApcState::InApc => {
                    let slice = &bytes[i..];
                    let delim_pos = slice
                        .iter()
                        .position(|&b| matches!(b, 0x1B | 0x07 | 0x18 | 0x1A));
                    match delim_pos {
                        None => {
                            self.push_apc_slice(slice);
                            break;
                        }
                        Some(rel) => {
                            if rel > 0 {
                                self.push_apc_slice(&slice[..rel]);
                            }
                            let delim = slice[rel];
                            i += rel + 1;
                            match delim {
                                0x1B => {
                                    self.apc_state = ApcState::InApcEsc;
                                }
                                0x07 => {
                                    self.parser.advance(&mut self.state, b"\x1b\\");
                                    self.finish_apc();
                                }
                                _ => {
                                    // CAN (0x18) or SUB (0x1A) aborts the control string.
                                    self.parser.advance(&mut self.state, &[delim]);
                                    self.apc_buf.clear();
                                    self.apc_overflowed = false;
                                    self.apc_state = ApcState::Normal;
                                }
                            }
                        }
                    }
                }
                ApcState::InApcEsc => {
                    let b = bytes[i];
                    if b == b'\\' {
                        i += 1;
                        self.parser.advance(&mut self.state, b"\x1b\\");
                        self.finish_apc();
                    } else if b == b'_' {
                        // Interrupted APC immediately followed by a new APC (`ESC _`).
                        i += 1;
                        self.parser.advance(&mut self.state, b"\x1b_");
                        self.apc_buf.clear();
                        self.apc_overflowed = false;
                        self.apc_state = ApcState::InApc;
                    } else {
                        // Interrupted APC followed by another ESC sequence (e.g. `ESC [`):
                        // transition `vte` into `Escape` and let `Normal` feed `b` onward.
                        self.apc_buf.clear();
                        self.apc_overflowed = false;
                        self.parser.advance(&mut self.state, b"\x1b");
                        self.apc_state = ApcState::Normal;
                    }
                }
            }
        }
    }

    #[inline]
    fn push_apc_slice(&mut self, slice: &[u8]) {
        if self.apc_overflowed {
            return;
        }
        if self.apc_buf.len().saturating_add(slice.len()) > MAX_APC_BYTES {
            let header_keep = (256usize).saturating_sub(self.apc_buf.len());
            if header_keep > 0 {
                let take = header_keep.min(slice.len());
                self.apc_buf.extend_from_slice(&slice[..take]);
            }
            self.apc_overflowed = true;
        } else {
            self.apc_buf.extend_from_slice(slice);
        }
    }

    fn finish_apc(&mut self) {
        let payload = std::mem::take(&mut self.apc_buf);
        let overflowed = self.apc_overflowed;
        self.apc_overflowed = false;
        self.apc_state = ApcState::Normal;

        if overflowed {
            self.state.handle_apc_overflow(&payload);
        } else {
            self.state.handle_apc(&payload);
        }
        // Retain allocation capacity up to 64 KiB so normal chunks avoid reallocating,
        // without holding multi-megabyte buffers indefinitely after a large image upload.
        if payload.capacity() <= 65536 {
            self.apc_buf = payload;
            self.apc_buf.clear();
        }
    }

    /// Drain available output from the PTY process and update terminal state.
    /// Returns `true` if any data was processed.
    ///
    /// Reads are accumulated into one reusable contiguous buffer and handed to the
    /// parser in a single `advance` call: this avoids re-entering the VTE state machine
    /// per 8 KiB chunk and keeps locality. At most `MAX_BYTES_PER_POLL` is consumed per
    /// call so that `cat`-ing a huge file cannot stall input handling and rendering --
    /// anything left over stays queued and is picked up on the next poll.
    pub fn poll_pty(&mut self) -> bool {
        /// Roughly a few frames' worth of output at typical terminal throughput.
        const MAX_BYTES_PER_POLL: usize = 4 * 1024 * 1024;

        let mut buf = std::mem::take(&mut self.read_buf);
        buf.clear();

        if let Some(pty) = &self.pty {
            while buf.len() < MAX_BYTES_PER_POLL {
                match pty.try_recv() {
                    Some(PtyEvent::Output(bytes)) => buf.extend_from_slice(&bytes),
                    Some(PtyEvent::Exit(code)) => {
                        // Previously discarded. Recording it lets the UI report the exit
                        // status and lets `is_child_alive` answer without a syscall.
                        self.exit_code = Some(code);
                        self.has_exited = true;
                    }
                    None => break,
                }
            }
        }

        let had_data = !buf.is_empty();
        if had_data {
            self.last_output_time = std::time::Instant::now();
            self.feed_bytes(&buf);
            if !self.state.pending_responses.is_empty() {
                let responses = std::mem::take(&mut self.state.pending_responses);
                if let Some(pty) = &self.pty {
                    let _ = pty.write_all(&responses);
                }
            }
        }

        // Return the buffer so its capacity is reused next poll.
        self.read_buf = buf;
        had_data
    }

    /// Check if the attached PTY process has exited.
    pub fn is_child_alive(&self) -> bool {
        // Fast path: the reader thread already told us the child is gone, so there is
        // no need to ask the OS again.
        if self.has_exited {
            return false;
        }
        self.pty.as_ref().map(|p| p.is_alive()).unwrap_or(false)
    }

    /// Alias to is_child_alive.
    pub fn is_alive(&self) -> bool {
        self.is_child_alive()
    }

    /// Exit status of the child, if it has exited and a status was available.
    ///
    /// Returns `None` while the child is running, `Some(None)` if it exited but no
    /// status could be retrieved, and `Some(Some(code))` otherwise.
    pub fn exit_code(&self) -> Option<Option<u32>> {
        self.exit_code
    }

    /// Return the OS process ID of the attached shell process.
    pub fn process_id(&self) -> Option<u32> {
        self.pty.as_ref().and_then(|p| p.process_id())
    }

    /// Return true if the terminal has received output within the last 1.5 seconds (Active / Thinking).
    pub fn is_active_output(&self) -> bool {
        self.last_output_time.elapsed() < std::time::Duration::from_millis(1500)
    }

    /// Write user input directly to the PTY process.
    pub fn write_to_pty(&self, data: &[u8]) -> anyhow::Result<()> {
        if let Some(pty) = &self.pty {
            pty.write_all(data)?;
        }
        Ok(())
    }

    /// Notify PTY and Grid of window resize.
    ///
    /// No-ops when the size is unchanged. This matters because `resize` is called from
    /// many UI paths (splitter drags, every layout sync, most keyboard shortcuts) and
    /// both halves are expensive: `grid.resize` reflows the entire scrollback, and
    /// `pty.resize` is a `TIOCSWINSZ` ioctl that sends `SIGWINCH` to the child --
    /// spamming it at mouse-move rate makes vim/htop redraw continuously.
    pub fn resize(&mut self, rows: usize, cols: usize) -> anyhow::Result<()> {
        let cell_w = self.state.grid.cell_width_px;
        let cell_h = self.state.grid.cell_height_px;
        self.resize_with_cell_size(rows, cols, cell_w, cell_h)
    }

    /// Notify PTY and Grid of window resize along with exact cell physical pixel dimensions
    /// (`cell_w`, `cell_h`) so `TIOCGWINSZ`, `CSI 14 t`, `CSI 16 t`, Sixel, and Kitty Graphics
    /// placements all compute exact pixel/cell ratios.
    pub fn resize_with_cell_size(
        &mut self,
        rows: usize,
        cols: usize,
        cell_w: f32,
        cell_h: f32,
    ) -> anyhow::Result<()> {
        let cell_w = if cell_w > 0.0 { cell_w } else { 9.0 };
        let cell_h = if cell_h > 0.0 { cell_h } else { 18.0 };
        self.state.grid.set_cell_pixel_size(cell_w, cell_h);

        let pixel_w = (cols as f32 * cell_w).round().clamp(0.0, u16::MAX as f32) as u16;
        let pixel_h = (rows as f32 * cell_h).round().clamp(0.0, u16::MAX as f32) as u16;

        let active = self.state.grid.active();
        let grid_changed = active.rows != rows || active.cols != cols;
        let pixels_changed = self.last_pty_pixels != (pixel_w, pixel_h);

        if !grid_changed && !pixels_changed {
            return Ok(());
        }

        if grid_changed {
            self.state.grid.resize(rows, cols);
        }
        if let Some(pty) = &self.pty {
            pty.resize_with_pixels(rows as u16, cols as u16, pixel_w, pixel_h)?;
            self.last_pty_pixels = (pixel_w, pixel_h);
        }
        Ok(())
    }

    /// Immutable reference to the underlying ScreenGrid.
    pub fn grid(&self) -> &ScreenGrid {
        &self.state.grid
    }

    /// Mutable reference to the underlying ScreenGrid.
    pub fn grid_mut(&mut self) -> &mut ScreenGrid {
        &mut self.state.grid
    }

    /// Set cursor visual shape (Block / Beam / Underline).
    pub fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.state.grid.set_cursor_shape(shape);
    }

    /// Retrieve the URL at the given screen cell position (row, col).
    pub fn url_at(&self, row: usize, col: usize) -> Option<String> {
        self.state.grid.url_at(row, col)
    }

    /// Name of the process currently in the foreground of this terminal's pty,
    /// e.g. `nvim` while editing and the shell name otherwise.
    pub fn foreground_process_name(&self) -> Option<String> {
        self.pty.as_ref()?.foreground_process_name()
    }

    /// Command-line arguments of the process currently in the foreground of this terminal's pty.
    pub fn foreground_process_cmdline(&self) -> Option<String> {
        self.pty.as_ref()?.foreground_process_cmdline()
    }

    /// OS process group leader ID currently in the foreground of this terminal's pty.
    pub fn foreground_process_group_id(&self) -> Option<u32> {
        self.pty.as_ref()?.foreground_process_group_id()
    }

    /// Return the current working directory of this terminal (from OSC 7 or child OS process CWD).
    pub fn current_working_directory(&self) -> Option<String> {
        if let Some(ref cwd) = self.state.current_working_directory {
            if std::path::Path::new(cwd).exists() {
                return Some(cwd.clone());
            }
        }

        let candidates = [self.foreground_process_group_id(), self.process_id()];
        for pid in candidates.into_iter().flatten() {
            #[cfg(target_os = "macos")]
            {
                if let Some(path) = crate::pty::process_cwd(pid as libc::pid_t) {
                    if std::path::Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Ok(path) = std::fs::read_link(format!("/proc/{}/cwd", pid)) {
                    if path.exists() {
                        return Some(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
        None
    }

    /// Take any pending OSC 52 clipboard text decoded by the VT state machine.
    pub fn take_pending_clipboard(&mut self) -> Option<String> {
        self.state.pending_clipboard.take()
    }

    /// Take any pending VT device query responses (DA1, DA2, DSR, CPR, DECRQM, OSC 10/11/12, OSC 7501, Kitty `a=q`).
    pub fn take_pending_responses(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.state.pending_responses)
    }

    /// Return the highest-priority active OSC 7501 program status record in this terminal, if any.
    pub fn active_program_status(&self) -> Option<&crate::vt::ProgramStatusRecord> {
        self.state.active_program_status()
    }

    /// Take and clear the `program_status_dirty` flag indicating that an OSC 7501 status record changed.
    pub fn take_program_status_dirty(&mut self) -> bool {
        std::mem::replace(&mut self.state.program_status_dirty, false)
    }
}
