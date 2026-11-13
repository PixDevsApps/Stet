//! Find in Files (Ctrl+Shift+F): the files row searches a folder in the background
//! (`stet_infrastructure::files::find`). Open documents with unsaved changes, and those whose
//! file the search would not read as the text they show (other encodings, NUL, CR line
//! endings), are searched from their decoded text.

use super::super::bar::{Field, FilesRequest};
use super::super::results::{FileNode, FileTarget};
use super::{Feed, FilesSearch, RunSlot, StreamingRun};
use crate::editor::DocState;
use crate::window::Window;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use stet_domain::document::tilde;
use stet_domain::search::Query;
use stet_domain::search::report::{find_in_files_progress, find_in_files_status, results_heading};
use stet_infrastructure::files::find::{
    self as find_in_files, FifRequest, FifSummary, FileHits, OpenDocument,
};

/// What Find in Files adds to its run: the query and folder its heading and status name, and
/// when the search reported its first file.
pub(super) struct FindFeed {
    pattern: String,
    folder: String,
    home: Option<PathBuf>,
    /// When the search reported its first file, on its own thread.
    found_first: Arc<OnceLock<Duration>>,
}

/// How the last Find in Files went, for the self-test's report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FifStats {
    /// The first matching file reached the app's channel.
    pub first_found: Option<Duration>,
    /// The panel first showed results (it looks every 33 ms).
    pub first_result: Option<Duration>,
    pub total: Duration,
    /// From Stop until the search's threads had ended, as seen by the panel (which looks every
    /// 33 ms).
    pub search_ended_after_stop: Option<Duration>,
    /// From Stop until the panel was done.
    pub stopped_after: Option<Duration>,
    /// The longest time one 33 ms drain spent adding results to the panel.
    pub slowest_drain: Duration,
    pub summary: FifSummary,
}

impl Feed for FindFeed {
    type Event = FileHits;
    type Summary = FifSummary;
    type Progress = ();
    type Stats = FifStats;
    const DROPS_AFTER_STOP: bool = true;

    fn slot(files: &FilesSearch) -> &RunSlot<Self> {
        &files.find
    }

    fn absorb(&mut self, file: FileHits, _stopped: bool) -> Option<(FileNode, usize)> {
        let lines = file.hits.len();
        Some((
            FileNode {
                label: tilde(&file.path, self.home.as_deref()),
                target: FileTarget::Path(file.path),
                hits: file.hits,
                summary: None,
            },
            lines,
        ))
    }

    fn heading(run: &StreamingRun<Self>, searched: Option<usize>) -> String {
        results_heading(
            &run.feed.pattern,
            run.search.hits(),
            run.search.files(),
            searched,
        )
    }

    fn progress(run: &StreamingRun<Self>, added: bool, _finished: bool) -> Option<String> {
        added.then(|| find_in_files_progress(&run.feed.folder, run.search.hits()))
    }

    fn finish(run: StreamingRun<Self>, window: &Window) -> FifStats {
        let summary = run.handle.wait();
        let total = run.started.elapsed();
        let heading = results_heading(
            &run.feed.pattern,
            run.search.hits(),
            run.search.files(),
            Some(summary.files_searched),
        );
        let results = &window.inner.results;
        results.set_heading(&run.search, heading.clone());
        let status = find_in_files_status(
            &heading,
            total,
            summary.cancelled,
            summary.truncated.then_some(summary.hits),
            summary.errors.len(),
        );
        let details = (!summary.errors.is_empty()).then(|| {
            summary
                .errors
                .iter()
                .take(20)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        });
        results.set_status(&status, details.as_deref());
        tracing::info!(
            files_searched = summary.files_searched,
            files_matched = summary.files_matched,
            hits = summary.hits,
            truncated = summary.truncated,
            cancelled = summary.cancelled,
            first_result_ms = run.first_shown.map(|first| first.as_secs_f64() * 1e3),
            total_ms = total.as_secs_f64() * 1e3,
            "find in files"
        );
        FifStats {
            first_found: run.feed.found_first.get().copied(),
            first_result: run.first_shown,
            total,
            search_ended_after_stop: run.ended_after_stop,
            stopped_after: run.stop_requested.map(|at| at.elapsed()),
            slowest_drain: run.slowest_drain,
            summary,
        }
    }
}

impl Window {
    /// Starts Find in Files with the bar's query, folder, filters and options; a search
    /// already running is stopped first.
    pub(in crate::window) fn start_find_in_files(&self) {
        if !self.files_row_shown() {
            return;
        }
        let find = &self.inner.find;
        let Some(compiled) = find.query() else {
            return;
        };
        let Some(request) = self.files_request("Choose a folder to search") else {
            return;
        };
        // Restored tabs whose unsaved text is still in their backup load it first, as
        // documents with unsaved changes are searched from memory.
        if self
            .pages()
            .iter()
            .any(|page| Self::is_stub(page) && Self::has_unsaved_changes(page))
        {
            let window = self.clone();
            self.spawn(async move {
                window.load_stubs().await;
                window.start_find_in_files();
            });
            return;
        }
        find.remember(Field::Find, &compiled.query.pattern);
        find.remember(Field::Directory, &request.folder);
        find.remember(Field::Filters, &request.filters);
        self.stop_find_in_files();
        self.inner.files.find.stats.replace(None);
        let open_documents = self.searched_from_memory();
        let query = compiled.query.clone();
        let window = self.clone();
        self.spawn(async move {
            if window.is_folder(&request).await {
                window.run_find_in_files(query, request, open_documents);
            }
        });
    }

