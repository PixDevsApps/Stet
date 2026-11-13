//! Find in Files: walk folders and search each file with `grep-searcher`.
//!
//! - Normal and Extended text is matched by Rust's regex engine (`grep-regex`, fixed strings
//!   when possible); Regex mode and whole word by PCRE2 (`grep-pcre2`) with the pattern the
//!   in-document search uses, translated for files ([`Target::Files`]): a line break in the query
//!   matches CRLF, LF or CR. The only known dialect difference: `grep-pcre2` cannot take
//!   GtkSourceView's newline convention ANY, so files use ANYCRLF, and VT, FF, NEL, LS and PS
//!   are not line breaks here.
//! - Files are searched line by line, or whole when a match can span lines
//!   ([`stet_domain::search::Translated::needs_whole_text`]); whole-file searches skip files
//!   over [`MAX_WHOLE_FILE_BYTES`]. Line numbers count LF, so a CR-only file is one line.
//! - Binary files (a NUL in the first 64 KiB, or in a matching line) are skipped. UTF-16 and
//!   UTF-8 files with a BOM are decoded; other encodings are searched as bytes.
//! - Results reach the sink one file at a time, on a thread of their own, in no fixed order.
//! - Cancelling stops the walk and every search in progress within one read (64 KiB) or one
//!   match; a single match scan over one large file cannot be interrupted. The sink gets no
//!   more files.
//! - Open documents ([`FifRequest::open_documents`]) are searched from their text instead of
//!   the file on disk, with the in-document matcher, when the walk reaches their file. The app
//!   passes those with unsaved changes and those whose file the walk would not read as the
//!   text they show ([`reads_as_text`]).
//!
//! [`Target::Files`]: stet_domain::search::Target::Files

mod engine;
#[cfg(test)]
mod tests;

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::{DirEntry, WalkState};
use stet_domain::search::Query;

use super::job::{self, Control, JobKind, Reporter};
use super::open::OpenIndex;
use super::walk::{Root, WalkOptions};
use super::{FileError, JobHandle, PathError, StartError};
use crate::search::{Hit, Matcher, SearchError, document_hits};
use engine::{CancelRead, FileMatcher, FileSink};

pub const DEFAULT_MAX_HITS: usize = 50_000;
/// Searches that need whole files in memory skip larger files.
pub const MAX_WHOLE_FILE_BYTES: u64 = 256 << 20;
/// At most this many per-file errors are kept in [`FifSummary::errors`].
const MAX_ERRORS: usize = 100;
/// How much of each file is checked for binary content before searching it.
const HEAD_BYTES: usize = 64 << 10;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FifRequest {
    /// Folders (or files) to search.
    pub roots: Vec<PathBuf>,
    /// The filter list, such as `*.rs *.toml !*.min.js !\target !+\node_modules`.
    pub filters: String,
    pub include_hidden: bool,
    /// Skip what `.gitignore`, `.ignore` and git's excludes ignore (inside git repositories).
    pub respect_gitignore: bool,
    pub include_subfolders: bool,
    pub query: Query,
    /// Stop after this many matches and set [`FifSummary::truncated`].
    pub max_hits: usize,
    /// Open documents searched from their text: when the walk reaches one of their files, the
    /// document's text is searched instead of the file; the filters and options decide as for
    /// any file. A document whose file is not on disk is not searched.
    pub open_documents: Vec<OpenDocument>,
}

/// An open document's text, searched in place of its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenDocument {
    /// The file's path; the search resolves symbolic links in it.
    pub path: PathBuf,
    /// The buffer text: LF line breaks, NUL as U+2400 (ADR-007, ADR-008).
    pub text: Arc<str>,
}

