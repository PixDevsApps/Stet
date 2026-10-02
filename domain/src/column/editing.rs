//! Column edits: insert, typing, Backspace, Delete and block delete.

use super::rect::{Rect, clamp_lines};
use super::visual::{locate, next_column, text_width};
use crate::text::{LineIndex, TextEdit};
use std::fmt;
use std::ops::Range;

/// The edits of one column operation and the rectangle after it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnEdit {
    /// Sorted, non-overlapping edits in character offsets of the snapshot, at most one per line.
    pub edits: Vec<TextEdit>,
    /// The rectangle once the edits are applied.
    pub rect: Rect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnError {
    /// Text typed or inserted in column mode must not contain a line break; a block of several
    /// rows goes through [`paste`](super::paste).
    LineBreak,
    /// A number of the Column Editor's sequence does not fit in an `i64`.
    Overflow,
}

impl fmt::Display for ColumnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::LineBreak => write!(f, "column text must not contain a line break"),
            Self::Overflow => write!(f, "the number sequence does not fit in 64 bits"),
        }
    }
}

impl std::error::Error for ColumnError {}

/// Inserts `inserted` at the rectangle's left column on each of its lines, padding short lines
/// with spaces. The block's cells are kept, and the rectangle moves right with them.
pub fn insert(
    text: &str,
    index: &LineIndex,
    rect: Rect,
    inserted: &str,
    tab_width: usize,
) -> Result<ColumnEdit, ColumnError> {
    single_line(inserted)?;
    let column = rect.left();
    let edits = edit_lines(text, index, rect.lines(), |_, line| {
        splice(line, column, column, inserted, 0, tab_width)
    });
    let width = text_width(inserted, column, tab_width);
    let rect = Rect::new(
        (rect.anchor.line, rect.anchor.column + width),
        (rect.cursor.line, rect.cursor.column + width),
    );
    Ok(ColumnEdit { edits, rect })
}

/// Types `typed` on every line of the rectangle: at its column when it is a caret, and in place
/// of its block otherwise. The caret ends up after the typed text.
pub fn type_text(
    text: &str,
    index: &LineIndex,
    rect: Rect,
    typed: &str,
    tab_width: usize,
) -> Result<ColumnEdit, ColumnError> {
    single_line(typed)?;
    let (left, right) = (rect.left(), rect.right());
    let edits = edit_lines(text, index, rect.lines(), |_, line| {
        splice(line, left, right, typed, 0, tab_width)
    });
    let rect = rect.with_column(left + text_width(typed, left, tab_width));
    Ok(ColumnEdit { edits, rect })
}

/// Deletes the block's cells on every line; the caret ends up at its left column.
pub fn delete_block(text: &str, index: &LineIndex, rect: Rect, tab_width: usize) -> ColumnEdit {
    delete_cells(text, index, rect, rect.left(), rect.right(), tab_width)
}

/// Backspace in column mode. A block is deleted. With a caret, the character before the caret
/// goes on every line when those characters are equally wide, and otherwise one cell; lines
/// that end before the deleted cells keep their text. Nothing happens at column 0.
pub fn backspace(text: &str, index: &LineIndex, rect: Rect, tab_width: usize) -> ColumnEdit {
    let column = rect.left();
    if !rect.is_empty() || column == 0 {
        return delete_block(text, index, rect, tab_width);
    }
    let cells = backspace_cells(line_texts(text, index, rect), column, tab_width);
    delete_cells(text, index, rect, column - cells, column, tab_width)
}

/// Delete in column mode. A block is deleted. With a caret, the character after the caret goes
/// on every line when those characters are equally wide, and otherwise one cell. Line ends are
/// never deleted.
pub fn delete_forward(text: &str, index: &LineIndex, rect: Rect, tab_width: usize) -> ColumnEdit {
    let column = rect.left();
    if !rect.is_empty() {
        return delete_block(text, index, rect, tab_width);
    }
    let cells = delete_cells_after(line_texts(text, index, rect), column, tab_width);
    delete_cells(text, index, rect, column, column + cells, tab_width)
}

