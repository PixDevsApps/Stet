//! The current Omarchy theme: `current/theme.name` and the palette of
//! `current/theme/colors.toml`.

use std::collections::BTreeMap;
use std::io;
use std::path::Path;

use super::colors;

/// The current Omarchy theme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentTheme {
    /// `theme.name`, trimmed; empty when it could not be read.
    pub name: String,
    /// The palette with every fallback applied ([`colors::resolve_palette`]).
    pub resolved: BTreeMap<String, String>,
}

/// Reads the theme in `current` (Omarchy's `current` directory). `None` when it has no
/// `theme/colors.toml`; an error when that file could not be read.
pub fn read_current_theme(current: &Path) -> io::Result<Option<CurrentTheme>> {
    read_current_theme_with(current, super::theme_color_script().as_deref())
}

fn read_current_theme_with(
    current: &Path,
    script: Option<&Path>,
) -> io::Result<Option<CurrentTheme>> {
    let colors_file = current.join("theme/colors.toml");
    if !colors_file.is_file() {
        return Ok(None);
    }
    let resolved = colors::resolve_palette_with(&colors_file, script)?;
    let name = std::fs::read_to_string(current.join("theme.name"))
        .map(|name| name.trim().to_owned())
        .unwrap_or_default();
    Ok(Some(CurrentTheme { name, resolved }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn reads_the_name_and_resolves_the_palette() {
        let current = tempfile::tempdir().unwrap();
        fs::create_dir(current.path().join("theme")).unwrap();
        fs::write(
            current.path().join("theme/colors.toml"),
            "background = \"#101010\"\naccent = \"#89b4fa\"\n",
        )
        .unwrap();
        fs::write(current.path().join("theme.name"), "tokyo-night\n").unwrap();
        let theme = read_current_theme_with(current.path(), None)
            .unwrap()
            .unwrap();
        assert_eq!(theme.name, "tokyo-night");
        assert_eq!(theme.resolved["accent"], "#89b4fa");
        assert_eq!(theme.resolved["dark_background"], "#0c0c0c");
        assert_eq!(theme.resolved["mode"], "dark");
    }

    #[test]
    fn a_missing_name_is_empty() {
        let current = tempfile::tempdir().unwrap();
        fs::create_dir(current.path().join("theme")).unwrap();
        fs::write(current.path().join("theme/colors.toml"), "").unwrap();
        let theme = read_current_theme_with(current.path(), None)
            .unwrap()
            .unwrap();
        assert_eq!(theme.name, "");
    }

    #[test]
    fn no_colors_file_is_no_theme() {
        let current = tempfile::tempdir().unwrap();
        assert_eq!(read_current_theme_with(current.path(), None).unwrap(), None);
        fs::create_dir_all(current.path().join("theme/colors.toml")).unwrap();
        assert_eq!(read_current_theme_with(current.path(), None).unwrap(), None);
    }
}
