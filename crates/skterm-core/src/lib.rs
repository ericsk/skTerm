//! `skterm-core`: VT100/xterm State Machine, ScreenGrid, PTY Abstraction, Sixel, and Kitty Graphics.

pub mod graphics;
pub mod grid;
pub mod pty;
pub mod terminal;
pub mod vt;

pub use graphics::{GraphicImage, KittyGraphicsParser, SixelDecoder};
pub use grid::{
    Cell, CellFlags, Color, Cursor, CursorShape, NamedColor, Row, ScreenBuffer, ScreenGrid,
    Selection, SelectionMode,
};
pub use pty::{PtyEvent, PtyProcess, PtySpawnOptions};
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
        assert!(buf.grid[0].cells[1]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER));
    }

    #[test]
    fn test_alternate_screen_buffer() {
        let mut term = Terminal::new_headless(24, 80, 1000);
        term.feed_bytes(b"Primary Screen Text");
        assert!(!term.grid().is_alternate);

        // Switch to Alt Screen (?1049h)
        term.feed_bytes(b"\x1b[?1049h");
        assert!(term.grid().is_alternate);
        term.feed_bytes(b"Alt Screen Text");

        let alt_text: String = term.grid().active().grid[0].cells[0..15]
            .iter()
            .map(|c| c.c)
            .collect();
        assert_eq!(alt_text, "Alt Screen Text");

        // Switch back to Primary (?1049l)
        term.feed_bytes(b"\x1b[?1049l");
        assert!(!term.grid().is_alternate);
        let prim_text: String = term.grid().active().grid[0].cells[0..19]
            .iter()
            .map(|c| c.c)
            .collect();
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

        // Verify url_at returns the OSC 8 hyperlink on the linked cells
        assert_eq!(
            term.url_at(0, 0),
            Some("https://github.com/ericsk/skTerm".to_string())
        );
        assert_eq!(
            term.url_at(0, 5),
            Some("https://github.com/ericsk/skTerm".to_string())
        );
        assert_eq!(term.url_at(0, 15), None);
    }

    #[test]
    fn test_plain_text_url_detection() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"Visit https://github.com/ericsk/skTerm for details. Also check www.rust-lang.org!\r\n");

        // Hover over https://github.com/ericsk/skTerm
        assert_eq!(
            term.url_at(0, 6),
            Some("https://github.com/ericsk/skTerm".to_string())
        );
        assert_eq!(
            term.url_at(0, 10),
            Some("https://github.com/ericsk/skTerm".to_string())
        );
        assert_eq!(
            term.url_at(0, 30),
            Some("https://github.com/ericsk/skTerm".to_string())
        );

        // Hover over "Visit " -> None
        assert_eq!(term.url_at(0, 2), None);

        // Hover over www.rust-lang.org
        assert_eq!(
            term.url_at(0, 65),
            Some("https://www.rust-lang.org".to_string())
        );
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
            255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 255, 255, 255, 255,
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

    #[test]
    fn test_buffer_reflow_on_resize() {
        let mut term = Terminal::new_headless(24, 20, 1000);
        term.feed_bytes(b"0123456789abcdefghijklmnopqrst");

        // At 20 cols:
        // Row 0: "0123456789abcdefghij" (wrapped: true)
        // Row 1: "klmnopqrst" (wrapped: false)
        {
            let buf = term.grid().active();
            let row0: String = buf.grid[0].cells[0..20].iter().map(|c| c.c).collect();
            let row1: String = buf.grid[1].cells[0..10].iter().map(|c| c.c).collect();
            assert_eq!(row0, "0123456789abcdefghij");
            assert_eq!(row1, "klmnopqrst");
            assert!(buf.grid[0].is_wrapped);
            assert!(!buf.grid[1].is_wrapped);
        }

        // Resize down to 10 cols (simulating vertical pane split):
        term.resize(24, 10).unwrap();
        {
            let buf = term.grid().active();
            let r0: String = buf.grid[0].cells[0..10].iter().map(|c| c.c).collect();
            let r1: String = buf.grid[1].cells[0..10].iter().map(|c| c.c).collect();
            let r2: String = buf.grid[2].cells[0..10].iter().map(|c| c.c).collect();
            assert_eq!(r0, "0123456789");
            assert_eq!(r1, "abcdefghij");
            assert_eq!(r2, "klmnopqrst");
            assert!(buf.grid[0].is_wrapped);
            assert!(buf.grid[1].is_wrapped);
            assert!(!buf.grid[2].is_wrapped);
            assert_eq!(buf.cursor.row, 2);
            assert_eq!(buf.cursor.col, 9);
        }

        // Resize back up to 40 cols (unwrapping):
        term.resize(24, 40).unwrap();
        {
            let buf = term.grid().active();
            let r0: String = buf.grid[0].cells[0..30].iter().map(|c| c.c).collect();
            assert_eq!(r0, "0123456789abcdefghijklmnopqrst");
            assert!(!buf.grid[0].is_wrapped);
            assert_eq!(buf.cursor.row, 0);
            assert_eq!(buf.cursor.col, 29);
        }
    }

    #[test]
    fn test_buffer_reflow_cjk_double_width() {
        let mut term = Terminal::new_headless(24, 10, 1000);
        // 9 single-width chars + 2 double-width chars (4 cols)
        term.feed_bytes("123456789中文".as_bytes());

        // At 10 cols:
        // "123456789" occupies cols 0..8.
        // Col 9 cannot fit '中' (width 2), so col 9 has WRAP_SPACER, wrapped to row 1.
        // Row 1 has "中文" at cols 0..3.
        {
            let buf = term.grid().active();
            let r0: String = buf.grid[0].cells[0..9].iter().map(|c| c.c).collect();
            assert_eq!(r0, "123456789");
            assert!(buf.grid[0].is_wrapped);
            assert_eq!(buf.grid[1].cells[0].c, '中');
            assert_eq!(buf.grid[1].cells[2].c, '文');
        }

        // Reflow to 20 cols:
        // Everything should unwrap cleanly on row 0 without broken characters or extra spaces.
        term.resize(24, 20).unwrap();
        {
            let buf = term.grid().active();
            let r0_prefix: String = buf.grid[0].cells[0..9].iter().map(|c| c.c).collect();
            assert_eq!(r0_prefix, "123456789");
            assert_eq!(buf.grid[0].cells[9].c, '中');
            assert!(buf.grid[0].cells[9].flags.contains(CellFlags::WIDE_CHAR));
            assert!(buf.grid[0].cells[10]
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER));
            assert_eq!(buf.grid[0].cells[11].c, '文');
            assert!(buf.grid[0].cells[11].flags.contains(CellFlags::WIDE_CHAR));
            assert!(buf.grid[0].cells[12]
                .flags
                .contains(CellFlags::WIDE_CHAR_SPACER));
            assert!(!buf.grid[0].is_wrapped);
        }
    }

    #[test]
    fn test_pane_split_reflow_does_not_pull_scrollback() {
        let mut term = Terminal::new_headless(10, 80, 1000);
        // Generate 20 lines so scrollback is populated
        for i in 0..20 {
            term.feed_bytes(format!("History Line {}\r\n", i).as_bytes());
        }
        term.feed_bytes(b"$ my-command");

        let initial_scrollback_len = term.grid().active().scrollback.len();
        assert!(initial_scrollback_len > 0);

        let initial_cursor_row = term.grid().active().cursor.row;

        // Narrow columns to simulate pane split (80 -> 40)
        term.resize(10, 40).unwrap();

        let buf = term.grid().active();
        // Cursor and visible command should remain intact without scrollback invading
        assert_eq!(buf.cursor.row, initial_cursor_row);
        let current_line_text: String = buf.grid[initial_cursor_row].cells[0..12]
            .iter()
            .map(|c| c.c)
            .collect();
        assert_eq!(current_line_text, "$ my-command");
    }
}
