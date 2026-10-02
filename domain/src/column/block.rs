//! The rectangular clipboard: copying a block and pasting it at a column.

use super::editing::{ColumnEdit, edit_lines, push_spaces, splice};
use super::rect::{Rect, clamp_lines};
use super::visual::{locate, text_width};
use crate::text::{LineIndex, TextEdit};

/// The clipboard MIME type of a copied block (ADR-016); `text/plain` carries the same text.
pub const MIME_TYPE: &str = "application/x-stet-column-block";

/// A copied block: one row per line of the rectangle, without line breaks.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Block {
    pub rows: Vec<String>,
}

impl Block {
    /// The rows joined with `\n`: the clipboard text under [`MIME_TYPE`] and `text/plain`.
    pub fn to_text(&self) -> String {
        self.rows.join("\n")
    }

    /// The block whose clipboard text is `text`.
    pub fn from_text(text: &str) -> Self {
        Self {
            rows: text.split('\n').map(str::to_owned).collect(),
        }
    }

    /// The rows of plain text from another application's clipboard: line endings of every kind
    /// end a row, and one line break at the very end ends the last row instead of starting an
    /// empty one. One row means the text is typed on every line of a rectangle; several are
    /// pasted as a block (ADR-016 amendment).
    pub fn from_plain_text(text: &str) -> Self {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = text.strip_suffix('\n').unwrap_or(&text);
        Self::from_text(text)
    }

    /// The cells of the widest row when the rows start at visual column `column`.
    pub fn width(&self, column: usize, tab_width: usize) -> usize {
        self.rows
            .iter()
            .map(|row| text_width(row, column, tab_width))
            .max()
            .unwrap_or(0)
    }
}

/// The rectangle's cells on each of its lines. A tab that an edge cuts contributes the spaces
/// it is drawn as inside the rectangle; virtual space contributes nothing, so a row from a line
/// that ends inside the rectangle is shorter.
pub fn copy(text: &str, index: &LineIndex, rect: Rect, tab_width: usize) -> Block {
    let (left, right) = (rect.left(), rect.right());
    let rows = clamp_lines(index, rect.lines())
        .map(|line| row(&text[index.line_bytes(line)], left, right, tab_width))
        .collect();
    Block { rows }
}

fn row(line: &str, left: usize, right: usize, tab_width: usize) -> String {
    let start = locate(line, left, tab_width);
    if right <= left || start.at_end(line) {
        return String::new();
    }
    let end = locate(line, right, tab_width);
    let mut row = String::new();
    if start.inside_tab.is_some() && end.byte_offset == start.byte_offset {
        push_spaces(&mut row, right - left);
        return row;
    }
    let from = match start.inside_tab {
        Some(cut) => {
            push_spaces(&mut row, cut.after);
            start.byte_offset + 1
        }
        None => start.byte_offset,
    };
    row.push_str(&line[from..end.byte_offset]);
    if let Some(cut) = end.inside_tab {
        push_spaces(&mut row, cut.before);
    }
    row
}

