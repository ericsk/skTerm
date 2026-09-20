//! Comprehensive ANSI / VT100 / xterm Compatibility Test Suite.

use skterm_core::{Color, CursorShape, NamedColor, Terminal};

#[test]
fn test_vt_cursor_positioning_and_bounds() {
    let mut term = Terminal::new_headless(24, 80, 100);

    // CUP: Cursor Position \x1b[10;20H (1-indexed => 0-indexed row 9, col 19)
    term.feed_bytes(b"\x1b[10;20H");
    assert_eq!(term.grid().active().cursor.row, 9);
    assert_eq!(term.grid().active().cursor.col, 19);

    // HVP: Horizontal and Vertical Position \x1b[5;15f
    term.feed_bytes(b"\x1b[5;15f");
    assert_eq!(term.grid().active().cursor.row, 4);
    assert_eq!(term.grid().active().cursor.col, 14);

    // VPA: Vertical Position Absolute \x1b[12d
    term.feed_bytes(b"\x1b[12d");
    assert_eq!(term.grid().active().cursor.row, 11);

    // CHA: Cursor Horizontal Absolute \x1b[30G
    term.feed_bytes(b"\x1b[30G");
    assert_eq!(term.grid().active().cursor.col, 29);

    // Clamp beyond grid bounds
    term.feed_bytes(b"\x1b[999;999H");
    assert_eq!(term.grid().active().cursor.row, 23);
    assert_eq!(term.grid().active().cursor.col, 79);
}

#[test]
fn test_vt_relative_cursor_movements() {
    let mut term = Terminal::new_headless(24, 80, 100);
    term.feed_bytes(b"\x1b[10;10H");

    // CUU: Cursor Up \x1b[3A
    term.feed_bytes(b"\x1b[3A");
    assert_eq!(term.grid().active().cursor.row, 6);

    // CUD: Cursor Down \x1b[4B
    term.feed_bytes(b"\x1b[4B");
    assert_eq!(term.grid().active().cursor.row, 10);

    // CUF: Cursor Forward \x1b[5C
    term.feed_bytes(b"\x1b[5C");
    assert_eq!(term.grid().active().cursor.col, 14);

    // CUB: Cursor Backward \x1b[6D
    term.feed_bytes(b"\x1b[6D");
    assert_eq!(term.grid().active().cursor.col, 8);
}

#[test]
fn test_vt_erase_operations() {
    let mut term = Terminal::new_headless(10, 20, 100);
    // Fill screen with 'X'
    for _ in 0..10 {
        term.feed_bytes(b"XXXXXXXXXXXXXXXXXXXX");
    }

    // EL 0: Erase from cursor to end of line \x1b[K
    term.feed_bytes(b"\x1b[3;10H\x1b[0K");
    let row2: String = term.grid().active().grid[2]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(&row2[0..9], "XXXXXXXXX");
    assert_eq!(&row2[9..20], "           ");

    // ED 2: Erase Entire Display \x1b[2J
    term.feed_bytes(b"\x1b[2J");
    for row in &term.grid().active().grid {
        let text: String = row.cells.iter().map(|c| c.c).collect();
        assert_eq!(text, "                    ");
    }
}

#[test]
fn test_vt_scroll_region_decstbm() {
    let mut term = Terminal::new_headless(10, 20, 100);

    // Set scroll region from row 3 to 6: \x1b[3;6r (0-indexed 2..=5)
    term.feed_bytes(b"\x1b[3;6r");
    assert_eq!(term.grid().active().scroll_region, (2, 5));

    // Place cursor at bottom of region and send linefeeds
    term.feed_bytes(b"\x1b[6;1HRow 6 Text\r\nNew Row");
    assert_eq!(term.grid().active().cursor.row, 5); // Kept inside region
}

#[test]
fn test_vt_reverse_index_ri() {
    let mut term = Terminal::new_headless(10, 20, 100);
    term.feed_bytes(b"\x1b[5;1HLine 5");

    // Reverse Index: \x1bM (moves cursor up)
    term.feed_bytes(b"\x1bM");
    assert_eq!(term.grid().active().cursor.row, 3);
}

