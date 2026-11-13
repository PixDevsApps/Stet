//! The rectangle and where its rows fall on each line.

use super::visual::locate;
use crate::text::{CharOffset, Line, LineIndex, VisualColumn};
use crate::view::TabWidth;
use std::ops::Range;

/// A line and a visual column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VisualPos {
    /// The line, counted from 0.
    pub line: Line,
    /// The column, with tabs expanded to their tab stops.
    pub column: VisualColumn,
}

impl VisualPos {
    /// The position at `column` on `line`.
    #[must_use]
    pub const fn new(line: Line, column: VisualColumn) -> Self {
        Self { line, column }
    }
}

/// A rectangular selection in visual columns, from the anchor to the cursor. The cursor's line
/// holds the primary (GTK) caret. When the left and right columns are equal, it is a column
/// caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    /// The corner where the selection started; it stays put while the cursor moves.
    pub anchor: VisualPos,
    /// The corner that moves, on the line of the primary caret.
    pub cursor: VisualPos,
}

impl Rect {
    /// The rectangle spanned from `anchor` to `cursor`.
    #[must_use]
    pub const fn new(anchor: VisualPos, cursor: VisualPos) -> Self {
        Self { anchor, cursor }
    }

    /// The first line the rectangle covers.
    #[must_use]
    pub fn top(self) -> Line {
        self.anchor.line.min(self.cursor.line)
    }

    /// The last line the rectangle covers.
    #[must_use]
    pub fn bottom(self) -> Line {
        self.anchor.line.max(self.cursor.line)
    }

    /// The column of the rectangle's left edge.
    #[must_use]
    pub fn left(self) -> VisualColumn {
        self.anchor.column.min(self.cursor.column)
    }

    /// The column of the rectangle's right edge.
    #[must_use]
    pub fn right(self) -> VisualColumn {
        self.anchor.column.max(self.cursor.column)
    }

    /// The cells between the left and right columns.
    #[must_use]
    pub fn width(self) -> usize {
        self.right() - self.left()
    }

    /// Whether the rectangle covers no cells, that is, whether it is a column caret.
    #[must_use]
    pub fn is_empty(self) -> bool {
        self.anchor.column == self.cursor.column
    }

    /// The lines the rectangle covers, whether or not the text has them.
    #[must_use]
    pub fn lines(self) -> Range<Line> {
        self.top()..self.bottom().saturating_add(1)
    }

    /// A column caret at `column` on the same lines.
    #[must_use]
    pub fn with_column(self, column: VisualColumn) -> Self {
        Self::new(
            VisualPos::new(self.anchor.line, column),
            VisualPos::new(self.cursor.line, column),
        )
    }

    /// The same rectangle `delta` lines down (up when negative), stopping at line 0. It moves a
    /// rectangle into and out of a snapshot of some of the document's lines.
    #[must_use]
    pub fn shift_lines(self, delta: isize) -> Self {
        let shift = |at: VisualPos| VisualPos::new(at.line.saturating_add_signed(delta), at.column);
        Self::new(shift(self.anchor), shift(self.cursor))
    }
}

/// Where a rectangle's row on one line starts and ends, for painting.
///
/// Each edge is given as the character at or containing it plus a number of cells, so its x is
/// that character's x (`iter_location`) plus `pad_*` cell advances. An edge that cuts a tab is
/// the tab plus the tab's cells before the edge; an edge past the end of the line is the line's
/// end plus the virtual cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineSpan {
    /// The line the span is on.
    pub line: Line,
    /// Document character offsets of the characters at or containing the left and right edges.
    pub chars: Range<CharOffset>,
    /// Cells from the character at `chars.start` to the left edge.
    pub pad_left: usize,
    /// Cells from the character at `chars.end` to the right edge.
    pub pad_right: usize,
}

