//! Line and blank operations (Edit > Line Operations, Edit > Blank Operations). Blanks are
//! spaces and tabs; other whitespace counts as content.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ops::Range;

use super::{
    Change, Doc, Target, char_len, delete_lines, is_blank, is_blank_char, leading_blanks,
    replace_lines, trailing_blanks,
};
use crate::text::{Bias, TextEdit, map_offset};

/// Which repeated lines [`remove_duplicates`] removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Duplicates {
    /// Every repeat of an earlier line; the first occurrence stays.
    All,
    /// Only a line equal to the line before it.
    Consecutive,
}

/// Which lines [`remove_empty`] removes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmptyLines {
    /// Lines without any character.
    Empty,
    /// Also lines of only spaces and tabs.
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trim {
    Leading,
    Trailing,
    Both,
}

/// Where tab and space conversion applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The indentation at the start of each line.
    Leading,
    /// Everywhere in the line.
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    /// By code point, so `B` sorts before `a`.
    Lexical,
    /// By lowercase code point.
    LexicalIgnoreCase,
    /// Natural order: runs of digits, with a minus sign unless it follows a letter or digit,
    /// compare as integers of any size, the rest by code point ("As Integers").
    Integer,
    /// By the decimal number at the start of the line, with `.` as separator. Lines without
    /// one come first (last when descending), in their order ("As Decimals (Dot)").
    DecimalDot,
    /// The same with `,` as separator ("As Decimals (Comma)").
    DecimalComma,
    /// By length in characters.
    Length,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Order {
    Ascending,
    Descending,
}

/// Duplicates the selected text after itself (Ctrl+D). With a caret, `Lines` or `Document`
/// it duplicates the lines instead. The selection or caret stays on the original.
pub fn duplicate(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    if let Target::Selection(range) = &target {
        let range = doc.clamp(range);
        if !range.is_empty() {
            let copy = doc.slice(range.clone()).to_owned();
            return Change::new(vec![TextEdit::insert(range.end, copy)], Some(range));
        }
    }
    let lines = doc.lines(&target);
    if lines.is_empty() {
        return Change::default();
    }
    let copy = format!("\n{}", doc.block(lines.clone()));
    let at = doc.end(lines.end - 1);
    Change::new(vec![TextEdit::insert(at, copy)], caret_of(&doc, &target))
}

/// Deletes the lines of the target with their line break (Ctrl+Shift+L). The caret goes to
/// the start of the line that takes their place, or of the new last line.
pub fn delete(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    if lines.is_empty() {
        return Change::default();
    }
    let caret = if lines.end < doc.line_count() {
        doc.start(lines.start)
    } else if lines.start > 0 {
        doc.start(lines.start - 1)
    } else {
        0
    };
    Change::new(delete_lines(&doc, lines), Some(caret..caret))
}

/// Swaps the lines of the target with the line above (Ctrl+Shift+Up). The selection moves
/// with them.
pub fn move_up(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    if lines.is_empty() || lines.start == 0 {
        return Change::default();
    }
    let above = lines.start - 1;
    let moved = format!("{}\n{}", doc.block(lines.clone()), doc.line(above));
    let edit = TextEdit::new(doc.start(above)..doc.end(lines.end - 1), moved);
    let shift = doc.end(above) - doc.start(above) + 1;
    let selection = selection_of(&doc, &target).map(|range| range.start - shift..range.end - shift);
    Change::new(vec![edit], selection)
}

/// Swaps the lines of the target with the line below (Ctrl+Shift+Down). The selection moves
/// with them.
pub fn move_down(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    if lines.is_empty() || lines.end >= doc.line_count() {
        return Change::default();
    }
    let below = lines.end;
    let moved = format!("{}\n{}", doc.line(below), doc.block(lines.clone()));
    let edit = TextEdit::new(doc.start(lines.start)..doc.end(below), moved);
    let shift = doc.end(below) - doc.start(below) + 1;
    let len = doc.len();
    let selection = selection_of(&doc, &target)
        .map(|range| (range.start + shift).min(len)..(range.end + shift).min(len));
    Change::new(vec![edit], selection)
}

/// Joins the lines of the target into one (Ctrl+J). Line breaks, blank lines and the
/// indentation of the joined lines go; one space separates the parts unless the part before
/// ends in a blank. The first non-blank line keeps its indentation; if all lines are blank,
/// the first one with spaces or tabs is kept. A caret or a selection within one line joins
/// that line with the next.
pub fn join(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let mut lines = doc.lines(&target);
    let selection = selection_of(&doc, &target);
    if lines.len() == 1 && selection.is_some() && lines.end < doc.line_count() {
        lines.end += 1;
    }
    if lines.len() < 2 {
        return Change::default();
    }
    let (first, last) = (lines.start, lines.end - 1);
    let mut edits = Vec::new();
    match (first..=last).find(|&line| !is_blank(doc.line(line))) {
        None => match (first..=last).find(|&line| !doc.line(line).is_empty()) {
            None => edits.push(TextEdit::delete(doc.start(first)..doc.end(last))),
            Some(kept) => {
                edits.push(TextEdit::delete(doc.start(first)..doc.start(kept)));
                edits.push(TextEdit::delete(doc.end(kept)..doc.end(last)));
            }
        },
        Some(kept) => {
            if kept > first {
                edits.push(TextEdit::delete(doc.start(first)..doc.start(kept)));
            }
            let mut ends_blank = doc.line(kept).ends_with(is_blank_char);
            for line in kept + 1..=last {
                let content = doc.line(line);
                let indent = leading_blanks(content);
                let rest = &content[indent..];
                let separator = if rest.is_empty() || ends_blank {
                    ""
                } else {
                    " "
                };
                edits.push(TextEdit::new(
                    doc.end(line - 1)..doc.start(line) + indent,
                    separator,
                ));
                if !rest.is_empty() {
                    ends_blank = rest.ends_with(is_blank_char);
                }
            }
        }
    }
    edits.retain(|edit| !edit.is_noop());
    let selection = selection.map(|range| {
        if range.is_empty() {
            let joint = doc.start(first + 1) + leading_blanks(doc.line(first + 1));
            let caret = map_offset(joint, &edits, Bias::After);
            caret..caret
        } else {
            let delta: isize = edits.iter().map(TextEdit::delta).sum();
            let joined = (doc.end(last) - doc.start(first)).saturating_add_signed(delta);
            doc.start(first)..doc.start(first) + joined
        }
    });
    Change::new(edits, selection)
}

