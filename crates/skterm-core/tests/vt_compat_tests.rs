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
        b"\x1b[?62;4;22;52c\x1b[>41;350;0c"
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
    assert_eq!(term.take_pending_responses(), b"\x1bP>|ghostty 1.2.0\x1b\\");

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

#[test]
fn test_osc1337_inline_image_large_streaming_units_and_aspect_ratio() {
    use base64::Engine;

    let mut term = Terminal::new_headless(24, 80, 100);
    term.resize_with_cell_size(24, 80, 9.0, 18.0).unwrap();

    // Generate a 64x32 pseudo-random RGBA PNG whose Base64 encoding exceeds 4 KiB
    // (well above `vte::Parser`'s 1024-byte `ArrayVec` OSC limit).
    let (w, h) = (64u32, 32u32);
    let mut raw_rgba = Vec::with_capacity((w * h * 4) as usize);
    let mut seed: u32 = 0x9E37_79B9;
    for y in 0..h {
        for x in 0..w {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            raw_rgba.push(((seed >> 24) as u8) ^ (x as u8));
            raw_rgba.push(((seed >> 16) as u8) ^ (y as u8));
            raw_rgba.push((seed >> 8) as u8);
            raw_rgba.push(255);
        }
    }
    let mut png_bytes = Vec::new();
    image::RgbaImage::from_raw(w, h, raw_rgba.clone())
        .unwrap()
        .write_to(
            &mut std::io::Cursor::new(&mut png_bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    let b64_raw = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
    assert!(
        b64_raw.len() > 2048,
        "PNG payload should exceed vte's 1024-byte OSC buffer (got {} bytes)",
        b64_raw.len()
    );

    // Wrap Base64 with `\r\n` every 76 chars (matching GNU `base64` output) to ensure
    // embedded C0 newlines inside OSC do not execute linefeeds on the terminal grid.
    let wrapped_b64: String = b64_raw
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap())
        .collect::<Vec<_>>()
        .join("\r\n");

    // 1. `inline=0` (and omitted `inline`) must NOT place an image on the grid
    let dl_seq = format!(
        "\x1b]1337;File=size={};inline=0:{wrapped_b64}\x07",
        png_bytes.len()
    );
    term.feed_bytes(dl_seq.as_bytes());
    assert!(term.grid().active().images.is_empty());
    assert_eq!(term.grid().active().cursor.row, 0);
    assert_eq!(term.grid().active().cursor.col, 0);

    // 2. Stream a >2 KiB `inline=1;width=10;height=4;preserveAspectRatio=1` sequence in 37-byte chunks
    // followed by `\r\nNext` (standard `imgcat` output).
    let name_b64 = base64::engine::general_purpose::STANDARD.encode("banner.png");
    let seq = format!(
        "\x1b]1337;File=name={name_b64};size={};inline=1;width=10;height=4;preserveAspectRatio=1:{wrapped_b64}\x07\r\nNext",
        png_bytes.len()
    );
    for chunk in seq.as_bytes().chunks(37) {
        term.feed_bytes(chunk);
    }

    assert_eq!(term.grid().active().images.len(), 1);
    let img0 = &term.grid().active().images[0];
    assert_eq!((img0.width_px, img0.height_px), (64, 32));
    assert_eq!((img0.row, img0.col), (0, 0));
    assert_eq!((img0.width_cells, img0.height_cells), (10, 4));
    assert_eq!(&*img0.rgba_data, &raw_rgba);

    // Box is `10*9 = 90px` wide by `4*18 = 72px` tall; a 2:1 image fits at `90x45px`, centered vertically (`pad_y = 13.5px`).
    let [ox, oy, dw, dh] = img0.compute_display_box(9.0, 18.0);
    assert!((ox - 0.0).abs() < 1e-3);
    assert!((oy - 13.5).abs() < 1e-3);
    assert!((dw - 90.0).abs() < 1e-3);
    assert!((dh - 45.0).abs() < 1e-3);

    // Trailing `\r\nNext` landed cleanly on row 4 (immediately below the 4-row image), col 4.
    assert_eq!(term.grid().active().cursor.row, 4);
    assert_eq!(term.grid().active().cursor.col, 4);
    let row4: String = term.grid().active().grid[4].cells[0..4]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(row4, "Next");

    // 3. Pixel dimensions + `preserveAspectRatio=0` stretch + `doNotMoveCursor=1` (`ST` terminator)
    term.feed_bytes(b"\x1b[7;6H"); // row 6, col 5 (0-based)
    let stretch_seq = format!(
        "\x1b]1337;File=inline=1;width=90px;height=54px;preserveAspectRatio=0;doNotMoveCursor=1:{b64_raw}\x1b\\"
    );
    term.feed_bytes(stretch_seq.as_bytes());
    assert_eq!(term.grid().active().images.len(), 2);
    // Cursor stayed at (6, 5) because `doNotMoveCursor=1`
    assert_eq!(term.grid().active().cursor.row, 6);
    assert_eq!(term.grid().active().cursor.col, 5);
    let img1 = &term.grid().active().images[1];
    assert_eq!((img1.row, img1.col), (6, 5));
    assert_eq!((img1.width_cells, img1.height_cells), (10, 3));
    let [ox1, oy1, dw1, dh1] = img1.compute_display_box(9.0, 18.0);
    assert!((ox1 - 0.0).abs() < 1e-3 && (oy1 - 0.0).abs() < 1e-3);
    assert!((dw1 - 90.0).abs() < 1e-3 && (dh1 - 54.0).abs() < 1e-3);

    // 4. Viewport percentage width (`width=50%` of 80 cols = 40 cols = 360px, height auto -> 180px = 10 rows)
    let pct_seq = format!("\x1b]1337;File=inline=1;width=50%;doNotMoveCursor=1:{b64_raw}\x07");
    term.feed_bytes(pct_seq.as_bytes());
    assert_eq!(term.grid().active().images.len(), 3);
    let img2 = &term.grid().active().images[2];
    assert_eq!((img2.width_cells, img2.height_cells), (40, 10));
    let [_, _, dw2, dh2] = img2.compute_display_box(9.0, 18.0);
    assert!((dw2 - 360.0).abs() < 1e-3 && (dh2 - 180.0).abs() < 1e-3);
}

#[test]
fn test_osc1337_multipart_bmp_and_auxiliary_commands() {
    use base64::Engine;
    use skterm_core::CursorShape;

    let mut term = Terminal::new_headless(10, 20, 50);
    term.resize_with_cell_size(10, 20, 9.0, 18.0).unwrap();

    // Encode a 4x2 BMP image to verify both BMP support and MultipartFile/FilePart/FileEnd assembly
    let rgba_4x2: Vec<u8> = [20u8, 120, 240, 255].repeat(8);
    let mut bmp_bytes = Vec::new();
    image::RgbaImage::from_raw(4, 2, rgba_4x2.clone())
        .unwrap()
        .write_to(
            &mut std::io::Cursor::new(&mut bmp_bytes),
            image::ImageFormat::Bmp,
        )
        .unwrap();
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bmp_bytes);
    // Split into 3 non-4-byte-aligned chunks
    let (p1, rest) = b64.split_at(7);
    let (p2, p3) = rest.split_at(11);

    term.feed_bytes(b"\x1b]1337;MultipartFile=inline=1;width=6;height=2;preserveAspectRatio=0\x07");
    assert!(term.grid().active().images.is_empty());
    term.feed_bytes(format!("\x1b]1337;FilePart={p1}\x07").as_bytes());
    term.feed_bytes(format!("\x1b]1337;FilePart={p2}\x1b\\").as_bytes());
    term.feed_bytes(format!("\x1b]1337;FilePart={p3}\x07").as_bytes());
    assert!(term.grid().active().images.is_empty());
    term.feed_bytes(b"\x1b]1337;FileEnd\x07");

    assert_eq!(term.grid().active().images.len(), 1);
    let img = &term.grid().active().images[0];
    assert_eq!((img.width_px, img.height_px), (4, 2));
    assert_eq!((img.width_cells, img.height_cells), (6, 2));
    assert_eq!(&*img.rgba_data, &rgba_4x2);
    assert_eq!(term.grid().active().cursor.row, 1);
    assert_eq!(term.grid().active().cursor.col, 6);

    // Interrupted OSC followed immediately by CSI must recover cleanly without losing text
    term.feed_bytes(b"\x1b]1337;File=inline=1:AAAA\x1b[32mOK\x1b[0m");
    assert_eq!(term.grid().active().grid[1].cells[6].c, 'O');
    assert_eq!(term.grid().active().grid[1].cells[7].c, 'K');

    // Auxiliary OSC 1337 commands: CursorShape, CurrentDir, ReportCellSize, ClearScrollback
    term.feed_bytes(b"\x1b]1337;CursorShape=1\x07");
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Beam);
    term.feed_bytes(b"\x1b]1337;CursorShape=2\x07");
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Underline);
    term.feed_bytes(b"\x1b]1337;CursorShape=0\x07");
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Block);

    term.feed_bytes(b"\x1b]1337;CurrentDir=/tmp/skterm\x07");
    assert_eq!(
        term.state.current_working_directory.as_deref(),
        Some("/tmp/skterm")
    );

    term.feed_bytes(b"\x1b]1337;ReportCellSize\x07\x1b]1337;ReportCellSize\x1b\\");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]1337;ReportCellSize=18.0;9.0\x07\x1b]1337;ReportCellSize=18.0;9.0\x1b\\"
    );

    // Scroll image into scrollback, then verify `OSC 1337 ; ClearScrollback` clears scrollback & scrolled-off images
    term.grid_mut().active_mut().scroll_up(5);
    assert!(!term.grid().active().scrollback.is_empty());
    assert_eq!(term.grid().active().images.len(), 1);
    assert!(term.grid().active().images[0].row < 0);
    term.feed_bytes(b"\x1b]1337;ClearScrollback\x07");
    assert!(term.grid().active().scrollback.is_empty());
    assert!(term.grid().active().images.is_empty());
}

