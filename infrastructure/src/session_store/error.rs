//! The session store's errors. They are cloneable, the I/O error shared, so the store's status
//! can keep the last one and show it after the operation that caused it has returned.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Why a session store operation failed. Cloneable, so [`super::StoreStatus`] can keep the
/// last one.
#[derive(Debug, Clone, thiserror::Error)]
pub enum StoreError {
    /// Another store holds the lock: a second Stet process, or a second store in this one.
    /// `pid` is the process that last took the lock, when it could be read.
    #[error("the session in {} is in use by another Stet process", .state_dir.display())]
    Locked {
        /// Stet's state folder, whose lock is taken.
        state_dir: PathBuf,
        /// The process that holds the lock.
        pid: Option<u32>,
    },
    /// A file operation failed; `action` and `path` say which.
    #[error("could not {action} {}: {source}", .path.display())]
    Io {
        /// What the store was doing, as a verb phrase such as `write` or `remove`.
        action: &'static str,
        /// The file or folder involved.
        path: PathBuf,
        /// The underlying error.
        source: Arc<io::Error>,
    },
    /// An id that is not a plain file name; see `stet_domain::session::is_valid_id`.
    #[error("{0:?} is not a valid backup id")]
    InvalidId(String),
    /// The writer didn't finish within the timeout; the work stays queued.
    #[error("timed out waiting for the session writer")]
    Timeout,
    /// The writer thread is gone, so nothing more is written.
    #[error("the session writer has stopped")]
    WriterStopped,
}

impl StoreError {
    pub(super) fn io(action: &'static str, path: &Path, source: io::Error) -> Self {
        Self::Io {
            action,
            path: path.to_owned(),
            source: Arc::new(source),
        }
    }
}
