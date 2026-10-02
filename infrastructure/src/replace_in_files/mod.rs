//! Replace in Files (M8, ADR-005 amendment): walk folders as Find in Files does, and replace in
//! every file with the in-document engine, keeping each file's encoding, byte-order mark and
//! line endings byte for byte outside the replacements.
//!
//! - The walk, the filters and the options are Find in Files' ([`crate::find_in_files`]):
//!   hidden files, `.gitignore`, subfolders, `!` exclusions. Symbolic links are not followed;
//!   a file reached by its own path is written through [`crate::fs::safe_save`], which keeps
//!   links that point at it, its mode and its owner.
//! - Each file is matched as the editor would open it: decoded, normalized to LF, NUL as
//!   U+2400, so the query means what it means in an open document, Extended `\r\n` included.
//!   Only the replacements are encoded; every other byte is copied from the file
//!   ([`plan::plan`]).
//! - Files left alone, with the reason ([`stet_domain::replace_files::Skip`]): binary, lossy
//!   or malformed decodes, replacements the encoding can't represent, read-only files, files
//!   over the size limit, files that changed between reading and writing, and the rare edits
//!   that would join a lone CR and a lone LF.
//! - Open documents ([`RifRequest::open_documents`]) are not written: when the walk reaches one
//!   of their files it reports [`RifEvent::OpenDocument`], and the app replaces in the buffer.
//! - Cancelling stops the walk and the matching; a file is either written whole or not at all.

mod plan;
#[cfg(test)]
mod tests;

pub use plan::{FileOptions, FileOutcome, Plan, PlanError, plan, replace_file, replaced_hits};

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use ignore::{DirEntry, WalkBuilder, WalkState};
use stet_domain::replace_files::Skip;
use stet_domain::search::{Query, Template};

use crate::find_in_files::error_path;
use crate::find_in_files::filters::Filters;
use crate::fs::{SaveMethod, SaveOptions};
use crate::search::{Hit, Matcher, PatternError};

/// Files from this size are skipped: the large-file threshold, where the editor itself turns
/// features off. Such a file can be opened and replaced in as a document.
pub const MAX_FILE_BYTES: u64 = 50 << 20;

/// How many replaced lines one file reports at most.
pub const FILE_HIT_LIMIT: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RifRequest {
    /// Folders (or files) to replace in.
    pub roots: Vec<PathBuf>,
    /// The filter list, as in Find in Files.
    pub filters: String,
    pub include_hidden: bool,
    pub respect_gitignore: bool,
    pub include_subfolders: bool,
    pub query: Query,
    pub template: Template,
    /// The files of open documents, which the run reports instead of writing.
    pub open_documents: Vec<PathBuf>,
    /// Where an in-place save keeps the previous content while it writes.
    pub backup_dir: PathBuf,
    pub max_file_bytes: u64,
}

impl RifRequest {
    /// Every file under `roots`, with subfolders, without hidden files, respecting
    /// `.gitignore`, up to [`MAX_FILE_BYTES`].
    pub fn new(roots: Vec<PathBuf>, query: Query, template: Template, backup_dir: PathBuf) -> Self {
        Self {
            roots,
            filters: String::new(),
            include_hidden: false,
            respect_gitignore: true,
            include_subfolders: true,
            query,
            template,
            open_documents: Vec::new(),
            backup_dir,
            max_file_bytes: MAX_FILE_BYTES,
        }
    }
}

/// What the run reports, one file at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RifEvent {
    /// Replaced; `written` is false when every replacement equalled its match.
    Replaced {
        path: PathBuf,
        replacements: usize,
        written: bool,
        /// The replaced text's lines, in the new text.
        hits: Vec<Hit>,
        method: Option<SaveMethod>,
    },
    Skipped {
        path: PathBuf,
        reason: Skip,
        /// The matches, when they were counted before the file was left alone.
        matches: Option<usize>,
    },
    Failed {
        path: PathBuf,
        message: String,
    },
    /// The file of an open document (its path with links resolved): the app replaces in the
    /// document's text.
    OpenDocument {
        path: PathBuf,
    },
}