#[test]
fn test_osc_1_icon_title_osc_6_doc_url_and_osc_8_id_grouping() {
    let mut term = Terminal::new_headless(10, 40, 50);

    // 1. OSC 1 sets `icon_title` independently of OSC 2 (`title`), and CSI 22;1t / 23;1t stacks it
    term.feed_bytes(b"\x1b]2;Main Window\x07\x1b]1;Worker Tab\x07");
    assert_eq!(term.state.title, "Main Window");
    assert_eq!(term.state.icon_title.as_deref(), Some("Worker Tab"));

    term.feed_bytes(b"\x1b[22;1t\x1b]1;Subtask\x07");
    assert_eq!(term.state.icon_title.as_deref(), Some("Subtask"));
    assert_eq!(term.state.title, "Main Window");

    term.feed_bytes(b"\x1b[23;1t");
    assert_eq!(term.state.icon_title.as_deref(), Some("Worker Tab"));

    // Empty OSC 1 clears `icon_title`
    term.feed_bytes(b"\x1b]1;\x07");
    assert_eq!(term.state.icon_title, None);

    // 2. OSC 6 sets `current_document_url` and populates fallback `current_working_directory`
    term.feed_bytes(b"\x1b]6;file://localhost/Users/eric/Projects/skTerm/Cargo.toml\x07");
    assert_eq!(
        term.state.current_document_url.as_deref(),
        Some("/Users/eric/Projects/skTerm/Cargo.toml")
    );
    assert_eq!(
        term.state.current_working_directory.as_deref(),
        Some("/Users/eric/Projects/skTerm")
    );

    // Explicit OSC 7 overrides CWD without erasing document URL
    term.feed_bytes(b"\x1b]7;file://localhost/Users/eric/Projects/skTerm/crates\x07");
    assert_eq!(
        term.state.current_working_directory.as_deref(),
        Some("/Users/eric/Projects/skTerm/crates")
    );

    // 3. OSC 8 `id=` parameter groups discontinuous link spans onto the same `NonZeroU32`
    term.feed_bytes(
        b"\x1b]8;id=doc1;https://example.com\x07A\x1b]8;;\x07 \x1b]8;id=doc1;https://example.com\x07B\x1b]8;;\x07 \x1b]8;id=doc2;https://example.com\x07C\x1b]8;;\x07",
    );
    let row0 = &term.grid().active().grid[0];
    let id_a = row0.cells[0].hyperlink_id.unwrap();
    let id_b = row0.cells[2].hyperlink_id.unwrap();
    let id_c = row0.cells[4].hyperlink_id.unwrap();
    assert_eq!(id_a, id_b, "matching OSC 8 id= must share hyperlink_id");
    assert_ne!(
        id_a, id_c,
        "distinct OSC 8 id= must allocate separate hyperlink_id"
    );
    assert_eq!(term.url_at(0, 0).as_deref(), Some("https://example.com"));
    assert_eq!(term.url_at(0, 4).as_deref(), Some("https://example.com"));
}

