use std::ops::Range;

use crate::text::{Bias, LineIndex, TextEdit};

/// The Mark dialog's style and the five style tokens (Search > Style All Occurrences of Token).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MarkStyle {
    Mark,
    Token1,
    Token2,
    Token3,
    Token4,
    Token5,
}

impl MarkStyle {
    pub const ALL: [Self; 6] = [
        Self::Mark,
        Self::Token1,
        Self::Token2,
        Self::Token3,
        Self::Token4,
        Self::Token5,
    ];

    pub const TOKENS: [Self; 5] = [
        Self::Token1,
        Self::Token2,
        Self::Token3,
        Self::Token4,
        Self::Token5,
    ];

    /// Style token `number`, from 1 to 5.
    pub const fn token(number: usize) -> Option<Self> {
        match number {
            1..=5 => Some(Self::TOKENS[number - 1]),
            _ => None,
        }
    }

    pub const fn index(self) -> usize {
        self as usize
    }

    /// A stable id for text tags and scheme styles.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Mark => "mark",
            Self::Token1 => "token-1",
            Self::Token2 => "token-2",
            Self::Token3 => "token-3",
            Self::Token4 => "token-4",
            Self::Token5 => "token-5",
        }
    }
}

/// Marked character ranges per style, each list sorted and non-overlapping. Empty matches are
/// never stored; they can still bookmark their line through [`bookmark_lines_of`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Marks {
    ranges: [Vec<Range<usize>>; 6],
}

impl Marks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn ranges(&self, style: MarkStyle) -> &[Range<usize>] {
        &self.ranges[style.index()]
    }

    pub fn count(&self, style: MarkStyle) -> usize {
        self.ranges(style).len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.iter().all(Vec::is_empty)
    }

    /// Adds marks, joining ranges that overlap existing ones (Mark All without Purge).
    pub fn add(&mut self, style: MarkStyle, ranges: impl IntoIterator<Item = Range<usize>>) {
        let mut added: Vec<Range<usize>> = ranges
            .into_iter()
            .filter(|range| range.start < range.end)
            .collect();
        added.sort_unstable_by_key(|range| (range.start, range.end));
        let existing = std::mem::take(&mut self.ranges[style.index()]);
        let mut merged: Vec<Range<usize>> = Vec::with_capacity(existing.len() + added.len());
        let (mut old, mut new) = (
            existing.into_iter().peekable(),
            added.into_iter().peekable(),
        );
        loop {
            let next = match (old.peek(), new.peek()) {
                (Some(a), Some(b)) if (a.start, a.end) <= (b.start, b.end) => old.next(),
                (Some(_), Some(_)) | (None, Some(_)) => new.next(),
                (Some(_), None) => old.next(),
                (None, None) => break,
            };
            let Some(range) = next else { break };
            match merged.last_mut() {
                Some(last) if range.start < last.end => last.end = last.end.max(range.end),
                _ => merged.push(range),
            }
        }
        self.ranges[style.index()] = merged;
    }

    /// Replaces the marks of `style` (Mark All with Purge for each search).
    pub fn set(&mut self, style: MarkStyle, ranges: impl IntoIterator<Item = Range<usize>>) {
        self.clear(style);
        self.add(style, ranges);
    }

    pub fn clear(&mut self, style: MarkStyle) {
        self.ranges[style.index()].clear();
    }

    pub fn clear_all(&mut self) {
        self.ranges.iter_mut().for_each(Vec::clear);
    }

    /// Jump Down: the first mark of `style` that starts after `offset`, wrapping to the first.
    pub fn next(&self, style: MarkStyle, offset: usize) -> Option<Range<usize>> {
        let ranges = self.ranges(style);
        let index = ranges.partition_point(|range| range.start <= offset);
        ranges.get(index).or(ranges.first()).cloned()
    }

    /// Jump Up: the last mark of `style` that starts before `offset`, wrapping to the last.
    pub fn prev(&self, style: MarkStyle, offset: usize) -> Option<Range<usize>> {
        let ranges = self.ranges(style);
        let index = ranges.partition_point(|range| range.start < offset);
        index
            .checked_sub(1)
            .map(|index| ranges[index].clone())
            .or(ranges.last().cloned())
    }

    /// Moves marks through `edits` (sorted and non-overlapping). Text inserted inside a mark
    /// joins it, text inserted at either end does not, and marks deleted entirely are dropped.
    pub fn remap(&mut self, edits: &[TextEdit]) {
        if edits.is_empty() {
            return;
        }
        let inserted: Vec<usize> = edits
            .iter()
            .map(|edit| edit.insert.chars().count())
            .collect();
        for ranges in &mut self.ranges {
            let mut map = OffsetMap::new(edits, &inserted);
            ranges.retain_mut(|range| {
                let start = map.map(range.start, Bias::After);
                let end = map.map(range.end, Bias::Before).max(start);
                *range = start..end;
                start < end
            });
        }
    }

    pub fn bookmark_lines(&self, style: MarkStyle, index: &LineIndex) -> Vec<usize> {
        bookmark_lines_of(self.ranges(style), index)
    }
}

