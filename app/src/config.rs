//! Where the app keeps and finds things. The self-test harness builds its own configuration,
//! so a test run never touches the user's state, cache or Omarchy directories.

use std::path::PathBuf;
use std::time::Duration;
use stet_infrastructure::omarchy::{fontconfig_dir, omarchy_dir};
use stet_infrastructure::xdg::StetDirs;

pub const APP_ID: &str = if cfg!(debug_assertions) {
    "io.github.pixdevsapps.Stet.Devel"
} else {
    "io.github.pixdevsapps.Stet"
};

/// The installed icon (hicolor), the same for debug and release builds.
pub const ICON_NAME: &str = "io.github.pixdevsapps.Stet";

/// Test hook: overrides the application id, so a self-test and the instances it starts share
/// an id that no real Stet uses.
pub const APP_ID_ENV: &str = "STET_APP_ID";

pub const EXIT_AFTER_ENV: &str = "STET_EXIT_AFTER_MS";

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub app_id: String,
    pub dirs: StetDirs,
    /// `~/.local/state/omarchy`, the parent of `current/`.
    pub omarchy_dir: PathBuf,
    /// `~/.config/fontconfig`, which `omarchy-font-set` writes.
    pub fontconfig_dir: PathBuf,
    pub exit_after: Option<Duration>,
    /// Never forward to, or accept command lines from, another instance.
    pub non_unique: bool,
}

impl AppConfig {
    pub fn from_env(exit_after: Option<Duration>) -> Self {
        Self {
            app_id: app_id_from_env(),
            dirs: StetDirs::from_env(),
            omarchy_dir: omarchy_dir(),
            fontconfig_dir: fontconfig_dir(),
            exit_after,
            non_unique: false,
        }
    }
}

pub fn app_id_from_env() -> String {
    std::env::var(APP_ID_ENV)
        .ok()
        .filter(|id| gio::Application::id_is_valid(id))
        .unwrap_or_else(|| APP_ID.to_owned())
}

/// `STET_EXIT_AFTER_MS`, for headless smoke runs.
pub fn exit_after_from_env() -> Result<Option<Duration>, String> {
    match std::env::var(EXIT_AFTER_ENV) {
        Err(_) => Ok(None),
        Ok(ms) => ms
            .parse()
            .map(|ms| Some(Duration::from_millis(ms)))
            .map_err(|error| {
                format!("{EXIT_AFTER_ENV} must be a whole number of milliseconds: {error}")
            }),
    }
}
