//! Backups: the timer, the commit of the session with the texts that are due, the records
//! `session.json` keeps, and Forget Unsaved Drafts.

use crate::convert::{BufferPositions, IterPosition};
use crate::editor::EditorPage;
use crate::runtime::worker;
use crate::window::Window;
use gtk4::glib;
use libadwaita::prelude::*;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use stet_domain::search::History;
use stet_domain::session::{
    BackupAction, BackupState, RecordId, SCHEMA_VERSION, Session, TabRecord, ViewSlot, WindowState,
};
use stet_domain::text::{CharOffset, Line};
use stet_infrastructure::encoding::entry_for;
use stet_infrastructure::session_store::Snapshot;

use super::{FLUSH_TIMEOUT, Phase, SnapshotStats};

/// Backup texts by id, for a commit.
type Texts = Vec<(RecordId, Arc<str>)>;

impl Window {
    /// Starts, or restarts, the backup timer: every `backup_interval` seconds of config.toml
    /// (7 by default), none with `backups = false`.
    pub(super) fn start_backups(&self) {
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
            (CharOffset::ZERO, None)
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
            (CharOffset::ZERO, None)
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
}

/// The first visible line, or the restored one while the view was never shown.
fn first_visible_line(page: &EditorPage) -> Line {
    if let Some(line) = page.top_line() {
        return line;
    }
    let view = page.view();
    let rect = view.visible_rect();
    let (iter, _) = view.line_at_y(rect.y());
    iter.buffer_line()
}
