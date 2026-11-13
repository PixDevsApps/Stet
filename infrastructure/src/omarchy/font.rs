//! The monospace font family. fontconfig is Omarchy's source of truth: `omarchy-font-set`
//! writes `~/.config/fontconfig/fonts.conf` with a `monospace` rule (INTEGRATIONS.md §2).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Used when `fc-match` is missing or fails; Pango resolves the alias itself.
pub const FALLBACK_FAMILY: &str = "monospace";

/// `fc-match monospace -f '%{family[0]}'`, or [`FALLBACK_FAMILY`]. In a Flatpak fontconfig
/// would read the app's own `XDG_CONFIG_HOME`, so `fc-match` is given the host's, where
/// `omarchy-font-set` writes `fontconfig/fonts.conf`; the host's `/etc/fonts` stays out of reach.
pub fn monospace_family() -> String {
    let config_home = crate::sandbox::is_flatpak().then(crate::sandbox::desktop_config_home);
    monospace_family_with(Path::new("fc-match"), config_home.as_deref())
}

/// [`monospace_family`] with the `fc-match` at `fc_match`, and `XDG_CONFIG_HOME` set to
/// `config_home` when given.
pub fn monospace_family_with(fc_match: &Path, config_home: Option<&Path>) -> String {
    let mut command = Command::new(fc_match);
    command
        .args(["monospace", "-f", "%{family[0]}"])
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    if let Some(config_home) = config_home {
        command.env("XDG_CONFIG_HOME", config_home);
    }
    let output = command.output();
    match output {
        Ok(output) if output.status.success() => {
            let family = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            if family.is_empty() {
                FALLBACK_FAMILY.to_owned()
            } else {
                family
            }
        }
        Ok(output) => {
            tracing::warn!(status = %output.status, "fc-match failed; using the monospace alias");
            FALLBACK_FAMILY.to_owned()
        }
        Err(error) => {
            tracing::warn!(%error, "could not run fc-match; using the monospace alias");
            FALLBACK_FAMILY.to_owned()
        }
    }
}

/// `$XDG_CONFIG_HOME/fontconfig`, which `omarchy-font-set` creates and writes; the host's in a
/// Flatpak.
#[must_use]
pub fn fontconfig_dir() -> PathBuf {
    crate::sandbox::desktop_config_home().join("fontconfig")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    fn script(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fc-match");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn reads_the_first_family() {
        let _spawning = crate::test_support::spawn_lock();
        let dir = tempfile::tempdir().unwrap();
        let fc_match = script(
            dir.path(),
            "[ \"$1\" = monospace ] && [ \"$3\" = '%{family[0]}' ] && printf 'Test Mono Nerd Font'",
        );
        assert_eq!(
            monospace_family_with(&fc_match, None),
            "Test Mono Nerd Font"
        );
    }

    #[test]
    fn gives_fc_match_the_config_home() {
        let _spawning = crate::test_support::spawn_lock();
        let dir = tempfile::tempdir().unwrap();
        let fc_match = script(
            dir.path(),
            "[ \"$XDG_CONFIG_HOME\" = /host/config ] && printf 'Host Mono' || printf 'Own Mono'",
        );
        assert_eq!(
            monospace_family_with(&fc_match, Some(Path::new("/host/config"))),
            "Host Mono"
        );
    }

    #[test]
    fn falls_back_to_the_alias() {
        let _spawning = crate::test_support::spawn_lock();
        let dir = tempfile::tempdir().unwrap();
        for body in ["exit 1", "printf '  \\n'"] {
            let fc_match = script(dir.path(), body);
            assert_eq!(
                monospace_family_with(&fc_match, None),
                FALLBACK_FAMILY,
                "{body}"
            );
        }
        assert_eq!(
            monospace_family_with(&dir.path().join("missing"), None),
            FALLBACK_FAMILY
        );
    }

    #[test]
    fn the_real_fc_match_gives_a_name() {
        let _spawning = crate::test_support::spawn_lock();
        assert!(!monospace_family().is_empty());
    }
}
