use std::fmt;
use std::ops::Range;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use pcre2::bytes::{CaptureLocations, Regex, RegexBuilder};
use stet_domain::search::{Captures, Query, TranslateError, Translated, translate};

use super::text::{CharCursor, LineCursor, char_to_byte, next_boundary};

/// GtkSourceView compiles with newline convention ANY and Unicode `\R` (`implregex.c`); the
/// `pcre2` crate has no option for either, so they lead the pattern.
pub(crate) const PREFIX: &str = "(*ANY)(*BSR_UNICODE)";
/// For the search after an empty match: an empty match at the start position is not a match.
const RETRY_PREFIX: &str = "(*NOTEMPTY_ATSTART)(*ANY)(*BSR_UNICODE)";
pub(crate) const JIT_STACK: usize = 8 << 20;
/// How far before the caret Find Previous starts looking; it grows until a match turns up.
const BACKWARD_WINDOW: usize = 64 << 10;

/// A pattern that does not compile, or a query with nothing to search for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PatternError {
    pub message: String,
    /// Character offset in the typed pattern.
    pub offset: usize,
}

impl fmt::Display for PatternError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.offset)
    }
}

impl std::error::Error for PatternError {}

impl From<TranslateError> for PatternError {
    fn from(error: TranslateError) -> Self {
        Self {
            message: error.to_string(),
            offset: error.offset(),
        }
    }
}

impl PatternError {
    /// A PCRE2 compile error for a pattern that had `prefix_len` bytes put in front of
    /// `translated.pattern`.
    pub(crate) fn from_pcre2(
        error: &pcre2::Error,
        translated: &Translated,
        prefix_len: usize,
    ) -> Self {
        let offset = error.offset().map_or(0, |offset| {
            translated.source_offset(offset.saturating_sub(prefix_len))
        });
        Self {
            message: pcre2_message(error),
            offset,
        }
    }
}

/// PCRE2's message without the crate's "PCRE2: error … at offset n:" preamble.
pub(crate) fn pcre2_message(error: impl fmt::Display) -> String {
    let text = error.to_string();
    text.strip_prefix("PCRE2: ")
        .and_then(|rest| rest.split_once(": "))
        .map_or(text.clone(), |(_, message)| message.to_owned())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchError {
    Pattern(PatternError),
    /// PCRE2 gave up while matching, for example on its JIT stack or match limit.
    Match(String),
    Cancelled,
    /// A character range outside the text of `len` characters.
    Range {
        range: Range<usize>,
        len: usize,
    },
    /// The text at the given match no longer matches.
    NotAMatch,
}

impl fmt::Display for SearchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pattern(error) => error.fmt(f),
            Self::Match(message) => write!(f, "matching failed: {message}"),
            Self::Cancelled => f.write_str("search cancelled"),
            Self::Range { range, len } => {
                write!(f, "range {range:?} is outside the text of {len} characters")
            }
            Self::NotAMatch => f.write_str("the text there no longer matches"),
        }
    }
}

impl std::error::Error for SearchError {}

impl From<PatternError> for SearchError {
    fn from(error: PatternError) -> Self {
        Self::Pattern(error)
    }
}

fn match_error(error: pcre2::Error) -> SearchError {
    SearchError::Match(pcre2_message(error))
}

/// A match in character offsets, the unit of `GtkTextIter` and [`stet_domain::text::TextEdit`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Match {
    pub start: usize,
    pub end: usize,
    /// Zero-based line of `start`.
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FindAll {
    pub matches: Vec<Match>,
    /// More matches exist beyond the limit.
    pub truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NextMatch {
    pub found: Match,
    /// The search went past the end (or, backward, the start) and continued from the other
    /// side.
    pub wrapped: bool,
}

/// A compiled query: our own PCRE2 engine over text snapshots.
///
/// Matches follow Perl: after a non-empty match the next one may be empty at its end, and after
/// an empty match the next one is the first that is not empty at the same place. So `a*` over
/// `aab` finds `aa`, the empty text before `b`, and the empty text at the end, like Python and
/// PCRE2's own global substitution.
pub struct Matcher {
    query: Query,
    translated: Translated,
    regex: Regex,
    retry: OnceLock<Option<Regex>>,
}

impl fmt::Debug for Matcher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Matcher")
            .field("query", &self.query)
            .field("pattern", &self.translated.pattern)
            .finish_non_exhaustive()
    }
}

