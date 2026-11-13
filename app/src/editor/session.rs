//! A tab's part in the session (ADR-006). The window side (opening the store, restoring, the
//! backups, quitting and `--wait`) is in `window::documents::session`.

use std::path::PathBuf;
use std::time::Instant;
use stet_domain::session::{RecordId, TabRecord};
use stet_infrastructure::session_store::new_id;

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
