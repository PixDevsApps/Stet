//! Visual columns: the cells a line's characters take.

use crate::text::VisualColumn;
use crate::view::TabWidth;

/// The visual column after `ch` when it starts at `column`: a tab advances to the next tab
/// stop, and every other character takes one cell.
pub(super) fn next_column(column: VisualColumn, ch: char, tab_width: TabWidth) -> VisualColumn {
    if ch == '\t' {
        tab_width.next_stop(column)
    } else {
        column + 1
    }
}

/// The cells `text` takes when it starts at visual column `column`.
pub fn text_width(text: &str, column: VisualColumn, tab_width: TabWidth) -> usize {
    text.chars()
        .fold(column, |at, ch| next_column(at, ch, tab_width))
        - column
}

/// Where Home takes a column cursor on `line` (Scintilla's "VC home"): to the end of the line's
/// indentation, or to column 0 when it is already there.
pub fn home_column(line: &str, current: VisualColumn, tab_width: TabWidth) -> VisualColumn {
    let indent = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .fold(VisualColumn::ZERO, |at, ch| next_column(at, ch, tab_width));
    if current == indent {
        VisualColumn::ZERO
    } else {
        indent
    }
}

/// The visual column where character `char_offset` of `line` starts, as
/// `gtk_source_view_get_visual_column` counts it. Offsets past the end give the line's width.
pub fn visual_column(line: &str, char_offset: usize, tab_width: TabWidth) -> VisualColumn {
    line.chars()
        .take(char_offset)
        .fold(VisualColumn::ZERO, |at, ch| next_column(at, ch, tab_width))
}

/// How a column cuts a tab: the tab's cells before the column and from it on. Both are non-zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TabCut {
    pub before: usize,
    pub after: usize,
}

/// Where a visual column falls in a line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Located {
    /// The character that starts at the column, or the tab that spans it, as a character offset
    /// in the line; the line's length when the column is at or past its end.
    pub char_offset: usize,
    /// `char_offset` in bytes.
    pub byte_offset: usize,
    /// Set when the column falls strictly inside the tab at `char_offset`.
    pub inside_tab: Option<TabCut>,
    /// Cells from the end of the line to the column; zero inside the line.
    pub virtual_space: usize,
}

impl Located {
    /// Cells from where the character at `char_offset` starts (or the line ends) to the column.
    /// A painter puts the column at that character's x plus this many cell advances.
    pub fn offset_cells(&self) -> usize {
        self.inside_tab.map_or(self.virtual_space, |cut| cut.before)
    }

    /// Whether the column is at or past the end of the line.
    pub fn at_end(&self, line: &str) -> bool {
        self.byte_offset == line.len()
    }
}