/// Inserts an empty line above the first line of the target and puts the caret on it
/// (Ctrl+Alt+Enter).
pub fn insert_blank_above(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    if lines.is_empty() {
        return Change::default();
    }
    let at = doc.start(lines.start);
    Change::new(vec![TextEdit::insert(at, "\n")], Some(at..at))
}

/// Inserts an empty line below the last line of the target and puts the caret on it
/// (Ctrl+Alt+Shift+Enter).
pub fn insert_blank_below(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    if lines.is_empty() {
        return Change::default();
    }
    let at = doc.end(lines.end - 1);
    Change::new(vec![TextEdit::insert(at, "\n")], Some(at + 1..at + 1))
}

/// Removes repeated lines of the target; lines must match exactly.
pub fn remove_duplicates(text: &str, target: Target, which: Duplicates) -> Change {
    let doc = Doc::new(text);
    let mut seen = HashSet::new();
    let mut previous = None;
    let repeated: Vec<usize> = doc
        .lines(&target)
        .filter(|&line| {
            let content = doc.line(line);
            match which {
                Duplicates::All => !seen.insert(content),
                Duplicates::Consecutive => previous.replace(content) == Some(content),
            }
        })
        .collect();
    Change::new(delete_lines(&doc, repeated), None)
}

/// Removes the empty lines of the target, or also the lines of spaces and tabs.
pub fn remove_empty(text: &str, target: Target, which: EmptyLines) -> Change {
    let doc = Doc::new(text);
    let empty: Vec<usize> = doc
        .lines(&target)
        .filter(|&line| match which {
            EmptyLines::Empty => doc.line(line).is_empty(),
            EmptyLines::Blank => is_blank(doc.line(line)),
        })
        .collect();
    Change::new(delete_lines(&doc, empty), None)
}

/// Removes spaces and tabs at the start, the end or both ends of each line.
pub fn trim(text: &str, target: Target, which: Trim) -> Change {
    let doc = Doc::new(text);
    let mut edits = Vec::new();
    for line in doc.lines(&target) {
        let content = doc.line(line);
        let (start, end) = (doc.start(line), doc.end(line));
        let lead = if which == Trim::Trailing {
            0
        } else {
            leading_blanks(content)
        };
        let trail = if which == Trim::Leading {
            0
        } else {
            trailing_blanks(content)
        };
        if (lead > 0 && lead == content.len()) || (trail > 0 && trail == content.len()) {
            edits.push(TextEdit::delete(start..end));
            continue;
        }
        if lead > 0 {
            edits.push(TextEdit::delete(start..start + lead));
        }
        if trail > 0 {
            edits.push(TextEdit::delete(end - trail..end));
        }
    }
    Change::new(edits, None)
}

/// Replaces tabs with spaces up to the next tab stop. Columns count characters, like
/// GtkSourceView's visual column. A tab width of 0 counts as 1.
pub fn tabs_to_spaces(text: &str, target: Target, tab_width: usize, scope: Scope) -> Change {
    let doc = Doc::new(text);
    let width = tab_width.max(1);
    let mut edits = Vec::new();
    for line in doc.lines(&target) {
        let content = doc.line(line);
        let limit = match scope {
            Scope::Leading => leading_blanks(content),
            Scope::All => content.len(),
        };
        let mut column = 0;
        for (index, c) in content[..limit].chars().enumerate() {
            if c == '\t' {
                let spaces = width - column % width;
                let at = doc.start(line) + index;
                edits.push(TextEdit::new(at..at + 1, " ".repeat(spaces)));
                column += spaces;
            } else {
                column += 1;
            }
        }
    }
    Change::new(edits, None)
}

/// Rewrites runs of spaces and tabs as tabs up to the last tab stop they cross, then spaces,
/// keeping every column. A single space stays a space. A tab width of 0 counts as 1.
pub fn spaces_to_tabs(text: &str, target: Target, tab_width: usize, scope: Scope) -> Change {
    let doc = Doc::new(text);
    let width = tab_width.max(1);
    let mut edits = Vec::new();
    for line in doc.lines(&target) {
        let content = doc.line(line);
        let limit = match scope {
            Scope::Leading => leading_blanks(content),
            Scope::All => content.len(),
        };
        let mut column = 0;
        let mut run: Option<BlankRun> = None;
        for (index, (byte, c)) in content[..limit].char_indices().enumerate() {
            if is_blank_char(c) {
                let run = run.get_or_insert(BlankRun {
                    index,
                    byte,
                    column,
                    tabs: false,
                    length: 0,
                });
                run.length += 1;
                if c == '\t' {
                    run.tabs = true;
                    column += width - column % width;
                } else {
                    column += 1;
                }
            } else {
                if let Some(run) = run.take() {
                    edits.extend(run.retabbed(&doc, line, column, width));
                }
                column += 1;
            }
        }
        if let Some(run) = run.take() {
            edits.extend(run.retabbed(&doc, line, column, width));
        }
    }
    Change::new(edits, None)
}

/// A run of spaces and tabs at character `index` (byte `byte`) of a line and visual `column`.
struct BlankRun {
    index: usize,
    byte: usize,
    column: usize,
    tabs: bool,
    length: usize,
}

impl BlankRun {
    /// The edit that rewrites the run, ending at visual column `end`, with tabs.
    fn retabbed(&self, doc: &Doc<'_>, line: usize, end: usize, width: usize) -> Option<TextEdit> {
        let single_space = !self.tabs && self.length == 1;
        if single_space || (self.column / width + 1) * width > end {
            return None;
        }
        let tabs = end / width - self.column / width;
        let spaces = end % width;
        let new = format!("{}{}", "\t".repeat(tabs), " ".repeat(spaces));
        let old = &doc.line(line)[self.byte..self.byte + self.length];
        let start = doc.start(line) + self.index;
        (old != new).then(|| TextEdit::new(start..start + self.length, new))
    }
}

