//! Number sequences, one per line: Insert Numbers and the Column Editor (Alt+C) write the same
//! numbers in the same formats, and refuse the same sequences.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Base {
    #[default]
    Decimal,
    Hex,
    Octal,
    Binary,
}

/// How a number narrower than the sequence's width is filled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Padding {
    /// Left-aligned and filled with spaces on the right, so the text after it stays aligned.
    #[default]
    None,
    /// Right-aligned, with zeros after the sign.
    Zeros,
    /// Right-aligned, with spaces.
    Spaces,
}

/// A number of the sequence does not fit in an `i64`. The sequence is refused as a whole: no
/// number is wrapped, clamped or written wider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberOverflow;

impl fmt::Display for NumberOverflow {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("the number sequence does not fit in 64 bits")
    }
}

impl std::error::Error for NumberOverflow {}

/// Numbers from `start`, adding `step` after every `repeat` lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NumberSequence {
    pub start: i64,
    /// May be negative.
    pub step: i64,
    /// How many lines get each number; 0 counts as 1.
    pub repeat: usize,
    pub base: Base,
    /// Hexadecimal digits A–F rather than a–f.
    pub uppercase: bool,
    pub padding: Padding,
    /// The minimum width in characters, sign included. Every number is as wide as the
    /// sequence's widest one, and at least this wide.
    pub width: usize,
}

impl Default for NumberSequence {
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

impl NumberSequence {
    /// The number of line `line` (from 0), or [`NumberOverflow`] when it does not fit in an
    /// `i64`. Computed exactly in `i128`, where `start + line * step` cannot overflow.
    fn value(&self, line: usize) -> Result<i64, NumberOverflow> {
        let steps = i128::try_from(line / self.repeat.max(1)).map_err(|_| NumberOverflow)?;
        i128::from(self.step)
            .checked_mul(steps)
            .and_then(|offset| offset.checked_add(i128::from(self.start)))
            .and_then(|value| i64::try_from(value).ok())
            .ok_or(NumberOverflow)
    }

    /// Whether every number of the first `count` lines fits, without making them. The sequence
    /// only rises or only falls, so its last number is the one furthest from `start`.
    pub fn check(&self, count: usize) -> Result<(), NumberOverflow> {
        match count.checked_sub(1) {
            Some(last) => self.value(last).map(drop),
            None => Ok(()),
        }
    }

    /// The numbers of the first `count` lines as text, all equally wide, trailing spaces
    /// included for [`Padding::None`].
    pub fn rows(&self, count: usize) -> Result<Vec<String>, NumberOverflow> {
        self.check(count)?;
        let plain = (0..count)
            .map(|line| self.value(line).map(|number| self.render(number)))
            .collect::<Result<Vec<_>, _>>()?;
        let width = plain.iter().map(String::len).fold(self.width, usize::max);
        Ok(plain
            .into_iter()
            .map(|number| self.pad(number, width))
            .collect())
    }

    fn render(&self, number: i64) -> String {
        let magnitude = number.unsigned_abs();
        let digits = match self.base {
            Base::Decimal => magnitude.to_string(),
            Base::Hex if self.uppercase => format!("{magnitude:X}"),
            Base::Hex => format!("{magnitude:x}"),
            Base::Octal => format!("{magnitude:o}"),
            Base::Binary => format!("{magnitude:b}"),
        };
        if number < 0 {
            format!("-{digits}")
        } else {
            digits
        }
    }

