use super::fsutil::{remove_file, remove_files_in, sync_dir, write_synced};
use super::layout::Layout;
use super::restore;
use super::{CommitStats, Restored, Snapshot, StoreError, StoreStatus};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io;
use std::mem;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Instant;
use stet_domain::session::{SCHEMA_VERSION, Session, is_valid_id};

pub(super) type Reply<T> = mpsc::Sender<Result<T, StoreError>>;

pub(super) enum Request {
    Commit(Box<Snapshot>),
    Load(Reply<Restored>),
    Flush(Reply<()>),
    ForgetDrafts(Reply<()>),
    Shutdown,
}

/// The status the writer publishes for [`super::SessionStore::status`].
#[derive(Default)]
pub(super) struct Shared(Mutex<StoreStatus>);

impl Shared {
    pub(super) fn status(&self) -> StoreStatus {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn update(&self, change: impl FnOnce(&mut StoreStatus)) {
        change(&mut self.0.lock().unwrap_or_else(PoisonError::into_inner));
    }
}

/// Points in a write where tests inject failures and crashes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Phase {
    /// A backup's temp file is written and synced, not yet renamed.
    BackupTempWritten,
    /// A backup has replaced its previous version.
    BackupRenamed,
    /// Every queued backup is written and `backup/` is synced; the manifest is next.
    BackupsSynced,
    /// The new manifest's temp file is written and synced.
    ManifestTempWritten,
    /// The old manifest is now `session.json.prev`, and `session.json` is missing.
    ManifestRetired,
    /// The new manifest is in place; deletions are next.
    ManifestRenamed,
}

#[cfg(test)]
pub(super) type Hook = Arc<dyn Fn(Phase) -> io::Result<()> + Send + Sync>;

/// Owns the lock and does every write, in commit order, on its own thread.
pub(super) struct BackupWriter {
    layout: Layout,
    /// Held until the thread ends, so no second store writes while this one still might.
    _lock: File,
    shared: Arc<Shared>,
    /// Backup texts not yet on disk, the newest per id.
    pending: BTreeMap<String, Arc<str>>,
    /// Backups to delete once a manifest that doesn't reference them is on disk.
    removals: BTreeSet<String>,
    /// Orphan files to delete once the manifest is on disk.
    adopted: BTreeSet<PathBuf>,
    /// The newest session not yet written.
    session: Option<Session>,
    /// Snapshots folded in since the last completed write.
    snapshots: usize,
    /// The session in `session.json`, once loaded or written.
    written: Option<Session>,
    /// Backups that exist as complete files.
    on_disk: BTreeSet<String>,
    /// Backups were renamed into place since `backup/` was last synced.
    unsynced: bool,
    #[cfg(test)]
    hook: Option<Hook>,
}

impl BackupWriter {
    /// Creates the layout, takes the lock and notes which backups exist.
    pub(super) fn new(layout: Layout) -> Result<Self, StoreError> {
        layout.create()?;
        let lock = layout.acquire_lock()?;
        let on_disk = restore::scan(&layout)?.backups.into_keys().collect();
        Ok(Self {
            layout,
            _lock: lock,
            shared: Arc::default(),
            pending: BTreeMap::new(),
            removals: BTreeSet::new(),
            adopted: BTreeSet::new(),
            session: None,
            snapshots: 0,
            written: None,
            on_disk,
            unsynced: false,
            #[cfg(test)]
            hook: None,
        })
    }

    #[cfg(test)]
    pub(super) fn with_hook(mut self, hook: Hook) -> Self {
        self.hook = Some(hook);
        self
    }

    pub(super) fn layout(&self) -> &Layout {
        &self.layout
    }

    pub(super) fn shared(&self) -> Arc<Shared> {
        Arc::clone(&self.shared)
    }

    /// Serves requests until `Shutdown` or until every sender is gone, then writes what is
    /// still queued. Commits already queued behind one are folded into a single write.
    pub(super) fn run(mut self, requests: mpsc::Receiver<Request>) {
        let mut next = None;
        while let Some(request) = next.take().or_else(|| requests.recv().ok()) {
            match request {
                Request::Commit(snapshot) => {
                    self.absorb(*snapshot);
                    while let Ok(request) = requests.try_recv() {
                        match request {
                            Request::Commit(snapshot) => self.absorb(*snapshot),
                            other => {
                                next = Some(other);
                                break;
                            }
                        }
                    }
                    let _ = self.write();
                }
                Request::Load(reply) => {
                    let _ = reply.send(self.load());
                }
                Request::Flush(reply) => {
                    let _ = reply.send(self.write());
                }
                Request::ForgetDrafts(reply) => {
                    let _ = reply.send(self.forget_drafts());
                }
                Request::Shutdown => break,
            }
        }
        let _ = self.write();
    }

