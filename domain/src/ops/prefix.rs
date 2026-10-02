//! Text around every line and incrementing numbers at a column: the MVP's stand-in for the
//! Column Editor (Alt+C), which arrives with column mode in M6.

use super::{Change, Doc, Target};
use crate::text::TextEdit;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    Decimal,
    Hex,
    Octal,
    Binary,
}

/// How numbers narrower than the widest one are filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Padding {
    /// Left-aligned and filled with spaces on the right, so the text after them stays aligned.
    None,
    /// Zeros after the sign.
    Zeros,
    /// Right-aligned with spaces.
    Spaces,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Numbering {
    pub start: i64,
    /// Added after every `repeat` lines; may be negative.
    pub step: i64,
    /// How many lines get each number; 0 counts as 1.
    pub repeat: usize,
    pub base: Base,
    /// Hex digits `A`–`F` instead of `a`–`f`.
    pub uppercase: bool,
    pub padding: Padding,
    /// The minimum width in characters, sign included. All numbers take the width of the
    /// widest one when that is more.
    pub width: usize,
}

impl Default for Numbering {
    fn default() -> Self {
        Self {
            start: 1,
            step: 1,
            repeat: 1,
            base: Base::Decimal,
            uppercase: false,
            padding: Padding::None,
            width: 0,
        }
    }
}

/// Adds `prefix` at the start and `suffix` at the end of every line of the target, empty
/// lines included.
pub fn add(text: &str, target: Target, prefix: &str, suffix: &str) -> Change {
    let doc = Doc::new(text);
    let mut edits = Vec::new();
    for line in doc.lines(&target) {
        let (start, end) = (doc.start(line), doc.end(line));
        if start == end {
            edits.push(TextEdit::insert(start, format!("{prefix}{suffix}")));
        } else {
            edits.push(TextEdit::insert(start, prefix));
            edits.push(TextEdit::insert(end, suffix));
        }
    }
    Change::new(edits, None)
}

/// Inserts one number per line of the target at character `column` (the caret's column),
/// filling shorter lines with spaces up to it.
pub fn insert_numbers(text: &str, target: Target, column: usize, numbering: &Numbering) -> Change {
    let doc = Doc::new(text);
    let lines = doc.lines(&target);
    let numbers = numbers(lines.len(), numbering);
    let mut edits = Vec::with_capacity(numbers.len());
    for (line, number) in lines.zip(numbers) {
        let (start, end) = (doc.start(line), doc.end(line));
        let edit = if start + column <= end {
            TextEdit::insert(start + column, number)
        } else {
            let fill = " ".repeat(start + column - end);
            TextEdit::insert(end, fill + &number)
        };
        edits.push(edit);
    }
    Change::new(edits, None)
}

/// The first `count` numbers of `numbering`, padded to a common width.
pub fn numbers(count: usize, numbering: &Numbering) -> Vec<String> {
    let repeat = numbering.repeat.max(1) as i128;
    let plain: Vec<String> = (0..count)
        .map(|index| {
            let value =
                i128::from(numbering.start) + index as i128 / repeat * i128::from(numbering.step);
            render(value, numbering)
        })
        .collect();
    let width = plain
        .iter()
        .map(String::len)
        .max()
        .unwrap_or(0)
        .max(numbering.width);
    plain
        .into_iter()
        .map(|number| pad(number, width, numbering.padding))
        .collect()
}

fn render(value: i128, numbering: &Numbering) -> String {
    let magnitude = value.unsigned_abs();
    let digits = match numbering.base {
        Base::Decimal => magnitude.to_string(),
        Base::Hex if numbering.uppercase => format!("{magnitude:X}"),
        Base::Hex => format!("{magnitude:x}"),
        Base::Octal => format!("{magnitude:o}"),
        Base::Binary => format!("{magnitude:b}"),
    };
    if value < 0 {
        format!("-{digits}")
    } else {
        digits
    }
}

