//! Replace in Files (ADR-005 amendment): walk folders as Find in Files does, and replace in
//! every file with the in-document engine, keeping each file's encoding, byte-order mark and
//! line endings byte for byte outside the replacements.
//!
//! - The walk, the filters and the options are Find in Files' ([`super::find`]):
//!   hidden files, `.gitignore`, subfolders, `!` exclusions. Symbolic links are not followed;
//!   a file reached by its own path is written through [`crate::fs::safe_save`], which keeps
//!   links that point at it, its mode and its owner.
//! - Each file is matched as the editor would open it: decoded, normalized to LF, NUL as
//!   U+2400, so the query means what it means in an open document, Extended `\r\n` included.
//!   Only the replacements are encoded; every other byte is copied from the file
//!   (`plan::plan`).
//! - Files left alone, with the reason ([`stet_domain::search::replace_files::Skip`]): binary, lossy
//!   or malformed decodes, replacements the encoding can't represent, read-only files, files
//!   over the size limit, files that changed between reading and writing, and the rare edits
//!   that would join a lone CR and a lone LF.
//! - Open documents ([`RifRequest::open_documents`]) are not written: when the walk reaches one
//!   of their files it reports [`RifEvent::OpenDocument`], and the app replaces in the buffer.
//! - Cancelling stops the walk and the matching; a file is either written whole or not at all,
//!   and every file written reaches the sink.

mod plan;
#[cfg(test)]
mod tests;

pub use plan::replaced_hits;
pub(crate) use plan::{FileOptions, FileOutcome, replace_file};

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use ignore::{DirEntry, WalkState};
use stet_domain::search::replace_files::Skip;
use stet_domain::search::{Query, Template};

use super::job::{self, Control, JobKind, Reporter};
use super::open::OpenIndex;
use super::walk::{Root, WalkOptions};
use super::{FileError, JobHandle, StartError};
use crate::fs::{SaveMethod, SaveOptions};
use crate::search::{Hit, Matcher};

/// Files from this size are skipped: the large-file threshold, where the editor itself turns
/// features off. Such a file can be opened and replaced in as a document.
pub const MAX_FILE_BYTES: u64 = 50 << 20;

/// How many replaced lines one file reports at most.
pub const FILE_HIT_LIMIT: usize = 1_000;

/// What Replace in Files replaces, where, and how.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RifRequest {
    /// Folders (or files) to replace in.
    pub roots: Vec<PathBuf>,
    /// The filter list, as in Find in Files.
    pub filters: String,
    /// Replace in hidden files and folders too.
    pub include_hidden: bool,
    /// Skip what `.gitignore`, `.ignore` and git's excludes ignore (inside git repositories).
    pub respect_gitignore: bool,
    /// Descend into subfolders of the roots.
    pub include_subfolders: bool,
    /// The search text and its options.
    pub query: Query,
    /// The replacement, with its references to the match's groups.
    pub template: Template,
    /// The files of open documents, which the run reports instead of writing. The run resolves
    /// symbolic links in these paths.
    pub open_documents: Vec<PathBuf>,
    /// Where an in-place save keeps the previous content while it writes.
    pub backup_dir: PathBuf,
    /// Files from this size are skipped.
    pub max_file_bytes: u64,
}

impl RifRequest {
    /// Every file under `roots`, with subfolders, without hidden files, respecting
    /// `.gitignore`, up to [`MAX_FILE_BYTES`].
    #[must_use]
    pub fn new(roots: Vec<PathBuf>, query: Query, template: Template, backup_dir: PathBuf) -> Self {
        let walk = WalkOptions::default();
        Self {
            roots,
            filters: String::new(),
            include_hidden: walk.include_hidden,
            respect_gitignore: walk.respect_gitignore,
            include_subfolders: walk.include_subfolders,
            query,
            template,
            open_documents: Vec::new(),
            backup_dir,
            max_file_bytes: MAX_FILE_BYTES,
        }
    }

