//! Opening and saving files (M3). A worker reads and decodes a file (ADR-007, ADR-008); the
//! editor inserts its text in pieces with the view read-only and undo off; saving encodes a
//! snapshot of the buffer on a worker and writes it through the safe save. Only taking the
//! snapshot runs on the GTK thread (ADR-003).

use super::dialogs::{LongLineChoice, Purpose, UnmappableChoice};
use super::{Location, Window};
use crate::editor::{
    DiskState, DocState, EditorPage, ReadOnly, TextOrigin, Unwritable, read_only_message,
};
use crate::loader::{self, DocInfo, FormatKind, ReadFile};
use crate::worker;
use gtk4::gio;
use sourceview5::prelude::*;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use stet_domain::indent;
use stet_domain::language as hints;
use stet_domain::text::{LONG_LINE_CHARS, LongestLine, Position, line_count, longest_line};
use stet_infrastructure::encoding::status_label;
use stet_infrastructure::fs::{
    DocumentFormat, LoadError, MIB, SaveError, SaveOptions, SaveOutcome, remove_stale_temps,
    save_document,
};

/// Temporary files that interrupted saves left next to a file are removed after it is saved
/// again, once they are this old.
const STALE_TEMP_AGE: Duration = Duration::from_secs(60);

/// How much of the text language detection looks at.
const HEAD_BYTES: usize = 4096;

/// Where an in-place save keeps the previous content until the new content is on disk.
const SAVE_BACKUPS_DIR: &str = "save-backups";

/// Where the caret goes once a file's text is in.
#[derive(Debug, Clone, Copy)]
pub(super) enum Caret {
    /// A position from the command line, or the start.
    Open(Option<Position>),
    /// The line and column, and the scroll position, from before a reload.
    Keep(Position, f64),
}

impl Window {
    /// Opens `locations` as tabs, or selects the tabs that show them already, and returns
    /// those tabs in order.
    pub fn open_locations(&self, locations: Vec<Location>) -> Vec<EditorPage> {
        let pages = self.pages();
        let pristine = match pages.as_slice() {
            [only] if only.is_pristine() => Some(only.clone()),
            _ => None,
        };
        let mut opened = Vec::with_capacity(locations.len());
        for location in locations {
            if let Some(page) = self
                .documents()
                .into_iter()
                .find(|page| page.shows(&location.path, None))
            {
                if location.position.is_some() {
                    self.note_jump();
                }
                self.select(&page);
                if let Some(position) = location.position {
                    page.go_to(position.position());
                }
                opened.push(page);
                continue;
            }
            let page = self.add_page(DocState {
                path: Some(location.path.clone()),
                ..DocState::default()
            });
            page.set_loading(true);
            page.update_load_stats(|stats| stats.started = Some(Instant::now()));
            self.refresh_page(&page);
            opened.push(page.clone());
            let window = self.clone();
            self.spawn(async move { window.load(page, location).await });
        }
        if let Some(pristine) = pristine
            && pristine.is_pristine()
            && self.pages().len() > 1
        {
            self.close_without_prompt(&pristine);
        }
        // Opening files is a request to work in them: the command line, the Open dialog, a
        // drop, a recent file or a search result.
        if !self.dialog_open() {
            self.focus_editor();
        }
        opened
    }

    async fn load(&self, page: EditorPage, location: Location) {
        let path = location.path.clone();
        let result = worker::run(move || loader::read(&path, None)).await;
        if self.tab_page(&page).is_none() {
            return;
        }
        let read = match result {
            Some(Ok(read)) => read,
            Some(Err(LoadError::NotFound { .. })) if location.create => {
                page.set_loading(false);
                self.detect_language(&page, "");
                self.refresh_page(&page);
                self.update_actions();
                self.toast(&format!("“{}” is a new file", page.name()));
                return;
            }
            Some(Err(error)) => {
                if matches!(error, LoadError::NotFound { .. }) {
                    self.inner.shared.remove_recent(&location.path);
                }
                self.toast(&open_error(&page.name(), &error));
                self.close_without_prompt(&page);
                return;
            }
            None => {
                self.toast(&format!("Could not open “{}”", page.name()));
                self.close_without_prompt(&page);
                return;
            }
        };
        page.update_load_stats(|stats| stats.decoded = Some(Instant::now()));
        if let Some(existing) = self.documents().into_iter().find(|other| {
            !other.same_document(&page)
                && other.shows(&location.path, Some(&read.info.meta.canonical))
        }) {
            self.transfer_waits(&page, &existing);
            self.close_without_prompt(&page);
            self.select(&existing);
            if let Some(position) = location.position {
                existing.go_to(position.position());
            }
            return;
        }
        let caret = Caret::Open(location.position.map(|position| position.position()));
        if !self.fill(&page, read, TextOrigin::File, caret).await {
            if self.tab_page(&page).is_some() {
                self.close_without_prompt(&page);
            }
            return;
        }
        // A `--wait` file (a commit message, a sudoedit copy) stays out of the session (M2).
        if page.session().waits.is_empty() {
            self.inner.shared.add_recent(location.path.clone());
        }
        // A file that finished loading in front takes the focus back, but not from the find
        // bar or a popover the user went to meanwhile.
        if self.current_page().as_ref() == Some(&page) {
            self.restore_focus();
        }
    }

