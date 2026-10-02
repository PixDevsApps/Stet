//! Visual columns: the cells a line's characters take.

/// The visual column after `ch` when it starts at `column`: a tab advances to the next multiple
/// of `tab_width` (0 counts as 1), and every other character takes one cell.
pub fn next_column(column: usize, ch: char, tab_width: usize) -> usize {
    if ch == '\t' {
        let tab_width = tab_width.max(1);
        column + tab_width - column % tab_width
    } else {
        column + 1
    }
}

/// The cells `text` takes when it starts at visual column `column`.
pub fn text_width(text: &str, column: usize, tab_width: usize) -> usize {
    text.chars()
        .fold(column, |at, ch| next_column(at, ch, tab_width))
        - column
}

/// Where Home takes a column cursor on `line` (Scintilla's "VC home"): to the end of the line's
/// indentation, or to column 0 when it is already there.
pub fn home_column(line: &str, current: usize, tab_width: usize) -> usize {
    let indent = line
        .chars()
        .take_while(|ch| matches!(ch, ' ' | '\t'))
        .fold(0, |at, ch| next_column(at, ch, tab_width));
    if current == indent { 0 } else { indent }
}

/// The visual column where character `char_offset` of `line` starts, as
/// `gtk_source_view_get_visual_column` counts it. Offsets past the end give the line's width.
pub fn visual_column(line: &str, char_offset: usize, tab_width: usize) -> usize {
    line.chars()
        .take(char_offset)
        .fold(0, |at, ch| next_column(at, ch, tab_width))
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
pub fn locate(line: &str, column: usize, tab_width: usize) -> Located {
    let (mut reached, mut char_offset) = (0, 0);
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
    use proptest::prelude::*;

    #[test]
    fn tabs_advance_to_the_next_stop_and_everything_else_takes_one_cell() {
        assert_eq!(next_column(0, '\t', 4), 4);
        assert_eq!(next_column(3, '\t', 4), 4);
        assert_eq!(next_column(4, '\t', 4), 8);
        assert_eq!(next_column(5, '\t', 0), 6);
        for wide in ['日', 'é', '😀', '\u{301}', '—'] {
            assert_eq!(next_column(7, wide, 4), 8);
        }
        assert_eq!(text_width("a\tb", 0, 4), 5);
        assert_eq!(text_width("a\tb", 2, 4), 3);
        assert_eq!(visual_column("héllo\twörld", 6, 4), 8);
        assert_eq!(visual_column("ab", 9, 4), 2);
    }

    #[test]
    fn locates_boundaries_tab_cuts_and_virtual_space() {
        let line = "abcdefgh\t= 3;";
        assert_eq!(
            locate(line, 10, 4),
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
        assert_eq!(locate(line, 8, 4).inside_tab, None);
        assert_eq!(locate(line, 12, 4).char_offset, 9);
        let past = locate(line, 20, 4);
        assert_eq!((past.char_offset, past.virtual_space), (13, 4));
        assert!(past.at_end(line));
        assert_eq!(past.offset_cells(), 4);
        let multibyte = locate("øx", 1, 4);
        assert_eq!((multibyte.char_offset, multibyte.byte_offset), (1, 2));
        assert_eq!(locate("", 3, 4).virtual_space, 3);
    }

    #[test]
    fn home_goes_to_the_indentation_then_to_column_zero() {
        assert_eq!(home_column("\t  x = 1;", 20, 4), 6);
        assert_eq!(home_column("\t  x = 1;", 6, 4), 0);
        assert_eq!(home_column("\t  x = 1;", 0, 4), 6);
        assert_eq!(home_column("x", 1, 4), 0);
        assert_eq!(home_column("    ", 9, 4), 4);
        assert_eq!(home_column("", 3, 4), 0);
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
        fn locate_inverts_visual_column(line in line(), tab in 1usize..9, column in 0usize..40) {
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
        fn home_lands_on_a_character_and_toggles(line in line(), tab in 1usize..9, current in 0usize..40) {
            let home = home_column(&line, current, tab);
            let at = locate(&line, home, tab);
            prop_assert_eq!((at.inside_tab, at.virtual_space), (None, 0));
            prop_assert!(line[..at.byte_offset].chars().all(|ch| matches!(ch, ' ' | '\t')));
            let indent = home_column(&line, usize::MAX, tab);
            prop_assert!(home == 0 || home == indent);
            prop_assert_eq!(home_column(&line, home_column(&line, indent, tab), tab), indent);
        }
    }
}
