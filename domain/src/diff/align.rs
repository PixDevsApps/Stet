//! Rows that show the two texts side by side, with padding where one side has fewer lines.

use std::ops::Range;

use super::Side;

/// A run of rows: left and right lines share rows in order, and the shorter side is padded
/// after its lines.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct AlignedChunk {
    pub left: Range<usize>,
    pub right: Range<usize>,
    /// The chunk's first row.
    pub row: usize,
}

impl AlignedChunk {
    pub fn rows(&self) -> usize {
        self.left.len().max(self.right.len())
    }

    pub fn lines(&self, side: Side) -> &Range<usize> {
        match side {
            Side::Left => &self.left,
            Side::Right => &self.right,
        }
    }

    fn lines_mut(&mut self, side: Side) -> &mut Range<usize> {
        match side {
            Side::Left => &mut self.left,
            Side::Right => &mut self.right,
        }
    }
}

/// What one side shows in a row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RowSlot {
    Line(usize),
    /// Padding placed before line `before`; `before` is the line count for padding after the
    /// last line.
    Pad {
        before: usize,
    },
}

/// Blank rows a view inserts before line `before` (the line count means after the last line)
/// so that both views have the same rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Pad {
    pub before: usize,
    pub rows: usize,
}

/// The side-by-side layout of a comparison: every line of both texts in order, one per row,
/// with equal and paired lines in the same row.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Alignment {
    chunks: Vec<AlignedChunk>,
    lines: [usize; 2],
}

impl Alignment {
    pub fn rows(&self) -> usize {
        self.chunks
            .last()
            .map_or(0, |chunk| chunk.row + chunk.rows())
    }

    pub fn line_count(&self, side: Side) -> usize {
        self.lines[side.index()]
    }

    pub fn chunks(&self) -> &[AlignedChunk] {
        &self.chunks
    }

    /// The row that shows `line` of `side`; lines past the end count as the last line.
    pub fn row_of(&self, side: Side, line: usize) -> usize {
        let line = line.min(self.line_count(side).saturating_sub(1));
        let index = self
            .chunks
            .partition_point(|chunk| chunk.lines(side).end <= line);
        self.chunks.get(index).map_or(self.rows(), |chunk| {
            chunk.row + line - chunk.lines(side).start
        })
    }

    pub fn slot(&self, side: Side, row: usize) -> RowSlot {
        let index = self
            .chunks
            .partition_point(|chunk| chunk.row + chunk.rows() <= row);
        match self.chunks.get(index) {
            Some(chunk) => {
                let lines = chunk.lines(side);
                let offset = row - chunk.row;
                if offset < lines.len() {
                    RowSlot::Line(lines.start + offset)
                } else {
                    RowSlot::Pad { before: lines.end }
                }
            }
            None => RowSlot::Pad {
                before: self.line_count(side),
            },
        }
    }

    /// The line of the other side in the same row as `line`, if that row is not padding.
    pub fn counterpart(&self, side: Side, line: usize) -> Option<usize> {
        match self.slot(side.other(), self.row_of(side, line)) {
            RowSlot::Line(other) => Some(other),
            RowSlot::Pad { .. } => None,
        }
    }

    /// The line of the other side to scroll level with `line`: its counterpart, or the line
    /// after the padding in that row. This drives synchronized scrolling of unpadded views.
    pub fn scroll_line(&self, side: Side, line: usize) -> usize {
        let other = side.other();
        match self.slot(other, self.row_of(side, line)) {
            RowSlot::Line(line) => line,
            RowSlot::Pad { before } => before.min(self.line_count(other).saturating_sub(1)),
        }
    }

    /// The padding `side` needs to line up with the other side, in line order.
    pub fn pads(&self, side: Side) -> Vec<Pad> {
        let mut pads: Vec<Pad> = Vec::new();
        for chunk in &self.chunks {
            let (mine, theirs) = (chunk.lines(side), chunk.lines(side.other()));
            if mine.len() >= theirs.len() {
                continue;
            }
            let rows = theirs.len() - mine.len();
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
    pub(super) fn span_rows(&self, left: &Range<usize>, right: &Range<usize>) -> Range<usize> {
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
    next: [usize; 2],
    open: bool,
}

impl AlignBuilder {
    pub(super) fn new() -> Self {
        Self {
            chunks: Vec::new(),
            next: [0, 0],
            open: false,
        }
    }

    pub(super) fn next(&self, side: Side) -> usize {
        self.next[side.index()]
    }

    pub(super) fn next_lines(&self) -> [usize; 2] {
        self.next
    }

    /// Puts the next line of each side in one row.
    pub(super) fn both(&mut self) {
        let [left, right] = self.next;
        match self.chunks.last_mut() {
            Some(chunk) if self.open && chunk.left.len() == chunk.right.len() => {
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

    fn push(&mut self, left: Range<usize>, right: Range<usize>) {
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
            lines: self.next,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(alignment.slot(Side::Left, 1), RowSlot::Line(1));
        assert_eq!(alignment.slot(Side::Right, 1), RowSlot::Line(1));
        assert_eq!(alignment.slot(Side::Left, 2), RowSlot::Pad { before: 2 });
        assert_eq!(alignment.slot(Side::Left, 3), RowSlot::Line(2));
        assert_eq!(alignment.slot(Side::Right, 3), RowSlot::Line(3));
        assert_eq!(alignment.pads(Side::Left), vec![Pad { before: 2, rows: 1 }]);
        assert!(alignment.pads(Side::Right).is_empty());
        assert_eq!(alignment.counterpart(Side::Right, 2), None);
        assert_eq!(alignment.scroll_line(Side::Right, 2), 2);
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
            vec![Pad { before: 0, rows: 1 }]
        );
        assert_eq!(alignment.pads(Side::Left), vec![Pad { before: 1, rows: 1 }]);
        assert_eq!(alignment.row_of(Side::Right, 0), 1);
        assert_eq!(alignment.slot(Side::Right, 9), RowSlot::Pad { before: 1 });
    }
}
