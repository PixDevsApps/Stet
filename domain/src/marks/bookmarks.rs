use std::ops::Range;

use crate::text::{LineIndex, TextEdit};

/// Bookmarked lines, sorted and unique.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Bookmarks {
    lines: Vec<usize>,
}

impl Bookmarks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(&self) -> &[usize] {
        &self.lines
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn contains(&self, line: usize) -> bool {
        self.lines.binary_search(&line).is_ok()
    }

    /// Returns whether the line was not bookmarked before.
    pub fn insert(&mut self, line: usize) -> bool {
        match self.lines.binary_search(&line) {
            Ok(_) => false,
            Err(at) => {
                self.lines.insert(at, line);
                true
            }
        }
    }

    /// Returns whether the line was bookmarked.
    pub fn remove(&mut self, line: usize) -> bool {
        match self.lines.binary_search(&line) {
            Ok(at) => {
                self.lines.remove(at);
                true
            }
            Err(_) => false,
        }
    }

    /// Toggle Bookmark (Ctrl+F2). Returns whether the line is bookmarked now.
    pub fn toggle(&mut self, line: usize) -> bool {
        if self.remove(line) {
            false
        } else {
            self.insert(line);
            true
        }
    }

    pub fn clear(&mut self) {
        self.lines.clear();
    }

    /// Next Bookmark (F2): the first bookmark below `line`, wrapping to the first one.
    pub fn next(&self, line: usize) -> Option<usize> {
        let index = self.lines.partition_point(|&bookmark| bookmark <= line);
        self.lines.get(index).or(self.lines.first()).copied()
    }

    /// Previous Bookmark (Shift+F2): the last bookmark above `line`, wrapping to the last one.
    pub fn prev(&self, line: usize) -> Option<usize> {
        let index = self.lines.partition_point(|&bookmark| bookmark < line);
        index
            .checked_sub(1)
            .map(|index| self.lines[index])
            .or(self.lines.last().copied())
    }

    /// The bookmarks within `lines`, for painting the visible part of the gutter.
    pub fn in_lines(&self, lines: Range<usize>) -> &[usize] {
        let start = self.lines.partition_point(|&line| line < lines.start);
        let end = self.lines.partition_point(|&line| line < lines.end);
        &self.lines[start..end.max(start)]
    }

    /// Inverse Bookmark: every line of a text of `line_count` lines that is not bookmarked.
    pub fn inverted(&self, line_count: usize) -> Self {
        let mut marked = self.lines.iter().peekable();
        let lines = (0..line_count)
            .filter(|line| {
                while marked.next_if(|&&marked| marked < *line).is_some() {}
                marked.peek() != Some(&line)
            })
            .collect();
        Self { lines }
    }

    /// Drops bookmarks at or past `line_count`.
    pub fn truncate(&mut self, line_count: usize) {
        let keep = self.lines.partition_point(|&line| line < line_count);
        self.lines.truncate(keep);
    }

    /// Moves bookmarks through a line mapping, for example a comparison of the text before and
    /// after a bulk edit. Lines mapped to `None` lose their bookmark.
    pub fn remap_with(&mut self, mut map: impl FnMut(usize) -> Option<usize>) {
        let mut lines: Vec<usize> = self.lines.iter().filter_map(|&line| map(line)).collect();
        lines.sort_unstable();
        lines.dedup();
        self.lines = lines;
    }

    /// Keeps bookmarks on their lines through `edits` (sorted and non-overlapping, as
    /// [`normalize`](crate::text::normalize) returns them) made to the text that `before`
    /// indexes.
    ///
    /// - Lines inserted or removed above a bookmark shift it.
    /// - Text inserted at the start of a bookmarked line pushes the bookmark down with the line.
    /// - A line whose content and line break are deleted loses its bookmark.
    /// - When a line break is deleted, the two lines merge and a bookmark on either part that
    ///   still has text survives on the merged line.
    pub fn remap(&mut self, edits: &[TextEdit], before: &LineIndex) {
        self.truncate(before.line_count());
        if edits.is_empty() {
            return;
        }
        let groups = Group::from_edits(edits, before);
        let mut next_group = 0;
        let mut shift = 0isize;
        let mut lines = Vec::with_capacity(self.lines.len());
        for &line in &self.lines {
            while let Some(group) = groups.get(next_group)
                && group.last_line < line
            {
                shift += group.line_delta();
                next_group += 1;
            }
            let span = before.line_chars(line);
            let mut target = Some(line as isize + shift);
            for group in groups[next_group..]
                .iter()
                .take_while(|group| group.first_line <= line)
            {
                if group.first_line < line {
                    let merged_from_above = group.last_line == line
                        && (group.end < span.end
                            || (group.end == span.start
                                && group.start == before.line_chars(group.first_line).start));
                    target = merged_from_above
                        .then(|| group.first_line as isize + shift + group.newlines as isize);
                } else if group.last_line > line {
                    if group.start == span.start {
                        target = None;
                    }
                } else if group.pure_insert && group.start == span.start {
                    target = target.map(|target| target + group.newlines as isize);
                }
                if target.is_none() {
                    break;
                }
            }
            lines.extend(target.map(|target| target as usize));
        }
        lines.sort_unstable();
        lines.dedup();
        self.lines = lines;
    }
}

