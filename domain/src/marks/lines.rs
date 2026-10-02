//! The bookmarked-line operations of Search > Bookmark. Each returns edits to apply as one undo
//! step (as a bulk edit above the ADR-003 threshold) and the bookmarks afterwards.
//!
//! Deleting lines keeps whether the text ends with a line break: a non-empty last line goes
//! together with the line break before it, and the empty line after a final line break stays.

use std::ops::Range;

use crate::text::{LineIndex, TextEdit};

use super::Bookmarks;

/// Edits to apply as one undo step, and the bookmarks once they are applied.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LineEdit {
    pub edits: Vec<TextEdit>,
    pub bookmarks: Bookmarks,
}

/// Cut Bookmarked Lines: the text for the clipboard and the edit that removes the lines.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Cut {
    pub clipboard: String,
    pub edit: LineEdit,
}

/// Copy Bookmarked Lines: each bookmarked line with its line break, in order.
pub fn copy_bookmarked_lines(text: &str, index: &LineIndex, bookmarks: &Bookmarks) -> String {
    let last = index.line_count() - 1;
    let mut clipboard = String::new();
    for &line in bookmarks.lines().iter().take_while(|&&line| line <= last) {
        clipboard.push_str(&text[index.line_bytes(line)]);
        if line < last {
            clipboard.push('\n');
        }
    }
    clipboard
}

pub fn cut_bookmarked_lines(text: &str, index: &LineIndex, bookmarks: &Bookmarks) -> Cut {
    Cut {
        clipboard: copy_bookmarked_lines(text, index, bookmarks),
        edit: delete_bookmarked_lines(index, bookmarks),
    }
}

/// Remove Bookmarked Lines.
pub fn delete_bookmarked_lines(index: &LineIndex, bookmarks: &Bookmarks) -> LineEdit {
    LineEdit {
        edits: delete_lines(index, bookmarks.lines().iter().copied()),
        bookmarks: Bookmarks::new(),
    }
}

/// Remove Unbookmarked Lines: keeps only the bookmarked lines, which stay bookmarked.
pub fn delete_unbookmarked_lines(index: &LineIndex, bookmarks: &Bookmarks) -> LineEdit {
    let line_count = index.line_count();
    let mut marked = bookmarks.lines().iter().copied().peekable();
    let unmarked = (0..line_count).filter(|&line| {
        while marked.next_if(|&marked| marked < line).is_some() {}
        marked.peek() != Some(&line)
    });
    let edits = delete_lines(index, unmarked);
    let kept = bookmarks.lines().partition_point(|&line| line < line_count);
    LineEdit {
        edits,
        bookmarks: (0..kept).collect(),
    }
}

/// Paste to (Replace) Bookmarked Lines: the text of every bookmarked line becomes `clipboard`
/// (LF-normalized). One final line break of the clipboard is dropped, since each line keeps its
/// own. The bookmarks stay on the first line of each pasted block.
pub fn paste_to_bookmarked_lines(
    index: &LineIndex,
    bookmarks: &Bookmarks,
    clipboard: &str,
) -> LineEdit {
    let pasted = clipboard.strip_suffix('\n').unwrap_or(clipboard);
    let added_lines = memchr::memchr_iter(b'\n', pasted.as_bytes()).count();
    let line_count = index.line_count();
    let marked = bookmarks
        .lines()
        .iter()
        .take_while(|&&line| line < line_count);
    let edits = marked
        .clone()
        .map(|&line| TextEdit::new(index.line_chars(line), pasted))
        .filter(|edit| !edit.is_noop())
        .collect();
    let bookmarks = marked
        .enumerate()
        .map(|(before, &line)| line + before * added_lines)
        .collect();
    LineEdit { edits, bookmarks }
}

