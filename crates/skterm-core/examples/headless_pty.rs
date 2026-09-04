//! Example demonstrating spawning a real PTY shell, sending commands, and reading the resulting screen grid.

use skterm_core::Terminal;
use std::thread;
use std::time::Duration;

fn main() -> anyhow::Result<()> {
    println!("=== skTerm Headless PTY Demo ===");
    println!("Spawning child shell in PTY (rows: 24, cols: 80)...");

    let mut term = Terminal::new(24, 80, 1000, None, None)?;

    // Give shell process a brief moment to initialize
    thread::sleep(Duration::from_millis(200));
    term.poll_pty();

    println!("Sending command: 'echo \"Hello from skTerm PTY!\"' to child shell...");
    term.write_to_pty(b"echo \"Hello from skTerm PTY!\"\n")?;

    // Wait and poll output
    thread::sleep(Duration::from_millis(300));
    term.poll_pty();

    println!("Current Active Screen Grid Content (Top 5 rows):");
    println!("------------------------------------------------------------");
    for (i, row) in term.grid().active().grid.iter().take(5).enumerate() {
        let line: String = row.cells.iter().map(|c| c.c).collect();
        let trimmed = line.trim_end();
        println!("{:2} | {}", i + 1, trimmed);
    }
    println!("------------------------------------------------------------");

    println!("Sending command: 'echo \"\\033[31;1mRed Bold Text\\033[0m \\033[38;2;0;255;128mTrueColor Text\\033[0m\"'...");
    term.write_to_pty(
        b"echo \"\x1b[31;1mRed Bold Text\x1b[0m \x1b[38;2;0;255;128mTrueColor Text\x1b[0m\"\n",
    )?;

    thread::sleep(Duration::from_millis(300));
    term.poll_pty();

    println!("Screen Grid Inspection after ANSI & TrueColor echo (Top 8 rows):");
    println!("------------------------------------------------------------");
    for (i, row) in term.grid().active().grid.iter().take(8).enumerate() {
        let line: String = row.cells.iter().map(|c| c.c).collect();
        let trimmed = line.trim_end();
        println!("{:2} | {}", i + 1, trimmed);
    }
    println!("------------------------------------------------------------");

    println!(
        "Demo completed successfully! Child shell is alive: {}",
        term.is_alive()
    );
    Ok(())
}
