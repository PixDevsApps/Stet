//! Find in Files: walk folders with `ignore` and search each file with `grep-searcher`.
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
//!   match; a single match scan over one large file cannot be interrupted.
//! - Open documents ([`FifRequest::open_documents`]) are searched from their text instead of
//!   the file on disk, with the in-document matcher, when the walk reaches their file. The app
//!   passes those with unsaved changes and those whose file the walk would not read as the
//!   text they show ([`reads_as_text`]).
//!
//! [`Target::Files`]: stet_domain::search::Target::Files

mod engine;
pub(crate) mod filters;
#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::ffi::OsString;
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crossbeam_channel::Sender;
use encoding_rs::{Encoding, UTF_8, UTF_16BE, UTF_16LE};
use grep_searcher::{BinaryDetection, Searcher, SearcherBuilder};
use ignore::{DirEntry, WalkBuilder, WalkState};
use stet_domain::search::Query;

use crate::search::{Hit, Matcher, PatternError, SearchError, document_hits};
use engine::{CancelRead, FileMatcher, FileSink};
use filters::Filters;

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
    /// The file's path with symlinks resolved, as `std::fs::canonicalize` gives it.
    pub path: PathBuf,
    /// The buffer text: LF line breaks, NUL as U+2400 (ADR-007, ADR-008).
    pub text: Arc<str>,
}

impl FifRequest {
    /// Every file under `roots`, with subfolders, without hidden files, respecting
    /// `.gitignore`, up to [`DEFAULT_MAX_HITS`] matches.
    pub fn new(roots: Vec<PathBuf>, query: Query) -> Self {
        Self {
            roots,
            filters: String::new(),
            include_hidden: false,
            respect_gitignore: true,
            include_subfolders: true,
            query,
            max_hits: DEFAULT_MAX_HITS,
            open_documents: Vec::new(),
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
    pub errors: Vec<FifFileError>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FifFileError {
    pub path: PathBuf,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FifError {
    Pattern(PatternError),
    Filter(String),
    Thread(String),
}

impl std::fmt::Display for FifError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Pattern(error) => error.fmt(f),
            Self::Filter(message) => write!(f, "invalid filter: {message}"),
            Self::Thread(message) => write!(f, "could not start the search: {message}"),
        }
    }
}

impl std::error::Error for FifError {}

/// A running search. Dropping it cancels the search.
pub struct FifHandle {
    stop: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<FifSummary>>,
}

impl FifHandle {
    /// Stops the search; the sink gets no more files.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        self.stop.store(true, Ordering::SeqCst);
    }

    pub fn is_finished(&self) -> bool {
        self.thread.as_ref().is_none_or(JoinHandle::is_finished)
    }

    /// Waits for the search, and for the sink's last call, to finish.
    pub fn wait(mut self) -> FifSummary {
        let Some(thread) = self.thread.take() else {
            return FifSummary::default();
        };
        thread.join().unwrap_or_else(|_| FifSummary {
            errors: vec![FifFileError {
                path: PathBuf::new(),
                message: "the search thread panicked".to_owned(),
            }],
            ..FifSummary::default()
        })
    }
}

impl Drop for FifHandle {
    fn drop(&mut self) {
        if self.thread.is_some() {
            self.cancel();
        }
    }
}

