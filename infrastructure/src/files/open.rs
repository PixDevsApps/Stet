//! The index of open documents a walk may reach, keyed by their file's path with symbolic
//! links resolved. Find in Files searches such a file's text instead of the file, and Replace
//! in Files hands it back to the app instead of writing it.

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The open documents a walk may reach, by their file's path with symbolic links resolved.
///
/// Paths are resolved when they are indexed, so callers may pass any path to the file; a
/// document whose file does not exist is left out. Resolving touches the disk, so the index is
/// built on the job's thread.
pub(crate) struct OpenIndex<T> {
    entries: HashMap<PathBuf, T>,
    /// Their file names, so that only likely candidates are resolved during the walk.
    names: HashSet<OsString>,
}

impl<T> OpenIndex<T> {
    pub(crate) fn new(documents: impl IntoIterator<Item = (PathBuf, T)>) -> Self {
        let entries: HashMap<PathBuf, T> = documents
            .into_iter()
            .filter_map(|(path, entry)| Some((std::fs::canonicalize(path).ok()?, entry)))
            .collect();
        let names = entries
            .keys()
            .filter_map(|path| path.file_name().map(OsString::from))
            .collect();
        Self { entries, names }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The resolved path and the entry of the open document whose file is at `path`.
    pub(crate) fn get(&self, path: &Path) -> Option<(PathBuf, &T)> {
        if self.entries.is_empty() || !self.names.contains(path.file_name()?) {
            return None;
        }
        let canonical = std::fs::canonicalize(path).ok()?;
        let entry = self.entries.get(&canonical)?;
        Some((canonical, entry))
    }
}
