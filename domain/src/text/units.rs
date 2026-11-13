//! Offsets into a text, one type per unit, so a byte offset never passes for a character
//! offset. GtkTextIter, [`TextEdit`](super::TextEdit) and every position the user sees count
//! characters (Unicode scalar values); slicing a `&str`, PCRE2 and file I/O count UTF-8 bytes.
//!
//! An offset plus a count of units is an offset; an offset minus an offset is a count. Two
//! offsets don't add, and a bare `usize` becomes an offset only through `new`, where the unit
//! is named.

use std::ops::{Add, AddAssign, Range, Sub};

use serde::{Deserialize, Serialize};

macro_rules! offset {
    ($(#[$doc:meta])* $name:ident, $unit:literal) => {
        $(#[$doc])*
        #[derive(
            Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
        )]
        #[serde(transparent)]
        #[repr(transparent)]
        pub struct $name(usize);

        impl $name {
            #[doc = concat!("The start of the text: ", $unit, " 0.")]
            pub const ZERO: Self = Self(0);

            #[doc = concat!("The offset of ", $unit, " `offset`, counted from the start of the text.")]
            #[must_use]
            pub const fn new(offset: usize) -> Self {
                Self(offset)
            }

            #[doc = concat!("The offset as a number of ", $unit, "s from the start of the text.")]
            #[must_use]
            pub const fn get(self) -> usize {
                self.0
            }

            #[doc = concat!("`count` ", $unit, "s earlier, or `None` before the start of the text.")]
            #[must_use]
            pub const fn checked_sub(self, count: usize) -> Option<Self> {
                match self.0.checked_sub(count) {
                    Some(offset) => Some(Self(offset)),
                    None => None,
                }
            }

            #[doc = concat!("`count` ", $unit, "s earlier, stopping at the start of the text.")]
            #[must_use]
            pub const fn saturating_sub(self, count: usize) -> Self {
                Self(self.0.saturating_sub(count))
            }

            #[doc = concat!("`delta` ", $unit, "s later, or earlier when negative, stopping at the start of the text.")]
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

        #[doc = concat!("`count` ", $unit, "s earlier; panics in debug builds before the start.")]
        impl Sub<usize> for $name {
            type Output = Self;

            fn sub(self, count: usize) -> Self {
                Self(self.0 - count)
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
            fn get(&self) -> Range<usize> {
                self.start.0..self.end.0
            }
        }
    };
}

offset!(
    /// An offset in characters (Unicode scalar values): the unit of GtkTextIter,
    /// [`TextEdit`](super::TextEdit) and the columns the user sees.
    CharOffset,
    "character"
);

offset!(
    /// An offset in UTF-8 bytes: the unit of `&str` slicing, PCRE2 and the bytes on disk. On
    /// valid text it falls on a character boundary.
    ByteOffset,
    "byte"
);

/// A range of offsets in one unit. Its length is `range.end - range.start`, a count.
pub trait OffsetRange {
    /// The range in plain numbers, for slicing and for APIs that count in the same unit.
    fn get(&self) -> Range<usize>;
}

/// A point known in both units: byte `byte` of a text is its character `char`. Conversions
/// start counting there instead of at the start of the text.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Anchor {
    pub byte: ByteOffset,
    pub char: CharOffset,
}

impl Anchor {
    /// The start of every text.
    pub const START: Self = Self {
        byte: ByteOffset::ZERO,
        char: CharOffset::ZERO,
    };

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
