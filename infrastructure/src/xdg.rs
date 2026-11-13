//! Stet's own XDG directories (ADR-010): `$XDG_STATE_HOME/stet`, `$XDG_CACHE_HOME/stet` and
//! `$XDG_CONFIG_HOME/stet`. Directories Stet creates for private data are mode 0700
//! (`fs::durable::ensure_private_dir`).

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The folder name Stet uses under each XDG base directory.
pub const APP_DIR: &str = "stet";

/// An XDG base directory: the variable when it holds an absolute path, else `home/fallback`.
pub fn base_dir(value: Option<&OsStr>, home: &Path, fallback: &str) -> PathBuf {
    match value.map(Path::new) {
        Some(dir) if dir.is_absolute() => dir.to_path_buf(),
        _ => home.join(fallback),
    }
}

fn from_env(variable: &str, fallback: &str) -> PathBuf {
    let home = std::env::home_dir().unwrap_or_default();
    base_dir(std::env::var_os(variable).as_deref(), &home, fallback)
}

/// `$XDG_STATE_HOME`, or `~/.local/state`.
#[must_use]
pub fn state_home() -> PathBuf {
    from_env("XDG_STATE_HOME", ".local/state")
}

/// `$XDG_CACHE_HOME`, or `~/.cache`.
#[must_use]
pub fn cache_home() -> PathBuf {
    from_env("XDG_CACHE_HOME", ".cache")
}

/// `$XDG_CONFIG_HOME`, or `~/.config`.
#[must_use]
pub fn config_home() -> PathBuf {
    from_env("XDG_CONFIG_HOME", ".config")
}

/// Where Stet keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StetDirs {
    /// Session, backups and the recent-files list.
    pub state: PathBuf,
    /// Generated style schemes.
    pub cache: PathBuf,
    /// `config.toml` and `keys.toml`.
    pub config: PathBuf,
}

impl StetDirs {
    /// Stet's folders under the user's XDG base directories.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            state: state_home().join(APP_DIR),
            cache: cache_home().join(APP_DIR),
            config: config_home().join(APP_DIR),
        }
    }

    /// Everything under one root, for tests and the self-test harness.
    #[must_use]
    pub fn under(root: &Path) -> Self {
        Self {
            state: root.join("state").join(APP_DIR),
            cache: root.join("cache").join(APP_DIR),
            config: root.join("config").join(APP_DIR),
        }
    }

    /// `recent.json`, the recent-files list.
    #[must_use]
    pub fn recent_file(&self) -> PathBuf {
        self.state.join("recent.json")
    }

    /// Where the generated style schemes are written.
    #[must_use]
    pub fn styles_dir(&self) -> PathBuf {
        self.cache.join("styles")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_dirs_need_an_absolute_value() {
        let home = Path::new("/home/user");
        assert_eq!(
            base_dir(Some(OsStr::new("/srv/state")), home, ".local/state"),
            Path::new("/srv/state")
        );
        for value in [None, Some(OsStr::new("")), Some(OsStr::new("rel"))] {
            assert_eq!(
                base_dir(value, home, ".cache"),
                Path::new("/home/user/.cache")
            );
        }
    }

    #[test]
    fn dirs_under_a_root() {
        let dirs = StetDirs::under(Path::new("/tmp/x"));
        assert_eq!(
            dirs.recent_file(),
            Path::new("/tmp/x/state/stet/recent.json")
        );
        assert_eq!(dirs.styles_dir(), Path::new("/tmp/x/cache/stet/styles"));
        assert!(StetDirs::from_env().state.ends_with("stet"));
    }
}
