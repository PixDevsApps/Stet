//! Offsets into a text, one type per unit, so a byte offset never passes for a character
//! offset and a line index never passes for a line number. GtkTextIter,
//! [`TextEdit`](super::TextEdit) and every position the user sees count characters (Unicode
//! scalar values); slicing a `&str`, PCRE2 and file I/O count UTF-8 bytes. Lines are stored and
//! computed as a [`Line`] counted from 0 and shown as a [`LineNumber`] counted from 1. Column
//! mode counts [`VisualColumn`]s, where a tab takes the cells to its tab stop.
//!
//! An offset plus a count of units is an offset; an offset minus an offset is a count. Two
//! offsets don't add, and a bare `usize` becomes an offset only through `new`, where the unit
//! is named.

use std::fmt;
use std::iter::Map;
use std::num::NonZeroUsize;
use std::ops::{Add, AddAssign, Range, Sub, SubAssign};

use serde::{Deserialize, Serialize};

macro_rules! offset {
    ($(#[$doc:meta])* $name:ident, $unit:literal, $zero:literal, $origin:literal) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        #[repr(transparent)]
        pub struct $name(usize);

        impl $name {
            #[doc = concat!($zero, ": ", $unit, " 0.")]
            pub const ZERO: Self = Self(0);

            #[doc = concat!("`offset` ", $unit, "s from ", $origin, ".")]
            #[must_use]
            pub const fn new(offset: usize) -> Self {
                Self(offset)
            }

            #[doc = concat!("The number of ", $unit, "s from ", $origin, ".")]
            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }

            #[doc = concat!("`count` ", $unit, "s later, stopping at the last one `usize` can count.")]
            #[must_use]
            pub const fn saturating_add(self, count: usize) -> Self {
                Self(self.0.saturating_add(count))
            }

            #[doc = concat!("`count` ", $unit, "s earlier, or `None` before ", $origin, ".")]
            #[must_use]
            pub const fn checked_sub(self, count: usize) -> Option<Self> {
                match self.0.checked_sub(count) {
                    Some(offset) => Some(Self(offset)),
                    None => None,
                }
            }

            #[doc = concat!("`count` ", $unit, "s earlier, stopping at ", $origin, ".")]
            #[must_use]
            pub const fn saturating_sub(self, count: usize) -> Self {
                Self(self.0.saturating_sub(count))
            }

            #[doc = concat!("The number of ", $unit, "s between `self` and `other`, in either order.")]
            #[must_use]
            pub const fn abs_diff(self, other: Self) -> usize {
                self.0.abs_diff(other.0)
            }

            #[doc = concat!("`delta` ", $unit, "s later, or earlier when negative, stopping at ", $origin, ".")]
            #[must_use]
            pub const fn saturating_add_signed(self, delta: isize) -> Self {
                Self(self.0.saturating_add_signed(delta))
            }
        }

        #[doc = concat!("`count` ", $unit, "s later.")]
        impl Add<usize> for $name {
            type Output = Self;

            fn add(self, count: usize) -> Self {
                Self(self.0 + count)
            }
        }

        impl AddAssign<usize> for $name {
            fn add_assign(&mut self, count: usize) {
                self.0 += count;
            }
        }

        #[doc = concat!("`count` ", $unit, "s earlier; panics in debug builds before ", $origin, ".")]
        impl Sub<usize> for $name {
            type Output = Self;

            fn sub(self, count: usize) -> Self {
                Self(self.0 - count)
            }
        }

        impl SubAssign<usize> for $name {
            fn sub_assign(&mut self, count: usize) {
                self.0 -= count;
            }
        }

        #[doc = concat!("The number of ", $unit, "s from `earlier` to `self`.")]
        impl Sub for $name {
            type Output = usize;

            fn sub(self, earlier: Self) -> usize {
                self.0 - earlier.0
            }
        }

        impl OffsetRange for Range<$name> {
            type Offset = $name;
            type Iter = Map<Range<usize>, fn(usize) -> $name>;

            fn get(&self) -> Range<usize> {
                self.start.0..self.end.0
            }

            fn iter(&self) -> Self::Iter {
                self.get().map($name)
            }
        }
    };
}

offset!(
    /// An offset in characters (Unicode scalar values): the unit of GtkTextIter,
    /// [`TextEdit`](super::TextEdit) and the columns the user sees.
    CharOffset,
    "character",
    "The start of the text",
    "the start of the text"
);

offset!(
    /// An offset in UTF-8 bytes: the unit of `&str` slicing, PCRE2 and the bytes on disk. On
    /// valid text it falls on a character boundary.
    ByteOffset,
    "byte",
    "The start of the text",
    "the start of the text"
);

offset!(
    /// A line of a text, counted from 0: how lines are stored and computed with. A line ends
    /// after its `\n`, so a text ending in `\n` has an empty last line. People count lines from
    /// 1, as a [`LineNumber`].
    Line,
    "line",
    "The first line",
    "the first line"
);

offset!(
    /// A visual column: the cells from the start of a line, where a tab advances to the next
    /// multiple of the tab width and every other character takes one cell. Column mode
    /// (ADR-016) and `gtk_source_view_get_visual_column` count in it; a column in characters is
    /// a count from the line's first [`CharOffset`].
    VisualColumn,
    "cell",
    "The start of the line",
    "the start of the line"
);

impl Line {
    /// The line as people count it, from 1.
    #[must_use]
    pub const fn number(self) -> LineNumber {
        LineNumber(NonZeroUsize::MIN.saturating_add(self.0))
    }
}

/// A line as people count lines, from 1: in the status bar, `file:line:col`, error messages,
/// result lists and the self-test scripts. Code that stores or computes with lines uses
/// [`Line`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LineNumber(NonZeroUsize);

