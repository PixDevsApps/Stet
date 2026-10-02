//! Find in Files (Ctrl+Shift+F): the bar's files row searches a folder in the background
//! (`stet_infrastructure::find_in_files`), and results stream into the results panel about 30
//! times a second. Open documents with unsaved changes, and those whose file the search would
//! not read as the text they show (other encodings, NUL, CR line endings), are searched from
//! their decoded text.

use super::Window;
use super::find::{BarMode, Field, Tone};
use super::results::{FileNode, FileTarget, SearchNode};
use crate::editor::DocState;
use crate::worker;
use gtk4 as gtk;
use gtk4::prelude::*;
use gtk4::{gio, glib};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use stet_domain::document::tilde;
use stet_domain::search::report::results_heading;
use stet_infrastructure::find_in_files::{
    self, FifHandle, FifRequest, FifSummary, FileHits, OpenDocument,
};

/// How often streamed results reach the panel: about 30 times a second.
const DRAIN_INTERVAL: Duration = Duration::from_millis(33);
/// About the most result lines added to the panel per drain (whole files at a time), so a
/// burst never stalls a frame.
const LINES_PER_DRAIN: usize = 1_000;

/// A Find in Files search in progress.
pub struct FifRun {
    id: u64,
    handle: FifHandle,
    receiver: async_channel::Receiver<FileHits>,
    search: Rc<SearchNode>,
    pattern: String,
    folder: String,
    started: Instant,
    /// When the search reported its first file, on its own thread.
    found_first: Arc<OnceLock<Duration>>,
    /// When the panel first showed results.
    first: Option<Duration>,
    cancel_requested: Option<Instant>,
    /// How long after Stop the search's threads had ended, as the panel saw it.
    search_ended: Option<Duration>,
    /// The longest time one drain spent adding files to the panel.
    slowest_drain: Duration,
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

impl Window {
    /// The folder field's default: the current file's folder, else the last folder searched,
    /// else the home folder.
    pub(super) fn default_search_folder(&self) {
        let find = &self.inner.find;
        let home = std::env::home_dir();
        let from_document = self
            .current_page()
            .and_then(|page| page.path())
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .map(|dir| tilde(&dir, home.as_deref()));
        let folder = from_document
            .or_else(|| find.history().directories.get(0).map(str::to_owned))
            .or_else(|| home.as_deref().map(|home| tilde(home, Some(home))));
        if let Some(folder) = folder {
            find.directory.set_text(&folder);
        }
    }

    pub(super) async fn choose_search_folder(&self) {
        let find = &self.inner.find;
        let dialog = gtk::FileDialog::builder()
            .title("Find in Folder")
            .modal(true)
            .build();
        let current = expand_home(find.directory.text().trim());
        if !current.as_os_str().is_empty() {
            dialog.set_initial_folder(Some(&gio::File::for_path(&current)));
        }
        match dialog.select_folder_future(Some(&self.inner.window)).await {
            Ok(folder) => {
                if let Some(path) = folder.path() {
                    let home = std::env::home_dir();
                    find.directory.set_text(&tilde(&path, home.as_deref()));
                }
            }
            Err(error) => {
                if !error.matches(gtk::DialogError::Dismissed) {
                    self.toast(&format!("Could not open the folder chooser: {error}"));
                }
            }
        }
    }

    /// Starts Find in Files with the bar's query, folder, filters and options; a search
    /// already running is stopped first.
    pub(super) fn start_find_in_files(&self) {
        let find = &self.inner.find;
        if !(find.is_open() && find.bar_mode() == BarMode::Files) {
            self.show_find_mode(BarMode::Files);
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
        let folder_text = find.directory.text().trim().to_owned();
        if folder_text.is_empty() {
            find.set_message("Choose a folder to search", Tone::Error);
            return;
        }
        // Restored tabs whose unsaved text is still in their backup load it first (M2), as
        // documents with unsaved changes are searched from memory.
        if self
            .pages()
            .iter()
            .any(|page| self.is_stub(page) && self.has_unsaved_changes(page))
        {
            let window = self.clone();
            self.spawn(async move {
                window.load_stubs().await;
                window.start_find_in_files();
            });
            return;
        }
        let folder = expand_home(&folder_text);
        let filters = find.filters.text().trim().to_owned();
        find.remember(Field::Find, &compiled.query.pattern);
        find.remember(Field::Directory, &folder_text);
        find.remember(Field::Filters, &filters);
        self.stop_find_in_files();
        self.inner.fif_stats.replace(None);

        let open_documents: Vec<OpenDocument> = self
            .documents()
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
            .collect();
        let mut request = FifRequest::new(vec![folder.clone()], compiled.query.clone());
        request.filters = filters;
        request.include_hidden = find.hidden.is_active();
        request.respect_gitignore = find.gitignore.is_active();
        request.include_subfolders = find.subfolders.is_active();
        request.open_documents = open_documents;
        let pattern = compiled.query.pattern.clone();
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
            window.run_find_in_files(request, pattern, folder_text);
        });
    }

