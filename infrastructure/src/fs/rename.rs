//! Renaming an open document's file (the tab menu's Rename File…).

use std::io;
use std::path::Path;

use rustix::fs::{CWD, RenameFlags, renameat_with};

/// Renames `from` to `to` unless `to` exists, atomically (`RENAME_NOREPLACE`). A file system
/// without that flag gets a check followed by a plain rename.
pub fn rename_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    match renameat_with(CWD, from, CWD, to, RenameFlags::NOREPLACE) {
        Ok(()) => Ok(()),
        Err(error) if error == rustix::io::Errno::INVAL || error == rustix::io::Errno::NOSYS => {
            if to.symlink_metadata().is_ok() {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            std::fs::rename(from, to)
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn renames_but_never_replaces() {
        let dir = tempfile::tempdir().unwrap();
        let (a, b, c) = (
            dir.path().join("a"),
            dir.path().join("b"),
            dir.path().join("c"),
        );
        fs::write(&a, "a").unwrap();
        fs::write(&c, "c").unwrap();
        rename_no_replace(&a, &b).unwrap();
        assert!(!a.exists());
        assert_eq!(fs::read_to_string(&b).unwrap(), "a");
        let error = rename_no_replace(&b, &c).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_to_string(&c).unwrap(), "c");
        assert_eq!(fs::read_to_string(&b).unwrap(), "a");
    }
}
