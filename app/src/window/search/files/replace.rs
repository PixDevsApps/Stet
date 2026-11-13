//! Replace in Files (ADR-005 amendment): the files row's other command. After a
//! confirmation that names the folder, the filters and that files on disk can't be undone, a
//! run on threads of its own (`stet_infrastructure::files::replace`) replaces in every file
//! the walk reaches, keeping each file's encoding, byte-order mark and line endings, and
//! reports each file to the results panel, with the reason for those it left alone. Open
//! documents get the change in their buffer instead, as one undoable edit (one bulk edit above
//! 2,000 replacements): a document without unsaved changes is then saved, so the disk matches
//! the other files and Undo still takes the change back; one with unsaved changes keeps it
//! unsaved.

use super::super::bar::{Field, FilesRequest, Tone};
use super::super::replace::apply_tracked;
use super::super::results::{FileNode, FileTarget};
use super::{Feed, FilesSearch, RunSlot, StreamingRun};
use crate::editor::EditorPage;
use crate::editor::load::LOAD_WAIT;
use crate::editor::search::Compiled;
use crate::runtime::{Ticket, worker};
use crate::window::Window;
use crate::window::snapshot::{PageSnapshot, Stale};
use libadwaita as adw;
use libadwaita::prelude::*;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use stet_domain::document::{TextOrigin, tilde};
use stet_domain::search::Template;
use stet_domain::search::replace_files::{DocumentOutcome, Skip, file_summary, heading};
use stet_domain::search::report::{replace_in_files_progress, replace_in_files_status};
use stet_domain::text::counted;
use stet_infrastructure::files::find::DEFAULT_MAX_HITS;
use stet_infrastructure::files::replace::{
    self as rif, FILE_HIT_LIMIT, RifCounters, RifEvent, RifRequest, RifSummary, replaced_hits,
};
use stet_infrastructure::fs::fingerprint;
use stet_infrastructure::search::{Matcher, SearchError};

/// The most changed lines a run lists, as Find in Files lists at most this many hits; files
/// past it are listed with their counts only.
const LISTED_LINES: usize = DEFAULT_MAX_HITS;
/// Where an in-place save keeps the previous content (the same folder the editor's saves use).
const SAVE_BACKUPS_DIR: &str = "save-backups";

/// What Replace in Files adds to its run: what its heading names, the query and template the
/// open documents are replaced with, and those documents still to do.
pub(super) struct ReplaceFeed {
    pattern: String,
    replacement: String,
    folder: String,
    home: Option<PathBuf>,
    matcher: Arc<Matcher>,
    template: Template,
    /// Stops the replace in an open document.
    cancel: Arc<AtomicBool>,
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

impl Feed for ReplaceFeed {
    type Event = RifEvent;
    type Summary = RifSummary;
    type Progress = Arc<RifCounters>;
    type Stats = RifStats;
    const DROPS_AFTER_STOP: bool = false;

    fn slot(files: &FilesSearch) -> &RunSlot<Self> {
        &files.replace
    }

    fn absorb(&mut self, event: RifEvent, stopped: bool) -> Option<(FileNode, usize)> {
        let label = |path: &Path| tilde(path, self.home.as_deref());
        match event {
            RifEvent::Replaced {
                path,
                replacements,
                written,
                hits,
                ..
            } => {
                self.totals.replacements += replacements;
                let mut summary = file_summary(replacements, None);
                if written {
                    self.totals.files += 1;
                } else {
                    summary.push_str(", already as replaced");
                }
                // After Stop, the files written before it are listed at once.
                let unlisted =
                    !hits.is_empty() && (self.listed + hits.len() > LISTED_LINES || stopped);
                let hits = if unlisted {
                    self.unlisted_files += 1;
                    summary.push_str(", lines not listed");
                    Vec::new()
                } else {
                    hits
                };
                self.listed += hits.len();
                let lines = hits.len().max(1);
                Some((
                    FileNode {
                        label: label(&path),
                        target: FileTarget::Path(path),
                        hits,
                        summary: Some(summary),
                    },
                    lines,
                ))
            }
            RifEvent::Skipped {
                path,
                reason,
                matches,
            } => {
                self.totals.skipped += 1;
                Some((skipped_node(label(&path), path, &reason, matches), 1))
            }
            RifEvent::Failed { path, error } => {
                self.totals.failed += 1;
                Some((
                    FileNode {
                        label: label(&path),
                        target: FileTarget::Path(path),
                        hits: Vec::new(),
                        summary: Some(format!("failed: {error}")),
                    },
                    1,
                ))
            }
            RifEvent::OpenDocument { path } => {
                self.open_queue.push_back(path);
                None
            }
        }
    }

    fn heading(run: &StreamingRun<Self>, searched: Option<usize>) -> String {
        let feed = &run.feed;
        heading(
            &feed.pattern,
            &feed.replacement,
            feed.totals.replacements,
            feed.totals.files,
            searched,
            feed.totals.skipped,
        )
    }

