//! The session in the window (ADR-006).
//!
//! - **Backups:** every `backup_interval` seconds of config.toml (7 by default) the tabs, the
//!   window and the histories are committed to the store, with the text of every dirty or
//!   untitled document that changed since its last backup ([`BackupState::action`] has the
//!   large-document rules). Taking those texts is the only part that runs on the GTK thread;
//!   the store writes on its own thread.
//! - **Restore:** at startup the session comes back. The active tab loads at once; every other
//!   tab is a stub (its title, dirty marker and path) that loads when it is first shown, or
//!   when an operation needs its text: from its backup when it had unsaved changes, else from
//!   its file through the file pipeline.
//! - **Quitting** (closing the window, Quit, SIGTERM, SIGHUP, SIGINT) commits one last time,
//!   waits up to [`FLUSH_TIMEOUT`] for the disk and goes, without asking, except about
//!   documents in large-file mode with unsaved changes, which have no backups.
//! - **`--wait`** command lines are answered when the tabs they opened close, with status 1
//!   when one closed with unsaved changes. Their tabs stay out of the session.
//! - **Split view**: each tab records its view; a document's first tab is its record, and
//!   a clone in the other view a record of its own that names it (`clone_of`), with its own
//!   caret and scroll position. The divider's position, the other view's tab and the bookmarks
//!   come back too. Texts a comparison opened stay out.

use crate::convert::{BufferPositions, IterPosition};
use crate::editor::BannerButton;
use crate::editor::load;
use crate::editor::load::LoadStats;
use crate::editor::{DocState, EditorPage};
use crate::runtime::{Notify, connect_once, wait_until, worker};
use crate::window::Window;
use crate::window::dialogs::SaveChoice;
use crate::window::documents::files::Caret;
use gtk4::{gio, glib};
use libadwaita::prelude::*;
use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use stet_domain::document::banners::BannerKind;
use stet_domain::document::language::HEAD_BYTES;
use stet_domain::document::{DiskState, Loss, TextOrigin, next_untitled_number};
use stet_domain::nav::RecentFiles;
use stet_domain::search::{HISTORY_LIMIT, History, SearchHistory};
use stet_domain::session::{
    BackupAction, BackupState, RecordId, SCHEMA_VERSION, Session, TabRecord, ViewSlot, WindowState,
};
use stet_domain::text::line_count;
use stet_domain::view::Zoom;
use stet_infrastructure::encoding::{DocumentFormat, encoding_for_name, entry_for, entry_for_id};
use stet_infrastructure::fs::{
    EncodingChoice, LoadError, SizeClass, SizePolicy, file_meta, read_file,
};
use stet_infrastructure::recent as recent_store;
#[cfg(feature = "selftest")]
use stet_infrastructure::session_store::StoreStatus;
use stet_infrastructure::session_store::{Orphan, Restored, SessionStore, Snapshot, StoreError};

/// How long a starting instance retries a locked store
/// ([`stet_infrastructure::session_store::SessionStore::open_waiting`]): a quitting instance
/// holds the lock until it has flushed, which takes at most [`FLUSH_TIMEOUT`].
const LOCK_RETRY: Duration = Duration::from_secs(2);

/// How long quitting, and SIGTERM, SIGHUP and SIGINT, wait for the last commit to reach the
/// disk.
const FLUSH_TIMEOUT: Duration = Duration::from_secs(2);

/// A mark at the first visible line of a restored tab, scrolled to once the view is laid out.
const TOP_MARK: &str = "stet-restore-top";

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

/// Backup texts by id, for a commit.
type Texts = Vec<(RecordId, Arc<str>)>;

/// A `--wait` command line and how many of its tabs are still open.
struct WaitGroup {
    id: u64,
    command_line: gio::ApplicationCommandLine,
    open: usize,
    unsaved: bool,
}

