//! Changes made to open files by other programs (M3). Each file has a monitor (renames into
//! its place included, debounced 300 ms), and every open file is checked again when the
//! window gets focus, for file systems that send no events. A change is a fingerprint that no
//! longer matches the baseline ([`Fingerprint::matches`], which ignores the device number).
//! A document without unsaved changes reloads quietly, as one undoable step; otherwise a
//! banner asks. Nothing here switches tabs or takes focus.

use super::Window;
use super::files::Caret;
use crate::banner::BannerKind;
use crate::editor::{DiskState, EditorPage, TextOrigin};
use crate::loader::{self, CHUNK_BYTES, DocInfo, ReadFile};
use crate::monitor::FileWatch;
use crate::worker;
use gtk4::prelude::*;
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use stet_domain::marks::Footprint;
use stet_domain::session::Fingerprint;
use stet_domain::text::{LONG_LINE_CHARS, LineIndex, TextEdit, minimal_edit};
use stet_infrastructure::fs::{EncodingChoice, LoadError, fingerprint};

/// Focus checks closer together than this are skipped.
const FOCUS_CHECK_INTERVAL: Duration = Duration::from_secs(1);

/// What a reload read, ready to apply.
enum Reloaded {
    /// The smallest edit from the buffer to the file, `None` when the text is the same, and
    /// the old text's lines when bookmarks must follow it (M7).
    Edit {
        info: DocInfo,
        edit: Option<TextEdit>,
        index: Option<LineIndex>,
    },
    /// The text goes in through the load path: long lines, or a large file.
    Full(ReadFile),
}

impl Window {
    /// Watches `page`'s file from now on, replacing an earlier watch.
    pub(super) fn watch_file(&self, page: &EditorPage) {
        let Some(path) = page.state().canonical.or_else(|| page.path()) else {
            page.set_watch(None);
            return;
        };
        let inner = std::rc::Rc::downgrade(&self.inner);
        let weak = page.downgrade();
        let watch = FileWatch::start(&path, move || {
            if let (Some(inner), Some(page)) = (inner.upgrade(), weak.upgrade()) {
                let window = Window { inner };
                let target = window.clone();
                window.spawn(async move { target.check_files(vec![page]).await });
            }
        });
        page.set_watch(watch);
    }

    /// Checks every open file again, when the window gets focus.
    pub fn check_all_files(&self) {
        let now = Instant::now();
        if self
            .inner
            .last_focus_check
            .get()
            .is_some_and(|last| now - last < FOCUS_CHECK_INTERVAL)
        {
            return;
        }
        self.inner.last_focus_check.set(Some(now));
        let pages = self.documents();
        let window = self.clone();
        self.spawn(async move { window.check_files(pages).await });
    }

    /// Compares the files of `pages` with their baselines, with one `stat` each on a worker.
    pub(super) async fn check_files(&self, pages: Vec<EditorPage>) {
        let checked: Vec<(EditorPage, PathBuf, Fingerprint)> = pages
            .into_iter()
            .filter_map(|page| {
                let state = page.state();
                let baseline = state.baseline?;
                let path = state.path?;
                (!state.loading && !state.saving).then_some((page, path, baseline))
            })
            .collect();
        if checked.is_empty() {
            return;
        }
        let paths: Vec<PathBuf> = checked.iter().map(|(_, path, _)| path.clone()).collect();
        let Some(results) = worker::run(move || {
            paths
                .iter()
                .map(|path| fingerprint(path))
                .collect::<Vec<io::Result<Fingerprint>>>()
        })
        .await
        else {
            return;
        };
        for ((page, path, baseline), result) in checked.into_iter().zip(results) {
            let state = page.state();
            if state.loading || state.saving || state.baseline != Some(baseline) {
                continue;
            }
            if state.path.as_deref() != Some(path.as_path()) || self.tab_page(&page).is_none() {
                continue;
            }
            match result {
                Ok(now) if baseline.matches(&now) => {
                    if matches!(state.disk, DiskState::Deleted { .. }) {
                        self.file_returned(&page, now).await;
                    }
                }
                Ok(now) => self.file_changed(&page, now).await,
                Err(error) if error.kind() == io::ErrorKind::NotFound => self.file_deleted(&page),
                Err(error) => tracing::debug!(%error, "could not check an open file"),
            }
        }
    }

