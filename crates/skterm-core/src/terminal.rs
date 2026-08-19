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
        })
    }

    /// Create a headless Terminal without an attached PTY process (ideal for unit tests).
    pub fn new_headless(rows: usize, cols: usize, max_scrollback: usize) -> Self {
        Self {
            state: TerminalState::new(rows, cols, max_scrollback),
            parser: Parser::new(),
            pty: None,
        }
    }

    /// Feed byte stream (from PTY or test) into the VTE parser to update state.
    pub fn feed_bytes(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.state, bytes);
    }

    /// Drain all available output from the PTY process and update terminal state.
    /// Returns `true` if any data was processed.
    pub fn poll_pty(&mut self) -> bool {
        let mut chunks = Vec::new();
        if let Some(pty) = &self.pty {
            while let Some(event) = pty.try_recv() {
                match event {
                    PtyEvent::Output(bytes) => chunks.push(bytes),
                    PtyEvent::Exit(_) => {}
                }
            }
        }

        if chunks.is_empty() {
            return false;
        }

        for chunk in chunks {
            self.parser.advance(&mut self.state, &chunk);
        }
        true
    }

    /// Check if the attached PTY process has exited.
    pub fn is_child_alive(&self) -> bool {
        self.pty.as_ref().map(|p| p.is_alive()).unwrap_or(false)
    }

    /// Alias to is_child_alive.
    pub fn is_alive(&self) -> bool {
        self.is_child_alive()
    }

    /// Write user input directly to the PTY process.
    pub fn write_to_pty(&self, data: &[u8]) -> anyhow::Result<()> {
        if let Some(pty) = &self.pty {
            pty.write_all(data)?;
        }
        Ok(())
    }

    /// Notify PTY and Grid of window resize.
    pub fn resize(&mut self, rows: usize, cols: usize) -> anyhow::Result<()> {
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
}