impl FifRequest {
    /// Every file under `roots`, with subfolders, without hidden files, respecting
    /// `.gitignore`, up to [`DEFAULT_MAX_HITS`] matches.
    pub fn new(roots: Vec<PathBuf>, query: Query) -> Self {
        let walk = WalkOptions::default();
        Self {
            roots,
            filters: String::new(),
            include_hidden: walk.include_hidden,
            respect_gitignore: walk.respect_gitignore,
            include_subfolders: walk.include_subfolders,
            query,
            max_hits: DEFAULT_MAX_HITS,
            open_documents: Vec::new(),
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

/// The matching lines of one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHits {
    pub path: PathBuf,
    pub hits: Vec<Hit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FifSummary {
    pub files_searched: usize,
    pub files_matched: usize,
    /// Matches reported, at most the request's `max_hits`.
    pub hits: usize,
    /// The hit cap was reached; the search stopped early.
    pub truncated: bool,
    pub cancelled: bool,
    pub elapsed: Duration,
    /// Unreadable folders and files, at most 100.
    pub errors: Vec<PathError>,
}

impl FifSummary {
    fn panicked() -> Self {
        Self {
            errors: vec![PathError {
                path: PathBuf::new(),
                error: FileError::Panicked,
            }],
            ..Self::default()
        }
    }
}

/// A running Find in Files. Dropping it cancels the search.
pub type FifHandle = JobHandle<FifSummary>;

/// Starts a search on its own threads. `sink` gets each matching file's hits, one file at a
/// time, from a thread of its own.
pub fn start<S>(request: FifRequest, sink: S) -> Result<FifHandle, StartError>
where
    S: FnMut(FileHits) + Send + 'static,
{
    let started = Instant::now();
    let buffer_matcher = Matcher::new(&request.query)?;
    let control = Control::default();
    let matcher = FileMatcher::new(&request.query, control.stop_token().clone())?;
    let roots = Root::parse_all(&request.roots, &request.filters)?;
    let options = request.walk_options();
    let job_control = control.clone();
    job::spawn(
        JobKind::FindInFiles,
        control,
        (),
        FifSummary::panicked,
        sink,
        move |reporter| {
            let open = OpenIndex::new(
                request
                    .open_documents
                    .into_iter()
                    .map(|document| (document.path, document.text)),
            );
            let shared = Shared {
                matcher,
                buffer_matcher: (!open.is_empty()).then_some(buffer_matcher),
                open,
                control: job_control,
                max_hits: request.max_hits,
                files_searched: AtomicUsize::new(0),
                files_matched: AtomicUsize::new(0),
                hits: AtomicUsize::new(0),
                truncated: AtomicBool::new(false),
                errors: Mutex::new(Vec::new()),
            };
            shared.run(&roots, options, reporter, started)
        },
    )
}

struct Shared {
    matcher: FileMatcher,
    /// The open documents searched from memory.
    open: OpenIndex<Arc<str>>,
    /// The in-document matcher, for their text.
    buffer_matcher: Option<Matcher>,
    control: Control,
    max_hits: usize,
    files_searched: AtomicUsize,
    files_matched: AtomicUsize,
    hits: AtomicUsize,
    truncated: AtomicBool,
    errors: Mutex<Vec<PathError>>,
}

impl Shared {
    fn run(
        &self,
        roots: &[Root],
        options: WalkOptions,
        reporter: Reporter<FileHits>,
        started: Instant,
    ) -> FifSummary {
        for root in roots {
            if self.control.stopping() {
                break;
            }
            self.walk(root, options, &reporter);
        }
        reporter.finish();
        let errors =
            std::mem::take(&mut *self.errors.lock().unwrap_or_else(PoisonError::into_inner));
        FifSummary {
            files_searched: self.files_searched.load(Ordering::SeqCst),
            files_matched: self.files_matched.load(Ordering::SeqCst),
            hits: self.hits.load(Ordering::SeqCst),
            truncated: self.truncated.load(Ordering::SeqCst),
            cancelled: self.control.cancelled(),
            elapsed: started.elapsed(),
            errors,
        }
    }

    fn walk(&self, root: &Root, options: WalkOptions, reporter: &Reporter<FileHits>) {
        let mut builder = root.builder(options);
        if self.matcher.whole_file {
            builder.max_filesize(Some(MAX_WHOLE_FILE_BYTES));
        }
        builder.build_parallel().run(|| {
            let mut searcher = SearcherBuilder::new()
                .line_number(true)
                .multi_line(self.matcher.whole_file)
                .binary_detection(BinaryDetection::quit(b'\0'))
                .build();
            Box::new(move |entry| self.visit(root, entry, &mut searcher, reporter))
        });
    }

    fn visit(
        &self,
        root: &Root,
        entry: Result<DirEntry, ignore::Error>,
        searcher: &mut Searcher,
        reporter: &Reporter<FileHits>,
    ) -> WalkState {
        if self.control.stopping() {
            return WalkState::Quit;
        }
        match root.file(entry) {
            Ok(Some(entry)) => self.search(entry.path(), searcher, reporter),
            Ok(None) => WalkState::Continue,
            Err(error) => {
                self.error(error);
                WalkState::Continue
            }
        }
    }

    fn search(
        &self,
        path: &Path,
        searcher: &mut Searcher,
        reporter: &Reporter<FileHits>,
    ) -> WalkState {
        self.files_searched.fetch_add(1, Ordering::Relaxed);
        if let Some((_, text)) = self.open.get(path) {
            return self.search_open(path, text, reporter);
        }
        let mut head = Vec::with_capacity(HEAD_BYTES);
        let file = File::open(path).and_then(|mut file| {
            (&mut file)
                .take(HEAD_BYTES as u64)
                .read_to_end(&mut head)
                .map(|_| file)
        });
        let file = match file {
            Ok(file) => file,
            Err(error) => {
                self.file_error(path, error);
                return WalkState::Continue;
            }
        };
        if is_binary(&head) {
            return WalkState::Continue;
        }
        let reader = CancelRead {
            inner: io::Cursor::new(head).chain(file),
            stop: self.control.stop_token(),
        };
        let mut sink = FileSink::new(&self.matcher);
        let result = searcher.search_reader(&self.matcher, reader, &mut sink);
        if self.control.stopping() {
            return WalkState::Quit;
        }
        if let Err(error) = result {
            self.file_error(path, error);
            return WalkState::Continue;
        }
        if sink.binary || sink.hits.is_empty() {
            return WalkState::Continue;
        }
        let granted = self.reserve(sink.matches);
        let mut hits = sink.hits;
        if granted < sink.matches {
            keep_matches(&mut hits, granted);
            self.truncate();
        }
        if !hits.is_empty() {
            self.files_matched.fetch_add(1, Ordering::Relaxed);
            reporter.report(FileHits {
                path: path.to_path_buf(),
                hits,
            });
        }
        self.next()
    }

    /// Searches an open document's text with the in-document matcher.
    fn search_open(&self, path: &Path, text: &str, reporter: &Reporter<FileHits>) -> WalkState {
        let Some(matcher) = &self.buffer_matcher else {
            return WalkState::Continue;
        };
        let remaining = self
            .max_hits
            .saturating_sub(self.hits.load(Ordering::SeqCst));
        let stop = self.control.stop_token().flag();
        let found = match matcher.find_all(text, None, remaining.saturating_add(1), stop) {
            Ok(found) => found.matches,
            Err(SearchError::Cancelled) => return WalkState::Quit,
            Err(error) => {
                self.file_error(path, error);
                return WalkState::Continue;
            }
        };
        let granted = self.reserve(found.len());
        if granted < found.len() {
            self.truncate();
        }
        if granted > 0 {
            self.files_matched.fetch_add(1, Ordering::Relaxed);
            reporter.report(FileHits {
                path: path.to_path_buf(),
                hits: document_hits(text, &found[..granted]),
            });
        }
        self.next()
    }

    /// Takes up to `wanted` matches from the cap; returns how many were granted.
    fn reserve(&self, wanted: usize) -> usize {
        let mut taken = self.hits.load(Ordering::SeqCst);
        loop {
            let granted = wanted.min(self.max_hits.saturating_sub(taken));
            match self.hits.compare_exchange_weak(
                taken,
                taken + granted,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return granted,
                Err(actual) => taken = actual,
            }
        }
    }

    /// The hit cap is reached: the search stops, uncancelled.
    fn truncate(&self) {
        self.truncated.store(true, Ordering::SeqCst);
        self.control.stop();
    }

    /// Whether the walk goes on after a file.
    fn next(&self) -> WalkState {
        if self.control.stopping() {
            WalkState::Quit
        } else {
            WalkState::Continue
        }
    }

    fn file_error(&self, path: &Path, error: impl Into<FileError>) {
        self.error(PathError {
            path: path.to_path_buf(),
            error: error.into(),
        });
    }

    fn error(&self, error: PathError) {
        let mut errors = self.errors.lock().unwrap_or_else(PoisonError::into_inner);
        if errors.len() < MAX_ERRORS {
            errors.push(error);
        }
    }
}

/// Whether the walk reads a file in `encoding` as text: UTF-8, with or without a byte-order
/// mark, and UTF-16 with one. Files in any other encoding are searched as bytes, so an open
/// document in one of them is better searched from its text.
pub fn reads_as_text(encoding: &'static Encoding, bom: bool) -> bool {
    encoding == UTF_8 || (bom && (encoding == UTF_16LE || encoding == UTF_16BE))
}

/// A NUL in a file's first bytes means binary, unless a UTF-16 byte-order mark says the NULs
/// are text. Checked before searching, because a whole-file search would otherwise read the
/// whole file before grep's own check.
fn is_binary(head: &[u8]) -> bool {
    let utf16 = head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]);
    !utf16 && memchr::memchr(0, head).is_some()
}

/// Keeps the first `keep` matches.
fn keep_matches(hits: &mut Vec<Hit>, mut keep: usize) {
    hits.retain_mut(|hit| {
        if keep == 0 {
            return false;
        }
        let kept = hit.match_ranges.len().min(keep);
        hit.match_ranges.truncate(kept);
        keep -= kept;
        true
    });
}

/// The byte spans Find in Files reports for `query` in `text`, for the parity tests.
#[cfg(test)]
pub(crate) fn file_spans(
    query: &Query,
    text: &str,
) -> Result<Vec<(usize, usize)>, crate::search::PatternError> {
    let matcher = FileMatcher::new(query, super::CancelToken::new())?;
    Ok(matcher.ranges(text.as_bytes(), 0..text.len()))
}
