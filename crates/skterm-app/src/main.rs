//! `skterm`: Main application entry point for skTerm cross-platform terminal emulator.

#![windows_subsystem = "windows"]

use skterm_gui::App;
use tracing::info;

fn main() -> anyhow::Result<()> {
    #[cfg(target_os = "windows")]
    unsafe {
        #[link(name = "kernel32")]
        extern "system" {
            fn AttachConsole(dwProcessId: u32) -> i32;
        }
        // Attach to parent console if started from CMD/PowerShell,
        // so CLI logging still works when run from a command prompt.
        let _ = AttachConsole(0xFFFFFFFF);

        #[link(name = "shell32")]
        extern "system" {
            fn SetCurrentProcessExplicitAppUserModelID(AppID: *const u16) -> i32;
        }
        use std::os::windows::ffi::OsStrExt;
        let app_id: Vec<u16> = std::ffi::OsStr::new("ericsk.skTerm")
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }

    tracing_subscriber::fmt::init();
    info!("Starting skTerm GPU-accelerated terminal emulator...");

    let app = App::new();
    app.run()?;

    Ok(())
}