pub(crate) fn build_regex(pattern: &str, caseless: bool) -> Result<Regex, pcre2::Error> {
    RegexBuilder::new()
        .utf(true)
        .ucp(true)
        .multi_line(true)
        .caseless(caseless)
        .jit_if_available(true)
        .max_jit_stack_size(Some(JIT_STACK))
        .build(pattern)
}

impl Matcher {
    pub fn new(query: &Query) -> Result<Self, PatternError> {
        let translated = translate(query)?;
        let regex = build_regex(
            &format!("{PREFIX}{}", translated.pattern),
            !translated.case_sensitive,
        )
        .map_err(|error| PatternError::from_pcre2(&error, &translated, PREFIX.len()))?;
        Ok(Self {
            query: query.clone(),
            translated,
            regex,
            retry: OnceLock::new(),
        })
    }

    pub fn query(&self) -> &Query {
        &self.query
    }

    pub fn translated(&self) -> &Translated {
        &self.translated
    }

    /// GtkSourceView cannot count, highlight or navigate this query; this engine must.
    pub fn needs_own_matcher(&self) -> bool {
        self.translated.needs_own_matcher
    }

    /// Every match inside `range` (characters; the whole text when `None`), at most `limit`
    /// of them. Lookbehind, `\b` and `^` see the text before the range; the range's end acts
    /// as the end of the text.
    pub fn find_all(
        &self,
        text: &str,
        range: Option<Range<usize>>,
        limit: usize,
        cancel: &AtomicBool,
    ) -> Result<FindAll, SearchError> {
        let scope = Scope::resolve(text, range)?;
        let subject = &text.as_bytes()[..scope.end];
        let mut matches = Matches::new(self, subject, scope.start);
        let mut chars = CharCursor::at(text, scope.start, scope.start_char);
        let mut lines = LineCursor::at(text, scope.start);
        let mut found = Vec::new();
        while let Some((start, end)) = matches.next()? {
            if cancel.load(Ordering::Relaxed) {
                return Err(SearchError::Cancelled);
            }
            if found.len() == limit {
                return Ok(FindAll {
                    matches: found,
                    truncated: true,
                });
            }
            found.push(Match {
                line: lines.line_at(start),
                start: chars.char_at(start),
                end: chars.char_at(end),
            });
        }
        Ok(FindAll {
            matches: found,
            truncated: false,
        })
    }

    /// The match after the caret at character `from`, or with `backward` the last one that
    /// starts before it; with `wrap`, continues from the other end.
    ///
    /// Forward, an empty match exactly at `from` is skipped: the caret is already there.
    /// Backward, the matches are those [`Matcher::find_all`] finds, but scanning starts at a
    /// line start up to 64 KiB before the caret (widening until one turns up), so with
    /// multi-line matches the split can differ from a scan of the whole text.
    pub fn find_next(
        &self,
        text: &str,
        from: usize,
        backward: bool,
        wrap: bool,
    ) -> Result<Option<NextMatch>, SearchError> {
        let from_byte = char_to_byte(text, 0, 0, from).ok_or_else(|| SearchError::Range {
            range: from..from,
            len: text.chars().count(),
        })?;
        let found = if backward {
            self.last_before(text, from_byte)?
        } else {
            self.find_after_empty(text.as_bytes(), from_byte)?
        };
        if let Some(found) = found {
            return Ok(Some(NextMatch {
                found: to_match(text, found),
                wrapped: false,
            }));
        }
        if !wrap {
            return Ok(None);
        }
        let found = if backward {
            self.last_before(text, text.len() + 1)?
        } else {
            self.find(text.as_bytes(), 0)?
        };
        Ok(found.map(|found| NextMatch {
            found: to_match(text, found),
            wrapped: true,
        }))
    }