/// Where visual column `column` falls in `line`, a line without its `\n`.
pub fn locate(line: &str, column: VisualColumn, tab_width: TabWidth) -> Located {
    let (mut reached, mut char_offset) = (VisualColumn::ZERO, 0);
    for (byte_offset, ch) in line.char_indices() {
        if reached == column {
            return Located {
                char_offset,
                byte_offset,
                inside_tab: None,
                virtual_space: 0,
            };
        }
        let next = next_column(reached, ch, tab_width);
        if next > column {
            return Located {
                char_offset,
                byte_offset,
                inside_tab: Some(TabCut {
                    before: column - reached,
                    after: next - column,
                }),
                virtual_space: 0,
            };
        }
        reached = next;
        char_offset += 1;
    }
    Located {
        char_offset,
        byte_offset: line.len(),
        inside_tab: None,
        virtual_space: column - reached,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::testing::{col, tab};
    use proptest::prelude::*;

    const TAB: TabWidth = TabWidth::DEFAULT;

    #[test]
    fn tabs_advance_to_the_next_stop_and_everything_else_takes_one_cell() {
        assert_eq!(next_column(col(0), '\t', TAB), col(4));
        assert_eq!(next_column(col(3), '\t', TAB), col(4));
        assert_eq!(next_column(col(4), '\t', TAB), col(8));
        assert_eq!(next_column(col(5), '\t', tab(1)), col(6));
        for wide in ['日', 'é', '😀', '\u{301}', '—'] {
            assert_eq!(next_column(col(7), wide, TAB), col(8));
        }
        assert_eq!(text_width("a\tb", col(0), TAB), 5);
        assert_eq!(text_width("a\tb", col(2), TAB), 3);
        assert_eq!(visual_column("héllo\twörld", 6, TAB), col(8));
        assert_eq!(visual_column("ab", 9, TAB), col(2));
    }

    #[test]
    fn locates_boundaries_tab_cuts_and_virtual_space() {
        let line = "abcdefgh\t= 3;";
        assert_eq!(
            locate(line, col(10), TAB),
            Located {
                char_offset: 8,
                byte_offset: 8,
                inside_tab: Some(TabCut {
                    before: 2,
                    after: 2
                }),
                virtual_space: 0,
            }
        );
        assert_eq!(locate(line, col(8), TAB).inside_tab, None);
        assert_eq!(locate(line, col(12), TAB).char_offset, 9);
        let past = locate(line, col(20), TAB);
        assert_eq!((past.char_offset, past.virtual_space), (13, 4));
        assert!(past.at_end(line));
        assert_eq!(past.offset_cells(), 4);
        let multibyte = locate("øx", col(1), TAB);
        assert_eq!((multibyte.char_offset, multibyte.byte_offset), (1, 2));
        assert_eq!(locate("", col(3), TAB).virtual_space, 3);
    }

    #[test]
    fn home_goes_to_the_indentation_then_to_column_zero() {
        assert_eq!(home_column("\t  x = 1;", col(20), TAB), col(6));
        assert_eq!(home_column("\t  x = 1;", col(6), TAB), col(0));
        assert_eq!(home_column("\t  x = 1;", col(0), TAB), col(6));
        assert_eq!(home_column("x", col(1), TAB), col(0));
        assert_eq!(home_column("    ", col(9), TAB), col(4));
        assert_eq!(home_column("", col(3), TAB), col(0));
    }

    fn line() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop::sample::select(vec!['a', 'b', ' ', '\t', '\t', 'é', '日', '😀']),
            0..14,
        )
        .prop_map(String::from_iter)
    }

    proptest! {
        #[test]
        fn locate_inverts_visual_column(line in line(), cells in 1u8..9, column in 0usize..40) {
            let (tab, column) = (tab(cells), col(column));
            let at = locate(&line, column, tab);
            let start = visual_column(&line, at.char_offset, tab);
            prop_assert_eq!(start + at.offset_cells(), column);
            prop_assert_eq!(&line[..at.byte_offset].chars().count(), &at.char_offset);
            if let Some(cut) = at.inside_tab {
                prop_assert_eq!(line[at.byte_offset..].chars().next(), Some('\t'));
                prop_assert!(cut.before > 0 && cut.after > 0);
                prop_assert_eq!(start + cut.before + cut.after, next_column(start, '\t', tab));
            }
            for offset in 0..=line.chars().count() {
                let column = visual_column(&line, offset, tab);
                let at = locate(&line, column, tab);
                prop_assert_eq!((at.char_offset, at.inside_tab, at.virtual_space), (offset, None, 0));
            }
        }

        #[test]
        fn home_lands_on_a_character_and_toggles(line in line(), cells in 1u8..9, current in 0usize..40) {
            let tab = tab(cells);
            let home = home_column(&line, col(current), tab);
            let at = locate(&line, home, tab);
            prop_assert_eq!((at.inside_tab, at.virtual_space), (None, 0));
            prop_assert!(line[..at.byte_offset].chars().all(|ch| matches!(ch, ' ' | '\t')));
            let indent = home_column(&line, col(usize::MAX), tab);
            prop_assert!(home == col(0) || home == indent);
            prop_assert_eq!(home_column(&line, home_column(&line, indent, tab), tab), indent);
        }
    }
}
