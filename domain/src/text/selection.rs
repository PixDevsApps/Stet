//! The caret and the selection in character offsets, and where edits move them.

use super::edit::{Bias, TextEdit, map_offset};
use std::ops::Range;

/// The caret (`cursor`) and the selection's other end (`anchor`), in character offsets; the
/// two are equal when nothing is selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub cursor: usize,
    pub anchor: usize,
}

impl Selection {
    /// `range` selected with the caret at its end.
    pub fn forward(range: Range<usize>) -> Self {
        Self {
            cursor: range.end,
            anchor: range.start,
        }
    }

    /// `range` selected with the caret at its start.
    pub fn backward(range: Range<usize>) -> Self {
        Self {
            cursor: range.start,
            anchor: range.end,
        }
    }

    /// `range` selected with the caret at the same end as in this selection.
    #[must_use]
    pub fn with_range(self, range: Range<usize>) -> Self {
        if self.is_backward() {
            Self::backward(range)
        } else {
            Self::forward(range)
        }
    }

    /// The selected characters, in order.
    pub fn range(self) -> Range<usize> {
        self.cursor.min(self.anchor)..self.cursor.max(self.anchor)
    }

    /// Whether the caret is at the start of a non-empty selection.
    pub fn is_backward(self) -> bool {
        self.cursor < self.anchor
    }