impl FromIterator<usize> for Bookmarks {
    fn from_iter<I: IntoIterator<Item = usize>>(lines: I) -> Self {
        let mut lines: Vec<usize> = lines.into_iter().collect();
        lines.sort_unstable();
        lines.dedup();
        Self { lines }
    }
}

impl Extend<usize> for Bookmarks {
    fn extend<I: IntoIterator<Item = usize>>(&mut self, lines: I) {
        self.lines.extend(lines);
        self.lines.sort_unstable();
        self.lines.dedup();
    }
}

/// Adjacent edits act as one: their deleted ranges join into one span.
struct Group {
    start: usize,
    end: usize,
    first_line: usize,
    last_line: usize,
    newlines: usize,
    pure_insert: bool,
}

impl Group {
    fn from_edits(edits: &[TextEdit], before: &LineIndex) -> Vec<Self> {
        debug_assert!(edits.windows(2).all(|pair| pair[0].end <= pair[1].start));
        let mut groups: Vec<Self> = Vec::new();
        for edit in edits {
            let newlines = memchr::memchr_iter(b'\n', edit.insert.as_bytes()).count();
            match groups.last_mut() {
                Some(group) if group.end == edit.start => {
                    group.end = edit.end;
                    group.last_line = before.line_of_char(edit.end);
                    group.newlines += newlines;
                    group.pure_insert &= edit.start == edit.end;
                }
                _ => groups.push(Self {
                    start: edit.start,
                    end: edit.end,
                    first_line: before.line_of_char(edit.start),
                    last_line: before.line_of_char(edit.end),
                    newlines,
                    pure_insert: edit.start == edit.end,
                }),
            }
        }
        groups
    }

