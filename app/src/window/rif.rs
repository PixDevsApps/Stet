//! Replace in Files (M8, ADR-005 amendment): the files row's other command. After a
//! confirmation that names the folder, the filters and that files on disk can't be undone, a
//! run on threads of its own (`stet_infrastructure::replace_in_files`) replaces in every file
//! the walk reaches, keeping each file's encoding, byte-order mark and line endings, and
//! reports each file to the results panel, with the reason for those it left alone. Open
//! documents get the change in their buffer instead, as one undoable edit (one bulk edit above
//! 2,000 replacements): a document without unsaved changes is then saved, so the disk matches
//! the other files and Undo still takes the change back; one with unsaved changes keeps it
//! unsaved.

use super::Window;
use super::find::{BarMode, Field, Tone};
use super::results::{FileNode, FileTarget, SearchNode};
use crate::editor::{EditorPage, TextOrigin};

use crate::worker;
use gtk4::glib;
use gtk4::prelude::*;
use libadwaita as adw;
use libadwaita::prelude::*;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};
use stet_domain::actions::ActionId;
use stet_domain::document::tilde;
use stet_domain::replace_files::{DocumentOutcome, Skip, file_summary, heading};
use stet_domain::search::Template;
use stet_domain::search::report::counted;
use stet_infrastructure::find_in_files;
use stet_infrastructure::fs::fingerprint;
use stet_infrastructure::replace_in_files::{
    self as rif, FILE_HIT_LIMIT, RifEvent, RifHandle, RifRequest, RifSummary, replaced_hits,
};
use stet_infrastructure::search::{Matcher, SearchError};

/// How often the run's files reach the panel.
const DRAIN_INTERVAL: Duration = Duration::from_millis(33);
/// About the most result lines added per drain.
const LINES_PER_DRAIN: usize = 1_000;
/// The most changed lines a run lists, as Find in Files lists at most this many hits; files
/// past it are listed with their counts only.
const LISTED_LINES: usize = find_in_files::DEFAULT_MAX_HITS;
/// How long an open document's replacement waits for it to load.
const LOAD_WAIT: Duration = Duration::from_secs(30);
/// Where an in-place save keeps the previous content (the same folder the editor's saves use).
const SAVE_BACKUPS_DIR: &str = "save-backups";

/// A Replace in Files run in progress.
pub struct RifRun {
    id: u64,
    handle: RifHandle,
    receiver: async_channel::Receiver<RifEvent>,
    search: Rc<SearchNode>,
    pattern: String,
    replacement: String,
    folder: String,
    started: Instant,
    matcher: Arc<Matcher>,
    template: Template,
    cancel: Arc<AtomicBool>,
    stop_requested: Option<Instant>,
    /// Open documents the walk reached, still to replace in.
    open_queue: VecDeque<PathBuf>,
    open_busy: bool,
    totals: Totals,
    /// Changed lines listed so far, up to [`LISTED_LINES`].
    listed: usize,
    /// Files listed without their lines: past [`LISTED_LINES`], or after Stop.
    unlisted_files: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Totals {
    replacements: usize,
    /// Files and open documents changed.
    files: usize,
    skipped: usize,
    failed: usize,
    open_documents: usize,
}

/// How the last run went, for the self-test's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RifStats {
    pub total: Duration,
    pub summary: RifSummary,
    pub replacements: usize,
    pub files: usize,
    pub skipped: usize,
    pub failed: usize,
    pub open_documents: usize,
    pub stopped_after: Option<Duration>,
}

impl Window {
    /// Replace in Files: needs the files row; asks first, then runs.
    pub(super) fn replace_in_files(&self) {
        let find = &self.inner.find;
        if !(find.is_open() && find.bar_mode() == BarMode::Files) {
            self.show_find_mode(BarMode::Files);
            return;
        }
        if self.replace_in_files_running() {
            find.set_message("Replace in Files is already running", Tone::Info);
            return;
        }
        if find.compiled().is_none() {
            find.compile();
        }
        let Some(compiled) = find.compiled() else {
            if find.pattern_error().is_none() {
                find.set_message("Type what to search for", Tone::Info);
            }
            return;
        };
        let template = match self.template() {
            Ok(template) => template,
            Err(message) => {
                find.set_message(&message, Tone::Error);
                return;
            }
        };
        let folder_text = find.directory.text().trim().to_owned();
        if folder_text.is_empty() {
            find.set_message("Choose a folder to replace in", Tone::Error);
            return;
        }
        let replacement = find.replace_entry.text().to_string();
        let filters = find.filters.text().trim().to_owned();
        find.remember(Field::Find, &compiled.query.pattern);
        find.remember(Field::Replace, &replacement);
        find.remember(Field::Directory, &folder_text);
        find.remember(Field::Filters, &filters);
        let folder = super::fif::expand_home(&folder_text);
        let window = self.clone();
        self.spawn(async move {
            let is_folder = worker::run({
                let folder = folder.clone();
                move || folder.is_dir()
            })
            .await
            .unwrap_or(false);
            if !is_folder {
                window
                    .inner
                    .find
                    .set_message(&format!("“{folder_text}” is not a folder"), Tone::Error);
                return;
            }
            let pattern = compiled.query.pattern.clone();
            if !window
                .ask_replace_in_files(&pattern, &replacement, &folder_text, &filters)
                .await
            {
                return;
            }
            window.run_replace_in_files(
                Arc::clone(&compiled.matcher),
                compiled.query.clone(),
                template,
                (pattern, replacement),
                folder,
                folder_text,
                filters,
            );
        });
    }

