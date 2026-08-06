//! `skterm`: Main application entry point for skTerm cross-platform terminal emulator.

use tracing::info;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting skTerm terminal emulator...");
    println!("skTerm - High-Performance Cross-Platform Terminal Emulator (v0.1.0)");
    println!("  - Core: {}", skterm_core::core_info());
    println!("  - Render: {}", skterm_render::render_info());
    println!("  - GUI: {}", skterm_gui::gui_info());
    Ok(())
}
