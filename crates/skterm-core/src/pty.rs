//! Cross-platform PTY subprocess management using portable-pty.

use crossbeam_channel::{bounded, Receiver, Sender};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
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
    /// Rate-limits the `waitpid` backstop in [`PtyProcess::is_alive`].
    last_liveness_probe: Mutex<Instant>,
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

        // On Windows: if spawning cmd.exe, ensure PROMPT ends with space ($S)
        // so the cursor is separated from the prompt character '>' by an extra space.
        #[cfg(target_os = "windows")]
        {
            let lower_shell = shell_path.to_lowercase();
            let is_cmd = lower_shell.ends_with("cmd.exe")
                || lower_shell.ends_with("cmd")
                || lower_shell == "cmd.exe"
                || lower_shell == "cmd";
            if is_cmd {
                let current_prompt = std::env::var("PROMPT").unwrap_or_else(|_| "$P$G".to_string());
                if !current_prompt.ends_with("$S") && !current_prompt.ends_with(' ') {
                    cmd.env("PROMPT", format!("{}$S", current_prompt));
                }
            }
        }

        let child = pair.slave.spawn_command(cmd)?;
        let pid = child.process_id();
        let child = Arc::new(Mutex::new(child));
        let child_clone = Arc::clone(&child);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));

        // Bounded, not unbounded: a producer that outruns the ~60 Hz consumer (`yes`,
        // `cat /dev/urandom`, a runaway build log) would otherwise grow this queue
        // without limit and blow up RSS. Blocking the reader here backpressures through
        // the pty to the child, which is the correct behaviour for a terminal.
        const EVENT_QUEUE_DEPTH: usize = 256;
        let (event_tx, event_rx): (Sender<PtyEvent>, Receiver<PtyEvent>) =
            bounded(EVENT_QUEUE_DEPTH);

        let wakeup_cb = options.wakeup_cb;
        let has_exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let has_exited_thread = has_exited.clone();

        let reader_thread = thread::Builder::new()
            .name("skterm-pty-reader".to_string())
            .spawn(move || {
                // 64 KiB rather than 8 KiB: under heavy output this cuts the number of
                // read syscalls, channel sends and per-chunk allocations by 8x.
                let mut buf = [0u8; 65536];
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
            last_liveness_probe: Mutex::new(Instant::now()),
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
    ///
    /// The reader thread is the primary exit signal (it sets `has_exited` on EOF or
    /// read error). The `try_wait` below is only a backstop for a child that exits
    /// while some other process still holds the pty open, so it is rate-limited:
    /// callers poll this per pane per frame, and an unthrottled `waitpid` there costs
    /// a syscall every time.
    pub fn is_alive(&self) -> bool {
        const LIVENESS_PROBE_INTERVAL: Duration = Duration::from_millis(500);

        if self.has_exited.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }

        match self.last_liveness_probe.lock() {
            Ok(mut last) => {
                if last.elapsed() < LIVENESS_PROBE_INTERVAL {
                    // Assume still alive; `has_exited` above would have caught a
                    // reader-observed exit already.
                    return true;
                }
                *last = Instant::now();
            }
            Err(_) => return true,
        }

        if let Ok(mut child) = self.child.lock() {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.has_exited
                        .store(true, std::sync::atomic::Ordering::Relaxed);
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

    /// Name of the process currently in the foreground of this pty.
    ///
    /// This is the terminal's own notion of "what is running right now" -- the
    /// kernel tracks it as the pty's foreground process group, so running `vim`
    /// reports `vim` and quitting it reports the shell again, with no process
    /// tree walking.
    ///
    /// Two cheap syscalls (`tcgetpgrp` plus a name lookup); no subprocess. Still
    /// worth throttling if called from a render loop.
    #[cfg(unix)]
    pub fn foreground_process_name(&self) -> Option<String> {
        let pgid = self.master.process_group_leader()?;
        if pgid <= 0 {
            return None;
        }
        let name = process_name(pgid)?;
        // portable-pty hands back the child's pid as soon as it forks, which is
        // before the child has exec'd the shell. For that window (~12ms on every
        // spawn) the child is still a copy of us and reports *our* name, which
        // would label a brand new tab with the application's own name.
        //
        // Suppressing the match costs only the rare case of running this
        // terminal inside itself, which then falls back to showing the
        // directory -- exactly what an idle shell already shows.
        if self_process_name() == Some(name.as_str()) {
            return None;
        }
        Some(name)
    }

    #[cfg(not(unix))]
    pub fn foreground_process_name(&self) -> Option<String> {
        None
    }
}

/// This process's own name, resolved once.
///
/// Used to recognise a forked-but-not-yet-exec'd child, which still carries our
/// name. Looked up through the same path as any other pid so the two results are
/// directly comparable.
#[cfg(unix)]
fn self_process_name() -> Option<&'static str> {
    use std::sync::OnceLock;
    static SELF_NAME: OnceLock<Option<String>> = OnceLock::new();
    SELF_NAME
        .get_or_init(|| process_name(unsafe { libc::getpid() }))
        .as_deref()
}

/// The current working directory of `pid`, via a single syscall.
///
/// The alternative is shelling out to `lsof`. That matters here because the tab
/// label asks for the cwd on a timer, and macOS's stock `zshrc` only emits the
/// OSC 7 that would make this unnecessary when `TERM_PROGRAM` is
/// `Apple_Terminal` -- so for us this is the normal path, not a rare fallback.
#[cfg(target_os = "macos")]
pub fn process_cwd(pid: libc::pid_t) -> Option<String> {
    let mut info: libc::proc_vnodepathinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_vnodepathinfo>() as libc::c_int;
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDVNODEPATHINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    // A short write means the kernel did not populate the struct, e.g. the
    // process is gone or we are not allowed to look at it.
    if written < size {
        return None;
    }
    // libc declares this as `[[c_char; 32]; 32]` rather than `[c_char; MAXPATHLEN]`
    // to work around an old-rustc limitation, so flatten it back out.
    let bytes: Vec<u8> = info
        .pvi_cdir
        .vip_path
        .iter()
        .flatten()
        .take_while(|&&c| c != 0)
        .map(|&c| c as u8)
        .collect();
    if bytes.is_empty() {
        return None;
    }
    String::from_utf8(bytes).ok()
}

/// Look up a process name by pid.
#[cfg(target_os = "macos")]
fn process_name(pid: libc::pid_t) -> Option<String> {
    // PROC_PIDPATHINFO_MAXSIZE is 4 * MAXPATHLEN; `proc_name` only ever writes a
    // short basename, but oversizing the buffer costs nothing on the stack.
    let mut buf = [0u8; 256];
    let len =
        unsafe { libc::proc_name(pid, buf.as_mut_ptr() as *mut libc::c_void, buf.len() as u32) };
    if len <= 0 {
        return None;
    }
    let name = String::from_utf8_lossy(&buf[..len as usize]).into_owned();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Look up a process name by pid.
#[cfg(all(unix, not(target_os = "macos")))]
fn process_name(pid: libc::pid_t) -> Option<String> {
    let comm = std::fs::read_to_string(format!("/proc/{}/comm", pid)).ok()?;
    let name = comm.trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
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
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if !comspec.is_empty() {
                return comspec;
            }
        }
        find_cmd()
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        "/bin/sh".to_string()
    }
}

