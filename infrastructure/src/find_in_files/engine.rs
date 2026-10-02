use std::fmt;
use std::io::{self, Read};
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grep_matcher::{ByteSet, LineMatchKind, LineTerminator, Match, Matcher, NoCaptures};
use grep_searcher::{Searcher, Sink, SinkMatch};
use stet_domain::search::{Query, SearchMode, Target, TranslateError, translate_for, unescape};

use crate::search::{Hit, JIT_STACK, PatternError, make_hit, next_boundary, pcre2_message};

/// How files are matched: Rust's regex engine for plain text (fast literal search), PCRE2
/// with the in-document dialect for regexes and whole word.
enum Engine {
    Text(grep_regex::RegexMatcher),
    Pcre(grep_pcre2::RegexMatcher),
}

/// The matcher for one Find in Files run. It stops matching once the run is stopped, so a
/// search inside a large file ends within one match.
pub(crate) struct FileMatcher {
    engine: Engine,
    stop: Arc<AtomicBool>,
    /// Matches can span lines or use whole-file anchors: search whole files, not lines.
    pub(crate) whole_file: bool,
}

#[derive(Debug)]
pub(crate) struct MatchError(String);

impl fmt::Display for MatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn pattern_error(error: impl fmt::Display) -> PatternError {
    PatternError {
        message: pcre2_message(error),
        offset: 0,
    }
}

impl FileMatcher {
    pub(crate) fn new(query: &Query, stop: Arc<AtomicBool>) -> Result<Self, PatternError> {
        let translated = translate_for(query, Target::Files)?;
        let options = query.options;
        let text = match options.mode {
            SearchMode::Normal if !options.whole_word => Some(query.pattern.clone()),
            SearchMode::Extended if !options.whole_word => {
                Some(unescape(&query.pattern).map_err(TranslateError::from)?)
            }
            _ => None,
        };
        let engine = match text {
            Some(text) => {
                let mut builder = grep_regex::RegexMatcherBuilder::new();
                builder.case_insensitive(!translated.case_sensitive);
                let matcher = if text.contains(['\n', '\r']) {
                    builder.build(&text_regex(&text))
                } else {
                    builder
                        .fixed_strings(true)
                        .line_terminator(Some(b'\n'))
                        .build(&text)
                };
                Engine::Text(matcher.map_err(pattern_error)?)
            }
            None => {
                let mut builder = grep_pcre2::RegexMatcherBuilder::new();
                builder
                    .caseless(!translated.case_sensitive)
                    .multi_line(true)
                    .crlf(true)
                    .ucp(true)
                    .utf(true)
                    .jit_if_available(true)
                    .max_jit_stack_size(Some(JIT_STACK));
                Engine::Pcre(builder.build(&translated.pattern).map_err(pattern_error)?)
            }
        };
        Ok(Self {
            engine,
            stop,
            whole_file: translated.needs_whole_text,
        })
    }

    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    fn find(&self, haystack: &[u8], at: usize) -> Result<Option<Match>, MatchError> {
        match &self.engine {
            Engine::Text(matcher) => matcher
                .find_at(haystack, at)
                .map_err(|_| MatchError(String::new())),
            Engine::Pcre(matcher) => matcher
                .find_at(haystack, at)
                .map_err(|error| MatchError(pcre2_message(error))),
        }
    }

    /// The matches that start in `range` of `buffer`, which the searcher reported as lines.
    /// After an empty match the search goes on at the next character.
    pub(crate) fn ranges(&self, buffer: &[u8], range: Range<usize>) -> Vec<(usize, usize)> {
        let mut found = Vec::new();
        let mut at = range.start;
        while at <= buffer.len() && !self.stopped() {
            let Ok(Some(m)) = self.find(buffer, at) else {
                break;
            };
            let past =
                m.start() > range.end || (m.start() == range.end && range.end < buffer.len());
            if past {
                break;
            }
            found.push((m.start(), m.end()));
            at = if m.is_empty() {
                next_boundary(buffer, m.end())
            } else {
                m.end()
            };
        }
        found
    }
}

impl Matcher for FileMatcher {
    type Captures = NoCaptures;
    type Error = MatchError;

    fn find_at(&self, haystack: &[u8], at: usize) -> Result<Option<Match>, MatchError> {
        if self.stopped() {
            return Ok(None);
        }
        self.find(haystack, at)
    }

    fn new_captures(&self) -> Result<NoCaptures, MatchError> {
        Ok(NoCaptures::new())
    }

