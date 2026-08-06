//! Cross-platform PTY subprocess management using portable-pty.

use crossbeam_channel::{unbounded, Receiver, Sender};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
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

/// A running PTY process with asynchronous reader and synchronized writer.
pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    event_rx: Receiver<PtyEvent>,
    _reader_thread: thread::JoinHandle<()>,
}

impl PtyProcess {
    /// Spawn a child shell with the given initial dimensions and optional custom command.
    pub fn spawn(
        rows: u16,
        cols: u16,
        custom_shell: Option<String>,
        cwd: Option<String>,
    ) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };

        let pair = pty_system.openpty(size)?;
        let shell_path = custom_shell.unwrap_or_else(default_shell);

        let mut cmd = CommandBuilder::new(shell_path);
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }

        // Set standard terminal environment variables
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

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

    /// Write bytes (e.g. keyboard input) directly to child process stdin.
    pub fn write_bytes(&self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut writer = self.writer.lock().map_err(|e| anyhow::anyhow!("Writer lock poisoned: {}", e))?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    /// Resize the virtual terminal dimensions.
    pub fn resize(&mut self, rows: u16, cols: u16) -> anyhow::Result<()> {
        self.master.resize(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        Ok(())
    }

    /// Get receiver for PTY output events.
    pub fn receiver(&self) -> &Receiver<PtyEvent> {
        &self.event_rx
    }

    /// Check if the child process has exited.
    pub fn is_alive(&self) -> bool {
        if let Ok(mut child) = self.child.lock() {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Terminate the child process.
    pub fn kill(&mut self) -> anyhow::Result<()> {
        if let Ok(mut child) = self.child.lock() {
            child.kill()?;
        }
        Ok(())
    }
}

/// Detect the default shell based on the host platform and environment variables.
pub fn default_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| {
            if cfg!(target_os = "macos") {
                "/bin/zsh".to_string()
            } else {
                "/bin/bash".to_string()
            }
        })
    }
}