/// Pastes `block` with its first row at the rectangle's top line and left column, and each
/// further row on the next line, after deleting the rectangle's cells when it is not a caret.
///
/// Rows start exactly at the column, as typing does. A row is padded with spaces to the width
/// of the block's widest row when its line goes on after it, so the text to the right stays
/// aligned. Rows past the end of the text go on new lines, and a row that contains a line break
/// counts as several rows. The result is a column caret after the pasted rows.
pub fn paste(
    text: &str,
    index: &LineIndex,
    rect: Rect,
    block: &Block,
    tab_width: usize,
) -> ColumnEdit {
    let split;
    let rows = if block.rows.iter().any(|row| row.contains('\n')) {
        split = Block::from_text(&block.to_text());
        &split.rows
    } else {
        &block.rows
    };
    let (top, left, right) = (rect.top(), rect.left(), rect.right());
    let width = rows
        .iter()
        .map(|row| text_width(row, left, tab_width))
        .max()
        .unwrap_or(0);
    let end = rect.lines().end.max(top.saturating_add(rows.len()));
    let mut edits = edit_lines(text, index, top..end, |line, line_text| {
        let right = if line <= rect.bottom() { right } else { left };
        match rows.get(line - top) {
            Some(row) => splice(line_text, left, right, row, width, tab_width),
            None => splice(line_text, left, right, "", 0, tab_width),
        }
    });
    let lines = index.line_count();
    if !rows.is_empty() && top + rows.len() > lines {
        let mut appended = String::new();
        for line in lines..top + rows.len() {
            appended.push('\n');
            if let Some(row) = line.checked_sub(top).map(|i| &rows[i])
                && !row.is_empty()
            {
                push_spaces(&mut appended, left);
                appended.push_str(row);
            }
        }
        let text_end = index.len_chars();
        match edits.last_mut() {
            Some(last) if index.line_of_char(last.start) + 1 == lines => {
                let rest = index.char_to_byte(text, last.end);
                last.insert.push_str(&text[rest..]);
                last.insert.push_str(&appended);
                last.end = text_end;
            }
            _ => edits.push(TextEdit::insert(text_end, appended)),
        }
    }
    let caret = left + width;
    let last_row = top + rows.len().saturating_sub(1);
    ColumnEdit {
        edits,
        rect: Rect::new((last_row, caret), (top, caret)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::apply;
    use proptest::prelude::*;

    fn paste_into(text: &str, rect: Rect, rows: &[&str]) -> (String, Rect) {
        let index = LineIndex::new(text);
        let block = Block {
            rows: rows.iter().map(|row| (*row).to_owned()).collect(),
        };
        let edit = paste(text, &index, rect, &block, 4);
        (apply(text, edit.edits).unwrap(), edit.rect)
    }

    #[test]
    fn copies_cells_with_cut_tabs_as_spaces_and_no_virtual_space() {
        let text = "abcdefgh\t= 3;\n0123456789ABCDEF\nabc\n\tx\t\ty\n日本語のテキストです";
        let index = LineIndex::new(text);
        let block = copy(text, &index, Rect::new((0, 2), (4, 10)), 4);
        assert_eq!(
            block.rows,
            ["cdefgh  ", "23456789", "c", "  x\t  ", "語のテキストです"]
        );
        assert_eq!(Block::from_text(&block.to_text()), block);
        assert_eq!(
            copy(
                "a\tb",
                &LineIndex::new("a\tb"),
                Rect::new((0, 2), (0, 3)),
                4
            )
            .rows,
            [" "]
        );
    }

    #[test]
    fn pastes_rows_at_the_column_padding_rows_that_have_text_after_them() {
        let (result, rect) = paste_into(
            "0123456789\nab\n0123456789",
            Rect::new((0, 4), (0, 4)),
            &["xy", "", "z"],
        );
        assert_eq!(result, "0123xy456789\nab\n0123z 456789");
        assert_eq!(rect, Rect::new((2, 6), (0, 6)));
    }

    #[test]
    fn pasting_past_the_end_appends_lines() {
        let (result, rect) = paste_into("ab\ncd", Rect::new((1, 3), (1, 3)), &["1", "", "3"]);
        assert_eq!(result, "ab\ncd 1\n\n   3");
        assert_eq!(rect, Rect::new((3, 4), (1, 4)));
        let (result, _) = paste_into("", Rect::new((2, 1), (2, 1)), &["x"]);
        assert_eq!(result, "\n\n x");
    }

    #[test]
    fn pasting_over_a_block_replaces_it() {
        let (result, rect) = paste_into(
            "0123456789\n0123456789\n0123456789",
            Rect::new((0, 2), (2, 5)),
            &["ab", "c"],
        );
        assert_eq!(result, "01ab56789\n01c 56789\n0156789");
        assert_eq!(rect, Rect::new((1, 4), (0, 4)));
    }

    #[test]
    fn a_row_with_a_line_break_counts_as_several_rows() {
        let (result, _) = paste_into("ab\ncd", Rect::new((0, 1), (0, 1)), &["x\ny"]);
        assert_eq!(result, "axb\ncyd");
    }

    #[test]
    fn plain_text_rows_end_at_any_line_ending_and_ignore_one_final_break() {
        let rows = |text: &str| Block::from_plain_text(text).rows;
        assert_eq!(rows("abc"), ["abc"]);
        assert_eq!(rows("abc\n"), ["abc"]);
        assert_eq!(rows("a\r\nb\rc\n"), ["a", "b", "c"]);
        assert_eq!(rows("a\n\n"), ["a", ""]);
        assert_eq!(rows("\n"), [""]);
        assert_eq!(rows(""), [""]);
    }

    proptest! {
        #[test]
        fn plain_text_rows_join_back_to_the_normalized_text(
            parts in prop::collection::vec(
                prop::sample::select(vec!["a", "é", "\t", " ", "\n", "\r\n", "\r"]),
                0..12,
            ),
        ) {
            let text: String = parts.concat();
            let block = Block::from_plain_text(&text);
            prop_assert!(!block.rows.is_empty());
            for row in &block.rows {
                prop_assert!(!row.contains(['\n', '\r']));
            }
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            let expected = normalized.strip_suffix('\n').unwrap_or(&normalized);
            prop_assert_eq!(block.to_text(), expected);
            if !text.contains(['\n', '\r']) {
                prop_assert_eq!(block.rows, vec![text]);
            }
        }
    }
}
