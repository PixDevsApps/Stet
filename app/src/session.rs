//! The session (ADR-006, M2): what each tab keeps for it, and opening the store. The window
//! side (restoring, the 7 s backups, quitting and `--wait`) is in `window::session`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use stet_domain::session::TabRecord;
use stet_infrastructure::session_store::{SessionStore, StoreError, new_id};

/// How long a starting instance retries a locked store: a quitting instance holds the lock
/// until it has flushed, which takes at most [`FLUSH_TIMEOUT`].
pub const LOCK_RETRY: Duration = Duration::from_secs(2);

/// How long quitting, and SIGTERM, SIGHUP and SIGINT, wait for the last commit to reach the
/// disk.
pub const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

const LOCK_POLL: Duration = Duration::from_millis(50);

/// A tab's part in the session.
#[derive(Debug, Clone)]
pub struct TabSession {
    /// The tab's id in `session.json`, also the name of its backup.
    pub id: String,
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

/// Opens the store in `dir`, retrying for [`LOCK_RETRY`] while another process holds its lock.
/// Blocks: call it on a worker.
pub fn open_store(dir: &Path) -> Result<SessionStore, StoreError> {
    let started = Instant::now();
    loop {
        match SessionStore::open(dir) {
            Err(StoreError::Locked { .. }) if started.elapsed() < LOCK_RETRY => {
                std::thread::sleep(LOCK_POLL);
            }
            result => return result,
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
    fn a_lock_held_briefly_is_waited_for() {
        let dir = tempfile_dir("lock-brief");
        let first = open_store(&dir).unwrap();
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(first);
        });
        let started = Instant::now();
        let second = open_store(&dir);
        assert!(second.is_ok(), "{second:?}");
        assert!(started.elapsed() >= Duration::from_millis(250));
        releaser.join().unwrap();
        drop(second);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_lock_held_for_good_is_reported() {
        let dir = tempfile_dir("lock-held");
        let first = open_store(&dir).unwrap();
        let started = Instant::now();
        let error = open_store(&dir).unwrap_err();
        assert!(started.elapsed() >= LOCK_RETRY);
        assert!(matches!(error, StoreError::Locked { .. }), "{error:?}");
        let message = unavailable_message(&error);
        assert!(
            message.contains(&std::process::id().to_string()),
            "{message}"
        );
        drop(first);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn tempfile_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "stet-session-{name}-{}-{}",
            std::process::id(),
            new_id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