#[test]
fn test_osc_9_notifications_progress_osc_99_and_osc_777() {
    use base64::Engine;
    use skterm_core::{ProgressState, TerminalProgress};

    let mut term = Terminal::new_headless(10, 40, 50);

    // 1. ConEmu / Ghostty `OSC 9;4;<state>;<pct>` progress bar states
    term.feed_bytes(b"\x1b]9;4;1;42\x07");
    assert_eq!(
        term.state.progress,
        Some(TerminalProgress {
            state: ProgressState::Normal,
            percent: 42,
        })
    );
    term.feed_bytes(b"\x1b]9;4;2;80\x07");
    assert_eq!(
        term.state.progress,
        Some(TerminalProgress {
            state: ProgressState::Error,
            percent: 80,
        })
    );
    term.feed_bytes(b"\x1b]9;4;3\x07");
    assert_eq!(
        term.state.progress,
        Some(TerminalProgress {
            state: ProgressState::Indeterminate,
            percent: 0,
        })
    );
    term.feed_bytes(b"\x1b]9;4;4;65\x07");
    assert_eq!(
        term.state.progress,
        Some(TerminalProgress {
            state: ProgressState::Paused,
            percent: 65,
        })
    );
    term.feed_bytes(b"\x1b]9;4;0\x07");
    assert_eq!(term.state.progress, None);

    // 2. iTerm2 `OSC 9;<msg>` and VTE/Ghostty `OSC 777;notify;<title>;<body>`
    term.feed_bytes(b"\x1b]9;Compilation finished\x07");
    term.feed_bytes(b"\x1b]777;notify;CI Runner;All 240 tests passed\x1b\\");
    let notifs = term.take_pending_notifications();
    assert_eq!(notifs.len(), 2);
    assert_eq!(notifs[0].title, "skTerm");
    assert_eq!(notifs[0].body, "Compilation finished");
    assert!(!notifs[0].force_when_focused);
    assert_eq!(notifs[1].title, "CI Runner");
    assert_eq!(notifs[1].body, "All 240 tests passed");

    // 3. Kitty `OSC 99` capability query (`p=?`) and multi-part Base64 notification (`d=0` -> `d=1`)
    term.feed_bytes(b"\x1b]99;i=q1:p=?;?\x1b\\");
    let resp = String::from_utf8(term.take_pending_responses()).unwrap();
    assert_eq!(
        resp,
        "\x1b]99;i=q1:p=?;title,body,close:o=always,unfocused,invisible:u=0,1,2\x1b\\"
    );

    let title_b64 = base64::engine::general_purpose::STANDARD.encode("Deploy Ready");
    let body_b64 = base64::engine::general_purpose::STANDARD.encode("Production v0.3.0");
    term.feed_bytes(format!("\x1b]99;i=n7:d=0:p=title:e=1;{title_b64}\x1b\\").as_bytes());
    assert!(term.take_pending_notifications().is_empty());
    term.feed_bytes(
        format!("\x1b]99;i=n7:d=1:p=body:e=1:o=always:u=2;{body_b64}\x1b\\").as_bytes(),
    );
    let kitty_notifs = term.take_pending_notifications();
    assert_eq!(kitty_notifs.len(), 1);
    assert_eq!(kitty_notifs[0].id.as_deref(), Some("n7"));
    assert_eq!(kitty_notifs[0].title, "Deploy Ready");
    assert_eq!(kitty_notifs[0].body, "Production v0.3.0");
    assert!(kitty_notifs[0].force_when_focused);
    assert!(kitty_notifs[0].urgent);
}

#[test]
fn test_osc_17_19_selection_colors_osc_21_kitty_colors_osc_22_pointer_and_osc_52() {
    use base64::Engine;
    use skterm_core::{ClipboardRequest, PointerShape};

    let mut term = Terminal::new_headless(10, 40, 50);

    // 1. OSC 17 / 19 dynamic selection BG / FG set, query, and OSC 117 / 119 reset
    term.feed_bytes(b"\x1b]17;#334455\x07\x1b]19;#ddeeff\x07\x1b]17;?\x07\x1b]19;?\x1b\\");
    assert_eq!(term.state.dynamic_selection_bg, Some((0x33, 0x44, 0x55)));
    assert_eq!(term.state.dynamic_selection_fg, Some((0xdd, 0xee, 0xff)));
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]17;rgb:3333/4444/5555\x07\x1b]19;rgb:dddd/eeee/ffff\x1b\\"
    );
    term.feed_bytes(b"\x1b]117\x07\x1b]119\x07");
    assert_eq!(term.state.dynamic_selection_bg, None);
    assert_eq!(term.state.dynamic_selection_fg, None);

    // 2. Kitty OSC 21 set, query (`?`), and reset (empty value)
    term.feed_bytes(
        b"\x1b]21;foreground=#123456;selection_background=#654321;color4=#abcdef;foreground=?;4=?\x1b\\",
    );
    assert_eq!(term.state.dynamic_fg, Some((0x12, 0x34, 0x56)));
    assert_eq!(term.state.dynamic_selection_bg, Some((0x65, 0x43, 0x21)));
    assert_eq!(
        term.state.dynamic_palette.get(&4),
        Some(&(0xab, 0xcd, 0xef))
    );
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]21;foreground=rgb:12/34/56;4=rgb:ab/cd/ef\x1b\\"
    );
    term.feed_bytes(b"\x1b]21;foreground=;4=\x1b\\");
    assert_eq!(term.state.dynamic_fg, None);
    assert_eq!(term.state.dynamic_palette.get(&4), None);

    // 3. OSC 22 pointer cursor shape set, stack push (`>`), query (`?`), pop (`<`), and alt-screen reset
    term.feed_bytes(b"\x1b]22;nonexistent_cursor,pointer\x07");
    assert_eq!(term.state.active_pointer_shape(), PointerShape::Pointer);
    term.feed_bytes(b"\x1b]22;>wait\x07\x1b]22;>grabbing\x07\x1b]22;?\x07");
    assert_eq!(term.state.active_pointer_shape(), PointerShape::Grabbing);
    assert_eq!(term.take_pending_responses(), b"\x1b]22;grabbing\x07");
    term.feed_bytes(b"\x1b]22;?pointer,bogus_shape,text\x1b\\");
    assert_eq!(term.take_pending_responses(), b"\x1b]22;1,0,1\x1b\\");
    term.feed_bytes(b"\x1b]22;<\x07");
    assert_eq!(term.state.active_pointer_shape(), PointerShape::Wait);
    // Leaving alternate screen (`?1049l`) clears pointer stack back to Default
    term.feed_bytes(b"\x1b[?1049h\x1b]22;crosshair\x07\x1b[?1049l");
    assert_eq!(term.state.active_pointer_shape(), PointerShape::Default);

    // 4. OSC 52 large (>2 KiB) write, clear, and `?` read query + OSC 1337 Copy
    let big_clip = "skTerm-OSC52-".repeat(250); // 3250 bytes > 1024-byte vte buffer
    let big_b64 = base64::engine::general_purpose::STANDARD.encode(big_clip.as_bytes());
    term.feed_bytes(format!("\x1b]52;c;{big_b64}\x07").as_bytes());
    term.feed_bytes(b"\x1b]52;c;\x07");
    term.feed_bytes(b"\x1b]52;p;?\x1b\\");
    let copy_b64 = base64::engine::general_purpose::STANDARD.encode("iTermCopy");
    term.feed_bytes(format!("\x1b]1337;Copy=:{copy_b64}\x07").as_bytes());

    let clip_reqs = term.take_pending_clipboard_requests();
    assert_eq!(
        clip_reqs,
        vec![
            ClipboardRequest::Write(big_clip),
            ClipboardRequest::Clear,
            ClipboardRequest::Query {
                selection: String::from("p"),
                bell_terminated: false,
            },
            ClipboardRequest::Write(String::from("iTermCopy")),
        ]
    );
}

