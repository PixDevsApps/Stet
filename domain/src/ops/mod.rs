//! The text toolbox (M5): pure operations over an LF-normalized `&str` snapshot that return
//! [`TextEdit`]s in character offsets (ADR-003). The UI applies a [`Change`] as one user action,
//! and replaces the affected range once when it has more than [`BULK_EDIT_THRESHOLD`] edits.

pub mod case;
pub mod comment;
pub mod json;
pub mod lines;
pub mod prefix;
pub mod xml;

use std::fmt;
use std::ops::Range;

use crate::text::edit::ByteCursor;
use crate::text::{Bias, LineIndex, Position, TextEdit, map_offset};

/// Above this many edits a change is applied as one replacement of the range it spans
/// (ADR-003 amendment).
pub const BULK_EDIT_THRESHOLD: usize = 2_000;

/// The part of the text an operation works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// Whole lines, zero-based and end-exclusive.
    Lines(Range<usize>),
    /// A selection in character offsets; an empty range is the caret. Line operations cover
    /// every line the selection touches, except a last line that it reaches only at column 0.
    Selection(Range<usize>),
    /// The whole text. Line operations leave out the empty line after a final newline, so the
    /// file keeps its final newline.
    Document,
}

impl Target {
    pub const fn caret(offset: usize) -> Self {
        Self::Selection(offset..offset)
    }
}

/// The result of an operation. An empty change means there is nothing to do: no undo step,
/// and the selection stays.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Change {
    /// Sorted, non-overlapping edits in character offsets of the original text.
    pub edits: Vec<TextEdit>,
    /// The selection to set afterwards, in character offsets of the edited text. `None` keeps
    /// the current selection, mapped through the edits by [`Change::selection_after`].
    pub selection: Option<Range<usize>>,
}

impl Change {
    fn new(edits: Vec<TextEdit>, selection: Option<Range<usize>>) -> Self {
        let edits: Vec<TextEdit> = edits.into_iter().filter(|edit| !edit.is_noop()).collect();
        debug_assert!(
            edits.windows(2).all(|pair| pair[0].end <= pair[1].start),
            "edits must be sorted and must not overlap"
        );
        if edits.is_empty() {
            return Self::default();
        }
        Self { edits, selection }
    }

    pub fn is_empty(&self) -> bool {
        self.edits.is_empty()
    }

    /// Whether the UI should apply this change as one bulk replacement.
    pub fn is_bulk(&self) -> bool {
        self.edits.len() > BULK_EDIT_THRESHOLD
    }

    /// The selection after the change: the explicit one if the operation set it, otherwise
    /// `current` mapped through the edits. A caret moves past text inserted at it; a range
    /// grows over text inserted at its ends.
    pub fn selection_after(&self, current: Range<usize>) -> Range<usize> {
        if let Some(selection) = &self.selection {
            return selection.clone();
        }
        let (start, end) = (
            current.start.min(current.end),
            current.start.max(current.end),
        );
        if start == end {
            let caret = map_offset(start, &self.edits, Bias::After);
            return caret..caret;
        }
        map_offset(start, &self.edits, Bias::Before)..map_offset(end, &self.edits, Bias::After)
    }

    /// The edits as a single replacement of the range they span, for the bulk path. `text` is
    /// the snapshot the change was computed from.
    pub fn coalesce(&self, text: &str) -> Option<TextEdit> {
        let (first, last) = (self.edits.first()?, self.edits.last()?);
        let mut cursor = ByteCursor::new(text);
        let mut copied = cursor.byte_at(first.start);
        let mut insert = String::new();
        for edit in &self.edits {
            let start = cursor.byte_at(edit.start);
            insert.push_str(&text[copied..start]);
            insert.push_str(&edit.insert);
            copied = cursor.byte_at(edit.end);
        }
        Some(TextEdit::new(first.start..last.end, insert))
    }
}

/// Indentation for the JSON and XML formatters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    Spaces(usize),
    Tab,
}

impl Indent {
    fn unit(self) -> String {
        match self {
            Self::Spaces(count) => " ".repeat(count),
            Self::Tab => "\t".to_owned(),
        }
    }
}

impl Default for Indent {
    fn default() -> Self {
        Self::Spaces(2)
    }
}

/// Line breaks followed by indentation, cut from one growing string.
struct Breaks {
    unit: String,
    cache: String,
}

impl Breaks {
    fn new(indent: Indent) -> Self {
        Self {
            unit: indent.unit(),
            cache: "\n".to_owned(),
        }
    }

    fn push(&mut self, out: &mut String, depth: usize) {
        let length = 1 + self.unit.len() * depth;
        while self.cache.len() < length {
            self.cache.push_str(&self.unit);
        }
        out.push_str(&self.cache[..length]);
    }
}

