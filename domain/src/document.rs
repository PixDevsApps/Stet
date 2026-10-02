//! Document and window naming: untitled numbers, tab titles and the window title.

use std::path::Path;

pub const APP_NAME: &str = "Stet";

/// `Untitled N`.
pub fn untitled_name(number: u32) -> String {
    format!("Untitled {number}")
}

/// The lowest number from 1 that no open untitled tab uses, so a closed tab's number is reused.
pub fn next_untitled_number(used: impl IntoIterator<Item = u32>) -> u32 {
    let mut used: Vec<u32> = used.into_iter().collect();
    used.sort_unstable();
    used.dedup();
    let mut candidate = 1;
    for number in used {
        if number == candidate {
            candidate += 1;
        } else if number > candidate {
            break;
        }
    }
    candidate
}

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
    fn untitled_numbers_reuse_the_lowest_gap() {
        assert_eq!(next_untitled_number([]), 1);
        assert_eq!(next_untitled_number([1, 2, 3]), 4);
        assert_eq!(next_untitled_number([2, 3]), 1);
        assert_eq!(next_untitled_number([1, 3, 3, 4]), 2);
        assert_eq!(next_untitled_number([5, 1]), 2);
        assert_eq!(untitled_name(2), "Untitled 2");
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
}