    /// Puts a decoded file into `page`: asks how to open long lines (a reload keeps the
    /// earlier answer, `keep`), inserts the text in pieces, and sets the document model, the
    /// language, highlighting (ADR-014) and the banners. Returns false when the user
    /// cancelled or the tab closed; the buffer is untouched until the text is ready.
    pub(super) async fn fill(
        &self,
        page: &EditorPage,
        read: ReadFile,
        keep: TextOrigin,
        caret: Caret,
    ) -> bool {
        let ReadFile {
            info,
            text,
            longest,
            lines,
        } = read;
        let (text, lines, origin) = if longest.chars > LONG_LINE_CHARS {
            match self.long_line_text(page, text, longest, keep).await {
                Some(prepared) => prepared,
                None => return false,
            }
        } else {
            (text, lines, TextOrigin::File)
        };
        if self.tab_page(page).is_none() {
            return false;
        }
        let total = text.len();
        let head = text[..text.floor_char_boundary(HEAD_BYTES)].to_owned();
        let previous = matches!(caret, Caret::Keep(..))
            .then(|| page.language())
            .flatten();
        let started = page.load_stats().started.unwrap_or_else(Instant::now);
        page.begin_load(started);
        page.update_state(|state| apply_info(state, &info, origin));
        // Now the document knows whether it is in large-file mode: an open find bar must not
        // highlight every match while the text streams in.
        if self.current_page().as_ref() == Some(page) {
            self.sync_search_highlight();
        }
        let detected = indent::detect(indent::sample(&text));
        if !page.feed(loader::stream(text), total).await {
            return false;
        }
        let language = match origin {
            TextOrigin::Formatted(kind) => {
                sourceview5::LanguageManager::default().language(kind.language())
            }
            _ => previous.or_else(|| self.guess_language(page, &head)),
        };
        let large = page.large_file_mode();
        let limited = !self.settings().highlight_by_default(total, lines);
        page.update_state(|state| state.highlight_limited = limited && !large);
        page.set_language(language.as_ref());
        page.set_highlighting(!large && !limited);
        page.apply_settings(self.inner.settings.get());
        self.indentation_after_load(page, detected);
        page.end_load();
        match caret {
            Caret::Open(Some(position)) => page.go_to(position),
            Caret::Open(None) => {
                let buffer = page.buffer();
                buffer.place_cursor(&buffer.start_iter());
            }
            Caret::Keep(position, scroll) => {
                page.place_cursor(position);
                page.set_scroll_position(scroll);
            }
        }
        self.watch_file(page);
        self.refresh_banners(page);
        self.refresh_page(page);
        self.update_actions();
        true
    }