    fn shortest_match_at(&self, haystack: &[u8], at: usize) -> Result<Option<usize>, MatchError> {
        if self.stopped() {
            return Ok(None);
        }
        match &self.engine {
            Engine::Text(matcher) => matcher
                .shortest_match_at(haystack, at)
                .map_err(|_| MatchError(String::new())),
            Engine::Pcre(matcher) => matcher
                .shortest_match_at(haystack, at)
                .map_err(|error| MatchError(pcre2_message(error))),
        }
    }

    fn non_matching_bytes(&self) -> Option<&ByteSet> {
        match &self.engine {
            Engine::Text(matcher) => matcher.non_matching_bytes(),
            Engine::Pcre(matcher) => matcher.non_matching_bytes(),
        }
    }

    /// A pattern that cannot match a line break (the translation's `needs_whole_text` is
    /// false) never matches `\n`; saying so lets grep scan whole buffers instead of calling
    /// PCRE2 once per line.
    fn line_terminator(&self) -> Option<LineTerminator> {
        match &self.engine {
            _ if !self.whole_file => Some(LineTerminator::byte(b'\n')),
            Engine::Text(matcher) => matcher.line_terminator(),
            Engine::Pcre(matcher) => matcher.line_terminator(),
        }
    }

    fn find_candidate_line(&self, haystack: &[u8]) -> Result<Option<LineMatchKind>, MatchError> {
        if self.stopped() {
            return Ok(None);
        }
        match &self.engine {
            Engine::Text(matcher) => matcher
                .find_candidate_line(haystack)
                .map_err(|_| MatchError(String::new())),
            Engine::Pcre(matcher) => matcher
                .find_candidate_line(haystack)
                .map_err(|error| MatchError(pcre2_message(error))),
        }
    }
}

/// Plain text with line breaks, for Rust's regex syntax: a line break matches CRLF, LF or CR.
fn text_regex(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' => {
                if c == '\r' {
                    chars.next_if_eq(&'\n');
                }
                out.push_str(r"(?:\r\n|\n|\r)");
            }
            c if c.is_ascii_control() => out.push_str(&format!(r"\x{{{:X}}}", u32::from(c))),
            c => {
                if matches!(
                    c,
                    '\\' | '.'
                        | '+'
                        | '*'
                        | '?'
                        | '('
                        | ')'
                        | '|'
                        | '['
                        | ']'
                        | '{'
                        | '}'
                        | '^'
                        | '$'
                        | '#'
                        | '&'
                        | '-'
                        | '~'
                ) {
                    out.push('\\');
                }
                out.push(c);
            }
        }
    }
    out
}

/// Collects one file's hits. Binary files are dropped whole: grep stops at the first NUL,
/// so their earlier hits never leave the file.
pub(crate) struct FileSink<'a> {
    matcher: &'a FileMatcher,
    pub(crate) hits: Vec<Hit>,
    pub(crate) matches: usize,
    pub(crate) binary: bool,
}

impl<'a> FileSink<'a> {
    pub(crate) fn new(matcher: &'a FileMatcher) -> Self {
        Self {
            matcher,
            hits: Vec::new(),
            matches: 0,
            binary: false,
        }
    }
}

impl Sink for FileSink<'_> {
    type Error = io::Error;

    fn matched(&mut self, _searcher: &Searcher, found: &SinkMatch<'_>) -> Result<bool, io::Error> {
        if self.matcher.stopped() {
            return Ok(false);
        }
        let buffer = found.buffer();
        let range = found.bytes_range_in_buffer();
        let spans = self.matcher.ranges(buffer, range.clone());
        let mut spans = spans.iter().peekable();
        let mut line_start = range.start;
        for (number, line) in (found.line_number().unwrap_or(1)..).zip(found.lines()) {
            let line_end = line_start + line.len();
            let open_end = !line.ends_with(b"\n");
            let mut ranges = Vec::new();
            while let Some(&(start, end)) =
                spans.next_if(|&&(start, _)| start < line_end || (open_end && start == line_end))
            {
                ranges.push(start - line_start..end.max(start) - line_start);
            }
            if !ranges.is_empty() {
                self.matches += ranges.len();
                self.hits.push(make_hit(number, line, &ranges));
            }
            line_start = line_end;
        }
        Ok(true)
    }

    fn binary_data(&mut self, _searcher: &Searcher, _offset: u64) -> Result<bool, io::Error> {
        self.binary = true;
        Ok(false)
    }
}

/// A reader that fails once the run is stopped, so reading a large file ends promptly.
pub(crate) struct CancelRead<'a, R> {
    pub(crate) inner: R,
    pub(crate) stop: &'a AtomicBool,
}

impl<R: Read> Read for CancelRead<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(io::Error::other("cancelled"));
        }
        self.inner.read(buf)
    }
}