#[test]
fn test_vt_dec_private_modes() {
    let mut term = Terminal::new_headless(10, 20, 100);

    // DECTCEM: Hide / Show Cursor \x1b[?25l / \x1b[?25h
    term.feed_bytes(b"\x1b[?25l");
    assert!(!term.grid().active().cursor.visible);
    term.feed_bytes(b"\x1b[?25h");
    assert!(term.grid().active().cursor.visible);

    // Bracketed Paste: \x1b[?2004h / \x1b[?2004l
    term.feed_bytes(b"\x1b[?2004h");
    assert!(term.state.bracketed_paste);
    term.feed_bytes(b"\x1b[?2004l");
    assert!(!term.state.bracketed_paste);

    // DECSCUSR: Cursor Shapes (\x1b[<n> q)
    term.feed_bytes(b"\x1b[1 q"); // Block
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Block);
    term.feed_bytes(b"\x1b[3 q"); // Underline
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Underline);
    term.feed_bytes(b"\x1b[5 q"); // Beam
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Beam);
}

#[test]
fn test_vt_sgr_full_palette() {
    let mut term = Terminal::new_headless(10, 40, 100);

    // 16 Standard & Bright colors
    term.feed_bytes(b"\x1b[32mGreen\x1b[91mBrightRed\x1b[0m");
    let buf = term.grid().active();
    assert_eq!(buf.grid[0].cells[0].fg, Color::Named(NamedColor::Green));
    assert_eq!(buf.grid[0].cells[5].fg, Color::Named(NamedColor::BrightRed));

    // 256 Indexed Color: \x1b[38;5;208m (Orange)
    term.feed_bytes(b"\x1b[38;5;208mIndexedOrange\x1b[0m");
    let cell_orange = term.grid().active().grid[0].cells[14];
    assert_eq!(cell_orange.fg, Color::Indexed(208));

    // 24-bit TrueColor: \x1b[38;2;123;45;67m
    term.feed_bytes(b"\x1b[38;2;123;45;67mRGBColor\x1b[0m");
    let cell_rgb = term.grid().active().grid[0].cells[27];
    assert_eq!(cell_rgb.fg, Color::Rgb(123, 45, 67));
}

#[test]
fn test_vt_zsh_prompt_sp_clear() {
    // Simulate zsh PROMPT_SP behavior after clear command
    // Terminal width is 20
    let mut term = Terminal::new_headless(10, 20, 100);

    // Fill entire line 0 with 20 characters: 1 '%' + 19 spaces
    term.feed_bytes(b"%                   ");
    assert_eq!(term.grid().active().cursor.col, 19);
    assert!(term.grid().active().cursor.pending_wrap);

    // Carriage Return (\r) should move col back to 0 and RESET pending_wrap to false
    term.feed_bytes(b"\r");
    assert_eq!(term.grid().active().cursor.col, 0);
    assert_eq!(term.grid().active().cursor.row, 0);
    assert!(!term.grid().active().cursor.pending_wrap);

    // Erase current line (\x1b[K) and print prompt
    term.feed_bytes(b"\x1b[Kprompt$ ");
    assert_eq!(term.grid().active().cursor.row, 0);
    let cells = &term.grid().active().grid[0].cells;
    let s: String = cells[0..8].iter().map(|c| c.c).collect();
    assert_eq!(s, "prompt$ ");
    let s_full: String = cells.iter().map(|c| c.c).collect();
    assert!(!s_full.contains('%'));
}

#[test]
fn test_vt_prompt_line_preservation_on_resize() {
    let mut term = Terminal::new_headless(10, 80, 100);

    // Line 0: Multiline prompt ruler (e.g. 50 characters) ending with \r\n
    term.feed_bytes(b"=== Home Directory Status Ruler Line 1234567890 ===\r\n");
    // Line 1: Input line with cursor
    term.feed_bytes(b"> ");

    assert_eq!(term.grid().active().cursor.row, 1);
    assert_eq!(term.grid().active().cursor.col, 2);
    assert!(!term.grid().active().grid[0].is_wrapped);
    assert!(!term.grid().active().grid[1].is_wrapped);

    // Resize to 40 cols (narrower than 50 chars)
    term.resize(10, 40).unwrap();

    // The hard line should stay at row 0 without wrapping and without displacing cursor row
    assert_eq!(term.grid().active().cursor.row, 1);
    assert_eq!(term.grid().active().cursor.col, 2);
    assert!(!term.grid().active().grid[0].is_wrapped);
    assert!(!term.grid().active().grid[1].is_wrapped);

    // Shell receives SIGWINCH and moves up 1 line to repaint prompt
    term.feed_bytes(b"\x1b[1A\r\x1b[K=== Narrow Prompt Ruler 12345 ===\r\n\x1b[K> ");

    assert_eq!(term.grid().active().cursor.row, 1);
    assert_eq!(term.grid().active().cursor.col, 2);

    let row0: String = term.grid().active().grid[0].cells[0..33]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row0, "=== Narrow Prompt Ruler 12345 ===");

    // Resize back to 80 cols
    term.resize(10, 80).unwrap();
    assert_eq!(term.grid().active().cursor.row, 1);

    // Shell repaints on 80 cols
    term.feed_bytes(b"\x1b[1A\r\x1b[K=== Wide Prompt Ruler Restored 1234567890 ===\r\n\x1b[K> ");
    let row0_wide: String = term.grid().active().grid[0].cells[0..45]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row0_wide, "=== Wide Prompt Ruler Restored 1234567890 ===");
}