impl LineSpan {
    /// The span of `rect`'s columns on `line`, whose text without `\n` is `line_text` and whose
    /// first character is at document offset `line_start`.
    #[must_use]
    pub fn new(
        line: Line,
        line_text: &str,
        line_start: CharOffset,
        rect: Rect,
        tab_width: TabWidth,
    ) -> Self {
        let left = locate(line_text, rect.left(), tab_width);
        let right = if rect.is_empty() {
            left
        } else {
            locate(line_text, rect.right(), tab_width)
        };
        Self {
            line,
            chars: line_start + left.char_offset..line_start + right.char_offset,
            pad_left: left.offset_cells(),
            pad_right: right.offset_cells(),
        }
    }
}

/// The part of `lines` that the indexed text has.
pub(super) fn clamp_lines(index: &LineIndex, lines: Range<Line>) -> Range<Line> {
    let end = index.end_line();
    lines.start.min(end)..lines.end.min(end)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::testing::{col, pos, tab};
    use crate::column::visual::visual_column;
    use crate::text::OffsetRange;
    use crate::text::testing::{chars, line};
    use proptest::prelude::*;

    /// The spans of `rect` on the lines of `text` it covers.
    fn spans(text: &str, index: &LineIndex, rect: Rect, tab_width: TabWidth) -> Vec<LineSpan> {
        clamp_lines(index, rect.lines())
            .iter()
            .map(|line| {
                LineSpan::new(
                    line,
                    &text[index.line_bytes(line)],
                    index.line_chars(line).start,
                    rect,
                    tab_width,
                )
            })
            .collect()
    }

    #[test]
    fn normalizes_anchor_and_cursor() {
        let rect = Rect::new(pos(9, 12), pos(2, 4));
        assert_eq!(
            (rect.top(), rect.bottom(), rect.left(), rect.right()),
            (line(2), line(9), col(4), col(12))
        );
        assert_eq!((rect.lines().get(), rect.width()), (2..10, 8));
        assert!(!rect.is_empty());
        let caret = rect.with_column(col(6));
        assert!(caret.is_empty());
        assert_eq!((caret.anchor, caret.cursor), (pos(9, 6), pos(2, 6)));
        let moved = rect.shift_lines(-3);
        assert_eq!((moved.top(), moved.bottom()), (line(0), line(6)));
        assert_eq!(moved.shift_lines(3).lines().get(), 3..10);
    }

    #[test]
    fn spans_give_each_edge_as_a_character_plus_cells() {
        let text = "abcdefgh\t= 3;\n\tx\nab";
        let index = LineIndex::new(text);
        let spans = spans(text, &index, Rect::new(pos(0, 2), pos(5, 10)), tab(4));
        assert_eq!(
            spans,
            vec![
                LineSpan {
                    line: line(0),
                    chars: chars(2..8),
                    pad_left: 0,
                    pad_right: 2,
                },
                LineSpan {
                    line: line(1),
                    chars: chars(14..16),
                    pad_left: 2,
                    pad_right: 5,
                },
                LineSpan {
                    line: line(2),
                    chars: chars(19..19),
                    pad_left: 0,
                    pad_right: 8,
                },
            ]
        );
    }

    fn text() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec!['a', ' ', '\t', '\t', 'é', '日', '\n', '\n']),
            0..30,
        )
        .prop_map(String::from_iter)
    }

    proptest! {
        #[test]
        fn span_edges_land_on_the_rectangle_columns(
            text in text(),
            cells in 1u8..9,
            (a, b) in (0usize..8, 0usize..8),
            (left, right) in (0usize..24, 0usize..24),
        ) {
            let tab = tab(cells);
            let index = LineIndex::new(&text);
            let rect = Rect::new(pos(a, left), pos(b, right));
            let spans = spans(&text, &index, rect, tab);
            let lines = clamp_lines(&index, rect.lines());
            prop_assert_eq!(spans.iter().map(|span| span.line).collect::<Vec<_>>(), lines.iter().collect::<Vec<_>>());
            for span in spans {
                let line_text = &text[index.line_bytes(span.line)];
                let start = index.line_chars(span.line).start;
                let column = |offset: CharOffset| visual_column(line_text, offset - start, tab);
                prop_assert_eq!(column(span.chars.start) + span.pad_left, rect.left());
                prop_assert_eq!(column(span.chars.end) + span.pad_right, rect.right());
            }
        }
    }
}
