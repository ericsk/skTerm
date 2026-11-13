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

/// Narrowing must not destroy output, and widening must put it back.
///
/// `reflow` used to keep every visible unwrapped line on exactly one row and drop
/// whatever no longer fit, so shrinking a pane silently ate the tail of every long
/// line. It now wraps them and scrolls the surplus rows off the top, which keeps the
/// cursor on the row the shell expects (see
/// `test_vt_prompt_line_preservation_on_resize`) without losing anything.
#[test]
fn test_vt_narrow_and_widen_roundtrip_preserves_output() {
    const LONG: &str = "-rw-r--r--  1 eric  staff   12345 Jan  1 00:00 a-rather-long-filename.txt";
    const CMD: &str = "$ ls -la /some/very/long/path/that/goes/on";

    let mut term = Terminal::new_headless(10, 80, 1000);
    term.feed_bytes(format!("{CMD}\r\n{LONG}\r\n$ ").as_bytes());
    assert_eq!(term.grid().active().cursor.row, 2);
    assert_eq!(term.grid().active().cursor.col, 2);

    // Everything visible on one line each, collapsed to a single searchable string.
    let flatten = |term: &Terminal| -> String {
        let b = term.grid().active();
        b.scrollback
            .iter()
            .chain(b.grid.iter())
            .flat_map(|r| r.cells.iter().map(|c| c.c))
            .collect::<String>()
            .replace(' ', "")
    };
    let squash = |s: &str| s.replace(' ', "");

    term.resize(10, 40).unwrap();

    let after_narrow = flatten(&term);
    assert!(
        after_narrow.contains(&squash(LONG)),
        "narrowing destroyed the long line"
    );
    assert!(
        after_narrow.contains(&squash(CMD)),
        "narrowing destroyed the command line"
    );
    // The prompt must not have slid down the screen, or the shell's relative
    // repaint after SIGWINCH would land on the wrong row.
    assert_eq!(
        term.grid().active().cursor.row,
        2,
        "cursor row must be anchored across the narrow"
    );
    assert_eq!(term.grid().active().cursor.col, 2);
    // The surplus rows went to scrollback rather than being dropped.
    assert!(!term.grid().active().scrollback.is_empty());

    term.resize(10, 80).unwrap();

    // Widening re-joins the wrapped fragments back into the original lines.
    let b = term.grid().active();
    let long_again: String = b.grid[0].cells.iter().map(|c| c.c).collect();
    assert_eq!(
        long_again.trim_end(),
        LONG,
        "widening did not restore the long line intact"
    );
    let cmd_again: String = b.scrollback[b.scrollback.len() - 1]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(
        cmd_again.trim_end(),
        CMD,
        "widening did not restore the command line intact"
    );
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

#[test]
fn test_vt_sgr_underline_color_and_subparams() {
    use skterm_core::CellFlags;
    let mut term = Terminal::new_headless(10, 80, 100);

    // Exact sequence pattern from Jetski CLI banner ("We're Hiring! (TPM, DevRel)"):
    // RGB components in `58;2;R;G;B` that fall in 90..=97 or 100..=107 must be consumed as
    // the underline color rather than leaking as bright fg/bg SGR codes or DIM (2).
    term.feed_bytes(
        b"\x1b[38;2;66;133;244;58;2;66;133;244;1;4m\x1b]8;id=jk;http://go/jetski-jobs\x07W\
          \x1b[38;2;93;124;231;58;2;93;124;231me\
          \x1b[38;2;100;122;228;58;2;100;122;228m \
          \x1b[38;2;107;120;225;58;2;107;120;225mH\
          \x1b[38;2;224;90;109;58;2;224;90;109m)\x1b[m\x1b]8;;\x07",
    );

    let cells = &term.grid().active().grid[0].cells;
    let expected = [
        ('W', Color::Rgb(66, 133, 244)),
        ('e', Color::Rgb(93, 124, 231)),
        (' ', Color::Rgb(100, 122, 228)),
        ('H', Color::Rgb(107, 120, 225)),
        (')', Color::Rgb(224, 90, 109)),
    ];

    for (idx, (ch, fg)) in expected.iter().enumerate() {
        assert_eq!(cells[idx].c, *ch);
        assert_eq!(cells[idx].fg, *fg, "cell {idx} ({ch:?}) fg mismatch");
        assert_eq!(
            cells[idx].bg,
            Color::DefaultBg,
            "cell {idx} ({ch:?}) bg must remain DefaultBg"
        );
        assert!(cells[idx].flags.contains(CellFlags::BOLD));
        assert!(cells[idx].flags.contains(CellFlags::UNDERLINE));
        assert!(
            !cells[idx].flags.contains(CellFlags::DIM),
            "58;2;... must not set DIM flag"
        );
    }

    // Also test colon-separated ITU T.416 SGR subparameters (4:3, 38:2::R:G:B, 58:2::R:G:B, 4:0)
    term.feed_bytes(b"\r\n\x1b[4:3;38:2::12:34:56;58:2::93:100:107mX\x1b[4:0mY\x1b[m");
    let row1 = &term.grid().active().grid[1].cells;
    assert_eq!(row1[0].c, 'X');
    assert_eq!(row1[0].fg, Color::Rgb(12, 34, 56));
    assert_eq!(row1[0].bg, Color::DefaultBg);
    assert!(row1[0].flags.contains(CellFlags::UNDERLINE));
    assert!(!row1[0].flags.contains(CellFlags::ITALIC));

    assert_eq!(row1[1].c, 'Y');
    assert_eq!(row1[1].fg, Color::Rgb(12, 34, 56));
    assert!(!row1[1].flags.contains(CellFlags::UNDERLINE));

    // Exiting a TUI (like Jetski CLI) emits xterm modifyOtherKeys teardown `\x1b[>4m`
    // (and startup `\x1b[>4;2m`). Because these have intermediate `>`, they must NOT
    // be interpreted as SGR 4 (UNDERLINE) or SGR 2 (DIM) on the shell prompt that follows.
    term.feed_bytes(b"\r\n\x1b[>4;2m\x1b[>4mprompt% ");
    let row2 = &term.grid().active().grid[2].cells;
    for cell in &row2[0..8] {
        assert!(
            !cell.flags.contains(CellFlags::UNDERLINE),
            "CSI > 4 m must not enable UNDERLINE on subsequent prompt text"
        );
        assert!(
            !cell.flags.contains(CellFlags::DIM),
            "CSI > 4 ; 2 m must not enable DIM on subsequent prompt text"
        );
    }
}

#[test]
fn test_vt_device_queries_and_responses() {
    let mut term = Terminal::new_headless(24, 80, 100);

    // DA1 (\x1b[c) and DA2 (\x1b[>c)
    term.feed_bytes(b"\x1b[c\x1b[>c");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b[?62;4;22c\x1b[>41;350;0c"
    );

    // DSR (\x1b[5n), CPR (\x1b[6n), and DECXCPR (\x1b[?6n)
    term.feed_bytes(b"\x1b[4;12H\x1b[5n\x1b[6n\x1b[?6n");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b[0n\x1b[4;12R\x1b[?4;12R"
    );

    // DECRQM for DEC Private Modes (?2026, ?25, ?7) and ANSI mode (4)
    term.feed_bytes(b"\x1b[?2026$p\x1b[?2026h\x1b[?2026$p\x1b[?25$p\x1b[?7$p\x1b[4$p");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b[?2026;2$y\x1b[?2026;1$y\x1b[?25;1$y\x1b[?7;1$y\x1b[4;2$y"
    );

    // OSC 10 (FG) and OSC 11 (BG) dynamic color queries with BEL and ST terminators
    term.feed_bytes(b"\x1b]10;?\x07\x1b]11;?\x1b\\");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]10;rgb:f5f5/f5f5/f7f7\x07\x1b]11;rgb:1a1a/1b1b/1e1e\x1b\\"
    );

    // Kitty Graphics query (a=q) via APC/OSC must reply OK and not add a probe image to the grid
    term.feed_bytes(b"\x1b]Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\");
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=31;OK\x1b\\");
    assert!(term.grid().active().images.is_empty());
}

#[test]
fn test_vt_rep_cht_cbt_hpa_hpr_vpr_and_cnl_cpl() {
    let mut term = Terminal::new_headless(10, 20, 100);

    // CSI b (REP): Repeat preceding graphic character
    term.feed_bytes(b"=\x1b[4b!");
    let row0: String = term.grid().active().grid[0].cells[0..6]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row0, "=====!");

    // CHT (\x1b[2I) forward 2 tab stops from col 0 -> col 16, then CBT (\x1b[1Z) back to col 8
    term.feed_bytes(b"\r\n\x1b[2I\x1b[1Z");
    assert_eq!(term.grid().active().cursor.row, 1);
    assert_eq!(term.grid().active().cursor.col, 8);

    // HPA (\x1b[5`), HPR (\x1b[3a), VPR (\x1b[2e)
    term.feed_bytes(b"\x1b[5`\x1b[3a\x1b[2e");
    assert_eq!(term.grid().active().cursor.col, 7);
    assert_eq!(term.grid().active().cursor.row, 3);

    // Fill row 3 to trigger pending_wrap at col 19, then verify CNL (\x1b[E) clears pending_wrap
    term.feed_bytes(b"\x1b[4;1H01234567890123456789");
    assert!(term.grid().active().cursor.pending_wrap);
    term.feed_bytes(b"\x1b[1E");
    assert_eq!(term.grid().active().cursor.row, 4);
    assert_eq!(term.grid().active().cursor.col, 0);
    assert!(!term.grid().active().cursor.pending_wrap);

    // Verify Kitty Keyboard Protocol push/pop (\x1b[>1u / \x1b[<u) does not trigger ANSI restore_cursor ('u')
    term.feed_bytes(b"\x1b[s\x1b[6;10H\x1b[>1u\x1b[<u");
    assert_eq!(term.grid().active().cursor.row, 5);
    assert_eq!(term.grid().active().cursor.col, 9);
}

#[test]
fn test_vt_bce_scroll_and_linefeed_background() {
    let mut term = Terminal::new_headless(4, 10, 100);

    // Set blue background (\x1b[44m) and scroll up 1 line (\x1b[1S)
    term.feed_bytes(b"\x1b[44m\x1b[1S");
    for cell in &term.grid().active().grid[3].cells {
        assert_eq!(cell.bg, Color::Named(NamedColor::Blue));
    }

    // Move to bottom row, set green background (\x1b[42m), and trigger linefeed (\n)
    term.feed_bytes(b"\x1b[4;1H\x1b[42m\n");
    for cell in &term.grid().active().grid[3].cells {
        assert_eq!(cell.bg, Color::Named(NamedColor::Green));
    }

    // Move to top row, set red background (\x1b[41m), and trigger Reverse Index (\x1bM)
    term.feed_bytes(b"\x1b[1;1H\x1b[41m\x1bM");
    for cell in &term.grid().active().grid[0].cells {
        assert_eq!(cell.bg, Color::Named(NamedColor::Red));
    }
}

#[test]
fn test_vt_decawm_autowrap_and_dec_special_graphics() {
    let mut term = Terminal::new_headless(5, 10, 100);

    // Disable auto-wrap (?7l) and write 12 characters on a 10-col line:
    // the last cell (col 9) should be overwritten in place without wrapping to row 1.
    term.feed_bytes(b"\x1b[?7l0123456789XY\x1b[?7h");
    assert_eq!(term.grid().active().cursor.row, 0);
    assert_eq!(term.grid().active().cursor.col, 9);
    assert!(!term.grid().active().cursor.pending_wrap);
    let row0: String = term.grid().active().grid[0]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row0, "012345678Y");

    // DEC Special Graphics box drawing via ESC ( 0 and ESC ( B
    term.feed_bytes(b"\r\n\x1b(0lqqqk\x1b(B text");
    let row1: String = term.grid().active().grid[1]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row1.trim_end(), "┌───┐ text");

    // G1 charset via ESC ) 0 + SO (0x0E) / SI (0x0F)
    term.feed_bytes(b"\r\n\x1b)0\x0emqqqj\x0fdone");
    let row2: String = term.grid().active().grid[2]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row2.trim_end(), "└───┘done");
}

#[test]
fn test_vt_underline_styles_independent_color_and_overline() {
    use skterm_core::CellFlags;
    let mut term = Terminal::new_headless(5, 20, 100);

    // Write cell 0 with curly underline (4:3) + red underline color (58;2;255;10;20) + green fg (32)
    // Write cell 1 with double underline (21) + blue underline color (58:5:21) + overline (53)
    // Write cell 2 after resetting underline color (59), underline (24), and overline (55)
    term.feed_bytes(b"\x1b[32;4:3;58;2;255;10;20mA\x1b[21;53;58:5:21mB\x1b[24;55;59mC");

    let c0 = term.grid().active().grid[0].cells[0];
    assert_eq!(c0.c, 'A');
    assert_eq!(c0.fg, Color::Named(NamedColor::Green));
    assert!(c0
        .flags
        .contains(CellFlags::UNDERLINE | CellFlags::CURLY_UNDERLINE));
    assert!(!c0.flags.contains(CellFlags::DOUBLE_UNDERLINE));
    let ul0 = c0
        .underline_color_id
        .and_then(|id| term.grid().get_underline_color(id));
    assert_eq!(ul0, Some(Color::Rgb(255, 10, 20)));

    let c1 = term.grid().active().grid[0].cells[1];
    assert_eq!(c1.c, 'B');
    assert_eq!(c1.fg, Color::Named(NamedColor::Green));
    assert!(c1.flags.contains(CellFlags::DOUBLE_UNDERLINE));
    assert!(c1.flags.contains(CellFlags::OVERLINE));
    assert!(!c1.flags.contains(CellFlags::CURLY_UNDERLINE));
    let ul1 = c1
        .underline_color_id
        .and_then(|id| term.grid().get_underline_color(id));
    assert_eq!(ul1, Some(Color::Indexed(21)));

    let c2 = term.grid().active().grid[0].cells[2];
    assert_eq!(c2.c, 'C');
    assert!(!c2.flags.intersects(CellFlags::ANY_UNDERLINE));
    assert!(!c2.flags.contains(CellFlags::OVERLINE));
    assert_eq!(c2.underline_color_id, None);
}

#[test]
fn test_vt_decom_origin_mode_and_focus_events() {
    let mut term = Terminal::new_headless(10, 20, 100);

    // Set scroll region rows 3..7 (1-based) -> 0-based rows 2..6
    // Enable Origin Mode (?6h): cursor should home to (row 2, col 0)
    term.feed_bytes(b"\x1b[3;7r\x1b[?6h");
    assert!(term.state.origin_mode);
    assert_eq!(term.grid().active().cursor.row, 2);
    assert_eq!(term.grid().active().cursor.col, 0);

    // Move to relative (2, 4) -> absolute (row 3, col 3)
    term.feed_bytes(b"\x1b[2;4H");
    assert_eq!(term.grid().active().cursor.row, 3);
    assert_eq!(term.grid().active().cursor.col, 3);

    // Trying to move beyond the scroll region (row 99) clamps to region bottom (row 6)
    term.feed_bytes(b"\x1b[99;1H");
    assert_eq!(term.grid().active().cursor.row, 6);

    // CPR (\x1b[6n) in Origin Mode reports relative row (5, since 6 - 2 + 1 = 5)
    term.feed_bytes(b"\x1b[6n");
    assert_eq!(term.take_pending_responses(), b"\x1b[5;1R");

    // Disable Origin Mode (?6l): cursor homes to absolute (0, 0)
    term.feed_bytes(b"\x1b[?6l");
    assert!(!term.state.origin_mode);
    assert_eq!(term.grid().active().cursor.row, 0);
    assert_eq!(term.grid().active().cursor.col, 0);

    // Focus events mode (?1004h / ?1004$p)
    term.feed_bytes(b"\x1b[?1004h\x1b[?1004$p");
    assert!(term.state.focus_events);
    assert_eq!(term.take_pending_responses(), b"\x1b[?1004;1$y");
}

#[test]
fn test_vt_kitty_keyboard_protocol_and_modify_other_keys() {
    let mut term = Terminal::new_headless(5, 20, 100);

    // Push flags 1 (\x1b[>1u), query (\x1b[?u)
    term.feed_bytes(b"\x1b[>1u\x1b[?u");
    assert_eq!(term.state.kitty_keyboard_flags(), 1);
    assert_eq!(term.take_pending_responses(), b"\x1b[?1u");

    // Push flags 3 (\x1b[>3u), then OR with 8 via mode 2 (\x1b[=8;2u) -> 11
    term.feed_bytes(b"\x1b[>3u\x1b[=8;2u\x1b[?u");
    assert_eq!(term.state.kitty_keyboard_flags(), 11);
    assert_eq!(term.take_pending_responses(), b"\x1b[?11u");

    // Pop 1 entry (\x1b[<1u) -> back to 1
    term.feed_bytes(b"\x1b[<1u\x1b[?u");
    assert_eq!(term.state.kitty_keyboard_flags(), 1);
    assert_eq!(term.take_pending_responses(), b"\x1b[?1u");

    // modifyOtherKeys level 2 (\x1b[>4;2m) and reset (\x1b[>4m)
    term.feed_bytes(b"\x1b[>4;2m");
    assert_eq!(term.state.modify_other_keys, 2);
    term.feed_bytes(b"\x1b[>4m");
    assert_eq!(term.state.modify_other_keys, 0);
}

#[test]
fn test_vt_osc_dynamic_colors_and_palette() {
    let mut term = Terminal::new_headless(5, 20, 100);

    // Set palette index 1 via #RRGGBB and query it via OSC 4;1;?
    term.feed_bytes(b"\x1b]4;1;#ff0080\x07\x1b]4;1;?\x07");
    assert_eq!(
        term.state.dynamic_palette.get(&1).copied(),
        Some((0xff, 0x00, 0x80))
    );
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]4;1;rgb:ffff/0000/8080\x07"
    );

    // Reset palette index 1 via OSC 104;1
    term.feed_bytes(b"\x1b]104;1\x07");
    assert_eq!(term.state.dynamic_palette.get(&1), None);

    // Set dynamic fg (OSC 10), bg (OSC 11), cursor (OSC 12) and query OSC 10;? with ST
    term.feed_bytes(b"\x1b]10;rgb:1212/3434/5656\x07\x1b]10;?\x1b\\");
    assert_eq!(term.state.dynamic_fg, Some((0x12, 0x34, 0x56)));
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]10;rgb:1212/3434/5656\x1b\\"
    );

    // Reset dynamic fg via OSC 110
    term.feed_bytes(b"\x1b]110\x07");
    assert_eq!(term.state.dynamic_fg, None);
}