    /// The last match starting before byte `limit`.
    fn last_before(&self, text: &str, limit: usize) -> Result<Option<(usize, usize)>, SearchError> {
        let bytes = text.as_bytes();
        let end = if self.translated.needs_whole_text {
            bytes.len()
        } else {
            let from = limit.min(bytes.len());
            memchr::memchr(b'\n', &bytes[from..]).map_or(bytes.len(), |i| from + i + 1)
        };
        let subject = &bytes[..end];
        let mut window = BACKWARD_WINDOW;
        loop {
            let low = limit.saturating_sub(window).min(bytes.len());
            let low = memchr::memrchr(b'\n', &bytes[..low]).map_or(0, |i| i + 1);
            let mut matches = Matches::new(self, subject, low);
            let mut last = None;
            while let Some((start, end)) = matches.next()? {
                if start >= limit {
                    break;
                }
                last = Some((start, end));
            }
            if last.is_some() || low == 0 {
                return Ok(last);
            }
            window = window.saturating_mul(4);
        }
    }

    pub(crate) fn find(
        &self,
        subject: &[u8],
        at: usize,
    ) -> Result<Option<(usize, usize)>, SearchError> {
        self.regex
            .find_at(subject, at)
            .map(|found| found.map(|m| (m.start(), m.end())))
            .map_err(match_error)
    }

    /// The first match at or after `at` that is not empty at `at`.
    pub(crate) fn find_after_empty(
        &self,
        subject: &[u8],
        at: usize,
    ) -> Result<Option<(usize, usize)>, SearchError> {
        match self.retry_regex() {
            Some(retry) => retry
                .find_at(subject, at)
                .map(|found| found.map(|m| (m.start(), m.end())))
                .map_err(match_error),
            None => {
                let next = next_boundary(subject, at);
                if next > subject.len() {
                    return Ok(None);
                }
                self.find(subject, next)
            }
        }
    }

    fn retry_regex(&self) -> Option<&Regex> {
        self.retry
            .get_or_init(|| {
                build_regex(
                    &format!("{RETRY_PREFIX}{}", self.translated.pattern),
                    !self.translated.case_sensitive,
                )
                .ok()
            })
            .as_ref()
    }

    pub(crate) fn locations(&self) -> Locations {
        Locations {
            main: self.regex.capture_locations(),
            retry: None,
            from_retry: false,
        }
    }

    /// Like [`Matcher::find`] or [`Matcher::find_after_empty`], filling `locations`.
    pub(crate) fn captures(
        &self,
        locations: &mut Locations,
        subject: &[u8],
        at: usize,
        after_empty: bool,
    ) -> Result<Option<(usize, usize)>, SearchError> {
        let retry = if after_empty {
            self.retry_regex()
        } else {
            None
        };
        let found = match retry {
            Some(retry) => {
                let locs = locations
                    .retry
                    .get_or_insert_with(|| retry.capture_locations());
                locations.from_retry = true;
                retry.captures_read_at(locs, subject, at)
            }
            None => {
                let at = if after_empty {
                    next_boundary(subject, at)
                } else {
                    at
                };
                if at > subject.len() {
                    return Ok(None);
                }
                locations.from_retry = false;
                self.regex
                    .captures_read_at(&mut locations.main, subject, at)
            }
        };
        found
            .map(|found| found.map(|m| (m.start(), m.end())))
            .map_err(match_error)
    }

    pub(crate) fn capture_names(&self) -> &[Option<String>] {
        self.regex.capture_names()
    }
}

/// Capture locations for both of a matcher's compiled forms.
pub(crate) struct Locations {
    main: CaptureLocations,
    retry: Option<CaptureLocations>,
    from_retry: bool,
}

impl Locations {
    pub(crate) fn get(&self, index: usize) -> Option<(usize, usize)> {
        match (&self.retry, self.from_retry) {
            (Some(retry), true) => retry.get(index),
            _ => self.main.get(index),
        }
    }
}

/// Perl-style iteration over the matches in `subject` from byte `pos`.
pub(crate) struct Matches<'m, 's> {
    matcher: &'m Matcher,
    subject: &'s [u8],
    pos: usize,
    after_empty: bool,
    done: bool,
}

