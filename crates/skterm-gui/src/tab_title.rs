//! Building the labels shown on tabs and the window title bar.
//!
//! Tabs and the window title bar have different space budgets and roles:
//! - **Tab Title**: Narrow capsule, so it shows only `{program} · {dir_basename}`
//!   when a foreground program is running, or `{dir_basename}` when idle (or
//!   `{custom_label}` if user-named).
//! - **Window Title**: Full title bar width, so it shows `{custom_label} — {full_cwd}`,
//!   `{osc_title_or_program} — {full_cwd}`, or `{full_cwd}` when idle, without a
//!   redundant `"skTerm — "` prefix.

use crate::layout::ProcessTreeCache;

/// Shell names that are not worth naming in a tab or window title.
const SHELL_NAMES: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh",
];

/// Format an absolute path with `~` for the user's home directory, keeping all
/// remaining path components intact for the window title bar:
///
/// `/Users/eric/Projects/skTerm` -> `~/Projects/skTerm`
pub fn format_full_path(path: &str, home: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }

    match home.map(|h| h.trim_end_matches('/')) {
        Some(h) if !h.is_empty() && trimmed == h => "~".to_string(),
        Some(h)
            if !h.is_empty() && trimmed.starts_with(h) && trimmed.as_bytes()[h.len()] == b'/' =>
        {
            format!("~/{}", &trimmed[h.len() + 1..])
        }
        _ => trimmed.to_string(),
    }
}

/// Extract just the last directory component for compact tab labels, mapping
/// `$HOME` to `~` and root to `/`:
///
/// `/Users/eric/Projects/skTerm` -> `skTerm`
/// `/Users/eric` -> `~`
/// `/` -> `/`
pub fn dir_basename(path: &str, home: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }

    if let Some(h) = home.map(|h| h.trim_end_matches('/')) {
        if !h.is_empty() && trimmed == h {
            return "~".to_string();
        }
    }

    std::path::Path::new(trimmed)
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("/")
        .to_string()
}

/// Shorten an absolute path for display by abbreviating intermediate components.
///
/// `/Users/eric/Projects/skTerm` -> `~/P/skTerm`
#[allow(dead_code)]
pub fn shorten_path(path: &str, home: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }

    let (prefix, rest) = match home.map(|h| h.trim_end_matches('/')) {
        Some(h) if !h.is_empty() && trimmed == h => return "~".to_string(),
        Some(h)
            if !h.is_empty() && trimmed.starts_with(h) && trimmed.as_bytes()[h.len()] == b'/' =>
        {
            ("~", &trimmed[h.len() + 1..])
        }
        _ => ("", trimmed.trim_start_matches('/')),
    };

    let components: Vec<&str> = rest.split('/').filter(|c| !c.is_empty()).collect();
    if components.is_empty() {
        return if prefix.is_empty() {
            "/".into()
        } else {
            prefix.into()
        };
    }

    let mut out = String::new();
    if prefix.is_empty() {
        out.push('/');
    } else {
        out.push_str(prefix);
        out.push('/');
    }

    for (i, comp) in components.iter().enumerate() {
        if i > 0 {
            out.push('/');
        }
        if i == components.len() - 1 {
            out.push_str(comp);
        } else {
            let mut chars = comp.chars();
            match chars.next() {
                Some('.') => {
                    out.push('.');
                    if let Some(c) = chars.next() {
                        out.push(c);
                    }
                }
                Some(c) => out.push(c),
                None => {}
            }
        }
    }
    out
}

/// True if `name` looks like an interactive shell rather than a running program.
pub fn is_shell(name: &str, configured_shell: Option<&str>) -> bool {
    let base = std::path::Path::new(name.trim_start_matches('-'))
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(name.trim_start_matches('-'));
    if SHELL_NAMES.contains(&base) {
        return true;
    }
    configured_shell
        .and_then(|s| std::path::Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == base)
}

/// Map a canonical agent brand name (from `ProcessTreeCache`) to its concise CLI name.
pub fn canonical_agent_to_short_name(agent_brand: &str) -> &'static str {
    match agent_brand {
        "Jetski" => "jetski",
        "Antigravity CLI" => "agy",
        "Claude Code" => "claude",
        "Gemini CLI" => "gemini",
        "Codex CLI" => "codex",
        "Aider" => "aider",
        _ => "agent",
    }
}

