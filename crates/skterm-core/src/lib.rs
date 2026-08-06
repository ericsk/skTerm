//! `skterm-core`: Core terminal state machine, PTY abstraction, and Screen Grid buffer.

pub mod grid;
pub mod pty;
pub mod terminal;
pub mod vt;

pub use grid::{Cell, CellFlags, Color, CursorShape, NamedColor, Row, ScreenBuffer, ScreenGrid};
pub use pty::{PtyEvent, PtyProcess};
pub use terminal::Terminal;
pub use vt::{MouseFormat, MouseProtocol, TerminalState};

/// skterm-core version and basic info
pub fn core_info() -> &'static str {
    "skterm-core v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_core_info() {
        assert_eq!(core_info(), "skterm-core v0.1.0");
    }

    #[test]
    fn test_basic_text_and_cursor() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"Hello, skTerm!");

        let row = &term.grid().active().grid[0];
        let text: String = row.cells.iter().take(14).map(|c| c.c).collect();
        assert_eq!(text, "Hello, skTerm!");
        assert_eq!(term.grid().active().cursor.col, 14);
        assert_eq!(term.grid().active().cursor.row, 0);
    }

    #[test]
    fn test_cjk_double_width_character() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes("你好，世界！".as_bytes());

        let row = &term.grid().active().grid[0];
        // 6 double-width characters = 12 columns
        assert_eq!(row.cells[0].c, '你');
        assert!(row.cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert!(row.cells[1].flags.contains(CellFlags::WIDE_CHAR_SPACER));

        assert_eq!(row.cells[2].c, '好');
        assert!(row.cells[2].flags.contains(CellFlags::WIDE_CHAR));
        assert!(row.cells[3].flags.contains(CellFlags::WIDE_CHAR_SPACER));

        assert_eq!(term.grid().active().cursor.col, 12);
    }

    #[test]
    fn test_sgr_colors_and_truecolor() {
        let mut term = Terminal::new_headless(24, 80, 100);
        // Set Red FG (31), Green BG (42), Bold (1)
        term.feed_bytes(b"\x1b[31;42;1mRed on Green\x1b[0m");

        let row = &term.grid().active().grid[0];
        assert_eq!(row.cells[0].c, 'R');
        assert_eq!(row.cells[0].fg, Color::Named(NamedColor::Red));
        assert_eq!(row.cells[0].bg, Color::Named(NamedColor::Green));
        assert!(row.cells[0].flags.contains(CellFlags::BOLD));

        // Test 256 Color: 38;5;123
        term.feed_bytes(b"\x1b[38;5;123mIndexed\x1b[0m");
        let cell = &term.grid().active().grid[0].cells[12];
        assert_eq!(cell.c, 'I');
        assert_eq!(cell.fg, Color::Indexed(123));

        // Test 24-bit TrueColor RGB: 38;2;255;128;64
        term.feed_bytes(b"\x1b[38;2;255;128;64mTrueColor\x1b[0m");
        let cell = &term.grid().active().grid[0].cells[19];
        assert_eq!(cell.c, 'T');
        assert_eq!(cell.fg, Color::Rgb(255, 128, 64));
    }

    #[test]
    fn test_alternate_screen_buffer() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"Primary Screen Text");
        assert!(!term.grid().is_alternate);

        // Switch to Alt Screen: \x1b[?1049h
        term.feed_bytes(b"\x1b[?1049hAlt Screen Text");
        assert!(term.grid().is_alternate);
        let alt_row = &term.grid().active().grid[0];
        let alt_text: String = alt_row.cells.iter().take(15).map(|c| c.c).collect();
        assert_eq!(alt_text, "Alt Screen Text");

        // Leave Alt Screen: \x1b[?1049l
        term.feed_bytes(b"\x1b[?1049l");
        assert!(!term.grid().is_alternate);
        let pri_row = &term.grid().active().grid[0];
        let pri_text: String = pri_row.cells.iter().take(19).map(|c| c.c).collect();
        assert_eq!(pri_text, "Primary Screen Text");
    }

    #[test]
    fn test_scrollback_history() {
        let mut term = Terminal::new_headless(3, 10, 10);
        term.feed_bytes(b"Line 1\r\nLine 2\r\nLine 3\r\nLine 4\r\nLine 5");

        let primary = &term.grid().primary;
        assert_eq!(primary.scrollback.len(), 2); // Line 1 and Line 2 pushed to scrollback
        let sb_line1: String = primary.scrollback[0].cells.iter().take(6).map(|c| c.c).collect();
        assert_eq!(sb_line1, "Line 1");
    }

    #[test]
    fn test_osc_8_hyperlink() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"\x1b]8;;https://example.com\x1b\\Example Link\x1b]8;;\x1b\\ Normal");

        let row = &term.grid().active().grid[0];
        assert_eq!(
            row.cells[0].hyperlink.as_deref().map(|s| s.as_str()),
            Some("https://example.com")
        );
        assert_eq!(row.cells[12].hyperlink, None);
    }
}
