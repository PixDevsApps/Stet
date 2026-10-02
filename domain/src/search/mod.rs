//! Search without a toolkit: the query model, Extended-mode escapes, the translation of
//! Boost-style regex syntax into the PCRE2 dialect that GtkSourceView and our own matcher
//! share, and replacement templates (ADR-005).
//!
//! The buffer is LF-normalized (ADR-008) and holds U+2400 in place of NUL (ADR-007). Whatever
//! is searched for or inserted is normalized the same way, so `\r\n` in a query finds a line
//! break and `\r\n` in a replacement inserts one.

pub mod extended;
pub mod history;
mod pattern;
pub mod report;
pub mod smart;
pub mod template;
mod translate;

use std::borrow::Cow;
use std::ops::Range;

pub use extended::{EscapeError, unescape};
pub use history::{History, Recall, SearchHistory};
pub use report::ScopeKind;
pub use smart::SmartHighlight;
pub use template::{Captures, Template, TemplateWarning, expand};
pub use translate::{Note, Target, TranslateError, Translated, translate, translate_for};

/// What the buffer holds instead of U+0000 (ADR-007).
pub const NUL_PLACEHOLDER: char = '\u{2400}';

/// How the text in the find and replace fields is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum SearchMode {
    /// Literal text.
    #[default]
    Normal,
    /// Literal text with backslash escapes ([`extended::unescape`]).
    Extended,
    /// A regular expression in Boost-style syntax, run by PCRE2 after translation.
    Regex,
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

    pub fn with_match_case(self, match_case: bool) -> Self {
        Self { match_case, ..self }
    }

    pub fn with_whole_word(self, whole_word: bool) -> Self {
        Self { whole_word, ..self }
    }

    pub fn with_dot_matches_newline(self, dot_matches_newline: bool) -> Self {
        Self {
            dot_matches_newline,
            ..self
        }
    }

    /// The character range that Count and Replace All work on: the selection with In selection;
    /// the whole text with Wrap around; otherwise from the caret (or the selection's start) to
    /// the end, or backward from the start to the caret (or the selection's end). An empty
    /// selection counts as no selection.
    pub fn scope(&self, caret: usize, selection: Option<Range<usize>>, len: usize) -> Range<usize> {
        let selection = selection.filter(|range| range.start < range.end);
        match selection {
            Some(range) if self.in_selection => range.start.min(len)..range.end.min(len),
            _ if self.wrap => 0..len,
            Some(range) if self.backward => 0..range.end.min(len),
            Some(range) => range.start.min(len)..len,
            None if self.backward => 0..caret.min(len),
            None => caret.min(len)..len,
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

    pub fn with_options(self, options: SearchOptions) -> Self {
        Self { options, ..self }
    }
}

/// Normalizes text the way the loader normalizes a document: CRLF and CR become LF
/// (ADR-008) and NUL becomes [`NUL_PLACEHOLDER`] (ADR-007).
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
            '\0' => out.push(NUL_PLACEHOLDER),
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(plain.scope(4, None, 10), 4..10);
        assert_eq!(plain.scope(4, Some(2..6), 10), 2..10);
        assert_eq!(backward.scope(4, None, 10), 0..4);
        assert_eq!(backward.scope(4, Some(2..6), 10), 0..6);
        assert_eq!(in_selection.scope(4, Some(2..6), 10), 2..6);
        assert_eq!(options.scope(4, Some(2..6), 10), 0..10);
        assert_eq!(in_selection.scope(4, Some(3..3), 10), 0..10);
        assert_eq!(plain.scope(40, None, 10), 10..10);
    }
}