/// Resolve the effective foreground program name, normalizing AI CLI wrappers
/// (which often appear as `node`, `python`, or `bash` at the OS process level)
/// to their concise CLI names (`claude`, `gemini`, `agy`, `jetski`, `codex`, `aider`).
///
/// Returns `None` when the terminal is sitting idle at a shell prompt.
pub fn resolve_program_name(
    foreground_process: Option<&str>,
    foreground_cmdline: Option<&str>,
    descendant_agent: Option<&str>,
    osc_title: &str,
    configured_shell: Option<&str>,
) -> Option<String> {
    // 1. Check if the foreground command line or process name matches a known AI CLI.
    if let Some(cmd) = foreground_cmdline {
        if let Some(brand) = ProcessTreeCache::match_agent_command(cmd) {
            return Some(canonical_agent_to_short_name(brand).to_string());
        }
    }
    if let Some(proc_name) = foreground_process {
        if let Some(brand) = ProcessTreeCache::match_agent_command(proc_name) {
            return Some(canonical_agent_to_short_name(brand).to_string());
        }
    }
    if let Some(brand) = descendant_agent {
        return Some(canonical_agent_to_short_name(brand).to_string());
    }

    let proc_name = foreground_process?;
    if is_shell(proc_name, configured_shell) {
        return None;
    }

    // 2. If the foreground process is a generic runtime (like `node` or `python3`),
    // check whether the OSC title identifies a known AI CLI.
    let base = proc_name.trim_start_matches('-');
    if matches!(
        base,
        "node" | "nodejs" | "bun" | "deno" | "python" | "python3" | "cli" | "cli_internal"
    ) || base.starts_with("python3.")
    {
        let title_lower = osc_title.to_lowercase();
        if title_lower.contains("jetski") {
            return Some("jetski".to_string());
        } else if title_lower.contains("antigravity") || title_lower.contains("agy") {
            return Some("agy".to_string());
        } else if title_lower.contains("claude") {
            return Some("claude".to_string());
        } else if title_lower.contains("gemini") {
            return Some("gemini".to_string());
        } else if title_lower.contains("codex") {
            return Some("codex".to_string());
        } else if title_lower.contains("aider") {
            return Some("aider".to_string());
        }
    }

    Some(base.to_string())
}

/// Filter out empty, default, or shell-prompt-noise OSC 0/2 titles so they do
/// not clobber cleaner process/directory formatting in the window title.
fn clean_osc_title<'a>(
    osc_title: &'a str,
    program: Option<&str>,
    cwd: Option<&str>,
    home: Option<&str>,
    configured_shell: Option<&str>,
) -> Option<&'a str> {
    let t = osc_title.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("skterm") {
        return None;
    }
    if is_shell(t, configured_shell) {
        return None;
    }
    if let Some(c) = cwd {
        let full = format_full_path(c, home);
        let base = dir_basename(c, home);
        let short = shorten_path(c, home);
        if t == c || t == full || t == base || t == short {
            return None;
        }
        // Ignore stock shell prompt titles like `user@host: ~/Projects/skTerm`
        // unless the foreground program is actually `ssh`.
        if program != Some("ssh") && t.contains('@') {
            if let Some((_, rhs)) = t.split_once(':') {
                let rhs = rhs.trim();
                if rhs == c
                    || rhs == full
                    || rhs == base
                    || rhs == short
                    || is_shell(rhs, configured_shell)
                {
                    return None;
                }
            }
        }
    }
    Some(t)
}

