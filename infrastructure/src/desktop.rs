//! The desktop around Stet: its desktop entry and MIME defaults (`xdg-mime`), and the programs
//! the tab menu starts, a file manager and a terminal (INTEGRATIONS.md). Everything here runs
//! child processes or reads directories; callers run it on a worker.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The desktop entry of the installed (release) app, whatever this build's app-id.
pub const DESKTOP_ID: &str = "io.github.pixdevsapps.Stet.desktop";

const DESKTOP_ENTRY: &str = include_str!("../../packaging/io.github.pixdevsapps.Stet.desktop");

/// The command Omarchy's `omarchy-launch-editor` runs for Stet.
pub const COMMAND: &str = "stet";

/// The MIME types Stet's desktop entry lists, in its order.
pub fn mime_types() -> Vec<&'static str> {
    parse_mime_types(DESKTOP_ENTRY)
}

/// The `MimeType=` list of a desktop entry's main group.
pub fn parse_mime_types(entry: &str) -> Vec<&str> {
    entry
        .lines()
        .skip_while(|line| line.trim() != "[Desktop Entry]")
        .skip(1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .find_map(|line| line.strip_prefix("MimeType="))
        .map(|types| {
            types
                .split(';')
                .map(str::trim)
                .filter(|kind| !kind.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Where the desktop entry `id` is installed: `$XDG_DATA_HOME/applications`, then each
/// `$XDG_DATA_DIRS` entry, as the desktop entry specification says.
pub fn find_desktop_file(id: &str) -> Option<PathBuf> {
    let data_home = crate::xdg::base_dir(
        std::env::var_os("XDG_DATA_HOME").as_deref(),
        &std::env::home_dir().unwrap_or_default(),
        ".local/share",
    );
    let data_dirs = std::env::var_os("XDG_DATA_DIRS")
        .filter(|dirs| !dirs.is_empty())
        .unwrap_or_else(|| OsString::from("/usr/local/share:/usr/share"));
    let dirs: Vec<PathBuf> = std::env::split_paths(&data_dirs).collect();
    find_desktop_file_in(id, &data_home, &dirs)
}

pub fn find_desktop_file_in(id: &str, data_home: &Path, data_dirs: &[PathBuf]) -> Option<PathBuf> {
    std::iter::once(data_home)
        .chain(data_dirs.iter().map(PathBuf::as_path))
        .map(|dir| dir.join("applications").join(id))
        .find(|path| path.is_file())
}

/// The program `name` on `$PATH`.
pub fn find_program(name: &str) -> Option<PathBuf> {
    find_program_in(name, &std::env::var_os("PATH").unwrap_or_default())
}

pub fn find_program_in(name: &str, path: &OsStr) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    std::env::split_paths(path)
        .map(|dir| dir.join(name))
        .find(|candidate| {
            candidate
                .metadata()
                .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
        })
}

/// `xdg-mime query default <mime>`: the desktop entry that opens `mime`, if any.
pub fn default_for(mime: &str) -> io::Result<Option<String>> {
    let output = Command::new("xdg-mime")
        .args(["query", "default", mime])
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Ok(None);
    }
    let id = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    Ok(Some(id).filter(|id| !id.is_empty()))
}

/// `xdg-mime default <id> <types…>`, which writes the user's `mimeapps.list`.
pub fn set_default(id: &str, types: &[&str]) -> io::Result<()> {
    let output = Command::new("xdg-mime")
        .arg("default")
        .arg(id)
        .args(types)
        .stdin(Stdio::null())
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "xdg-mime default failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// How to start a program for the desktop: detached from Stet with `setsid --fork`, and in its
/// own systemd scope with Omarchy's `uwsm-app` when that exists, as `omarchy-launch-terminal`
/// does. `has` says whether a program is installed.
fn detached(program: Vec<OsString>, has: &dyn Fn(&str) -> bool) -> Vec<OsString> {
    let mut command = Vec::new();
    if has("setsid") {
        command.extend(["setsid".into(), "--fork".into()]);
    }
    if has("uwsm-app") {
        command.extend(["uwsm-app".into(), "--".into()]);
    }
    command.extend(program);
    command
}

/// The command that opens a terminal in `dir`: `xdg-terminal-exec --dir=<dir>`, Omarchy's
/// default terminal launcher. `None` when it is not installed.
pub fn terminal_command(dir: &Path, has: &dyn Fn(&str) -> bool) -> Option<Vec<OsString>> {
    if !has("xdg-terminal-exec") {
        return None;
    }
    let mut argument = OsString::from("--dir=");
    argument.push(dir);
    Some(detached(vec!["xdg-terminal-exec".into(), argument], has))
}

/// The command that shows `dir` in the default file manager: `xdg-open <dir>`. `None` when
/// xdg-open is not installed.
pub fn folder_command(dir: &Path, has: &dyn Fn(&str) -> bool) -> Option<Vec<OsString>> {
    if !has("xdg-open") {
        return None;
    }
    Some(detached(vec!["xdg-open".into(), dir.into()], has))
}

/// Whether `name` is on `$PATH`, for [`terminal_command`] and [`folder_command`].
pub fn installed(name: &str) -> bool {
    find_program(name).is_some()
}

/// Starts `command` with no input or output and waits only for `setsid --fork` to hand it
/// off, so nothing is left to reap.
pub fn spawn_detached(command: &[OsString]) -> io::Result<()> {
    let (program, args) = command
        .split_first()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "empty command"))?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    if program == "setsid" {
        child.wait()?;
    } else {
        std::thread::spawn(move || child.wait());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn the_desktop_entry_lists_text_types() {
        let types = mime_types();
        assert_eq!(types.first(), Some(&"text/plain"));
        assert!(types.contains(&"text/markdown"));
        assert!(types.contains(&"application/json"));
        assert!(types.iter().all(|kind| kind.contains('/')));
        assert_eq!(
            parse_mime_types(
                "[Desktop Entry]\nName=x\nMimeType=a/b;c/d;\n[Other]\nMimeType=e/f;\n"
            ),
            ["a/b", "c/d"]
        );
        assert!(parse_mime_types("[Desktop Entry]\nName=x\n").is_empty());
    }

    #[test]
    fn the_package_texts_describe_stet() {
        let recipe = include_str!("../../packaging/PKGBUILD.in");
        assert!(DESKTOP_ENTRY.contains("Comment=Edit text and code files, keyboard-first\n"));
        assert!(DESKTOP_ENTRY.contains("Keywords=text;editor;code;notepad;\n"));
        assert!(
            recipe.contains("pkgdesc='Fast, keyboard-first text and code editor for Omarchy'\n")
        );
    }

    #[test]
    fn finds_desktop_files_in_data_home_first() {
        let root = tempfile::tempdir().unwrap();
        let (home, system) = (root.path().join("home"), root.path().join("usr"));
        for dir in [&home, &system] {
            fs::create_dir_all(dir.join("applications")).unwrap();
        }
        assert_eq!(
            find_desktop_file_in(DESKTOP_ID, &home, std::slice::from_ref(&system)),
            None
        );
        fs::write(system.join("applications").join(DESKTOP_ID), "x").unwrap();
        assert_eq!(
            find_desktop_file_in(DESKTOP_ID, &home, std::slice::from_ref(&system)),
            Some(system.join("applications").join(DESKTOP_ID))
        );
        fs::write(home.join("applications").join(DESKTOP_ID), "x").unwrap();
        assert_eq!(
            find_desktop_file_in(DESKTOP_ID, &home, std::slice::from_ref(&system)),
            Some(home.join("applications").join(DESKTOP_ID))
        );
    }

    #[test]
    fn finds_executables_on_the_path() {
        use std::os::unix::fs::PermissionsExt;
        let _guard = crate::test_support::spawn_lock();
        let root = tempfile::tempdir().unwrap();
        let program = root.path().join("tool");
        fs::write(&program, "#!/bin/sh\n").unwrap();
        assert_eq!(find_program_in("tool", root.path().as_os_str()), None);
        fs::set_permissions(&program, fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            find_program_in("tool", root.path().as_os_str()),
            Some(program)
        );
        assert_eq!(find_program_in("missing", root.path().as_os_str()), None);
    }

    #[test]
    fn launch_commands_follow_omarchy() {
        let dir = Path::new("/home/user/project");
        let everything = |_: &str| true;
        assert_eq!(
            terminal_command(dir, &everything).unwrap(),
            [
                "setsid",
                "--fork",
                "uwsm-app",
                "--",
                "xdg-terminal-exec",
                "--dir=/home/user/project"
            ]
            .map(OsString::from)
        );
        let plain = |name: &str| name == "xdg-terminal-exec" || name == "xdg-open";
        assert_eq!(
            terminal_command(dir, &plain).unwrap(),
            ["xdg-terminal-exec", "--dir=/home/user/project"].map(OsString::from)
        );
        assert_eq!(
            folder_command(dir, &plain).unwrap(),
            ["xdg-open", "/home/user/project"].map(OsString::from)
        );
        let nothing = |_: &str| false;
        assert_eq!(terminal_command(dir, &nothing), None);
        assert_eq!(folder_command(dir, &nothing), None);
    }
}
