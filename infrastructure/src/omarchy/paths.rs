use std::path::{Path, PathBuf};

const THEME_COLOR_SCRIPT: &str = "usr/share/omarchy/bin/omarchy-theme-color";

/// Points Stet at another Omarchy state directory (instead of `~/.local/state/omarchy`), so
/// tests and the self-test harness can swap themes in a fake one.
pub const STATE_DIR_ENV: &str = "STET_OMARCHY_STATE_DIR";

/// Where Omarchy keeps its state: `~/.local/state`. Omarchy's scripts write
/// `$HOME/.local/state/omarchy` whatever `$XDG_STATE_HOME` says, so Stet looks there too.
pub fn state_home(home: &Path) -> PathBuf {
    home.join(".local/state")
}

/// Omarchy's state directory, which holds `current/`.
pub fn omarchy_dir_in(state_home: &Path) -> PathBuf {
    state_home.join("omarchy")
}

/// Omarchy's `current/` directory, which holds `theme/`, `theme.name` and `background`.
pub fn current_dir_in(state_home: &Path) -> PathBuf {
    omarchy_dir_in(state_home).join("current")
}

/// The resolver script, when installed under `root` (`/` outside tests).
pub fn theme_color_script_in(root: &Path) -> Option<PathBuf> {
    let script = root.join(THEME_COLOR_SCRIPT);
    script.is_file().then_some(script)
}

/// `~/.local/state/omarchy`, or [`STATE_DIR_ENV`] when it holds an absolute path.
pub fn omarchy_dir() -> PathBuf {
    match std::env::var_os(STATE_DIR_ENV).map(PathBuf::from) {
        Some(dir) if dir.is_absolute() => dir,
        _ => omarchy_dir_in(&state_home(&std::env::home_dir().unwrap_or_default())),
    }
}

pub fn current_dir() -> PathBuf {
    omarchy_dir().join("current")
}

pub fn current_theme_dir() -> PathBuf {
    current_dir().join("theme")
}

pub fn theme_color_script() -> Option<PathBuf> {
    theme_color_script_in(Path::new("/"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const HOME: &str = "/home/user";

    #[test]
    fn state_home_is_under_home() {
        assert_eq!(
            state_home(Path::new(HOME)),
            Path::new("/home/user/.local/state")
        );
    }

    #[test]
    fn current_dir_is_under_omarchy() {
        assert_eq!(
            current_dir_in(Path::new("/home/user/.local/state")),
            Path::new("/home/user/.local/state/omarchy/current")
        );
    }

    #[test]
    fn theme_color_script_requires_the_file() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(theme_color_script_in(root.path()), None);

        let script = root.path().join(THEME_COLOR_SCRIPT);
        fs::create_dir_all(script.parent().unwrap()).unwrap();
        fs::write(&script, "#!/bin/sh\n").unwrap();
        assert_eq!(theme_color_script_in(root.path()), Some(script));
    }

    #[test]
    fn theme_color_script_rejects_a_directory() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join(THEME_COLOR_SCRIPT)).unwrap();
        assert_eq!(theme_color_script_in(root.path()), None);
    }

    #[test]
    fn wrappers_end_in_the_expected_components() {
        assert!(current_dir().ends_with("current"));
        assert!(current_theme_dir().ends_with("current/theme"));
    }
}