    async fn file_changed(&self, page: &EditorPage, now: Fingerprint) {
        let state = page.state();
        let clean_after_delete = match state.disk {
            DiskState::Deleted { clean_revision } => clean_revision == Some(page.revision()),
            _ => false,
        };
        let clean = clean_after_delete || (!page.is_dirty() && state.disk == DiskState::Same);
        if clean {
            if self.reload(page, true).await {
                self.toast(&format!(
                    "“{}” changed on disk and was reloaded",
                    page.name()
                ));
            }
            return;
        }
        if state.disk != DiskState::Changed {
            page.update_state(|state| state.disk = DiskState::Changed);
            self.refresh_banners(page);
        }
        tracing::debug!(
            size = now.size,
            "an open file with unsaved changes changed on disk"
        );
    }

    /// The file is back unchanged after it was gone: nothing to reload.
    async fn file_returned(&self, page: &EditorPage, now: Fingerprint) {
        let clean = matches!(
            page.state().disk,
            DiskState::Deleted { clean_revision } if clean_revision == Some(page.revision())
        );
        page.update_state(|state| {
            state.disk = DiskState::Same;
            state.baseline = Some(now);
        });
        if clean {
            page.buffer().set_modified(false);
        }
        self.refresh_banners(page);
        self.refresh_page(page);
    }

    fn file_deleted(&self, page: &EditorPage) {
        if matches!(page.state().disk, DiskState::Deleted { .. }) {
            return;
        }
        let clean_revision = (!page.is_dirty()).then(|| page.revision());
        page.update_state(|state| state.disk = DiskState::Deleted { clean_revision });
        page.buffer().set_modified(true);
        self.refresh_banners(page);
        self.refresh_page(page);
    }

    /// Reload from Disk: asks first when that replaces unsaved changes.
    pub(super) async fn reload_requested(&self, page: &EditorPage) {
        if page.is_loading() || page.path().is_none() {
            return;
        }
        let name = page.name();
        if page.state().baseline.is_none() {
            self.toast(&format!("“{name}” isn't on disk yet"));
            return;
        }
        if page.is_dirty() {
            let state = page.state();
            let body = if !state.large() && state.origin == TextOrigin::File {
                "Your unsaved changes are replaced by the file on disk; Undo brings them back."
            } else {
                "Your unsaved changes are lost."
            };
            let confirmed = self
                .confirm(
                    &format!("Reload “{name}” from disk?"),
                    body,
                    "_Reload",
                    true,
                )
                .await;
            if !confirmed {
                return;
            }
        }
        if self.reload(page, false).await {
            self.toast(&format!("Reloaded “{name}”"));
        }
    }

    /// Keeps the buffer as it is and takes the file on disk as the new baseline; the next
    /// save overwrites it.
    pub(super) fn keep_mine(&self, page: &EditorPage) {
        let Some(path) = page.path() else {
            return;
        };
        let window = self.clone();
        let page = page.clone();
        self.spawn(async move {
            let now = worker::run(move || fingerprint(&path)).await;
            if let Some(Ok(now)) = now {
                page.update_state(|state| {
                    state.baseline = Some(now);
                    state.disk = DiskState::Same;
                });
            }
            window.refresh_banners(&page);
        });
    }

