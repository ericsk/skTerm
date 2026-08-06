//! `skterm-core`: VT100/xterm State Machine, ScreenGrid, PTY Abstraction, Sixel, and Kitty Graphics.

pub mod graphics;
pub mod grid;
pub mod pty;
pub mod terminal;
pub mod vt;

pub use graphics::{GraphicImage, KittyGraphicsParser, SixelDecoder};
pub use grid::{Cell, CellFlags, Color, Cursor, CursorShape, NamedColor, Row, ScreenBuffer, ScreenGrid, Selection, SelectionMode};
pub use pty::{PtyEvent, PtyProcess};
pub use terminal::Terminal;
pub use vt::{MouseFormat, MouseProtocol, TerminalState};

/// skterm-core library version and basic info
pub fn core_info() -> &'static str {
    "skterm-core v0.1.0"
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    #[test]
    fn test_core_info() {
        assert_eq!(core_info(), "skterm-core v0.1.0");
    }

    #[test]
    fn test_basic_text_and_cursor() {
        let mut term = Terminal::new_headless(24, 80, 1000);
        term.feed_bytes(b"Hello, skTerm!");

        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 14);
        assert_eq!(buf.cursor.row, 0);

        let row_text: String = buf.grid[0].cells[0..14].iter().map(|c| c.c).collect();
        assert_eq!(row_text, "Hello, skTerm!");
    }

    #[test]
    fn test_sgr_colors_and_truecolor() {
        let mut term = Terminal::new_headless(24, 80, 1000);
        term.feed_bytes(b"\x1b[31;1mRed Bold\x1b[0m \x1b[38;2;255;128;0mOrange TrueColor\x1b[0m");

        let buf = term.grid().active();
        assert_eq!(buf.grid[0].cells[0].fg, Color::Named(NamedColor::Red));
        assert!(buf.grid[0].cells[0].flags.contains(CellFlags::BOLD));
        assert_eq!(buf.grid[0].cells[9].fg, Color::Rgb(255, 128, 0));
    }

    #[test]
    fn test_cjk_double_width_character() {
        let mut term = Terminal::new_headless(24, 80, 1000);
        term.feed_bytes("中文測試".as_bytes());

        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 8);
        assert_eq!(buf.grid[0].cells[0].c, '中');
        assert!(buf.grid[0].cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert!(buf.grid[0].cells[1].flags.contains(CellFlags::WIDE_CHAR_SPACER));
    }

    #[test]
    fn test_alternate_screen_buffer() {
        let mut term = Terminal::new_headless(24, 80, 1000);
        term.feed_bytes(b"Primary Screen Text");
        assert_eq!(term.grid().is_alternate, false);

        // Switch to Alt Screen (?1049h)
        term.feed_bytes(b"\x1b[?1049h");
        assert_eq!(term.grid().is_alternate, true);
        term.feed_bytes(b"Alt Screen Text");

        let alt_text: String = term.grid().active().grid[0].cells[0..15].iter().map(|c| c.c).collect();
        assert_eq!(alt_text, "Alt Screen Text");

        // Switch back to Primary (?1049l)
        term.feed_bytes(b"\x1b[?1049l");
        assert_eq!(term.grid().is_alternate, false);
        let prim_text: String = term.grid().active().grid[0].cells[0..19].iter().map(|c| c.c).collect();
        assert_eq!(prim_text, "Primary Screen Text");
    }

    #[test]
    fn test_scrollback_history() {
        let mut term = Terminal::new_headless(4, 20, 100);
        for i in 0..10 {
            term.feed_bytes(format!("Line {}\r\n", i).as_bytes());
        }

        let buf = term.grid().active();
        assert_eq!(buf.scrollback.len(), 7);
    }

    #[test]
    fn test_osc_8_hyperlink() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"\x1b]8;;https://github.com/ericsk/skTerm\x07skTerm Link\x1b]8;;\x07");

        let buf = term.grid().active();
        let link_text: String = buf.grid[0].cells[0..11].iter().map(|c| c.c).collect();
        assert_eq!(link_text, "skTerm Link");
        assert!(term.state.current_hyperlink.is_none());
    }

    #[test]
    fn test_text_selection() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"First Line\r\nSecond Line\r\nThird Line");

        let active = term.grid_mut().active_mut();
        active.selection = Some(Selection::new((0, 0), (1, 5), SelectionMode::Simple));

        let selected = active.extract_selected_text();
        assert_eq!(selected, "First Line\nSecond");
    }

    #[test]
    fn test_sixel_decoder() {
        let mut decoder = SixelDecoder::new();
        // Sixel data: #1;2;100;0;0 (Red) !10~ (repeat 10 full 6-pixel rows)
        let sixel_stream = b"#1;2;100;0;0!10~-!10~";
        let result = decoder.decode(sixel_stream);
        assert!(result.is_some());
        let (w, h, rgba) = result.unwrap();
        assert_eq!(w, 10);
        assert_eq!(h, 12);
        assert_eq!(rgba.len(), 10 * 12 * 4);
        assert_eq!(rgba[0], 255); // Red
        assert_eq!(rgba[1], 0);
        assert_eq!(rgba[2], 0);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn test_kitty_graphics_parser() {
        let mut parser = KittyGraphicsParser::new();
        // Raw RGBA 2x2 image (16 bytes base64 encoded)
        let raw_rgba = vec![
            255, 0, 0, 255,   0, 255, 0, 255,
            0, 0, 255, 255,   255, 255, 255, 255
        ];
        let b64 = base64::engine::general_purpose::STANDARD.encode(&raw_rgba);
        let cmd = format!("a=t,f=32,s=2,v=2,m=0;{}", b64);

        let result = parser.parse_command(&cmd);
        assert!(result.is_some());
        let (w, h, rgba, _) = result.unwrap();
        assert_eq!(w, 2);
        assert_eq!(h, 2);
        assert_eq!(rgba, raw_rgba);
    }
}