/// A JSON or XML syntax error. `line` and `column` are 1-based for display; `column` and
/// `offset` count characters, and `offset` is where the caret should go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyntaxError {
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub offset: usize,
}

impl SyntaxError {
    /// An error at byte `byte` of `text`, moved back to a character boundary if needed.
    fn at_byte(text: &str, byte: usize, message: impl Into<String>) -> Self {
        let mut byte = byte.min(text.len());
        while !text.is_char_boundary(byte) {
            byte -= 1;
        }
        let line_start = memchr::memrchr(b'\n', &text.as_bytes()[..byte]).map_or(0, |at| at + 1);
        Self {
            message: message.into(),
            line: memchr::memchr_iter(b'\n', &text.as_bytes()[..line_start]).count() + 1,
            column: text[line_start..byte].chars().count() + 1,
            offset: text[..byte].chars().count(),
        }
    }

    /// The same error in a text of which the parsed input starts at character `base`.
    fn shifted(self, index: &LineIndex, base: usize) -> Self {
        let offset = base + self.offset;
        let Position { line, column } = index.position(offset);
        Self {
            offset,
            line: line + 1,
            column: column + 1,
            ..self
        }
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} at line {}, column {}",
            self.message, self.line, self.column
        )
    }
}

impl std::error::Error for SyntaxError {}

/// A snapshot with its line index, and the target resolution every operation shares.
struct Doc<'a> {
    text: &'a str,
    index: LineIndex,
}

impl<'a> Doc<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            text,
            index: LineIndex::new(text),
        }
    }

    fn len(&self) -> usize {
        self.index.len_chars()
    }

    fn line_count(&self) -> usize {
        self.index.line_count()
    }

    /// The text of `line`, without its newline.
    fn line(&self, line: usize) -> &'a str {
        &self.text[self.index.line_bytes(line)]
    }

    fn start(&self, line: usize) -> usize {
        self.index.line_chars(line).start
    }

    fn end(&self, line: usize) -> usize {
        self.index.line_chars(line).end
    }

    /// The character offset of byte `byte` of `line`.
    fn offset_in(&self, line: usize, byte: usize) -> usize {
        self.start(line) + self.line(line)[..byte].chars().count()
    }

    /// `lines` with the newlines between them, without the last one's newline.
    fn block(&self, lines: Range<usize>) -> &'a str {
        let start = self.index.line_bytes(lines.start).start;
        let end = self.index.line_bytes(lines.end - 1).end;
        &self.text[start..end]
    }

    fn slice(&self, chars: Range<usize>) -> &'a str {
        let start = self.index.char_to_byte(self.text, chars.start);
        let end = self.index.char_to_byte(self.text, chars.end);
        &self.text[start..end]
    }

    /// `range` ordered and clamped to the text.
    fn clamp(&self, range: &Range<usize>) -> Range<usize> {
        let len = self.len();
        let (a, b) = (range.start.min(len), range.end.min(len));
        a.min(b)..a.max(b)
    }

    fn lines(&self, target: &Target) -> Range<usize> {
        let count = self.line_count();
        match target {
            Target::Lines(range) => {
                let end = range.end.min(count);
                range.start.min(end)..end
            }
            Target::Selection(range) => {
                let range = self.clamp(range);
                let first = self.index.line_of_char(range.start);
                let mut last = self.index.line_of_char(range.end);
                if last > first && self.start(last) == range.end {
                    last -= 1;
                }
                first..last + 1
            }
            Target::Document => {
                let trailing = count > 1 && self.text.ends_with('\n');
                0..count - usize::from(trailing)
            }
        }
    }
}

fn is_blank_char(c: char) -> bool {
    c == ' ' || c == '\t'
}

/// Spaces and tabs only; other whitespace counts as content.
fn is_blank(line: &str) -> bool {
    line.bytes().all(|byte| byte == b' ' || byte == b'\t')
}

/// Bytes (and characters) of leading spaces and tabs.
fn leading_blanks(line: &str) -> usize {
    line.len() - line.trim_start_matches(is_blank_char).len()
}

/// Bytes (and characters) of trailing spaces and tabs.
fn trailing_blanks(line: &str) -> usize {
    line.len() - line.trim_end_matches(is_blank_char).len()
}

fn char_len(text: &str) -> usize {
    text.chars().count()
}

/// Deletes `lines` (ascending) with the newlines that separate them from the rest of the text.
fn delete_lines(doc: &Doc<'_>, lines: impl IntoIterator<Item = usize>) -> Vec<TextEdit> {
    let mut edits = Vec::new();
    let mut run: Option<Range<usize>> = None;
    let flush = |run: Range<usize>, edits: &mut Vec<TextEdit>| {
        let range = if run.end < doc.line_count() {
            doc.start(run.start)..doc.start(run.end)
        } else if run.start > 0 {
            doc.end(run.start - 1)..doc.len()
        } else {
            0..doc.len()
        };
        edits.push(TextEdit::delete(range));
    };
    for line in lines {
        run = match run {
            Some(current) if current.end == line => Some(current.start..line + 1),
            Some(current) => {
                flush(current, &mut edits);
                Some(line..line + 1)
            }
            None => Some(line..line + 1),
        };
    }
    if let Some(current) = run {
        flush(current, &mut edits);
    }
    edits
}