/// Counts so far, while the run goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RifProgress {
    pub files_searched: usize,
    pub files_changed: usize,
    pub replacements: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RifSummary {
    pub files_searched: usize,
    /// Files written.
    pub files_changed: usize,
    /// Replacements in the files written or reported unchanged; open documents not counted.
    pub replacements: usize,
    pub skipped: usize,
    pub failed: usize,
    pub open_documents: usize,
    pub cancelled: bool,
    pub elapsed: Duration,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RifError {
    Pattern(PatternError),
    Filter(String),
    Thread(String),
}

impl std::fmt::Display for RifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pattern(error) => error.fmt(f),
            Self::Filter(message) => write!(f, "invalid filter: {message}"),
            Self::Thread(message) => write!(f, "could not start replacing: {message}"),
        }
    }
}

impl std::error::Error for RifError {}

/// A running Replace in Files. Dropping it cancels the run.
pub struct RifHandle {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<RifSummary>>,
}

impl RifHandle {
    /// Stops the run: no file is written after this. A file being written when it stops is
    /// written whole and still reported, so every change on disk reaches the sink.
    pub fn cancel(&self) {
        self.shared.cancelled.store(true, Ordering::SeqCst);
        self.shared.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    pub fn progress(&self) -> RifProgress {
        self.shared.progress()
    }

    /// Waits for the run, and for the sink's last call, to finish.
    pub fn wait(mut self) -> RifSummary {
        let Some(thread) = self.thread.take() else {
            return RifSummary::default();
        };
        thread.join().unwrap_or_else(|_| RifSummary {
            failed: 1,
            ..RifSummary::default()
        })
    }
}

impl Drop for RifHandle {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
        }
    }
}