    /// Queues a snapshot. A later text for an id replaces a queued one; removing an id drops
    /// its queued text, and a new text cancels a queued removal.
    fn absorb(&mut self, snapshot: Snapshot) {
        let Snapshot {
            session,
            backups,
            removed,
            adopted_orphans,
        } = snapshot;
        for id in removed {
            if is_valid_id(&id) {
                self.pending.remove(&id);
                self.removals.insert(id);
            } else {
                tracing::warn!("ignored the removal of a backup with an invalid id");
            }
        }
        for (id, text) in backups {
            if is_valid_id(&id) {
                self.removals.remove(&id);
                self.pending.insert(id, text);
            } else {
                tracing::warn!("ignored a backup with an invalid id");
            }
        }
        for path in adopted_orphans {
            if self.layout.is_orphan(&path) {
                self.adopted.insert(path);
            } else {
                tracing::warn!(path = %path.display(), "ignored an adopted orphan outside backup/orphaned/");
            }
        }
        self.session = Some(session);
        self.snapshots += 1;
    }

    /// Writes whatever is queued. On failure the rest stays queued for the next attempt.
    fn write(&mut self) -> Result<(), StoreError> {
        if self.pending.is_empty() && self.session.is_none() {
            return Ok(());
        }
        let started = Instant::now();
        let mut stats = CommitStats {
            snapshots: self.snapshots,
            ..CommitStats::default()
        };
        let result = self.write_queued(&mut stats);
        stats.total_time = started.elapsed();
        match &result {
            Ok(()) => {
                self.snapshots = 0;
                self.shared.update(|status| {
                    status.commits += 1;
                    status.last_commit = Some(stats);
                    status.error = None;
                });
            }
            Err(error) => {
                tracing::warn!(%error, "session write failed; it is retried with the next commit or flush");
                self.shared
                    .update(|status| status.error = Some(error.clone()));
            }
        }
        result
    }

    /// Backups, then the manifest, then deletions: `session.json` never references a backup
    /// that is missing or half-written.
    fn write_queued(&mut self, stats: &mut CommitStats) -> Result<(), StoreError> {
        let started = Instant::now();
        while let Some((id, text)) = self.pending.pop_first() {
            if let Err(error) = self.write_backup(&id, &text) {
                self.pending.insert(id, text);
                return Err(error);
            }
            self.unsynced = true;
            self.on_disk.insert(id);
            stats.backups += 1;
            stats.backup_bytes += text.len() as u64;
        }
        if self.unsynced {
            sync_dir(self.layout.backups())?;
            self.unsynced = false;
            self.checkpoint(Phase::BackupsSynced)?;
        }
        stats.backup_time = started.elapsed();

        let Some(mut session) = self.session.take() else {
            return Ok(());
        };
        let started = Instant::now();
        session.schema_version = SCHEMA_VERSION;
        self.drop_dangling_references(&mut session);
        if let Err(error) = self.write_manifest(&session, true) {
            self.session = Some(session);
            return Err(error);
        }
        stats.manifest_time = started.elapsed();
        self.delete_obsolete(&session);
        self.written = Some(session);
        Ok(())
    }

    /// Temp file in `backup/`, fsync, rename over the previous version.
    fn write_backup(&self, id: &str, text: &str) -> Result<(), StoreError> {
        let temp = self.layout.backup_temp(id);
        let target = self.layout.backup(id);
        let replaced = write_synced(&temp, text.as_bytes())
            .map_err(|error| StoreError::io("write", &temp, error))
            .and_then(|()| self.checkpoint(Phase::BackupTempWritten))
            .and_then(|()| {
                fs::rename(&temp, &target).map_err(|error| StoreError::io("rename", &target, error))
            });
        if replaced.is_err() {
            let _ = remove_file(&temp);
        }
        replaced?;
        self.checkpoint(Phase::BackupRenamed)
    }

