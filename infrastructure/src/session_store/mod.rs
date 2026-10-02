//! The plain-file session and backups under the state directory (ADR-006).
//!
//! ```text
//! <state_dir>/                  0700; the app passes $XDG_STATE_HOME/stet
//!   lock                        0600; exclusive flock while a store is open, holds its pid
//!   session.json                0600; the manifest, a serialized `Session`
//!   session.json.prev           0600; the manifest before the last write
//!   session.json.corrupt        0600; an unusable manifest that `load` set aside
//!   backup/<id>.txt             0600; the buffer text of a tab, UTF-8 with LF line endings
//!   backup/orphaned/            0700; backups no manifest references, kept until adopted
//! ```
//!
//! **Ordering.** One background thread does every write, in commit order. A write stores each
//! queued backup (temp file in `backup/`, fsync, rename), syncs `backup/`, writes the manifest
//! (temp file, fsync, the old one renamed to `.prev`, rename, sync the state directory), and
//! only then deletes the backups the commits removed. So `session.json` never references a
//! missing or half-written backup. A crash in between can leave a backup newer than the
//! manifest describes (clamp offsets), or backups no manifest references yet (orphans).
//! Commits that queue up while the writer is busy are folded into one write, the newest text
//! per backup winning. A failed write keeps its data queued and is retried with the next
//! commit or flush.
//!
//! **Restore.** [`SessionStore::load`] repairs what a crash can leave behind:
//! - it reads `session.json`, or `session.json.prev` when that is missing or unusable; an
//!   unusable `session.json` is renamed to `session.json.corrupt`;
//! - it deletes leftover temp files, which are incomplete or superseded by definition;
//! - it clears references to missing backups and reports those tabs;
//! - it moves every other file in `backup/` into `backup/orphaned/`, never deleting one, and
//!   returns all orphans, including those left by earlier runs. An orphan stays, and every
//!   `load` returns it, until a commit lists it in [`Snapshot::adopted_orphans`] or
//!   [`SessionStore::forget_drafts`] runs, so a crash before the UI adopted it can't hide it.
//!
//! `.prev` is a fallback, not a second guarantee: backups removed after it was current may be
//! gone, and those tabs are reported as missing.
//!
//! Nothing here logs document text, and `Snapshot` and `Orphan` show only lengths in `Debug`.
//!
//! ```no_run
//! use std::sync::Arc;
//! use std::time::Duration;
//! use stet_domain::session::{Session, TabRecord};
//! use stet_infrastructure::session_store::{SessionStore, Snapshot, new_id};
//!
//! # fn main() -> Result<(), stet_infrastructure::session_store::StoreError> {
//! let store = SessionStore::open("/home/user/.local/state/stet")?;
//! let restored = store.load()?;
//! // Build the active tab now and the others when first shown (`read_backup`), and reopen
//! // `restored.orphans` as untitled tabs.
//!
//! // Every 7 s: the whole session, the texts that changed and the backups no longer needed.
//! let id = new_id();
//! let tab = TabRecord {
//!     id: id.clone(),
//!     backup: Some(id.clone()),
//!     dirty: true,
//!     ..TabRecord::default()
//! };
//! store.commit(Snapshot {
//!     session: Session { tabs: vec![tab], ..restored.session },
//!     backups: vec![(id, Arc::from("unsaved text"))],
//!     ..Snapshot::default()
//! })?;
//!
//! // On quit, SIGTERM or SIGHUP: one last commit, then a bounded wait.
//! store.close(Duration::from_secs(2))?;
//! # Ok(())
//! # }
//! ```

mod error;
mod fsutil;
mod layout;
mod restore;
mod writer;

#[cfg(test)]
mod tests;

pub use error::StoreError;

use layout::Layout;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, SystemTime};
use stet_domain::session::{Session, is_valid_id};
use writer::{BackupWriter, Reply, Request, Shared};

/// One commit: the session to write and the backup texts that changed since the last one.
#[derive(Clone, Default)]
pub struct Snapshot {
    /// The session to write. Tabs keep referencing backups that earlier commits wrote, or that
    /// `load` found, without sending their text again; restored tabs that were never opened
    /// keep their `TabRecord` unchanged.
    pub session: Session,
    /// Backup texts by id, written before the manifest. A queued text for the same id is
    /// replaced, not written.
    pub backups: Vec<(String, Arc<str>)>,
    /// Backups no longer needed (tab saved, closed or reverted), deleted after the manifest.
    /// An id the session still references is kept.
    pub removed: Vec<String>,
    /// Paths from [`Restored::orphans`] whose text the session now holds in its own backups,
    /// deleted after the manifest.
    pub adopted_orphans: Vec<PathBuf>,
}