    /// How many lines longer the text is after the group.
    fn line_delta(&self) -> isize {
        self.newlines as isize - (self.last_line - self.first_line) as isize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::{Bias, apply, map_offset, normalize};
    use proptest::prelude::*;

    fn remapped(text: &str, lines: &[usize], edits: Vec<TextEdit>) -> Vec<usize> {
        let index = LineIndex::new(text);
        let edits = normalize(edits, index.len_chars()).unwrap();
        let mut bookmarks: Bookmarks = lines.iter().copied().collect();
        bookmarks.remap(&edits, &index);
        bookmarks.lines().to_vec()
    }

    #[test]
    fn toggles_and_navigates_with_wrapping() {
        let mut bookmarks = Bookmarks::new();
        assert_eq!(bookmarks.next(3), None);
        assert!(bookmarks.toggle(5));
        assert!(bookmarks.toggle(2));
        assert!(bookmarks.toggle(9));
        assert!(!bookmarks.toggle(9));
        assert_eq!(bookmarks.lines(), &[2, 5]);
        assert_eq!(bookmarks.next(2), Some(5));
        assert_eq!(bookmarks.next(5), Some(2));
        assert_eq!(bookmarks.prev(5), Some(2));
        assert_eq!(bookmarks.prev(2), Some(5));
        assert_eq!(bookmarks.prev(0), Some(5));
        assert_eq!(bookmarks.in_lines(3..9), &[5]);
        assert_eq!(bookmarks.in_lines(6..9), &[] as &[usize]);
        bookmarks.clear();
        assert!(bookmarks.is_empty());
        let single: Bookmarks = [4].into_iter().collect();
        assert_eq!(single.next(4), Some(4));
    }

    #[test]
    fn inverts_within_the_line_count() {
        let bookmarks: Bookmarks = [0, 2, 3, 9].into_iter().collect();
        assert_eq!(bookmarks.inverted(6).lines(), &[1, 4, 5]);
        assert_eq!(Bookmarks::new().inverted(2).lines(), &[0, 1]);
    }

    #[test]
    fn lines_inserted_above_shift_a_bookmark() {
        assert_eq!(
            remapped("a\nb\nc", &[2], vec![TextEdit::insert(0, "x\ny\n")]),
            [4]
        );
        assert_eq!(
            remapped("a\nb\nc", &[2], vec![TextEdit::insert(1, "\nx")]),
            [3]
        );
    }

    #[test]
    fn deleting_a_whole_line_drops_its_bookmark() {
        assert_eq!(
            remapped("a\nb\nc", &[1, 2], vec![TextEdit::delete(2..4)]),
            [1]
        );
        assert_eq!(
            remapped("a\nb", &[1], vec![TextEdit::delete(1..3)]),
            [] as [usize; 0]
        );
        assert_eq!(
            remapped("a\n", &[1], vec![TextEdit::delete(1..2)]),
            [] as [usize; 0]
        );
        assert_eq!(
            remapped("a\nb\n", &[1, 2], vec![TextEdit::delete(2..4)]),
            [1]
        );
        assert_eq!(
            remapped("x\ny\nz", &[1, 2], vec![TextEdit::delete(2..5)]),
            [] as [usize; 0]
        );
        assert_eq!(remapped("a\nb\nc", &[1], vec![TextEdit::delete(2..3)]), [1]);
    }

    #[test]
    fn enter_at_the_start_of_a_line_moves_the_bookmark_with_the_text() {
        assert_eq!(
            remapped("ab\ncd", &[1], vec![TextEdit::insert(3, "\n")]),
            [2]
        );
        assert_eq!(
            remapped("ab\ncd", &[1], vec![TextEdit::insert(4, "\n")]),
            [1]
        );
        assert_eq!(
            remapped("ab\ncd", &[1], vec![TextEdit::new(3..4, "x\ny")]),
            [1]
        );
    }

    #[test]
    fn merged_lines_keep_a_bookmark_on_the_surviving_text() {
        assert_eq!(remapped("a\nb", &[0, 1], vec![TextEdit::delete(1..2)]), [0]);
        assert_eq!(remapped("a\nb", &[1], vec![TextEdit::delete(1..2)]), [0]);
        assert_eq!(
            remapped("abc\ndef\nghi", &[0, 1, 2], vec![TextEdit::delete(1..9)]),
            [0]
        );
        assert_eq!(
            remapped("abc\ndef\nghi", &[2], vec![TextEdit::new(0..9, "x\n\ny")]),
            [2]
        );
        assert_eq!(
            remapped("\nb", &[0], vec![TextEdit::delete(0..1)]),
            [] as [usize; 0]
        );
    }

    #[test]
    fn adjacent_edits_count_as_one_deletion() {
        let edits = vec![TextEdit::delete(2..3), TextEdit::delete(3..4)];
        assert_eq!(remapped("a\nb\nc", &[1, 2], edits), [1]);
    }

    #[test]
    fn follows_a_mapping_and_drops_stale_lines() {
        let mut bookmarks: Bookmarks = [1, 4, 7].into_iter().collect();
        bookmarks.remap_with(|line| (line != 4).then_some(line * 2));
        assert_eq!(bookmarks.lines(), &[2, 14]);
        bookmarks.truncate(10);
        assert_eq!(bookmarks.lines(), &[2]);
        bookmarks.extend([0, 2]);
        assert_eq!(bookmarks.lines(), &[0, 2]);
    }

    fn edits_for(len: usize) -> impl Strategy<Value = Vec<TextEdit>> {
        prop::collection::vec((0..=len, 0..=len, "[ab\n]{0,3}"), 0..5).prop_map(|raw| {
            let mut cuts: Vec<(usize, usize, String)> = raw
                .into_iter()
                .map(|(a, b, s)| (a.min(b), a.max(b), s))
                .collect();
            cuts.sort();
            let mut last_end = 0;
            cuts.into_iter()
                .filter_map(|(start, end, insert)| {
                    (start >= last_end).then(|| {
                        last_end = end;
                        TextEdit::new(start..end, insert)
                    })
                })
                .collect()
        })
    }

    proptest! {
        #[test]
        fn untouched_bookmarked_lines_keep_their_text(
            (text, edits, marked) in "[a-c\n]{0,30}".prop_flat_map(|text| {
                let len = text.chars().count();
                let lines = text.matches('\n').count() + 1;
                (Just(text), edits_for(len), prop::collection::vec(0..lines, 0..6))
            })
        ) {
            let before = LineIndex::new(&text);
            let edits = normalize(edits, before.len_chars()).unwrap();
            let after_text = apply(&text, edits.clone()).unwrap();
            let after = LineIndex::new(&after_text);
            let mut bookmarks: Bookmarks = marked.iter().copied().collect();
            bookmarks.remap(&edits, &before);
            prop_assert!(bookmarks.lines().windows(2).all(|pair| pair[0] < pair[1]));
            prop_assert!(bookmarks.lines().iter().all(|&line| line < after.line_count()));
            for &line in &marked {
                let span = before.line_chars(line);
                if edits.iter().all(|edit| edit.end < span.start || edit.start > span.end) {
                    let moved = after.line_of_char(map_offset(span.start, &edits, Bias::Before));
                    prop_assert!(bookmarks.contains(moved), "line {line} lost its bookmark");
                    prop_assert_eq!(
                        &after_text[after.line_bytes(moved)],
                        &text[before.line_bytes(line)]
                    );
                }
            }
        }
    }
}
