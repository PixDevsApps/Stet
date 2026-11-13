//! Offsets and lines from plain numbers, for writing test data.

use std::ops::Range;

use super::units::{CharOffset, Line};

/// Character `offset`.
pub(crate) const fn at(offset: usize) -> CharOffset {
    CharOffset::new(offset)
}

/// The characters `range`.
pub(crate) fn chars(range: Range<usize>) -> Range<CharOffset> {
    at(range.start)..at(range.end)
}

/// Line `index`, counted from 0.
pub(crate) const fn line(index: usize) -> Line {
    Line::new(index)
}

/// The lines `range`.
pub(crate) fn lines(range: Range<usize>) -> Range<Line> {
    line(range.start)..line(range.end)
}

/// The indexes of `lines`, to compare with plain numbers.
pub(crate) fn indexes(lines: &[Line]) -> Vec<usize> {
    lines.iter().map(|line| line.get()).collect()
}