/// Starts a search on its own threads. `sink` gets each matching file's hits, one file at a
/// time, from a thread of its own.
pub fn start<S>(request: FifRequest, sink: S) -> Result<FifHandle, FifError>
where
    S: FnMut(FileHits) + Send + 'static,
{
    let started = Instant::now();
    let buffer_matcher = Matcher::new(&request.query).map_err(FifError::Pattern)?;
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let matcher = FileMatcher::new(&request.query, Arc::clone(&stop)).map_err(FifError::Pattern)?;
    let walks = request
        .roots
        .iter()
        .map(|root| {
            Filters::new(root, &request.filters)
                .map(|filters| (root.clone(), filters))
                .map_err(|error| FifError::Filter(error.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let shared = Arc::new(Shared {
        matcher,
        open: OpenTexts::new(&request.open_documents, buffer_matcher),
        stop: Arc::clone(&stop),
        max_hits: request.max_hits,
        files_searched: AtomicUsize::new(0),
        files_matched: AtomicUsize::new(0),
        hits: AtomicUsize::new(0),
        truncated: AtomicBool::new(false),
        errors: Mutex::new(Vec::new()),
    });
    let run_cancelled = Arc::clone(&cancelled);
    let thread = thread::Builder::new()
        .name("stet-find-in-files".to_owned())
        .spawn(move || run(&request, walks, &shared, &run_cancelled, sink, started))
        .map_err(|error| FifError::Thread(error.to_string()))?;
    Ok(FifHandle {
        stop,
        cancelled,
        thread: Some(thread),
    })
}

/// The open documents searched from memory, by canonical path.
struct OpenTexts {
    texts: HashMap<PathBuf, Arc<str>>,
    /// Their file names, so that only likely candidates are canonicalized.
    names: HashSet<OsString>,
    /// The in-document matcher, for buffer text.
    matcher: Option<Matcher>,
}

impl OpenTexts {
    fn new(documents: &[OpenDocument], matcher: Matcher) -> Self {
        let texts: HashMap<PathBuf, Arc<str>> = documents
            .iter()
            .map(|document| (document.path.clone(), Arc::clone(&document.text)))
            .collect();
        let names = texts
            .keys()
            .filter_map(|path| path.file_name().map(OsString::from))
            .collect();
        Self {
            matcher: (!texts.is_empty()).then_some(matcher),
            texts,
            names,
        }
    }

    /// The text to search instead of the file at `path`, if it is an open document.
    fn text_for(&self, path: &Path) -> Option<Arc<str>> {
        if self.texts.is_empty() || !self.names.contains(path.file_name()?) {
            return None;
        }
        let canonical = std::fs::canonicalize(path).ok()?;
        self.texts.get(&canonical).cloned()
    }
}

struct Shared {
    matcher: FileMatcher,
    open: OpenTexts,
    stop: Arc<AtomicBool>,
    max_hits: usize,
    files_searched: AtomicUsize,
    files_matched: AtomicUsize,
    hits: AtomicUsize,
    truncated: AtomicBool,
    errors: Mutex<Vec<FifFileError>>,
}

fn run<S>(
    request: &FifRequest,
    walks: Vec<(PathBuf, Filters)>,
    shared: &Arc<Shared>,
    cancelled: &Arc<AtomicBool>,
    mut sink: S,
    started: Instant,
) -> FifSummary
where
    S: FnMut(FileHits) + Send + 'static,
{
    let (sender, receiver) = crossbeam_channel::unbounded::<FileHits>();
    let dispatch_cancelled = Arc::clone(cancelled);
    let dispatcher = thread::Builder::new()
        .name("stet-find-in-files-sink".to_owned())
        .spawn(move || {
            for batch in receiver {
                if dispatch_cancelled.load(Ordering::Relaxed) {
                    break;
                }
                sink(batch);
            }
        });
    for (root, filters) in walks {
        if shared.stop.load(Ordering::Relaxed) {
            break;
        }
        walk(request, &root, &filters, shared, &sender);
    }
    drop(sender);
    match dispatcher {
        Ok(dispatcher) => {
            let _ = dispatcher.join();
        }
        Err(error) => shared.error(Path::new(""), format!("results thread: {error}")),
    }
    let errors = std::mem::take(
        &mut *shared
            .errors
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    );
    FifSummary {
        files_searched: shared.files_searched.load(Ordering::SeqCst),
        files_matched: shared.files_matched.load(Ordering::SeqCst),
        hits: shared.hits.load(Ordering::SeqCst),
        truncated: shared.truncated.load(Ordering::SeqCst),
        cancelled: cancelled.load(Ordering::SeqCst),
        elapsed: started.elapsed(),
        errors,
    }
}

fn walk(
    request: &FifRequest,
    root: &Path,
    filters: &Filters,
    shared: &Arc<Shared>,
    sender: &Sender<FileHits>,
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
    if shared.matcher.whole_file {
        builder.max_filesize(Some(MAX_WHOLE_FILE_BYTES));
    }
    builder.build_parallel().run(|| {
        let shared = Arc::clone(shared);
        let sender = sender.clone();
        let filters = filters.clone();
        let mut searcher = SearcherBuilder::new()
            .line_number(true)
            .multi_line(shared.matcher.whole_file)
            .binary_detection(BinaryDetection::quit(b'\0'))
            .build();
        Box::new(move |entry| shared.visit(entry, &filters, &mut searcher, &sender))
    });
}

impl Shared {
    fn visit(
        &self,
        entry: Result<DirEntry, ignore::Error>,
        filters: &Filters,
        searcher: &mut Searcher,
        sender: &Sender<FileHits>,
    ) -> WalkState {
        if self.stop.load(Ordering::Relaxed) {
            return WalkState::Quit;
        }
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                self.error(&error_path(&error), error.to_string());
                return WalkState::Continue;
            }
        };
        if !entry.file_type().is_some_and(|kind| kind.is_file()) || !filters.includes(entry.path())
        {
            return WalkState::Continue;
        }
        self.search(entry.path(), searcher, sender)
    }

    fn search(&self, path: &Path, searcher: &mut Searcher, sender: &Sender<FileHits>) -> WalkState {
        self.files_searched.fetch_add(1, Ordering::Relaxed);
        if let Some(text) = self.open.text_for(path) {
            return self.search_open(path, &text, sender);
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
                self.error(path, error.to_string());
                return WalkState::Continue;
            }
        };
        if is_binary(&head) {
            return WalkState::Continue;
        }
        let reader = CancelRead {
            inner: io::Cursor::new(head).chain(file),
            stop: &self.stop,
        };
        let mut sink = FileSink::new(&self.matcher);
        let result = searcher.search_reader(&self.matcher, reader, &mut sink);
        if self.stop.load(Ordering::Relaxed) {
            return WalkState::Quit;
        }
        if let Err(error) = result {
            self.error(path, error.to_string());
            return WalkState::Continue;
        }
        if sink.binary || sink.hits.is_empty() {
            return WalkState::Continue;
        }
        let granted = self.reserve(sink.matches);
        let mut hits = sink.hits;
        if granted < sink.matches {
            keep_matches(&mut hits, granted);
            self.truncated.store(true, Ordering::SeqCst);
            self.stop.store(true, Ordering::SeqCst);
        }
        if !hits.is_empty() {
            self.files_matched.fetch_add(1, Ordering::Relaxed);
            let _ = sender.send(FileHits {
                path: path.to_path_buf(),
                hits,
            });
        }
        if self.stop.load(Ordering::Relaxed) {
            WalkState::Quit
        } else {
            WalkState::Continue
        }
    }

    /// Searches an open document's text with the in-document matcher.
    fn search_open(&self, path: &Path, text: &str, sender: &Sender<FileHits>) -> WalkState {
        let Some(matcher) = &self.open.matcher else {
            return WalkState::Continue;
        };
        let remaining = self
            .max_hits
            .saturating_sub(self.hits.load(Ordering::SeqCst));
        let found = match matcher.find_all(text, None, remaining.saturating_add(1), &self.stop) {
            Ok(found) => found.matches,
            Err(SearchError::Cancelled) => return WalkState::Quit,
            Err(error) => {
                self.error(path, error.to_string());
                return WalkState::Continue;
            }
        };
        let granted = self.reserve(found.len());
        if granted < found.len() {
            self.truncated.store(true, Ordering::SeqCst);
            self.stop.store(true, Ordering::SeqCst);
        }
        if granted > 0 {
            self.files_matched.fetch_add(1, Ordering::Relaxed);
            let _ = sender.send(FileHits {
                path: path.to_path_buf(),
                hits: document_hits(text, &found[..granted]),
            });
        }
        if self.stop.load(Ordering::Relaxed) {
            WalkState::Quit
        } else {
            WalkState::Continue
        }
    }

    /// Takes up to `wanted` matches from the cap; returns how many were granted.
    fn reserve(&self, wanted: usize) -> usize {
        let mut granted = 0;
        let _ = self
            .hits
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |taken| {
                granted = wanted.min(self.max_hits.saturating_sub(taken));
                Some(taken + granted)
            });
        granted
    }

    fn error(&self, path: &Path, message: String) {
        let mut errors = self
            .errors
            .lock()
            .unwrap_or_else(|poison| poison.into_inner());
        if errors.len() < MAX_ERRORS {
            errors.push(FifFileError {
                path: path.to_path_buf(),
                message,
            });
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
pub(crate) fn file_spans(query: &Query, text: &str) -> Result<Vec<(usize, usize)>, PatternError> {
    let matcher = FileMatcher::new(query, Arc::new(AtomicBool::new(false)))?;
    Ok(matcher.ranges(text.as_bytes(), 0..text.len()))
}

pub(crate) fn error_path(error: &ignore::Error) -> PathBuf {
    match error {
        ignore::Error::WithPath { path, .. } => path.clone(),
        ignore::Error::WithDepth { err, .. } | ignore::Error::WithLineNumber { err, .. } => {
            error_path(err)
        }
        ignore::Error::Loop { child, .. } => child.clone(),
        _ => PathBuf::new(),
    }
}