    fn progress(run: &StreamingRun<Self>, _added: bool, finished: bool) -> Option<String> {
        (!finished).then(|| {
            replace_in_files_progress(
                &run.feed.folder,
                run.handle.progress().files_searched,
                run.feed.totals.files,
            )
        })
    }

    /// Starts the next open document, one at a time, until Stop.
    fn advance(run: &mut StreamingRun<Self>, window: &Window, ticket: &Ticket) {
        let feed = &mut run.feed;
        if feed.open_busy || run.stop_requested.is_some() {
            return;
        }
        let Some(path) = feed.open_queue.pop_front() else {
            return;
        };
        feed.open_busy = true;
        let matcher = Arc::clone(&feed.matcher);
        let template = feed.template.clone();
        let cancel = Arc::clone(&feed.cancel);
        let owned = window.clone();
        let ticket = ticket.clone();
        window.spawn(async move {
            let node = owned
                .replace_in_open_document(path, matcher, template, cancel)
                .await;
            owned.open_document_done(&ticket, node);
        });
    }

    fn busy(run: &StreamingRun<Self>) -> bool {
        run.feed.open_busy || (!run.feed.open_queue.is_empty() && run.stop_requested.is_none())
    }

    fn stop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    fn finish(run: StreamingRun<Self>, window: &Window) -> RifStats {
        let stopped_after = run.stop_requested.map(|at| at.elapsed());
        let total = run.started.elapsed();
        let summary = run.handle.wait();
        let feed = run.feed;
        let totals = feed.totals;
        let text = heading(
            &feed.pattern,
            &feed.replacement,
            totals.replacements,
            totals.files,
            Some(summary.files_searched),
            totals.skipped,
        );
        window.inner.results.set_heading(&run.search, text.clone());
        window
            .inner
            .results
            .set_status_text(&replace_in_files_status(
                &text,
                total,
                summary.cancelled,
                totals.failed,
                feed.unlisted_files,
            ));
        window.inner.find.set_message(&text, Tone::Info);
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
        RifStats {
            total,
            summary,
            replacements: totals.replacements,
            files: totals.files,
            skipped: totals.skipped,
            failed: totals.failed,
            open_documents: totals.open_documents,
            stopped_after,
        }
    }
}

impl Window {
    /// Replace in Files: needs the files row; asks first, then runs.
    pub(in crate::window) fn replace_in_files(&self) {
        if !self.files_row_shown() {
            return;
        }
        let find = &self.inner.find;
        if self.replace_in_files_running() {
            find.set_message("Replace in Files is already running", Tone::Info);
            return;
        }
        let Some(compiled) = find.query() else {
            return;
        };
        let template = match find.replace_template() {
            Ok(template) => template,
            Err(error) => {
                find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        let Some(request) = self.files_request("Choose a folder to replace in") else {
            return;
        };
        let replacement = find.replace_text();
        find.remember(Field::Find, &compiled.query.pattern);
        find.remember(Field::Replace, &replacement);
        find.remember(Field::Directory, &request.folder);
        find.remember(Field::Filters, &request.filters);
        let window = self.clone();
        self.spawn(async move {
            if !window.is_folder(&request).await
                || !window
                    .ask_replace_in_files(&compiled.query.pattern, &replacement, &request)
                    .await
            {
                return;
            }
            window.run_replace_in_files(&compiled, template, replacement, request);
        });
    }

    async fn ask_replace_in_files(
        &self,
        pattern: &str,
        replacement: &str,
        request: &FilesRequest,
    ) -> bool {
        let FilesRequest {
            folder,
            filters,
            subfolders,
            hidden,
            gitignore,
        } = request;
        let files = if filters.is_empty() || filters == "*.*" {
            "every file".to_owned()
        } else {
            format!("the files matching {filters}")
        };
        let subfolders = if *subfolders {
            "with subfolders"
        } else {
            "without subfolders"
        };
        let hidden = if *hidden {
            "hidden files included"
        } else {
            "hidden files left out"
        };
        let ignored = if *gitignore {
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

    fn run_replace_in_files(
        &self,
        compiled: &Rc<Compiled>,
        template: Template,
        replacement: String,
        request: FilesRequest,
    ) {
        self.stop_find_in_files();
        let backups = self.inner.shared.config.dirs.state.join(SAVE_BACKUPS_DIR);
        let mut job = RifRequest::new(
            vec![request.folder_path()],
            compiled.query.clone(),
            template.clone(),
            backups,
        );
        job.filters = request.filters;
        job.include_hidden = request.hidden;
        job.respect_gitignore = request.gitignore;
        job.include_subfolders = request.subfolders;
        job.open_documents = self
            .documents()
            .iter()
            .filter_map(|page| {
                let state = page.state();
                state.canonical.or(state.path)
            })
            .collect();
        let pattern = compiled.query.pattern.clone();
        let first_heading = heading(&pattern, &replacement, 0, 0, None, 0);
        let status = format!("Replacing in {}…", request.folder);
        let feed = ReplaceFeed {
            pattern,
            replacement,
            folder: request.folder,
            home: std::env::home_dir(),
            matcher: Arc::clone(&compiled.matcher),
            template,
            cancel: Arc::new(AtomicBool::new(false)),
            open_queue: VecDeque::new(),
            open_busy: false,
            totals: Totals::default(),
            listed: 0,
            unlisted_files: 0,
        };
        self.start_run(
            feed,
            first_heading,
            &status,
            "Replacing in files…",
            move |sender, _| {
                rif::start(job, move |event| {
                    let _ = sender.send_blocking(event);
                })
            },
        );
    }

    /// An open document is done: its row and totals join the run's, and the next document,
    /// or the end of the run, needn't wait for the next drain.
    fn open_document_done(&self, ticket: &Ticket, node: Option<(FileNode, Totals)>) {
        let mut current = self.inner.files.replace.run.borrow_mut();
        let Some(run) = current.as_mut().filter(|_| ticket.is_current()) else {
            return;
        };
        run.feed.open_busy = false;
        if let Some((node, totals)) = node {
            let sum = &mut run.feed.totals;
            sum.replacements += totals.replacements;
            sum.files += totals.files;
            sum.skipped += totals.skipped;
            sum.failed += totals.failed;
            sum.open_documents += 1;
            let results = &self.inner.results;
            results.add_files(&run.search, vec![node]);
            results.set_heading(&run.search, ReplaceFeed::heading(run, None));
        }
        drop(current);
        self.drain::<ReplaceFeed>(ticket);
    }

    /// Stop: no file is written after this; files already written are reported.
    pub(in crate::window) fn stop_replace_in_files(&self) {
        self.stop_run::<ReplaceFeed>();
    }

    pub fn replace_in_files_running(&self) -> bool {
        self.is_running::<ReplaceFeed>()
    }

    #[cfg(feature = "selftest")]
    pub fn replace_in_files_stats(&self) -> Option<RifStats> {
        self.inner.files.replace.stats.borrow().clone()
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
        let (page, label) = match self.loaded_document(&path, home.as_deref()).await {
            Ok(found) => found,
            Err(reason) => return unchanged(reason),
        };
        let skipped = |reason: Skip| {
            let node = skipped_node(label.clone(), path.clone(), &reason, None);
            Some((
                FileNode {
                    target: page_target(&page),
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
        let dirty = Self::has_unsaved_changes(&page);
        let (snapshot, text) = PageSnapshot::take(&page);
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
                        target: page_target(&page),
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
        match self.check_snapshot(&snapshot) {
            Ok(()) => {}
            Err(Stale::Closed) => return unchanged(Skip::DocumentClosed),
            Err(_) => return skipped(Skip::DocumentChanged),
        }
        let changed = !result.edits.is_empty();
        if changed {
            page.clear_current_match();
            apply_tracked(&page, &result, &text);
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
        } else {
            self.save_replaced(&page).await
        };
        let mut summary = file_summary(count, Some(outcome));
        if !changed {
            summary = format!("{}, already as replaced", file_summary(count, None));
        }
        Some((
            FileNode {
                label,
                target: page_target(&page),
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

    /// The open document whose file is `path`, loaded, and its label in the results; or why
    /// it is left alone.
    async fn loaded_document(
        &self,
        path: &Path,
        home: Option<&Path>,
    ) -> Result<(EditorPage, String), Skip> {
        let page = self.page_for_file(path).await.ok_or(Skip::DocumentClosed)?;
        let label = tilde(&page.path().unwrap_or_else(|| path.to_owned()), home);
        let loaded = self.ensure_loaded(&page).await;
        if loaded {
            page.loaded(LOAD_WAIT).await;
        }
        if self.tab_page(&page).is_none() {
            return Err(Skip::DocumentClosed);
        }
        if !loaded || page.is_loading() {
            return Err(Skip::DocumentNotLoaded);
        }
        Ok((page, label))
    }

    /// Saves an open document that had no unsaved changes before its replacements, when
    /// saving writes its bytes back exactly.
    async fn save_replaced(&self, page: &EditorPage) -> DocumentOutcome {
        if !self.saves_exactly(page).await {
            return DocumentOutcome::NotSaved;
        }
        let saved = match page.path() {
            Some(file) => self.write_to(page, file, false).await,
            None => false,
        };
        if saved {
            DocumentOutcome::Saved
        } else {
            DocumentOutcome::NotSaved
        }
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
            || state.eol.mixed
            || state.origin != TextOrigin::File
            || state.read_only().is_some()
            || state.save.format_changed
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
/// The results' target for the open document `page`.
fn page_target(page: &EditorPage) -> FileTarget {
    FileTarget::Page {
        page: page.downgrade(),
        path: page.path(),
    }
}

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
