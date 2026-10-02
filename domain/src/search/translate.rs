//! From a typed query to one PCRE2 pattern that GtkSourceView's `SearchContext` and our own
//! matcher both compile (ADR-005 and its amendment).
//!
//! Both engines get the same pattern text and compile it with the same options: UTF, UCP,
//! multi-line, newline convention ANY (GtkSourceView's default), and caseless when
//! [`Translated::case_sensitive`] is false. Everything else is spelled inside the pattern:
//! dot-matches-newline as a leading `(?s)`, and whole word as lookarounds. So what the widget
//! highlights is exactly what our engine replaces.
//!
//! Some patterns cannot be left to the widget ([`Translated::needs_own_matcher`]):
//! - **empty matches**: `SearchContext` matches with `PCRE2_NOTEMPTY`, so `^`, `$`, `a*` or
//!   `^(.*)$` count nothing or skip empty lines (spike S2);
//! - **`\K`**: counted and highlighted, but `forward()` finds nothing and replace does nothing;
//! - **`\G`**: the widget scans in 100-line segments and `\G` would match at each one.
//!
//! The engine that owns such a pattern counts, highlights and navigates it too.

use std::fmt;

use super::extended::{EscapeError, unescape};
use super::pattern::{self, SourceMap};
use super::{NUL_PLACEHOLDER, Query, SearchMode};

/// Where the pattern runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Target {
    /// A document buffer: LF line breaks only, NUL stored as U+2400.
    #[default]
    Buffer,
    /// Files on disk, with any line ending: a line break in the query matches CRLF, LF or CR.
    Files,
}

/// What the translation changed or found, for the UI's hints and for routing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Note {
    /// Boost's `\<` became `\b(?=\w)`.
    BoostWordStart,
    /// Boost's `\>` became `\b(?<=\w)`.
    BoostWordEnd,
    /// Boost's `` \` `` and `\'` became `\A` and `\z`.
    BoostTextAnchor,
    /// Boost's `\Z` (end of text after optional line breaks) became `(?=\v*\z)`.
    BoostEndOfText,
    /// Boost's `\l`, `\u`, `\L`, `\U` classes became `[[:lower:]]`, `[[:upper:]]` and their
    /// negations.
    BoostCaseClass,
    /// `\C` means any character in Boost; it became `.`.
    AnyCharacter,
    /// `\10` is back-reference 1 followed by `0` in Boost; it became `\g{1}0`.
    SingleDigitBackreference,
    /// `\0ooo` takes three octal digits in Boost; it became `\x{...}`.
    OctalEscape,
    /// A UTF-16 surrogate pair such as `\x{D83D}\x{DE82}` became one character.
    SurrogatePair,
    /// `\r\n` or `\r` became a line break of the target.
    LineBreak,
    /// NUL became U+2400, which the buffer holds instead (ADR-007).
    NulPlaceholder,
    /// Whole word is spelled as lookarounds in the pattern.
    WholeWord,
    /// Whole word does not apply in Regex mode; the find bar disables it there.
    WholeWordIgnored,
    /// The pattern can match empty text. Routes it to our own matcher.
    MatchesEmpty,
    /// The pattern uses `\K`. Routes it to our own matcher.
    KeepOut,
    /// The pattern uses `\G`. Routes it to our own matcher.
    Continuation,
}

