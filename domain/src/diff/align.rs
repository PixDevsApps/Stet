//! Rows that show the two texts side by side, with padding where one side has fewer lines.

use std::ops::Range;

use super::Side;
use crate::text::{Line, OffsetRange};

/// How many lines `lines` holds.
fn len(lines: &Range<Line>) -> usize {
    lines.iter().len()
}

/// A run of rows: left and right lines share rows in order, and the shorter side is padded
/// after its lines.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct AlignedChunk {
    pub left: Range<Line>,
    pub right: Range<Line>,
    /// The chunk's first row.
    pub row: usize,
}

impl AlignedChunk {
    pub(super) fn rows(&self) -> usize {
        len(&self.left).max(len(&self.right))
    }

    pub(super) fn lines(&self, side: Side) -> &Range<Line> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn lines_mut(&mut self, side: Side) -> &mut Range<Line> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }
}

/// What one side shows in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowSlot {
    Line(Line),
    /// Padding placed before line `before`; `before` is one past the last line for padding
    /// after it.
    Pad {
        before: Line,
    },
}

/// Blank rows a view inserts before line `before` (one past the last line means after it) so
/// that both views have the same rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pad {
    pub before: Line,
    pub rows: usize,
}

/// The side-by-side layout of a comparison: every line of both texts in order, one per row,
/// with equal and paired lines in the same row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Alignment {
    chunks: Vec<AlignedChunk>,
    /// One past the last line of each side.
    ends: [Line; 2],
}

impl Alignment {
    pub fn rows(&self) -> usize {
        self.chunks
            .last()
            .map_or(0, |chunk| chunk.row + chunk.rows())
    }

    pub fn line_count(&self, side: Side) -> usize {
        self.end(side) - Line::ZERO
    }

    /// One past the last line of `side`.
    pub fn end(&self, side: Side) -> Line {
        self.ends[side.index()]
    }

    #[cfg(test)]
    pub(super) fn chunks(&self) -> &[AlignedChunk] {
        &self.chunks
    }

    /// The row that shows `line` of `side`; lines past the end count as the last line.
    pub fn row_of(&self, side: Side, line: Line) -> usize {
        let line = line.min(self.end(side).saturating_sub(1));
        let index = self
            .chunks
            .partition_point(|chunk| chunk.lines(side).end <= line);
        self.chunks.get(index).map_or_else(
            || self.rows(),
            |chunk| chunk.row + (line - chunk.lines(side).start),
        )
    }

    pub fn slot(&self, side: Side, row: usize) -> RowSlot {
        let index = self
            .chunks
            .partition_point(|chunk| chunk.row + chunk.rows() <= row);
        match self.chunks.get(index) {
            Some(chunk) => {
                let lines = chunk.lines(side);
                let offset = row - chunk.row;
                if offset < len(lines) {
                    RowSlot::Line(lines.start + offset)
                } else {
                    RowSlot::Pad { before: lines.end }
                }
            }
            None => RowSlot::Pad {
                before: self.end(side),
            },
        }
    }

    /// The line of the other side in the same row as `line`, if that row is not padding.
    #[cfg(test)]
    pub fn counterpart(&self, side: Side, line: Line) -> Option<Line> {
        match self.slot(side.other(), self.row_of(side, line)) {
            RowSlot::Line(other) => Some(other),
            RowSlot::Pad { .. } => None,
        }
    }

    /// The padding `side` needs to line up with the other side, in line order.
    pub fn pads(&self, side: Side) -> Vec<Pad> {
        let mut pads: Vec<Pad> = Vec::new();
        for chunk in &self.chunks {
            let (mine, theirs) = (chunk.lines(side), chunk.lines(side.other()));
            if len(mine) >= len(theirs) {
                continue;
            }
            let rows = len(theirs) - len(mine);
            match pads.last_mut() {
                Some(pad) if pad.before == mine.end => pad.rows += rows,
                _ => pads.push(Pad {
                    before: mine.end,
                    rows,
                }),
            }
        }
        pads
    }

    /// The rows covered by the non-empty ranges among `left` and `right`.
    pub(super) fn span_rows(&self, left: &Range<Line>, right: &Range<Line>) -> Range<usize> {
        let (mut start, mut end) = (usize::MAX, 0);
        for (side, lines) in [(Side::Left, left), (Side::Right, right)] {
            if !lines.is_empty() {
                start = start.min(self.row_of(side, lines.start));
                end = end.max(self.row_of(side, lines.end - 1) + 1);
            }
        }
        start..end
    }
}

