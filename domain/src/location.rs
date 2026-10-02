//! Positions as users type them: `file:line:col` on the command line, and `line` or
//! `line:col` in the go-to-line prompt. Lines and columns are 1-based; columns count
//! characters.

use crate::text::Position;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineCol {
    /// 1-based.
    pub line: u32,
    /// 1-based; `None` means the start of the line.
    pub column: Option<u32>,
}

impl LineCol {
    pub const fn new(line: u32, column: Option<u32>) -> Self {
        Self { line, column }
    }

    /// The zero-based position; the caller clamps it to the text.
    pub fn position(self) -> Position {
        Position::new(
            self.line.saturating_sub(1) as usize,
            self.column.unwrap_or(1).saturating_sub(1) as usize,
        )
    }
}

fn number(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|&value| value > 0)
}

/// Parses the go-to-line prompt: `12`, `12:5`, and the same with a leading `:`.
pub fn parse_line_col(text: &str) -> Option<LineCol> {
    let text = text.trim();
    let text = text.strip_prefix(':').unwrap_or(text).trim();
    match text.split_once(':') {
        None => Some(LineCol::new(number(text)?, None)),
        Some((line, column)) => Some(LineCol::new(
            number(line.trim())?,
            Some(number(column.trim())?),
        )),
    }
}

/// Splits a `:line` or `:line:col` suffix off a command-line argument, as compilers and
/// `grep -n` print it (a trailing `:` is allowed). Returns `None` when there is no such suffix.
/// The caller uses the split only when the literal path does not exist.
pub fn split_location(arg: &str) -> Option<(&str, LineCol)> {
    let trimmed = arg.strip_suffix(':').unwrap_or(arg);
    let (head, last) = trimmed.rsplit_once(':')?;
    let last = number(last)?;
    if let Some((path, line)) = head.rsplit_once(':')
        && let Some(line) = number(line)
        && !path.is_empty()
    {
        return Some((path, LineCol::new(line, Some(last))));
    }
    (!head.is_empty()).then_some((head, LineCol::new(last, None)))
}

/// Applies `-n` and `-c` to a file that has no position of its own.
pub fn from_options(line: Option<u32>, column: Option<u32>) -> Option<LineCol> {
    match (line.filter(|&l| l > 0), column.filter(|&c| c > 0)) {
        (None, None) => None,
        (line, column) => Some(LineCol::new(line.unwrap_or(1), column)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_accepts_line_and_line_col() {
        assert_eq!(parse_line_col("12"), Some(LineCol::new(12, None)));
        assert_eq!(parse_line_col(" 12:5 "), Some(LineCol::new(12, Some(5))));
        assert_eq!(parse_line_col(":7"), Some(LineCol::new(7, None)));
        assert_eq!(parse_line_col(":7:2"), Some(LineCol::new(7, Some(2))));
        assert_eq!(parse_line_col(": 7 : 2"), Some(LineCol::new(7, Some(2))));
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
        assert_eq!(
            split_location("b.txt:40"),
            Some(("b.txt", LineCol::new(40, None)))
        );
        assert_eq!(
            split_location("src/main.rs:40:3"),
            Some(("src/main.rs", LineCol::new(40, Some(3))))
        );
        assert_eq!(
            split_location("src/main.rs:40:"),
            Some(("src/main.rs", LineCol::new(40, None)))
        );
        assert_eq!(
            split_location("a:b:40"),
            Some(("a:b", LineCol::new(40, None)))
        );
        assert_eq!(
            split_location("/tmp/x:1:2:3"),
            Some(("/tmp/x:1", LineCol::new(2, Some(3))))
        );
        assert_eq!(split_location(":3:4"), Some((":3", LineCol::new(4, None))));
        for none in [
            "b.txt", "b.txt:", "b.txt:0", ":40", "b.txt:x", "b.txt:4x", "notes",
        ] {
            assert_eq!(split_location(none), None, "{none}");
        }
    }

    #[test]
    fn options_apply_line_then_column() {
        assert_eq!(from_options(None, None), None);
        assert_eq!(from_options(Some(5), None), Some(LineCol::new(5, None)));
        assert_eq!(
            from_options(Some(5), Some(2)),
            Some(LineCol::new(5, Some(2)))
        );
        assert_eq!(from_options(None, Some(2)), Some(LineCol::new(1, Some(2))));
        assert_eq!(from_options(Some(0), Some(0)), None);
    }

    #[test]
    fn positions_are_zero_based() {
        assert_eq!(LineCol::new(1, None).position(), Position::new(0, 0));
        assert_eq!(LineCol::new(40, Some(3)).position(), Position::new(39, 2));
    }
}
