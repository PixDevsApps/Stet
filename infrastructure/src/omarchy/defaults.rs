//! Omarchy's default editor: `~/.local/state/omarchy/defaults/editor` holds the command that
//! `omarchy-launch-editor` (SUPER+SHIFT+N) runs. `omarchy-default-editor` writes it only for
//! the editors it knows, so "Set as Default Editor" writes it directly (INTEGRATIONS.md).

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// The file in Omarchy's state directory (the parent of `current/`).
pub fn editor_file(omarchy_dir: &Path) -> PathBuf {
    omarchy_dir.join("defaults").join("editor")
}

/// The command in the file, as `omarchy-launch-editor` reads it (its first line, trimmed), or
/// `None` when there is no file (Omarchy then runs nvim).
pub fn read_default_editor(omarchy_dir: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(editor_file(omarchy_dir)) {
        Ok(text) => Ok(Some(text.lines().next().unwrap_or("").trim().to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes `command` as the default editor, as `omarchy-default-editor` does (`printf '%s\n'`),
/// through a temporary file renamed into place.
pub fn write_default_editor(omarchy_dir: &Path, command: &str) -> io::Result<PathBuf> {
    let path = editor_file(omarchy_dir);
    let dir = path.parent().expect("the editor file has a directory");
    fs::create_dir_all(dir)?;
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    writeln!(temp, "{command}")?;
    temp.as_file().sync_all()?;
    temp.persist(&path).map_err(|error| error.error)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_writes_like_omarchy() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_default_editor(dir.path()).unwrap(), None);
        let path = write_default_editor(dir.path(), "stet").unwrap();
        assert_eq!(path, dir.path().join("defaults/editor"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "stet\n");
        assert_eq!(
            read_default_editor(dir.path()).unwrap().as_deref(),
            Some("stet")
        );
        fs::write(&path, "  code  \nignored\n").unwrap();
        assert_eq!(
            read_default_editor(dir.path()).unwrap().as_deref(),
            Some("code")
        );
        write_default_editor(dir.path(), "stet").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "stet\n");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }
}