/// The lines to bookmark for matches in `ranges` (character offsets): every line a match spans,
/// and the line of an empty match. Sorted and unique.
pub fn bookmark_lines_of(ranges: &[Range<usize>], index: &LineIndex) -> Vec<usize> {
    let mut lines = Vec::new();
    for range in ranges {
        let first = index.line_of_char(range.start);
        let last = index.line_of_char(range.end.saturating_sub(1).max(range.start));
        lines.extend(first..=last);
    }
    lines.sort_unstable();
    lines.dedup();
    lines
}

/// Maps a non-decreasing sequence of offsets through normalized edits in one pass.
struct OffsetMap<'a> {
    edits: &'a [TextEdit],
    inserted: &'a [usize],
    next: usize,
    shift: isize,
}

impl<'a> OffsetMap<'a> {
    fn new(edits: &'a [TextEdit], inserted: &'a [usize]) -> Self {
        Self {
            edits,
            inserted,
            next: 0,
            shift: 0,
        }
    }

    /// Where `offset` ends up, as [`map_offset`](crate::text::map_offset) computes it.
    fn map(&mut self, offset: usize, bias: Bias) -> usize {
        while let Some(edit) = self.edits.get(self.next) {
            if offset < edit.start || (offset == edit.start && bias == Bias::Before) {
                break;
            }
            let inserted = self.inserted[self.next];
            if offset < edit.end || (offset == edit.end && edit.start == edit.end) {
                let start = (edit.start as isize + self.shift) as usize;
                return match bias {
                    Bias::Before => start,
                    Bias::After => start + inserted,
                };
            }
            self.shift += inserted as isize - (edit.end - edit.start) as isize;
            self.next += 1;
        }
        (offset as isize + self.shift) as usize
    }
}

#[cfg(test)]
#[allow(
    clippy::single_range_in_vec_init,
    reason = "single marks are one-element lists of ranges"
)]
mod tests {
    use super::*;
    use crate::text::{apply, map_offset, normalize};
    use proptest::prelude::*;

    #[test]
    fn style_ids_are_stable() {
        assert_eq!(MarkStyle::token(3), Some(MarkStyle::Token3));
        assert_eq!(MarkStyle::token(0), None);
        assert_eq!(MarkStyle::token(6), None);
        let names: Vec<&str> = MarkStyle::ALL.iter().map(|style| style.name()).collect();
        assert_eq!(
            names,
            [
                "mark", "token-1", "token-2", "token-3", "token-4", "token-5"
            ]
        );
        assert!(
            MarkStyle::ALL
                .iter()
                .enumerate()
                .all(|(i, style)| style.index() == i)
        );
    }

