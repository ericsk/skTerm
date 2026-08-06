//! High-level Terminal engine integrating PTY, VTE Parser, and ScreenGrid.

use crate::grid::ScreenGrid;
use crate::pty::{PtyEvent, PtyProcess};
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
        let pty = PtyProcess::spawn(rows as u16, cols as u16, custom_shell, cwd)?;
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
            while let Ok(event) = pty.receiver().try_recv() {
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
            self.feed_bytes(&chunk);
        }
        true
    }

    /// Send keyboard input or encoded escape sequences to the PTY stdin.
    pub fn write_to_pty(&self, bytes: &[u8]) -> anyhow::Result<()> {
        if let Some(pty) = &self.pty {
            pty.write_bytes(bytes)?;
        }
        Ok(())
    }

    /// Resize terminal rows and cols across both ScreenGrid and the PTY master.
    pub fn resize(&mut self, rows: usize, cols: usize) -> anyhow::Result<()> {
        self.state.grid.resize(rows, cols);
        if let Some(pty) = &mut self.pty {
            pty.resize(rows as u16, cols as u16)?;
        }
        Ok(())
    }

    /// Direct access to ScreenGrid.
    pub fn grid(&self) -> &ScreenGrid {
        &self.state.grid
    }

    /// Mutable access to ScreenGrid.
    pub fn grid_mut(&mut self) -> &mut ScreenGrid {
        &mut self.state.grid
    }

    /// Check if the child PTY process is still alive.
    pub fn is_alive(&self) -> bool {
        self.pty.as_ref().map(|p| p.is_alive()).unwrap_or(true)
    }
}