impl LineNumber {
    /// Line 1.
    pub const FIRST: Self = Self(NonZeroUsize::MIN);

    /// Line `number`, or `None` for 0.
    #[must_use]
    pub const fn new(number: usize) -> Option<Self> {
        match NonZeroUsize::new(number) {
            Some(number) => Some(Self(number)),
            None => None,
        }
    }

    /// The number as shown, at least 1.
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }

    /// The line this number names.
    #[must_use]
    pub const fn index(self) -> Line {
        Line(self.0.get() - 1)
    }
}

impl From<Line> for LineNumber {
    fn from(line: Line) -> Self {
        line.number()
    }
}

impl From<LineNumber> for Line {
    fn from(number: LineNumber) -> Self {
        number.index()
    }
}

impl From<NonZeroUsize> for LineNumber {
    fn from(number: NonZeroUsize) -> Self {
        Self(number)
    }
}

impl fmt::Display for LineNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// A range of offsets in one unit. Its length is `range.end - range.start`, a count.
pub trait OffsetRange {
    /// The unit of the range's ends.
    type Offset;
    /// The iterator [`OffsetRange::iter`] returns.
    type Iter: DoubleEndedIterator<Item = Self::Offset> + ExactSizeIterator;

    /// The range in plain numbers, for slicing and for APIs that count in the same unit.
    fn get(&self) -> Range<usize>;

    /// Each offset in the range, from its start.
    fn iter(&self) -> Self::Iter;
}

/// A point known in both units: byte `byte` of a text is its character `char`. Conversions
/// start counting there instead of at the start of the text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Anchor {
    /// The point as a byte offset.
    pub byte: ByteOffset,
    /// The same point as a character offset.
    pub char: CharOffset,
}

impl Anchor {
    /// The start of every text.
    pub const START: Self = Self {
        byte: ByteOffset::ZERO,
        char: CharOffset::ZERO,
    };

    /// The point that is byte `byte` and character `char` of a text.
    #[must_use]
    pub const fn new(byte: ByteOffset, char: CharOffset) -> Self {
        Self { byte, char }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn counts_add_to_offsets_and_offsets_subtract_to_counts() {
        let mut at = CharOffset::new(3) + 4;
        assert_eq!(at, CharOffset::new(7));
        at += 2;
        assert_eq!(at.get(), 9);
        assert_eq!(at - 9, CharOffset::ZERO);
        assert_eq!(at - CharOffset::new(5), 4);
        assert_eq!(ByteOffset::new(2).checked_sub(3), None);
        assert_eq!(ByteOffset::new(5).checked_sub(3), Some(ByteOffset::new(2)));
        assert_eq!(ByteOffset::new(2).saturating_sub(3), ByteOffset::ZERO);
        assert_eq!(ByteOffset::default(), ByteOffset::ZERO);
        assert!(CharOffset::new(1) < CharOffset::new(2));
    }

    #[test]
    fn ranges_measure_and_unwrap_to_numbers() {
        let range = ByteOffset::new(2)..ByteOffset::new(6);
        assert_eq!(range.end - range.start, 4);
        assert_eq!(range.get(), 2..6);
        assert_eq!(&"abcdefgh"[range.get()], "cdef");
    }

    #[test]
    fn serializes_as_a_plain_number() {
        assert_eq!(serde_json::to_string(&CharOffset::new(42)).unwrap(), "42");
        assert_eq!(
            serde_json::from_str::<ByteOffset>("7").unwrap(),
            ByteOffset::new(7)
        );
    }

    #[test]
    fn lines_count_from_zero_and_line_numbers_from_one() {
        let line = Line::new(11);
        assert_eq!(line.number().get(), 12);
        assert_eq!(LineNumber::from(line), line.number());
        assert_eq!(Line::from(line.number()), line);
        assert_eq!(Line::ZERO.number(), LineNumber::FIRST);
        assert_eq!(LineNumber::new(0), None);
        assert_eq!(LineNumber::new(12).map(LineNumber::index), Some(line));
        assert_eq!(LineNumber::FIRST.to_string(), "1");
        assert_eq!(Line::new(usize::MAX).number().get(), usize::MAX);
        assert_eq!((Line::new(2)..Line::new(5)).iter().len(), 3);
        assert_eq!(
            (Line::new(2)..Line::new(4))
                .iter()
                .rev()
                .collect::<Vec<_>>(),
            [Line::new(3), Line::new(2)]
        );
        assert_eq!(Line::new(3).abs_diff(Line::new(7)), 4);
        assert_eq!(VisualColumn::new(4) - VisualColumn::new(1), 3);
        assert_eq!(serde_json::to_string(&Line::new(9)).unwrap(), "9");
    }

    proptest! {
        #[test]
        fn arithmetic_matches_usize(a in 0usize..1 << 40, b in 0usize..1 << 40) {
            let (low, high) = (a.min(b), a.max(b));
            let at = CharOffset::new(low);
            prop_assert_eq!((at + (high - low)).get(), high);
            prop_assert_eq!(CharOffset::new(high) - at, high - low);
            prop_assert_eq!(CharOffset::new(high) - (high - low), at);
            prop_assert_eq!(at.checked_sub(high).map(CharOffset::get), low.checked_sub(high));
            prop_assert_eq!(at.saturating_sub(high).get(), low.saturating_sub(high));
            let delta = b.cast_signed() - a.cast_signed();
            prop_assert_eq!(at.saturating_add_signed(delta).get(), low.saturating_add_signed(delta));
            prop_assert_eq!(ByteOffset::new(a).cmp(&ByteOffset::new(b)), a.cmp(&b));
        }
    }
}
