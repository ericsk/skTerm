//! `skterm`: Main application entry point for skTerm cross-platform terminal emulator.

use skterm_gui::App;
use tracing::info;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting skTerm GPU-accelerated terminal emulator...");

    let app = App::new(14.0);
    app.run()?;

    Ok(())
}
