//! `skterm-core`: VT100/xterm State Machine, ScreenGrid, PTY Abstraction, Sixel, and Kitty Graphics.

pub mod graphics;
pub mod grid;
pub mod pty;
pub mod terminal;
pub mod vt;

pub use graphics::{GraphicImage, KittyGraphicsParser, SixelDecoder};
pub use grid::{
    Cell, CellFlags, Color, Cursor, CursorShape, NamedColor, Row, ScreenBuffer, ScreenGrid,
    SearchMatch, Selection, SelectionMode,
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
    fn test_wrapped_line_selection_and_copy() {
        // 10 columns wide: a 25-character line wraps across 3 rows (row 0, row 1, row 2).
        let mut term = Terminal::new_headless(10, 10, 100);
        term.feed_bytes(b"hello world wrapped line!");

        let active = term.grid_mut().active_mut();
        assert!(active.grid[0].is_wrapped);
        assert!(active.grid[1].is_wrapped);
        assert!(!active.grid[2].is_wrapped);

        // 1. Selecting across wrapped rows joins them without inserting artificial '\n'
        // and preserves intentional spaces at the end of soft-wrapped rows.
        active.selection = Some(Selection::new((0, 0), (2, 4), SelectionMode::Simple));
        assert_eq!(
            active.extract_selected_text(),
            "hello world wrapped line!"
        );

        // 2. Triple-click (select_line_at) on any part of the wrapped line selects all wrapped rows,
        // and simulated winit mouseUp CursorMoved (`extend_selection_to` at click cell `(0, 0)`)
        // must preserve the full multi-row Line selection instead of collapsing `end` to `(0, 0)`.
        active.select_line_at(1);
        active.extend_selection_to((1, 0), (1, 0));
        assert_eq!(
            active.extract_selected_text(),
            "hello world wrapped line!"
        );

        // 3. Double-click (select_word_at) on a word that wraps across row 0 and row 1 ("world" at cols 6..10 + col 0)
        active.select_word_at(0, 7);
        active.extend_selection_to((0, 7), (0, 7));
        assert_eq!(active.extract_selected_text(), "world");
    }

    #[test]
    fn test_wrapped_cjk_selection_skips_wrap_spacer() {
        // 10 columns wide: 9 ASCII chars + 2 CJK wide chars ("中文") wraps before '中'
        // leaving a synthetic WRAP_SPACER at row 0 col 9.
        let mut term = Terminal::new_headless(10, 10, 100);
        term.feed_bytes("123456789中文".as_bytes());

        let active = term.grid_mut().active_mut();
        assert!(active.grid[0].cells[9].flags.contains(CellFlags::WRAP_SPACER));

        active.selection = Some(Selection::new((0, 0), (1, 3), SelectionMode::Simple));
        assert_eq!(
            active.extract_selected_text(),
            "123456789中文",
            "WRAP_SPACER should not introduce an extra space when copying wrapped CJK text"
        );
    }

    #[test]
    fn test_osc52_clipboard_sequence() {
        let mut term = Terminal::new_headless(24, 80, 100);
        // "Copied from Vim" in base64 is "Q29waWVkIGZyb20gVmlt"
        term.feed_bytes(b"\x1b]52;c;Q29waWVkIGZyb20gVmlt\x07");
        assert_eq!(
            term.take_pending_clipboard().as_deref(),
            Some("Copied from Vim")
        );
        assert_eq!(term.take_pending_clipboard(), None);
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

    #[test]
    fn test_osc7_working_directory() {
        let mut term = Terminal::new_headless(24, 80, 100);
        term.feed_bytes(b"\x1b]7;file://macbook.local/Users/ericsk/Projects/skTerm\x07");
        assert_eq!(
            term.state.current_working_directory.as_deref(),
            Some("/Users/ericsk/Projects/skTerm")
        );
    }

    #[test]
    fn test_clear_scrollback_and_screen() {
        let mut term = Terminal::new_headless(5, 80, 100);
        for i in 0..10 {
            term.feed_bytes(format!("Line {}\r\n", i).as_bytes());
        }
        assert!(!term.grid().active().scrollback.is_empty());

        term.grid_mut().active_mut().clear_scrollback_and_screen();
        assert!(term.grid().active().scrollback.is_empty());
        assert_eq!(term.grid().active().cursor.row, 0);
        assert_eq!(term.grid().active().cursor.col, 0);
    }

    #[test]
    fn test_word_and_line_selection() {
        let mut term = Terminal::new_headless(5, 80, 100);
        term.feed_bytes(b"hello /usr/local/bin/skterm world\r\n");

        // Double click inside path `/usr/local/bin/skterm` (e.g., col 10)
        term.grid_mut().active_mut().select_word_at(0, 10);
        let selected = term.grid().active().extract_selected_text();
        assert_eq!(selected, "/usr/local/bin/skterm");

        // Triple click selects entire line
        term.grid_mut().active_mut().select_line_at(0);
        let line_selected = term.grid().active().extract_selected_text();
        assert!(line_selected.starts_with("hello /usr/local/bin/skterm world"));
    }

    #[test]
    fn test_scrollback_and_screen_search() {
        let mut term = Terminal::new_headless(5, 80, 100);
        for i in 0..8 {
            term.feed_bytes(format!("Log entry #{} with TargetKeyword\r\n", i).as_bytes());
        }
        let matches = term.grid().active().search("targetkeyword");
        assert_eq!(matches.len(), 8);
        assert_eq!(matches[0].start_col, 18);
    }
    /// Scrolling recycles row buffers instead of allocating. If a recycled row is
    /// not fully blanked, old text reappears at the bottom of the screen -- these
    /// cover every recycling source.
    mod row_recycling {
        use super::*;

        fn row_text(term: &Terminal, row: usize) -> String {
            term.grid().active().grid[row]
                .cells
                .iter()
                .map(|c| c.c)
                .collect::<String>()
                .trim_end()
                .to_string()
        }

        #[test]
        fn scroll_up_recycles_evicted_scrollback_row() {
            // max_scrollback = 2 so the ring starts evicting almost immediately and
            // the evicted buffers get reused for the new bottom rows.
            let mut term = Terminal::new_headless(3, 20, 2);
            for i in 0..10 {
                term.feed_bytes(format!("line{}\r\n", i).as_bytes());
            }
            assert_eq!(term.grid().active().scrollback.len(), 2);
            assert_eq!(row_text(&term, 0), "line8");
            assert_eq!(row_text(&term, 1), "line9");
            assert_eq!(row_text(&term, 2), "", "recycled row still holds old text");
        }

        #[test]
        fn scroll_up_recycles_own_row_when_scrollback_disabled() {
            let mut term = Terminal::new_headless(3, 20, 0);
            for i in 0..10 {
                term.feed_bytes(format!("line{}\r\n", i).as_bytes());
            }
            assert!(term.grid().active().scrollback.is_empty());
            assert_eq!(row_text(&term, 0), "line8");
            assert_eq!(row_text(&term, 1), "line9");
            assert_eq!(row_text(&term, 2), "");
        }

        #[test]
        fn scroll_region_recycles_row() {
            let mut term = Terminal::new_headless(5, 20, 100);
            term.feed_bytes(b"a\r\nb\r\nc\r\nd\r\ne");
            // Restrict the region to rows 1-3, park the cursor at its bottom, then
            // force a scroll inside it with a linefeed.
            term.feed_bytes(b"\x1b[2;4r\x1b[4;1H\n");
            assert_eq!(row_text(&term, 0), "a");
            assert_eq!(row_text(&term, 1), "c");
            assert_eq!(row_text(&term, 2), "d");
            assert_eq!(row_text(&term, 3), "");
            assert_eq!(row_text(&term, 4), "e");
        }

        #[test]
        fn insert_and_delete_lines_recycle_rows() {
            let mut term = Terminal::new_headless(4, 20, 100);
            term.feed_bytes(b"one\r\ntwo\r\nthree\r\nfour");

            // IL at row 1 pushes "four" off the bottom and blanks row 1.
            term.feed_bytes(b"\x1b[2;1H\x1b[L");
            assert_eq!(row_text(&term, 0), "one");
            assert_eq!(row_text(&term, 1), "");
            assert_eq!(row_text(&term, 2), "two");
            assert_eq!(row_text(&term, 3), "three");

            // DL at row 1 pulls the rest up and blanks the bottom row.
            term.feed_bytes(b"\x1b[2;1H\x1b[M");
            assert_eq!(row_text(&term, 0), "one");
            assert_eq!(row_text(&term, 1), "two");
            assert_eq!(row_text(&term, 2), "three");
            assert_eq!(row_text(&term, 3), "");
        }

        #[test]
        fn delete_and_insert_chars_shift_correctly() {
            let mut term = Terminal::new_headless(2, 10, 100);

            // DCH: delete 3 chars at col 2 of "abcdefghij".
            term.feed_bytes(b"abcdefghij");
            term.feed_bytes(b"\x1b[1;3H\x1b[3P");
            assert_eq!(row_text(&term, 0), "abfghij");

            // ICH: insert 2 blanks at col 2, pushing the tail right and off the edge.
            term.feed_bytes(b"\x1b[1;1H\x1b[2K\x1b[1;1Habcdefghij");
            term.feed_bytes(b"\x1b[1;3H\x1b[2@");
            let cells = &term.grid().active().grid[0].cells;
            assert_eq!(cells[0].c, 'a');
            assert_eq!(cells[1].c, 'b');
            assert_eq!(cells[2].c, ' ');
            assert_eq!(cells[3].c, ' ');
            assert_eq!(cells[4].c, 'c');
            assert_eq!(cells[9].c, 'h');
        }

        #[test]
        fn erase_chars_uses_current_background() {
            let mut term = Terminal::new_headless(2, 10, 100);
            term.feed_bytes(b"abcdefghij");
            // Red background, then ECH 3 at col 1.
            term.feed_bytes(b"\x1b[41m\x1b[1;1H\x1b[3X");
            let cells = &term.grid().active().grid[0].cells;
            for cell in cells.iter().take(3) {
                assert_eq!(cell.c, ' ');
                assert_eq!(cell.bg, Color::Named(NamedColor::Red));
            }
            assert_eq!(cells[3].c, 'd');
        }
    }

    /// Covers the lookup behind the macOS native tab label.
    #[cfg(unix)]
    mod foreground_process {
        use super::*;
        use std::time::{Duration, Instant};

        fn spawn_sh() -> PtyProcess {
            PtyProcess::spawn_with_options(
                24,
                80,
                PtySpawnOptions {
                    shell: Some("/bin/sh".to_string()),
                    ..Default::default()
                },
            )
            .expect("spawn pty")
        }

        /// Poll until the reported name is `want`, or give up after 5s.
        fn wait_for_name(pty: &PtyProcess, want: &str) -> Option<String> {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut last = None;
            while Instant::now() < deadline {
                last = pty.foreground_process_name();
                if last.as_deref() == Some(want) {
                    return last;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            last
        }

        /// Poll until a name is reported at all, or give up after 5s.
        fn wait_for_any_name(pty: &PtyProcess) -> Option<String> {
            let deadline = Instant::now() + Duration::from_secs(5);
            while Instant::now() < deadline {
                if let Some(name) = pty.foreground_process_name() {
                    return Some(name);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        }

        #[test]
        fn reports_the_program_running_in_the_foreground() {
            let pty = spawn_sh();

            // Deliberately not asserting the exact shell name: macOS `/bin/sh -l`
            // re-execs itself as bash a few ms in, so this alternates between
            // "sh" and "bash" depending on when we sample.
            let shell = wait_for_any_name(&pty);
            assert!(
                shell.is_some(),
                "expected some foreground process name for a live shell"
            );

            // `exec` replaces the shell image in place, so the pid owning the
            // foreground process group stays the same and only its name changes.
            // That keeps this independent of whether job control is enabled.
            pty.write_all(b"exec sleep 30\n").expect("write to pty");

            let running = wait_for_name(&pty, "sleep");
            assert_eq!(
                running.as_deref(),
                Some("sleep"),
                "foreground name did not follow the exec'd program, got {:?}",
                running
            );
        }

        /// portable-pty reports the child's pid as soon as it forks, before the
        /// child has exec'd the shell. Until then the child is a copy of us and
        /// carries *our* name, which used to surface as a tab labelled with the
        /// application's own name for the first ~12ms of every new tab.
        #[test]
        fn never_reports_the_host_process_during_spawn() {
            // Poll from the instant of spawn, with no sleep, to land inside the
            // pre-exec window rather than skipping over it.
            let pty = spawn_sh();

            let deadline = Instant::now() + Duration::from_secs(2);
            let mut observed = Vec::new();
            let mut saw_shell = false;
            while Instant::now() < deadline {
                if let Some(name) = pty.foreground_process_name() {
                    if !observed.contains(&name) {
                        observed.push(name.clone());
                    }
                    // The test binary is named `skterm_core-<hash>`, so leaking
                    // our own identity shows up as this substring.
                    assert!(
                        !name.to_lowercase().contains("skterm"),
                        "reported the host process as the foreground program: {:?} \
                         (all observed: {:?})",
                        name,
                        observed
                    );
                    saw_shell = true;
                }
            }

            assert!(
                saw_shell,
                "never observed any foreground process, so the assertion above \
                 never actually ran"
            );
        }

        /// The cwd lookup replaced an `lsof` subprocess, so it is worth pinning
        /// to a known answer rather than just "returns something".
        #[test]
        #[cfg(target_os = "macos")]
        fn process_cwd_matches_our_own_working_directory() {
            let expected = std::env::current_dir().expect("current_dir");
            let got = crate::pty::process_cwd(unsafe { libc::getpid() })
                .expect("process_cwd returned None for our own pid");

            // Canonicalise both: on macOS /tmp and /private/tmp style symlinks
            // make a raw string comparison unreliable.
            assert_eq!(
                std::fs::canonicalize(&got).expect("canonicalize result"),
                std::fs::canonicalize(&expected).expect("canonicalize expected"),
            );
        }
    }
}