impl<'m, 's> Matches<'m, 's> {
    pub(crate) fn new(matcher: &'m Matcher, subject: &'s [u8], pos: usize) -> Self {
        Self {
            matcher,
            subject,
            pos,
            after_empty: false,
            done: pos > subject.len(),
        }
    }

    pub(crate) fn next(&mut self) -> Result<Option<(usize, usize)>, SearchError> {
        if self.done {
            return Ok(None);
        }
        let found = if self.after_empty {
            self.matcher.find_after_empty(self.subject, self.pos)?
        } else {
            self.matcher.find(self.subject, self.pos)?
        };
        self.advance(found);
        Ok(found)
    }

    /// Like [`Matches::next`], with the groups in `locations`.
    pub(crate) fn next_captures(
        &mut self,
        locations: &mut Locations,
    ) -> Result<Option<(usize, usize)>, SearchError> {
        if self.done {
            return Ok(None);
        }
        let found = self
            .matcher
            .captures(locations, self.subject, self.pos, self.after_empty)?;
        self.advance(found);
        Ok(found)
    }

    fn advance(&mut self, found: Option<(usize, usize)>) {
        match found {
            Some((start, end)) => {
                self.pos = end;
                self.after_empty = start == end;
            }
            None => self.done = true,
        }
    }
}

/// A searched range in bytes.
pub(crate) struct Scope {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) start_char: usize,
}

impl Scope {
    pub(crate) fn resolve(text: &str, range: Option<Range<usize>>) -> Result<Self, SearchError> {
        let Some(range) = range else {
            return Ok(Self {
                start: 0,
                end: text.len(),
                start_char: 0,
            });
        };
        let out_of_range = || SearchError::Range {
            range: range.clone(),
            len: text.chars().count(),
        };
        if range.start > range.end {
            return Err(out_of_range());
        }
        let start = char_to_byte(text, 0, 0, range.start).ok_or_else(out_of_range)?;
        let end = char_to_byte(text, start, range.start, range.end).ok_or_else(out_of_range)?;
        Ok(Self {
            start,
            end,
            start_char: range.start,
        })
    }
}

fn to_match(text: &str, (start, end): (usize, usize)) -> Match {
    let mut chars = CharCursor::at(text, 0, 0);
    Match {
        line: LineCursor::at(text, 0).line_at(start),
        start: chars.char_at(start),
        end: chars.char_at(end),
    }
}

/// The groups of one match, for templates.
pub(crate) struct MatchCaptures<'a> {
    pub(crate) text: &'a str,
    pub(crate) locations: &'a Locations,
    pub(crate) names: &'a [Option<String>],
    pub(crate) prefix: &'a str,
    pub(crate) suffix: &'a str,
}

impl Captures for MatchCaptures<'_> {
    fn get(&self, index: usize) -> Option<&str> {
        let (start, end) = self.locations.get(index)?;
        self.text.get(start..end)
    }

    fn group_count(&self) -> usize {
        self.names.len().saturating_sub(1)
    }

    fn name_to_index(&self, name: &str) -> Option<usize> {
        let mut first = None;
        for (index, candidate) in self.names.iter().enumerate() {
            if candidate.as_deref() == Some(name) {
                if self.locations.get(index).is_some() {
                    return Some(index);
                }
                first.get_or_insert(index);
            }
        }
        first
    }

    fn prefix(&self) -> &str {
        self.prefix
    }

    fn suffix(&self) -> &str {
        self.suffix
    }

    /// Approximates Perl's `$^N` from the spans, which is all PCRE2 reports: the group that
    /// ends last, and of those the outermost.
    fn last_closed(&self) -> Option<usize> {
        (1..self.names.len())
            .filter_map(|index| {
                self.locations
                    .get(index)
                    .map(|(start, end)| (index, start, end))
            })
            .max_by(|a, b| a.2.cmp(&b.2).then(b.1.cmp(&a.1)).then(b.0.cmp(&a.0)))
            .map(|(index, _, _)| index)
    }
}