    fn walk_options(&self) -> WalkOptions {
        WalkOptions {
            include_hidden: self.include_hidden,
            respect_gitignore: self.respect_gitignore,
            include_subfolders: self.include_subfolders,
        }
    }
}

/// What the run reports, one file at a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RifEvent {
    /// Replaced; `written` is false when every replacement equalled its match.
    Replaced {
        /// The file replaced in, with its links resolved.
        path: PathBuf,
        /// How many matches were replaced.
        replacements: usize,
        /// Whether the file was written.
        written: bool,
        /// The replaced text's lines, in the new text.
        hits: Vec<Hit>,
        /// How the file was written, when it was.
        method: Option<SaveMethod>,
    },
    /// Left alone for `reason`.
    Skipped {
        /// The file left alone.
        path: PathBuf,
        /// Why it was left alone.
        reason: Skip,
        /// The matches, when they were counted before the file was left alone.
        matches: Option<usize>,
    },
    /// Reading, replacing in or writing the file failed.
    Failed {
        /// The file that failed.
        path: PathBuf,
        /// What went wrong.
        error: FileError,
    },
    /// The file of an open document (its path with links resolved): the app replaces in the
    /// document's text.
    OpenDocument {
        /// The document's file.
        path: PathBuf,
    },
}

/// Counts so far, while the run goes on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RifProgress {
    /// Files searched so far.
    pub files_searched: usize,
    /// Files written so far.
    pub files_changed: usize,
    /// Replacements made so far.
    pub replacements: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
/// How a Replace in Files run ended.
pub struct RifSummary {
    /// How many files were searched.
    pub files_searched: usize,
    /// Files written.
    pub files_changed: usize,
    /// Replacements in the files written or reported unchanged; open documents not counted.
    pub replacements: usize,
    /// Files left alone.
    pub skipped: usize,
    /// Files that failed.
    pub failed: usize,
    /// Open documents the run handed back to the app.
    pub open_documents: usize,
    /// The run was cancelled before it finished.
    pub cancelled: bool,
    /// How long the run took.
    pub elapsed: Duration,
}

impl RifSummary {
    fn panicked() -> Self {
        Self {
            failed: 1,
            ..Self::default()
        }
    }
}

/// A running Replace in Files. Dropping it cancels the run.
pub type RifHandle = JobHandle<RifSummary, Arc<RifCounters>>;

impl RifHandle {
    /// The counts so far, read without stopping the run.
    #[must_use]
    pub fn progress(&self) -> RifProgress {
        let counters = self.progress_state();
        RifProgress {
            files_searched: counters.files_searched.load(Ordering::Relaxed),
            files_changed: counters.files_changed.load(Ordering::Relaxed),
            replacements: counters.replacements.load(Ordering::Relaxed),
        }
    }
}

/// A running Replace in Files' counts, which its handle reads for [`RifProgress`].
#[derive(Debug, Default)]
pub struct RifCounters {
    files_searched: AtomicUsize,
    files_changed: AtomicUsize,
    replacements: AtomicUsize,
    skipped: AtomicUsize,
    failed: AtomicUsize,
    open_documents: AtomicUsize,
}

/// Starts a run on its own threads. `sink` gets each file's event from a thread of its own.
pub fn start<S>(request: RifRequest, sink: S) -> Result<RifHandle, StartError>
where
    S: FnMut(RifEvent) + Send + 'static,
{
    let started = Instant::now();
    let matcher = Matcher::new(&request.query)?;
    let roots = Root::parse_all(&request.roots, &request.filters)?;
    let options = request.walk_options();
    let control = Control::default();
    let counters = Arc::new(RifCounters::default());
    let (job_control, job_counters) = (control.clone(), Arc::clone(&counters));
    job::spawn(
        JobKind::ReplaceInFiles,
        control,
        counters,
        RifSummary::panicked,
        sink,
        move |reporter| {
            let shared = Shared {
                open: OpenIndex::new(request.open_documents.into_iter().map(|path| (path, ()))),
                matcher,
                template: request.template,
                options: FileOptions {
                    max_bytes: request.max_file_bytes,
                    save: SaveOptions::new(request.backup_dir),
                    hit_limit: FILE_HIT_LIMIT,
                },
                control: job_control,
                counters: job_counters,
            };
            shared.run(&roots, options, reporter, started)
        },
    )
}

