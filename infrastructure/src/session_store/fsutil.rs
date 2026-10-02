use super::StoreError;
use std::fs::{self, File, OpenOptions, Permissions};
use std::io::{self, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::Path;

pub(super) const FILE_MODE: u32 = 0o600;

/// Creates or truncates `path` with mode 0600, writes `bytes` and syncs the file.
pub(super) fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(FILE_MODE)
        .open(path)?;
    file.set_permissions(Permissions::from_mode(FILE_MODE))?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Syncs a directory, so renames and new entries in it survive a power loss.
pub(super) fn sync_dir(dir: &Path) -> Result<(), StoreError> {
    File::open(dir)
        .and_then(|dir| dir.sync_all())
        .map_err(|error| StoreError::io("sync", dir, error))
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