    fn pad(&self, number: String, width: usize) -> String {
        let fill = width.saturating_sub(number.len());
        match self.padding {
            Padding::None => number + &" ".repeat(fill),
            Padding::Spaces => " ".repeat(fill) + &number,
            Padding::Zeros => match number.strip_prefix('-') {
                Some(digits) => format!("-{}{digits}", "0".repeat(fill)),
                None => "0".repeat(fill) + &number,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn sequence(start: i64, step: i64) -> NumberSequence {
        NumberSequence {
            start,
            step,
            ..NumberSequence::default()
        }
    }

    #[test]
    fn a_sequence_that_leaves_64_bits_is_refused_whole() {
        assert_eq!(
            sequence(i64::MAX, 1).rows(1).unwrap(),
            [i64::MAX.to_string()]
        );
        assert_eq!(sequence(i64::MAX, 1).rows(2), Err(NumberOverflow));
        assert_eq!(sequence(i64::MIN, -1).rows(2), Err(NumberOverflow));
        assert_eq!(
            sequence(i64::MIN, 0).rows(3).unwrap()[2],
            i64::MIN.to_string()
        );
        assert_eq!(sequence(i64::MAX, i64::MAX).check(2), Err(NumberOverflow));
        let repeated = NumberSequence {
            repeat: 3,
            ..sequence(i64::MAX - 1, 1)
        };
        assert_eq!(repeated.check(6), Ok(()));
        assert_eq!(repeated.check(7), Err(NumberOverflow));
        assert_eq!(sequence(i64::MAX, 1).check(0), Ok(()));
        assert_eq!(sequence(0, i64::MAX).check(usize::MAX), Err(NumberOverflow));
    }

    #[test]
    fn only_the_numbers_themselves_must_fit() {
        // 3 × (−2⁶²) is below i64::MIN, but i64::MAX plus it is not.
        let falling = sequence(i64::MAX, -(1 << 62));
        assert_eq!(
            falling.rows(4).unwrap()[3],
            (i128::from(i64::MAX) - 3 * (1 << 62)).to_string()
        );
        assert_eq!(falling.check(5), Err(NumberOverflow));
    }

    fn radix(base: Base, uppercase: bool, magnitude: u64, width: usize) -> String {
        match (base, uppercase) {
            (Base::Decimal, _) => format!("{magnitude:0width$}"),
            (Base::Hex, false) => format!("{magnitude:0width$x}"),
            (Base::Hex, true) => format!("{magnitude:0width$X}"),
            (Base::Octal, _) => format!("{magnitude:0width$o}"),
            (Base::Binary, _) => format!("{magnitude:0width$b}"),
        }
    }

    proptest! {
        #[test]
        fn a_number_renders_as_format_does(
            number: i64,
            width in 0usize..72,
            base in prop::sample::select(vec![Base::Decimal, Base::Hex, Base::Octal, Base::Binary]),
            padding in prop::sample::select(vec![Padding::None, Padding::Zeros, Padding::Spaces]),
            uppercase: bool,
        ) {
            let one = NumberSequence { base, uppercase, padding, width, ..sequence(number, 0) };
            let sign = if number < 0 { "-" } else { "" };
            let plain = format!("{sign}{}", radix(base, uppercase, number.unsigned_abs(), 0));
            let expected = match padding {
                Padding::None => format!("{plain:<width$}"),
                Padding::Spaces => format!("{plain:>width$}"),
                Padding::Zeros => format!(
                    "{sign}{}",
                    radix(base, uppercase, number.unsigned_abs(), width.saturating_sub(sign.len()))
                ),
            };
            if base == Base::Decimal && padding == Padding::Zeros {
                prop_assert_eq!(&expected, &format!("{number:0width$}"));
            }
            prop_assert_eq!(one.rows(1).unwrap(), [expected]);
        }

        #[test]
        fn overflow_is_refused_exactly_when_a_number_leaves_i64(
            start in prop_oneof![Just(i64::MAX), Just(i64::MIN), -1000i64..1000, any::<i64>()],
            step in prop_oneof![Just(i64::MAX), Just(i64::MIN), -1000i64..1000, any::<i64>()],
            repeat in 0usize..4,
            count in 0usize..6,
        ) {
            let numbers = NumberSequence { repeat, ..sequence(start, step) };
            let exact: Vec<i128> = (0..count)
                .map(|line| i128::from(start) + (line / repeat.max(1)) as i128 * i128::from(step))
                .collect();
            let fits = exact.iter().all(|&value| i64::try_from(value).is_ok());
            prop_assert_eq!(numbers.check(count).is_ok(), fits);
            match numbers.rows(count) {
                Ok(rows) => {
                    prop_assert!(fits);
                    for (row, value) in rows.iter().zip(&exact) {
                        prop_assert_eq!(row.trim_end(), value.to_string());
                    }
                }
                Err(NumberOverflow) => prop_assert!(!fits),
            }
        }
    }
}