impl Note {
    pub fn routes_to_own_matcher(self) -> bool {
        matches!(
            self,
            Self::MatchesEmpty | Self::KeepOut | Self::Continuation
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslateError {
    /// Nothing to search for.
    EmptyPattern,
    /// An Extended-mode escape that no text can hold.
    Escape(EscapeError),
}

impl TranslateError {
    /// The character offset in the typed pattern that the error is about.
    pub fn offset(&self) -> usize {
        match self {
            Self::EmptyPattern => 0,
            Self::Escape(error) => error.offset(),
        }
    }
}

impl fmt::Display for TranslateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyPattern => f.write_str("nothing to search for"),
            Self::Escape(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for TranslateError {}

impl From<EscapeError> for TranslateError {
    fn from(error: EscapeError) -> Self {
        Self::Escape(error)
    }
}

/// A query translated for PCRE2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated {
    /// The pattern for both engines. GtkSourceView gets it with regex search enabled, even in
    /// Normal and Extended modes.
    pub pattern: String,
    /// GtkSourceView's `case-sensitive`; our matcher compiles caseless when it is false.
    pub case_sensitive: bool,
    /// GtkSourceView's `at-word-boundaries`: always false. Whole word is in the pattern,
    /// because GtkSourceView 5.20 implements the setting in regex mode as `\b` + pattern +
    /// `\b` without a group, which breaks alternations and differs from Scintilla's rule.
    pub at_word_boundaries: bool,
    /// GtkSourceView cannot count, highlight or navigate this pattern correctly; our matcher
    /// does all of it (see the module docs).
    pub needs_own_matcher: bool,
    /// A match can span lines, or the pattern uses whole-text anchors: Find in Files must
    /// search whole files instead of line by line.
    pub needs_whole_text: bool,
    pub notes: Vec<Note>,
    map: SourceMap,
}

impl Translated {
    /// The character offset in the typed pattern for byte `offset` of [`Translated::pattern`],
    /// for placing PCRE2's error offsets.
    pub fn source_offset(&self, offset: usize) -> usize {
        self.map.source_offset(&self.pattern, offset)
    }

    pub fn has_note(&self, note: Note) -> bool {
        self.notes.contains(&note)
    }
}

/// Translates `query` for a document buffer.
pub fn translate(query: &Query) -> Result<Translated, TranslateError> {
    translate_for(query, Target::Buffer)
}

pub fn translate_for(query: &Query, target: Target) -> Result<Translated, TranslateError> {
    if query.pattern.is_empty() {
        return Err(TranslateError::EmptyPattern);
    }
    let options = query.options;
    match options.mode {
        SearchMode::Normal => Ok(literal(
            &query.pattern,
            options.match_case,
            options.whole_word,
            target,
        )),
        SearchMode::Extended => {
            let text = unescape(&query.pattern)?;
            Ok(literal(
                &text,
                options.match_case,
                options.whole_word,
                target,
            ))
        }
        SearchMode::Regex => {
            let rewrite = pattern::rewrite(&query.pattern, target, options.dot_matches_newline);
            let mut notes = rewrite.notes;
            if options.whole_word {
                notes.push(Note::WholeWordIgnored);
            }
            let routes = [
                (rewrite.nullable, Note::MatchesEmpty),
                (rewrite.keep_out, Note::KeepOut),
                (rewrite.continuation, Note::Continuation),
            ];
            for (applies, note) in routes {
                if applies {
                    notes.push(note);
                }
            }
            Ok(Translated {
                pattern: rewrite.pattern,
                case_sensitive: options.match_case,
                at_word_boundaries: false,
                needs_own_matcher: notes.iter().any(|note| note.routes_to_own_matcher()),
                needs_whole_text: rewrite.whole_text,
                notes,
                map: rewrite.map,
            })
        }
    }
}

/// Normal and Extended modes: an escaped literal, with line breaks and NUL as in the target.
/// Whole word follows Scintilla: a match starting with a word character must not follow one, a
/// match starting with punctuation must not follow punctuation, and the same at the end; a
/// match that starts or ends with white space is not restricted on that side.
fn literal(text: &str, match_case: bool, whole_word: bool, target: Target) -> Translated {
    let mut notes = Vec::new();
    let mut body = String::with_capacity(text.len() + 8);
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' | '\n' => {
                if c == '\r' {
                    chars.next_if_eq(&'\n');
                    notes_push(&mut notes, Note::LineBreak);
                }
                body.push_str(match target {
                    Target::Buffer => r"\n",
                    Target::Files => r"(?:\r\n|\n|\r)",
                });
            }
            '\0' => match target {
                Target::Buffer => {
                    notes_push(&mut notes, Note::NulPlaceholder);
                    body.push(NUL_PLACEHOLDER);
                }
                Target::Files => body.push_str(r"\x{0}"),
            },
            c => push_literal(&mut body, c),
        }
    }
    let needs_whole_text = text.contains(['\n', '\r']);
    let map = if !whole_word && body == text {
        SourceMap::identity()
    } else {
        SourceMap::start()
    };
    let mut pattern = body;
    if whole_word {
        notes.push(Note::WholeWord);
        let first = text.chars().next().map_or(CharClass::Space, CharClass::of);
        let last = text
            .chars()
            .next_back()
            .map_or(CharClass::Space, CharClass::of);
        let before = match first {
            CharClass::Word => r"(?<!\w)",
            CharClass::Punctuation => r"(?<![^\s\w])",
            CharClass::Space => "",
        };
        let after = match last {
            CharClass::Word => r"(?!\w)",
            CharClass::Punctuation => r"(?![^\s\w])",
            CharClass::Space => "",
        };
        pattern = format!("{before}{pattern}{after}");
    }
    Translated {
        pattern,
        case_sensitive: match_case,
        at_word_boundaries: false,
        needs_own_matcher: false,
        needs_whole_text,
        notes,
        map,
    }
}

