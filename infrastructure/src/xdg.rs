//! Stet's own XDG directories (ADR-010): `$XDG_STATE_HOME/stet`, `$XDG_CACHE_HOME/stet` and
//! `$XDG_CONFIG_HOME/stet`. Directories Stet creates for private data are mode 0700.

use std::ffi::OsStr;
use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::path::{Path, PathBuf};

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

pub fn state_home() -> PathBuf {
    from_env("XDG_STATE_HOME", ".local/state")
}

pub fn cache_home() -> PathBuf {
    from_env("XDG_CACHE_HOME", ".cache")
}

pub fn config_home() -> PathBuf {
    from_env("XDG_CONFIG_HOME", ".config")
}

/// Where Stet keeps its files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StetDirs {
    /// Session, backups (M2) and the recent-files list.
    pub state: PathBuf,
    /// Generated style schemes.
    pub cache: PathBuf,
    /// `config.toml` and `keys.toml` (M5).
    pub config: PathBuf,
}

impl StetDirs {
    pub fn from_env() -> Self {
        Self {
            state: state_home().join(APP_DIR),
            cache: cache_home().join(APP_DIR),
            config: config_home().join(APP_DIR),
        }
    }

    /// Everything under one root, for tests and the self-test harness.
    pub fn under(root: &Path) -> Self {
        Self {
            state: root.join("state").join(APP_DIR),
            cache: root.join("cache").join(APP_DIR),
            config: root.join("config").join(APP_DIR),
        }
    }

    pub fn recent_file(&self) -> PathBuf {
        self.state.join("recent.json")
    }

    pub fn styles_dir(&self) -> PathBuf {
        self.cache.join("styles")
    }

    pub fn config_file(&self) -> PathBuf {
        self.config.join(stet_domain::settings::CONFIG_FILE)
    }

    pub fn keys_file(&self) -> PathBuf {
        self.config.join(stet_domain::settings::KEYS_FILE)
    }
}

/// Creates `dir` (and missing parents) and makes `dir` itself mode 0700, also when it existed
/// with wider permissions.
pub fn ensure_private_dir(dir: &Path) -> io::Result<()> {
    DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    let mode = fs::metadata(dir)?.permissions().mode() & 0o777;
    if mode != 0o700 {
        fs::set_permissions(dir, Permissions::from_mode(0o700))?;
    }
    Ok(())
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
        assert_eq!(
            dirs.config_file(),
            Path::new("/tmp/x/config/stet/config.toml")
        );
        assert_eq!(dirs.keys_file(), Path::new("/tmp/x/config/stet/keys.toml"));
        assert!(StetDirs::from_env().state.ends_with("stet"));
    }

    #[test]
    fn private_dirs_are_0700() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("a/b");
        ensure_private_dir(&dir).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );

        fs::set_permissions(&dir, Permissions::from_mode(0o755)).unwrap();
        ensure_private_dir(&dir).unwrap();
        assert_eq!(
            fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
