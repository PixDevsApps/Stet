//! The rectangle and where its rows fall on each line.

use super::visual::locate;
use crate::text::LineIndex;
use std::ops::Range;

/// A line and a visual column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct VisualPos {
    pub line: usize,
    pub column: usize,
}

impl VisualPos {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

impl From<(usize, usize)> for VisualPos {
    fn from((line, column): (usize, usize)) -> Self {
        Self::new(line, column)
    }
}

/// A rectangular selection in visual columns, from the anchor to the cursor. The cursor's line
/// holds the primary (GTK) caret. When the left and right columns are equal, it is a column
/// caret.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rect {
    pub anchor: VisualPos,
    pub cursor: VisualPos,
}

impl Rect {
    pub fn new(anchor: impl Into<VisualPos>, cursor: impl Into<VisualPos>) -> Self {
        Self {
            anchor: anchor.into(),
            cursor: cursor.into(),
        }
    }

    pub fn top(self) -> usize {
        self.anchor.line.min(self.cursor.line)
    }

    pub fn bottom(self) -> usize {
        self.anchor.line.max(self.cursor.line)
    }

    pub fn left(self) -> usize {
        self.anchor.column.min(self.cursor.column)
    }

    pub fn right(self) -> usize {
        self.anchor.column.max(self.cursor.column)
    }

    pub fn width(self) -> usize {
        self.right() - self.left()
    }

    /// Whether the rectangle covers no cells, that is, whether it is a column caret.
    pub fn is_empty(self) -> bool {
        self.anchor.column == self.cursor.column
    }

    /// The lines the rectangle covers, whether or not the text has them.
    pub fn lines(self) -> Range<usize> {
        self.top()..self.bottom().saturating_add(1)
    }

    /// A column caret at `column` on the same lines.
    pub fn with_column(self, column: usize) -> Self {
        Self::new((self.anchor.line, column), (self.cursor.line, column))
    }

    /// The same rectangle `delta` lines down (up when negative), stopping at line 0. It moves a
    /// rectangle into and out of a snapshot of some of the document's lines.
    pub fn shift_lines(self, delta: isize) -> Self {
        Self::new(
            (
                self.anchor.line.saturating_add_signed(delta),
                self.anchor.column,
            ),
            (
                self.cursor.line.saturating_add_signed(delta),
                self.cursor.column,
            ),
        )
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
    pub line: usize,
    /// Document character offsets of the characters at or containing the left and right edges.
    pub chars: Range<usize>,
    pub pad_left: usize,
    pub pad_right: usize,
}

impl LineSpan {
    /// The span of `rect`'s columns on `line`, whose text without `\n` is `line_text` and whose
    /// first character is at document offset `line_start`.
    pub fn new(
        line: usize,
        line_text: &str,
        line_start: usize,
        rect: Rect,
        tab_width: usize,
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

/// The spans of `rect` on the lines of `text` it covers.
pub fn spans(text: &str, index: &LineIndex, rect: Rect, tab_width: usize) -> Vec<LineSpan> {
    clamp_lines(index, rect.lines())
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

/// The part of `lines` that the indexed text has.
pub(super) fn clamp_lines(index: &LineIndex, lines: Range<usize>) -> Range<usize> {
    let count = index.line_count();
    lines.start.min(count)..lines.end.min(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::visual::visual_column;
    use proptest::prelude::*;

    #[test]
    fn normalizes_anchor_and_cursor() {
        let rect = Rect::new((9, 12), (2, 4));
        assert_eq!(
            (rect.top(), rect.bottom(), rect.left(), rect.right()),
            (2, 9, 4, 12)
        );
        assert_eq!((rect.lines(), rect.width()), (2..10, 8));
        assert!(!rect.is_empty());
        let caret = rect.with_column(6);
        assert!(caret.is_empty());
        assert_eq!((caret.anchor, caret.cursor), ((9, 6).into(), (2, 6).into()));
        let moved = rect.shift_lines(-3);
        assert_eq!((moved.top(), moved.bottom()), (0, 6));
        assert_eq!(moved.shift_lines(3).lines(), 3..10);
    }

    #[test]
    fn spans_give_each_edge_as_a_character_plus_cells() {
        let text = "abcdefgh\t= 3;\n\tx\nab";
        let index = LineIndex::new(text);
        let spans = spans(text, &index, Rect::new((0, 2), (5, 10)), 4);
        assert_eq!(
            spans,
            vec![
                LineSpan {
                    line: 0,
                    chars: 2..8,
                    pad_left: 0,
                    pad_right: 2,
                },
                LineSpan {
                    line: 1,
                    chars: 14..16,
                    pad_left: 2,
                    pad_right: 5,
                },
                LineSpan {
                    line: 2,
                    chars: 19..19,
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
            tab in 1usize..9,
            (a, b) in (0usize..8, 0usize..8),
            (left, right) in (0usize..24, 0usize..24),
        ) {
            let index = LineIndex::new(&text);
            let rect = Rect::new((a, left), (b, right));
            let spans = spans(&text, &index, rect, tab);
            let lines = clamp_lines(&index, rect.lines());
            prop_assert_eq!(spans.iter().map(|span| span.line).collect::<Vec<_>>(), lines.collect::<Vec<_>>());
            for span in spans {
                let line_text = &text[index.line_bytes(span.line)];
                let start = index.line_chars(span.line).start;
                let column = |offset: usize| visual_column(line_text, offset - start, tab);
                prop_assert_eq!(column(span.chars.start) + span.pad_left, rect.left());
                prop_assert_eq!(column(span.chars.end) + span.pad_right, rect.right());
            }
        }
    }
}
