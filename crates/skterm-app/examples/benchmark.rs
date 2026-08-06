use skterm_core::Terminal;
use std::time::Instant;

fn main() {
    println!("=== skTerm Performance & Throughput Benchmark ===");

    let total_lines = 100_000;
    let mut term = Terminal::new_headless(24, 80, 10_000);

    // Pre-generate sample ANSI stream with colors, bold, numbers, and escape sequences
    let mut test_stream = Vec::new();
    for i in 0..1000 {
        let line = format!(
            "\x1b[32m[INFO]\x1b[0m \x1b[1mProcess #{}\x1b[0m: status=\x1b[38;2;100;200;255mACTIVE\x1b[0m, load=\x1b[33m0.{}%\x1b[0m\r\n",
            i, i % 100
        );
        test_stream.extend_from_slice(line.as_bytes());
    }

    let iterations = total_lines / 1000;
    let start = Instant::now();

    for _ in 0..iterations {
        term.feed_bytes(&test_stream);
    }

    let elapsed = start.elapsed();
    let total_bytes = test_stream.len() * iterations;
    let mb = total_bytes as f64 / (1024.0 * 1024.0);
    let seconds = elapsed.as_secs_f64();
    let mb_per_sec = mb / seconds;
    let lines_per_sec = total_lines as f64 / seconds;

    println!("Processed {} lines ({:.2} MB) in {:.3}s", total_lines, mb, seconds);
    println!("Throughput: {:.2} MB/s", mb_per_sec);
    println!("Line Rate:  {:.0} lines/second", lines_per_sec);
    println!("Benchmark Completed Successfully!");
}