#[test]
fn test_osc_133_633_semantic_prompts_navigation_and_shell_integration() {
    use skterm_core::{ensure_shell_integration_files, PtySpawnOptions, SemanticZone};

    let mut term = Terminal::new_headless(5, 40, 100);

    // Emit 3 prompts with outputs so earlier prompts scroll into scrollback (`rows = 5`)
    // Prompt 1: exit 0
    term.feed_bytes(b"\x1b]9;4;1;50\x07"); // Stale progress bar before prompt
    term.feed_bytes(
        b"\x1b]133;A\x07$ \x1b]133;B\x07echo one\r\n\x1b]133;C\x07one\r\n\x1b]133;D;0\x07",
    );
    assert_eq!(
        term.state.progress, None,
        "OSC 133;A/D must clear stale progress bar"
    );

    // Prompt 2: exit 127 (error) + multi-line output
    term.feed_bytes(
        b"\x1b]133;A\x07$ \x1b]133;B\x07badcmd\r\n\x1b]133;C\x07err1\r\nerr2\r\nerr3\r\n\x1b]133;D;127\x07",
    );

    // Prompt 3 (VS Code OSC 633 with hex-escaped CWD): active prompt
    term.feed_bytes(b"\x1b]633;P;Cwd=/Users/eric/my\\x20project\x07\x1b]633;A\x07$ \x1b]633;B\x07");
    assert_eq!(
        term.state.current_working_directory.as_deref(),
        Some("/Users/eric/my project")
    );
    assert_eq!(
        term.grid().active().semantic_zone,
        SemanticZone::CommandInput
    );

    let marks: Vec<_> = term.grid().active().prompt_marks.iter().copied().collect();
    assert_eq!(marks.len(), 3);
    assert_eq!(marks[0].exit_code, Some(0));
    assert_eq!(marks[1].exit_code, Some(127));
    assert_eq!(marks[2].exit_code, None);
    assert!(
        marks[0].row < 0,
        "oldest prompt should have scrolled into scrollback"
    );

    // Navigate upward through prompt history (`Cmd+Up`) and back down (`Cmd+Down`)
    assert_eq!(term.grid().active().scroll_offset, 0);
    assert!(term.grid_mut().active_mut().scroll_to_prev_prompt());
    let off1 = term.grid().active().scroll_offset;
    assert!(off1 > 0);
    // Top visible row (0) now aligns with a recorded prompt mark!
    assert!(term.grid().active().prompt_mark_at_visible_row(0).is_some());

    if term.grid_mut().active_mut().scroll_to_prev_prompt() {
        let off2 = term.grid().active().scroll_offset;
        assert!(off2 > off1);
        assert!(term.grid_mut().active_mut().scroll_to_next_prompt());
        assert_eq!(term.grid().active().scroll_offset, off1);
    }
    assert!(term.grid_mut().active_mut().scroll_to_next_prompt());
    assert_eq!(term.grid().active().scroll_offset, 0);

    // Verify shell integration bootstrap files materialize cleanly
    let int_dir =
        ensure_shell_integration_files().expect("shell integration files should materialize");
    assert!(int_dir.join("zsh").join(".zshrc").is_file());
    assert!(int_dir.join("bash").join("skterm.bash").is_file());
    assert!(int_dir
        .join("fish_data")
        .join("fish")
        .join("vendor_conf.d")
        .join("skterm.fish")
        .is_file());

    // Verify `PtyProcess::spawn_with_options` exports `TERM_PROGRAM=ghostty` & `TERM_PROGRAM_VERSION=1.2.0`
    // without leaking `KITTY_WINDOW_ID` or `GHOSTTY_RESOURCES_DIR`.
    #[cfg(unix)]
    {
        let mut live = Terminal::new_with_options(
            10,
            80,
            50,
            PtySpawnOptions {
                shell: Some("/bin/sh".to_string()),
                args: vec![
                    "-c".to_string(),
                    "printf '%s:%s:%s:%s\\n' \"$TERM_PROGRAM\" \"$TERM_PROGRAM_VERSION\" \"$TERM\" \"${GHOSTTY_RESOURCES_DIR:-unset}\"".to_string(),
                ],
                cwd: Some(std::env::temp_dir().to_string_lossy().into_owned()),
                ..Default::default()
            },
        )
        .unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while std::time::Instant::now() < deadline {
            live.poll_pty();
            let line0: String = live.grid().active().grid[0]
                .cells
                .iter()
                .map(|c| c.c)
                .collect();
            if line0.contains("ghostty:") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        let line0: String = live.grid().active().grid[0]
            .cells
            .iter()
            .map(|c| c.c)
            .collect();
        assert!(
            line0.trim().starts_with("ghostty:1.2.0:xterm-256color:"),
            "expected TERM_PROGRAM=ghostty:1.2.0:xterm-256color, got {:?}",
            line0.trim()
        );
    }
}

#[test]
fn test_ghostty_xtgettcap_da1_and_osc8_semicolons() {
    let mut term = Terminal::new_headless(10, 80, 50);

    // DA1 advertises OSC 52 clipboard (52)
    term.feed_bytes(b"\x1b[c");
    assert_eq!(term.take_pending_responses(), b"\x1b[?62;4;22;52c");

    // XTGETTCAP: Hls, Sync, Smol, Rmol, BE, ED, BD, fullkbd
    // Hex: Hls=486c73, Sync=53796e63, Smol=536d6f6c, Rmol=526d6f6c, BE=4245, ED=4544, BD=4244, fullkbd=66756c6c6b6264
    term.feed_bytes(
        b"\x1bP+q486c73;53796e63;536d6f6c;526d6f6c;4245;4544;4244;66756c6c6b6264\x1b\\",
    );
    let resp = String::from_utf8(term.take_pending_responses()).unwrap();
    assert!(
        resp.contains("\x1bP1+r486c73\x1b\\"),
        "missing Hls: {resp:?}"
    );
    assert!(resp.contains("\x1bP1+r53796e63="), "missing Sync: {resp:?}");
    assert!(
        resp.contains("\x1bP1+r536d6f6c=1b5b35336d\x1b\\"),
        "missing Smol: {resp:?}"
    );
    assert!(
        resp.contains("\x1bP1+r526d6f6c=1b5b35356d\x1b\\"),
        "missing Rmol: {resp:?}"
    );
    assert!(
        resp.contains("\x1bP1+r4245=1b5b3f3230303468\x1b\\"),
        "missing BE: {resp:?}"
    );
    assert!(
        resp.contains("\x1bP1+r4544=1b5b3f3230303468\x1b\\"),
        "missing ED: {resp:?}"
    );
    assert!(
        resp.contains("\x1bP1+r4244=1b5b3f323030346c\x1b\\"),
        "missing BD: {resp:?}"
    );
    assert!(
        resp.contains("\x1bP1+r66756c6c6b6264\x1b\\"),
        "missing fullkbd: {resp:?}"
    );

    // OSC 8 URI containing semicolons must not be truncated
    term.feed_bytes(b"\x1b]8;;https://example.com/path;a=1;b=2\x07Link\x1b]8;;\x07");
    assert_eq!(
        term.url_at(0, 0).as_deref(),
        Some("https://example.com/path;a=1;b=2")
    );
}

#[test]
fn test_dual_kkp_stack_mok_and_alt_screen_clearing() {
    use base64::Engine;

    let mut term = Terminal::new_headless(10, 40, 50);

    // Primary shell enables KKP flags=1 and modifyOtherKeys=2
    term.feed_bytes(b"\x1b[>1u\x1b[>4;2mPrimary");
    assert_eq!(term.state.kitty_keyboard_flags(), 1);
    assert_eq!(term.state.modify_other_keys, 2);

    // Enter alt screen (?1049h): alt starts fresh with KKP=0 and MOK=0
    term.feed_bytes(b"\x1b[?1049h");
    assert!(term.grid().is_alternate);
    assert_eq!(term.state.kitty_keyboard_flags(), 0);
    assert_eq!(term.state.modify_other_keys, 0);

    // TUI inside alt screen pushes KKP=3, sets MOK=1, writes text and places an image
    let px = base64::engine::general_purpose::STANDARD.encode([255u8, 0, 0, 255]);
    term.feed_bytes(
        format!("\x1b[>3u\x1b[>4;1mAltContent\x1b_Ga=T,i=42,f=32,s=1,v=1,q=2;{px}\x1b\\")
            .as_bytes(),
    );
    assert_eq!(term.state.kitty_keyboard_flags(), 3);
    assert_eq!(term.state.modify_other_keys, 1);
    assert_eq!(term.grid().alternate.images.len(), 1);

    // Exit alt screen (?1049l): primary shell's KKP=1 and MOK=2 must survive intact!
    term.feed_bytes(b"\x1b[?1049l");
    assert!(!term.grid().is_alternate);
    assert_eq!(term.state.kitty_keyboard_flags(), 1);
    assert_eq!(term.state.modify_other_keys, 2);

    // Re-entering alt screen via ?1049h must clear stale alt screen content AND alternate.images
    term.feed_bytes(b"\x1b[?1049h");
    assert!(term.grid().alternate.images.is_empty());
    let alt_row0: String = term.grid().alternate.grid[0]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(alt_row0.trim(), "");

    // ?1047l clears alt buffer and images on exit
    term.feed_bytes(format!("Stale1047\x1b_Ga=T,i=43,f=32,s=1,v=1,q=2;{px}\x1b\\").as_bytes());
    assert_eq!(term.grid().alternate.images.len(), 1);
    term.feed_bytes(b"\x1b[?1047l");
    assert!(!term.grid().is_alternate);
    assert!(term.grid().alternate.images.is_empty());
    let cleared_row0: String = term.grid().alternate.grid[0]
        .cells
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(cleared_row0.trim(), "");

    // ?47 stays non-clearing per xterm spec
    term.feed_bytes(b"\x1b[?47hKeepMe\x1b[?47l\x1b[?47h");
    let kept: String = term.grid().alternate.grid[0].cells[0..6]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(kept, "KeepMe");
}

#[test]
fn test_mouse_modes_sgr_pixels_cursor_blink_decrqm_and_xtsave_xtrestore() {
    use skterm_core::{MouseEncoding, MouseProtocol};

    let mut term = Terminal::new_headless(10, 40, 50);

    // Selective mouse protocol reset: disabling ?1000l while ?1003 is active must not clear ?1003
    term.feed_bytes(b"\x1b[?1003h\x1b[?1000l");
    assert_eq!(term.state.mouse_protocol, MouseProtocol::AnyEvent);
    term.feed_bytes(b"\x1b[?1003l");
    assert_eq!(term.state.mouse_protocol, MouseProtocol::None);

    // SGR-Pixels (?1016) & selective format reset (?1006l must not clobber ?1016)
    term.feed_bytes(b"\x1b[?1016h\x1b[?1006l");
    assert_eq!(term.state.mouse_format, MouseEncoding::SgrPixels);
    term.feed_bytes(b"\x1b[?1016$p");
    assert_eq!(term.take_pending_responses(), b"\x1b[?1016;1$y");

    // Cursor blink (?12), DECSCUSR odd/even, and DECRQSS " q"
    term.feed_bytes(b"\x1b[2 q\x1bP$q q\x1b\\");
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Block);
    assert!(!term.grid().active().cursor.blink);
    assert_eq!(term.take_pending_responses(), b"\x1bP1$r2 q\x1b\\");

    term.feed_bytes(b"\x1b[6 q\x1bP$q q\x1b\\");
    assert_eq!(term.grid().active().cursor.shape, CursorShape::Beam);
    assert!(!term.grid().active().cursor.blink);
    assert_eq!(term.take_pending_responses(), b"\x1bP1$r6 q\x1b\\");

    term.feed_bytes(b"\x1b[?12h\x1b[?12$p");
    assert!(term.grid().active().cursor.blink);
    assert_eq!(term.take_pending_responses(), b"\x1b[?12;1$y");

    // ?1048 save/restore cursor
    term.feed_bytes(b"\x1b[4;7H\x1b[?1048h\x1b[1;1H\x1b[?1048l");
    assert_eq!(term.grid().active().cursor.row, 3);
    assert_eq!(term.grid().active().cursor.col, 6);

    // DECRQM recognizes ?1007, ?1048, ?2027, ?2031, ?2048
    term.feed_bytes(b"\x1b[?1007$p\x1b[?1048$p\x1b[?2027$p\x1b[?2031$p\x1b[?2048$p");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b[?1007;1$y\x1b[?1048;1$y\x1b[?2027;1$y\x1b[?2031;2$y\x1b[?2048;2$y"
    );

    // XTSAVE (CSI ? Ps s) and XTRESTORE (CSI ? Ps r)
    term.feed_bytes(b"\x1b[?1004h\x1b[?2004h\x1b[?1007l");
    term.feed_bytes(b"\x1b[?1004;2004;1007;2027s");
    term.feed_bytes(b"\x1b[?1004l\x1b[?2004l\x1b[?1007h\x1b[?2027l");
    assert!(!term.state.focus_events);
    assert!(!term.state.bracketed_paste);
    assert!(term.state.alternate_scroll);
    assert!(!term.state.grapheme_clustering);
    term.feed_bytes(b"\x1b[?1004;2004;1007;2027r");
    assert!(term.state.focus_events);
    assert!(term.state.bracketed_paste);
    assert!(!term.state.alternate_scroll);
    assert!(term.state.grapheme_clustering);
}

#[test]
fn test_color_scheme_2031_in_band_resize_2048_theme_defaults_and_progress_timeout() {
    let mut term = Terminal::new_headless(24, 80, 50);

    // 1. ?2031 + CSI ? 996 n color scheme query and notifications
    term.feed_bytes(b"\x1b[?996n");
    assert_eq!(term.take_pending_responses(), b"\x1b[?997;1n"); // default dark

    // Changing scheme while ?2031 is off updates state without emitting unsolicited report
    term.set_color_scheme(false);
    assert!(term.take_pending_responses().is_empty());
    term.feed_bytes(b"\x1b[?996n");
    assert_eq!(term.take_pending_responses(), b"\x1b[?997;2n");

    // Enabling ?2031 subscribes to future changes (no duplicate on unchanged value)
    term.feed_bytes(b"\x1b[?2031h");
    term.set_color_scheme(false);
    assert!(term.take_pending_responses().is_empty());
    term.set_color_scheme(true);
    assert_eq!(term.take_pending_responses(), b"\x1b[?997;1n");
    term.set_color_scheme(false);
    assert_eq!(term.take_pending_responses(), b"\x1b[?997;2n");

    // 2. ?2048 in-band window resize reporting
    term.resize_with_cell_size(24, 80, 10.0, 20.0).unwrap();
    term.feed_bytes(b"\x1b[?2048h");
    // Enabling ?2048 sends an immediate size report
    assert_eq!(term.take_pending_responses(), b"\x1b[48;24;80;480;800t");
    // Resizing sends an updated CSI 48 report
    term.resize_with_cell_size(30, 100, 10.0, 20.0).unwrap();
    assert_eq!(term.take_pending_responses(), b"\x1b[48;30;100;600;1000t");
    // No-op resize emits nothing
    term.resize_with_cell_size(30, 100, 10.0, 20.0).unwrap();
    assert!(term.take_pending_responses().is_empty());
    // Disabling ?2048 suppresses further reports
    term.feed_bytes(b"\x1b[?2048l");
    term.resize_with_cell_size(25, 90, 10.0, 20.0).unwrap();
    assert!(term.take_pending_responses().is_empty());

    // 3. Theme default colors reflected in OSC 4/10/11/12/21;? queries
    let mut pal = [[0u8; 3]; 16];
    pal[1] = [0xaa, 0x11, 0x22];
    term.set_default_colors(
        [0xee, 0xdd, 0xcc],
        [0x11, 0x22, 0x33],
        [0x44, 0x55, 0x66],
        pal,
    );
    term.feed_bytes(b"\x1b]10;?\x07\x1b]11;?\x07\x1b]12;?\x07\x1b]4;1;?\x07");
    assert_eq!(
        term.take_pending_responses(),
        b"\x1b]10;rgb:eeee/dddd/cccc\x07\x1b]11;rgb:1111/2222/3333\x07\x1b]12;rgb:4444/5555/6666\x07\x1b]4;1;rgb:aaaa/1111/2222\x07"
    );

    // 4. Bare malformed `OSC 9;4` must be ignored (no notification toast `"4"`),
    //    and `poll_progress_timeout` clears stale progress after 15s.
    term.feed_bytes(b"\x1b]9;4\x07");
    assert!(term.take_pending_notifications().is_empty());
    assert!(term.state.progress.is_none());

    term.feed_bytes(b"\x1b]9;4;1;55\x07");
    assert!(term.state.progress.is_some());
    let now = std::time::Instant::now();
    assert!(!term.poll_progress_timeout(now + std::time::Duration::from_secs(10)));
    assert!(term.state.progress.is_some());
    assert!(term.poll_progress_timeout(now + std::time::Duration::from_secs(16)));
    assert!(term.state.progress.is_none());
}

#[test]
fn test_unicode_grapheme_clustering_mode_2027_and_reflow() {
    use skterm_core::{CellFlags, Selection, SelectionMode};

    let mut term = Terminal::new_headless(10, 40, 50);

    // 1. NFD combining accents: "e\u{0301}" stays 1 cell with full grapheme preserved
    term.feed_bytes("e\u{0301}x".as_bytes());
    {
        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 2);
        assert_eq!(buf.grid[0].cells[0].c, 'e');
        assert!(!buf.grid[0].cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert_eq!(buf.cell_grapheme(0, 0), Some("e\u{0301}"));
        assert_eq!(buf.grid[0].cells[1].c, 'x');
    }

    // 2. VS16 emoji promotion: "❤️" (U+2764 + U+FE0F) promotes width 1 -> 2
    term.feed_bytes("\r\n\u{2764}\u{FE0F}A".as_bytes());
    {
        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 3);
        assert_eq!(buf.grid[1].cells[0].c, '\u{2764}');
        assert!(buf.grid[1].cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert!(buf.grid[1].cells[1]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER));
        assert_eq!(buf.cell_grapheme(1, 0), Some("\u{2764}\u{FE0F}"));
        assert_eq!(buf.grid[1].cells[2].c, 'A');
    }

    // 3. ZWJ emoji family: "👨‍👩‍👧‍👦" collapses into a single 2-cell cluster
    term.feed_bytes("\r\n👨\u{200D}👩\u{200D}👧\u{200D}👦!".as_bytes());
    {
        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 3);
        assert!(buf.grid[2].cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert!(buf.grid[2].cells[1]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER));
        assert_eq!(
            buf.cell_grapheme(2, 0),
            Some("👨\u{200D}👩\u{200D}👧\u{200D}👦")
        );
        assert_eq!(buf.grid[2].cells[2].c, '!');
    }

    // 4. Skin-tone modifier: "👍🏽" occupies 2 cols (not 4!)
    term.feed_bytes("\r\n👍🏽Z".as_bytes());
    {
        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 3);
        assert!(buf.grid[3].cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert!(buf.grid[3].cells[1]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER));
        assert_eq!(buf.cell_grapheme(3, 0), Some("👍🏽"));
        assert_eq!(buf.grid[3].cells[2].c, 'Z');
    }

    // 5. Regional indicator flag pair: "🇺🇸🇯🇵" occupies 4 cols (two 2-cell clusters)
    term.feed_bytes("\r\n🇺🇸🇯🇵".as_bytes());
    {
        let buf = term.grid().active();
        assert_eq!(buf.cursor.col, 4);
        assert!(buf.grid[4].cells[0].flags.contains(CellFlags::WIDE_CHAR));
        assert!(buf.grid[4].cells[1]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER));
        assert_eq!(buf.cell_grapheme(4, 0), Some("🇺🇸"));
        assert!(buf.grid[4].cells[2].flags.contains(CellFlags::WIDE_CHAR));
        assert!(buf.grid[4].cells[3]
            .flags
            .contains(CellFlags::WIDE_CHAR_SPACER));
        assert_eq!(buf.cell_grapheme(4, 2), Some("🇯🇵"));
    }

    // 6. Selection extracts full grapheme clusters cleanly
    term.grid_mut().active_mut().selection =
        Some(Selection::new((0, 0), (4, 3), SelectionMode::Simple));
    let copied = term.grid().active().extract_selected_text();
    assert_eq!(
        copied,
        "e\u{0301}x\n\u{2764}\u{FE0F}A\n👨\u{200D}👩\u{200D}👧\u{200D}👦!\n👍🏽Z\n🇺🇸🇯🇵"
    );

    // 7. Reflow preserves grapheme clusters across column changes
    term.resize(10, 20).unwrap();
    assert_eq!(term.grid().cell_grapheme(0, 0), Some("e\u{0301}"));
    assert_eq!(
        term.grid().cell_grapheme(2, 0),
        Some("👨\u{200D}👩\u{200D}👧\u{200D}👦")
    );
    assert_eq!(term.grid().cell_grapheme(4, 0), Some("🇺🇸"));
    assert_eq!(term.grid().cell_grapheme(4, 2), Some("🇯🇵"));

    // 8. VS15 narrows Emoji_Presentation base (⌛ U+231B + U+FE0E -> width 1)
    let mut t2 = Terminal::new_headless(5, 20, 10);
    t2.feed_bytes("\u{231B}\u{FE0E}B".as_bytes());
    assert_eq!(t2.grid().active().cursor.col, 2);
    assert!(!t2.grid().active().grid[0].cells[0]
        .flags
        .contains(CellFlags::WIDE_CHAR));
    assert_eq!(t2.grid().cell_grapheme(0, 0), Some("\u{231B}\u{FE0E}"));
    assert_eq!(t2.grid().active().grid[0].cells[1].c, 'B');
}

