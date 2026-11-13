//! Text around every line and incrementing numbers at a column: a simpler form of the Column
//! Editor (Alt+C).

use super::numbers::{NumberOverflow, NumberSequence};
use super::{Change, Doc, Target};
use crate::text::TextEdit;

/// Adds `prefix` at the start and `suffix` at the end of every line of the target, empty
/// lines included.
pub fn add(text: &str, target: &Target, prefix: &str, suffix: &str) -> Change {
    let doc = Doc::new(text);
    let mut edits = Vec::new();
    for line in doc.lines(target) {
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
pub fn insert_numbers(
    text: &str,
    target: &Target,
    column: usize,
    sequence: &NumberSequence,
) -> Result<Change, NumberOverflow> {
    let doc = Doc::new(text);
    let lines = doc.lines(target);
    let numbers = sequence.rows(lines.len())?;
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
    Ok(Change::new(edits, None))
}

#[cfg(test)]
mod tests {
    use super::super::numbers::{Base, Padding};
    use super::super::testing::*;
    use super::*;
    use proptest::prelude::*;

    fn numbering(start: i64, step: i64, padding: Padding) -> NumberSequence {
        NumberSequence {
            start,
            step,
            padding,
            ..NumberSequence::default()
        }
    }

    fn numbers(count: usize, sequence: &NumberSequence) -> Vec<String> {
        sequence.rows(count).unwrap()
    }

    #[test]
    fn prefix_and_suffix_go_on_every_line_including_empty_ones() {
        let text = "a\n\nb\n";
        let change = add(text, &Target::Document, "- ", ";");
        assert_eq!(applied(text, &change), "- a;\n- ;\n- b;\n");
        assert_eq!(change.selection_after(chars(0..5)), chars(0..14));
        assert_eq!(
            applied(
                "x\ny\nz",
                &add("x\ny\nz", &Target::Selection(chars(0..4)), "<", ">")
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
        let wide = NumberSequence {
            width: 4,
            padding: Padding::Zeros,
            ..NumberSequence::default()
        };
        assert_eq!(numbers(2, &wide), ["0001", "0002"]);
    }

    #[test]
    fn numbers_count_in_any_base_and_direction() {
        let hex = |uppercase| NumberSequence {
            start: 250,
            step: 3,
            base: Base::Hex,
            uppercase,
            ..NumberSequence::default()
        };
        assert_eq!(numbers(3, &hex(false)), ["fa ", "fd ", "100"]);
        assert_eq!(numbers(3, &hex(true)), ["FA ", "FD ", "100"]);
        let binary = NumberSequence {
            start: 0,
            base: Base::Binary,
            padding: Padding::Zeros,
            ..NumberSequence::default()
        };
        assert_eq!(numbers(3, &binary), ["00", "01", "10"]);
        let octal = NumberSequence {
            start: 7,
            base: Base::Octal,
            ..NumberSequence::default()
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
        let repeated = NumberSequence {
            repeat: 2,
            ..NumberSequence::default()
        };
        assert_eq!(numbers(5, &repeated), ["1", "1", "2", "2", "3"]);
    }

    #[test]
    fn numbers_go_in_at_the_column_and_pad_short_lines() {
        let text = "abc\nx\n\nlonger";
        let change =
            insert_numbers(text, &Target::Document, 2, &numbering(8, 1, Padding::Zeros)).unwrap();
        assert_eq!(applied(text, &change), "ab08c\nx 09\n  10\nlo11nger");
        let change =
            insert_numbers(text, &Target::Lines(0..2), 0, &NumberSequence::default()).unwrap();
        assert_eq!(applied(text, &change), "1abc\n2x\n\nlonger");
    }

    #[test]
    fn numbers_past_64_bits_insert_nothing() {
        let text = "a\nb\nc";
        let extreme = numbering(i64::MAX, i64::MAX, Padding::None);
        assert_eq!(
            insert_numbers(text, &Target::Document, 0, &extreme),
            Err(NumberOverflow)
        );
        assert_eq!(
            insert_numbers(text, &Target::Lines(0..1), 0, &extreme)
                .map(|change| applied(text, &change)),
            Ok(format!("{}a\nb\nc", i64::MAX))
        );
        let falling = numbering(i64::MIN + 1, -1, Padding::None);
        assert!(insert_numbers(text, &Target::Lines(1..3), 0, &falling).is_ok());
        assert_eq!(
            insert_numbers(text, &Target::Document, 0, &falling),
            Err(NumberOverflow)
        );
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
                applied(&text, &add(&text, &target, &prefix, &suffix)),
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
                applied(&text, &insert_numbers(&text, &target, column, &settings).unwrap()),
                splice_lines(&text, range, lines)
            );
        }
    }
}