struct Shared {
    matcher: Matcher,
    template: Template,
    options: FileOptions,
    open: OpenIndex<()>,
    control: Control,
    counters: Arc<RifCounters>,
}

impl Shared {
    fn run(
        &self,
        roots: &[Root],
        options: WalkOptions,
        reporter: Reporter<RifEvent>,
        started: Instant,
    ) -> RifSummary {
        for root in roots {
            if self.control.stopping() {
                break;
            }
            self.walk(root, options, &reporter);
        }
        reporter.finish();
        let counters = &self.counters;
        RifSummary {
            files_searched: counters.files_searched.load(Ordering::SeqCst),
            files_changed: counters.files_changed.load(Ordering::SeqCst),
            replacements: counters.replacements.load(Ordering::SeqCst),
            skipped: counters.skipped.load(Ordering::SeqCst),
            failed: counters.failed.load(Ordering::SeqCst),
            open_documents: counters.open_documents.load(Ordering::SeqCst),
            cancelled: self.control.cancelled(),
            elapsed: started.elapsed(),
        }
    }

    fn walk(&self, root: &Root, options: WalkOptions, reporter: &Reporter<RifEvent>) {
        root.builder(options)
            .build_parallel()
            .run(|| Box::new(move |entry| self.visit(root, entry, reporter)));
    }

    fn visit(
        &self,
        root: &Root,
        entry: Result<DirEntry, ignore::Error>,
        reporter: &Reporter<RifEvent>,
    ) -> WalkState {
        if self.control.stopping() {
            return WalkState::Quit;
        }
        let entry = match root.file(entry) {
            Ok(Some(entry)) => entry,
            Ok(None) => return WalkState::Continue,
            Err(error) => {
                self.counters.failed.fetch_add(1, Ordering::Relaxed);
                reporter.report(RifEvent::Failed {
                    path: error.path,
                    error: error.error,
                });
                return WalkState::Continue;
            }
        };
        let path = entry.path();
        self.counters.files_searched.fetch_add(1, Ordering::Relaxed);
        if let Some((canonical, ())) = self.open.get(path) {
            self.counters.open_documents.fetch_add(1, Ordering::Relaxed);
            reporter.report(RifEvent::OpenDocument { path: canonical });
            return WalkState::Continue;
        }
        let outcome = replace_file(
            path,
            &self.matcher,
            &self.template,
            &self.options,
            self.control.stop_token(),
        );
        let counters = &self.counters;
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
                    counters.files_changed.fetch_add(1, Ordering::Relaxed);
                }
                counters
                    .replacements
                    .fetch_add(replacements, Ordering::Relaxed);
                RifEvent::Replaced {
                    path: path.to_path_buf(),
                    replacements,
                    written,
                    hits,
                    method,
                }
            }
            FileOutcome::Skipped { reason, matches } => {
                counters.skipped.fetch_add(1, Ordering::Relaxed);
                RifEvent::Skipped {
                    path: path.to_path_buf(),
                    reason,
                    matches,
                }
            }
            FileOutcome::Failed(error) => {
                counters.failed.fetch_add(1, Ordering::Relaxed);
                RifEvent::Failed {
                    path: path.to_path_buf(),
                    error,
                }
            }
        };
        reporter.report(event);
        if self.control.stopping() {
            WalkState::Quit
        } else {
            WalkState::Continue
        }
    }
}