    /// The open documents Find in Files searches from their text, not their file.
    fn searched_from_memory(&self) -> Vec<OpenDocument> {
        self.documents()
            .into_iter()
            .filter(|page| !page.is_loading())
            .filter_map(|page| {
                let state = page.state();
                if !searched_from_memory(&state, page.is_dirty()) {
                    return None;
                }
                let path = state.canonical.or(state.path)?;
                Some(OpenDocument {
                    path,
                    text: Arc::from(page.text()),
                })
            })
            .collect()
    }

    fn run_find_in_files(
        &self,
        query: Query,
        request: FilesRequest,
        open_documents: Vec<OpenDocument>,
    ) {
        let mut job = FifRequest::new(vec![request.folder_path()], query);
        job.filters = request.filters;
        job.include_hidden = request.hidden;
        job.respect_gitignore = request.gitignore;
        job.include_subfolders = request.subfolders;
        job.open_documents = open_documents;
        let pattern = job.query.pattern.clone();
        let found_first = Arc::new(OnceLock::new());
        let feed = FindFeed {
            pattern: pattern.clone(),
            folder: request.folder.clone(),
            home: std::env::home_dir(),
            found_first: Arc::clone(&found_first),
        };
        self.start_run(
            feed,
            results_heading(&pattern, 0, 0, None),
            &format!("Searching {}…", request.folder),
            "",
            move |sender, started| {
                find_in_files::start(job, move |file| {
                    found_first.get_or_init(|| started.elapsed());
                    let _ = sender.send_blocking(file);
                })
            },
        );
    }

    /// Stops the running Find in Files; its results so far stay in the panel.
    pub(in crate::window) fn stop_find_in_files(&self) {
        self.stop_run::<FindFeed>();
    }

    /// Whether a Find in Files search is running.
    pub fn find_in_files_running(&self) -> bool {
        self.is_running::<FindFeed>()
    }

    #[cfg(feature = "selftest")]
    pub fn find_in_files_stats(&self) -> Option<FifStats> {
        self.inner.files.find.stats.borrow().clone()
    }
}

/// Whether Find in Files searches an open document's text, as the tab shows it, instead of its
/// file: with unsaved changes, and whenever reading the file gives other text. The search reads
/// only UTF-8 and UTF-16 with a byte-order mark as text, skips files with NUL bytes, and counts
/// lines by LF; the tab shows a decoding error as U+FFFD.
fn searched_from_memory(state: &DocState, dirty: bool) -> bool {
    dirty
        || !find_in_files::reads_as_text(state.save.format.encoding, state.save.format.bom)
        || state.lossy()
        || state.nul_count() > 0
        || state.eol_stats().cr > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use stet_domain::document::Loss;
    use stet_domain::text::EolStats;
    use stet_infrastructure::encoding::{Detection, DetectionSource, Encoding};
    use stet_infrastructure::fs::DecodeReport;

    /// A document read from a UTF-8 file of which decoding reported `change`.
    fn decoded(change: fn(&mut DecodeReport)) -> DocState {
        let encoding = Encoding::for_label(b"utf-8").unwrap();
        let mut report = DecodeReport {
            encoding,
            bom: false,
            eol_stats: EolStats::default(),
            detection: Detection {
                encoding,
                bom: false,
                source: DetectionSource::ValidUtf8,
                binary: false,
            },
            had_errors: false,
            lossy_roundtrip: false,
            nul_count: 0,
            placeholder_conflict: false,
        };
        change(&mut report);
        let mut state = DocState::default();
        state.decode = Some(report);
        state
    }

    #[test]
    fn documents_that_the_file_search_would_read_differently_come_from_memory() {
        let clean = DocState::default();
        assert!(!searched_from_memory(&clean, false));
        assert!(searched_from_memory(&clean, true));
        assert!(!searched_from_memory(&decoded(|_| {}), false));
        let with = |label: &str, bom: bool| {
            let mut state = DocState::default();
            state.save.format.encoding = Encoding::for_label(label.as_bytes()).unwrap();
            state.save.format.bom = bom;
            searched_from_memory(&state, false)
        };
        assert!(!with("utf-8", true));
        assert!(!with("utf-16le", true));
        assert!(with("utf-16le", false));
        assert!(with("windows-1252", false));
        assert!(with("shift_jis", false));
        let mut lossy = DocState::default();
        lossy.loss = Some(Loss::Malformed);
        for state in [
            decoded(|report| report.nul_count = 1),
            decoded(|report| report.eol_stats.cr = 1),
            lossy,
        ] {
            assert!(searched_from_memory(&state, false), "{state:?}");
        }
    }
}