    /// Temp file, fsync, the old manifest renamed to `.prev` when `keep_previous`, rename,
    /// directory sync.
    fn write_manifest(&self, session: &Session, keep_previous: bool) -> Result<(), StoreError> {
        let manifest = self.layout.manifest();
        let bytes = serde_json::to_vec_pretty(session)
            .map_err(|error| StoreError::io("encode", &manifest, error.into()))?;
        let temp = self.layout.manifest_temp();
        write_synced(&temp, &bytes).map_err(|error| StoreError::io("write", &temp, error))?;
        self.checkpoint(Phase::ManifestTempWritten)?;
        if keep_previous {
            match fs::rename(&manifest, self.layout.previous()) {
                Err(error) if error.kind() != io::ErrorKind::NotFound => {
                    return Err(StoreError::io("rename", &manifest, error));
                }
                _ => {}
            }
            self.checkpoint(Phase::ManifestRetired)?;
        }
        fs::rename(&temp, &manifest).map_err(|error| StoreError::io("rename", &manifest, error))?;
        self.checkpoint(Phase::ManifestRenamed)?;
        sync_dir(self.layout.root())
    }

    /// Clears references to backups that were never written, so the manifest stays valid even
    /// when the app references a backup it didn't send.
    fn drop_dangling_references(&self, session: &mut Session) {
        for tab in &mut session.tabs {
            if let Some(id) = &tab.backup
                && !self.on_disk.contains(id)
            {
                tracing::warn!(tab = %tab.id, "a tab references a backup that was never written; dropping the reference");
                tab.backup = None;
            }
        }
    }

    /// Deletes removed backups that `session` (now on disk) doesn't reference, and adopted
    /// orphans. A failed deletion leaves a file that the next load treats as an orphan.
    fn delete_obsolete(&mut self, session: &Session) {
        let referenced: BTreeSet<&str> = session.backup_ids().collect();
        for id in mem::take(&mut self.removals) {
            if referenced.contains(id.as_str()) {
                continue;
            }
            let path = self.layout.backup(&id);
            match remove_file(&path) {
                Ok(()) => {
                    self.on_disk.remove(&id);
                }
                Err(error) => {
                    tracing::warn!(path = %path.display(), %error, "could not delete an obsolete backup");
                }
            }
        }
        for path in mem::take(&mut self.adopted) {
            if let Err(error) = remove_file(&path) {
                tracing::warn!(path = %path.display(), %error, "could not delete an adopted orphan");
            }
        }
    }

    fn load(&mut self) -> Result<Restored, StoreError> {
        let _ = self.write();
        let restored = restore::restore(&self.layout)?;
        self.on_disk = restored.session.backup_ids().map(str::to_owned).collect();
        self.written = Some(restored.session.clone());
        Ok(restored)
    }

    /// Writes the newest session without drafts, then deletes every backup and orphan. Queued
    /// work is dropped. The manifest goes first, so a crash in between leaves orphans rather
    /// than references to missing backups.
    fn forget_drafts(&mut self) -> Result<(), StoreError> {
        self.pending.clear();
        self.removals.clear();
        self.adopted.clear();
        self.snapshots = 0;
        let newest = self
            .session
            .take()
            .or_else(|| self.written.clone())
            .unwrap_or_else(|| restore::read_manifests(&self.layout).0);
        let mut session = newest.without_drafts();
        session.schema_version = SCHEMA_VERSION;
        self.write_manifest(&session, false)?;
        self.written = Some(session);
        for path in [self.layout.previous(), self.layout.corrupt()] {
            remove_file(&path).map_err(|error| StoreError::io("delete", &path, error))?;
        }
        for dir in [self.layout.orphans(), self.layout.backups()] {
            remove_files_in(dir)?;
            sync_dir(dir)?;
        }
        self.on_disk.clear();
        self.unsynced = false;
        self.shared.update(|status| status.error = None);
        tracing::info!("forgot all unsaved drafts");
        Ok(())
    }

    fn checkpoint(&self, phase: Phase) -> Result<(), StoreError> {
        #[cfg(test)]
        if let Some(hook) = &self.hook {
            return hook(phase).map_err(|error| StoreError::io("write", self.layout.root(), error));
        }
        let _ = phase;
        Ok(())
    }
}
