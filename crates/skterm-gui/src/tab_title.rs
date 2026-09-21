//! Building the label shown on a macOS native window tab.
//!
//! Native tabs are narrow, so the label has to identify a session in very few
//! characters. These are pure string helpers, kept separate from the window
//! plumbing so the formatting rules can be unit tested.

/// Shell names that are not worth naming in a tab label.
///
/// When the foreground process is just the shell, the interesting information is
/// the directory, so the label drops the process name entirely.
const SHELL_NAMES: &[&str] = &[
    "sh", "bash", "zsh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "elvish", "xonsh",
];

/// Shorten an absolute path for display.
///
/// Rewrites the home directory as `~` and abbreviates every component except the
/// last to its first character, which is what keeps a deep path readable in the
/// handful of characters a native tab actually gets:
///
/// `/Users/eric/Projects/skTerm` -> `~/P/skTerm`
pub fn shorten_path(path: &str, home: Option<&str>) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return "/".to_string();
    }

    // Only treat home as a prefix on a component boundary, so `/Users/eric2` is
    // not mistaken for something inside `/Users/eric`.
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
            // Keep a leading dot so hidden directories stay recognisable
            // (".config" abbreviates to ".c", not "." which reads as "here").
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
fn is_shell(name: &str, configured_shell: Option<&str>) -> bool {
    let base = name.trim_start_matches('-');
    if SHELL_NAMES.contains(&base) {
        return true;
    }
    configured_shell
        .and_then(|s| std::path::Path::new(s).file_name())
        .and_then(|s| s.to_str())
        .is_some_and(|s| s == base)
}

/// Build the text for a native tab.
///
/// `nvim · ~/P/skTerm` while a program is running, and just `~/P/skTerm` when the
/// shell is idle -- naming the shell on every tab would spend scarce width on the
/// one thing that is the same everywhere.
///
/// Falls back to whatever title the shell reported (OSC 0/2) when there is no cwd
/// to show, and finally to the app name.
pub fn build_tab_label(
    foreground_process: Option<&str>,
    cwd: Option<&str>,
    home: Option<&str>,
    configured_shell: Option<&str>,
    shell_title: &str,
) -> String {
    let short_cwd = cwd.map(|c| shorten_path(c, home));
    let program = foreground_process.filter(|p| !is_shell(p, configured_shell));

    match (program, short_cwd) {
        (Some(p), Some(dir)) => format!("{} · {}", p, dir),
        (Some(p), None) => p.to_string(),
        (None, Some(dir)) => dir,
        (None, None) => {
            let t = shell_title.trim();
            if t.is_empty() || t == "skTerm" {
                "skTerm".to_string()
            } else {
                t.to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOME: Option<&str> = Some("/Users/eric");

    #[test]
    fn abbreviates_intermediate_components_only() {
        assert_eq!(
            shorten_path("/Users/eric/Projects/skTerm", HOME),
            "~/P/skTerm"
        );
        assert_eq!(shorten_path("/usr/local/bin", None), "/u/l/bin");
    }

    #[test]
    fn home_itself_is_just_tilde() {
        assert_eq!(shorten_path("/Users/eric", HOME), "~");
        assert_eq!(shorten_path("/Users/eric/", HOME), "~");
    }

    #[test]
    fn home_prefix_must_land_on_a_component_boundary() {
        // Would become "~2/x" if the prefix were matched as a bare string.
        assert_eq!(shorten_path("/Users/eric2/x", HOME), "/U/e/x");
    }

    #[test]
    fn single_component_is_left_alone() {
        assert_eq!(shorten_path("/tmp", None), "/tmp");
        assert_eq!(shorten_path("/Users/eric/src", HOME), "~/src");
    }

    #[test]
    fn hidden_directories_keep_a_letter_after_the_dot() {
        // ".c/skterm" stays traceable to ".config"; "./skterm" would not.
        assert_eq!(
            shorten_path("/Users/eric/.config/skterm", HOME),
            "~/.c/skterm"
        );
    }

    #[test]
    fn root_and_empty_are_not_mangled() {
        assert_eq!(shorten_path("/", None), "/");
        assert_eq!(shorten_path("", None), "/");
    }

    #[test]
    fn running_program_is_named_before_the_directory() {
        assert_eq!(
            build_tab_label(
                Some("nvim"),
                Some("/Users/eric/Projects/skTerm"),
                HOME,
                Some("/bin/zsh"),
                ""
            ),
            "nvim · ~/P/skTerm"
        );
    }

    #[test]
    fn idle_shell_shows_only_the_directory() {
        // Every tab would otherwise start with "zsh · ", wasting the width that
        // makes tabs distinguishable in the first place.
        for shell in ["zsh", "-zsh", "bash", "fish"] {
            assert_eq!(
                build_tab_label(
                    Some(shell),
                    Some("/Users/eric/Projects/skTerm"),
                    HOME,
                    Some("/bin/zsh"),
                    ""
                ),
                "~/P/skTerm"
            );
        }
    }

    #[test]
    fn a_configured_nonstandard_shell_is_also_treated_as_idle() {
        assert_eq!(
            build_tab_label(
                Some("myshell"),
                Some("/tmp"),
                None,
                Some("/opt/myshell"),
                ""
            ),
            "/tmp"
        );
    }

    #[test]
    fn falls_back_to_the_shell_reported_title_then_the_app_name() {
        assert_eq!(
            build_tab_label(None, None, HOME, None, "build logs"),
            "build logs"
        );
        assert_eq!(build_tab_label(None, None, HOME, None, ""), "skTerm");
        assert_eq!(build_tab_label(None, None, HOME, None, "skTerm"), "skTerm");
    }
}