    /// Where `edits` (normalized, in the old text's offsets) move the selection. A selection
    /// grows over text inserted at its edges, so a replaced selection stays selected; a caret
    /// inside a replaced match moves to its start.
    #[must_use]
    pub fn map(self, edits: &[TextEdit]) -> Self {
        if self.cursor == self.anchor {
            let caret = map_offset(self.cursor, edits, Bias::Before);
            return Self {
                cursor: caret,
                anchor: caret,
            };
        }
        let range = self.range();
        let start = map_offset(range.start, edits, Bias::Before);
        let end = map_offset(range.end, edits, Bias::After).max(start);
        self.with_range(start..end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::edit::tests::arbitrary_edits;
    use crate::text::{apply, normalize};
    use proptest::prelude::*;

    fn caret(offset: usize) -> Selection {
        Selection::forward(offset..offset)
    }

    /// The edit that undoes `edit` on `text`, in the edited text's offsets.
    fn inverse(text: &str, edit: &TextEdit) -> TextEdit {
        let removed: String = text
            .chars()
            .skip(edit.start)
            .take(edit.end - edit.start)
            .collect();
        TextEdit::new(
            edit.start..edit.start + edit.insert.chars().count(),
            removed,
        )
    }

    #[test]
    fn the_caret_and_the_range() {
        let forward = Selection::forward(2..5);
        assert_eq!((forward.cursor, forward.anchor), (5, 2));
        assert_eq!(forward.range(), 2..5);
        assert!(!forward.is_backward());
        let backward = Selection::backward(2..5);
        assert_eq!((backward.cursor, backward.anchor), (2, 5));
        assert_eq!(backward.range(), 2..5);
        assert!(backward.is_backward());
        assert_eq!(caret(4).range(), 4..4);
        assert!(!caret(4).is_backward());
    }

    #[test]
    fn a_replaced_selection_stays_selected() {
        let edits = [TextEdit::new(2..5, "abcdef")];
        assert_eq!(Selection::forward(2..5).map(&edits).range(), 2..8);
        let backward = Selection::backward(2..5).map(&edits);
        assert_eq!(backward.range(), 2..8);
        assert!(backward.is_backward());
        let at_edges = [TextEdit::insert(2, "<"), TextEdit::insert(5, ">")];
        assert_eq!(Selection::forward(2..5).map(&at_edges).range(), 2..7);
    }

    #[test]
    fn a_caret_inside_a_replaced_match_moves_to_its_start() {
        let edits = [TextEdit::new(2..6, "xy")];
        assert_eq!(caret(4).map(&edits), caret(2));
        assert_eq!(caret(6).map(&edits), caret(4));
        assert_eq!(caret(1).map(&edits), caret(1));
        assert_eq!(caret(9).map(&edits), caret(7));
        assert_eq!(caret(2).map(&[TextEdit::insert(2, "ab")]), caret(2));
    }

    #[test]
    fn a_deleted_selection_collapses_to_a_caret() {
        let mapped = Selection::backward(3..6).map(&[TextEdit::delete(2..8)]);
        assert_eq!(mapped, caret(2));
    }

    /// A text length, normalized edits for it and two offsets in it.
    fn edits_and_offsets() -> impl Strategy<Value = (usize, Vec<TextEdit>, usize, usize)> {
        (0..=20usize).prop_flat_map(|len| {
            (
                Just(len),
                arbitrary_edits(len).prop_map(move |edits| normalize(edits, len).unwrap()),
                0..=len,
                0..=len,
            )
        })
    }

    /// One edit of a text of up to 20 characters, and an offset in it.
    fn edit_and_offset() -> impl Strategy<Value = (TextEdit, usize)> {
        (0..=20usize)
            .prop_flat_map(|len| (0..=len, 0..=len, "[xyé]{0,4}", 0..=len))
            .prop_map(|(a, b, insert, offset)| (TextEdit::new(a.min(b)..a.max(b), insert), offset))
    }

    proptest! {
        #[test]
        fn mapped_selections_stay_in_the_new_text((len, edits, a, b) in edits_and_offsets()) {
            let new_len = edits.iter().fold(len.cast_signed(), |len, edit| len + edit.delta());
            for selection in [Selection { cursor: a, anchor: b }, caret(a)] {
                let mapped = selection.map(&edits);
                prop_assert!(mapped.range().end.cast_signed() <= new_len, "{mapped:?} past {new_len}");
                if !mapped.range().is_empty() {
                    prop_assert_eq!(mapped.is_backward(), selection.is_backward());
                }
            }
        }

        #[test]
        fn mapping_keeps_the_order_of_carets((_, edits, a, b) in edits_and_offsets()) {
            let (low, high) = (a.min(b), a.max(b));
            let (low_caret, high_caret) = (caret(low).map(&edits), caret(high).map(&edits));
            prop_assert!(low_caret.cursor <= high_caret.cursor);
            let selected = Selection::forward(low..high).map(&edits).range();
            prop_assert!(selected.start <= low_caret.cursor);
            prop_assert!(selected.end >= high_caret.cursor);
        }

        #[test]
        fn offsets_around_an_edit_shift_by_its_length((edit, offset) in edit_and_offset()) {
            let mapped = caret(offset).map(std::slice::from_ref(&edit)).cursor;
            if offset <= edit.start {
                prop_assert_eq!(mapped, offset);
            } else if offset >= edit.end {
                prop_assert_eq!(mapped.cast_signed(), offset.cast_signed() + edit.delta());
            } else {
                prop_assert_eq!(mapped, edit.start);
            }
        }

        #[test]
        fn an_edit_then_its_inverse_leaves_outside_selections_alone(
            (text, edit, a, b) in "[a-zé日\n]{1,20}".prop_flat_map(|text| {
                let len = text.chars().count();
                (Just(text), 0..=len, 0..=len, "[xyé]{0,4}", 0..=len, 0..=len)
            }).prop_map(|(text, s, e, insert, a, b)| {
                (text, TextEdit::new(s.min(e)..s.max(e), insert), a, b)
            })
        ) {
            let outside = |offset: usize| offset < edit.start || offset > edit.end;
            prop_assume!(outside(a) && outside(b));
            let undo = inverse(&text, &edit);
            let edited = apply(&text, vec![edit.clone()]).unwrap();
            prop_assert_eq!(apply(&edited, vec![undo.clone()]).unwrap(), text);
            for selection in [Selection { cursor: a, anchor: b }, caret(a)] {
                let there = selection.map(std::slice::from_ref(&edit));
                prop_assert_eq!(there.map(std::slice::from_ref(&undo)), selection);
            }
        }
    }
}