fn delete_cells(
    text: &str,
    index: &LineIndex,
    rect: Rect,
    left: usize,
    right: usize,
    tab_width: usize,
) -> ColumnEdit {
    let edits = edit_lines(text, index, rect.lines(), |_, line| {
        splice(line, left, right, "", 0, tab_width)
    });
    ColumnEdit {
        edits,
        rect: rect.with_column(left),
    }
}

/// How many cells Backspace deletes before `column` (> 0).
fn backspace_cells<'a>(
    lines: impl Iterator<Item = &'a str>,
    column: usize,
    tab_width: usize,
) -> usize {
    let (mut width, mut widest_short) = (None, 0);
    for line in lines {
        let mut reached = 0;
        let mut before = None;
        for ch in line.chars() {
            let next = next_column(reached, ch, tab_width);
            if next >= column {
                before = Some((next == column).then_some(next - reached));
                break;
            }
            reached = next;
        }
        match before {
            Some(Some(cells)) if width.is_none_or(|width| width == cells) => width = Some(cells),
            Some(_) => return 1,
            None => widest_short = widest_short.max(reached),
        }
    }
    match width {
        Some(cells) if widest_short + cells <= column => cells,
        _ => 1,
    }
}

/// How many cells Delete deletes from `column`.
fn delete_cells_after<'a>(
    lines: impl Iterator<Item = &'a str>,
    column: usize,
    tab_width: usize,
) -> usize {
    let mut width = None;
    for line in lines {
        let at = locate(line, column, tab_width);
        if at.inside_tab.is_some() {
            return 1;
        }
        if let Some(ch) = line[at.byte_offset..].chars().next() {
            let cells = next_column(column, ch, tab_width) - column;
            if width.is_some_and(|width| width != cells) {
                return 1;
            }
            width = Some(cells);
        }
    }
    width.unwrap_or(1)
}

pub(super) fn single_line(text: &str) -> Result<(), ColumnError> {
    if text.contains(['\n', '\r']) {
        Err(ColumnError::LineBreak)
    } else {
        Ok(())
    }
}

/// A replacement in one line: the line's characters it replaces and their new text.
pub(super) type Splice = (Range<usize>, String);

/// Collects the edits `splice` returns for the lines of `lines` that the text has, in document
/// character offsets.
pub(super) fn edit_lines(
    text: &str,
    index: &LineIndex,
    lines: Range<usize>,
    mut splice: impl FnMut(usize, &str) -> Option<Splice>,
) -> Vec<TextEdit> {
    clamp_lines(index, lines)
        .filter_map(|line| {
            let start = index.line_chars(line).start;
            splice(line, &text[index.line_bytes(line)]).map(|(chars, insert)| {
                TextEdit::new(start + chars.start..start + chars.end, insert)
            })
        })
        .collect()
}

fn line_texts<'a>(
    text: &'a str,
    index: &'a LineIndex,
    rect: Rect,
) -> impl Iterator<Item = &'a str> {
    clamp_lines(index, rect.lines()).map(|line| &text[index.line_bytes(line)])
}

/// Replaces cells `left..right` of `line` with `insert`, which then starts exactly at `left`,
/// padded with spaces to `fill` cells when the line goes on after the replaced cells. A tab
/// that `left` or `right` cuts becomes the spaces it is drawn as, and a line that ends before
/// `left` is padded to it. Returns `None` when nothing would change.
pub(super) fn splice(
    line: &str,
    left: usize,
    right: usize,
    insert: &str,
    fill: usize,
    tab_width: usize,
) -> Option<Splice> {
    let start = locate(line, left, tab_width);
    let end = if right > left {
        locate(line, right, tab_width)
    } else {
        start
    };
    let (to, to_byte, tail) = match end.inside_tab {
        Some(cut) => (end.char_offset + 1, end.byte_offset + 1, cut.after),
        None => (end.char_offset, end.byte_offset, 0),
    };
    let follows = to_byte < line.len() || tail > 0;
    let pad = if follows && fill > 0 {
        fill.saturating_sub(text_width(insert, left, tab_width))
    } else {
        0
    };
    if insert.is_empty() && pad == 0 && (right <= left || start.at_end(line)) {
        return None;
    }
    let head = start.offset_cells();
    let mut replacement = String::with_capacity(head + insert.len() + pad + tail);
    push_spaces(&mut replacement, head);
    replacement.push_str(insert);
    push_spaces(&mut replacement, pad + tail);
    Some((start.char_offset..to, replacement))
}

