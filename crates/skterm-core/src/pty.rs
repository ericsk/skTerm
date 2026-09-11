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
#[derive(Clone, Default)]
pub struct PtySpawnOptions {
    pub shell: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub term: Option<String>,
    pub env: HashMap<String, String>,
    pub wakeup_cb: Option<Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for PtySpawnOptions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PtySpawnOptions")
            .field("shell", &self.shell)
            .field("args", &self.args)
            .field("cwd", &self.cwd)
            .field("term", &self.term)
            .field("env", &self.env)
            .field("has_wakeup_cb", &self.wakeup_cb.is_some())
            .finish()
    }
}

/// A running PTY process with asynchronous reader and synchronized writer.
pub struct PtyProcess {
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child: Arc<Mutex<Box<dyn Child + Send + Sync>>>,
    event_rx: Receiver<PtyEvent>,
    has_exited: Arc<std::sync::atomic::AtomicBool>,
    pid: Option<u32>,
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

        #[cfg(target_os = "macos")]
        if options.args.is_empty() {
            cmd.arg("-l");
        } else {
            for arg in &options.args {
                cmd.arg(arg);
            }
        }
        #[cfg(not(target_os = "macos"))]
        for arg in &options.args {
            cmd.arg(arg);
        }

        let target_dir = if let Some(dir) = options.cwd {
            // Expand ~ if needed
            if dir.starts_with('~') {
                if let Some(home) =
                    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
                {
                    let mut path = std::path::PathBuf::from(home);
                    if dir.len() > 1 {
                        path.push(dir.trim_start_matches("~/").trim_start_matches("~\\"));
                    }
                    Some(path.to_string_lossy().to_string())
                } else {
                    Some(dir)
                }
            } else {
                Some(dir)
            }
        } else {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|h| h.to_string_lossy().to_string())
        };

        if let Some(dir) = target_dir {
            cmd.cwd(dir);
        }

        // Set terminal environment variables
        let term_name = options.term.unwrap_or_else(|| "xterm-256color".to_string());
        cmd.env("TERM", term_name);
        cmd.env("COLORTERM", "truecolor");

        // Ensure UTF-8 locale environment variables are set if missing
        if std::env::var_os("LANG").is_none() {
            cmd.env("LANG", "en_US.UTF-8");
        }
        if std::env::var_os("LC_ALL").is_none() {
            cmd.env("LC_ALL", "en_US.UTF-8");
        }

        // On macOS, if PATH only has minimal system paths (typical when launched via Finder/LaunchServices),
        // enrich PATH with common tool locations (/usr/local/bin, /opt/homebrew/bin, $HOME/homebrew/bin, $HOME/.cargo/bin)
        #[cfg(target_os = "macos")]
        {
            let current_path = std::env::var("PATH").unwrap_or_default();
            let mut paths: Vec<std::path::PathBuf> = std::env::split_paths(&current_path).collect();
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);

            let candidates = [
                Some(std::path::PathBuf::from("/opt/homebrew/bin")),
                Some(std::path::PathBuf::from("/usr/local/bin")),
                home.as_ref().map(|h| h.join("homebrew/bin")),
                home.as_ref().map(|h| h.join(".cargo/bin")),
                home.as_ref().map(|h| h.join(".local/bin")),
            ];

            let mut modified = false;
            for candidate in candidates.into_iter().flatten() {
                if candidate.is_dir() && !paths.contains(&candidate) {
                    paths.insert(0, candidate);
                    modified = true;
                }
            }

            if modified {
                if let Ok(new_path) = std::env::join_paths(paths) {
                    cmd.env("PATH", new_path);
                }
            }
        }

        // Custom environment variables
        for (k, v) in options.env {
            cmd.env(k, v);
        }

        let child = pair.slave.spawn_command(cmd)?;
        let pid = child.process_id();
        let child = Arc::new(Mutex::new(child));
        let child_clone = Arc::clone(&child);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));

        let (event_tx, event_rx): (Sender<PtyEvent>, Receiver<PtyEvent>) = unbounded();

        let wakeup_cb = options.wakeup_cb;
        let has_exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let has_exited_thread = has_exited.clone();

        let reader_thread = thread::Builder::new()
            .name("skterm-pty-reader".to_string())
            .spawn(move || {
                let mut buf = [0u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => {
                            info!("PTY stream reached EOF");
                            has_exited_thread.store(true, std::sync::atomic::Ordering::SeqCst);
                            let exit_code = child_clone
                                .lock()
                                .ok()
                                .and_then(|mut c| c.try_wait().ok().flatten())
                                .map(|status| status.exit_code());
                            let _ = event_tx.send(PtyEvent::Exit(exit_code));
                            if let Some(ref cb) = wakeup_cb {
                                cb();
                            }
                            break;
                        }
                        Ok(n) => {
                            if event_tx.send(PtyEvent::Output(buf[..n].to_vec())).is_err() {
                                break;
                            }
                            if let Some(ref cb) = wakeup_cb {
                                cb();
                            }
                        }
                        Err(e) => {
                            error!("PTY read error: {:?}", e);
                            has_exited_thread.store(true, std::sync::atomic::Ordering::SeqCst);
                            let exit_code = child_clone
                                .lock()
                                .ok()
                                .and_then(|mut c| c.try_wait().ok().flatten())
                                .map(|status| status.exit_code());
                            let _ = event_tx.send(PtyEvent::Exit(exit_code));
                            if let Some(ref cb) = wakeup_cb {
                                cb();
                            }
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
            has_exited,
            pid,
            _reader_thread: reader_thread,
        })
    }

    /// Write user input bytes into the PTY.
    pub fn write_all(&self, data: &[u8]) -> anyhow::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|e| anyhow::anyhow!("Writer lock poisoned: {:?}", e))?;
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
        if self.has_exited.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.has_exited.store(true, std::sync::atomic::Ordering::Relaxed);
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Return the OS process ID of the spawned shell/child process.
    pub fn process_id(&self) -> Option<u32> {
        self.pid
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