    /// Reads the file again and puts it in the buffer as one undoable step, keeping the caret
    /// and the scroll position: only the part that differs is replaced. Large files and text
    /// that went through the long-line path load again instead, without undo. With `quiet`,
    /// edits made while the file was read cancel the reload; the banner asks instead.
    pub(super) async fn reload(&self, page: &EditorPage, quiet: bool) -> bool {
        let Some(path) = page.path() else {
            return false;
        };
        if page.is_loading() {
            return false;
        }
        let state = page.state();
        let choice = state.chosen;
        if state.large() || state.origin != TextOrigin::File {
            return self.reload_full(page, choice).await;
        }
        let old = page.text();
        let revision = page.revision();
        let bookmarked = page.has_bookmarks();
        page.set_loading(true);
        let result = worker::run(move || -> Result<Reloaded, LoadError> {
            let read = loader::read(&path, choice)?;
            if read.longest.chars > LONG_LINE_CHARS || read.info.class != state.class {
                return Ok(Reloaded::Full(read));
            }
            let edit = minimal_edit(&old, &read.text);
            let index = (bookmarked && edit.is_some()).then(|| LineIndex::new(&old));
            Ok(Reloaded::Edit {
                info: read.info,
                edit,
                index,
            })
        })
        .await;
        page.set_loading(false);
        if self.tab_page(page).is_none() {
            return false;
        }
        if page.revision() != revision {
            if quiet {
                page.update_state(|state| state.disk = DiskState::Changed);
                self.refresh_banners(page);
            }
            return false;
        }
        match result {
            Some(Ok(Reloaded::Edit { info, edit, index })) => {
                self.apply_reload(page, info, edit, index).await;
                true
            }
            Some(Ok(Reloaded::Full(read))) => {
                let caret = Caret::Keep(page.cursor(), page.scroll_position());
                self.fill(page, read, state.origin, caret).await
            }
            Some(Err(LoadError::NotFound { .. })) => {
                self.file_deleted(page);
                false
            }
            Some(Err(error)) => {
                self.toast(&format!("Could not reload “{}”: {error}", page.name()));
                false
            }
            None => false,
        }
    }

    /// Replaces the changed part of the buffer as one user action, so Undo brings back what
    /// was there, and takes the file's format and fingerprint.
    async fn apply_reload(
        &self,
        page: &EditorPage,
        info: DocInfo,
        edit: Option<TextEdit>,
        index: Option<LineIndex>,
    ) {
        let buffer = page.buffer();
        let caret = page.cursor();
        let scroll = page.scroll_position();
        if let Some(edit) = edit {
            page.set_loading(true);
            let begun = page.marks().begin(&buffer);
            buffer.begin_user_action();
            let mut start = buffer.iter_at_offset(edit.start as i32);
            let mut end = buffer.iter_at_offset(edit.end as i32);
            buffer.delete(&mut start, &mut end);
            let mark = buffer.create_mark(None, &start, false);
            let mut first = true;
            for piece in stet_infrastructure::fs::text_chunks(&edit.insert, CHUNK_BYTES) {
                if !first {
                    gtk4::glib::timeout_future(crate::editor::CHUNK_INTERVAL).await;
                }
                first = false;
                let mut at = buffer.iter_at_mark(&mark);
                buffer.insert(&mut at, piece);
            }
            buffer.delete_mark(&mark);
            buffer.end_user_action();
            let edits = std::slice::from_ref(&edit);
            page.marks().finish(
                &buffer,
                begun,
                index.as_ref(),
                edits,
                None,
                Footprint::of(edits),
            );
            page.retag_marks();
            page.set_loading(false);
            page.place_cursor(caret);
            page.set_scroll_position(scroll);
        }
        page.update_state(|state| super::files::apply_info(state, &info, TextOrigin::File));
        page.set_read_only(page.read_only());
        buffer.set_modified(false);
        page.hide_banner(BannerKind::Disk);
        self.refresh_banners(page);
        self.refresh_page(page);
    }

    /// Loads the file into `page` again from scratch (Reinterpret, a large file, the long-line
    /// path), keeping the caret's line and column. There is no undo for it.
    pub(super) async fn reload_full(
        &self,
        page: &EditorPage,
        choice: Option<EncodingChoice>,
    ) -> bool {
        let Some(path) = page.path() else {
            return false;
        };
        let origin = page.state().origin;
        let caret = Caret::Keep(page.cursor(), page.scroll_position());
        page.set_loading(true);
        page.update_load_stats(|stats| {
            *stats = crate::editor::LoadStats {
                started: Some(Instant::now()),
                ..Default::default()
            };
        });
        let result = worker::run(move || loader::read(&path, choice)).await;
        if self.tab_page(page).is_none() {
            return false;
        }
        match result {
            Some(Ok(read)) => {
                if self.fill(page, read, origin, caret).await {
                    page.update_state(|state| state.chosen = choice);
                    true
                } else {
                    page.set_loading(false);
                    false
                }
            }
            Some(Err(LoadError::NotFound { .. })) => {
                page.set_loading(false);
                self.file_deleted(page);
                false
            }
            Some(Err(error)) => {
                page.set_loading(false);
                self.toast(&format!("Could not reload “{}”: {error}", page.name()));
                false
            }
            None => {
                page.set_loading(false);
                false
            }
        }
    }
}