/// Builds an [`Alignment`] row by row. A one-sided row fills the first padding row of the
/// open chunk, so unpaired lines of the two sides end up side by side.
pub(super) struct AlignBuilder {
    chunks: Vec<AlignedChunk>,
    next: [Line; 2],
    open: bool,
}

impl AlignBuilder {
    pub(super) fn new() -> Self {
        Self {
            chunks: Vec::new(),
            next: [Line::ZERO; 2],
            open: false,
        }
    }

    pub(super) fn next(&self, side: Side) -> Line {
        self.next[side.index()]
    }

    pub(super) fn next_lines(&self) -> [Line; 2] {
        self.next
    }

    /// Puts the next line of each side in one row.
    pub(super) fn both(&mut self) {
        let [left, right] = self.next;
        match self.chunks.last_mut() {
            Some(chunk) if self.open && len(&chunk.left) == len(&chunk.right) => {
                chunk.left.end += 1;
                chunk.right.end += 1;
            }
            _ => self.push(left..left + 1, right..right + 1),
        }
        self.next = [left + 1, right + 1];
    }

    /// Puts the next line of `side` in a row where the other side is padding.
    pub(super) fn one(&mut self, side: Side) {
        let line = self.next(side);
        match self.chunks.last_mut() {
            Some(chunk) if self.open => chunk.lines_mut(side).end += 1,
            _ => {
                let other = self.next(side.other());
                match side {
                    Side::Left => self.push(line..line + 1, other..other),
                    Side::Right => self.push(other..other, line..line + 1),
                }
            }
        }
        self.next[side.index()] += 1;
    }

    /// Starts a new chunk with the next row, so rows on either side of a hunk boundary never
    /// share padding.
    pub(super) fn seal(&mut self) {
        self.open = false;
    }

    fn push(&mut self, left: Range<Line>, right: Range<Line>) {
        let row = self
            .chunks
            .last()
            .map_or(0, |chunk| chunk.row + chunk.rows());
        self.chunks.push(AlignedChunk { left, right, row });
        self.open = true;
    }

    pub(super) fn finish(self) -> Alignment {
        Alignment {
            chunks: self.chunks,
            ends: self.next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::testing as t;

    #[test]
    fn one_sided_rows_fill_padding_of_the_open_chunk() {
        let mut builder = AlignBuilder::new();
        builder.both();
        builder.one(Side::Right);
        builder.one(Side::Right);
        builder.one(Side::Left);
        builder.both();
        let alignment = builder.finish();
        assert_eq!(alignment.rows(), 4);
        assert_eq!(alignment.slot(Side::Left, 1), RowSlot::Line(t::line(1)));
        assert_eq!(alignment.slot(Side::Right, 1), RowSlot::Line(t::line(1)));
        assert_eq!(
            alignment.slot(Side::Left, 2),
            RowSlot::Pad { before: t::line(2) }
        );
        assert_eq!(alignment.slot(Side::Left, 3), RowSlot::Line(t::line(2)));
        assert_eq!(alignment.slot(Side::Right, 3), RowSlot::Line(t::line(3)));
        assert_eq!(
            alignment.pads(Side::Left),
            vec![Pad {
                before: t::line(2),
                rows: 1
            }]
        );
        assert!(alignment.pads(Side::Right).is_empty());
        assert_eq!(alignment.counterpart(Side::Right, t::line(2)), None);
    }

    #[test]
    fn a_sealed_chunk_keeps_its_padding() {
        let mut builder = AlignBuilder::new();
        builder.one(Side::Left);
        builder.seal();
        builder.one(Side::Right);
        let alignment = builder.finish();
        assert_eq!(alignment.rows(), 2);
        assert_eq!(
            alignment.pads(Side::Right),
            vec![Pad {
                before: t::line(0),
                rows: 1
            }]
        );
        assert_eq!(
            alignment.pads(Side::Left),
            vec![Pad {
                before: t::line(1),
                rows: 1
            }]
        );
        assert_eq!(alignment.row_of(Side::Right, t::line(0)), 1);
        assert_eq!(
            alignment.slot(Side::Right, 9),
            RowSlot::Pad { before: t::line(1) }
        );
    }
}