/// Sorts the lines of the target. Every sort is stable, also descending: equal lines keep
/// their order.
pub fn sort(text: &str, target: Target, key: SortKey, order: Order) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    let old: Vec<&str> = lines.clone().map(|line| doc.line(line)).collect();
    let new = sort_lines(&old, key, order);
    Change::new(replace_lines(&doc, lines, &old, &new), None)
}

/// Reverses the order of the lines of the target.
pub fn reverse(text: &str, target: Target) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    let old: Vec<&str> = lines.clone().map(|line| doc.line(line)).collect();
    let new: Vec<&str> = old.iter().rev().copied().collect();
    Change::new(replace_lines(&doc, lines, &old, &new), None)
}

/// The stable sort behind [`sort`].
pub fn sort_lines<'a>(lines: &[&'a str], key: SortKey, order: Order) -> Vec<&'a str> {
    let directed = |ordering: Ordering| match order {
        Order::Ascending => ordering,
        Order::Descending => ordering.reverse(),
    };
    let sorted_by = |compare: fn(&str, &str) -> Ordering| {
        let mut sorted = lines.to_vec();
        sorted.sort_by(|a, b| directed(compare(a, b)));
        sorted
    };
    match key {
        SortKey::Lexical => sorted_by(|a, b| a.cmp(b)),
        SortKey::LexicalIgnoreCase => sorted_by(compare_ignoring_case),
        SortKey::Integer => {
            let mut arena = Vec::new();
            let spans: Vec<Range<usize>> = lines
                .iter()
                .map(|line| {
                    let start = arena.len();
                    arena.extend(chunks(line));
                    start..arena.len()
                })
                .collect();
            let mut order: Vec<usize> = (0..lines.len()).collect();
            order.sort_by(|&a, &b| {
                let (a, b) = (&arena[spans[a].clone()], &arena[spans[b].clone()]);
                directed(compare_chunk_lists(a, b))
            });
            order.into_iter().map(|index| lines[index]).collect()
        }
        SortKey::Length => sorted_by_key(lines, char_len, |a, b| directed(a.cmp(b))),
        SortKey::DecimalDot => sorted_by_key(
            lines,
            |line| leading_decimal(line, '.'),
            |a, b| directed(compare_decimals(*a, *b)),
        ),
        SortKey::DecimalComma => sorted_by_key(
            lines,
            |line| leading_decimal(line, ','),
            |a, b| directed(compare_decimals(*a, *b)),
        ),
    }
}

fn sorted_by_key<'a, K>(
    lines: &[&'a str],
    key: impl Fn(&'a str) -> K,
    compare: impl Fn(&K, &K) -> Ordering,
) -> Vec<&'a str> {
    let mut keyed: Vec<(K, &'a str)> = lines.iter().map(|&line| (key(line), line)).collect();
    keyed.sort_by(|a, b| compare(&a.0, &b.0));
    keyed.into_iter().map(|(_, line)| line).collect()
}

fn compare_ignoring_case(a: &str, b: &str) -> Ordering {
    a.chars()
        .flat_map(char::to_lowercase)
        .cmp(b.chars().flat_map(char::to_lowercase))
}

fn compare_decimals(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.partial_cmp(&b).unwrap_or(Ordering::Equal),
        (a, b) => a.is_some().cmp(&b.is_some()),
    }
}

/// The decimal number at the start of `line` after spaces and tabs: an optional `-`, digits
/// and a fraction after `separator`. Anything after it is ignored.
fn leading_decimal(line: &str, separator: char) -> Option<f64> {
    let rest = line.trim_start_matches(is_blank_char);
    let (negative, rest) = match rest.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, rest),
    };
    let digits = |text: &str| {
        text.find(|c: char| !c.is_ascii_digit())
            .unwrap_or(text.len())
    };
    let integer = &rest[..digits(rest)];
    let fraction = rest[integer.len()..]
        .strip_prefix(separator)
        .map_or("", |rest| &rest[..digits(rest)]);
    if integer.is_empty() && fraction.is_empty() {
        return None;
    }
    let sign = if negative { "-" } else { "" };
    let integer = if integer.is_empty() { "0" } else { integer };
    let fraction = if fraction.is_empty() { "0" } else { fraction };
    format!("{sign}{integer}.{fraction}").parse().ok()
}

#[derive(Debug, Clone, Copy)]
enum Chunk<'a> {
    Number {
        negative: bool,
        zeros: usize,
        digits: &'a str,
        first: char,
    },
    Text(&'a str),
}

/// Splits a line into numbers and the text between them.
fn chunks(line: &str) -> impl Iterator<Item = Chunk<'_>> {
    let bytes = line.as_bytes();
    let starts_number = move |at: usize, after_alphanumeric: bool| {
        bytes[at].is_ascii_digit()
            || bytes[at] == b'-'
                && !after_alphanumeric
                && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)
    };
    let mut position = 0;
    let mut after_alphanumeric = false;
    std::iter::from_fn(move || {
        if position >= line.len() {
            return None;
        }
        let start = position;
        if starts_number(start, after_alphanumeric) {
            let negative = bytes[start] == b'-';
            let digits_start = start + usize::from(negative);
            let length = bytes[digits_start..]
                .iter()
                .take_while(|byte| byte.is_ascii_digit())
                .count();
            let zeros = bytes[digits_start..digits_start + length]
                .iter()
                .take_while(|&&byte| byte == b'0')
                .count();
            position = digits_start + length;
            after_alphanumeric = true;
            return Some(Chunk::Number {
                negative,
                zeros,
                digits: &line[digits_start + zeros..position],
                first: char::from(bytes[start]),
            });
        }
        for (offset, c) in line[start..].char_indices() {
            if offset > 0 && starts_number(start + offset, after_alphanumeric) {
                position = start + offset;
                return Some(Chunk::Text(&line[start..position]));
            }
            after_alphanumeric = c.is_alphanumeric();
        }
        position = line.len();
        Some(Chunk::Text(&line[start..]))
    })
}