    /// The long-line path (S1): the text formatted or broken for display, with its line
    /// count, on a worker; `None` when the user cancelled.
    async fn long_line_text(
        &self,
        page: &EditorPage,
        text: String,
        longest: LongestLine,
        keep: TextOrigin,
    ) -> Option<(String, usize, TextOrigin)> {
        let kind = FormatKind::sniff(&text);
        let choice = match keep {
            TextOrigin::Formatted(_) if kind.is_some() => LongLineChoice::Formatted,
            TextOrigin::DisplayBreaks => LongLineChoice::DisplayBreaks,
            _ => self.ask_long_lines(&page.name(), longest, kind).await,
        };
        let breaks = |text: String| {
            let text = loader::display_breaks(&text);
            let lines = line_count(&text);
            (text, lines, TextOrigin::DisplayBreaks)
        };
        match (choice, kind) {
            (LongLineChoice::Formatted, Some(kind)) => {
                let formatted = worker::run(move || match loader::format(&text, kind) {
                    Ok(formatted) if longest_line(&formatted).chars > LONG_LINE_CHARS => {
                        Ok(breaks(formatted))
                    }
                    Ok(formatted) => {
                        let lines = line_count(&formatted);
                        Ok((formatted, lines, TextOrigin::Formatted(kind)))
                    }
                    Err(error) => Err((error, text)),
                })
                .await?;
                match formatted {
                    Ok(prepared) => Some(prepared),
                    Err((error, text)) => {
                        self.toast(&format!(
                            "“{}” isn't valid {}: {error}. It opens read-only with line breaks.",
                            page.name(),
                            kind.label()
                        ));
                        worker::run(move || breaks(text)).await
                    }
                }
            }
            (LongLineChoice::DisplayBreaks, _) => worker::run(move || breaks(text)).await,
            _ => None,
        }
    }

    /// Opens dropped files as tabs. Returns whether anything was accepted.
    pub fn drop_files(&self, files: Vec<gio::File>) -> bool {
        let locations: Vec<Location> = files
            .into_iter()
            .filter_map(|file| file.path())
            .map(|path| Location {
                path,
                position: None,
                create: false,
            })
            .collect();
        if locations.is_empty() {
            return false;
        }
        self.open_locations(locations);
        true
    }

    pub(super) async fn open_with_dialog(&self) {
        let folder = self
            .current_page()
            .and_then(|page| page.path())
            .and_then(|path| path.parent().map(Path::to_path_buf));
        let paths = self.choose_files_to_open(folder.as_deref()).await;
        self.open_locations(
            paths
                .into_iter()
                .map(|path| Location {
                    path,
                    position: None,
                    create: false,
                })
                .collect(),
        );
    }

    // ----- languages ---------------------------------------------------------------------