#[test]
fn test_vt_dch_delete_char_on_backspace_in_middle() {
    let mut term = Terminal::new_headless(10, 20, 100);

    // User types "abcdef"
    term.feed_bytes(b"abcdef");
    assert_eq!(term.grid().active().cursor.col, 6);

    // User moves cursor left 2 times (cursor now at 'e', col 4)
    term.feed_bytes(b"\x1b[2D");
    assert_eq!(term.grid().active().cursor.col, 4);

    // User presses backspace to delete 'd': CLI sends \x08 (BS) + \x1b[P (DCH - Delete 1 char)
    term.feed_bytes(b"\x08\x1b[P");
    assert_eq!(term.grid().active().cursor.col, 3);

    let line: String = term.grid().active().grid[0].cells[0..6]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(line, "abcef ");
}

#[test]
fn test_vt_ich_insert_char_and_irm_mode() {
    let mut term = Terminal::new_headless(10, 20, 100);

    // User types "abef"
    term.feed_bytes(b"abef");
    // Move left 2 to 'e' (col 2)
    term.feed_bytes(b"\x1b[2D");

    // Insert 2 blank chars using ICH (\x1b[2@) and type "cd"
    term.feed_bytes(b"\x1b[2@cd");
    let line: String = term.grid().active().grid[0].cells[0..6]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(line, "abcdef");

    // Test Insert Mode (IRM - \x1b[4h / \x1b[4l) at col 4
    term.feed_bytes(b"\x1b[4hXY\x1b[4l");
    let line2: String = term.grid().active().grid[0].cells[0..8]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(line2, "abcdXYef");
}

#[test]
fn test_vt_cjk_wide_char_overwrite_and_backspace() {
    use skterm_core::CellFlags;
    let mut term = Terminal::new_headless(10, 20, 100);

    // Type "你好世界" (cols 0..8)
    term.feed_bytes("你好世界".as_bytes());
    assert_eq!(term.grid().active().cursor.col, 8);

    // Move cursor left to '界' (col 6), then press backspace to delete '世' (cols 4..6)
    // CLI moves cursor to col 4 (\x1b[5G) and sends DCH 2 (\x1b[2P)
    term.feed_bytes(b"\x1b[5G\x1b[2P");

    let cells = &term.grid().active().grid[0].cells;
    assert_eq!(cells[0].c, '你');
    assert!(cells[0].flags.contains(CellFlags::WIDE_CHAR));
    assert!(cells[1].flags.contains(CellFlags::WIDE_CHAR_SPACER));

    assert_eq!(cells[2].c, '好');
    assert!(cells[2].flags.contains(CellFlags::WIDE_CHAR));
    assert!(cells[3].flags.contains(CellFlags::WIDE_CHAR_SPACER));

    assert_eq!(cells[4].c, '界');
    assert!(cells[4].flags.contains(CellFlags::WIDE_CHAR));
    assert!(cells[5].flags.contains(CellFlags::WIDE_CHAR_SPACER));

    assert_eq!(cells[6].c, ' ');
    assert!(!cells[6].flags.contains(CellFlags::WIDE_CHAR));
    assert!(!cells[6].flags.contains(CellFlags::WIDE_CHAR_SPACER));

    // Now test overwriting the right half of a wide char (col 1) with an ASCII char 'A'
    term.feed_bytes(b"\x1b[2GA");
    let cells = &term.grid().active().grid[0].cells;
    // Left half of '你' at col 0 must have been cleared to ' ' with no WIDE_CHAR flag
    assert_eq!(cells[0].c, ' ');
    assert!(!cells[0].flags.contains(CellFlags::WIDE_CHAR));
    assert_eq!(cells[1].c, 'A');
    assert!(!cells[1].flags.contains(CellFlags::WIDE_CHAR_SPACER));
}