impl WaitGroup {
    /// Lets the caller go: status 1 when a tab closed with unsaved changes, so that git
    /// aborts instead of committing a stale message. Dropping the command line sends the
    /// answer.
    fn answer(self, unsaved: bool) {
        self.command_line
            .set_exit_code(glib::ExitCode::from(u8::from(unsaved)));
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
    // ----- startup ---------------------------------------------------------------------------

    /// Opens the store and restores the last session in the background; with `enabled` false
    /// (`--no-session`) the window keeps no session.
    pub fn start_session(&self, enabled: bool) {
        if !enabled {
            self.inner.session.set_phase(Phase::Off);
            return;
        }
        let dirs = self.inner.shared.config.dirs.clone();
        let window = self.clone();
        self.spawn(async move {
            let state_dir = dirs.state.clone();
            let recent_file = dirs.recent_file();
            let opened = worker::run(move || {
                let store = SessionStore::open_waiting(&state_dir, LOCK_RETRY)?;
                let restored = store.load()?;
                // Older versions kept the recent files in recent.json; the session holds them now.
                let recent = (restored.session.recent_files.is_empty() && recent_file.exists())
                    .then(|| (recent_store::load(&recent_file), recent_file));
                Ok::<_, StoreError>((store, restored, recent))
            })
            .await;
            match opened {
                Some(Ok((store, restored, recent))) => {
                    window.restore(Arc::new(store), restored, recent);
                }
                Some(Err(error)) => {
                    tracing::warn!(%error, "the session is off in this window");
                    window.toast(&unavailable_message(&error));
                    window.inner.session.set_phase(Phase::Off);
                }
                None => window.inner.session.set_phase(Phase::Off),
            }
        });
    }

    /// Resolves once the session is restored or off, and the current tab shows text: it is
    /// loaded or, while it loads in pieces, the first piece is in. Or once `limit` passed.
    pub async fn session_ready(&self, limit: Duration) {
        let session = &self.inner.session;
        wait_until(
            None,
            || session.phase.get() != Phase::Pending,
            || session.changed.notified(),
        )
        .await;
        wait_until(
            Some(limit),
            || self.current_page().is_none_or(|page| page.shows_text()),
            || match self.current_page() {
                Some(page) => Notify::any(&[&session.changed, page.load_changed()]),
                None => Notify::any(&[&session.changed]),
            },
        )
        .await;
    }

    /// Puts the session back: the window, the histories, the recent files, a stub per tab,
    /// the orphans as untitled tabs; then selects the active tab, which loads it.
    fn restore(
        &self,
        store: Arc<SessionStore>,
        restored: Restored,
        recent: Option<(RecentFiles, PathBuf)>,
    ) {
        let read = Instant::now();
        let Restored {
            session,
            source,
            missing_backups,
            orphans,
        } = restored;
        tracing::info!(
            ?source,
            tabs = session.tabs.len(),
            orphans = orphans.len(),
            "restoring"
        );
        self.inner.session.store.replace(Some(store));

        self.restore_state(&session, recent);

        let initial = self.pages();
        let missing: BTreeSet<&RecordId> = missing_backups.iter().collect();
        // A document's record comes before its clones'.
        let mut pages: Vec<Option<EditorPage>> = Vec::with_capacity(session.tabs.len());
        for record in &session.tabs {
            let page = match &record.clone_of {
                Some(of) => session
                    .tabs
                    .iter()
                    .zip(&pages)
                    .find(|(other, _)| other.clone_of.is_none() && other.id == *of)
                    .and_then(|(_, page)| page.clone())
                    .map(|source| self.add_clone_stub(&source, record)),
                None => Some(self.add_stub(record, missing.contains(&record.id))),
            };
            pages.push(page);
        }
        let orphan_count = orphans.len();
        let orphan_pages: Vec<EditorPage> = orphans
            .into_iter()
            .map(|orphan| self.add_orphan(orphan))
            .collect();
        let restored_pages: Vec<EditorPage> = pages.iter().flatten().cloned().collect();
        let tab =
            |index: Option<usize>| index.and_then(|index| pages.get(index).cloned().flatten());
        let active = tab(session.active_tab)
            .or_else(|| restored_pages.first().cloned())
            .or_else(|| orphan_pages.first().cloned());
        if let Some(active) = active {
            // Selected before the start's empty tab goes, so no other stub loads meanwhile.
            self.select(&active);
            for page in initial {
                if page.is_pristine() {
                    self.close_without_prompt(&page);
                }
            }
            if let Some(other) = tab(session.other_tab)
                && self.view_of(&other) != self.view_of(&active)
            {
                self.show_in_view(&other);
            }
        }
        self.inner.session.restore.set(Some(RestoreStats {
            tabs: restored_pages.len(),
            orphans: orphan_count,
            read,
            built: Instant::now(),
        }));
        if orphan_count > 0 {
            self.toast(&if orphan_count == 1 {
                "Recovered an unsaved document that was not in the last session".to_owned()
            } else {
                format!(
                    "Recovered {orphan_count} unsaved documents that were not in the last session"
                )
            });
        }
        self.inner.session.set_phase(Phase::On);
        self.start_backups();
        self.update_actions();
    }

    /// Puts back what the session keeps besides the tabs: the recent files (from
    /// `recent.json` when it was migrated), the histories, the window's size, the zoom and
    /// the divider.
    fn restore_state(&self, session: &Session, recent: Option<(RecentFiles, PathBuf)>) {
        let recent_files = match recent {
            Some((recent, file)) => {
                self.inner.session.migrated_recent.replace(Some(file));
                recent
            }
            None => RecentFiles::new(session.recent_files.iter().cloned()),
        };
        self.inner.shared.set_recent(&recent_files);
        let history = |entries: &[String]| History::from_entries(entries.to_vec(), HISTORY_LIMIT);
        self.set_search_history(SearchHistory {
            find: history(&session.find_history),
            replace: history(&session.replace_history),
            directories: history(&session.directory_history),
            filters: history(&session.filter_history),
        });
        let window = session.window;
        if window.width > 0 && window.height > 0 {
            self.inner
                .window
                .set_default_size(window.width, window.height);
        }
        if window.maximized {
            self.inner.window.maximize();
        }
        self.inner
            .shared
            .appearance
            .set_zoom(Zoom::new(window.zoom));
        self.inner
            .split
            .position
            .set((window.split_position > 0).then_some(window.split_position));
    }

    /// A tab for `record`, in its view, that loads its text when first shown.
    fn add_stub(&self, record: &TabRecord, backup_missing: bool) -> EditorPage {
        let state = match &record.path {
            Some(path) => DocState::file(path.clone()),
            None => {
                let number = record.untitled_number.unwrap_or_else(|| {
                    next_untitled_number(
                        self.pages()
                            .into_iter()
                            .filter_map(|page| page.state().untitled),
                    )
                });
                let mut state = DocState::untitled(number);
                state.custom_name.clone_from(&record.custom_name);
                state
            }
        };
        let page = self.add_page_in(record.view.index(), state, false);
        let lost = backup_missing || (record.dirty && record.backup.is_none());
        page.update_session(|session| {
            session.id = record.id.clone();
            session.backup = record.backup.is_some();
            session.lost_changes = lost;
            session.stub = Some(Box::new(record.clone()));
        });
        page.set_top_line(Some(record.first_visible_line));
        page.set_pending_record(record.clone());
        if record.backup.is_some() {
            page.buffer().set_modified(true);
        }
        self.refresh_page(&page);
        if record.pinned {
            self.set_pinned(&page, true);
        }
        page
    }

    /// A clone of `source`'s document for `record`, in its view.
    fn add_clone_stub(&self, source: &EditorPage, record: &TabRecord) -> EditorPage {
        let clone = source.clone_view(
            self.inner.shared.appearance.scheme().as_ref(),
            self.inner.settings.get(),
        );
        clone.set_clone_id(record.id.clone());
        clone.set_top_line(Some(record.first_visible_line));
        clone.set_pending_record(record.clone());
        self.attach_page(record.view.index(), &clone, false);
        if record.pinned {
            self.set_pinned(&clone, true);
        }
        clone
    }

    /// Puts `page` in front of its view without making that view the active one.
    pub(in crate::window) fn show_in_view(&self, page: &EditorPage) {
        if let Some((index, tab_page)) = self.locate(page) {
            self.inner.views[index].tabs.set_selected_page(&tab_page);
        }
    }

    /// An untitled tab with an orphan's text, adopted by the next commit.
    fn add_orphan(&self, orphan: Orphan) -> EditorPage {
        let number = next_untitled_number(
            self.pages()
                .into_iter()
                .filter_map(|page| page.state().untitled),
        );
        let page = self.add_page_with(DocState::untitled(number), false);
        page.update_session(|session| session.orphan = Some(orphan.path));
        page.set_loading(true);
        let window = self.clone();
        let target = page.clone();
        self.spawn(async move {
            let lines = line_count(&orphan.text);
            window
                .put_restored_text(&target, orphan.text, lines, None)
                .await;
            window.refresh_page(&target);
        });
        page
    }

    // ----- loading restored tabs -----------------------------------------------------------

    /// A restored tab that has not loaded its text yet.
    pub fn is_stub(page: &EditorPage) -> bool {
        page.session().stub.is_some()
    }

    /// Whether any tab still waits to load its text.
    pub(in crate::window) fn has_stubs(&self) -> bool {
        self.pages().iter().any(Self::is_stub)
    }

    /// A tab is shown: a stub loads its text.
    pub(in crate::window) fn session_page_selected(&self, page: &EditorPage) {
        self.inner.session.changed.notify();
        if Self::is_stub(page) && !page.is_loading() {
            let window = self.clone();
            let page = page.clone();
            self.spawn(async move { window.load_stub(&page).await });
        }
    }

    /// Loads `page` if it is a stub and waits until its text is in. False when it closed.
    pub(in crate::window) async fn ensure_loaded(&self, page: &EditorPage) -> bool {
        self.session_page_selected(page);
        wait_until(
            None,
            || !Self::is_stub(page) || self.tab_page(page).is_none(),
            || page.load_changed().notified(),
        )
        .await;
        self.tab_page(page).is_some()
    }

    /// Loads every stub, for operations over all open documents.
    pub async fn load_stubs(&self) {
        for page in self.documents() {
            if Self::is_stub(&page) {
                self.ensure_loaded(&page).await;
            }
        }
    }

    /// Unsaved changes, also in a stub that has not loaded them from its backup yet.
    pub(in crate::window) fn has_unsaved_changes(page: &EditorPage) -> bool {
        page.is_dirty()
            || page
                .session()
                .stub
                .as_ref()
                .is_some_and(|record| record.backup.is_some())
    }

    /// Loads a stub's text: from its backup, else from its file, else nothing (untitled).
    async fn load_stub(&self, page: &EditorPage) {
        let Some(record) = page.session().stub.clone() else {
            return;
        };
        if page.is_loading() {
            return;
        }
        page.set_loading(true);
        page.update_load_stats(|stats| {
            *stats = LoadStats {
                started: Some(Instant::now()),
                ..LoadStats::default()
            }
        });
        self.refresh_page(page);
        let store = self.inner.session.store.borrow().clone();
        let from_backup = match (&record.backup, store) {
            (Some(id), Some(store)) => self.restore_backup(page, &record, id, store).await,
            _ => false,
        };
        if self.tab_page(page).is_none() {
            return;
        }
        let kept = if from_backup {
            true
        } else if let Some(path) = record.path.clone() {
            self.restore_file(page, &record, path).await
        } else {
            page.set_loading(false);
            true
        };
        if !kept || self.tab_page(page).is_none() {
            return;
        }
        if !record.bookmarks.is_empty() {
            page.set_bookmarks(&record.bookmarks.iter().copied().collect());
        }
        page.update_session(|session| session.stub = None);
        self.restore_banner(page);
        self.refresh_banners(page);
        self.refresh_page(page);
        self.update_actions();
        if self.current_page().as_ref() == Some(page) {
            self.restore_focus();
        }
    }

    /// Puts a backup's text into `page`, with the file's state now: a file that changed since
    /// the backup gets the conflict banner, a deleted one the deleted banner. False when the
    /// backup could not be read; the tab then loads from its file and shows that its changes
    /// were lost.
    async fn restore_backup(
        &self,
        page: &EditorPage,
        record: &TabRecord,
        id: &str,
        store: Arc<SessionStore>,
    ) -> bool {
        let id = id.to_owned();
        let path = record.path.clone();
        let read = worker::run(move || {
            let text = store.read_backup(&id).map(|text| {
                let lines = line_count(&text);
                (text, lines)
            });
            (text, path.as_deref().map(file_meta))
        })
        .await;
        // A backup that can't be read is left alone: no longer referenced, the next start
        // finds it among the orphans.
        let (text, lines, meta) = match read {
            Some((Ok((text, lines)), meta)) => (text, lines, meta),
            failed => {
                if let Some((Err(error), _)) = failed {
                    tracing::warn!(%error, "a backup could not be read");
                }
                page.update_session(|session| {
                    session.lost_changes = true;
                    session.backup = false;
                });
                return false;
            }
        };
        if self.tab_page(page).is_none() {
            return true;
        }
        let mut conflict = false;
        page.update_state(|state| {
            let default = DocumentFormat::default();
            state.save.format = DocumentFormat {
                encoding: record
                    .encoding
                    .as_deref()
                    .and_then(encoding_for_name)
                    .unwrap_or(default.encoding),
                bom: record.bom.unwrap_or(false),
                eol: record.eol.unwrap_or(default.eol),
                map_placeholder: true,
            };
            state.chosen = record
                .chosen_encoding
                .as_deref()
                .and_then(entry_for_id)
                .map(EncodingChoice::from);
            // Saving still can't write the file's bytes back; the save asks first.
            state.loss = record.lossy.then_some(Loss::NotRoundTrip);
            state.class = if text.len() as u64 >= SizePolicy::default().large_from {
                SizeClass::Large
            } else {
                SizeClass::Normal
            };
            match &meta {
                Some(Ok(meta)) => {
                    let now = meta.fingerprint();
                    conflict = record
                        .disk_fingerprint
                        .is_none_or(|before| !before.matches(&now));
                    state.canonical = Some(meta.canonical.clone());
                    // An unresolved conflict keeps the old baseline, so it is still one after
                    // the next restart; Keep Mine or Load Disk Version settles it.
                    state.baseline = if conflict {
                        record.disk_fingerprint
                    } else {
                        Some(now)
                    };
                    state.disk = if conflict {
                        DiskState::Changed
                    } else {
                        DiskState::Same
                    };
                    if !meta.writable {
                        state.deny_writes();
                    }
                }
                Some(Err(error)) if error.kind() == io::ErrorKind::NotFound => {
                    state.baseline = None;
                    if record.disk_fingerprint.is_some() {
                        state.disk = DiskState::Deleted {
                            clean_revision: None,
                        };
                    }
                }
                _ => {}
            }
        });
        page.update_session(|session| session.conflict = conflict);
        let language = record
            .language
            .as_deref()
            .and_then(|id| sourceview5::LanguageManager::default().language(id));
        self.put_restored_text(page, text, lines, language).await;
        page.update_session(|session| {
            session.backup = true;
            session.backup_revision = Some(page.revision());
            session.backed_up_at = Some(Instant::now());
        });
        Self::restore_places(page);
        if record.path.is_some() {
            self.watch_file(page);
        }
        true
    }

    /// Inserts restored unsaved text in pieces, as a file loads, and leaves it modified.
    async fn put_restored_text(
        &self,
        page: &EditorPage,
        text: String,
        lines: usize,
        language: Option<sourceview5::Language>,
    ) {
        let total = text.len();
        let head = text[..text.floor_char_boundary(HEAD_BYTES)].to_owned();
        let started = page.load_stats().started.unwrap_or_else(Instant::now);
        page.begin_load(started);
        if !page.feed(load::stream(text), total).await {
            return;
        }
        let language = language.or_else(|| Self::guess_language(page, &head));
        page.set_language(language.as_ref());
        page.limit_highlighting(!self.settings().highlight_by_default(total, lines));
        page.apply_settings(self.inner.settings.get());
        page.end_load();
        page.buffer().set_modified(true);
    }

    /// Opens a restored tab's file through the file pipeline, in the encoding the user had
    /// picked. False when the tab was closed instead.
    async fn restore_file(&self, page: &EditorPage, record: &TabRecord, path: PathBuf) -> bool {
        let choice = record
            .chosen_encoding
            .as_deref()
            .and_then(entry_for_id)
            .map(EncodingChoice::from);
        let target = path.clone();
        let result = worker::run(move || read_file(&target, choice)).await;
        if self.tab_page(page).is_none() {
            return false;
        }
        let name = page.name();
        match result {
            Some(Ok(read)) => {
                if !self
                    .fill(page, read, TextOrigin::File, Caret::Open(None))
                    .await
                {
                    if self.tab_page(page).is_some() {
                        self.close_without_prompt(page);
                    }
                    return false;
                }
                page.set_encoding_choice(choice);
                if let Some(language) = record
                    .language
                    .as_deref()
                    .and_then(|id| sourceview5::LanguageManager::default().language(id))
                {
                    page.set_language(Some(&language));
                }
                Self::restore_places(page);
                true
            }
            Some(Err(LoadError::NotFound { .. })) if page.session().lost_changes => {
                // Nothing on disk and nothing restored: an empty document for the path, with
                // the banner saying what was lost.
                page.set_loading(false);
                self.detect_language(page, "");
                true
            }
            Some(Err(LoadError::NotFound { .. })) => {
                self.toast(&format!(
                    "“{name}” from the last session is no longer on disk"
                ));
                self.close_without_prompt(page);
                false
            }
            Some(Err(error)) => {
                self.toast(&format!("Could not open “{name}”: {error}"));
                self.close_without_prompt(page);
                false
            }
            None => {
                self.toast(&format!("Could not open “{name}”"));
                self.close_without_prompt(page);
                false
            }
        }
    }

    /// Every tab of `page`'s document takes the caret and scroll position it was restored
    /// with, now that the text is in.
    fn restore_places(page: &EditorPage) {
        for view_page in page.document_pages() {
            if let Some(record) = view_page.take_pending_record() {
                Self::restore_position(&view_page, &record);
            }
        }
    }

    /// The caret, the selection with its direction, and the first visible line.
    fn restore_position(page: &EditorPage, record: &TabRecord) {
        let buffer = page.buffer();
        let (anchor, caret) = record.anchor_and_caret(buffer.total_chars());
        page.place_selection(anchor, caret);
        let top = buffer.iter_at_line_or_last(record.first_visible_line);
        let name = format!("{TOP_MARK}-{}", page.clone_id());
        let mark = match buffer.mark(&name) {
            Some(mark) => {
                buffer.move_mark(&mark, &top);
                mark
            }
            None => buffer.create_mark(Some(&name), &top, true),
        };
        page.view().scroll_to_mark(&mark, 0.0, true, 0.0, 0.0);
        page.set_top_line(Some(record.first_visible_line));
        // Until the view is shown it has no scroll position of its own to save.
        connect_once(
            page.view(),
            |view, run| view.connect_map(run),
            glib::clone!(
                #[weak]
                page,
                move |_| page.set_top_line(None)
            ),
        );
    }

    /// The banner for unsaved changes that could not be restored, on every tab of the document.
    fn restore_banner(&self, page: &EditorPage) {
        for view_page in page.document_pages() {
            self.restore_page_banner(&view_page);
        }
    }

    fn restore_page_banner(&self, page: &EditorPage) {
        if !page.session().lost_changes {
            page.hide_banner(BannerKind::Restore);
            return;
        }
        let window = self.downgrade();
        let weak = page.downgrade();
        let dismiss = BannerButton::new("Dismiss", move || {
            if let (Some(window), Some(page)) = (window.upgrade(), weak.upgrade()) {
                page.update_session(|session| session.lost_changes = false);
                window.restore_banner(&page);
                window.restore_focus_later();
            }
        });
        page.show_banner(
            BannerKind::Restore,
            &format!(
                "The unsaved changes to “{}” from the last session could not be restored.",
                page.name()
            ),
            vec![dismiss],
        );
    }

    // ----- backups ---------------------------------------------------------------------------

    /// Starts, or restarts, the backup timer: every `backup_interval` seconds of config.toml
    /// (7 by default), none with `backups = false`.
    fn start_backups(&self) {
        if let Some(old) = self.inner.session.tick.take() {
            old.remove();
        }
        let Some(every) = self.settings().backup_every() else {
            tracing::info!("backups are off");
            return;
        };
        tracing::info!(seconds = every.as_secs(), "backups on");
        let weak = self.downgrade();
        let source = glib::timeout_add_local(every, move || {
            let Some(window) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            window.commit_session(false);
            glib::ControlFlow::Continue
        });
        self.inner.session.tick.replace(Some(source));
    }

    /// config.toml changed `backups` or `backup_interval`: restarts the timer, and commits at
    /// once, so that turning backups off deletes the backups there are.
    pub(in crate::window) fn backup_settings_changed(&self) {
        let session = &self.inner.session;
        if session.phase.get() != Phase::On || session.quitting.get() {
            return;
        }
        self.start_backups();
        self.commit_session(false);
    }

    /// Builds the session and commits it with every text that is due, unless nothing changed
    /// since the last commit; `last`, before quitting, writes every changed text. Returns
    /// whether a commit was sent.
    pub fn commit_session(&self, last: bool) -> bool {
        let session = &self.inner.session;
        if session.phase.get() != Phase::On {
            return false;
        }
        let Some(store) = session.store.borrow().clone() else {
            return false;
        };
        let started = Instant::now();
        let mut stats = SnapshotStats {
            commits: session.stats.get().commits,
            ..SnapshotStats::default()
        };
        let (built, backups, mut adopted) = self.build_session(last, &mut stats);
        let removed = session.removed.take();
        adopted.extend(session.adopted.take());
        let unchanged = backups.is_empty()
            && removed.is_empty()
            && adopted.is_empty()
            && session.last.borrow().as_ref() == Some(&built);
        if unchanged {
            return false;
        }
        let snapshot = Snapshot {
            session: built.clone(),
            backups,
            removed,
            adopted_orphans: adopted,
        };
        if let Err(error) = store.commit(snapshot) {
            tracing::warn!(%error, "could not commit the session");
            return false;
        }
        session.last.replace(Some(built));
        stats.commits += 1;
        stats.build_time = started.elapsed();
        stats.committed_at = Some(Instant::now());
        session.stats.set(stats);
        if stats.texts > 0 {
            tracing::debug!(
                texts = stats.texts,
                bytes = stats.bytes,
                ms = stats.text_time.as_secs_f64() * 1e3,
                "backed up"
            );
        }
        // The old recent.json goes once a commit with the list it gave is on disk.
        if let Some(file) = session.migrated_recent.take() {
            let window = self.clone();
            self.spawn(async move {
                let target = file.clone();
                let removed = worker::run(move || {
                    store.flush(FLUSH_TIMEOUT).is_ok() && std::fs::remove_file(&target).is_ok()
                })
                .await;
                if removed != Some(true) {
                    window.inner.session.migrated_recent.replace(Some(file));
                }
            });
        }
        true
    }

    /// The session as the window has it now, the texts to back up, and the orphans whose
    /// text a backup now holds.
    fn build_session(
        &self,
        last: bool,
        stats: &mut SnapshotStats,
    ) -> (Session, Texts, Vec<PathBuf>) {
        let now = Instant::now();
        let current = self.current_page();
        let other = self
            .view_page(1 - self.active_view())
            .filter(|_| self.is_split());
        let mut tabs = Vec::new();
        let mut backups = Vec::new();
        let mut adopted = Vec::new();
        let mut active_tab = None;
        let mut other_tab = None;
        // The documents recorded so far, by a tab of each and its record's id.
        let mut recorded: Vec<(EditorPage, RecordId)> = Vec::new();
        for page in self.pages() {
            if !page.session().waits.is_empty() || page.state().temporary.is_some() {
                continue;
            }
            if current.as_ref() == Some(&page) {
                active_tab = Some(tabs.len());
            }
            if other.as_ref() == Some(&page) {
                other_tab = Some(tabs.len());
            }
            let view = ViewSlot::from_index(self.view_of(&page).unwrap_or(0));
            if let Some((_, of)) = recorded.iter().find(|(doc, _)| doc.same_document(&page)) {
                tabs.push(self.clone_record(&page, of.clone(), view));
                continue;
            }
            let stub = page.session().stub.clone();
            let record = match stub {
                // A stub keeps its record, with the tab's pinned state, view and name now.
                Some(record) => TabRecord {
                    pinned: self.is_pinned(&page),
                    view,
                    display_name: page.name(),
                    custom_name: record
                        .path
                        .is_none()
                        .then(|| page.state().custom_name)
                        .flatten(),
                    ..*record
                },
                None => {
                    if !page.is_loading() {
                        self.back_up(&page, last, now, stats, &mut backups, &mut adopted);
                    }
                    self.tab_record(&page, view)
                }
            };
            recorded.push((page.clone(), record.id.clone()));
            tabs.push(record);
        }
        let window = &self.inner.window;
        let (width, height) = window.default_size();
        let history = self.search_history();
        let entries = |history: &History| history.entries().to_vec();
        let session = Session {
            schema_version: SCHEMA_VERSION,
            tabs,
            active_tab,
            window: WindowState {
                width,
                height,
                maximized: window.is_maximized(),
                zoom: self.inner.shared.appearance.zoom().points(),
                split_position: self.split_position(),
            },
            recent_files: self.inner.shared.recent().paths().to_vec(),
            find_history: entries(&history.find),
            replace_history: entries(&history.replace),
            directory_history: entries(&history.directories),
            filter_history: entries(&history.filters),
            other_tab,
        };
        (session, backups, adopted)
    }

    /// Takes `page`'s text for its backup when [`BackupState::action`] says so, or drops a
    /// backup it no longer needs.
    fn back_up(
        &self,
        page: &EditorPage,
        last: bool,
        now: Instant,
        stats: &mut SnapshotStats,
        backups: &mut Texts,
        adopted: &mut Vec<PathBuf>,
    ) {
        let state = page.state();
        let session = page.session().clone();
        let buffer = page.buffer();
        let chars = buffer.total_chars();
        let needs_backup = self.settings().backups
            && !session.discarded
            && (page.is_dirty() || (state.path.is_none() && chars > 0));
        let revision = page.revision();
        let backup = BackupState {
            needs_backup,
            changed: !session.backup || session.backup_revision != Some(revision),
            large_file: state.large(),
            chars,
            since_backup: session
                .backed_up_at
                .map(|at| now.saturating_duration_since(at)),
            since_edit: page
                .last_change()
                .map_or(Duration::MAX, |at| now.saturating_duration_since(at)),
        };
        match backup.action(last) {
            BackupAction::Write => {
                let taken = Instant::now();
                let text: Arc<str> = Arc::from(
                    buffer
                        .text(&buffer.start_iter(), &buffer.end_iter(), true)
                        .as_str(),
                );
                let took = taken.elapsed();
                stats.texts += 1;
                stats.bytes += text.len();
                stats.text_time += took;
                stats.longest_text = stats.longest_text.max(took);
                backups.push((session.id.clone(), text));
                page.update_session(|session| {
                    session.backup = true;
                    session.backup_revision = Some(revision);
                    session.backed_up_at = Some(now);
                });
            }
            BackupAction::Keep => {}
            BackupAction::Remove => {
                if session.backup {
                    self.inner
                        .session
                        .removed
                        .borrow_mut()
                        .push(session.id.clone());
                    page.update_session(|session| {
                        session.backup = false;
                        session.backup_revision = None;
                        session.backed_up_at = None;
                    });
                }
            }
        }
        // An orphan's file goes once a backup holds its text, or once nothing needs keeping.
        if session.orphan.is_some() && (page.session().backup || !needs_backup) {
            adopted.extend(page.update_session(|session| session.orphan.take()));
        }
    }

    /// `page` as `session.json` keeps it, in `view`.
    fn tab_record(&self, page: &EditorPage, view: ViewSlot) -> TabRecord {
        let state = page.state();
        let session = page.session().clone();
        let loading = page.is_loading();
        let (caret, selection) = if loading {
            (0, None)
        } else {
            page.own_selection()
        };
        TabRecord {
            id: session.id.clone(),
            path: state.path.clone(),
            display_name: page.name(),
            custom_name: state
                .path
                .is_none()
                .then(|| state.custom_name.clone())
                .flatten(),
            untitled_number: state.path.is_none().then_some(state.untitled).flatten(),
            // A loading tab's unsaved text, if any, is in its backup until it is in again.
            dirty: if loading {
                session.backup
            } else {
                page.is_dirty() && !session.discarded
            },
            backup: session.backup.then(|| session.id.clone()),
            read_only: state.read_only().is_some(),
            encoding: Some(state.save.format.encoding.name().to_owned()),
            bom: Some(state.save.format.bom),
            eol: Some(state.save.format.eol),
            language: page.language().map(|language| language.id().to_string()),
            caret,
            selection,
            first_visible_line: first_visible_line(page),
            pinned: self.is_pinned(page),
            view,
            clone_of: None,
            bookmarks: if loading {
                Vec::new()
            } else {
                page.bookmarks().lines().to_vec()
            },
            disk_fingerprint: state.baseline,
            chosen_encoding: state
                .chosen
                .and_then(|choice| entry_for(choice.encoding, choice.bom))
                .map(stet_infrastructure::encoding::EncodingEntry::id),
            lossy: state.lossy(),
        }
    }

    /// A clone in `view` of the document recorded as `of`: its own place only.
    fn clone_record(&self, page: &EditorPage, of: RecordId, view: ViewSlot) -> TabRecord {
        let (caret, selection) = if page.is_loading() {
            (0, None)
        } else {
            page.own_selection()
        };
        TabRecord {
            id: page.clone_id(),
            path: page.path(),
            display_name: page.name(),
            caret,
            selection,
            first_visible_line: first_visible_line(page),
            pinned: self.is_pinned(page),
            view,
            clone_of: Some(of),
            ..TabRecord::default()
        }
    }

    // ----- closing tabs and the window ------------------------------------------------------

    /// A tab closed: its backup and orphan file go with the next commit, and `--wait`
    /// command lines whose last tab it was are answered (status 1 when it had unsaved
    /// changes).
    pub(in crate::window) fn tab_closed(&self, page: &EditorPage, unsaved: bool) {
        let session = page.session().clone();
        let state = &self.inner.session;
        if session.backup {
            state.removed.borrow_mut().push(session.id.clone());
        }
        if let Some(orphan) = session.orphan {
            state.adopted.borrow_mut().push(orphan);
        }
        if session.waits.is_empty() {
            return;
        }
        let finished: Vec<WaitGroup> = {
            let mut waits = state.waits.borrow_mut();
            for group in waits
                .iter_mut()
                .filter(|group| session.waits.contains(&group.id))
            {
                group.open = group.open.saturating_sub(1);
                group.unsaved |= unsaved;
            }
            let (finished, open) = waits.drain(..).partition(|group| group.open == 0);
            *waits = open;
            finished
        };
        for group in finished {
            let unsaved = group.unsaved;
            group.answer(unsaved);
        }
    }

    /// Holds `command_line` until every page in `pages` closes (`--wait`).
    pub fn add_wait(&self, command_line: gio::ApplicationCommandLine, pages: &[EditorPage]) {
        let mut waited: Vec<EditorPage> = Vec::new();
        for page in pages {
            if !waited.contains(page) && self.tab_page(page).is_some() {
                waited.push(page.clone());
            }
        }
        if waited.is_empty() {
            return;
        }
        let session = &self.inner.session;
        let id = session.next_wait.get();
        session.next_wait.set(id + 1);
        for page in &waited {
            page.update_session(|session| session.waits.push(id));
        }
        session.waits.borrow_mut().push(WaitGroup {
            id,
            command_line,
            open: waited.len(),
            unsaved: false,
        });
    }

    /// `--wait` command lines still waiting.
    #[cfg(feature = "selftest")]
    pub fn waiting(&self) -> usize {
        self.inner.session.waits.borrow().len()
    }

    /// Moves the `--wait` command lines of `from`, a tab about to close in favour of `to`
    /// (the same file through another path), to `to`.
    pub(in crate::window) fn transfer_waits(from: &EditorPage, to: &EditorPage) {
        let waits = from.update_session(|session| std::mem::take(&mut session.waits));
        to.update_session(|session| session.waits.extend(waits));
    }

    /// Answers every `--wait` command line, as the window goes: status 1 where a tab it
    /// waits for has unsaved changes.
    pub(in crate::window) fn answer_waits(&self) {
        let groups = self.inner.session.waits.take();
        for group in groups {
            let unsaved = group.unsaved
                || self.pages().iter().any(|page| {
                    page.session().waits.contains(&group.id) && Self::has_unsaved_changes(page)
                });
            group.answer(unsaved);
        }
    }

    /// Whether closing the window goes through the session: no prompts, a last commit.
    pub(in crate::window) fn session_closes_window(&self) -> bool {
        self.inner.session.phase.get() != Phase::Off
    }

    /// Closing the window with the session on: asks only about unsaved changes that have no
    /// backups (documents in large-file mode, or every document when config.toml turns
    /// backups off), then commits one last time, waits for the disk and goes.
    pub(in crate::window) async fn quit_with_session(&self) {
        let session = &self.inner.session;
        if session.quitting.replace(true) {
            return;
        }
        wait_until(
            None,
            || session.phase.get() != Phase::Pending,
            || session.changed.notified(),
        )
        .await;
        let backups = self.settings().backups;
        let large: Vec<EditorPage> = self
            .documents()
            .into_iter()
            .filter(|page| {
                page.session().waits.is_empty()
                    && (page.large_file_mode() || !backups)
                    && page.is_dirty()
            })
            .collect();
        if !large.is_empty() {
            let names: Vec<String> = large.iter().map(EditorPage::name).collect();
            match self.ask_save_changes(&names).await {
                SaveChoice::Save => {
                    for page in &large {
                        self.select(page);
                        if !self.save(page).await {
                            session.quitting.set(false);
                            return;
                        }
                    }
                }
                SaveChoice::Discard => {
                    for page in &large {
                        page.update_session(|session| session.discarded = true);
                    }
                }
                SaveChoice::Cancel => {
                    session.quitting.set(false);
                    return;
                }
            }
        }
        self.leave().await;
    }

    /// SIGTERM, SIGHUP or SIGINT: commit everything, large documents included, and go. With
    /// backups off nothing keeps unsaved changes, and there is no one to ask.
    pub fn shutdown_for_signal(&self) {
        if self.inner.session.quitting.replace(true) {
            return;
        }
        if !self.settings().backups {
            let unsaved = self
                .documents()
                .iter()
                .filter(|page| page.is_dirty())
                .count();
            if unsaved > 0 {
                tracing::warn!(unsaved, "backups are off: unsaved changes are lost");
            }
        }
        let window = self.clone();
        self.spawn(async move { window.leave().await });
    }

    /// The window disappears, the session is committed and flushed, the `--wait` command
    /// lines are answered and the window is destroyed, which ends the application.
    async fn leave(&self) {
        self.inner.window.set_visible(false);
        self.end_session().await;
        self.answer_waits();
        self.inner.tabs.set_closing_window();
        self.inner.window.destroy();
    }

    /// The last commit, then a wait of at most [`FLUSH_TIMEOUT`] for it to reach the disk,
    /// on a worker. The store is closed, so the lock is free for the next instance.
    async fn end_session(&self) {
        let session = &self.inner.session;
        if let Some(tick) = session.tick.take() {
            tick.remove();
        }
        if session.phase.get() != Phase::On {
            return;
        }
        self.commit_session(true);
        session.set_phase(Phase::Off);
        let Some(store) = session.store.take() else {
            return;
        };
        let started = Instant::now();
        let flushed = worker::run(move || match Arc::try_unwrap(store) {
            Ok(store) => store.close(FLUSH_TIMEOUT),
            Err(store) => store.flush(FLUSH_TIMEOUT),
        })
        .await;
        let ms = started.elapsed().as_secs_f64() * 1e3;
        match flushed {
            Some(Ok(())) => tracing::info!(ms, "the session is saved"),
            Some(Err(error)) => tracing::warn!(%error, ms, "the session may be incomplete"),
            None => tracing::warn!("the session writer failed"),
        }
    }

    // ----- forgetting drafts ----------------------------------------------------------------

    /// Forget Unsaved Drafts: after a confirmation, untitled documents close, files with
    /// unsaved changes go back to what is on disk, and the store deletes every backup and
    /// orphan. `--wait` tabs are left alone.
    pub(in crate::window) fn forget_drafts(&self) {
        let window = self.clone();
        self.spawn(async move {
            if window.inner.session.phase.get() != Phase::On {
                window.toast("This window keeps no session, so it has no drafts to forget");
                return;
            }
            let confirmed = window
                .confirm(
                    "Forget all unsaved drafts?",
                    "Untitled documents close, and files with unsaved changes go back to what \
                     is on disk. Stet deletes the backups it keeps of them. This can't be \
                     undone.",
                    "_Forget Drafts",
                    true,
                )
                .await;
            if confirmed {
                window.drop_drafts().await;
            }
        });
    }

    async fn drop_drafts(&self) {
        for page in self.documents() {
            if !page.session().waits.is_empty() || self.tab_page(&page).is_none() {
                continue;
            }
            if page.path().is_none() {
                self.close_document(&page);
                continue;
            }
            let stub = page.session().stub.clone();
            if let Some(mut record) = stub {
                record.dirty = false;
                record.backup = None;
                page.update_session(|session| {
                    session.stub = Some(record);
                    session.backup = false;
                    session.lost_changes = false;
                });
                page.buffer().set_modified(false);
                self.refresh_page(&page);
                continue;
            }
            if page.is_dirty() && !self.reload(&page, false).await && page.is_dirty() {
                // A file that is gone takes its draft with it.
                self.close_document(&page);
            }
        }
        let session = &self.inner.session;
        self.commit_session(false);
        let Some(store) = session.store.borrow().clone() else {
            return;
        };
        let forgotten = worker::run(move || store.forget_drafts()).await;
        for page in self.documents() {
            page.update_session(|session| {
                session.backup = false;
                session.backup_revision = None;
                session.backed_up_at = None;
                session.orphan = None;
            });
        }
        session.removed.borrow_mut().clear();
        session.adopted.borrow_mut().clear();
        session.last.replace(None);
        match forgotten {
            Some(Ok(())) => self.toast("Forgot every unsaved draft"),
            Some(Err(error)) => self.toast(&format!("Could not forget every draft: {error}")),
            None => self.toast("Could not forget the drafts"),
        }
        self.update_actions();
    }

    // ----- for the self-test -----------------------------------------------------------------

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

/// The first visible line, or the restored one while the view was never shown.
fn first_visible_line(page: &EditorPage) -> usize {
    if let Some(line) = page.top_line() {
        return line;
    }
    let view = page.view();
    let rect = view.visible_rect();
    let (iter, _) = view.line_at_y(rect.y());
    iter.line_number()
}

/// Why the session is off in this window, for a toast.
fn unavailable_message(error: &StoreError) -> String {
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