fn compare_chunks(a: Chunk<'_>, b: Chunk<'_>) -> Ordering {
    let first_char = |text: &str| text.chars().next().unwrap_or('\0');
    match (a, b) {
        (Chunk::Text(a), Chunk::Text(b)) => a.cmp(b),
        (Chunk::Number { first, .. }, Chunk::Text(text)) => {
            first.cmp(&first_char(text)).then(Ordering::Less)
        }
        (Chunk::Text(text), Chunk::Number { first, .. }) => {
            first_char(text).cmp(&first).then(Ordering::Greater)
        }
        (
            Chunk::Number {
                negative: a_negative,
                zeros: a_zeros,
                digits: a_digits,
                ..
            },
            Chunk::Number {
                negative: b_negative,
                zeros: b_zeros,
                digits: b_digits,
                ..
            },
        ) => b_negative.cmp(&a_negative).then_with(|| {
            let magnitude = a_digits
                .len()
                .cmp(&b_digits.len())
                .then_with(|| a_digits.cmp(b_digits));
            let value = if a_negative {
                magnitude.reverse()
            } else {
                magnitude
            };
            value.then_with(|| b_zeros.cmp(&a_zeros))
        }),
    }
}

/// Natural order: equal numbers with more leading zeros come first, and a number sorts
/// before text that starts with the same character.
#[cfg(test)]
fn compare_natural(a: &str, b: &str) -> Ordering {
    compare_chunk_lists(
        &chunks(a).collect::<Vec<_>>(),
        &chunks(b).collect::<Vec<_>>(),
    )
}

fn compare_chunk_lists(a: &[Chunk<'_>], b: &[Chunk<'_>]) -> Ordering {
    a.iter()
        .zip(b)
        .map(|(x, y)| compare_chunks(*x, *y))
        .find(|ordering| ordering.is_ne())
        .unwrap_or_else(|| a.len().cmp(&b.len()))
}

fn selection_of(doc: &Doc<'_>, target: &Target) -> Option<Range<usize>> {
    match target {
        Target::Selection(range) => Some(doc.clamp(range)),
        _ => None,
    }
}

