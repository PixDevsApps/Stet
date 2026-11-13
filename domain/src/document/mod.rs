//! Documents: their names and titles, how paths are shown, and what is detected from their
//! text (indentation, language) or named on the command line (a line and column).

pub mod indent;
pub mod language;
pub mod location;
mod untitled;

use std::path::{Path, PathBuf};

pub use untitled::{
    FIRST_LINE_CHARS, FIRST_LINE_READ, FIRST_LINE_SCAN, extension_from_globs, first_line_name,
    next_untitled_number, save_file_name, untitled_name,
};

/// The application's name, as window titles show it.
pub const APP_NAME: &str = "Stet";

/// The tab label: the name, with a dot in front while there are unsaved changes.
pub fn tab_title(name: &str, dirty: bool) -> String {
    if dirty {
        format!("● {name}")
    } else {
        name.to_owned()
    }
}

/// `file — dir — Stet`, or `Untitled 1 — Stet`; a `*` in front marks unsaved changes.
pub fn window_title(name: &str, dir: Option<&str>, dirty: bool) -> String {
    let marker = if dirty { "*" } else { "" };
    match dir {
        Some(dir) => format!("{marker}{name} — {dir} — {APP_NAME}"),
        None => format!("{marker}{name} — {APP_NAME}"),
    }
}

/// `path` with a leading `home` shown as `~`.
pub fn tilde(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home.filter(|home| !home.as_os_str().is_empty() && home != &Path::new("/"))
        && let Ok(rest) = path.strip_prefix(home)
    {
        return if rest.as_os_str().is_empty() {
            "~".to_owned()
        } else {
            format!("~/{}", rest.display())
        };
    }
    path.display().to_string()
}

/// `text` as a path, where `~` and `~/…` name `home`; unchanged without one.
pub fn expand_home(text: &str, home: Option<&Path>) -> PathBuf {
    match (text.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_path_buf(),
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(text),
    }
}

/// The command that edits `path` as root with Stet (INTEGRATIONS.md), for the read-only
/// banner: `SUDO_EDITOR="stet --wait" sudoedit <path>`, the path quoted for a POSIX shell.
pub fn sudoedit_command(path: &str) -> String {
    format!("SUDO_EDITOR=\"stet --wait\" sudoedit {}", shell_quote(path))
}

/// `text` as one word of a POSIX shell command line: unchanged when it only has characters
/// the shell takes literally, otherwise in single quotes.
pub fn shell_quote(text: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-+,:@%=".contains(c);
    if !text.is_empty() && text.chars().all(plain) {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sudoedit_quotes_the_path_for_the_shell() {
        assert_eq!(
            sudoedit_command("/etc/hosts"),
            "SUDO_EDITOR=\"stet --wait\" sudoedit /etc/hosts"
        );
        assert_eq!(shell_quote("/etc/my file"), "'/etc/my file'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
        assert_eq!(shell_quote("$HOME/x"), "'$HOME/x'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn titles() {
        assert_eq!(tab_title("main.rs", false), "main.rs");
        assert_eq!(tab_title("main.rs", true), "● main.rs");
        assert_eq!(
            window_title("main.rs", Some("~/src"), false),
            "main.rs — ~/src — Stet"
        );
        assert_eq!(
            window_title("main.rs", Some("~/src"), true),
            "*main.rs — ~/src — Stet"
        );
        assert_eq!(window_title("Untitled 1", None, true), "*Untitled 1 — Stet");
    }

    #[test]
    fn home_becomes_a_tilde() {
        let home = Path::new("/home/user");
        assert_eq!(tilde(Path::new("/home/user/src"), Some(home)), "~/src");
        assert_eq!(tilde(Path::new("/home/user"), Some(home)), "~");
        assert_eq!(
            tilde(Path::new("/home/username"), Some(home)),
            "/home/username"
        );
        assert_eq!(tilde(Path::new("/etc"), Some(home)), "/etc");
        assert_eq!(tilde(Path::new("/etc"), Some(Path::new("/"))), "/etc");
        assert_eq!(tilde(Path::new("/etc"), None), "/etc");
    }

    #[test]
    fn a_tilde_names_home() {
        let home = Some(Path::new("/home/user"));
        assert_eq!(expand_home("~", home), Path::new("/home/user"));
        assert_eq!(expand_home("~/src/x", home), Path::new("/home/user/src/x"));
        assert_eq!(expand_home("~/", home), Path::new("/home/user/"));
        assert_eq!(expand_home("~other/src", home), Path::new("~other/src"));
        assert_eq!(expand_home("/etc/~", home), Path::new("/etc/~"));
        assert_eq!(expand_home("src", home), Path::new("src"));
        assert_eq!(expand_home("~/src", None), Path::new("~/src"));
        assert_eq!(expand_home("~", None), Path::new("~"));
    }
}