fn notes_push(notes: &mut Vec<Note>, note: Note) {
    if !notes.contains(&note) {
        notes.push(note);
    }
}

/// Escapes PCRE2's metacharacters.
fn push_literal(out: &mut String, c: char) {
    if matches!(
        c,
        '\\' | '^' | '$' | '.' | '|' | '?' | '*' | '+' | '(' | ')' | '[' | ']' | '{' | '}'
    ) {
        out.push('\\');
    }
    out.push(c);
}

/// Scintilla's character classes for whole-word matching, approximated with PCRE2's `\w`
/// (letters, digits, marks, connector punctuation) and `\s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CharClass {
    Word,
    Punctuation,
    Space,
}

impl CharClass {
    fn of(c: char) -> Self {
        if c.is_alphanumeric() || c == '_' {
            Self::Word
        } else if c.is_whitespace() {
            Self::Space
        } else {
            Self::Punctuation
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::SearchOptions;
    use super::*;

    fn query(mode: SearchMode, pattern: &str) -> Query {
        Query::new(pattern, SearchOptions::new(mode))
    }

    fn pattern_of(query: &Query) -> String {
        translate(query).unwrap().pattern
    }

    #[test]
    fn normal_mode_escapes_metacharacters() {
        let translated = translate(&query(SearchMode::Normal, r"a.b*(c)\d$")).unwrap();
        assert_eq!(translated.pattern, r"a\.b\*\(c\)\\d\$");
        assert!(!translated.needs_own_matcher);
        assert_eq!(translated.source_offset(0), 0);
    }

    #[test]
    fn extended_mode_unescapes_then_escapes() {
        assert_eq!(
            pattern_of(&query(SearchMode::Extended, r"a\tb\x2A")),
            "a\tb\\*"
        );
        assert_eq!(
            pattern_of(&query(SearchMode::Extended, r"a\r\nb\rc\nd")),
            r"a\nb\nc\nd"
        );
        assert_eq!(
            pattern_of(&query(SearchMode::Extended, r"x\0")),
            "x\u{2400}"
        );
        let files = translate_for(&query(SearchMode::Extended, r"a\r\nb"), Target::Files).unwrap();
        assert_eq!(files.pattern, r"a(?:\r\n|\n|\r)b");
        assert!(files.needs_whole_text);
        assert_eq!(
            translate(&query(SearchMode::Extended, r"\uD800")),
            Err(TranslateError::Escape(EscapeError::LoneSurrogate {
                offset: 0,
                value: 0xD800
            }))
        );
    }

    #[test]
    fn whole_word_boundaries() {
        let whole = |pattern: &str| {
            let options = SearchOptions::new(SearchMode::Normal).with_whole_word(true);
            translate(&Query::new(pattern, options)).unwrap().pattern
        };
        assert_eq!(whole("user"), r"(?<!\w)user(?!\w)");
        assert_eq!(whole("->"), r"(?<![^\s\w])->(?![^\s\w])");
        assert_eq!(whole("f("), r"(?<!\w)f\((?![^\s\w])");
        assert_eq!(whole(" x"), r" x(?!\w)");
        let regex = SearchOptions::new(SearchMode::Regex).with_whole_word(true);
        let translated = translate(&Query::new("a|b", regex)).unwrap();
        assert_eq!(translated.pattern, "a|b");
        assert!(translated.has_note(Note::WholeWordIgnored));
    }

    #[test]
    fn routes_what_the_widget_cannot_do() {
        let routed = |pattern: &str| translate(&query(SearchMode::Regex, pattern)).unwrap();
        assert!(routed("^").needs_own_matcher);
        assert!(routed(r"user=\K\w+").has_note(Note::KeepOut));
        assert!(routed(r"\Ga").has_note(Note::Continuation));
        assert!(!routed(r"(?<=user=)\w+").needs_own_matcher);
        assert!(!routed(r"\<user\>").needs_own_matcher);
        assert_eq!(routed(r"\<user\>").pattern, r"\b(?=\w)user\b(?<=\w)");
    }

    #[test]
    fn dot_matches_newline_and_case_travel_with_the_pattern() {
        let options = SearchOptions::new(SearchMode::Regex)
            .with_dot_matches_newline(true)
            .with_match_case(true);
        let translated = translate(&Query::new("a.b", options)).unwrap();
        assert_eq!(translated.pattern, "(?s)a.b");
        assert!(translated.case_sensitive);
        assert!(!translated.at_word_boundaries);
    }

    #[test]
    fn empty_patterns_are_rejected() {
        assert_eq!(
            translate(&query(SearchMode::Regex, "")),
            Err(TranslateError::EmptyPattern)
        );
    }
}