/// Build the compact text for a tab capsule and macOS native tab.
///
/// Priority:
/// 1. `custom_label` (if set by the user): `{custom_label}`
/// 2. Foreground program + CWD: `{program} · {dir_basename}` (e.g. `nvim · skTerm`)
/// 3. Foreground program without CWD: `{program}`
/// 4. Idle shell + CWD: `{dir_basename}` (e.g. `skTerm` or `~`)
/// 5. Fallback: meaningful OSC title, or `fallback_title` (`Tab N` / `skTerm`).
#[allow(clippy::too_many_arguments)]
pub fn build_tab_label(
    custom_label: Option<&str>,
    foreground_process: Option<&str>,
    foreground_cmdline: Option<&str>,
    descendant_agent: Option<&str>,
    cwd: Option<&str>,
    home: Option<&str>,
    configured_shell: Option<&str>,
    osc_title: &str,
    fallback_title: &str,
) -> String {
    if let Some(custom) = custom_label.map(str::trim).filter(|s| !s.is_empty()) {
        return custom.to_string();
    }

    let dir_name = cwd.map(|c| dir_basename(c, home));
    let program = resolve_program_name(
        foreground_process,
        foreground_cmdline,
        descendant_agent,
        osc_title,
        configured_shell,
    );

    match (program, dir_name) {
        (Some(p), Some(dir)) => format!("{} · {}", p, dir),
        (Some(p), None) => p,
        (None, Some(dir)) => dir,
        (None, None) => {
            if let Some(osc) = clean_osc_title(osc_title, None, cwd, home, configured_shell) {
                osc.to_string()
            } else {
                let fb = fallback_title.trim();
                if fb.is_empty() {
                    "skTerm".to_string()
                } else {
                    fb.to_string()
                }
            }
        }
    }
}