#[test]
fn test_kitty_graphics_stretch_animation_tick_and_delete_f() {
    use base64::Engine;
    use skterm_core::AnimationState;

    let mut term = Terminal::new_headless(10, 40, 50);
    term.resize_with_cell_size(10, 40, 10.0, 20.0).unwrap();

    // 1. Both `c=` and `r=` stretch to fill `c x r` cells (no letterboxing)
    let red_px = base64::engine::general_purpose::STANDARD.encode([255u8, 0, 0, 255].repeat(4));
    term.feed_bytes(format!("\x1b_Ga=T,i=10,f=32,s=2,v=2,c=6,r=2,C=1;{red_px}\x1b\\").as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=10;OK\x1b\\");
    let img = &term.grid().active().images[0];
    let [ox, oy, dw, dh] = img.compute_display_box(10.0, 20.0);
    assert!((ox - 0.0).abs() < 1e-3 && (oy - 0.0).abs() < 1e-3);
    assert!((dw - 60.0).abs() < 1e-3 && (dh - 40.0).abs() < 1e-3);

    // 2. `a=f` returns `r=<frame_num>` in OK response and accounts for `kitty_storage_bytes`
    let base_bytes = term.grid().kitty_storage_bytes;
    let green_px = base64::engine::general_purpose::STANDARD.encode([0u8, 255, 0, 255].repeat(4));
    let blue_px = base64::engine::general_purpose::STANDARD.encode([0u8, 0, 255, 255].repeat(4));
    term.feed_bytes(format!("\x1b_Ga=f,i=10,f=32,s=2,v=2,X=1,z=50;{green_px}\x1b\\").as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=10,r=2;OK\x1b\\");
    term.feed_bytes(format!("\x1b_Ga=f,i=10,f=32,s=2,v=2,X=1,z=60;{blue_px}\x1b\\").as_bytes());
    assert_eq!(term.take_pending_responses(), b"\x1b_Gi=10,r=3;OK\x1b\\");
    assert_eq!(term.grid().kitty_storage_bytes, base_bytes * 3);

    // 3. `a=a,s=3,v=2` plays frames 0 -> 1 -> 2 and stops after 1 loop (`v=2` => `v-1 = 1` loop)
    term.feed_bytes(b"\x1b_Ga=a,i=10,s=3,v=2\x1b\\");
    let _ = term.take_pending_responses();
    assert_eq!(
        term.grid().kitty_images[&10].animation_state,
        AnimationState::Running
    );
    assert_eq!(term.grid().kitty_images[&10].active_frame, 0);

    let t0 = std::time::Instant::now();
    // Advance to frame 1 (green)
    let (dirty1, next1) = term.tick_animations(t0 + std::time::Duration::from_millis(100));
    assert!(dirty1);
    assert!(next1.is_some());
    assert_eq!(term.grid().kitty_images[&10].active_frame, 1);
    assert_eq!(
        &term.grid().active().images[0].rgba_data[..4],
        &[0, 255, 0, 255]
    );

    // Advance to frame 2 (blue)
    let (dirty2, next2) = term.tick_animations(t0 + std::time::Duration::from_millis(200));
    assert!(dirty2);
    assert!(next2.is_some());
    assert_eq!(term.grid().kitty_images[&10].active_frame, 2);
    assert_eq!(
        &term.grid().active().images[0].rgba_data[..4],
        &[0, 0, 255, 255]
    );

    // Finish loop 1 -> stops because `v=2` (`max_loops = 1`)
    let (dirty3, next3) = term.tick_animations(t0 + std::time::Duration::from_millis(300));
    assert!(!dirty3);
    assert!(next3.is_none());
    assert_eq!(
        term.grid().kitty_images[&10].animation_state,
        AnimationState::Stopped
    );

    // 4. Lowercase `d=f` resets to frame 0 and refreshes texture WITHOUT freeing stored frames;
    //    uppercase `d=F` with `i=0` truncates extra frames and reclaims `kitty_storage_bytes`.
    let tex_before = term.grid().kitty_images[&10].texture_id;
    term.feed_bytes(b"\x1b_Ga=d,d=f,i=10\x1b\\");
    assert_eq!(term.grid().kitty_images[&10].active_frame, 0);
    assert_eq!(term.grid().kitty_images[&10].frames.len(), 3);
    assert_ne!(term.grid().kitty_images[&10].texture_id, tex_before);
    assert_eq!(
        &term.grid().active().images[0].rgba_data[..4],
        &[255, 0, 0, 255]
    );

    term.feed_bytes(b"\x1b_Ga=d,d=F\x1b\\");
    assert_eq!(term.grid().kitty_images[&10].frames.len(), 1);
    assert_eq!(term.grid().kitty_storage_bytes, base_bytes);
}

#[test]
fn test_vt_spec_bugfixes_decstbm_restore_cursor_ed3_ris_and_image_lifecycle() {
    use base64::Engine;

    let mut term = Terminal::new_headless(8, 30, 50);
    term.resize_with_cell_size(8, 30, 11.0, 22.0).unwrap();

    // 1. DECSTBM homes cursor to (0, 0) when DECOM is OFF, and to (top-1, 0) when DECOM is ON
    term.feed_bytes(b"\x1b[5;10H\x1b[3;6r");
    assert_eq!(term.grid().active().cursor.row, 0);
    assert_eq!(term.grid().active().cursor.col, 0);
    term.feed_bytes(b"\x1b[?6h\x1b[3;6r");
    assert_eq!(term.grid().active().cursor.row, 2);
    assert_eq!(term.grid().active().cursor.col, 0);
    term.feed_bytes(b"\x1b[?6l\x1b[r");

    // 2. DECRC / restore_cursor is non-destructive and restores auto_wrap
    term.feed_bytes(b"\x1b[?7l\x1b[4;8H\x1b7\x1b[?7h\x1b[1;1H\x1b8");
    assert_eq!(term.grid().active().cursor.row, 3);
    assert_eq!(term.grid().active().cursor.col, 7);
    assert!(!term.state.auto_wrap);
    // Second restore without re-saving must still work!
    term.feed_bytes(b"\x1b[?7h\x1b[2;2H\x1b8");
    assert_eq!(term.grid().active().cursor.row, 3);
    assert_eq!(term.grid().active().cursor.col, 7);
    assert!(!term.state.auto_wrap);
    term.feed_bytes(b"\x1b[?7h");

    // 3. CSI 3 J (ED 3) clears scrollback ONLY and leaves visible screen intact
    let mut t_ed3 = Terminal::new_headless(4, 20, 50);
    for i in 0..8 {
        t_ed3.feed_bytes(format!("Row {i}\r\n").as_bytes());
    }
    t_ed3.feed_bytes(b"VisibleBottom");
    assert!(!t_ed3.grid().active().scrollback.is_empty());
    t_ed3.feed_bytes(b"\x1b[3J");
    assert!(t_ed3.grid().active().scrollback.is_empty());
    let bottom: String = t_ed3.grid().active().grid[3].cells[0..13]
        .iter()
        .map(|c| c.c)
        .collect();
    assert_eq!(bottom, "VisibleBottom");

    // 4. RIS (ESC c) preserves pixel cell dimensions
    term.feed_bytes(b"\x1bc");
    assert!((term.grid().cell_width_px - 11.0).abs() < 1e-3);
    assert!((term.grid().cell_height_px - 22.0).abs() < 1e-3);

    // 5. IL / DL shift images and home cursor to col 0; ED 0 / ED 1 prune covered images
    let px = base64::engine::general_purpose::STANDARD.encode([255u8, 0, 0, 255]);
    term.feed_bytes(
        format!("\x1b[3;5H\x1b_Ga=T,i=90,f=32,s=1,v=1,c=2,r=1,C=1,q=2;{px}\x1b\\").as_bytes(),
    );
    assert_eq!(term.grid().active().images[0].row, 2);
    // IL at row 2 col 5 shifts image from row 2 -> row 3 and moves cursor to col 0
    term.feed_bytes(b"\x1b[3;5H\x1b[1L");
    assert_eq!(term.grid().active().cursor.col, 0);
    assert_eq!(term.grid().active().images[0].row, 3);
    // DL at row 3 shifts image back to row 2? Wait: DL at row 2 shifts row 3 -> row 2
    term.feed_bytes(b"\x1b[3;5H\x1b[1M");
    assert_eq!(term.grid().active().cursor.col, 0);
    assert_eq!(term.grid().active().images[0].row, 2);
    // ED 0 from row 2 col 0 prunes the image at row 2
    term.feed_bytes(b"\x1b[3;1H\x1b[0J");
    assert!(term.grid().active().images.is_empty());

    // ED 1 from row 3 prunes an image at row 1 while keeping an image at row 5
    term.feed_bytes(
        format!("\x1b[2;1H\x1b_Ga=T,i=91,f=32,s=1,v=1,c=2,r=1,C=1,q=2;{px}\x1b\\").as_bytes(),
    );
    term.feed_bytes(
        format!("\x1b[6;1H\x1b_Ga=T,i=92,f=32,s=1,v=1,c=2,r=1,C=1,q=2;{px}\x1b\\").as_bytes(),
    );
    assert_eq!(term.grid().active().images.len(), 2);
    term.feed_bytes(b"\x1b[4;1H\x1b[1J");
    assert_eq!(term.grid().active().images.len(), 1);
    assert_eq!(term.grid().active().images[0].image_id, 92);

    // 6. Reflow adjusts prompt_marks and image rows cleanly
    let mut t_reflow = Terminal::new_headless(6, 20, 50);
    t_reflow.feed_bytes(b"0123456789abcdefghij\r\n");
    t_reflow.feed_bytes(
        format!("\x1b]133;A\x07$ \x1b_Ga=T,i=99,f=32,s=1,v=1,c=1,r=1,C=1,q=2;{px}\x1b\\")
            .as_bytes(),
    );
    assert_eq!(t_reflow.grid().active().prompt_marks.back().unwrap().row, 1);
    assert_eq!(t_reflow.grid().active().images[0].row, 1);
    // Narrow 20 -> 10: row 0 wraps into 2 rows, cursor stays anchored at row 1 (overflow scrolled to scrollback)
    t_reflow.resize(6, 10).unwrap();
    assert_eq!(t_reflow.grid().active().cursor.row, 1);
    assert_eq!(t_reflow.grid().active().prompt_marks.back().unwrap().row, 1);
    assert_eq!(t_reflow.grid().active().images[0].row, 1);
}