    #[test]
    fn adding_joins_overlaps_and_keeps_neighbours_apart() {
        let mut marks = Marks::new();
        marks.add(MarkStyle::Mark, [10..12, 0..3, 3..5, 4..4]);
        assert_eq!(marks.ranges(MarkStyle::Mark), &[0..3, 3..5, 10..12]);
        marks.add(MarkStyle::Mark, [2..4, 11..20]);
        assert_eq!(marks.ranges(MarkStyle::Mark), &[0..5, 10..20]);
        marks.set(MarkStyle::Token2, [7..9]);
        marks.set(MarkStyle::Mark, [1..2]);
        assert_eq!(marks.ranges(MarkStyle::Mark), &[1..2]);
        assert_eq!(marks.count(MarkStyle::Token2), 1);
        marks.clear(MarkStyle::Mark);
        assert!(!marks.is_empty());
        marks.clear_all();
        assert!(marks.is_empty());
    }

    #[test]
    fn jumps_between_marks_with_wrapping() {
        let mut marks = Marks::new();
        marks.set(MarkStyle::Token1, [4..6, 10..12, 20..21]);
        assert_eq!(marks.next(MarkStyle::Token1, 4), Some(10..12));
        assert_eq!(marks.next(MarkStyle::Token1, 20), Some(4..6));
        assert_eq!(marks.prev(MarkStyle::Token1, 10), Some(4..6));
        assert_eq!(marks.prev(MarkStyle::Token1, 4), Some(20..21));
        assert_eq!(marks.next(MarkStyle::Token5, 0), None);
    }

    #[test]
    fn marks_follow_edits() {
        let mut marks = Marks::new();
        marks.set(MarkStyle::Mark, [2..5, 8..10, 12..14]);
        let edits = normalize(
            vec![
                TextEdit::insert(2, "ab"),
                TextEdit::insert(9, "X"),
                TextEdit::insert(10, "Y"),
                TextEdit::delete(11..15),
            ],
            20,
        )
        .unwrap();
        marks.remap(&edits);
        assert_eq!(marks.ranges(MarkStyle::Mark), &[4..7, 10..13]);
    }

    #[test]
    fn bookmarks_every_line_a_match_spans() {
        let text = "one\ntwo\nthree\nfour";
        let index = LineIndex::new(text);
        assert_eq!(bookmark_lines_of(&[1..6], &index), [0, 1]);
        assert_eq!(bookmark_lines_of(&[4..8], &index), [1]);
        assert_eq!(bookmark_lines_of(&[8..8, 18..18, 0..1], &index), [0, 2, 3]);
        let mut marks = Marks::new();
        marks.set(MarkStyle::Mark, [9..10, 15..16]);
        assert_eq!(marks.bookmark_lines(MarkStyle::Mark, &index), [2, 3]);
    }

    fn edits_for(len: usize) -> impl Strategy<Value = Vec<TextEdit>> {
        prop::collection::vec((0..=len, 0..=len, "[xy]{0,3}"), 0..5).prop_map(|raw| {
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
        fn marks_untouched_by_edits_keep_their_text(
            (text, edits, raw) in "[a-dé]{1,30}".prop_flat_map(|text| {
                let len = text.chars().count();
                (Just(text), edits_for(len), prop::collection::vec((0..len, 1usize..4), 0..6))
            })
        ) {
            let len = text.chars().count();
            let edits = normalize(edits, len).unwrap();
            let after = apply(&text, edits.clone()).unwrap();
            let mut marks = Marks::new();
            marks.add(MarkStyle::Mark, raw.iter().map(|&(start, width)| start..(start + width).min(len)));
            let before = marks.ranges(MarkStyle::Mark).to_vec();
            marks.remap(&edits);
            let chars: Vec<char> = text.chars().collect();
            let after_chars: Vec<char> = after.chars().collect();
            for range in &before {
                let expected = map_offset(range.start, &edits, Bias::After)
                    ..map_offset(range.end, &edits, Bias::Before);
                let untouched = edits.iter().all(|e| e.end < range.start || e.start > range.end);
                if untouched {
                    prop_assert!(marks.ranges(MarkStyle::Mark).contains(&expected));
                    prop_assert_eq!(&after_chars[expected.clone()], &chars[range.clone()]);
                }
            }
            let remapped = marks.ranges(MarkStyle::Mark);
            prop_assert!(remapped.iter().all(|r| r.start < r.end && r.end <= after_chars.len()));
            prop_assert!(remapped.windows(2).all(|pair| pair[0].end <= pair[1].start));
        }
    }
}
