//! Loading restored tabs: a stub loads its text from its backup or its file when it is first
//! shown, or when an operation needs it.

use crate::convert::BufferPositions;
use crate::editor::BannerButton;
use crate::editor::EditorPage;
use crate::editor::load;
use crate::editor::load::LoadStats;
use crate::runtime::{connect_once, wait_until, worker};
use crate::window::Window;
use crate::window::documents::files::Caret;
use gtk4::glib;
use libadwaita::prelude::*;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use stet_domain::document::banners::BannerKind;
use stet_domain::document::language::HEAD_BYTES;
use stet_domain::document::{DiskState, Loss, TextOrigin};
use stet_domain::session::TabRecord;
use stet_domain::text::line_count;
use stet_infrastructure::encoding::{DocumentFormat, encoding_for_name, entry_for_id};
use stet_infrastructure::fs::{
    EncodingChoice, LoadError, SizeClass, SizePolicy, file_meta, read_file,
};
use stet_infrastructure::session_store::SessionStore;

/// A mark at the first visible line of a restored tab, scrolled to once the view is laid out.
const TOP_MARK: &str = "stet-restore-top";

impl Window {
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
    pub(super) async fn put_restored_text(
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
}