/// Starts a run on its own threads. `sink` gets each file's event from a thread of its own.
pub fn start<S>(request: RifRequest, sink: S) -> Result<RifHandle, RifError>
where
    S: FnMut(RifEvent) + Send + 'static,
{
    let started = Instant::now();
    let matcher = Matcher::new(&request.query).map_err(RifError::Pattern)?;
    let walks = request
        .roots
        .iter()
        .map(|root| {
            Filters::new(root, &request.filters)
                .map(|filters| (root.clone(), filters))
                .map_err(|error| RifError::Filter(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let shared = Arc::new(Shared {
        matcher,
        template: request.template.clone(),
        options: FileOptions {
            max_bytes: request.max_file_bytes,
            save: SaveOptions::new(request.backup_dir.clone()),
            hit_limit: FILE_HIT_LIMIT,
        },
        open: OnceLock::new(),
        stop: AtomicBool::new(false),
        cancelled: AtomicBool::new(false),
        files_searched: AtomicUsize::new(0),
        files_changed: AtomicUsize::new(0),
        replacements: AtomicUsize::new(0),
        skipped: AtomicUsize::new(0),
        failed: AtomicUsize::new(0),
        open_documents: AtomicUsize::new(0),
    });
    let run_shared = Arc::clone(&shared);
    let thread = thread::Builder::new()
        .name("stet-replace-in-files".to_owned())
        .spawn(move || run(&request, walks, run_shared, sink, started))
        .map_err(|error| RifError::Thread(error.to_string()))?;
    Ok(RifHandle {
        shared,
        thread: Some(thread),
    })
}

/// The open documents' files, by path with links resolved.
struct OpenFiles {
    paths: HashSet<PathBuf>,
    /// Their file names, so that only likely candidates are canonicalized.
    names: HashSet<OsString>,
}

impl OpenFiles {
    fn new(paths: &[PathBuf]) -> Self {
        let paths: HashSet<PathBuf> = paths
            .iter()
            .filter_map(|path| std::fs::canonicalize(path).ok())
            .collect();
        let names = paths
            .iter()
            .filter_map(|path| path.file_name().map(OsString::from))
            .collect();
        Self { paths, names }
    }

    /// The canonical path of `path` when it is an open document's file.
    fn find(&self, path: &Path) -> Option<PathBuf> {
        if self.paths.is_empty() || !self.names.contains(path.file_name()?) {
            return None;
        }
        let canonical = std::fs::canonicalize(path).ok()?;
        self.paths.contains(&canonical).then_some(canonical)
    }
}

struct Shared {
    matcher: Matcher,
    template: Template,
    options: FileOptions,
    /// Set by the run's thread before the walk, since canonicalizing touches the disk.
    open: OnceLock<OpenFiles>,
    stop: AtomicBool,
    cancelled: AtomicBool,
    files_searched: AtomicUsize,
    files_changed: AtomicUsize,
    replacements: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    open_documents: AtomicUsize,
}

impl Shared {
    fn progress(&self) -> RifProgress {
        RifProgress {
            files_searched: self.files_searched.load(Ordering::Relaxed),
            files_changed: self.files_changed.load(Ordering::Relaxed),
            replacements: self.replacements.load(Ordering::Relaxed),
        }
    }
}

fn run<S>(
    request: &RifRequest,
    walks: Vec<(PathBuf, Filters)>,
    shared: Arc<Shared>,
    mut sink: S,
    started: Instant,
) -> RifSummary
where
    S: FnMut(RifEvent) + Send + 'static,
{
    let _ = shared.open.set(OpenFiles::new(&request.open_documents));
    let (sender, receiver) = crossbeam_channel::unbounded::<RifEvent>();
    let dispatcher = thread::Builder::new()
        .name("stet-replace-in-files-sink".to_owned())
        .spawn(move || {
            for event in receiver {
                sink(event);
            }
        });
    for (root, filters) in walks {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        walk(request, &root, &filters, &shared, &sender);
    }
    drop(sender);
    if let Ok(dispatcher) = dispatcher {
        let _ = dispatcher.join();
    }
    RifSummary {
        files_searched: shared.files_searched.load(Ordering::SeqCst),
        files_changed: shared.files_changed.load(Ordering::SeqCst),
        replacements: shared.replacements.load(Ordering::SeqCst),
        skipped: shared.skipped.load(Ordering::SeqCst),
        failed: shared.failed.load(Ordering::SeqCst),
        open_documents: shared.open_documents.load(Ordering::SeqCst),
        cancelled: shared.cancelled.load(Ordering::SeqCst),
        elapsed: started.elapsed(),
    }
}

fn walk(
    request: &RifRequest,
    root: &Path,
    filters: &Filters,
    shared: &Arc<Shared>,
    sender: &Sender<RifEvent>,
) {
    let respect = request.respect_gitignore;
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(!request.include_hidden)
        .ignore(respect)
        .git_ignore(respect)
        .git_global(respect)
        .git_exclude(respect)
        .parents(respect)
        .follow_links(false)
        .overrides(filters.excludes.clone());
    if !request.include_subfolders {
        builder.max_depth(Some(1));
    }
    builder.build_parallel().run(|| {
        let shared = Arc::clone(shared);
        let sender = sender.clone();
        let filters = filters.clone();
        Box::new(move |entry| shared.visit(entry, &filters, &sender))
    });
}

impl Shared {
    fn visit(
        &self,
        entry: Result<DirEntry, ignore::Error>,
        filters: &Filters,
        sender: &Sender<RifEvent>,
    ) -> WalkState {
        if self.stop.load(Ordering::Relaxed) {
            return WalkState::Quit;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                let _ = sender.send(RifEvent::Failed {
                    path: error_path(&error),
                    message: error.to_string(),
                });
                return WalkState::Continue;
            }
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || !filters.includes(entry.path())
        {
            return WalkState::Continue;
        }
        let path = entry.path();
        self.files_searched.fetch_add(1, Ordering::Relaxed);
        if let Some(canonical) = self.open.get().and_then(|open| open.find(path)) {
            self.open_documents.fetch_add(1, Ordering::Relaxed);
            let _ = sender.send(RifEvent::OpenDocument { path: canonical });
            return WalkState::Continue;
        }
        let outcome = replace_file(
            path,
            &self.matcher,
            &self.template,
            &self.options,
            &self.stop,
        );
        let event = match outcome {
            FileOutcome::NoMatch => return WalkState::Continue,
            FileOutcome::Cancelled => return WalkState::Quit,
            FileOutcome::Replaced {
                replacements,
                written,
                hits,
                method,
            } => {
                if written {
                    self.files_changed.fetch_add(1, Ordering::Relaxed);
                }
                self.replacements.fetch_add(replacements, Ordering::Relaxed);
                RifEvent::Replaced {
                    path: path.to_path_buf(),
                    replacements,
                    written,
                    hits,
                    method,
                }
            }
            FileOutcome::Skipped { reason, matches } => {
                self.skipped.fetch_add(1, Ordering::Relaxed);
                RifEvent::Skipped {
                    path: path.to_path_buf(),
                    reason,
                    matches,
                }
            }
            FileOutcome::Failed(message) => {
                self.failed.fetch_add(1, Ordering::Relaxed);
                RifEvent::Failed {
                    path: path.to_path_buf(),
                    message,
                }
            }
        };
        let _ = sender.send(event);
        if self.stop.load(Ordering::Relaxed) {
            WalkState::Quit
        } else {
            WalkState::Continue
        }
    }
}