    fn run_find_in_files(&self, request: FifRequest, pattern: String, folder: String) {
        let (sender, receiver) = async_channel::unbounded::<FileHits>();
        let started = Instant::now();
        let found_first = Arc::new(OnceLock::new());
        let sink_first = Arc::clone(&found_first);
        let handle = match find_in_files::start(request, move |file| {
            sink_first.get_or_init(|| started.elapsed());
            let _ = sender.send_blocking(file);
        }) {
            Ok(handle) => handle,
            Err(error) => {
                self.inner.find.set_message(&error.to_string(), Tone::Error);
                return;
            }
        };
        let results = &self.inner.results;
        let search = results.begin(results_heading(&pattern, 0, 0, None));
        results.status.set_label(&format!("Searching {folder}…"));
        results.stop.set_visible(true);
        if let Some(action) = self.action(stet_domain::actions::ActionId::StopFindInFiles) {
            action.set_enabled(true);
        }
        self.inner.find.set_message("", Tone::Info);
        self.show_results(false);
        let id = self.inner.fif_runs.get() + 1;
        self.inner.fif_runs.set(id);
        self.inner.fif.replace(Some(FifRun {
            id,
            handle,
            receiver,
            search,
            pattern,
            folder,
            started,
            found_first,
            first: None,
            cancel_requested: None,
            search_ended: None,
            slowest_drain: Duration::ZERO,
        }));
        glib::timeout_add_local(
            DRAIN_INTERVAL,
            glib::clone!(
                #[weak(rename_to = inner)]
                self.inner,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || Window { inner }.drain_find_in_files(id)
            ),
        );
    }

    /// Moves streamed files into the panel; finishes the run once the search has ended. After
    /// Stop, files still waiting in the channel are dropped, so the panel stops at once.
    fn drain_find_in_files(&self, id: u64) -> glib::ControlFlow {
        let mut slot = self.inner.fif.borrow_mut();
        let Some(run) = slot.as_mut().filter(|run| run.id == id) else {
            return glib::ControlFlow::Break;
        };
        let finished = run.handle.is_finished();
        if finished && let Some(stopped) = run.cancel_requested {
            if run.search_ended.is_none() {
                run.search_ended = Some(stopped.elapsed());
            }
            while run.receiver.try_recv().is_ok() {}
        }
        let started = Instant::now();
        let home = std::env::home_dir();
        let mut files = Vec::new();
        let mut lines = 0;
        while lines < LINES_PER_DRAIN {
            let Ok(file) = run.receiver.try_recv() else {
                break;
            };
            lines += file.hits.len();
            files.push(FileNode {
                label: tilde(&file.path, home.as_deref()),
                target: FileTarget::Path(file.path),
                hits: file.hits,
                summary: None,
            });
        }
        let results = &self.inner.results;
        if !files.is_empty() {
            run.first.get_or_insert_with(|| run.started.elapsed());
            results.add_files(&run.search, files);
            let heading =
                results_heading(&run.pattern, run.search.hits(), run.search.files(), None);
            results.set_heading(&run.search, heading);
            results.status.set_label(&format!(
                "Searching {}… {} so far",
                run.folder,
                hits(run.search.hits())
            ));
            run.slowest_drain = run.slowest_drain.max(started.elapsed());
        }
        if !finished || !run.receiver.is_empty() {
            return glib::ControlFlow::Continue;
        }
        let run = slot.take().expect("checked above");
        drop(slot);
        let summary = run.handle.wait();
        let total = run.started.elapsed();
        let heading = results_heading(
            &run.pattern,
            run.search.hits(),
            run.search.files(),
            Some(summary.files_searched),
        );
        results.set_heading(&run.search, heading.clone());
        let mut status = format!("{heading} in {:.2} s", total.as_secs_f64());
        if summary.cancelled {
            status.push_str(" — stopped");
        } else if summary.truncated {
            status.push_str(&format!(" — stopped at {} hits", summary.hits));
        }
        if !summary.errors.is_empty() {
            let count = summary.errors.len();
            status.push_str(&format!(
                " — {count} {} could not be read",
                if count == 1 { "file" } else { "files" }
            ));
            let details: Vec<String> = summary
                .errors
                .iter()
                .take(20)
                .map(|error| format!("{}: {}", error.path.display(), error.message))
                .collect();
            results.status.set_tooltip_text(Some(&details.join("\n")));
        } else {
            results.status.set_tooltip_text(None);
        }
        results.status.set_label(&status);
        results.stop.set_visible(false);
        if let Some(action) = self.action(stet_domain::actions::ActionId::StopFindInFiles) {
            action.set_enabled(false);
        }
        tracing::info!(
            files_searched = summary.files_searched,
            files_matched = summary.files_matched,
            hits = summary.hits,
            truncated = summary.truncated,
            cancelled = summary.cancelled,
            first_result_ms = run.first.map(|first| first.as_secs_f64() * 1e3),
            total_ms = total.as_secs_f64() * 1e3,
            "find in files"
        );
        self.inner.fif_stats.replace(Some(FifStats {
            first_found: run.found_first.get().copied(),
            first_result: run.first,
            total,
            search_ended_after_stop: run.search_ended,
            stopped_after: run.cancel_requested.map(|at| at.elapsed()),
            slowest_drain: run.slowest_drain,
            summary,
        }));
        glib::ControlFlow::Break
    }

