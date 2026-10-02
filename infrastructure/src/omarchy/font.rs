//! The monospace font family. fontconfig is Omarchy's source of truth: `omarchy-font-set`
//! writes `~/.config/fontconfig/fonts.conf` with a `monospace` rule (INTEGRATIONS.md §2).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// Used when `fc-match` is missing or fails; Pango resolves the alias itself.
pub const FALLBACK_FAMILY: &str = "monospace";

/// `fc-match monospace -f '%{family[0]}'`, or [`FALLBACK_FAMILY`].
pub fn monospace_family() -> String {
    monospace_family_with(Path::new("fc-match"))
}

pub fn monospace_family_with(fc_match: &Path) -> String {
    let output = Command::new(fc_match)
        .args(["monospace", "-f", "%{family[0]}"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output();
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

/// `$XDG_CONFIG_HOME/fontconfig`, which `omarchy-font-set` creates and writes.
pub fn fontconfig_dir() -> PathBuf {
    crate::xdg::config_home().join("fontconfig")
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
        assert_eq!(monospace_family_with(&fc_match), "Test Mono Nerd Font");
    }

    #[test]
    fn falls_back_to_the_alias() {
        let _spawning = crate::test_support::spawn_lock();
        let dir = tempfile::tempdir().unwrap();
        for body in ["exit 1", "printf '  \\n'"] {
            let fc_match = script(dir.path(), body);
            assert_eq!(monospace_family_with(&fc_match), FALLBACK_FAMILY, "{body}");
        }
        assert_eq!(
            monospace_family_with(&dir.path().join("missing")),
            FALLBACK_FAMILY
        );
    }

    #[test]
    fn the_real_fc_match_gives_a_name() {
        let _spawning = crate::test_support::spawn_lock();
        assert!(!monospace_family().is_empty());
    }
}
