//! Positions as users type them: `file:line:col` on the command line, and `line` or
//! `line:col` in the go-to-line prompt. Columns count characters.

use std::num::{NonZeroU32, NonZeroUsize};

use crate::text::{LineNumber, Position};

/// A line and an optional column, counted from 1, as the user typed them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// The line.
    pub line: LineNumber,
    /// `None` means the start of the line.
    pub column: Option<NonZeroUsize>,
}

impl LineCol {
    /// Line `line`, at `column` or the start of the line.
    #[must_use]
    pub const fn new(line: LineNumber, column: Option<NonZeroUsize>) -> Self {
        Self { line, column }
    }

    /// Line `line`, column `column`, both counted from 1.
    #[cfg(test)]
    pub(crate) fn at(line: usize, column: Option<usize>) -> Self {
        Self::new(
            LineNumber::new(line).unwrap(),
            column.map(|column| NonZeroUsize::new(column).unwrap()),
        )
    }

    /// The position in the text; the caller clamps it to the text.
    #[must_use]
    pub fn position(self) -> Position {
        Position::new(
            self.line.index(),
            self.column.map_or(0, |column| column.get() - 1),
        )
    }
}

/// The position as people count lines and columns.
impl From<Position> for LineCol {
    fn from(position: Position) -> Self {
        Self::new(position.line.number(), Some(position.column_number()))
    }
}

/// A line or column number as typed: digits only, from 1 to `u32::MAX`.
fn number(text: &str) -> Option<NonZeroUsize> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<NonZeroU32>().ok().map(widen)
}

/// `number` as a `usize`, which holds every `u32` on the targets Stet builds for.
fn widen(number: NonZeroU32) -> NonZeroUsize {
    NonZeroUsize::try_from(number).unwrap_or(NonZeroUsize::MAX)
}

fn line_number(text: &str) -> Option<LineNumber> {
    number(text).map(LineNumber::from)
}

/// Parses the go-to-line prompt: `12`, `12:5`, and the same with a leading `:`.
#[must_use]
pub fn parse_line_col(text: &str) -> Option<LineCol> {
    let text = text.trim();
    let text = text.strip_prefix(':').unwrap_or(text).trim();
    match text.split_once(':') {
        None => Some(LineCol::new(line_number(text)?, None)),
        Some((line, column)) => Some(LineCol::new(
            line_number(line.trim())?,
            Some(number(column.trim())?),
        )),
    }
}

/// Splits a `:line` or `:line:col` suffix off a command-line argument, as compilers and
/// `grep -n` print it (a trailing `:` is allowed). Returns `None` when there is no such suffix.
/// The caller uses the split only when the literal path does not exist.
#[must_use]
pub fn split_location(arg: &str) -> Option<(&str, LineCol)> {
    let trimmed = arg.strip_suffix(':').unwrap_or(arg);
    let (head, last) = trimmed.rsplit_once(':')?;
    let last = number(last)?;
    if let Some((path, line)) = head.rsplit_once(':')
        && let Some(line) = line_number(line)
        && !path.is_empty()
    {
        return Some((path, LineCol::new(line, Some(last))));
    }
    (!head.is_empty()).then_some((head, LineCol::new(last.into(), None)))
}

/// Applies `-n` and `-c` to a file that has no position of its own; 0 counts as not given.
pub fn from_options(line: Option<u32>, column: Option<u32>) -> Option<LineCol> {
    let given = |value: Option<u32>| value.and_then(NonZeroU32::new).map(widen);
    match (given(line), given(column)) {
        (None, None) => None,
        (line, column) => Some(LineCol::new(
            line.map_or(LineNumber::FIRST, LineNumber::from),
            column,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Line;

    fn at(line: usize, column: Option<usize>) -> LineCol {
        LineCol::at(line, column)
    }

    #[test]
    fn prompt_accepts_line_and_line_col() {
        assert_eq!(parse_line_col("12"), Some(at(12, None)));
        assert_eq!(parse_line_col(" 12:5 "), Some(at(12, Some(5))));
        assert_eq!(parse_line_col(":7"), Some(at(7, None)));
        assert_eq!(parse_line_col(":7:2"), Some(at(7, Some(2))));
        assert_eq!(parse_line_col(": 7 : 2"), Some(at(7, Some(2))));
        for bad in [
            "",
            ":",
            "0",
            "x",
            "12:",
            "12:0",
            "1:2:3",
            "-4",
            "+4",
            "99999999999",
        ] {
            assert_eq!(parse_line_col(bad), None, "{bad}");
        }
    }

    #[test]
    fn splits_compiler_style_suffixes() {
        assert_eq!(split_location("b.txt:40"), Some(("b.txt", at(40, None))));
        assert_eq!(
            split_location("src/main.rs:40:3"),
            Some(("src/main.rs", at(40, Some(3))))
        );
        assert_eq!(
            split_location("src/main.rs:40:"),
            Some(("src/main.rs", at(40, None)))
        );
        assert_eq!(split_location("a:b:40"), Some(("a:b", at(40, None))));
        assert_eq!(
            split_location("/tmp/x:1:2:3"),
            Some(("/tmp/x:1", at(2, Some(3))))
        );
        assert_eq!(split_location(":3:4"), Some((":3", at(4, None))));
        for none in [
            "b.txt", "b.txt:", "b.txt:0", ":40", "b.txt:x", "b.txt:4x", "notes",
        ] {
            assert_eq!(split_location(none), None, "{none}");
        }
    }

    #[test]
    fn options_apply_line_then_column() {
        assert_eq!(from_options(None, None), None);
        assert_eq!(from_options(Some(5), None), Some(at(5, None)));
        assert_eq!(from_options(Some(5), Some(2)), Some(at(5, Some(2))));
        assert_eq!(from_options(None, Some(2)), Some(at(1, Some(2))));
        assert_eq!(from_options(Some(0), Some(0)), None);
    }

    #[test]
    fn positions_are_zero_based() {
        assert_eq!(at(1, None).position(), Position::new(Line::ZERO, 0));
        assert_eq!(at(40, Some(3)).position(), Position::new(Line::new(39), 2));
    }
}