impl fmt::Debug for Snapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let backups: Vec<(&str, usize)> = self
            .backups
            .iter()
            .map(|(id, text)| (id.as_str(), text.len()))
            .collect();
        f.debug_struct("Snapshot")
            .field("tabs", &self.session.tabs.len())
            .field("backup_bytes", &backups)
            .field("removed", &self.removed)
            .field("adopted_orphans", &self.adopted_orphans)
            .finish()
    }
}

/// What [`SessionStore::load`] found.
#[derive(Debug, Clone)]
pub struct Restored {
    /// The session, with invalid or duplicate tab ids replaced and references to missing
    /// backups cleared.
    pub session: Session,
    /// Which manifest the session came from.
    pub source: ManifestSource,
    /// Tabs whose backup was missing, by id. Their `backup` is now `None`, so they reopen
    /// from their file or empty; warn about lost changes when `dirty` is set.
    pub missing_backups: Vec<String>,
    /// Texts that no manifest references, oldest first, to reopen as untitled tabs.
    pub orphans: Vec<Orphan>,
}

/// Which manifest [`SessionStore::load`] used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestSource {
    /// `session.json`.
    Current,
    /// `session.json.prev`, because `session.json` was unusable.
    Previous { current: ManifestProblem },
    /// Neither, so the session is empty; on a first run both are `Missing`.
    Empty {
        current: ManifestProblem,
        previous: ManifestProblem,
    },
}

/// Why a manifest couldn't be used.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestProblem {
    /// The file doesn't exist.
    Missing,
    /// The file exists but couldn't be read.
    Unreadable(io::ErrorKind),
    /// Not a JSON object, or a malformed `schema_version`; where the first error is.
    Invalid { line: usize, column: usize },
}

/// A text that no manifest references, in `backup/orphaned/`.
#[derive(Clone)]
pub struct Orphan {
    /// List it in [`Snapshot::adopted_orphans`] once a tab holds the text and the snapshot
    /// carries that tab's backup.
    pub path: PathBuf,
    /// Safe to insert into a buffer: invalid UTF-8 replaced, line endings normalized to LF and
    /// NUL shown as U+2400. Backups Stet wrote pass through unchanged.
    pub text: String,
    /// When the file was last written, if known.
    pub modified: Option<SystemTime>,
}

impl fmt::Debug for Orphan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Orphan")
            .field("path", &self.path)
            .field("text_len", &self.text.len())
            .field("modified", &self.modified)
            .finish()
    }
}

/// The writer's progress, readable without waiting.
#[derive(Debug, Clone, Default)]
pub struct StoreStatus {
    /// Writes completed since the store opened.
    pub commits: u64,
    /// The most recent completed write.
    pub last_commit: Option<CommitStats>,
    /// Why the last write failed, until one succeeds. Its data stays queued.
    pub error: Option<StoreError>,
}

/// What one write did and how long it took; `backup_time` against `backup_bytes` gives the
/// cost to plan the large-document backoff with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CommitStats {
    /// Snapshots folded into this write.
    pub snapshots: usize,
    /// Backups written.
    pub backups: usize,
    /// Bytes of backup text written.
    pub backup_bytes: u64,
    /// Writing, syncing and renaming the backups, and syncing `backup/`.
    pub backup_time: Duration,
    /// Writing and renaming the manifest, and syncing the state directory.
    pub manifest_time: Duration,
    /// The whole write, deletions included.
    pub total_time: Duration,
}

/// The session store. It holds the lock and a writer thread until it is closed or dropped.
/// Every method but `close` takes `&self`, so it can be shared behind an `Arc`.
pub struct SessionStore {
    layout: Layout,
    requests: mpsc::Sender<Request>,
    shared: Arc<Shared>,
    writer: Option<JoinHandle<()>>,
}

impl SessionStore {
    /// Creates the layout under `state_dir`, takes the lock and starts the writer. Fails with
    /// [`StoreError::Locked`] while another store holds the lock.
    ///
    /// `Locked` can be brief: a quitting instance holds the lock until it has flushed, and
    /// the lock belongs to the open file, which a process forked by another thread shares
    /// until it execs. Retry for a moment before treating it as a second instance.
    pub fn open(state_dir: impl Into<PathBuf>) -> Result<Self, StoreError> {
        Self::start(BackupWriter::new(Layout::new(state_dir.into()))?)
    }