/// Deletions for `lines` (sorted), merged where they touch. A non-empty last line takes the
/// line break before the run of deleted lines that ends with it.
fn delete_lines(index: &LineIndex, lines: impl IntoIterator<Item = usize>) -> Vec<TextEdit> {
    fn push(ranges: &mut Vec<Range<usize>>, range: Range<usize>) {
        match ranges.last_mut() {
            Some(previous) if previous.end >= range.start => {
                previous.end = previous.end.max(range.end);
            }
            _ => ranges.push(range),
        }
    }

    let last = index.line_count() - 1;
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for line in lines {
        if line > last {
            break;
        }
        let span = index.line_chars(line);
        if line < last {
            push(&mut ranges, span.start..index.line_chars(line + 1).start);
        } else if !span.is_empty() {
            push(&mut ranges, span);
            if let Some(run) = ranges.pop() {
                push(&mut ranges, run.start.saturating_sub(1)..run.end);
            }
        }
    }
    ranges.into_iter().map(TextEdit::delete).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marks::bookmark_lines_of;
    use crate::text::apply;
    use proptest::prelude::*;

    fn bookmarks(lines: &[usize]) -> Bookmarks {
        lines.iter().copied().collect()
    }

    fn after(text: &str, edit: &LineEdit) -> String {
        apply(text, edit.edits.clone()).unwrap()
    }

    #[test]
    fn copies_bookmarked_lines_with_their_line_breaks() {
        let text = "one\ntwo\nthree";
        let index = LineIndex::new(text);
        assert_eq!(
            copy_bookmarked_lines(text, &index, &bookmarks(&[0, 2])),
            "one\nthree"
        );
        assert_eq!(
            copy_bookmarked_lines(text, &index, &bookmarks(&[1, 7])),
            "two\n"
        );
        let cut = cut_bookmarked_lines(text, &index, &bookmarks(&[1]));
        assert_eq!(cut.clipboard, "two\n");
        assert_eq!(after(text, &cut.edit), "one\nthree");
        assert!(cut.edit.bookmarks.is_empty());
    }

    #[test]
    fn deleting_lines_keeps_the_final_line_break_state() {
        let cases: [(&str, &[usize], &str); 8] = [
            ("a\nb\nc", &[1], "a\nc"),
            ("a\nb\nc", &[2], "a\nb"),
            ("a\nb\nc", &[1, 2], "a"),
            ("a\nb\nc", &[0, 1, 2], ""),
            ("a\nb\n", &[1], "a\n"),
            ("a\nb\n", &[0, 1, 2], ""),
            ("a\n", &[1], "a\n"),
            ("only", &[0], ""),
        ];
        for (text, lines, expected) in cases {
            let edit = delete_bookmarked_lines(&LineIndex::new(text), &bookmarks(lines));
            assert_eq!(after(text, &edit), expected, "{text:?} without {lines:?}");
            assert!(edit.bookmarks.is_empty());
        }
    }

    #[test]
    fn merges_neighbouring_deletions_into_one_edit() {
        let text = "a\nb\nc\nd\ne";
        let edit = delete_bookmarked_lines(&LineIndex::new(text), &bookmarks(&[1, 2, 4]));
        assert_eq!(
            edit.edits,
            vec![TextEdit::delete(2..6), TextEdit::delete(7..9)]
        );
    }

    #[test]
    fn mark_bookmark_then_remove_unmarked_lines_is_one_edit_set() {
        let log = "info start\nERROR disk full\ninfo retry\nERROR disk full again\ninfo done\n";
        let index = LineIndex::new(log);
        let matches: Vec<Range<usize>> = log
            .match_indices("ERROR")
            .map(|(byte, found)| {
                let start = index.byte_to_char(log, byte);
                start..start + found.chars().count()
            })
            .collect();
        let marked: Bookmarks = bookmark_lines_of(&matches, &index).into_iter().collect();
        assert_eq!(marked.lines(), &[1, 3]);
        let edit = delete_unbookmarked_lines(&index, &marked);
        assert_eq!(
            after(log, &edit),
            "ERROR disk full\nERROR disk full again\n"
        );
        assert_eq!(edit.bookmarks.lines(), &[0, 1]);
    }

    #[test]
    fn inverse_then_remove_is_the_other_removal() {
        let text = "a\nb\nc\nd";
        let index = LineIndex::new(text);
        let marked = bookmarks(&[0, 2]);
        let kept = delete_unbookmarked_lines(&index, &marked);
        let removed = delete_bookmarked_lines(&index, &marked.inverted(index.line_count()));
        assert_eq!(after(text, &kept), "a\nc");
        assert_eq!(after(text, &kept), after(text, &removed));
    }

    #[test]
    fn pastes_over_bookmarked_lines() {
        let text = "a\nb\nc\n";
        let index = LineIndex::new(text);
        let edit = paste_to_bookmarked_lines(&index, &bookmarks(&[0, 2]), "x\ny\n");
        assert_eq!(after(text, &edit), "x\ny\nb\nx\ny\n");
        assert_eq!(edit.bookmarks.lines(), &[0, 3]);
        let edit = paste_to_bookmarked_lines(&index, &bookmarks(&[1]), "new");
        assert_eq!(after(text, &edit), "a\nnew\nc\n");
        assert_eq!(edit.bookmarks.lines(), &[1]);
    }

    fn expected_without(text: &str, deleted: &[bool]) -> String {
        let lines: Vec<&str> = text.split('\n').collect();
        let last = lines.len() - 1;
        let kept = |line: usize| !deleted[line];
        if lines[last].is_empty() {
            (0..last)
                .filter(|&line| kept(line))
                .map(|line| format!("{}\n", lines[line]))
                .collect()
        } else {
            (0..=last)
                .filter(|&line| kept(line))
                .map(|line| lines[line])
                .collect::<Vec<_>>()
                .join("\n")
        }
    }

    proptest! {
        #[test]
        fn removals_keep_exactly_the_other_lines(
            (text, marked) in "[a-cé\n]{0,24}".prop_flat_map(|text| {
                let lines = text.matches('\n').count() + 1;
                (Just(text), prop::collection::vec(0..lines, 0..8))
            })
        ) {
            let index = LineIndex::new(&text);
            let marked: Bookmarks = marked.into_iter().collect();
            let is_marked: Vec<bool> = (0..index.line_count()).map(|line| marked.contains(line)).collect();
            let is_unmarked: Vec<bool> = is_marked.iter().map(|marked| !marked).collect();

            let removed = delete_bookmarked_lines(&index, &marked);
            prop_assert_eq!(after(&text, &removed), expected_without(&text, &is_marked));

            let kept = delete_unbookmarked_lines(&index, &marked);
            let result = after(&text, &kept);
            prop_assert_eq!(&result, &expected_without(&text, &is_unmarked));
            let result_index = LineIndex::new(&result);
            let copied = copy_bookmarked_lines(&text, &index, &marked);
            let recopied = copy_bookmarked_lines(&result, &result_index, &kept.bookmarks);
            prop_assert_eq!(copied.trim_end_matches('\n'), recopied.trim_end_matches('\n'));
        }
    }
}
