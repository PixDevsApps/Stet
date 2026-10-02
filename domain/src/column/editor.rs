//! The Column Editor (Alt+C): text or a number sequence on every line of the rectangle.

use super::editing::{ColumnEdit, ColumnError, edit_lines, push_spaces, single_line, splice};
use super::rect::{Rect, clamp_lines};
use super::visual::text_width;
use crate::text::LineIndex;

/// What the Column Editor writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnEditor {
    /// The same text on every line.
    Text(String),
    /// One number per line, from the top.
    Numbers(NumberSequence),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Base {
    #[default]
    Decimal,
    Hex,
    Octal,
    Binary,
}

/// How a number narrower than the sequence's width is padded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Leading {
    /// Left-aligned, followed by spaces where the line goes on, so its text stays aligned.
    #[default]
    None,
    /// Right-aligned, with zeros after the sign.
    Zeros,
    /// Right-aligned, with spaces.
    Spaces,
}

/// Numbers from `start`, adding `step` every `repeat` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberSequence {
    pub start: i64,
    pub step: i64,
    /// How many lines get each number; 0 counts as 1.
    pub repeat: usize,
    pub base: Base,
    pub leading: Leading,
    /// The minimum width in cells, sign included. Every number is as wide as the sequence's
    /// widest one, and at least this wide.
    pub width: usize,
    /// Hexadecimal digits A–F rather than a–f.
    pub uppercase: bool,
}

impl Default for NumberSequence {
    fn default() -> Self {
        Self {
            start: 1,
            step: 1,
            repeat: 1,
            base: Base::Decimal,
            leading: Leading::None,
            width: 0,
            uppercase: true,
        }
    }
}

impl NumberSequence {
    /// The numbers of the first `count` lines.
    pub fn numbers(&self, count: usize) -> Result<Vec<i64>, ColumnError> {
        let repeat = self.repeat.max(1);
        (0..count)
            .map(|line| {
                i64::try_from(line / repeat)
                    .ok()
                    .and_then(|steps| self.step.checked_mul(steps))
                    .and_then(|offset| self.start.checked_add(offset))
                    .ok_or(ColumnError::Overflow)
            })
            .collect()
    }

    /// The numbers of the first `count` lines as text, all equally wide, trailing spaces
    /// included for [`Leading::None`].
    pub fn rows(&self, count: usize) -> Result<Vec<String>, ColumnError> {
        let numbers: Vec<(&str, String)> = self
            .numbers(count)?
            .into_iter()
            .map(|number| {
                let sign = if number < 0 { "-" } else { "" };
                (sign, self.digits(number.unsigned_abs()))
            })
            .collect();
        let width = numbers
            .iter()
            .map(|(sign, digits)| sign.len() + digits.len())
            .fold(self.width, usize::max);
        Ok(numbers
            .into_iter()
            .map(|(sign, digits)| {
                let pad = width - sign.len() - digits.len();
                let mut row = String::with_capacity(width);
                match self.leading {
                    Leading::None => {
                        row.push_str(sign);
                        row.push_str(&digits);
                        push_spaces(&mut row, pad);
                    }
                    Leading::Zeros => {
                        row.push_str(sign);
                        row.extend(std::iter::repeat_n('0', pad));
                        row.push_str(&digits);
                    }
                    Leading::Spaces => {
                        push_spaces(&mut row, pad);
                        row.push_str(sign);
                        row.push_str(&digits);
                    }
                }
                row
            })
            .collect())
    }

    fn digits(&self, magnitude: u64) -> String {
        match self.base {
            Base::Decimal => magnitude.to_string(),
            Base::Hex if self.uppercase => format!("{magnitude:X}"),
            Base::Hex => format!("{magnitude:x}"),
            Base::Octal => format!("{magnitude:o}"),
            Base::Binary => format!("{magnitude:b}"),
        }
    }
}

