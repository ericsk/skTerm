#[cfg(target_os = "windows")]
use skterm_core::Terminal;
#[cfg(target_os = "windows")]
use std::time::{Duration, Instant};

#[test]
#[cfg(target_os = "windows")]
fn test_terminal_pwsh_poll_coalescence() {
    let pwsh_path = match skterm_core::find_pwsh_path() {
        Some(p) => p,
        None => return,
    };

    println!("\n=== Testing Terminal with pwsh ===");
    let mut term =
        Terminal::new(24, 80, 1000, Some(pwsh_path), None).expect("failed to spawn terminal");

    // Wait for startup prompt (DA1 is automatically answered!)
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(2000) {
        if term.poll_pty() {
            // Check if cursor moved or prompt appeared
            if term.grid().active().cursor.col > 0 {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    // Now send key 'a'
    let key_time = Instant::now();
    term.write_to_pty(b"a").expect("failed to write key");

    let mut poll_true_count = 0;
    while key_time.elapsed() < Duration::from_millis(500) {
        if term.poll_pty() {
            poll_true_count += 1;
        }
        std::thread::sleep(Duration::from_millis(1));
    }

    assert!(
        poll_true_count <= 2,
        "poll_pty should coalesce burst into <= 2 polls, but got {}",
        poll_true_count
    );
}