fn caret_of(doc: &Doc<'_>, target: &Target) -> Option<Range<usize>> {
    selection_of(doc, target).filter(Range::is_empty)
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use proptest::prelude::*;

    fn run(text: &str, change: Change) -> String {
        applied(text, &change)
    }

    fn sel(range: Range<usize>) -> Target {
        Target::Selection(range)
    }

    #[test]
    fn duplicate_copies_the_caret_line_below_and_keeps_the_caret() {
        let change = duplicate("one\ntwo\nthree", Target::caret(5));
        assert_eq!(change.selection, Some(5..5));
        assert_eq!(run("one\ntwo\nthree", change), "one\ntwo\ntwo\nthree");
        assert_eq!(
            run("one\ntwo", duplicate("one\ntwo", Target::caret(7))),
            "one\ntwo\ntwo"
        );
        assert_eq!(run("", duplicate("", Target::caret(0))), "\n");
    }

    #[test]
    fn duplicate_copies_a_selection_right_after_itself() {
        let change = duplicate("say hello", sel(4..9));
        assert_eq!(change.selection, Some(4..9));
        assert_eq!(run("say hello", change), "say hellohello");
        assert_eq!(run("ab\ncd", duplicate("ab\ncd", sel(1..4))), "ab\ncb\ncd");
    }

    #[test]
    fn duplicate_copies_whole_lines_for_line_and_document_targets() {
        assert_eq!(
            run("a\nb\nc", duplicate("a\nb\nc", Target::Lines(0..2))),
            "a\nb\na\nb\nc"
        );
        assert_eq!(
            run("a\nb\n", duplicate("a\nb\n", Target::Document)),
            "a\nb\na\nb\n"
        );
    }

    #[test]
    fn delete_removes_lines_with_their_line_break() {
        let change = delete("one\ntwo\nthree", Target::caret(5));
        assert_eq!(change.selection, Some(4..4));
        assert_eq!(run("one\ntwo\nthree", change), "one\nthree");
        let change = delete("one\ntwo", Target::caret(6));
        assert_eq!(change.selection, Some(0..0));
        assert_eq!(run("one\ntwo", change), "one");
        assert_eq!(run("one\ntwo\n", delete("one\ntwo\n", sel(4..7))), "one\n");
        assert_eq!(run("only", delete("only", Target::caret(2))), "");
        assert!(delete("", Target::caret(0)).is_empty());
    }

    #[test]
    fn delete_leaves_out_a_line_the_selection_reaches_at_column_zero() {
        assert_eq!(run("a\nb\nc\n", delete("a\nb\nc\n", sel(0..4))), "c\n");
    }

    #[test]
    fn delete_on_the_line_after_a_final_newline_removes_that_newline() {
        assert_eq!(run("a\n", delete("a\n", Target::caret(2))), "a");
    }

    #[test]
    fn move_up_swaps_with_the_line_above_and_moves_the_selection() {
        let change = move_up("a\nbb\ncc", sel(3..4));
        assert_eq!(change.selection, Some(1..2));
        assert_eq!(run("a\nbb\ncc", change), "bb\na\ncc");
        assert_eq!(run("a\nb", move_up("a\nb", Target::caret(3))), "b\na");
        assert!(move_up("a\nb", Target::caret(0)).is_empty());
    }

    #[test]
    fn move_down_swaps_with_the_line_below_and_moves_the_selection() {
        let change = move_down("a\nbb\ncc", sel(0..5));
        assert_eq!(change.selection, Some(3..7));
        assert_eq!(run("a\nbb\ncc", change), "cc\na\nbb");
        assert!(move_down("a\nb", Target::caret(3)).is_empty());
        let change = move_down("a\nb\nc", sel(0..2));
        assert_eq!(change.selection, Some(2..4));
        assert_eq!(run("a\nb\nc", change), "b\na\nc");
    }

    #[test]
    fn move_down_into_the_line_after_a_final_newline_swaps_with_it() {
        assert_eq!(
            run("a\nb\n", move_down("a\nb\n", Target::caret(2))),
            "a\n\nb"
        );
    }

    #[test]
    fn join_trims_indentation_and_separates_with_one_space() {
        let text = "fn f(\n    a,\n    b\n)";
        let change = join(text, sel(0..text.len()));
        assert_eq!(change.selection, Some(0..12));
        assert_eq!(run(text, change), "fn f( a, b )");
    }

    #[test]
    fn join_skips_blank_lines_and_existing_trailing_blanks() {
        let text = "a \n\n   \n\tb\nc";
        assert_eq!(run(text, join(text, Target::Document)), "a b c");
        let text = "\n  \n  first\nsecond";
        assert_eq!(run(text, join(text, Target::Document)), "  first second");
        assert_eq!(run("\n \n\t", join("\n \n\t", Target::Document)), " ");
        assert_eq!(run("\n\n", join("\n\n", Target::Document)), "\n");
    }

    #[test]
    fn join_with_a_caret_joins_the_next_line() {
        let change = join("one\n  two\nthree", Target::caret(1));
        assert_eq!(change.selection, Some(4..4));
        assert_eq!(run("one\n  two\nthree", change), "one two\nthree");
        assert!(join("one\ntwo", Target::caret(5)).is_empty());
        assert!(join("one\ntwo", Target::Lines(1..2)).is_empty());
    }

    #[test]
    fn join_of_the_document_keeps_the_final_newline() {
        assert_eq!(run("a\nb\n", join("a\nb\n", Target::Document)), "a b\n");
    }

    #[test]
    fn blank_lines_are_inserted_above_and_below_with_the_caret_on_them() {
        let change = insert_blank_above("a\nb", Target::caret(3));
        assert_eq!(change.selection, Some(2..2));
        assert_eq!(run("a\nb", change), "a\n\nb");
        let change = insert_blank_below("a\nb", Target::caret(0));
        assert_eq!(change.selection, Some(2..2));
        assert_eq!(run("a\nb", change), "a\n\nb");
        let change = insert_blank_below("a\nb", Target::caret(3));
        assert_eq!(change.selection, Some(4..4));
        assert_eq!(run("a\nb", change), "a\nb\n");
    }

    #[test]
    fn remove_duplicates_keeps_first_occurrences() {
        let text = "b\na\nb\nc\na\n";
        assert_eq!(
            run(
                text,
                remove_duplicates(text, Target::Document, Duplicates::All)
            ),
            "b\na\nc\n"
        );
        let text = "a\na\nb\na\na";
        assert_eq!(
            run(
                text,
                remove_duplicates(text, Target::Document, Duplicates::Consecutive)
            ),
            "a\nb\na"
        );
        assert_eq!(
            run(
                text,
                remove_duplicates(text, Target::Document, Duplicates::All)
            ),
            "a\nb"
        );
    }

    #[test]
    fn remove_duplicates_is_exact_and_limited_to_the_target() {
        let text = "a\nA\na \na\nx\na";
        assert_eq!(
            run(text, remove_duplicates(text, sel(0..8), Duplicates::All)),
            "a\nA\na \nx\na"
        );
    }

    #[test]
    fn remove_empty_keeps_the_final_newline() {
        let text = "a\n\n  \nb\n\n";
        assert_eq!(
            run(
                text,
                remove_empty(text, Target::Document, EmptyLines::Empty)
            ),
            "a\n  \nb\n"
        );
        assert_eq!(
            run(
                text,
                remove_empty(text, Target::Document, EmptyLines::Blank)
            ),
            "a\nb\n"
        );
        assert_eq!(
            run(
                "a\n ",
                remove_empty("a\n ", Target::Document, EmptyLines::Blank)
            ),
            "a"
        );
    }

    #[test]
    fn trim_removes_only_spaces_and_tabs() {
        let text = " \ta b \t\n\u{a0}x\u{a0}\n   \nend";
        assert_eq!(
            run(text, trim(text, Target::Document, Trim::Trailing)),
            " \ta b\n\u{a0}x\u{a0}\n\nend"
        );
        assert_eq!(
            run(text, trim(text, Target::Document, Trim::Leading)),
            "a b \t\n\u{a0}x\u{a0}\n\nend"
        );
        assert_eq!(
            run(text, trim(text, Target::Document, Trim::Both)),
            "a b\n\u{a0}x\u{a0}\n\nend"
        );
    }

    #[test]
    fn tabs_become_spaces_up_to_the_next_tab_stop() {
        let text = "\tx\tyz\t!\n  \t\tq";
        assert_eq!(
            run(text, tabs_to_spaces(text, Target::Document, 4, Scope::All)),
            "    x   yz  !\n        q"
        );
        assert_eq!(
            run(
                text,
                tabs_to_spaces(text, Target::Document, 4, Scope::Leading)
            ),
            "    x\tyz\t!\n        q"
        );
        assert_eq!(
            run(
                "é\tx",
                tabs_to_spaces("é\tx", Target::Document, 0, Scope::All)
            ),
            "é x"
        );
    }

    #[test]
    fn spaces_become_tabs_without_moving_any_column() {
        let text = "        x = 1;      // note\n      y\n abc d\n  \t z";
        assert_eq!(
            run(text, spaces_to_tabs(text, Target::Document, 4, Scope::All)),
            "\t\tx = 1;\t\t// note\n\t  y\n abc d\n\t z"
        );
        assert_eq!(
            run(
                text,
                spaces_to_tabs(text, Target::Document, 4, Scope::Leading)
            ),
            "\t\tx = 1;      // note\n\t  y\n abc d\n\t z"
        );
    }

    #[test]
    fn a_single_space_at_a_tab_stop_stays_a_space() {
        assert!(spaces_to_tabs("abc d", Target::Document, 4, Scope::All).is_empty());
        assert_eq!(
            run(
                "ab  d",
                spaces_to_tabs("ab  d", Target::Document, 4, Scope::All)
            ),
            "ab\td"
        );
    }

    fn sorted(lines: &[&str], key: SortKey, order: Order) -> Vec<String> {
        sort_lines(lines, key, order)
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn lexical_sorts_by_code_point_and_ignoring_case_is_stable() {
        let lines = ["b", "B", "a", "A", "_"];
        assert_eq!(
            sorted(&lines, SortKey::Lexical, Order::Ascending),
            ["A", "B", "_", "a", "b"]
        );
        assert_eq!(
            sorted(&lines, SortKey::LexicalIgnoreCase, Order::Ascending),
            ["_", "a", "A", "b", "B"]
        );
        assert_eq!(
            sorted(&lines, SortKey::LexicalIgnoreCase, Order::Descending),
            ["b", "B", "a", "A", "_"]
        );
    }

    #[test]
    fn integer_sort_is_natural() {
        let lines = [
            "item10",
            "item9",
            "-3",
            "12",
            "003",
            "3",
            "x",
            "",
            "2024-10-01",
            "2024-09-30",
            "-12",
            "5 apples",
            "-0",
        ];
        assert_eq!(
            sorted(&lines, SortKey::Integer, Order::Ascending),
            [
                "",
                "-12",
                "-3",
                "-0",
                "003",
                "3",
                "5 apples",
                "12",
                "2024-09-30",
                "2024-10-01",
                "item9",
                "item10",
                "x"
            ]
        );
        assert_eq!(
            sorted(
                &["a-5", "a-10", "a -10"],
                SortKey::Integer,
                Order::Ascending
            ),
            ["a -10", "a-5", "a-10"]
        );
        let big = ["99999999999999999999999", "100000000000000000000000", "7"];
        assert_eq!(
            sorted(&big, SortKey::Integer, Order::Ascending),
            ["7", "99999999999999999999999", "100000000000000000000000"]
        );
    }

    #[test]
    fn decimal_sorts_read_the_number_at_the_start() {
        let lines = [
            "10.5 kg", "", "abc", "-2.25", " 3", "3.10", "-", ".5", "1.2.3",
        ];
        assert_eq!(
            sorted(&lines, SortKey::DecimalDot, Order::Ascending),
            [
                "", "abc", "-", "-2.25", ".5", "1.2.3", " 3", "3.10", "10.5 kg"
            ]
        );
        assert_eq!(
            sorted(&lines, SortKey::DecimalDot, Order::Descending),
            [
                "10.5 kg", "3.10", " 3", "1.2.3", ".5", "-2.25", "", "abc", "-"
            ]
        );
        let lines = ["1,5", "1.9", "1,25", "-0,5"];
        assert_eq!(
            sorted(&lines, SortKey::DecimalComma, Order::Ascending),
            ["-0,5", "1.9", "1,25", "1,5"]
        );
    }

    #[test]
    fn length_sort_counts_characters_and_is_stable() {
        let lines = ["ccc", "a", "日本", "bb", "x"];
        assert_eq!(
            sorted(&lines, SortKey::Length, Order::Ascending),
            ["a", "x", "日本", "bb", "ccc"]
        );
        assert_eq!(
            sorted(&lines, SortKey::Length, Order::Descending),
            ["ccc", "日本", "bb", "a", "x"]
        );
    }

    #[test]
    fn sorting_the_document_keeps_the_final_newline_at_the_end() {
        let text = "b\nc\na\n";
        assert_eq!(
            run(
                text,
                sort(text, Target::Document, SortKey::Lexical, Order::Ascending)
            ),
            "a\nb\nc\n"
        );
    }

    #[test]
    fn sorting_touches_only_the_lines_that_move() {
        let text = "a\nc\nb\nd";
        let change = sort(text, Target::Document, SortKey::Lexical, Order::Ascending);
        assert_eq!(change.edits, vec![TextEdit::new(2..5, "b\nc")]);
        assert!(sort("a\nb", Target::Document, SortKey::Lexical, Order::Ascending).is_empty());
    }

    #[test]
    fn reverse_reverses_the_target_lines() {
        let text = "1\n2\n3\n4";
        assert_eq!(run(text, reverse(text, sel(2..5))), "1\n3\n2\n4");
        assert_eq!(run("a\nb\n", reverse("a\nb\n", Target::Document)), "b\na\n");
    }

    fn lines_of(text: &str) -> Vec<String> {
        text.split('\n').map(str::to_owned).collect()
    }

    /// Replaces the target lines of `text` with `f(lines)`.
    fn model(text: &str, target: &Target, f: impl FnOnce(Vec<String>) -> Vec<String>) -> String {
        let range = naive_lines(text, target);
        let old = lines_of(text)[range.clone()].to_vec();
        let mut new = f(old);
        let mut all = lines_of(text);
        if new.is_empty() && range.len() == all.len() {
            new.push(String::new());
        }
        all.splice(range, new);
        all.join("\n")
    }

    fn is_blank_line(line: &str) -> bool {
        line.chars().all(|c| c == ' ' || c == '\t')
    }

    fn naive_join(lines: &[String]) -> String {
        if lines.iter().all(|line| is_blank_line(line)) {
            return lines
                .iter()
                .find(|line| !line.is_empty())
                .cloned()
                .unwrap_or_default();
        }
        let mut out = String::new();
        let mut started = false;
        for line in lines {
            if is_blank_line(line) {
                continue;
            }
            if !started {
                out.push_str(line);
                started = true;
                continue;
            }
            let rest = line.trim_start_matches([' ', '\t']);
            if !out.ends_with([' ', '\t']) {
                out.push(' ');
            }
            out.push_str(rest);
        }
        out
    }

    fn naive_tabs_to_spaces(line: &str, width: usize) -> String {
        let mut out = String::new();
        let mut column = 0;
        for c in line.chars() {
            if c == '\t' {
                let spaces = width - column % width;
                out.push_str(&" ".repeat(spaces));
                column += spaces;
            } else {
                out.push(c);
                column += 1;
            }
        }
        out
    }

    const KEYS: [SortKey; 6] = [
        SortKey::Lexical,
        SortKey::LexicalIgnoreCase,
        SortKey::Integer,
        SortKey::DecimalDot,
        SortKey::DecimalComma,
        SortKey::Length,
    ];

    fn key_order(key: SortKey, a: &str, b: &str) -> Ordering {
        match key {
            SortKey::Lexical => a.cmp(b),
            SortKey::LexicalIgnoreCase => compare_ignoring_case(a, b),
            SortKey::Integer => compare_natural(a, b),
            SortKey::DecimalDot => {
                compare_decimals(leading_decimal(a, '.'), leading_decimal(b, '.'))
            }
            SortKey::DecimalComma => {
                compare_decimals(leading_decimal(a, ','), leading_decimal(b, ','))
            }
            SortKey::Length => char_len(a).cmp(&char_len(b)),
        }
    }

    fn sort_line() -> impl Strategy<Value = String> {
        "[-a-cA-C0-9 .,é]{0,6}"
    }

    proptest! {
        #[test]
        fn line_edits_match_naive_models((text, target) in text_and_target()) {
            prop_assert_eq!(
                run(&text, delete(&text, target.clone())),
                model(&text, &target, |_| Vec::new())
            );
            let range = naive_lines(&text, &target);
            let expected = if !range.is_empty() && range.start > 0 {
                let mut lines = lines_of(&text);
                let above = lines.remove(range.start - 1);
                lines.insert(range.end - 1, above);
                lines.join("\n")
            } else {
                text.clone()
            };
            prop_assert_eq!(run(&text, move_up(&text, target.clone())), expected);
            let count = lines_of(&text).len();
            let expected = if !range.is_empty() && range.end < count {
                let mut lines = lines_of(&text);
                let below = lines.remove(range.end);
                lines.insert(range.start, below);
                lines.join("\n")
            } else {
                text.clone()
            };
            prop_assert_eq!(run(&text, move_down(&text, target.clone())), expected);
            prop_assert_eq!(
                run(&text, insert_blank_above(&text, target.clone())),
                if range.is_empty() { text.clone() } else {
                    splice_lines(&text, range.start..range.start, vec![String::new()])
                }
            );
            prop_assert_eq!(
                run(&text, insert_blank_below(&text, target.clone())),
                if range.is_empty() { text.clone() } else {
                    splice_lines(&text, range.end..range.end, vec![String::new()])
                }
            );
            prop_assert_eq!(
                run(&text, reverse(&text, target.clone())),
                model(&text, &target, |lines| lines.into_iter().rev().collect())
            );
        }

        #[test]
        fn duplicate_matches_a_naive_model((text, target) in text_and_target()) {
            let chars: Vec<char> = text.chars().collect();
            let len = chars.len();
            let selected = match &target {
                Target::Selection(range) => {
                    let (a, b) = (range.start.min(len), range.end.min(len));
                    Some((a.min(b), a.max(b))).filter(|(start, end)| start != end)
                }
                _ => None,
            };
            let expected = match selected {
                Some((start, end)) => {
                    let mut out: Vec<char> = chars[..end].to_vec();
                    out.extend(&chars[start..end]);
                    out.extend(&chars[end..]);
                    out.into_iter().collect()
                }
                None => model(&text, &target, |lines| [lines.clone(), lines].concat()),
            };
            prop_assert_eq!(run(&text, duplicate(&text, target)), expected);
        }

        #[test]
        fn cleanups_match_naive_models((text, target) in text_and_target()) {
            let trimmed = |f: fn(&str) -> &str| {
                model(&text, &target, |lines| lines.iter().map(|l| f(l).to_owned()).collect())
            };
            prop_assert_eq!(
                run(&text, trim(&text, target.clone(), Trim::Leading)),
                trimmed(|l| l.trim_start_matches([' ', '\t']))
            );
            prop_assert_eq!(
                run(&text, trim(&text, target.clone(), Trim::Trailing)),
                trimmed(|l| l.trim_end_matches([' ', '\t']))
            );
            prop_assert_eq!(
                run(&text, trim(&text, target.clone(), Trim::Both)),
                trimmed(|l| l.trim_matches([' ', '\t']))
            );
            prop_assert_eq!(
                run(&text, remove_empty(&text, target.clone(), EmptyLines::Empty)),
                model(&text, &target, |lines| lines.into_iter().filter(|l| !l.is_empty()).collect())
            );
            prop_assert_eq!(
                run(&text, remove_empty(&text, target.clone(), EmptyLines::Blank)),
                model(&text, &target, |lines| {
                    lines.into_iter().filter(|l| !is_blank_line(l)).collect()
                })
            );
            prop_assert_eq!(
                run(&text, remove_duplicates(&text, target.clone(), Duplicates::All)),
                model(&text, &target, |lines| {
                    let mut seen = HashSet::new();
                    lines.into_iter().filter(|l| seen.insert(l.clone())).collect()
                })
            );
            prop_assert_eq!(
                run(&text, remove_duplicates(&text, target.clone(), Duplicates::Consecutive)),
                model(&text, &target, |mut lines| { lines.dedup(); lines })
            );
        }

        #[test]
        fn join_matches_a_naive_model((text, target) in text_and_target()) {
            let mut range = naive_lines(&text, &target);
            let count = lines_of(&text).len();
            let caret_mode = matches!(target, Target::Selection(_));
            if range.len() == 1 && caret_mode && range.end < count {
                range.end += 1;
            }
            let expected = if range.len() < 2 {
                text.clone()
            } else {
                let joined = naive_join(&lines_of(&text)[range.clone()]);
                splice_lines(&text, range, vec![joined])
            };
            prop_assert_eq!(run(&text, join(&text, target)), expected);
        }

        #[test]
        fn tab_conversions_keep_every_column(
            (text, target) in text_and_target(),
            width in 1usize..6,
        ) {
            let range = naive_lines(&text, &target);
            let expanded = |text: &str| {
                let mut lines = lines_of(text);
                for line in &mut lines[range.clone()] {
                    *line = naive_tabs_to_spaces(line, width);
                }
                lines.join("\n")
            };
            prop_assert_eq!(
                run(&text, tabs_to_spaces(&text, target.clone(), width, Scope::All)),
                expanded(&text)
            );
            let tabbed = run(&text, spaces_to_tabs(&text, target.clone(), width, Scope::All));
            prop_assert_eq!(expanded(&tabbed), expanded(&text));
            let leading = run(&text, spaces_to_tabs(&text, target.clone(), width, Scope::Leading));
            prop_assert_eq!(expanded(&leading), expanded(&text));
            let leading = run(&text, tabs_to_spaces(&text, target.clone(), width, Scope::Leading));
            prop_assert_eq!(expanded(&leading), expanded(&text));
        }

        #[test]
        fn sorts_are_stable_permutations(
            lines in prop::collection::vec(sort_line(), 0..12),
            descending in any::<bool>(),
        ) {
            let order = if descending { Order::Descending } else { Order::Ascending };
            let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
            for key in KEYS {
                let result = sort_lines(&refs, key, order);
                let mut a = result.clone();
                let mut b = refs.clone();
                a.sort_unstable();
                b.sort_unstable();
                prop_assert_eq!(a, b, "{:?} is not a permutation", key);
                let mut indexed: Vec<(usize, &str)> = refs.iter().copied().enumerate().collect();
                indexed.sort_by(|x, y| {
                    let ordering = key_order(key, x.1, y.1);
                    let ordering = if descending { ordering.reverse() } else { ordering };
                    ordering.then(x.0.cmp(&y.0))
                });
                let expected: Vec<&str> = indexed.into_iter().map(|(_, l)| l).collect();
                prop_assert_eq!(result, expected, "{:?} is not stable", key);
            }
        }

        #[test]
        fn sort_orders_are_total(a in sort_line(), b in sort_line(), c in sort_line()) {
            for key in KEYS {
                let ab = key_order(key, &a, &b);
                prop_assert_eq!(ab, key_order(key, &b, &a).reverse());
                if ab != Ordering::Greater && key_order(key, &b, &c) != Ordering::Greater {
                    prop_assert_ne!(key_order(key, &a, &c), Ordering::Greater, "{:?}", key);
                }
            }
        }

        #[test]
        fn sort_applies_the_sorted_lines((text, target) in text_and_target(), key_index in 0usize..6) {
            let key = KEYS[key_index];
            let expected = model(&text, &target, |lines| {
                let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
                sort_lines(&refs, key, Order::Ascending).into_iter().map(str::to_owned).collect()
            });
            prop_assert_eq!(run(&text, sort(&text, target, key, Order::Ascending)), expected);
        }

        #[test]
        fn removing_duplicates_is_idempotent(text in text()) {
            for which in [Duplicates::All, Duplicates::Consecutive] {
                let once = run(&text, remove_duplicates(&text, Target::Document, which));
                let again = remove_duplicates(&once, Target::Document, which);
                prop_assert!(again.is_empty(), "{:?} changed {:?} again", which, once);
            }
        }

        #[test]
        fn removing_empty_lines_and_joining_commute(text in text()) {
            let joined = run(&text, join(&text, Target::Document));
            for which in [EmptyLines::Empty, EmptyLines::Blank] {
                let cleaned = run(&text, remove_empty(&text, Target::Document, which));
                prop_assert_eq!(
                    run(&cleaned, join(&cleaned, Target::Document)),
                    run(&joined, remove_empty(&joined, Target::Document, which))
                );
            }
        }
    }

    #[test]
    #[ignore = "timing; run with --release -- --ignored --nocapture"]
    fn timing_on_a_million_lines() {
        use crate::ops::{case, comment, prefix};
        use std::time::Instant;
        let mut text = String::new();
        for line in 0..1_000_000u64 {
            let key = line.wrapping_mul(2_654_435_761) % 1_000_003;
            match line % 10 {
                0 => text.push('\n'),
                1 => text.push_str("\tindented line with tabs\t \n"),
                _ => text.push_str(&std::format!(
                    "item {key} value-{} dup {}  \n",
                    key % 97,
                    line % 3
                )),
            }
        }
        println!(
            "input: {:.1} MB, 1,000,000 lines",
            text.len() as f64 / 1_048_576.0
        );
        let tokens = comment::CommentTokens {
            line: Some("//".to_owned()),
            block: None,
        };
        let runs: [(&str, &dyn Fn() -> Change); 9] = [
            ("sort lexical", &|| {
                sort(&text, Target::Document, SortKey::Lexical, Order::Ascending)
            }),
            ("sort integer", &|| {
                sort(&text, Target::Document, SortKey::Integer, Order::Ascending)
            }),
            ("remove duplicates (all)", &|| {
                remove_duplicates(&text, Target::Document, Duplicates::All)
            }),
            ("remove empty lines", &|| {
                remove_empty(&text, Target::Document, EmptyLines::Blank)
            }),
            ("trim trailing", &|| {
                trim(&text, Target::Document, Trim::Trailing)
            }),
            ("tabs to spaces", &|| {
                tabs_to_spaces(&text, Target::Document, 4, Scope::All)
            }),
            ("toggle line comment", &|| {
                comment::toggle_line(&text, Target::Document, &tokens)
            }),
            ("add prefix", &|| {
                prefix::add(&text, Target::Document, "> ", "")
            }),
            ("uppercase", &|| {
                case::convert(&text, Target::Document, case::Case::Upper)
            }),
        ];
        for (label, run) in runs {
            let mut best = f64::MAX;
            let mut edits = 0;
            for _ in 0..3 {
                let start = Instant::now();
                let change = run();
                best = best.min(start.elapsed().as_secs_f64() * 1000.0);
                edits = change.edits.len();
            }
            println!("{label}: best of 3 {best:.1} ms, {edits} edits");
        }
    }
}