/// Build the full-context text for the window title bar.
///
/// Priority:
/// 1. `custom_label` (if set): `{custom_label} — {full_cwd}` (or `{custom_label}`)
/// 2. Foreground program running:
///    - Uses meaningful OSC 0/2 title if present, otherwise `{program}`.
///    - Appends ` — {full_cwd}` (unless the OSC title already contains the path).
/// 3. Idle shell:
///    - `{full_cwd}` (e.g. `~/Projects/skTerm` or `~`).
/// 4. Fallback: meaningful OSC title, or `"skTerm"`.
#[allow(clippy::too_many_arguments)]
pub fn build_window_title(
    custom_label: Option<&str>,
    foreground_process: Option<&str>,
    foreground_cmdline: Option<&str>,
    descendant_agent: Option<&str>,
    cwd: Option<&str>,
    home: Option<&str>,
    configured_shell: Option<&str>,
    osc_title: &str,
) -> String {
    let full_dir = cwd.map(|c| format_full_path(c, home));

    if let Some(custom) = custom_label.map(str::trim).filter(|s| !s.is_empty()) {
        return match full_dir {
            Some(dir) => format!("{} — {}", custom, dir),
            None => custom.to_string(),
        };
    }

    let program = resolve_program_name(
        foreground_process,
        foreground_cmdline,
        descendant_agent,
        osc_title,
        configured_shell,
    );

    if let Some(prog) = program {
        let osc = clean_osc_title(osc_title, Some(&prog), cwd, home, configured_shell);
        let descriptor = osc.unwrap_or(&prog);
        return match full_dir {
            Some(dir) => {
                if descriptor.contains(&dir) || cwd.is_some_and(|c| descriptor.contains(c)) {
                    descriptor.to_string()
                } else {
                    format!("{} — {}", descriptor, dir)
                }
            }
            None => descriptor.to_string(),
        };
    }

    if let Some(dir) = full_dir {
        return dir;
    }

    if let Some(osc) = clean_osc_title(osc_title, None, cwd, home, configured_shell) {
        return osc.to_string();
    }

    "skTerm".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: Option<&str> = Some("/Users/eric");

    #[test]
    fn dir_basename_extracts_last_component_or_tilde() {
        assert_eq!(dir_basename("/Users/eric/Projects/skTerm", HOME), "skTerm");
        assert_eq!(dir_basename("/Users/eric", HOME), "~");
        assert_eq!(dir_basename("/Users/eric/", HOME), "~");
        assert_eq!(dir_basename("/", HOME), "/");
        assert_eq!(dir_basename("", HOME), "/");
        assert_eq!(dir_basename("/tmp", None), "tmp");
    }

    #[test]
    fn format_full_path_replaces_home_on_boundary_only() {
        assert_eq!(
            format_full_path("/Users/eric/Projects/skTerm", HOME),
            "~/Projects/skTerm"
        );
        assert_eq!(format_full_path("/Users/eric", HOME), "~");
        assert_eq!(format_full_path("/Users/eric/", HOME), "~");
        assert_eq!(format_full_path("/Users/eric2/x", HOME), "/Users/eric2/x");
        assert_eq!(format_full_path("/usr/local/bin", HOME), "/usr/local/bin");
        assert_eq!(format_full_path("/", HOME), "/");
    }

    #[test]
    fn abbreviates_intermediate_components_only() {
        assert_eq!(
            shorten_path("/Users/eric/Projects/skTerm", HOME),
            "~/P/skTerm"
        );
        assert_eq!(shorten_path("/usr/local/bin", None), "/u/l/bin");
    }

    #[test]
    fn tab_label_shows_program_and_basename() {
        assert_eq!(
            build_tab_label(
                None,
                Some("nvim"),
                None,
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "",
                "Tab 1"
            ),
            "nvim · skTerm"
        );
    }

    #[test]
    fn tab_label_idle_shell_shows_only_basename() {
        for shell in ["zsh", "-zsh", "bash", "fish"] {
            assert_eq!(
                build_tab_label(
                    None,
                    Some(shell),
                    None,
                    None,
                    Some("/Users/eric/Projects/skTerm"),
                    HOME,
                    Some("/bin/zsh"),
                    "",
                    "Tab 1"
                ),
                "skTerm"
            );
        }
        assert_eq!(
            build_tab_label(
                None,
                Some("zsh"),
                None,
                None,
                Some("/Users/eric"),
                HOME,
                Some("/bin/zsh"),
                "",
                "Tab 1"
            ),
            "~"
        );
    }

    #[test]
    fn tab_label_normalizes_ai_cli_wrappers() {
        assert_eq!(
            build_tab_label(
                None,
                Some("node"),
                Some("node /opt/homebrew/lib/node_modules/@anthropic-ai/claude-code/cli.js"),
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "✳ Refactoring",
                "Tab 1"
            ),
            "claude · skTerm"
        );
        assert_eq!(
            build_tab_label(
                None,
                Some("node"),
                Some("node /usr/local/bin/gemini"),
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "",
                "Tab 1"
            ),
            "gemini · skTerm"
        );
    }

    #[test]
    fn custom_label_takes_priority_in_tab_and_window() {
        assert_eq!(
            build_tab_label(
                Some("Auth Refactor"),
                Some("nvim"),
                None,
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "main.rs",
                "Tab 1"
            ),
            "Auth Refactor"
        );
        assert_eq!(
            build_window_title(
                Some("Auth Refactor"),
                Some("nvim"),
                None,
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "main.rs"
            ),
            "Auth Refactor — ~/Projects/skTerm"
        );
    }

    #[test]
    fn window_title_shows_full_path_when_idle_and_osc_or_program_when_busy() {
        // Idle shell -> full path with ~, no "skTerm — " prefix
        assert_eq!(
            build_window_title(
                None,
                Some("zsh"),
                None,
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "skTerm"
            ),
            "~/Projects/skTerm"
        );

        // Foreground program without OSC title -> "{program} — {full_path}"
        assert_eq!(
            build_window_title(
                None,
                Some("cargo"),
                None,
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "skTerm"
            ),
            "cargo — ~/Projects/skTerm"
        );

        // Foreground program with meaningful OSC title -> "{osc_title} — {full_path}"
        assert_eq!(
            build_window_title(
                None,
                Some("node"),
                Some("node /path/@anthropic-ai/claude-code/cli.js"),
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "✳ Updating tab titles"
            ),
            "✳ Updating tab titles — ~/Projects/skTerm"
        );

        // Foreground program with leftover shell prompt OSC title -> ignores noise, uses program
        assert_eq!(
            build_window_title(
                None,
                Some("cargo"),
                None,
                None,
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                "eric@macbook: ~/Projects/skTerm"
            ),
            "cargo — ~/Projects/skTerm"
        );
    }

    #[test]
    fn falls_back_to_osc_title_then_default() {
        assert_eq!(
            build_tab_label(
                None,
                None,
                None,
                None,
                None,
                HOME,
                None,
                "build logs",
                "Tab 1"
            ),
            "build logs"
        );
        assert_eq!(
            build_tab_label(None, None, None, None, None, HOME, None, "", "Tab 1"),
            "Tab 1"
        );
        assert_eq!(
            build_window_title(None, None, None, None, None, HOME, None, ""),
            "skTerm"
        );
    }
}