/// Preset information for installed or supported shells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellPresetInfo {
    pub id: String,
    pub name: String,
    pub program: String,
    pub args: Vec<String>,
    pub is_available: bool,
    pub description: String,
}

/// Find installed PowerShell 7 / Core (pwsh.exe) path on Windows.
pub fn find_pwsh_path() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        // 1. Program Files
        if let Ok(pf) = std::env::var("ProgramFiles") {
            let p = std::path::PathBuf::from(&pf)
                .join("PowerShell")
                .join("7")
                .join("pwsh.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
        // 2. Program Files (x86)
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            let p = std::path::PathBuf::from(&pf86)
                .join("PowerShell")
                .join("7")
                .join("pwsh.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
        // 3. LocalAppData \ Programs \ PowerShell \ 7 \ pwsh.exe
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = std::path::PathBuf::from(&local)
                .join("Programs")
                .join("PowerShell")
                .join("7")
                .join("pwsh.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
            let p_ms = std::path::PathBuf::from(&local)
                .join("Microsoft")
                .join("WindowsApps")
                .join("pwsh.exe");
            if p_ms.exists() {
                return Some(p_ms.to_string_lossy().to_string());
            }
        }
        // 4. PATH search
        if let Ok(path) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path) {
                let p = dir.join("pwsh.exe");
                if p.exists() {
                    return Some(p.to_string_lossy().to_string());
                }
            }
        }
    }
    None
}