impl ColumnEditor {
    /// Writes the text or the numbers at the rectangle's left column on each of its lines, from
    /// the top: in place of the block's cells when it has any, and inserted at a column caret.
    /// Short lines are padded. The result selects the written column.
    pub fn apply(
        &self,
        text: &str,
        index: &LineIndex,
        rect: Rect,
        tab_width: usize,
    ) -> Result<ColumnEdit, ColumnError> {
        let (left, right) = (rect.left(), rect.right());
        let (edits, width) = match self {
            Self::Text(value) => {
                single_line(value)?;
                let edits = edit_lines(text, index, rect.lines(), |_, line| {
                    splice(line, left, right, value, 0, tab_width)
                });
                (edits, text_width(value, left, tab_width))
            }
            Self::Numbers(sequence) => {
                let lines = clamp_lines(index, rect.lines());
                let rows = sequence.rows(lines.len())?;
                let width = rows.first().map_or(0, String::len);
                let edits = edit_lines(text, index, lines.clone(), |line, line_text| {
                    let row = rows[line - lines.start].as_str();
                    match sequence.leading {
                        Leading::None => splice(
                            line_text,
                            left,
                            right,
                            row.trim_end_matches(' '),
                            width,
                            tab_width,
                        ),
                        Leading::Zeros | Leading::Spaces => {
                            splice(line_text, left, right, row, 0, tab_width)
                        }
                    }
                });
                (edits, width)
            }
        };
        let rect = Rect::new((rect.anchor.line, left), (rect.cursor.line, left + width));
        Ok(ColumnEdit { edits, rect })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::apply;
    use proptest::prelude::*;

    fn sequence(start: i64, step: i64) -> NumberSequence {
        NumberSequence {
            start,
            step,
            ..NumberSequence::default()
        }
    }

    fn rows(sequence: NumberSequence, count: usize) -> Vec<String> {
        sequence.rows(count).unwrap()
    }

    #[test]
    fn pads_to_a_width_with_zeros() {
        let zeros = NumberSequence {
            width: 3,
            leading: Leading::Zeros,
            ..sequence(1, 2)
        };
        assert_eq!(rows(zeros, 3), ["001", "003", "005"]);
    }

    #[test]
    fn every_number_is_as_wide_as_the_widest() {
        assert_eq!(rows(sequence(8, 1), 3), ["8 ", "9 ", "10"]);
        let spaces = NumberSequence {
            leading: Leading::Spaces,
            ..sequence(8, 1)
        };
        assert_eq!(rows(spaces, 3), [" 8", " 9", "10"]);
        let down = NumberSequence {
            leading: Leading::Zeros,
            ..sequence(1, -1)
        };
        assert_eq!(rows(down, 3), ["01", "00", "-1"]);
    }

    #[test]
    fn formats_hex_octal_and_binary() {
        let hex = NumberSequence {
            base: Base::Hex,
            ..sequence(0xfe, 1)
        };
        assert_eq!(rows(hex, 3), ["FE ", "FF ", "100"]);
        let lower = NumberSequence {
            uppercase: false,
            leading: Leading::Zeros,
            ..hex
        };
        assert_eq!(rows(lower, 2), ["fe", "ff"]);
        let octal = NumberSequence {
            base: Base::Octal,
            ..sequence(7, 1)
        };
        assert_eq!(rows(octal, 2), ["7 ", "10"]);
        let binary = NumberSequence {
            base: Base::Binary,
            leading: Leading::Zeros,
            width: 4,
            ..sequence(1, 1)
        };
        assert_eq!(rows(binary, 3), ["0001", "0010", "0011"]);
    }

    #[test]
    fn repeats_each_number() {
        let repeated = NumberSequence {
            repeat: 2,
            ..sequence(1, 2)
        };
        assert_eq!(rows(repeated, 5), ["1", "1", "3", "3", "5"]);
        let zero = NumberSequence {
            repeat: 0,
            ..sequence(1, 1)
        };
        assert_eq!(rows(zero, 2), ["1", "2"]);
    }

    #[test]
    fn reports_overflow() {
        assert_eq!(sequence(i64::MAX, 1).rows(2), Err(ColumnError::Overflow));
        assert_eq!(rows(sequence(i64::MIN, 0), 1), [i64::MIN.to_string()]);
    }

    fn run(text: &str, rect: Rect, editor: &ColumnEditor) -> (String, Rect) {
        let index = LineIndex::new(text);
        let edit = editor.apply(text, &index, rect, 4).unwrap();
        (apply(text, edit.edits).unwrap(), edit.rect)
    }

    #[test]
    fn writes_numbers_down_a_column_caret() {
        let editor = ColumnEditor::Numbers(sequence(9, 1));
        let (result, rect) = run("a = x;\nbb = y;\n\nc", Rect::new((3, 1), (0, 1)), &editor);
        assert_eq!(result, "a9  = x;\nb10b = y;\n 11\nc12");
        assert_eq!(rect, Rect::new((3, 1), (0, 3)));
    }

    #[test]
    fn text_replaces_the_block() {
        let editor = ColumnEditor::Text("<>".to_owned());
        let (result, rect) = run("0123456\n0123456", Rect::new((0, 1), (1, 4)), &editor);
        assert_eq!(result, "0<>456\n0<>456");
        assert_eq!(rect, Rect::new((0, 1), (1, 3)));
        let multiline = ColumnEditor::Text("a\nb".to_owned());
        let index = LineIndex::new("x");
        assert_eq!(
            multiline.apply("x", &index, Rect::new((0, 0), (0, 0)), 4),
            Err(ColumnError::LineBreak)
        );
    }

    proptest! {
        #[test]
        fn rows_are_equally_wide_and_parse_back(
            start in -5000i64..5000,
            step in -300i64..300,
            repeat in 0usize..4,
            width in 0usize..8,
            base in prop::sample::select(vec![Base::Decimal, Base::Hex, Base::Octal, Base::Binary]),
            leading in prop::sample::select(vec![Leading::None, Leading::Zeros, Leading::Spaces]),
            uppercase: bool,
            count in 0usize..40,
        ) {
            let sequence = NumberSequence { start, step, repeat, base, leading, width, uppercase };
            let rows = sequence.rows(count).unwrap();
            let radix = match base { Base::Decimal => 10, Base::Hex => 16, Base::Octal => 8, Base::Binary => 2 };
            for (line, row) in rows.iter().enumerate() {
                prop_assert_eq!(row.len(), rows[0].len());
                prop_assert!(row.len() >= width);
                let trimmed = row.trim();
                let (negative, digits) = match trimmed.strip_prefix('-') {
                    Some(digits) => (true, digits),
                    None => (false, trimmed),
                };
                let magnitude = i64::from_str_radix(digits, radix).unwrap();
                let expected = start + step * (line / repeat.max(1)) as i64;
                prop_assert_eq!(if negative { -magnitude } else { magnitude }, expected);
                prop_assert_eq!(digits.chars().any(|c| c.is_ascii_uppercase()), uppercase && base == Base::Hex && digits.chars().any(|c| c.is_ascii_alphabetic()));
            }
        }
    }
}
