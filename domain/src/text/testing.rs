//! Offsets from plain numbers, for writing test data.

use std::ops::Range;

use super::units::CharOffset;

/// Character `offset`.
pub(crate) const fn at(offset: usize) -> CharOffset {
    CharOffset::new(offset)
}

/// The characters `range`.
pub(crate) fn chars(range: Range<usize>) -> Range<CharOffset> {
    at(range.start)..at(range.end)
}