#[test]
fn test_vt_custom_tab_stops_hts_and_tbc() {
    let mut term = Terminal::new_headless(5, 20, 100);

    // Clear all tab stops (\x1b[3g), move to col 4 (1-based col 5), set tab stop (\x1bH),
    // move to col 11 (1-based col 12), set tab stop (\x1bH).
    term.feed_bytes(b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;12H\x1bH\x1b[1;1H");
    term.feed_bytes(b"\t");
    assert_eq!(term.grid().active().cursor.col, 4);
    term.feed_bytes(b"\t");
    assert_eq!(term.grid().active().cursor.col, 11);

    // Clear current tab stop at col 11 (\x1b[0g), go to col 19, and backward tab (\x1b[Z)
    // should land directly on col 4.
    term.feed_bytes(b"\x1b[0g\x1b[1;20H\x1b[Z");
    assert_eq!(term.grid().active().cursor.col, 4);
}

#[test]
fn test_vt_synchronized_output_mode_2026() {
    let mut term = Terminal::new_headless(5, 20, 100);
    assert!(!term.state.is_synchronized_output_active());

    // Enable synchronized output (?2026h)
    term.feed_bytes(b"\x1b[?2026h");
    assert!(term.state.synchronized_output);
    assert!(term.state.is_synchronized_output_active());

    // Disable synchronized output (?2026l)
    term.feed_bytes(b"\x1b[?2026l");
    assert!(!term.state.synchronized_output);
    assert!(!term.state.is_synchronized_output_active());
}

#[test]
fn test_vt_dcs_xtgettcap_and_decrqss() {
    let mut term = Terminal::new_headless(10, 40, 100);

    // 1. XTGETTCAP: query TN (544e), RGB (524742), and unknown "xx" (7878)
    term.feed_bytes(b"\x1bP+q544e;524742;7878\x1b\\");
    let resp = String::from_utf8(term.take_pending_responses()).unwrap();
    // TN = "xterm-256color" -> 787465726d2d323536636f6c6f72
    assert!(resp.contains("\x1bP1+r544e=787465726d2d323536636f6c6f72\x1b\\"));
    // RGB = "8/8/8" -> 382f382f38
    assert!(resp.contains("\x1bP1+r524742=382f382f38\x1b\\"));
    // Unknown capability "xx" -> 0+r7878
    assert!(resp.contains("\x1bP0+r7878\x1b\\"));

    // 2. DECRQSS for SGR (`m`), DECSTBM (`r`), and DECSCUSR (` q`)
    term.feed_bytes(b"\x1b[1;4:3;38;2;10;20;30m\x1b[2;8r\x1b[5 q");
    term.feed_bytes(b"\x1bP$qm\x1b\\\x1bP$qr\x1b\\\x1bP$q q\x1b\\");
    let rqss_resp = String::from_utf8(term.take_pending_responses()).unwrap();
    assert!(
        rqss_resp.contains("\x1bP1$r0;1;4:3;38;2;10;20;30m\x1b\\"),
        "unexpected SGR DECRQSS response: {:?}",
        rqss_resp
    );
    assert!(
        rqss_resp.contains("\x1bP1$r2;8r\x1b\\"),
        "unexpected DECSTBM DECRQSS response: {:?}",
        rqss_resp
    );
    assert!(
        rqss_resp.contains("\x1bP1$r5 q\x1b\\"),
        "unexpected DECSCUSR DECRQSS response: {:?}",
        rqss_resp
    );
}

#[test]
fn test_vt_decstr_xtversion_and_xtwinops() {
    let mut term = Terminal::new_headless(24, 80, 100);

    // XTVERSION (CSI > q)
    term.feed_bytes(b"\x1b[>q");
    assert_eq!(
        term.take_pending_responses(),
        concat!("\x1bP>|skTerm(", env!("CARGO_PKG_VERSION"), ")\x1b\\").as_bytes()
    );

    // XTWINOPS: report text area size in chars (CSI 18 t) and cell size (CSI 16 t)
    term.feed_bytes(b"\x1b[18t\x1b[16t");
    assert_eq!(term.take_pending_responses(), b"\x1b[8;24;80t\x1b[6;18;9t");

    // XTWINOPS title stack push (CSI 22;0 t) and pop (CSI 23;0 t)
    term.feed_bytes(b"\x1b]2;Initial Title\x07\x1b[22;0t\x1b]2;Temp Title\x07");
    assert_eq!(term.state.title, "Temp Title");
    term.feed_bytes(b"\x1b[23;0t");
    assert_eq!(term.state.title, "Initial Title");

    // DECSTR soft reset (CSI ! p): resets SGR, origin mode, scroll region, app cursor keys, insert mode
    term.feed_bytes(b"\x1b[1;31m\x1b[5;15r\x1b[?6h\x1b[?1h\x1b[4h");
    assert!(term.state.origin_mode);
    assert!(term.state.application_cursor_keys);
    assert!(term.state.insert_mode);
    term.feed_bytes(b"\x1b[!p");
    assert!(!term.state.origin_mode);
    assert!(!term.state.application_cursor_keys);
    assert!(!term.state.insert_mode);
    assert_eq!(term.grid().active().scroll_region, (0, 23));
    assert!(!term.state.flags.contains(skterm_core::CellFlags::BOLD));
}

#[test]
fn test_kitty_graphics_apc_streaming_chunking_zlib_and_formats() {
    use base64::Engine;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let mut term = Terminal::new_headless(24, 80, 100);

    // 1. Standard APC query (`\x1b_G...a=q...\x1b\\`) fed byte-by-byte across stream boundaries
    let query = b"\x1b_Gi=31,s=1,v=1,a=q,t=d,f=24;AAAA\x1b\\";
    for &b in query {
        term.feed_bytes(&[b]);
    }
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=31;OK\x1b\\");
    assert!(term.grid().active().images.is_empty());
    assert!(term.grid().kitty_images.is_empty());

    // 2. Interrupted APC followed immediately by CSI SGR must recover without swallowing text
    term.feed_bytes(b"\x1b_Ga=T,f=32,s=1,v=1;AAAA\x1b[32mOK\x1b[0m\r\n");
    assert_eq!(term.grid().active().grid[0].cells[0].c, 'O');
    assert_eq!(term.grid().active().grid[0].cells[1].c, 'K');

    // 3. Multi-chunk (`m=1` -> `m=0`, split at non-multiple-of-4 boundary) + zlib (`o=z`) RGBA upload (`a=t`)
    let rgba_2x2: Vec<u8> = vec![
        255, 0, 0, 255, // red
        0, 255, 0, 255, // green
        0, 0, 255, 255, // blue
        255, 255, 0, 128, // semi-transparent yellow
    ];
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
    enc.write_all(&rgba_2x2).unwrap();
    let compressed = enc.finish().unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&compressed);
    let (part1, part2) = b64.split_at(5); // intentionally not divisible by 4!

    let cmd1 = format!("\x1b_Ga=t,i=7,f=32,s=2,v=2,o=z,m=1;{part1}\x1b\\");
    let cmd2 = format!("\x1b_Gm=0;{part2}\x1b\\");
    term.feed_bytes(cmd1.as_bytes());
    assert!(term.take_pending_responses().is_empty()); // No response until m=0
    term.feed_bytes(cmd2.as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=7;OK\x1b\\");

    // `a=t` stores image 7 without placing it on screen
    assert!(term.grid().kitty_images.contains_key(&7));
    assert_eq!(&*term.grid().kitty_images[&7].rgba_data, &rgba_2x2);
    assert!(term.grid().active().images.is_empty());

    // 4. `f=100` real PNG image via `a=T` (transmit & display) with `C=1` (fixed cursor) and `z=-1`
    let mut png_bytes: Vec<u8> = Vec::new();
    {
        let img_buf = image::RgbaImage::from_raw(2, 2, rgba_2x2.clone()).unwrap();
        img_buf
            .write_to(
                &mut std::io::Cursor::new(&mut png_bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
    }
    let png_b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    term.feed_bytes(b"\x1b[5;10H"); // row 4, col 9 (0-based)
    let png_cmd = format!("\x1b_Ga=T,i=9,p=3,f=100,c=4,r=2,C=1,z=-1;{png_b64}\x1b\\");
    term.feed_bytes(png_cmd.as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=9,p=3;OK\x1b\\");
    // Cursor did not move because C=1
    assert_eq!(term.grid().active().cursor.row, 4);
    assert_eq!(term.grid().active().cursor.col, 9);
    assert_eq!(term.grid().active().images.len(), 1);
    let placed = &term.grid().active().images[0];
    assert_eq!(placed.image_id, 9);
    assert_eq!(placed.placement_id, 3);
    assert_eq!((placed.row, placed.col), (4, 9));
    assert_eq!((placed.width_cells, placed.height_cells), (4, 2));
    assert_eq!(placed.z_index, -1);
}

#[test]
fn test_kitty_graphics_put_delete_scroll_and_animation() {
    use base64::Engine;

    let mut term = Terminal::new_headless(10, 20, 5);
    let rgb_2x2: Vec<u8> = vec![
        255, 0, 0, // (0,0)
        0, 255, 0, // (1,0)
        0, 0, 255, // (0,1)
        255, 255, 255, // (1,1)
    ];
    let b64 = base64::engine::general_purpose::STANDARD.encode(&rgb_2x2);

    // Upload by image number `I=55` (`a=t`)
    let upload = format!("\x1b_Ga=t,I=55,f=24,s=2,v=2;{b64}\x1b\\");
    term.feed_bytes(upload.as_bytes());
    let resp = String::from_utf8(term.take_pending_responses()).unwrap();
    assert!(resp.contains(",I=55;OK\x1b\\"), "unexpected resp: {resp}");

    // Place using `I=55,p=1` at (0, 0) with `c=3,r=2` (`C=0` advances cursor down 1 row, right 3 cols)
    term.feed_bytes(b"\x1b[1;1H\x1b_Ga=p,I=55,p=1,c=3,r=2\x1b\\");
    let _ = term.take_pending_responses();
    assert_eq!(term.grid().active().images.len(), 1);
    assert_eq!(term.grid().active().cursor.row, 1);
    assert_eq!(term.grid().active().cursor.col, 3);

    // Re-putting the same `(I=55, p=1)` moves/replaces the placement rather than duplicating it
    term.feed_bytes(b"\x1b[4;6H\x1b_Ga=p,I=55,p=1,c=2,r=2,C=1\x1b\\");
    let _ = term.take_pending_responses();
    assert_eq!(term.grid().active().images.len(), 1);
    assert_eq!(term.grid().active().images[0].row, 3);
    assert_eq!(term.grid().active().images[0].col, 5);

    // Relative placement (`P=<id>,Q=1,H=2,V=1`)
    let stored_id = term.grid().active().images[0].image_id;
    let rel_cmd = format!("\x1b_Ga=p,i={stored_id},p=2,P={stored_id},Q=1,H=2,V=1,c=1,r=1\x1b\\");
    term.feed_bytes(rel_cmd.as_bytes());
    let _ = term.take_pending_responses();
    assert_eq!(term.grid().active().images.len(), 2);
    assert_eq!(term.grid().active().images[1].row, 4);
    assert_eq!(term.grid().active().images[1].col, 7);

    // Animation frame (`a=f` + `a=a,c=2`) updates texture binding & RGBA pixels
    let green_frame =
        base64::engine::general_purpose::STANDARD.encode([0u8, 200, 0, 255].repeat(4));
    let old_tex_id = term.grid().kitty_images[&stored_id].texture_id;
    let frame_cmd = format!("\x1b_Ga=f,i={stored_id},f=32,s=2,v=2,X=1;{green_frame}\x1b\\");
    let anim_cmd = format!("\x1b_Ga=a,i={stored_id},c=2\x1b\\");
    term.feed_bytes(frame_cmd.as_bytes());
    term.feed_bytes(anim_cmd.as_bytes());
    let _ = term.take_pending_responses();
    assert_ne!(term.grid().kitty_images[&stored_id].texture_id, old_tex_id);
    assert_eq!(
        &term.grid().active().images[0].rgba_data[..4],
        &[0, 200, 0, 255]
    );

    // Lowercase `d=p` (1-based x=8, y=5 -> 0-based col=7, row=4) removes placement 2 only, keeping StoredImage
    term.feed_bytes(b"\x1b_Ga=d,d=p,x=8,y=5\x1b\\");
    assert_eq!(term.grid().active().images.len(), 1);
    assert!(term.grid().kitty_images.contains_key(&stored_id));

    // Scroll placement 1 (at row 3, height 2) up by 5 lines -> row becomes -2 (in scrollback, NOT stuck at 0!)
    term.grid_mut().active_mut().scroll_up(5);
    assert_eq!(term.grid().active().images.len(), 1);
    assert_eq!(term.grid().active().images[0].row, -2);

    // Scroll 6 more lines past `max_scrollback = 5` -> image is evicted from scrollback automatically
    term.grid_mut().active_mut().scroll_up(6);
    assert!(term.grid().active().images.is_empty());

    // Uppercase `d=I` frees the stored image from memory
    let del_cmd = format!("\x1b_Ga=d,d=I,i={stored_id}\x1b\\");
    term.feed_bytes(del_cmd.as_bytes());
    assert!(!term.grid().kitty_images.contains_key(&stored_id));
}

#[test]
fn test_kitty_graphics_file_temp_and_shm_mediums() {
    use base64::Engine;

    let mut term = Terminal::new_headless(10, 20, 100);
    let pixels: Vec<u8> = vec![10, 20, 30, 255, 40, 50, 60, 255];

    // 1. Safe `t=t` temporary file containing `tty-graphics-protocol` in system temp_dir IS deleted after read
    let safe_tmp = std::env::temp_dir().join(format!(
        "skterm-tty-graphics-protocol-{}.raw",
        std::process::id()
    ));
    std::fs::write(&safe_tmp, &pixels).unwrap();
    let safe_b64 =
        base64::engine::general_purpose::STANDARD.encode(safe_tmp.to_string_lossy().as_bytes());
    let cmd_t = format!("\x1b_Ga=t,i=101,t=t,f=32,s=2,v=1;{safe_b64}\x1b\\");
    term.feed_bytes(cmd_t.as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=101;OK\x1b\\");
    assert!(
        !safe_tmp.exists(),
        "tty-graphics-protocol temp file must be removed"
    );
    assert_eq!(&*term.grid().kitty_images[&101].rgba_data, &pixels);

    // 2. Security guard: `t=t` file WITHOUT `tty-graphics-protocol` in its name is read, NOT deleted!
    let unsafe_tmp =
        std::env::temp_dir().join(format!("skterm-keep-me-{}.raw", std::process::id()));
    let mut padded = vec![99u8; 4];
    padded.extend_from_slice(&pixels);
    std::fs::write(&unsafe_tmp, &padded).unwrap();
    let unsafe_b64 =
        base64::engine::general_purpose::STANDARD.encode(unsafe_tmp.to_string_lossy().as_bytes());
    // Also exercise `O=4,S=8` offset/length slicing!
    let cmd_keep = format!("\x1b_Ga=t,i=102,t=t,f=32,s=2,v=1,O=4,S=8;{unsafe_b64}\x1b\\");
    term.feed_bytes(cmd_keep.as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=102;OK\x1b\\");
    assert!(
        unsafe_tmp.exists(),
        "file without tty-graphics-protocol marker must NOT be deleted"
    );
    let _ = std::fs::remove_file(&unsafe_tmp);
    assert_eq!(&*term.grid().kitty_images[&102].rgba_data, &pixels);

    // 3. POSIX Shared Memory (`t=s`) on Unix (using POSIX `mmap`, required on macOS XNU)
    #[cfg(unix)]
    {
        let shm_name = format!("/skterm_{}", std::process::id());
        let c_name = std::ffi::CString::new(shm_name.clone()).unwrap();
        unsafe {
            let _ = libc::shm_unlink(c_name.as_ptr());
        }
        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        if fd >= 0 {
            unsafe {
                let _ = libc::ftruncate(fd, pixels.len() as libc::off_t);
                let ptr = libc::mmap(
                    std::ptr::null_mut(),
                    pixels.len(),
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    fd,
                    0,
                );
                assert_ne!(ptr, libc::MAP_FAILED);
                std::ptr::copy_nonoverlapping(pixels.as_ptr(), ptr as *mut u8, pixels.len());
                let _ = libc::munmap(ptr, pixels.len());
                let _ = libc::close(fd);
            }
            let shm_b64 = base64::engine::general_purpose::STANDARD.encode(shm_name.as_bytes());
            let cmd_s = format!("\x1b_Ga=t,i=103,t=s,f=32,s=2,v=1;{shm_b64}\x1b\\");
            term.feed_bytes(cmd_s.as_bytes());
            assert_eq!(term.take_pending_responses(), b"\x1b_Gi=103;OK\x1b\\");
            assert_eq!(&*term.grid().kitty_images[&103].rgba_data, &pixels);

            // Verify the SHM object was unlinked by the terminal
            let reopen = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
            assert!(
                reopen < 0,
                "POSIX SHM object must be unlinked after t=s read"
            );
        }
    }
}

#[test]
fn test_kitty_unicode_placeholders_and_diacritics() {
    use base64::Engine;
    use skterm_core::{
        kitty_diacritic_index, PlaceholderCellMeta, Selection, SelectionMode, KITTY_DIACRITICS,
        KITTY_PLACEHOLDER_CHAR,
    };

    // Verify canonical 297 diacritics table invariants
    assert_eq!(KITTY_DIACRITICS.len(), 297);
    assert_eq!(KITTY_DIACRITICS[0], 0x0305);
    assert_eq!(KITTY_DIACRITICS[296], 0x1D244);
    for pair in KITTY_DIACRITICS.windows(2) {
        assert!(
            pair[0] < pair[1],
            "KITTY_DIACRITICS must be strictly sorted"
        );
    }
    assert_eq!(kitty_diacritic_index('\u{0305}'), Some(0));
    assert_eq!(kitty_diacritic_index('\u{1D244}'), Some(296));

    let mut term = Terminal::new_headless(6, 20, 50);
    let b64 = base64::engine::general_purpose::STANDARD.encode([255u8, 128, 0, 255].repeat(4));
    // Upload image `i=0x05010203` (requires 3rd diacritic MSB = 5!) and register `U=1` virtual placement `p=7`
    let id_full: u32 = 0x0501_0203;
    let upload = format!("\x1b_Ga=t,i={id_full},f=32,s=2,v=2,q=2;{b64}\x1b\\");
    let virt = format!("\x1b_Ga=p,U=1,i={id_full},p=7,c=3,r=2,q=2\x1b\\");
    term.feed_bytes(upload.as_bytes());
    term.feed_bytes(virt.as_bytes());
    assert!(term.grid().active().images.is_empty());
    assert!(term.grid().lookup_virtual_placement(id_full, 7).is_some());

    // Emit SGR fg `38;2;1;2;3` + underline color `58;2;0;0;7`, then placeholder at (0,0) with
    // row=1 (0x030D), col=2 (0x030E), id_msb=5 (0x033D), followed by an abbreviated bare placeholder!
    let d_row1 = char::from_u32(KITTY_DIACRITICS[1]).unwrap();
    let d_col2 = char::from_u32(KITTY_DIACRITICS[2]).unwrap();
    let d_msb5 = char::from_u32(KITTY_DIACRITICS[5]).unwrap();
    let text = format!(
        "\x1b[38;2;1;2;3;58:2::0:0:7m{p}{d_row1}{d_col2}{d_msb5}{p}\x1b[0mHi",
        p = KITTY_PLACEHOLDER_CHAR
    );
    term.feed_bytes(text.as_bytes());

    let row0 = &term.grid().active().grid[0];
    assert_eq!(row0.cells[0].c, KITTY_PLACEHOLDER_CHAR);
    let meta0 = PlaceholderCellMeta::unpack(row0.cells[0].hyperlink_id.unwrap()).unwrap();
    assert_eq!(meta0.diacritic_count, 3);
    assert_eq!((meta0.row_idx, meta0.col_idx, meta0.id_msb), (1, 2, 5));
    // Placeholder metadata must never masquerade as an OSC 8 URL
    assert!(term.url_at(0, 0).is_none());

    // Second cell has 0 diacritics (omitted-diacritic run-length continuation)
    assert_eq!(row0.cells[1].c, KITTY_PLACEHOLDER_CHAR);
    let meta1 = PlaceholderCellMeta::unpack(row0.cells[1].hyperlink_id.unwrap()).unwrap();
    assert_eq!(meta1.diacritic_count, 0);
    assert_eq!(row0.cells[2].c, 'H');
    assert_eq!(row0.cells[3].c, 'i');

    // Selecting across placeholder cells substitutes spaces instead of leaking PUA codepoints
    term.grid_mut().active_mut().selection =
        Some(Selection::new((0, 0), (0, 3), SelectionMode::Simple));
    assert_eq!(term.grid().active().extract_selected_text(), "  Hi");
}
