//! The session (ADR-006): what each tab keeps for it, and how long opening and closing the
//! store wait. The window side (restoring, the 7 s backups, quitting and `--wait`) is in
//! `window::session`.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use stet_domain::session::{RecordId, TabRecord};
use stet_infrastructure::session_store::{StoreError, new_id};

/// How long a starting instance retries a locked store
/// ([`stet_infrastructure::session_store::SessionStore::open_waiting`]): a quitting instance
/// holds the lock until it has flushed, which takes at most [`FLUSH_TIMEOUT`].
pub const LOCK_RETRY: Duration = Duration::from_secs(2);

/// How long quitting, and SIGTERM, SIGHUP and SIGINT, wait for the last commit to reach the
/// disk.
pub const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// A tab's part in the session.
#[derive(Debug, Clone)]
pub struct TabSession {
    /// The tab's id in `session.json`, also the name of its backup.
    pub id: RecordId,
    /// A restored tab that was not shown yet: its text is still in its backup or file, and
    /// the session keeps this record for it unchanged.
    pub stub: Option<Box<TabRecord>>,
    /// The backup holds this tab's text (written, or queued in the store).
    pub backup: bool,
    /// The buffer revision the backup holds.
    pub backup_revision: Option<u64>,
    /// When the backup was last written, for the large-document backoff.
    pub backed_up_at: Option<Instant>,
    /// A recovered orphan file, deleted once a commit holds the text.
    pub orphan: Option<PathBuf>,
    /// The tab's unsaved changes were not restored: show a banner once it is loaded.
    pub lost_changes: bool,
    /// The file changed on disk after the backup this tab was restored from.
    pub conflict: bool,
    /// Closing the window discarded this large document's changes: restore it from its file.
    pub discarded: bool,
    /// `--wait` command lines waiting for this tab to close; such a tab stays out of the
    /// session.
    pub waits: Vec<u64>,
}

impl Default for TabSession {
    fn default() -> Self {
        Self {
            id: new_id(),
            stub: None,
            backup: false,
            backup_revision: None,
            backed_up_at: None,
            orphan: None,
            lost_changes: false,
            conflict: false,
            discarded: false,
            waits: Vec::new(),
        }
    }
}

/// Why the session is off in this window, for a toast.
pub fn unavailable_message(error: &StoreError) -> String {
    match error {
        StoreError::Locked {
            pid: Some(pid),
            state_dir,
        } => format!(
            "Another Stet (process {pid}) is using the session in {}, so this window won't \
             keep its tabs or back up unsaved changes",
            state_dir.display()
        ),
        StoreError::Locked { state_dir, .. } => format!(
            "Another Stet is using the session in {}, so this window won't keep its tabs or \
             back up unsaved changes",
            state_dir.display()
        ),
        error => format!(
            "Stet couldn't open its session ({error}), so this window won't keep its tabs or \
             back up unsaved changes"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_locked_session_names_the_process_holding_it() {
        let state_dir = PathBuf::from("/state/stet");
        let message = unavailable_message(&StoreError::Locked {
            state_dir: state_dir.clone(),
            pid: Some(4242),
        });
        assert!(message.contains("process 4242"), "{message}");
        assert!(message.contains("/state/stet"), "{message}");
        let message = unavailable_message(&StoreError::Locked {
            state_dir,
            pid: None,
        });
        assert!(message.starts_with("Another Stet is using"), "{message}");
    }
}