    fn start(writer: BackupWriter) -> Result<Self, StoreError> {
        let layout = writer.layout().clone();
        let shared = writer.shared();
        let (requests, receiver) = mpsc::channel();
        let writer = thread::Builder::new()
            .name("stet-session".into())
            .spawn(move || writer.run(receiver))
            .map_err(|error| StoreError::io("start the writer for", layout.root(), error))?;
        Ok(Self {
            layout,
            requests,
            shared,
            writer: Some(writer),
        })
    }

    /// The state directory this store manages.
    pub fn state_dir(&self) -> &Path {
        self.layout.root()
    }

    /// Reads the session and repairs what a crash left behind (see the module docs). Call it
    /// once at startup, before the first commit. Blocks until the writer has done it.
    pub fn load(&self) -> Result<Restored, StoreError> {
        self.ask(Request::Load, None)
    }

    /// Where the backup `id` is kept, or `None` for an invalid id.
    pub fn backup_path(&self, id: &str) -> Option<PathBuf> {
        is_valid_id(id).then(|| self.layout.backup(id))
    }

    /// Reads a backup's text, for building a restored tab. Runs on the calling thread and is
    /// safe while commits run: a backup is replaced by rename, never rewritten in place.
    pub fn read_backup(&self, id: &str) -> Result<String, StoreError> {
        let path = self
            .backup_path(id)
            .ok_or_else(|| StoreError::InvalidId(id.to_owned()))?;
        let bytes = fs::read(&path).map_err(|error| StoreError::io("read", &path, error))?;
        Ok(String::from_utf8(bytes).unwrap_or_else(|error| {
            tracing::warn!(path = %path.display(), "backup is not valid UTF-8; invalid bytes replaced");
            String::from_utf8_lossy(error.as_bytes()).into_owned()
        }))
    }

    /// Queues a snapshot and returns at once.
    pub fn commit(&self, snapshot: Snapshot) -> Result<(), StoreError> {
        self.requests
            .send(Request::Commit(Box::new(snapshot)))
            .map_err(|_| StoreError::WriterStopped)
    }

    /// Blocks until every commit queued so far is on disk, trying a failed write once more.
    /// For quitting and for SIGTERM and SIGHUP.
    pub fn flush(&self, timeout: Duration) -> Result<(), StoreError> {
        self.ask(Request::Flush, Some(timeout))
    }

    /// Deletes every backup and orphan, and rewrites the manifest from the newest session
    /// without drafts ([`Session::without_drafts`]). Commits queued before it are written
    /// first; texts a failed write left queued are dropped. Drop the drafts from the UI before
    /// calling this, and send no snapshot that still holds them. Blocks until done.
    pub fn forget_drafts(&self) -> Result<(), StoreError> {
        self.ask(Request::ForgetDrafts, None)
    }

    /// The writer's progress and last error, without waiting for it.
    pub fn status(&self) -> StoreStatus {
        self.shared.status()
    }

    /// Flushes, stops the writer and releases the lock. After a timeout the writer finishes in
    /// the background, keeping the lock until it does, and this returns `Timeout`.
    pub fn close(mut self, timeout: Duration) -> Result<(), StoreError> {
        let result = self.flush(timeout);
        let writer = self.writer.take();
        let _ = self.requests.send(Request::Shutdown);
        if !matches!(result, Err(StoreError::Timeout))
            && let Some(writer) = writer
        {
            let _ = writer.join();
        }
        result
    }

    fn ask<T>(
        &self,
        request: impl FnOnce(Reply<T>) -> Request,
        timeout: Option<Duration>,
    ) -> Result<T, StoreError> {
        let (reply, response) = mpsc::channel();
        self.requests
            .send(request(reply))
            .map_err(|_| StoreError::WriterStopped)?;
        let received = match timeout {
            Some(timeout) => response.recv_timeout(timeout).map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => StoreError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => StoreError::WriterStopped,
            }),
            None => response.recv().map_err(|_| StoreError::WriterStopped),
        };
        received?
    }
}

impl fmt::Debug for SessionStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionStore")
            .field("state_dir", &self.layout.root())
            .finish_non_exhaustive()
    }
}

impl Drop for SessionStore {
    /// Lets the writer finish what is queued, then waits for it; use [`SessionStore::close`]
    /// for a bounded wait.
    fn drop(&mut self) {
        if let Some(writer) = self.writer.take() {
            let _ = self.requests.send(Request::Shutdown);
            let _ = writer.join();
        }
    }
}

/// A new tab or backup id: a UUIDv7, so ids sort by creation time.
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// A file's fingerprint, from `fs::metadata(path)` or from the handle a load read: the one
/// [`crate::fs`] records at load and save.
pub use crate::fs::fingerprint_of as fingerprint;
