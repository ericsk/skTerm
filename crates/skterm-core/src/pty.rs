//! Cross-platform PTY subprocess management using portable-pty.

use crossbeam_channel::{unbounded, Receiver, Sender};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use tracing::{error, info};

/// Events produced by the background PTY reader thread.
#[derive(Debug, Clone)]
pub enum PtyEvent {
    Output(Vec<u8>),
    Exit(Option<u32>),
}

/// Advanced options for spawning a PTY subprocess.
#[derive(Debug, Clone, Default)]
pub struct PtySpawnOptions {
    pub shell: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub term: Option<String>,
    pub env: HashMap<String, String>,
}

/// A running PTY process with asynchronous reader and synchronized writer.
pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    event_rx: Receiver<PtyEvent>,
    _reader_thread: thread::JoinHandle<()>,
}

impl PtyProcess {
    /// Spawn a child shell with basic parameters.
    pub fn spawn(
        rows: u16,
        cols: u16,
        custom_shell: Option<String>,
        cwd: Option<String>,
    ) -> anyhow::Result<Self> {
        Self::spawn_with_options(
            rows,
            cols,
            PtySpawnOptions {
                shell: custom_shell,
                cwd,
                ..Default::default()
            },
        )
    }

    /// Spawn a child shell with full custom options.
    pub fn spawn_with_options(
        rows: u16,
        cols: u16,
        options: PtySpawnOptions,
    ) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(size)?;
        let shell_path = options.shell.unwrap_or_else(default_shell);

        let mut cmd = CommandBuilder::new(&shell_path);
        for arg in &options.args {
            cmd.arg(arg);
        }

        if let Some(dir) = options.cwd {
            // Expand ~ if needed
            let expanded_dir = if dir.starts_with('~') {
                if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
                    let mut path = std::path::PathBuf::from(home);
                    if dir.len() > 1 {
                        path.push(dir.trim_start_matches("~/").trim_start_matches("~\\"));
                    }
                    path.to_string_lossy().to_string()
                } else {
                    dir
                }
            } else {
                dir
            };
            cmd.cwd(expanded_dir);
        }

        // Set terminal environment variables
        let term_name = options.term.unwrap_or_else(|| "xterm-256color".to_string());
        cmd.env("TERM", term_name);
        cmd.env("COLORTERM", "truecolor");

        // Custom environment variables
        for (k, v) in options.env {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd)?;
        let child = Arc::new(Mutex::new(child));
        let child_clone = Arc::clone(&child);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));

        let (event_tx, event_rx): (Sender<PtyEvent>, Receiver<PtyEvent>) = unbounded();

        let reader_thread = thread::Builder::new()
            .name("skterm-pty-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            info!("PTY stream reached EOF");
                            let exit_code = child_clone
                                .lock()
                                .ok()
                                .and_then(|mut c| c.try_wait().ok().flatten())
                                .map(|status| status.exit_code());
                            let _ = event_tx.send(PtyEvent::Exit(exit_code));
                            break;
                        }
                        Ok(n) => {
                            if event_tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(e) => {
                            error!("PTY read error: {:?}", e);
                            let exit_code = child_clone
                                .lock()
                                .ok()
                                .and_then(|mut c| c.try_wait().ok().flatten())
                                .map(|status| status.exit_code());
                            let _ = event_tx.send(PtyEvent::Exit(exit_code));
                            break;
                        }
                    }
                }
            })?;

        Ok(Self {
            master: pair.master,
            writer,
            child,
            event_rx,
            _reader_thread: reader_thread,
        })
    }

    /// Write user input bytes into the PTY.
    pub fn write_all(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().map_err(|e| anyhow::anyhow!("Writer lock poisoned: {:?}", e))?;
        writer.write_all(data)?;
        writer.flush()?;
        Ok(())
    }

    /// Non-blocking try to read an event from the reader thread.
    pub fn try_recv(&self) -> Option<PtyEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Resize the master PTY window size.
    pub fn resize(&self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Check if the child process has exited.
    pub fn is_alive(&self) -> bool {
        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(_)) => false,
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }
}

/// Detect default system shell path based on platform and environment.
fn default_shell() -> String {
    if let Ok(shell) = std::env::var("SHELL") {
        if !shell.is_empty() {
            return shell;
        }
    }

    #[cfg(target_os = "macos")]
    {
        if std::path::Path::new("/bin/zsh").exists() {
            return "/bin/zsh".to_string();
        }
        "/bin/bash".to_string()
    }

    #[cfg(target_os = "linux")]
    {
        if std::path::Path::new("/bin/bash").exists() {
            return "/bin/bash".to_string();
        }
        "/bin/sh".to_string()
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(powershell) = std::env::var("COMSPEC") {
            return powershell;
        }
        "powershell.exe".to_string()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "/bin/sh".to_string()
    }
}
