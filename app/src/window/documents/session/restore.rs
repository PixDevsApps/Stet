//! Restoring the session at startup: the store opens on a worker, the window, histories and
//! recent files come back, and every tab returns as a stub.

use crate::editor::{DocState, EditorPage};
use crate::runtime::{Notify, wait_until, worker};
use crate::window::Window;
use libadwaita::prelude::*;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use stet_domain::document::next_untitled_number;
use stet_domain::nav::RecentFiles;
use stet_domain::search::{HISTORY_LIMIT, History, SearchHistory};
use stet_domain::session::{RecordId, Session, TabRecord};
use stet_domain::text::line_count;
use stet_domain::view::Zoom;
use stet_infrastructure::recent as recent_store;
use stet_infrastructure::session_store::{Orphan, Restored, SessionStore, StoreError};

use super::{Phase, RestoreStats};

/// How long a starting instance retries a locked store
/// ([`stet_infrastructure::session_store::SessionStore::open_waiting`]): a quitting instance
/// holds the lock until it has flushed, which takes at most
/// [`FLUSH_TIMEOUT`](super::FLUSH_TIMEOUT).
const LOCK_RETRY: Duration = Duration::from_secs(2);

impl Window {
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
