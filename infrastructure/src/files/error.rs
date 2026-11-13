//! The errors of Find in Files, Replace in Files and the project walk: why a run didn't
//! start, and why one file or folder was skipped. Per-file errors are cloneable and compare by
//! kind and message, so a run's report can hold and test them.

use std::io;
use std::mem::discriminant;
use std::path::PathBuf;
use std::sync::Arc;

use crate::fs::{LoadError, SaveError};
use crate::search::{PatternError, SearchError};

/// Why a Find in Files or Replace in Files run did not start.
#[derive(Debug, thiserror::Error)]
pub enum StartError {
    #[error(transparent)]
    /// The search pattern doesn't compile.
    Pattern(#[from] PatternError),
    #[error("invalid filter: {0}")]
    /// An include or exclude filter isn't a valid glob.
    Filter(#[source] ignore::Error),
    #[error("could not start the search: {0}")]
    /// The thread that runs the search couldn't be spawned.
    SearchThread(#[source] io::Error),
    #[error("could not start replacing: {0}")]
    /// The thread that runs the replace couldn't be spawned.
    ReplaceThread(#[source] io::Error),
}

/// Why a file or folder could not be searched or replaced in. Cloneable, so the I/O errors
/// are shared.
#[derive(Debug, Clone, thiserror::Error)]
pub enum FileError {
    /// The walk could not read a folder or a file's entry.
    #[error(transparent)]
    Walk(Arc<ignore::Error>),
    #[error(transparent)]
    /// Reading or writing a file failed.
    Io(Arc<io::Error>),
    #[error(transparent)]
    /// Reading a file to replace in failed.
    Load(Arc<LoadError>),
    #[error(transparent)]
    /// Saving a file after replacing in it failed.
    Save(Arc<SaveError>),
    #[error(transparent)]
    /// Searching a file's content failed.
    Search(#[from] SearchError),
    #[error("the search thread panicked")]
    /// The thread running the search panicked.
    Panicked,
}

/// Errors compare by kind and message, since `io::Error` has no equality of its own.
impl PartialEq for FileError {
    fn eq(&self, other: &Self) -> bool {
        discriminant(self) == discriminant(other) && self.to_string() == other.to_string()
    }
}

impl Eq for FileError {}

impl From<ignore::Error> for FileError {
    fn from(error: ignore::Error) -> Self {
        Self::Walk(Arc::new(error))
    }
}

impl From<io::Error> for FileError {
    fn from(error: io::Error) -> Self {
        Self::Io(Arc::new(error))
    }
}

impl From<LoadError> for FileError {
    fn from(error: LoadError) -> Self {
        Self::Load(Arc::new(error))
    }
}

impl From<SaveError> for FileError {
    fn from(error: SaveError) -> Self {
        Self::Save(Arc::new(error))
    }
}

/// A file or folder a run could not read or write, and why.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{}: {error}", .path.display())]
pub struct PathError {
    /// Empty when the error names no path.
    pub path: PathBuf,
    /// What went wrong there.
    pub error: FileError,
}

impl PathError {
    /// A walk error, at the path it names.
    pub(crate) fn walk(error: ignore::Error) -> Self {
        Self {
            path: walk_error_path(&error),
            error: error.into(),
        }
    }
}

fn walk_error_path(error: &ignore::Error) -> PathBuf {
    match error {
        ignore::Error::WithPath { path, .. } => path.clone(),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            walk_error_path(err)
        }
        ignore::Error::Loop { child, .. } => child.clone(),
        _ => PathBuf::new(),
    }
}
