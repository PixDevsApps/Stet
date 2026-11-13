//! The session in the window (ADR-006).
//!
//! - **Backups** ([`backup`]): every `backup_interval` seconds of config.toml (7 by default)
//!   the tabs, the window and the histories are committed to the store, with the text of every
//!   dirty or untitled document that changed since its last backup
//!   ([`BackupState::action`](stet_domain::session::BackupState::action) has the
//!   large-document rules). Taking those texts is the only part that runs on the GTK thread;
//!   the store writes on its own thread.
//! - **Restore** ([`restore`], [`stubs`]): at startup the session comes back. The active tab
//!   loads at once; every other tab is a stub (its title, dirty marker and path) that loads
//!   when it is first shown, or when an operation needs its text: from its backup when it had
//!   unsaved changes, else from its file through the file pipeline.
//! - **Quitting** ([`quit`]: closing the window, Quit, SIGTERM, SIGHUP, SIGINT) commits one
//!   last time, waits up to [`FLUSH_TIMEOUT`] for the disk and goes, without asking, except
//!   about documents in large-file mode with unsaved changes, which have no backups.
//! - **`--wait`** ([`wait`]) command lines are answered when the tabs they opened close, with
//!   status 1 when one closed with unsaved changes. Their tabs stay out of the session.
//! - **Split view**: each tab records its view; a document's first tab is its record, and
//!   a clone in the other view a record of its own that names it (`clone_of`), with its own
//!   caret and scroll position. The divider's position, the other view's tab and the bookmarks
//!   come back too. Texts a comparison opened stay out.

mod backup;
mod quit;
mod restore;
mod stubs;
mod wait;

use crate::runtime::Notify;
#[cfg(feature = "selftest")]
use crate::runtime::worker;
use crate::window::Window;
use gtk4::glib;
#[cfg(feature = "selftest")]
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use stet_domain::session::{RecordId, Session};
use stet_infrastructure::session_store::SessionStore;
#[cfg(feature = "selftest")]
use stet_infrastructure::session_store::StoreStatus;
use wait::WaitGroup;

/// How long quitting, and SIGTERM, SIGHUP and SIGINT, wait for the last commit to reach the
/// disk.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// Whether this window keeps a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    /// Opening the store and restoring.
    #[default]
    Pending,
    On,
    /// `--no-session`, the store could not be opened, or the window is going.
    Off,
}

/// The window's session state.
#[derive(Default)]
pub struct WindowSession {
    phase: Cell<Phase>,
    /// Wakes whoever waits for the session: the phase changed, or another tab is shown.
    changed: Notify,
    store: RefCell<Option<Arc<SessionStore>>>,
    /// The session the last commit sent, to skip commits that would change nothing.
    last: RefCell<Option<Session>>,
    /// Backups of closed tabs, removed by the next commit.
    removed: RefCell<Vec<RecordId>>,
    /// Recovered orphan files whose tab closed, deleted by the next commit.
    adopted: RefCell<Vec<PathBuf>>,
    tick: RefCell<Option<glib::SourceId>>,
    waits: RefCell<Vec<WaitGroup>>,
    next_wait: Cell<u64>,
    quitting: Cell<bool>,
    stats: Cell<SnapshotStats>,
    restore: Cell<Option<RestoreStats>>,
    /// The old `recent.json`, deleted once a commit holds the list it was migrated from.
    migrated_recent: RefCell<Option<PathBuf>>,
}

impl WindowSession {
    fn set_phase(&self, phase: Phase) {
        self.phase.set(phase);
        self.changed.notify();
    }
}

/// What the last commit took from the GTK thread.
#[derive(Debug, Clone, Copy, Default)]
pub struct SnapshotStats {
    /// Commits sent since the window opened.
    pub commits: u64,
    /// Texts taken by the last commit, their bytes, and how long taking them took.
    pub texts: usize,
    pub bytes: usize,
    pub text_time: Duration,
    pub longest_text: Duration,
    /// Building the whole last commit, texts included.
    pub build_time: Duration,
    /// When the last commit was sent to the store.
    pub committed_at: Option<Instant>,
}

/// When the parts of the last restore were done.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(
    not(feature = "selftest"),
    expect(dead_code, reason = "only the self-test reads the figures")
)]
pub struct RestoreStats {
    pub tabs: usize,
    pub orphans: usize,
    /// The store was open and the session read (on a worker).
    pub read: Instant,
    /// Every tab was in the strip.
    pub built: Instant,
}

impl Window {
    /// What the last commit took from the GTK thread.
    #[cfg(feature = "selftest")]
    pub fn session_stats(&self) -> SnapshotStats {
        self.inner.session.stats.get()
    }

    #[cfg(feature = "selftest")]
    pub fn restore_stats(&self) -> Option<RestoreStats> {
        self.inner.session.restore.get()
    }

    pub fn session_phase(&self) -> Phase {
        self.inner.session.phase.get()
    }

    /// The store writer's progress, while the session is on.
    #[cfg(feature = "selftest")]
    pub fn store_status(&self) -> Option<StoreStatus> {
        self.inner
            .session
            .store
            .borrow()
            .as_ref()
            .map(|store| store.status())
    }

    /// Waits until every commit sent so far is on disk, on a worker.
    #[cfg(feature = "selftest")]
    pub async fn flush_session(&self) -> Result<(), String> {
        let Some(store) = self.inner.session.store.borrow().clone() else {
            return Err("the session is off".to_owned());
        };
        worker::run(move || store.flush(FLUSH_TIMEOUT))
            .await
            .ok_or("the worker failed")?
            .map_err(|error| error.to_string())
    }

    /// Whether every document that needs a backup has its current text in one.
    #[cfg(feature = "selftest")]
    pub fn backups_current(&self) -> bool {
        self.documents().iter().all(|page| {
            let session = page.session();
            if session.stub.is_some() || page.is_loading() || !session.waits.is_empty() {
                return true;
            }
            let state = page.state();
            let needs = !session.discarded
                && (page.is_dirty() || (state.path.is_none() && page.buffer().char_count() > 0));
            !needs
                || state.large()
                || (session.backup && session.backup_revision == Some(page.revision()))
        })
    }
}