/// Find built-in Windows PowerShell (powershell.exe) path on Windows.
pub fn find_windows_powershell() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(sysroot) = std::env::var("SystemRoot") {
            let p = std::path::PathBuf::from(sysroot)
                .join("System32")
                .join("WindowsPowerShell")
                .join("v1.0")
                .join("powershell.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
        Some("powershell.exe".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        None
    }
}

/// Find Windows Command Prompt (cmd.exe) path on Windows.
pub fn find_cmd() -> String {
    #[cfg(target_os = "windows")]
    {
        if let Ok(comspec) = std::env::var("COMSPEC") {
            if !comspec.is_empty() {
                return comspec;
            }
        }
    }
    "cmd.exe".to_string()
}

/// Find Git Bash on Windows.
pub fn find_git_bash_path() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(pf) = std::env::var("ProgramFiles") {
            let p = std::path::PathBuf::from(&pf)
                .join("Git")
                .join("bin")
                .join("bash.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
        if let Ok(pf86) = std::env::var("ProgramFiles(x86)") {
            let p = std::path::PathBuf::from(&pf86)
                .join("Git")
                .join("bin")
                .join("bash.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            let p = std::path::PathBuf::from(&local)
                .join("Programs")
                .join("Git")
                .join("bin")
                .join("bash.exe");
            if p.exists() {
                return Some(p.to_string_lossy().to_string());
            }
        }
    }
    None
}

/// Query system for available shell presets.
pub fn available_shell_presets() -> Vec<ShellPresetInfo> {
    #[cfg(target_os = "windows")]
    {
        let mut presets = Vec::new();

        // 1. Command Prompt (cmd.exe)
        let cmd_path = find_cmd();
        presets.push(ShellPresetInfo {
            id: "cmd".to_string(),
            name: "命令提示字元 (cmd.exe)".to_string(),
            program: cmd_path,
            args: Vec::new(),
            is_available: true,
            description: "Windows 系統預設命令提示字元 (cmd.exe)".to_string(),
        });

        // 2. Windows PowerShell (powershell.exe)
        let ps_path = find_windows_powershell().unwrap_or_else(|| "powershell.exe".to_string());
        presets.push(ShellPresetInfo {
            id: "powershell".to_string(),
            name: "Windows PowerShell (powershell.exe)".to_string(),
            program: ps_path,
            args: vec!["-NoLogo".to_string()],
            is_available: true,
            description: "Windows 內建 PowerShell 5.1 (powershell.exe)".to_string(),
        });

        // 3. PowerShell 7+ (pwsh.exe)
        let pwsh_found = find_pwsh_path();
        let (pwsh_prog, pwsh_avail) = if let Some(path) = pwsh_found {
            (path, true)
        } else {
            ("pwsh.exe".to_string(), false)
        };
        let pwsh_desc = if pwsh_avail {
            "自訂安裝的最新 PowerShell 7+ (已偵測到)".to_string()
        } else {
            "自訂安裝的最新 PowerShell 7+ (未偵測到，需自行安裝)".to_string()
        };
        presets.push(ShellPresetInfo {
            id: "pwsh".to_string(),
            name: if pwsh_avail {
                "PowerShell 7+ (pwsh.exe - 已偵測)".to_string()
            } else {
                "PowerShell 7+ (pwsh.exe)".to_string()
            },
            program: pwsh_prog,
            args: vec!["-NoLogo".to_string()],
            is_available: pwsh_avail,
            description: pwsh_desc,
        });

        // 4. Git Bash (bash.exe)
        if let Some(git_bash) = find_git_bash_path() {
            presets.push(ShellPresetInfo {
                id: "git_bash".to_string(),
                name: "Git Bash (bash.exe)".to_string(),
                program: git_bash,
                args: vec!["--login".to_string(), "-i".to_string()],
                is_available: true,
                description: "Git for Windows 隨附 Bash 終端".to_string(),
            });
        }

        presets
    }

    #[cfg(target_os = "macos")]
    {
        let mut presets = Vec::new();
        if std::path::Path::new("/bin/zsh").exists() {
            presets.push(ShellPresetInfo {
                id: "zsh".to_string(),
                name: "Zsh (/bin/zsh)".to_string(),
                program: "/bin/zsh".to_string(),
                args: vec!["-l".to_string()],
                is_available: true,
                description: "macOS 系統預設 Zsh".to_string(),
            });
        }
        if std::path::Path::new("/bin/bash").exists() {
            presets.push(ShellPresetInfo {
                id: "bash".to_string(),
                name: "Bash (/bin/bash)".to_string(),
                program: "/bin/bash".to_string(),
                args: vec!["-l".to_string()],
                is_available: true,
                description: "系統內建 GNU Bash".to_string(),
            });
        }
        for candidate in [
            "/opt/homebrew/bin/fish",
            "/usr/local/bin/fish",
            "/opt/homebrew/bin/zsh",
            "/opt/homebrew/bin/bash",
        ] {
            if std::path::Path::new(candidate).exists() {
                let name = candidate.rsplit('/').next().unwrap_or("shell");
                presets.push(ShellPresetInfo {
                    id: name.to_string(),
                    name: format!("{} ({})", name, candidate),
                    program: candidate.to_string(),
                    args: vec!["-l".to_string()],
                    is_available: true,
                    description: "Homebrew 套件管理安裝之 Shell".to_string(),
                });
            }
        }
        presets
    }

    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        let mut presets = Vec::new();
        let mut seen_canonical = std::collections::HashSet::new();
        let candidates: &[(&str, &str, &str, &[&str], &str)] = &[
            ("bash", "GNU Bash", "/bin/bash", &["-l"], "Linux 標準 Bash Shell"),
            ("bash", "GNU Bash", "/usr/bin/bash", &["-l"], "Linux 標準 Bash Shell"),
            ("zsh", "Zsh", "/bin/zsh", &["-l"], "Z Shell 互動式終端"),
            ("zsh", "Zsh", "/usr/bin/zsh", &["-l"], "Z Shell 互動式終端"),
            ("fish", "Fish Shell", "/usr/bin/fish", &["-l"], "Friendly Interactive Shell"),
            ("fish", "Fish Shell", "/bin/fish", &["-l"], "Friendly Interactive Shell"),
            ("nu", "Nushell", "/usr/bin/nu", &["-l"], "現代結構化資料 Shell"),
            ("sh", "POSIX sh", "/bin/sh", &[], "系統輕量 POSIX Shell"),
        ];
        for &(id, title, candidate, default_args, desc) in candidates {
            let p = std::path::Path::new(candidate);
            if p.exists() {
                let canon = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
                // Deduplicate /bin/bash and /usr/bin/bash when /bin is a symlink to /usr/bin,
                // unless it's a distinct shell binary (like /bin/sh pointing to dash/bash).
                let dedup_key = (id.to_string(), canon);
                if seen_canonical.insert(dedup_key) {
                    presets.push(ShellPresetInfo {
                        id: id.to_string(),
                        name: format!("{} ({})", title, candidate),
                        program: candidate.to_string(),
                        args: default_args.iter().map(|s| (*s).to_string()).collect(),
                        is_available: true,
                        description: desc.to_string(),
                    });
                }
            }
        }
        presets
    }
}