    /// Stops the running Find in Files; its results so far stay in the panel.
    pub(super) fn stop_find_in_files(&self) {
        if let Some(run) = self.inner.fif.borrow_mut().as_mut() {
            run.cancel_requested.get_or_insert_with(Instant::now);
            run.handle.cancel();
        }
    }

    /// Whether a Find in Files search is running.
    pub fn find_in_files_running(&self) -> bool {
        self.inner.fif.borrow().is_some()
    }

    pub fn find_in_files_stats(&self) -> Option<FifStats> {
        self.inner.fif_stats.borrow().clone()
    }
}

fn hits(count: usize) -> String {
    if count == 1 {
        "1 hit".to_owned()
    } else {
        format!("{count} hits")
    }
}

/// `~` and `~/…` name the home folder.
pub fn expand_home(text: &str) -> PathBuf {
    match (text.strip_prefix('~'), std::env::home_dir()) {
        (Some(""), Some(home)) => home,
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(text),
    }
}

/// Whether Find in Files searches an open document's text, as the tab shows it, instead of its
/// file: with unsaved changes, and whenever reading the file gives other text. The search reads
/// only UTF-8 and UTF-16 with a byte-order mark as text, skips files with NUL bytes, and counts
/// lines by LF; the tab shows a decoding error as U+FFFD.
fn searched_from_memory(state: &DocState, dirty: bool) -> bool {
    dirty
        || !find_in_files::reads_as_text(state.format.encoding, state.format.bom)
        || state.lossy()
        || state.nul_count > 0
        || state.eol_stats.cr > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use stet_infrastructure::encoding::Encoding;

    #[test]
    fn documents_that_the_file_search_would_read_differently_come_from_memory() {
        let clean = DocState::default();
        assert!(!searched_from_memory(&clean, false));
        assert!(searched_from_memory(&clean, true));
        let with = |label: &str, bom: bool| {
            let mut state = DocState::default();
            state.format.encoding = Encoding::for_label(label.as_bytes()).unwrap();
            state.format.bom = bom;
            searched_from_memory(&state, false)
        };
        assert!(!with("utf-8", true));
        assert!(!with("utf-16le", true));
        assert!(with("utf-16le", false));
        assert!(with("windows-1252", false));
        assert!(with("shift_jis", false));
        for change in [
            |state: &mut DocState| state.nul_count = 1,
            |state: &mut DocState| state.eol_stats.cr = 1,
            |state: &mut DocState| state.had_errors = true,
        ] {
            let mut state = DocState::default();
            change(&mut state);
            assert!(searched_from_memory(&state, false), "{state:?}");
        }
    }
}
