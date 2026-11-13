//! Search without a toolkit: the query model, Extended-mode escapes, the translation of
//! Boost-style regex syntax into the PCRE2 dialect that GtkSourceView and our own matcher
//! share, and replacement templates (ADR-005). [`replace_files`] writes replacements back into
//! files, and [`report`] words the counts and the results panel.
//!
//! The buffer is LF-normalized (ADR-008) and holds U+2400 in place of NUL (ADR-007). Whatever
//! is searched for or inserted is normalized the same way, so `\r\n` in a query finds a line
//! break and `\r\n` in a replacement inserts one.

mod extended;
mod history;
mod pattern;
pub mod replace_files;
pub mod report;
mod smart;
mod template;
mod translate;

use std::borrow::Cow;
use std::ops::Range;

use crate::text::CharOffset;

pub use extended::unescape;
pub use history::{HISTORY_LIMIT, History, Recall, SearchHistory};
pub use smart::SmartHighlight;
pub use template::{Captures, Template, TemplateWarning};
pub use translate::{Target, TranslateError, Translated, translate, translate_for};

/// How the text in the find and replace fields is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SearchMode {
    /// Literal text.
    #[default]
    Normal,
    /// Literal text with backslash escapes ([`unescape`]).
    Extended,
    /// A regular expression in Boost-style syntax, run by PCRE2 after translation.
    Regex,
}

/// Which way a search goes from the caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// What a search does when it reaches the end of the text (the start, going backward).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Wrap {
    /// Goes on from the other end.
    Around,
    /// Stops there.
    Stop,
}

/// The find bar's options, named after its toggles and check boxes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SearchOptions {
    pub mode: SearchMode,
    pub match_case: bool,
    /// Normal and Extended modes only; the find bar disables it in Regex mode.
    pub whole_word: bool,
    pub wrap: bool,
    pub backward: bool,
    /// Count and Replace All work inside the selection ([`SearchOptions::scope`]).
    pub in_selection: bool,
    /// Regex mode only: `.` also matches a line break.
    pub dot_matches_newline: bool,
}

impl Default for SearchOptions {
    /// Normal mode with Wrap around on.
    fn default() -> Self {
        Self {
            mode: SearchMode::Normal,
            match_case: false,
            whole_word: false,
            wrap: true,
            backward: false,
            in_selection: false,
            dot_matches_newline: false,
        }
    }
}

impl SearchOptions {
    pub fn new(mode: SearchMode) -> Self {
        Self {
            mode,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_match_case(self, match_case: bool) -> Self {
        Self { match_case, ..self }
    }

    #[must_use]
    pub fn with_whole_word(self, whole_word: bool) -> Self {
        Self { whole_word, ..self }
    }

    #[must_use]
    pub fn with_dot_matches_newline(self, dot_matches_newline: bool) -> Self {
        Self {
            dot_matches_newline,
            ..self
        }
    }

    /// The character range that Count and Replace All work on: the selection with In selection;
    /// the whole text with Wrap around; otherwise from the caret (or the selection's start) to
    /// the end, or backward from the start to the caret (or the selection's end). An empty
    /// selection counts as no selection. `end` is the end of the text.
    pub fn scope(
        &self,
        caret: CharOffset,
        selection: Option<Range<CharOffset>>,
        end: CharOffset,
    ) -> Range<CharOffset> {
        let start = CharOffset::ZERO;
        let selection = selection.filter(|range| range.start < range.end);
        match selection {
            Some(range) if self.in_selection => range.start.min(end)..range.end.min(end),
            _ if self.wrap => start..end,
            Some(range) if self.backward => start..range.end.min(end),
            Some(range) => range.start.min(end)..end,
            None if self.backward => start..caret.min(end),
            None => caret.min(end)..end,
        }
    }
}

/// A search as typed: the find field and the options.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Query {
    pub pattern: String,
    pub options: SearchOptions,
}

impl Query {
    pub fn new(pattern: impl Into<String>, options: SearchOptions) -> Self {
        Self {
            pattern: pattern.into(),
            options,
        }
    }

    pub fn normal(pattern: impl Into<String>) -> Self {
        Self::new(pattern, SearchOptions::new(SearchMode::Normal))
    }

    pub fn extended(pattern: impl Into<String>) -> Self {
        Self::new(pattern, SearchOptions::new(SearchMode::Extended))
    }

    pub fn regex(pattern: impl Into<String>) -> Self {
        Self::new(pattern, SearchOptions::new(SearchMode::Regex))
    }

    #[must_use]
    pub fn with_options(self, options: SearchOptions) -> Self {
        Self { options, ..self }
    }
}

/// Normalizes text the way the loader normalizes a document: CRLF and CR become LF
/// (ADR-008) and NUL becomes [`NUL_PLACEHOLDER`](crate::text::NUL_PLACEHOLDER) (ADR-007).
pub fn to_buffer_text(text: &str) -> Cow<'_, str> {
    if memchr::memchr2(b'\r', 0, text.as_bytes()).is_none() {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push('\n');
            }
            '\0' => out.push(crate::text::NUL_PLACEHOLDER),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing::{at, chars};

    #[test]
    fn buffer_text_normalizes_line_breaks_and_nul() {
        assert_eq!(to_buffer_text("a\r\nb\rc\nd\0"), "a\nb\nc\nd\u{2400}");
        assert!(matches!(to_buffer_text("plain\n"), Cow::Borrowed(_)));
    }

    #[test]
    fn scope_follows_the_replace_all_table() {
        let options = SearchOptions::default();
        let plain = SearchOptions {
            wrap: false,
            ..options
        };
        let backward = SearchOptions {
            backward: true,
            ..plain
        };
        let in_selection = SearchOptions {
            in_selection: true,
            ..options
        };
        assert_eq!(plain.scope(at(4), None, at(10)), chars(4..10));
        assert_eq!(plain.scope(at(4), Some(chars(2..6)), at(10)), chars(2..10));
        assert_eq!(backward.scope(at(4), None, at(10)), chars(0..4));
        assert_eq!(
            backward.scope(at(4), Some(chars(2..6)), at(10)),
            chars(0..6)
        );
        assert_eq!(
            in_selection.scope(at(4), Some(chars(2..6)), at(10)),
            chars(2..6)
        );
        assert_eq!(
            options.scope(at(4), Some(chars(2..6)), at(10)),
            chars(0..10)
        );
        assert_eq!(
            in_selection.scope(at(4), Some(chars(3..3)), at(10)),
            chars(0..10)
        );
        assert_eq!(plain.scope(at(40), None, at(10)), chars(10..10));
    }
}
