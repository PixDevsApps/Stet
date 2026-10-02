//! Large column edits as one replacement of their line range (ADR-003 amendment).

use super::editing::ColumnEdit;
use crate::text::edit::ByteCursor;
use crate::text::{LineIndex, TextEdit};
use std::ops::Range;

/// Above this many edits, a column edit goes to the buffer as one [`BulkEdit`].
pub const BULK_THRESHOLD: usize = 2_000;

/// A column edit as a single replacement of whole lines.
///
/// Replacing a range collapses the marks inside it to the range's start, in the edit and in its
/// undo and redo, so bookmarks on [`BulkEdit::marks_to_restore`] lines must be re-created on the
/// same line numbers after each of them. The numbers hold: a column edit adds or removes no
/// line inside the range, and the lines a paste appends come after the old last line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkEdit {
    /// Replaces the characters of `lines`, from the start of the first to the end of the last
    /// (without its `\n`).
    pub edit: TextEdit,
    /// The lines the edit rewrites, numbered before it.
    pub lines: Range<usize>,
    /// Lines the edit adds after the old last line (a paste past the end).
    pub appended_lines: usize,
}

impl BulkEdit {
    /// The lines among `marked_lines` whose marks the replacement collapses, sorted.
    pub fn marks_to_restore(&self, marked_lines: impl IntoIterator<Item = usize>) -> Vec<usize> {
        let mut lines: Vec<usize> = marked_lines
            .into_iter()
            .filter(|line| self.lines.contains(line))
            .collect();
        lines.sort_unstable();
        lines.dedup();
        lines
    }
}

impl ColumnEdit {
    /// Whether the edit has too many parts to go to the buffer one by one.
    pub fn needs_bulk(&self) -> bool {
        self.edits.len() > BULK_THRESHOLD
    }

    /// The edits as one replacement of the lines they touch, for the text they were made on.
    /// `None` when there are no edits.
    pub fn to_bulk(&self, text: &str, index: &LineIndex) -> Option<BulkEdit> {
        let (first, last) = (self.edits.first()?, self.edits.last()?);
        let lines = index.line_of_char(first.start)..index.line_of_char(last.end) + 1;
        let chars = index.line_chars(lines.start).start..index.line_chars(lines.end - 1).end;
        let bytes = index.line_bytes(lines.start).start..index.line_bytes(lines.end - 1).end;
        let old = &text[bytes];
        let mut new = String::with_capacity(old.len() + self.edits.len());
        let mut cursor = ByteCursor::new(old);
        let mut copied = 0;
        let mut appended_lines = 0;
        for edit in &self.edits {
            let start = cursor.byte_at(edit.start - chars.start);
            new.push_str(&old[copied..start]);
            new.push_str(&edit.insert);
            copied = cursor.byte_at(edit.end - chars.start);
            appended_lines += memchr::memchr_iter(b'\n', edit.insert.as_bytes()).count();
        }
        new.push_str(&old[copied..]);
        Some(BulkEdit {
            edit: TextEdit::new(chars, new),
            lines,
            appended_lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::column::{Block, Rect, insert, paste};
    use crate::text::apply;

    #[test]
    fn a_bulk_edit_rewrites_the_touched_lines_once() {
        let text = "keep\nab\ncd\n\nkeep";
        let index = LineIndex::new(text);
        let edit = insert(text, &index, Rect::new((1, 3), (3, 3)), "|", 4).unwrap();
        assert!(!edit.needs_bulk());
        let bulk = edit.to_bulk(text, &index).unwrap();
        assert_eq!(bulk.lines, 1..4);
        assert_eq!(bulk.edit, TextEdit::new(5..11, "ab |\ncd |\n   |"));
        assert_eq!(bulk.appended_lines, 0);
        assert_eq!(bulk.marks_to_restore([4, 2, 0, 3, 2]), [2, 3]);
        assert_eq!(
            apply(text, vec![bulk.edit]).unwrap(),
            apply(text, edit.edits).unwrap()
        );
    }

    #[test]
    fn a_paste_past_the_end_appends_lines() {
        let text = "a\nb";
        let index = LineIndex::new(text);
        let block = Block::from_text("1\n2\n3");
        let edit = paste(text, &index, Rect::new((1, 1), (1, 1)), &block, 4);
        let bulk = edit.to_bulk(text, &index).unwrap();
        assert_eq!((bulk.lines.clone(), bulk.appended_lines), (1..2, 2));
        assert_eq!(apply(text, vec![bulk.edit]).unwrap(), "a\nb1\n 2\n 3");
    }

    #[test]
    fn needs_bulk_above_the_threshold() {
        let text = "x\n".repeat(BULK_THRESHOLD);
        let index = LineIndex::new(&text);
        let rect = Rect::new((0, 0), (BULK_THRESHOLD - 1, 0));
        assert!(!insert(&text, &index, rect, "-", 4).unwrap().needs_bulk());
        let rect = Rect::new((0, 0), (BULK_THRESHOLD, 0));
        assert!(insert(&text, &index, rect, "-", 4).unwrap().needs_bulk());
        assert_eq!(
            ColumnEdit {
                edits: Vec::new(),
                rect
            }
            .to_bulk(&text, &index),
            None
        );
    }
}
