//! `skterm`: Main application entry point for skTerm cross-platform terminal emulator.

#![cfg_attr(windows, windows_subsystem = "windows")]

use skterm_gui::App;
use tracing::info;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting skTerm GPU-accelerated terminal emulator...");

    let app = App::new();
    app.run()?;

    Ok(())
}
