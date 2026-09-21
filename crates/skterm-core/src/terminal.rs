//! High-level Terminal engine integrating PTY, VTE Parser, and ScreenGrid.

use crate::grid::{CursorShape, ScreenGrid};
use crate::pty::{PtyEvent, PtyProcess, PtySpawnOptions};
use crate::vt::TerminalState;
use vte::Parser;

/// The complete terminal instance combining terminal state, parser, and child PTY process.
pub struct Terminal {
    pub state: TerminalState,
    parser: Parser,
    pty: Option<PtyProcess>,
    pub last_output_time: std::time::Instant,
    pub custom_label: Option<String>,
    /// Reused across `poll_pty` calls so steady-state reading allocates nothing.
    read_buf: Vec<u8>,
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
            exit_code: None,
            has_exited: false,
        }
    }

    /// Feed byte stream (from PTY or test) into the VTE parser to update state.
    pub fn feed_bytes(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
        if self.pty.is_some() {
            let responses = self.state.take_pending_responses();
            for resp in responses {
                let _ = self.write_to_pty(&resp);
            }
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
            // 1. Initial non-blocking drain of all currently available PTY chunks
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

            // 2. Burst coalescence: If output was received, gather trailing chunks
            // from the same burst (e.g. PSReadLine multi-chunk ANSI escape sequences
            // for cursor hiding, syntax highlighting, and cursor repositioning that arrive
            // within 1-2ms of each other).
            // A short coalescing window (up to ~3ms) merges the entire burst into a single buffer,
            // preventing slicing a single keystroke's echo into 5-7 separate frames and VSync waits!
            if !buf.is_empty() && !self.has_exited {
                let burst_start = std::time::Instant::now();
                const MAX_BURST_DURATION: std::time::Duration = std::time::Duration::from_millis(3);
                const QUIET_WINDOW: std::time::Duration = std::time::Duration::from_micros(600);
                let mut last_chunk_time = burst_start;

                while burst_start.elapsed() < MAX_BURST_DURATION && buf.len() < MAX_BYTES_PER_POLL {
                    match pty.try_recv() {
                        Some(PtyEvent::Output(bytes)) => {
                            buf.extend_from_slice(&bytes);
                            last_chunk_time = std::time::Instant::now();
                        }
                        Some(PtyEvent::Exit(code)) => {
                            self.exit_code = Some(code);
                            self.has_exited = true;
                            break;
                        }
                        None => {
                            if last_chunk_time.elapsed() >= QUIET_WINDOW {
                                break;
                            }
                            std::thread::yield_now();
                        }
                    }
                }
            }
        }

        let had_data = !buf.is_empty();
        if had_data {
            self.last_output_time = std::time::Instant::now();
            self.parser.advance(&mut self.state, &buf);

            // Send any VT query responses (DA1, DA2, DSR, CPR, window size) back to the PTY
            let responses = self.state.take_pending_responses();
            for resp in responses {
                let _ = self.write_to_pty(&resp);
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
        let active = self.state.grid.active();
        if active.rows == rows && active.cols == cols {
            return Ok(());
        }

        self.state.grid.resize(rows, cols);
        if let Some(pty) = &self.pty {
            pty.resize(rows as u16, cols as u16)?;
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

    /// Return the current working directory of this terminal (from OSC 7 or child OS process CWD).
    pub fn current_working_directory(&self) -> Option<String> {
        if let Some(ref cwd) = self.state.current_working_directory {
            if std::path::Path::new(cwd).exists() {
                return Some(cwd.clone());
            }
        }

        if let Some(_pid) = self.process_id() {
            #[cfg(target_os = "macos")]
            {
                if let Some(path) = crate::pty::process_cwd(_pid as libc::pid_t) {
                    if std::path::Path::new(&path).exists() {
                        return Some(path);
                    }
                }
            }
            #[cfg(target_os = "linux")]
            {
                if let Ok(path) = std::fs::read_link(format!("/proc/{}/cwd", _pid)) {
                    if path.exists() {
                        return Some(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
        None
    }
}
