//! Reading `config.toml` and `keys.toml`, and creating them with commented defaults when the
//! palette opens one that does not exist yet (ADR-017).

use std::fs;
use std::io;
use std::path::Path;

use crate::fs::durable::{self, FileMode, WriteOptions};

/// Settings files larger than this are refused rather than read.
pub const MAX_BYTES: u64 = 1024 * 1024;

/// The text of a settings file, or `None` when there is none.
pub fn read_optional(path: &Path) -> io::Result<Option<String>> {
    match fs::metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(meta) if meta.len() > MAX_BYTES => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} is larger than {MAX_BYTES} bytes", path.display()),
            ));
        }
        Ok(_) => {}
    }
    match fs::read(path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map(Some)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "not UTF-8 text")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Writes `text` to `path` unless the file exists, creating its directory. Returns whether it
/// wrote the file; an existing file is never touched. The file appears complete or not at all
/// (`durable::write_atomic`), with the umask's default mode, like a file the user creates.
pub fn create_if_missing(path: &Path, text: &str) -> io::Result<bool> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let options = WriteOptions::create_new(FileMode::Umask);
    match durable::write_atomic(path, text.as_bytes(), options) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_files_read_as_none() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            read_optional(&dir.path().join("config.toml")).unwrap(),
            None
        );
        fs::write(dir.path().join("config.toml"), "tab_width = 2\n").unwrap();
        assert_eq!(
            read_optional(&dir.path().join("config.toml")).unwrap(),
            Some("tab_width = 2\n".to_owned())
        );
        fs::write(dir.path().join("bad.toml"), [0xff, 0xfe]).unwrap();
        assert!(read_optional(&dir.path().join("bad.toml")).is_err());
    }

    #[test]
    fn creates_once_and_never_overwrites() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stet/keys.toml");
        assert!(create_if_missing(&path, "# defaults\n").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# defaults\n");
        fs::write(&path, "quit = []\n").unwrap();
        assert!(!create_if_missing(&path, "# defaults\n").unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "quit = []\n");
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn created_files_get_the_umask_default_mode() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let reference = dir.path().join("reference");
        fs::write(&reference, "").unwrap();
        let path = dir.path().join("config.toml");
        assert!(create_if_missing(&path, "# defaults\n").unwrap());
        let mode = |path: &Path| fs::metadata(path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), mode(&reference));
    }
}