    /// Syntax detection: our file-name hints, GtkSourceView's globs, first-line hints
    /// (shebang, modelines), then GtkSourceView by sniffed content type.
    pub(super) fn guess_language(
        &self,
        page: &EditorPage,
        head: &str,
    ) -> Option<sourceview5::Language> {
        let manager = sourceview5::LanguageManager::default();
        let installed = |id: &str| manager.language(id);
        let name = page
            .path()
            .and_then(|path| {
                path.file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_default();
        let first_line = head.lines().next().unwrap_or_default();
        let sample = &head.as_bytes()[..head.len().min(HEAD_BYTES)];
        // An untitled document has no name: GtkSourceView refuses an empty one (a CRITICAL).
        let file_name = (!name.is_empty()).then_some(name.as_str());
        hints::from_file_name(&name)
            .and_then(installed)
            .or_else(|| file_name.and_then(|name| manager.guess_language(Some(name), None)))
            .or_else(|| hints::from_first_line(first_line).and_then(installed))
            .or_else(|| {
                let (content_type, _) = gio::content_type_guess(file_name, sample);
                (!content_type.is_empty())
                    .then(|| manager.guess_language(None::<&str>, Some(content_type.as_str())))
                    .flatten()
            })
    }

    /// Sets the language detected from the file name and `text`.
    pub(super) fn detect_language(&self, page: &EditorPage, text: &str) {
        let head = &text[..text.floor_char_boundary(HEAD_BYTES)];
        let language = self.guess_language(page, head);
        page.set_language(language.as_ref());
        self.refresh_indentation(page);
        self.update_status();
    }

    // ----- saving ------------------------------------------------------------------------

    /// Saves `page`; an untitled page goes through Save As. Returns whether it was saved.
    pub async fn save(&self, page: &EditorPage) -> bool {
        match page.path() {
            _ if !self.can_save(page) => false,
            Some(path) => self.save_to(page, path).await,
            None => self.save_as(page).await,
        }
    }

    pub async fn save_as(&self, page: &EditorPage) -> bool {
        if !self.can_save(page) {
            return false;
        }
        let suggested = self.save_as_name(page);
        match self
            .choose_save_path(page.path().as_deref(), &suggested)
            .await
        {
            Some(path) => self.save_to(page, path).await,
            None => false,
        }
    }

    fn can_save(&self, page: &EditorPage) -> bool {
        if page.is_loading() {
            return false;
        }
        match page.read_only() {
            Some(reason @ (ReadOnly::Binary | ReadOnly::DisplayBreaks)) => {
                self.toast(read_only_message(reason));
                false
            }
            _ => true,
        }
    }

    /// Encodes `page` in its format and writes it to `path` on a worker. The buffer is marked
    /// saved only if it did not change while the write ran. A restored tab loads its text
    /// first.
    pub async fn save_to(&self, page: &EditorPage, path: PathBuf) -> bool {
        self.write_to(page, path, true).await
    }

    /// [`Self::save_to`], with a toast when `announce`; Replace in Files saves the open
    /// documents it changed without one each (M8).
    pub(super) async fn write_to(&self, page: &EditorPage, path: PathBuf, announce: bool) -> bool {
        if !self.ensure_loaded(page).await || !self.can_save(page) {
            return false;
        }
        let same_file = page.path().as_deref() == Some(path.as_path());
        if same_file && !self.confirm_save(page).await {
            return false;
        }
        loop {
            let text = page.text();
            let revision = page.revision();
            let format = page.state().format;
            let options =
                SaveOptions::new(self.inner.shared.config.dirs.state.join(SAVE_BACKUPS_DIR));
            let target = path.clone();
            page.update_state(|state| state.saving = true);
            let result = worker::run(move || {
                let outcome = save_document(&target, &text, &format, &options)?;
                remove_stale_temps(&outcome.target, STALE_TEMP_AGE);
                Ok::<_, SaveError>(outcome)
            })
            .await;
            page.update_state(|state| state.saving = false);
            let name = page.name();
            match result {
                Some(Ok(outcome)) => {
                    self.saved(page, path, outcome, revision, format, announce);
                    return true;
                }
                Some(Err(SaveError::Unmappable(error))) => {
                    match self
                        .ask_unmappable(page, &error, format.bom, Purpose::Save)
                        .await
                    {
                        UnmappableChoice::UseUtf8 => {
                            page.update_state(|state| {
                                state.format.encoding = DocumentFormat::default().encoding;
                                state.format.bom = false;
                                state.format_changed = true;
                            });
                            self.refresh_page(page);
                        }
                        UnmappableChoice::GoTo(offset) => {
                            page.select_char(offset);
                            return false;
                        }
                        UnmappableChoice::Cancel => return false,
                    }
                }
                Some(Err(SaveError::PermissionDenied { .. })) => {
                    page.update_state(|state| state.unwritable = Some(Unwritable::Permission));
                    self.refresh_banners(page);
                    self.toast(&format!("You may not write “{name}”"));
                    return false;
                }
                Some(Err(SaveError::ReadOnlyFilesystem { .. })) => {
                    page.update_state(|state| {
                        state.unwritable = Some(Unwritable::ReadOnlyFilesystem);
                    });
                    self.refresh_banners(page);
                    self.toast(&format!("“{name}” is on a read-only file system"));
                    return false;
                }
                Some(Err(SaveError::Interrupted { backup, source, .. })) => {
                    self.show_error(
                        &format!("“{name}” was only partly saved"),
                        &format!(
                            "Writing it failed partway: {source}. Its previous content is in \
                             {}. Copy that file back to restore it.",
                            backup.display()
                        ),
                    )
                    .await;
                    return false;
                }
                Some(Err(error)) => {
                    self.toast(&format!("Could not save “{name}”: {error}"));
                    return false;
                }
                None => {
                    self.toast(&format!("Could not save “{name}”"));
                    return false;
                }
            }
        }
    }

    /// Asks before a save to the document's own file writes bytes that differ from what was
    /// read in ways the user may not expect.
    async fn confirm_save(&self, page: &EditorPage) -> bool {
        let state = page.state();
        let name = page.name();
        if let TextOrigin::Formatted(kind) = state.origin
            && !state.save_confirmed
        {
            let confirmed = self
                .confirm(
                    &format!("Save the formatted {} to “{name}”?", kind.label()),
                    &format!(
                        "“{name}” was opened formatted, so saving replaces its long lines with \
                         the formatted text."
                    ),
                    "_Save Formatted",
                    false,
                )
                .await;
            if !confirmed {
                return false;
            }
            page.update_state(|state| state.save_confirmed = true);
        }
        if state.lossy() && !state.save_confirmed {
            let encoding = status_label(state.format.encoding, state.format.bom);
            let confirmed = self
                .confirm(
                    &format!("Save “{name}” anyway?"),
                    &format!(
                        "Stet couldn't read all of “{name}” as {encoding}, so saving changes \
                         the bytes it couldn't read. To keep them, cancel and reinterpret the \
                         file in another encoding."
                    ),
                    "_Save Anyway",
                    true,
                )
                .await;
            if !confirmed {
                return false;
            }
            page.update_state(|state| state.save_confirmed = true);
        }
        if state.mixed_eol {
            let eol = state.format.eol.label();
            return self
                .confirm(
                    &format!("Save “{name}” with {eol} line endings?"),
                    &format!(
                        "“{name}” has mixed line endings. Stet keeps one line ending per \
                         document, so saving writes {eol} everywhere."
                    ),
                    "_Save",
                    false,
                )
                .await;
        }
        true
    }

    /// Records a successful save: the new baseline (so the monitor doesn't take the write for
    /// an outside change), the file as it is now, and the recent files.
    fn saved(
        &self,
        page: &EditorPage,
        path: PathBuf,
        outcome: SaveOutcome,
        revision: u64,
        written: DocumentFormat,
        announce: bool,
    ) {
        let renamed = page.path().as_deref() != Some(path.as_path());
        page.update_state(|state| {
            state.path = Some(path.clone());
            state.canonical = Some(outcome.target.clone());
            state.untitled = None;
            state.baseline = Some(outcome.fingerprint);
            state.disk = DiskState::Same;
            state.had_errors = false;
            state.lossy_roundtrip = false;
            state.mixed_eol = false;
            state.mixed_kept = false;
            state.origin = TextOrigin::File;
            state.unwritable = None;
            state.save_confirmed = false;
            if state.read_only == Some(ReadOnly::NotWritable) {
                state.read_only = None;
            }
            if state.format == written {
                state.format_changed = false;
            }
        });
        page.set_read_only(page.read_only());
        if page.revision() == revision {
            page.buffer().set_modified(false);
        }
        if renamed {
            let text = page.text();
            self.detect_language(page, &text);
        }
        self.watch_file(page);
        if page.session().waits.is_empty() {
            self.inner.shared.add_recent(path);
        }
        self.refresh_banners(page);
        self.refresh_page(page);
        self.update_actions();
        if announce {
            self.toast(&format!("Saved “{}”", page.name()));
        }
    }
}

/// Copies what decoding found into the document model.
pub(super) fn apply_info(state: &mut DocState, info: &DocInfo, origin: TextOrigin) {
    state.canonical = Some(info.meta.canonical.clone());
    state.format = DocumentFormat {
        encoding: info.encoding,
        bom: info.bom,
        eol: info.eol,
        map_placeholder: !info.placeholder_conflict,
    };
    state.detection = Some(info.detection.source);
    state.eol_stats = info.eol_stats;
    state.mixed_eol = info.mixed_eol && origin == TextOrigin::File;
    state.mixed_kept = false;
    state.had_errors = info.had_errors;
    state.lossy_roundtrip = info.lossy_roundtrip;
    state.binary = info.binary;
    state.nul_count = info.nul_count;
    state.placeholder_conflict = info.placeholder_conflict;
    state.class = info.class;
    state.origin = origin;
    state.baseline = Some(info.meta.fingerprint());
    state.disk = DiskState::Same;
    state.format_changed = false;
    state.save_confirmed = false;
    state.unwritable = (!info.meta.writable).then_some(Unwritable::Permission);
    state.read_only = if origin == TextOrigin::DisplayBreaks {
        Some(ReadOnly::DisplayBreaks)
    } else if info.binary {
        Some(ReadOnly::Binary)
    } else if !info.meta.writable {
        Some(ReadOnly::NotWritable)
    } else {
        None
    };
}

/// Why a file didn't open, for a toast.
fn open_error(name: &str, error: &LoadError) -> String {
    match error {
        LoadError::NotFound { .. } => format!("“{name}” does not exist"),
        LoadError::PermissionDenied { .. } => format!("You may not read “{name}”"),
        LoadError::IsDirectory { .. } => format!("“{name}” is a folder"),
        LoadError::NotRegularFile { .. } => format!("“{name}” is not a regular file"),
        LoadError::TooLarge { size, refusal, .. } => format!(
            "“{name}” is too large to open ({:.1} MiB): {refusal}",
            *size as f64 / MIB as f64
        ),
        LoadError::Io { source, .. } => format!("Could not read “{name}”: {source}"),
    }
}