    async fn ask_replace_in_files(
        &self,
        pattern: &str,
        replacement: &str,
        folder: &str,
        filters: &str,
    ) -> bool {
        let find = &self.inner.find;
        let files = if filters.is_empty() || filters == "*.*" {
            "every file".to_owned()
        } else {
            format!("the files matching {filters}")
        };
        let subfolders = if find.subfolders.is_active() {
            "with subfolders"
        } else {
            "without subfolders"
        };
        let hidden = if find.hidden.is_active() {
            "hidden files included"
        } else {
            "hidden files left out"
        };
        let ignored = if find.gitignore.is_active() {
            "what .gitignore ignores left out"
        } else {
            "what .gitignore ignores included"
        };
        let body = format!(
            "Every match of “{pattern}” is replaced with “{replacement}” in {files} in {folder} \
             ({subfolders}, {hidden}, {ignored}).\n\nFiles on disk are changed in place, and \
             that can't be undone. Open documents get one edit each, which Undo takes back."
        );
        let dialog = adw::AlertDialog::new(Some("Replace in Files?"), Some(&body));
        dialog.add_responses(&[("cancel", "_Cancel"), ("replace", "_Replace in Files")]);
        dialog.set_response_appearance("replace", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        self.ask(&dialog).await == "replace"
    }

    #[allow(clippy::too_many_arguments)]
    fn run_replace_in_files(
        &self,
        matcher: Arc<Matcher>,
        query: stet_domain::search::Query,
        template: Template,
        (pattern, replacement): (String, String),
        folder: PathBuf,
        folder_text: String,
        filters: String,
    ) {
        self.stop_find_in_files();
        let find = &self.inner.find;
        let backups = self.inner.shared.config.dirs.state.join(SAVE_BACKUPS_DIR);
        let mut request = RifRequest::new(vec![folder], query, template.clone(), backups);
        request.filters = filters;
        request.include_hidden = find.hidden.is_active();
        request.respect_gitignore = find.gitignore.is_active();
        request.include_subfolders = find.subfolders.is_active();
        request.open_documents = self
            .documents()
            .iter()
            .filter_map(|page| {
                let state = page.state();
                state.canonical.or(state.path)
            })
            .collect();
        let started = Instant::now();
        let (sender, receiver) = async_channel::unbounded::<RifEvent>();
        let handle = match rif::start(request, move |event| {
            let _ = sender.send_blocking(event);
        }) {
            Ok(handle) => handle,
            Err(error) => {
                find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        let results = &self.inner.results;
        let search = results.begin(heading(&pattern, &replacement, 0, 0, None, 0));
        results
            .status
            .set_label(&format!("Replacing in {folder_text}…"));
        results.status.set_tooltip_text(None);
        results.stop.set_visible(true);
        if let Some(action) = self.action(ActionId::StopFindInFiles) {
            action.set_enabled(true);
        }
        find.set_message("Replacing in files…", Tone::Info);
        self.show_results(false);
        let nav = &self.inner.nav;
        let id = nav.rif_runs.get() + 1;
        nav.rif_runs.set(id);
        nav.rif_stats.replace(None);
        nav.rif.replace(Some(RifRun {
            id,
            handle,
            receiver,
            search,
            pattern,
            replacement,
            folder: folder_text,
            started,
            matcher,
            template,
            cancel: Arc::new(AtomicBool::new(false)),
            stop_requested: None,
            open_queue: VecDeque::new(),
            open_busy: false,
            totals: Totals::default(),
            listed: 0,
            unlisted_files: 0,
        }));
        glib::timeout_add_local(
            DRAIN_INTERVAL,
            glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || Window { inner }.drain_replace_in_files(id)
            ),
        );
    }

    /// Moves reported files into the panel, starts the next open document, and finishes the
    /// run once the walk, every event and every open document are done.
    fn drain_replace_in_files(&self, id: u64) -> glib::ControlFlow {
        let nav = &self.inner.nav;
        let mut slot = nav.rif.borrow_mut();
        let Some(run) = slot.as_mut().filter(|run| run.id == id) else {
            return glib::ControlFlow::Break;
        };
        let finished = run.handle.is_finished();
        let home = std::env::home_dir();
        let mut files = Vec::new();
        let mut lines = 0;
        while lines < LINES_PER_DRAIN {
            let Ok(event) = run.receiver.try_recv() else {
                break;
            };
            let label = |path: &Path| tilde(path, home.as_deref());
            match event {
                RifEvent::Replaced {
                    path,
                    replacements,
                    written,
                    hits,
                    ..
                } => {
                    run.totals.replacements += replacements;
                    let mut summary = file_summary(replacements, None);
                    if written {
                        run.totals.files += 1;
                    } else {
                        summary.push_str(", already as replaced");
                    }
                    // After Stop, the files written before it are listed at once.
                    let unlisted = !hits.is_empty()
                        && (run.listed + hits.len() > LISTED_LINES || run.stop_requested.is_some());
                    let hits = if unlisted {
                        run.unlisted_files += 1;
                        summary.push_str(", lines not listed");
                        Vec::new()
                    } else {
                        hits
                    };
                    run.listed += hits.len();
                    lines += hits.len().max(1);
                    files.push(FileNode {
                        label: label(&path),
                        target: FileTarget::Path(path),
                        hits,
                        summary: Some(summary),
                    });
                }
                RifEvent::Skipped {
                    path,
                    reason,
                    matches,
                } => {
                    run.totals.skipped += 1;
                    lines += 1;
                    files.push(skipped_node(label(&path), path, &reason, matches));
                }
                RifEvent::Failed { path, message } => {
                    run.totals.failed += 1;
                    lines += 1;
                    files.push(FileNode {
                        label: label(&path),
                        target: FileTarget::Path(path),
                        hits: Vec::new(),
                        summary: Some(format!("failed: {message}")),
                    });
                }
                RifEvent::OpenDocument { path } => run.open_queue.push_back(path),
            }
        }
        if !files.is_empty() {
            self.inner.results.add_files(&run.search, files);
            self.update_rif_heading(run, None);
        }
        let progress = run.handle.progress();
        if !finished {
            self.inner.results.status.set_label(&format!(
                "Replacing in {}… {} searched, {} changed",
                run.folder,
                counted(progress.files_searched, "file", "files"),
                counted(run.totals.files, "file", "files"),
            ));
        }
        if !run.open_busy
            && run.stop_requested.is_none()
            && let Some(path) = run.open_queue.pop_front()
        {
            run.open_busy = true;
            let matcher = Arc::clone(&run.matcher);
            let template = run.template.clone();
            let cancel = Arc::clone(&run.cancel);
            let window = self.clone();
            self.spawn(async move {
                let node = window
                    .replace_in_open_document(path, matcher, template, cancel)
                    .await;
                window.open_document_done(id, node);
            });
        }
        let done = finished
            && run.receiver.is_empty()
            && !run.open_busy
            && (run.open_queue.is_empty() || run.stop_requested.is_some());
        if !done {
            return glib::ControlFlow::Continue;
        }
        let run = slot.take().expect("checked above");
        drop(slot);
        self.finish_replace_in_files(run);
        glib::ControlFlow::Break
    }

    fn open_document_done(&self, id: u64, node: Option<(FileNode, Totals)>) {
        let mut slot = self.inner.nav.rif.borrow_mut();
        let Some(run) = slot.as_mut().filter(|run| run.id == id) else {
            return;
        };
        run.open_busy = false;
        if let Some((node, totals)) = node {
            run.totals.replacements += totals.replacements;
            run.totals.files += totals.files;
            run.totals.skipped += totals.skipped;
            run.totals.failed += totals.failed;
            run.totals.open_documents += 1;
            self.inner.results.add_files(&run.search, vec![node]);
            self.update_rif_heading(run, None);
        }
        drop(slot);
        // The next open document, or the end of the run, needn't wait for the next drain.
        self.drain_replace_in_files(id);
    }

    fn update_rif_heading(&self, run: &RifRun, searched: Option<usize>) {
        let text = heading(
            &run.pattern,
            &run.replacement,
            run.totals.replacements,
            run.totals.files,
            searched,
            run.totals.skipped,
        );
        self.inner.results.set_heading(&run.search, text);
    }

    fn finish_replace_in_files(&self, run: RifRun) {
        let stopped_after = run.stop_requested.map(|at| at.elapsed());
        let total = run.started.elapsed();
        let RifRun {
            handle,
            search,
            pattern,
            replacement,
            totals,
            unlisted_files,
            ..
        } = run;
        let summary = handle.wait();
        let text = heading(
            &pattern,
            &replacement,
            totals.replacements,
            totals.files,
            Some(summary.files_searched),
            totals.skipped,
        );
        let results = &self.inner.results;
        results.set_heading(&search, text.clone());
        let mut status = format!("{text} in {:.2} s", total.as_secs_f64());
        if summary.cancelled {
            status.push_str(" — stopped");
        }
        if totals.failed > 0 {
            status.push_str(&format!(
                " — {} failed",
                counted(totals.failed, "file", "files")
            ));
        }
        if unlisted_files > 0 {
            status.push_str(&format!(
                " — {} listed without their lines",
                counted(unlisted_files, "file", "files")
            ));
        }
        results.status.set_label(&status);
        results.stop.set_visible(false);
        if let Some(action) = self.action(ActionId::StopFindInFiles) {
            action.set_enabled(self.find_in_files_running());
        }
        self.inner.find.set_message(&text, Tone::Info);
        tracing::info!(
            files_searched = summary.files_searched,
            files_changed = totals.files,
            replacements = totals.replacements,
            skipped = totals.skipped,
            failed = totals.failed,
            open_documents = totals.open_documents,
            cancelled = summary.cancelled,
            total_ms = total.as_secs_f64() * 1e3,
            "replace in files"
        );
        self.inner.nav.rif_stats.replace(Some(RifStats {
            total,
            summary,
            replacements: totals.replacements,
            files: totals.files,
            skipped: totals.skipped,
            failed: totals.failed,
            open_documents: totals.open_documents,
            stopped_after,
        }));
    }

    /// Stop: no file is written after this; files already written are reported.
    pub(super) fn stop_replace_in_files(&self) {
        if let Some(run) = self.inner.nav.rif.borrow_mut().as_mut() {
            run.stop_requested.get_or_insert_with(Instant::now);
            run.cancel.store(true, Ordering::Relaxed);
            run.handle.cancel();
        }
    }

    pub fn replace_in_files_running(&self) -> bool {
        self.inner.nav.rif.borrow().is_some()
    }

    pub fn replace_in_files_stats(&self) -> Option<RifStats> {
        self.inner.nav.rif_stats.borrow().clone()
    }

    /// Replaces in the open document whose file is `path` (links resolved): in its buffer, as
    /// one user action, then saves it when it had no unsaved changes and saving writes its
    /// bytes back exactly. `None` when nothing matched or the run was stopped.
    async fn replace_in_open_document(
        &self,
        path: PathBuf,
        matcher: Arc<Matcher>,
        template: Template,
        cancel: Arc<AtomicBool>,
    ) -> Option<(FileNode, Totals)> {
        let home = std::env::home_dir();
        let unchanged = |reason: Skip| {
            Some((
                skipped_node(tilde(&path, home.as_deref()), path.clone(), &reason, None),
                Totals {
                    skipped: 1,
                    ..Totals::default()
                },
            ))
        };
        let Some(page) = self.page_for_file(&path).await else {
            return unchanged(Skip::DocumentClosed);
        };
        let label = tilde(
            &page.path().unwrap_or_else(|| path.clone()),
            home.as_deref(),
        );
        let loaded = self.ensure_loaded(&page).await;
        let started = Instant::now();
        while loaded && page.is_loading() && started.elapsed() < LOAD_WAIT {
            glib::timeout_future(Duration::from_millis(10)).await;
        }
        if self.tab_page(&page).is_none() {
            return unchanged(Skip::DocumentClosed);
        }
        if !loaded || page.is_loading() {
            return unchanged(Skip::DocumentNotLoaded);
        }
        let skipped = |reason: Skip| {
            let node = skipped_node(label.clone(), path.clone(), &reason, None);
            Some((
                FileNode {
                    target: FileTarget::Page {
                        page: page.downgrade(),
                        path: page.path(),
                    },
                    ..node
                },
                Totals {
                    skipped: 1,
                    ..Totals::default()
                },
            ))
        };
        if page.read_only().is_some() {
            return skipped(Skip::DocumentReadOnly);
        }
        let dirty = self.has_unsaved_changes(&page);
        let text = page.text();
        let revision = page.revision();
        let replaced = worker::run(move || {
            let result = matcher.replace_all(&text, &template, None, &cancel);
            (text, result)
        })
        .await;
        let (text, result) = match replaced {
            Some((text, Ok(result))) => (text, result),
            Some((_, Err(SearchError::Cancelled))) | None => return None,
            Some((_, Err(error))) => {
                return Some((
                    FileNode {
                        label,
                        target: FileTarget::Page {
                            page: page.downgrade(),
                            path: page.path(),
                        },
                        hits: Vec::new(),
                        summary: Some(format!("failed: {error}")),
                    },
                    Totals {
                        failed: 1,
                        ..Totals::default()
                    },
                ));
            }
        };
        if result.count == 0 {
            return None;
        }
        if self.tab_page(&page).is_none() {
            return unchanged(Skip::DocumentClosed);
        }
        if page.revision() != revision {
            return skipped(Skip::DocumentChanged);
        }
        let changed = !result.edits.is_empty();
        if changed {
            page.clear_current_match();
            super::replace::apply_tracked(&page, &result, &text);
        }
        let count = result.count;
        let hits = worker::run(move || {
            let ranges = result.byte_ranges(&text);
            let new_text = result.apply(&text);
            replaced_hits(&new_text, &ranges, &result.edits, FILE_HIT_LIMIT)
        })
        .await
        .unwrap_or_default();
        let outcome = if !changed || dirty {
            DocumentOutcome::Unsaved
        } else if self.saves_exactly(&page).await {
            let saved = match page.path() {
                Some(file) => self.write_to(&page, file, false).await,
                None => false,
            };
            if saved {
                DocumentOutcome::Saved
            } else {
                DocumentOutcome::NotSaved
            }
        } else {
            DocumentOutcome::NotSaved
        };
        let mut summary = file_summary(count, Some(outcome));
        if !changed {
            summary = format!("{}, already as replaced", file_summary(count, None));
        }
        Some((
            FileNode {
                label,
                target: FileTarget::Page {
                    page: page.downgrade(),
                    path: page.path(),
                },
                hits,
                summary: Some(summary),
            },
            Totals {
                replacements: count,
                files: usize::from(changed),
                ..Totals::default()
            },
        ))
    }

    /// The tab whose file is `path` (links resolved), looked up on a worker.
    async fn page_for_file(&self, path: &Path) -> Option<EditorPage> {
        let candidates: Vec<(EditorPage, PathBuf)> = self
            .documents()
            .into_iter()
            .filter_map(|page| {
                let state = page.state();
                let file = state.canonical.or(state.path)?;
                Some((page, file))
            })
            .collect();
        let files: Vec<PathBuf> = candidates.iter().map(|(_, file)| file.clone()).collect();
        let canonical: Vec<Option<PathBuf>> = worker::run(move || {
            files
                .iter()
                .map(|file| file.canonicalize().ok())
                .collect::<Vec<_>>()
        })
        .await
        .unwrap_or_default();
        candidates
            .into_iter()
            .zip(canonical)
            .find(|(_, canonical)| canonical.as_deref() == Some(path))
            .map(|((page, _), _)| page)
    }

    /// Whether saving `page` now writes the file it was read from, with only the edits
    /// changed: its text came from the file as it is, decoded without loss, with one kind of
    /// line ending, and the file hasn't changed on disk since.
    async fn saves_exactly(&self, page: &EditorPage) -> bool {
        let state = page.state();
        if state.lossy()
            || state.mixed_eol
            || state.origin != TextOrigin::File
            || state.read_only.is_some()
            || state.format_changed
        {
            return false;
        }
        let (Some(path), Some(baseline)) = (state.path, state.baseline) else {
            return false;
        };
        let now = worker::run(move || fingerprint(&path)).await;
        matches!(now, Some(Ok(now)) if baseline.matches(&now))
    }
}

/// A row for a file Replace in Files left alone.
fn skipped_node(label: String, path: PathBuf, reason: &Skip, matches: Option<usize>) -> FileNode {
    let summary = match matches {
        Some(matches) => format!(
            "{} not replaced: {reason}",
            counted(matches, "match", "matches")
        ),
        None => format!("skipped: {reason}"),
    };
    FileNode {
        label,
        target: FileTarget::Path(path),
        hits: Vec::new(),
        summary: Some(summary),
    }
}