pub(super) fn push_spaces(text: &mut String, count: usize) {
    text.extend(std::iter::repeat_n(' ', count));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::apply;

    fn run(text: &str, edit: impl Fn(&str, &LineIndex) -> ColumnEdit) -> (String, Rect) {
        let index = LineIndex::new(text);
        let result = edit(text, &index);
        (apply(text, result.edits).unwrap(), result.rect)
    }

    fn caret(top: usize, bottom: usize, column: usize) -> Rect {
        Rect::new((bottom, column), (top, column))
    }

    #[test]
    fn inserts_at_the_column_padding_short_lines_and_splitting_a_cut_tab() {
        let text = "abcdefgh\t= 3;\nshort\n\n\tx";
        let (result, rect) = run(text, |text, index| {
            insert(text, index, caret(0, 3, 10), "// ", 4).unwrap()
        });
        assert_eq!(
            result,
            "abcdefgh  //   = 3;\nshort     // \n          // \n\tx     // "
        );
        assert_eq!(rect, caret(0, 3, 13));
    }

    #[test]
    fn a_tab_that_starts_or_ends_at_the_column_stays_a_tab() {
        let text = "ab\tcd";
        for (column, expected) in [(2, "abX\tcd"), (4, "ab\tXcd"), (3, "ab X cd")] {
            let (result, _) = run(text, |text, index| {
                insert(text, index, caret(0, 0, column), "X", 4).unwrap()
            });
            assert_eq!(result, expected, "column {column}");
        }
    }

    #[test]
    fn insert_keeps_the_block_and_moves_it_right() {
        let (result, rect) = run("abcdef\nabcdef", |text, index| {
            insert(text, index, Rect::new((0, 4), (1, 2)), "__", 4).unwrap()
        });
        assert_eq!(result, "ab__cdef\nab__cdef");
        assert_eq!(rect, Rect::new((0, 6), (1, 4)));
    }

    #[test]
    fn typing_across_a_tab_stop_keeps_the_typed_text_together() {
        let mut text = "abcdefgh\t= 3;\nabcdefghijklmnop".to_owned();
        let mut rect = caret(0, 1, 10);
        for key in ["a", "b", "c", "d", "e"] {
            let index = LineIndex::new(&text);
            let edit = type_text(&text, &index, rect, key, 4).unwrap();
            text = apply(&text, edit.edits).unwrap();
            rect = edit.rect;
        }
        assert_eq!(text, "abcdefgh  abcde  = 3;\nabcdefghijabcdeklmnop");
        assert_eq!(rect, caret(0, 1, 15));
    }

    #[test]
    fn typing_over_a_block_replaces_it() {
        let (result, rect) = run("0123456789\n0123\n", |text, index| {
            type_text(text, index, Rect::new((0, 2), (2, 6)), "xy", 4).unwrap()
        });
        assert_eq!(result, "01xy6789\n01xy\n  xy");
        assert_eq!(rect, Rect::new((0, 4), (2, 4)));
    }

    #[test]
    fn line_breaks_are_rejected() {
        let index = LineIndex::new("ab");
        for bad in ["a\nb", "\r"] {
            assert_eq!(
                type_text("ab", &index, caret(0, 0, 1), bad, 4),
                Err(ColumnError::LineBreak)
            );
            assert_eq!(
                insert("ab", &index, caret(0, 0, 1), bad, 4),
                Err(ColumnError::LineBreak)
            );
        }
    }

    #[test]
    fn deletes_exactly_the_block_cells() {
        let text = "abcdefgh\t= 3;\n0123456789ABCDEF\nabc\n\t\t\t\tx";
        let (result, rect) = run(text, |text, index| {
            delete_block(text, index, Rect::new((3, 14), (0, 10)), 4)
        });
        assert_eq!(result, "abcdefgh  3;\n0123456789EF\nabc\n\t\t    x");
        assert_eq!(rect, caret(0, 3, 10));
        let (inside, _) = run("a\tb", |text, index| {
            delete_block(text, index, Rect::new((0, 2), (0, 3)), 4)
        });
        assert_eq!(inside, "a  b");
    }

    #[test]
    fn backspace_does_nothing_at_column_zero_and_only_moves_in_virtual_space() {
        let (result, rect) = run("ab\ncd", |text, index| {
            backspace(text, index, caret(0, 1, 0), 4)
        });
        assert_eq!((result.as_str(), rect), ("ab\ncd", caret(0, 1, 0)));
        let (result, rect) = run("ab\ncd", |text, index| {
            backspace(text, index, caret(0, 1, 5), 4)
        });
        assert_eq!((result.as_str(), rect), ("ab\ncd", caret(0, 1, 4)));
        let (result, rect) = run("ab\nabcdef", |text, index| {
            backspace(text, index, caret(0, 1, 5), 4)
        });
        assert_eq!((result.as_str(), rect), ("ab\nabcdf", caret(0, 1, 4)));
    }

    #[test]
    fn backspace_deletes_equally_wide_characters_whole() {
        let (result, rect) = run("\tfoo\n\tbar\n", |text, index| {
            backspace(text, index, caret(0, 2, 4), 4)
        });
        assert_eq!((result.as_str(), rect), ("foo\nbar\n", caret(0, 2, 0)));
        let (result, rect) = run("ab\tc\nxy\tz", |text, index| {
            backspace(text, index, caret(0, 1, 4), 4)
        });
        assert_eq!((result.as_str(), rect), ("abc\nxyz", caret(0, 1, 2)));
    }

    #[test]
    fn backspace_deletes_one_cell_when_the_widths_differ() {
        let (result, rect) = run("\tfoo\n    bar\nab", |text, index| {
            backspace(text, index, caret(0, 2, 4), 4)
        });
        assert_eq!(
            (result.as_str(), rect),
            ("   foo\n   bar\nab", caret(0, 2, 3))
        );
        let (result, _) = run("\tfoo\nabc", |text, index| {
            backspace(text, index, caret(0, 1, 4), 4)
        });
        assert_eq!(
            result, "   foo\nabc",
            "a line ending inside the tab's cells"
        );
        let (result, _) = run("a\tb\nabcd", |text, index| {
            backspace(text, index, caret(0, 1, 3), 4)
        });
        assert_eq!(result, "a  b\nabd", "a caret inside a tab");
    }

    #[test]
    fn delete_forward_deletes_equally_wide_characters_and_never_joins_lines() {
        let (result, rect) = run("\tfoo\n\tbar\n", |text, index| {
            delete_forward(text, index, caret(0, 2, 0), 4)
        });
        assert_eq!((result.as_str(), rect), ("foo\nbar\n", caret(0, 2, 0)));
        let (result, rect) = run("ab\ncd", |text, index| {
            delete_forward(text, index, caret(0, 1, 2), 4)
        });
        assert_eq!((result.as_str(), rect), ("ab\ncd", caret(0, 1, 2)));
        let (result, _) = run("\tfoo\nabcde", |text, index| {
            delete_forward(text, index, caret(0, 1, 0), 4)
        });
        assert_eq!(result, "   foo\nbcde");
    }
}