fn pad(number: String, width: usize, padding: Padding) -> String {
    let fill = width.saturating_sub(number.len());
    match padding {
        Padding::None => number + &" ".repeat(fill),
        Padding::Spaces => " ".repeat(fill) + &number,
        Padding::Zeros => match number.strip_prefix('-') {
            Some(digits) => format!("-{}{digits}", "0".repeat(fill)),
            None => "0".repeat(fill) + &number,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::super::testing::*;
    use super::*;
    use proptest::prelude::*;

    fn numbering(start: i64, step: i64, padding: Padding) -> Numbering {
        Numbering {
            start,
            step,
            padding,
            ..Numbering::default()
        }
    }

    #[test]
    fn prefix_and_suffix_go_on_every_line_including_empty_ones() {
        let text = "a\n\nb\n";
        let change = add(text, Target::Document, "- ", ";");
        assert_eq!(applied(text, &change), "- a;\n- ;\n- b;\n");
        assert_eq!(change.selection_after(0..5), 0..14);
        assert_eq!(
            applied(
                "x\ny\nz",
                &add("x\ny\nz", Target::Selection(0..4), "<", ">")
            ),
            "<x>\n<y>\nz"
        );
    }

    #[test]
    fn numbers_share_the_width_of_the_widest() {
        let plain = |padding| numbers(11, &numbering(1, 1, padding));
        assert_eq!(plain(Padding::Zeros)[..2], ["01", "02"]);
        assert_eq!(plain(Padding::Zeros)[10], "11");
        assert_eq!(plain(Padding::Spaces)[..2], [" 1", " 2"]);
        assert_eq!(plain(Padding::None)[..2], ["1 ", "2 "]);
        let wide = Numbering {
            width: 4,
            padding: Padding::Zeros,
            ..Numbering::default()
        };
        assert_eq!(numbers(2, &wide), ["0001", "0002"]);
    }

    #[test]
    fn numbers_count_in_any_base_and_direction() {
        let hex = |uppercase| Numbering {
            start: 250,
            step: 3,
            base: Base::Hex,
            uppercase,
            ..Numbering::default()
        };
        assert_eq!(numbers(3, &hex(false)), ["fa ", "fd ", "100"]);
        assert_eq!(numbers(3, &hex(true)), ["FA ", "FD ", "100"]);
        let binary = Numbering {
            start: 0,
            base: Base::Binary,
            padding: Padding::Zeros,
            ..Numbering::default()
        };
        assert_eq!(numbers(3, &binary), ["00", "01", "10"]);
        let octal = Numbering {
            start: 7,
            base: Base::Octal,
            ..Numbering::default()
        };
        assert_eq!(numbers(2, &octal), ["7 ", "10"]);
        assert_eq!(
            numbers(4, &numbering(1, -1, Padding::Zeros)),
            ["01", "00", "-1", "-2"]
        );
        assert_eq!(
            numbers(3, &numbering(-10, 10, Padding::Spaces)),
            ["-10", "  0", " 10"]
        );
        let repeated = Numbering {
            repeat: 2,
            ..Numbering::default()
        };
        assert_eq!(numbers(5, &repeated), ["1", "1", "2", "2", "3"]);
        let extreme = numbering(i64::MAX, i64::MAX, Padding::None);
        assert_eq!(numbers(2, &extreme)[1], "18446744073709551614");
    }

    #[test]
    fn numbers_go_in_at_the_column_and_pad_short_lines() {
        let text = "abc\nx\n\nlonger";
        let change = insert_numbers(text, Target::Document, 2, &numbering(8, 1, Padding::Zeros));
        assert_eq!(applied(text, &change), "ab08c\nx 09\n  10\nlo11nger");
        let change = insert_numbers(text, Target::Lines(0..2), 0, &Numbering::default());
        assert_eq!(applied(text, &change), "1abc\n2x\n\nlonger");
    }

    proptest! {
        #[test]
        fn prefix_matches_a_naive_model((text, target) in text_and_target(), prefix in "[#> ]{0,2}", suffix in "[;, ]{0,2}") {
            let range = naive_lines(&text, &target);
            let lines: Vec<String> = text.split('\n').collect::<Vec<_>>()[range.clone()]
                .iter()
                .map(|line| format!("{prefix}{line}{suffix}"))
                .collect();
            prop_assert_eq!(
                applied(&text, &add(&text, target, &prefix, &suffix)),
                splice_lines(&text, range, lines)
            );
        }

        #[test]
        fn numbers_match_a_naive_model((text, target) in text_and_target(), column in 0usize..6, start in -20i64..20, step in -3i64..4) {
            let settings = numbering(start, step, Padding::Spaces);
            let range = naive_lines(&text, &target);
            let rendered = numbers(range.len(), &settings);
            let lines: Vec<String> = text.split('\n').collect::<Vec<_>>()[range.clone()]
                .iter()
                .zip(&rendered)
                .map(|(line, number)| {
                    let mut chars: Vec<char> = line.chars().collect();
                    while chars.len() < column {
                        chars.push(' ');
                    }
                    let tail: String = chars.split_off(column).into_iter().collect();
                    format!("{}{number}{tail}", chars.into_iter().collect::<String>())
                })
                .collect();
            prop_assert_eq!(
                applied(&text, &insert_numbers(&text, target, column, &settings)),
                splice_lines(&text, range, lines)
            );
        }
    }
}
