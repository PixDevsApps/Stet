//! The few file operations the session store repeats: syncing a folder, removing a file that
//! may already be gone, and emptying a folder of its files. Errors keep the action and path.

use super::StoreError;
use crate::fs::durable;
use std::fs;
use std::io;
use std::path::Path;

/// Syncs a directory, so renames and new entries in it survive a power loss. The writer
/// returns the error: its next step (a manifest referencing new backups) depends on it.
pub(super) fn sync_dir(dir: &Path) -> Result<(), StoreError> {
    durable::sync_dir(dir).map_err(|error| StoreError::io("sync", dir, error))
}

/// Removes a file; one that is already gone counts as removed.
pub(super) fn remove_file(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// Removes every entry of `dir` except directories.
pub(super) fn remove_files_in(dir: &Path) -> Result<(), StoreError> {
    let entries = fs::read_dir(dir).map_err(|error| StoreError::io("read", dir, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| StoreError::io("read", dir, error))?;
        if entry.file_type().is_ok_and(|kind| !kind.is_dir()) {
            let path = entry.path();
            remove_file(&path).map_err(|error| StoreError::io("delete", &path, error))?;
        }
    }
    Ok(())
}