/// Replaces the lines of `range` with `new`, touching only the lines that differ.
fn replace_lines(doc: &Doc<'_>, range: Range<usize>, old: &[&str], new: &[&str]) -> Vec<TextEdit> {
    debug_assert_eq!(old.len(), new.len());
    let Some(first) = old.iter().zip(new).position(|(a, b)| a != b) else {
        return Vec::new();
    };
    let last = old
        .iter()
        .zip(new)
        .rposition(|(a, b)| a != b)
        .unwrap_or(first);
    let replaced = doc.start(range.start + first)..doc.end(range.start + last);
    vec![TextEdit::new(replaced, new[first..=last].join("\n"))]
}

/// Runs `transform` on the target's text without its surrounding whitespace (JSON and XML
/// whitespace alike) and replaces that part. An empty selection means the whole document.
/// Errors are reported in positions of `text`.
fn reformat_target(
    text: &str,
    target: Target,
    transform: impl FnOnce(&str) -> Result<String, SyntaxError>,
) -> Result<Change, SyntaxError> {
    let doc = Doc::new(text);
    let selected = matches!(&target, Target::Selection(range) if range.start != range.end);
    let range = match &target {
        Target::Selection(range) if selected => doc.clamp(range),
        Target::Lines(_) => {
            let lines = doc.lines(&target);
            if lines.is_empty() {
                return Ok(Change::default());
            }
            doc.start(lines.start)..doc.end(lines.end - 1)
        }
        _ => 0..doc.len(),
    };
    let slice = doc.slice(range.clone());
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\r');
    let inner = slice.trim_matches(is_space);
    let lead = slice.len() - slice.trim_start_matches(is_space).len();
    let start = range.start + char_len(&slice[..lead]);
    let result = transform(inner).map_err(|error| error.shifted(&doc.index, start))?;
    if result == inner {
        return Ok(Change::default());
    }
    let end = start + char_len(inner);
    let selection = if selected {
        start..start + char_len(&result)
    } else {
        start..start
    };
    Ok(Change::new(
        vec![TextEdit::new(start..end, result)],
        Some(selection),
    ))
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use crate::text::{apply, normalize};
    use proptest::prelude::*;

    /// Lines from a small alphabet that exercises blanks, case, digits, signs and non-ASCII.
    pub fn text() -> impl Strategy<Value = String> {
        let line = prop::collection::vec(
            prop_oneof![
                4 => "[a-cA-C]",
                2 => Just(" ".to_owned()),
                1 => Just("\t".to_owned()),
                2 => "[0-9]",
                1 => "[-.,]",
                1 => "[éßΣ日]",
            ],
            0..8,
        )
        .prop_map(|parts| parts.concat());
        (prop::collection::vec(line, 1..8), any::<bool>()).prop_map(|(lines, newline)| {
            let mut text = lines.join("\n");
            if newline {
                text.push('\n');
            }
            text
        })
    }

    /// A text with a target: lines, a selection (often from and to line starts) or the document.
    pub fn text_and_target() -> impl Strategy<Value = (String, Target)> {
        text().prop_flat_map(|text| {
            let len = char_len(&text);
            let lines = text.split('\n').count();
            let starts: Vec<usize> = std::iter::once(0)
                .chain(
                    text.chars()
                        .enumerate()
                        .filter(|&(_, c)| c == '\n')
                        .map(|(at, _)| at + 1),
                )
                .collect();
            let offset = prop_oneof![0..=len, prop::sample::select(starts)];
            let target = prop_oneof![
                (0..=lines, 0..=lines).prop_map(|(a, b)| Target::Lines(a.min(b)..a.max(b))),
                (offset.clone(), offset).prop_map(|(a, b)| Target::Selection(a..b)),
                Just(Target::Document),
            ];
            (Just(text), target)
        })
    }

    /// The lines a target covers, computed on a plain vector of lines.
    pub fn naive_lines(text: &str, target: &Target) -> Range<usize> {
        let lines: Vec<&str> = text.split('\n').collect();
        let line_of = |offset: usize| text.chars().take(offset).filter(|&c| c == '\n').count();
        match target {
            Target::Lines(range) => {
                range.start.min(range.end).min(lines.len())..range.end.min(lines.len())
            }
            Target::Selection(range) => {
                let len = char_len(text);
                let (a, b) = (range.start.min(len), range.end.min(len));
                let (start, end) = (a.min(b), a.max(b));
                let first = line_of(start);
                let mut last = line_of(end);
                let at_line_start = end == 0 || text.chars().nth(end - 1) == Some('\n');
                if last > first && at_line_start {
                    last -= 1;
                }
                first..last + 1
            }
            Target::Document => {
                let trailing = lines.len() > 1 && text.ends_with('\n');
                0..lines.len() - usize::from(trailing)
            }
        }
    }

    /// Applies `change` to `text`, checking that its edits are already normalized.
    pub fn applied(text: &str, change: &Change) -> String {
        let len = char_len(text);
        assert_eq!(
            normalize(change.edits.clone(), len).as_ref(),
            Ok(&change.edits),
            "edits are not normalized"
        );
        let result = apply(text, change.edits.clone()).unwrap();
        if let Some(selection) = &change.selection {
            let new_len = char_len(&result);
            assert!(
                selection.start <= selection.end && selection.end <= new_len,
                "selection {selection:?} outside the result of {new_len} characters"
            );
        }
        if let Some(bulk) = change.coalesce(text) {
            assert_eq!(apply(text, vec![bulk]).unwrap(), result);
        }
        result
    }

    /// Replaces the lines `range` of `text` with `new`.
    pub fn splice_lines(text: &str, range: Range<usize>, new: Vec<String>) -> String {
        let mut lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        lines.splice(range, new);
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn a_selection_ending_at_column_zero_leaves_out_that_line() {
        let doc = Doc::new("one\ntwo\nthree\n");
        assert_eq!(doc.lines(&Target::Selection(0..4)), 0..1);
        assert_eq!(doc.lines(&Target::Selection(0..5)), 0..2);
        assert_eq!(doc.lines(&Target::Selection(2..8)), 0..2);
        assert_eq!(doc.lines(&Target::caret(4)), 1..2);
        let reversed = Range { start: 8, end: 4 };
        assert_eq!(doc.lines(&Target::Selection(reversed)), 1..2);
    }

    #[test]
    fn the_document_leaves_out_the_line_after_a_final_newline() {
        assert_eq!(Doc::new("a\nb\n").lines(&Target::Document), 0..2);
        assert_eq!(Doc::new("a\nb").lines(&Target::Document), 0..2);
        assert_eq!(Doc::new("").lines(&Target::Document), 0..1);
        assert_eq!(Doc::new("\n").lines(&Target::Document), 0..1);
        assert_eq!(Doc::new("a\nb\n").lines(&Target::Selection(0..4)), 0..2);
        assert_eq!(Doc::new("a\nb\n").lines(&Target::caret(4)), 2..3);
    }

    #[test]
    fn line_targets_are_clamped() {
        let doc = Doc::new("a\nb");
        assert_eq!(doc.lines(&Target::Lines(1..9)), 1..2);
        assert_eq!(doc.lines(&Target::Lines(5..9)), 2..2);
    }

    #[test]
    fn selection_after_maps_through_the_edits() {
        let change = Change::new(
            vec![TextEdit::insert(0, "// "), TextEdit::delete(5..7)],
            None,
        );
        assert_eq!(change.selection_after(0..9), 0..10);
        assert_eq!(change.selection_after(0..0), 3..3);
        assert_eq!(change.selection_after(6..6), 8..8);
        let explicit = Change::new(vec![TextEdit::insert(0, "x")], Some(1..1));
        assert_eq!(explicit.selection_after(0..0), 1..1);
    }

    #[test]
    fn coalesce_replaces_the_spanned_range_once() {
        let text = "a b c d";
        let change = Change::new(
            vec![TextEdit::new(2..3, "B"), TextEdit::new(6..7, "D!")],
            None,
        );
        assert_eq!(change.coalesce(text), Some(TextEdit::new(2..7, "B c D!")));
        assert_eq!(Change::default().coalesce(text), None);
    }

    #[test]
    fn no_op_edits_make_an_empty_change() {
        let change = Change::new(vec![TextEdit::insert(3, "")], Some(0..0));
        assert!(change.is_empty());
        assert_eq!(change.selection, None);
    }

    #[test]
    fn syntax_errors_count_characters() {
        let error = SyntaxError::at_byte("ab\nøx!", 6, "bad");
        assert_eq!((error.line, error.column, error.offset), (2, 3, 5));
        let error = SyntaxError::at_byte("ø", 1, "inside a character");
        assert_eq!((error.line, error.column, error.offset), (1, 1, 0));
        assert_eq!(error.to_string(), "inside a character at line 1, column 1");
    }

    proptest! {
        #[test]
        fn line_targets_match_a_naive_model((text, target) in text_and_target()) {
            prop_assert_eq!(Doc::new(&text).lines(&target), naive_lines(&text, &target));
        }
    }
}
