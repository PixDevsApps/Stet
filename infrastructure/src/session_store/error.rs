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
        state_dir: PathBuf,
        pid: Option<u32>,
    },
    /// A file operation failed; `action` and `path` say which.
    #[error("could not {action} {}: {source}", .path.display())]
    Io {
        action: &'static str,
        path: PathBuf,
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
