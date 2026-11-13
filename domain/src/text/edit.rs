//! Edits in character offsets, the unit `GtkTextIter` counts, so they apply to the buffer
//! without conversion (ADR-003).

use super::offset::ByteCursor;
use super::units::{ByteOffset, CharOffset, OffsetRange};
use std::fmt;
use std::ops::Range;

/// Above this many edits a change is applied as one replacement of the range it spans
/// (ADR-003 amendment).
pub const BULK_EDIT_THRESHOLD: usize = 2_000;

/// Replaces the characters of a range with `insert`. The range never ends before it starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    range: Range<CharOffset>,
    /// The text that takes the range's place.
    pub insert: String,
}

impl TextEdit {
    /// Replaces `range` with `insert`.
    ///
    /// # Panics
    ///
    /// When `range` ends before it starts.
    pub fn new(range: Range<CharOffset>, insert: impl Into<String>) -> Self {
        assert!(
            range.start <= range.end,
            "edit range {:?} ends before it starts",
            range.get()
        );
        Self {
            range,
            insert: insert.into(),
        }
    }

    /// Inserts `text` at `at`.
    pub fn insert(at: CharOffset, text: impl Into<String>) -> Self {
        Self::new(at..at, text)
    }

    /// Deletes `range`.
    #[must_use]
    pub fn delete(range: Range<CharOffset>) -> Self {
        Self::new(range, "")
    }

    /// The replaced characters.
    #[must_use]
    pub fn range(&self) -> Range<CharOffset> {
        self.range.clone()
    }

    /// Where the replaced characters start, and where the inserted text goes.
    #[must_use]
    pub fn start(&self) -> CharOffset {
        self.range.start
    }

    /// Where the replaced characters end.
    #[must_use]
    pub fn end(&self) -> CharOffset {
        self.range.end
    }

    /// How many characters the edit replaces.
    #[must_use]
    pub fn removed(&self) -> usize {
        self.range.end - self.range.start
    }

    /// Whether the edit changes nothing: it replaces no characters with no text.
    #[must_use]
    pub fn is_noop(&self) -> bool {
        self.range.is_empty() && self.insert.is_empty()
    }

    /// How many characters longer the text is after this edit.
    #[must_use]
    pub fn delta(&self) -> isize {
        self.insert.chars().count().cast_signed() - self.removed().cast_signed()
    }
}

/// Why a set of edits can't be applied to a text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    /// An edit reaches past the end of the text.
    OutOfBounds {
        /// The edit's characters.
        range: Range<CharOffset>,
        /// The text's length in characters.
        len: usize,
    },
    /// Two edits replace some of the same characters.
    Overlap {
        /// The characters of the earlier edit.
        first: Range<CharOffset>,
        /// The characters of the later edit.
        second: Range<CharOffset>,
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfBounds { range, len } => {
                write!(
                    f,
                    "edit range {:?} is outside the text of {len} characters",
                    range.get()
                )
            }
            Self::Overlap { first, second } => {
                write!(
                    f,
                    "edit ranges {:?} and {:?} overlap",
                    first.get(),
                    second.get()
                )
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Sorts `edits` by position, drops no-ops and checks that they neither overlap nor leave a
/// text of `len` characters. Inserts at the same offset keep their order.
pub fn normalize(mut edits: Vec<TextEdit>, len: usize) -> Result<Vec<TextEdit>, EditError> {
    edits.retain(|edit| !edit.is_noop());
    edits.sort_by_key(|edit| (edit.start(), edit.end()));
    let end = CharOffset::ZERO + len;
    if let Some(edit) = edits.iter().find(|edit| edit.end() > end) {
        return Err(EditError::OutOfBounds {
            range: edit.range(),
            len,
        });
    }
    for pair in edits.windows(2) {
        if pair[0].end() > pair[1].start() {
            return Err(EditError::Overlap {
                first: pair[0].range(),
                second: pair[1].range(),
            });
        }
    }
    Ok(edits)
}

/// Applies `edits` to `text`. This is the model the buffer bridge must match, and the way
/// whole-document operations run on snapshots.
pub fn apply(text: &str, edits: Vec<TextEdit>) -> Result<String, EditError> {
    let edits = normalize(edits, text.chars().count())?;
    let mut out = String::with_capacity(text.len());
    let mut cursor = ByteCursor::new(text);
    let mut copied = ByteOffset::ZERO;
    for edit in &edits {
        let start = cursor.byte_at(edit.start());
        out.push_str(&text[(copied..start).get()]);
        out.push_str(&edit.insert);
        copied = cursor.byte_at(edit.end());
    }
    out.push_str(&text[copied.get()..]);
    Ok(out)
}

/// Which side of a replaced range a position inside it moves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bias {
    /// To the start of the replaced range.
    Before,
    /// To the end of the inserted text.
    After,
}

/// Where `offset` ends up after `edits` (normalized) are applied. Positions inside a replaced
/// range move to its start or, with [`Bias::After`], its end.
#[must_use]
pub fn map_offset(offset: CharOffset, edits: &[TextEdit], bias: Bias) -> CharOffset {
    let mut shift: isize = 0;
    for edit in edits {
        let (start, end) = (edit.start(), edit.end());
        if offset < start || (offset == start && bias == Bias::Before) {
            break;
        }
        if offset < end || (offset == end && start == end) {
            let start = start.saturating_add_signed(shift);
            return match bias {
                Bias::Before => start,
                Bias::After => start + edit.insert.chars().count(),
            };
        }
        shift += edit.delta();
    }
    offset.saturating_add_signed(shift)
}

/// The one edit that turns `old` into `new` by replacing only what lies between their common
/// prefix and suffix; `None` when they are equal. A reload from disk applies it, so marks and
/// the view outside the change stay where they were.
#[must_use]
pub fn minimal_edit(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let (old_bytes, new_bytes) = (old.as_bytes(), new.as_bytes());
    let mismatch = old_bytes
        .iter()
        .zip(new_bytes)
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| old.len().min(new.len()));
    // Equal bytes have equal character boundaries, so one check covers both texts.
    let prefix = old.floor_char_boundary(mismatch);
    let room = (old.len() - prefix).min(new.len() - prefix);
    let mut suffix = old_bytes[prefix..]
        .iter()
        .rev()
        .zip(new_bytes[prefix..].iter().rev())
        .take(room)
        .take_while(|(a, b)| a == b)
        .count();
    while !old.is_char_boundary(old.len() - suffix) {
        suffix -= 1;
    }
    let start = CharOffset::ZERO + old[..prefix].chars().count();
    let end = start + old[prefix..old.len() - suffix].chars().count();
    Some(TextEdit::new(start..end, &new[prefix..new.len() - suffix]))
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::text::testing::{at, chars};
    use proptest::prelude::*;

    #[test]
    fn minimal_edits_replace_only_the_changed_middle() {
        assert_eq!(minimal_edit("same", "same"), None);
        assert_eq!(
            minimal_edit("one two three", "one 2 three"),
            Some(TextEdit::new(chars(4..7), "2"))
        );
        assert_eq!(
            minimal_edit("abc", "abXc"),
            Some(TextEdit::insert(at(2), "X"))
        );
        assert_eq!(
            minimal_edit("aaa", "aa"),
            Some(TextEdit::delete(chars(2..3)))
        );
        assert_eq!(
            minimal_edit("", "new"),
            Some(TextEdit::insert(at(0), "new"))
        );
        // é (C3 A9) and è (C3 A8) share their first byte; the edit takes whole characters.
        assert_eq!(
            minimal_edit("café!", "cafè!"),
            Some(TextEdit::new(chars(3..4), "è"))
        );
        assert_eq!(
            minimal_edit("ab", "ba"),
            Some(TextEdit::new(chars(0..2), "ba"))
        );
    }

    fn apply_ok(text: &str, edits: Vec<TextEdit>) -> String {
        apply(text, edits).unwrap()
    }

    #[test]
    fn applies_inserts_deletes_and_replacements_in_any_order() {
        let edits = vec![
            TextEdit::new(chars(6..11), "Stet"),
            TextEdit::insert(at(0), ">> "),
            TextEdit::delete(chars(5..6)),
        ];
        assert_eq!(apply_ok("hello world", edits), ">> helloStet");
    }

    #[test]
    fn counts_characters_not_bytes() {
        let edits = vec![
            TextEdit::new(chars(1..2), "o"),
            TextEdit::insert(at(4), "!"),
        ];
        assert_eq!(apply_ok("pøst", edits), "post!");
        assert_eq!(
            apply_ok("日本語", vec![TextEdit::delete(chars(1..2))]),
            "日語"
        );
    }

    #[test]
    fn inserts_at_one_offset_keep_their_order() {
        let edits = vec![TextEdit::insert(at(1), "a"), TextEdit::insert(at(1), "b")];
        assert_eq!(apply_ok("xy", edits), "xaby");
    }

    #[test]
    fn rejects_overlap_and_out_of_bounds() {
        let overlap = vec![TextEdit::delete(chars(0..3)), TextEdit::delete(chars(2..4))];
        let error = apply("abcdef", overlap).unwrap_err();
        assert!(matches!(error, EditError::Overlap { .. }));
        assert_eq!(error.to_string(), "edit ranges 0..3 and 2..4 overlap");
        let error = apply("abc", vec![TextEdit::delete(chars(2..4))]).unwrap_err();
        assert!(matches!(error, EditError::OutOfBounds { .. }));
        assert_eq!(
            error.to_string(),
            "edit range 2..4 is outside the text of 3 characters"
        );
    }

    #[test]
    #[should_panic(expected = "edit range 2..1 ends before it starts")]
    fn a_reversed_range_is_no_edit() {
        let _ = TextEdit::new(at(2)..at(1), "x");
    }

    #[test]
    fn maps_offsets_through_edits() {
        let edits = normalize(
            vec![
                TextEdit::new(chars(2..4), "XYZ"),
                TextEdit::insert(at(6), "!"),
            ],
            8,
        )
        .unwrap();
        let map = |offset, bias| map_offset(at(offset), &edits, bias).get();
        assert_eq!(map(1, Bias::Before), 1);
        assert_eq!(map(3, Bias::Before), 2);
        assert_eq!(map(3, Bias::After), 5);
        assert_eq!(map(5, Bias::Before), 6);
        assert_eq!(map(6, Bias::Before), 7);
        assert_eq!(map(6, Bias::After), 8);
        assert_eq!(map(8, Bias::Before), 10);
    }

    fn naive_apply(text: &str, edits: &[TextEdit]) -> String {
        let mut chars: Vec<char> = text.chars().collect();
        for edit in edits.iter().rev() {
            chars.splice(edit.range().get(), edit.insert.chars());
        }
        chars.into_iter().collect()
    }

    /// Sorted, non-overlapping edits of a text of `len` characters.
    pub(in crate::text) fn arbitrary_edits(len: usize) -> impl Strategy<Value = Vec<TextEdit>> {
        prop::collection::vec((0..=len, 0..=len, "[a-zé日\n]{0,3}"), 0..6).prop_map(|raw| {
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
                        TextEdit::new(chars(start..end), insert)
                    })
                })
                .collect()
        })
    }

    proptest! {
        #[test]
        fn minimal_edits_turn_old_into_new(old in "[abé日\n]{0,16}", new in "[abé日\n]{0,16}") {
            match minimal_edit(&old, &new) {
                None => prop_assert_eq!(&old, &new),
                Some(edit) => {
                    prop_assert_eq!(apply(&old, vec![edit.clone()]).unwrap(), new.clone());
                    let kept = old.chars().count() - edit.removed();
                    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
                    prop_assert!(kept >= prefix.min(edit.start().get()));
                }
            }
        }

        #[test]
        fn apply_matches_a_char_vector_model(
            (text, edits) in "[a-zøé日\n ]{0,24}".prop_flat_map(|text| {
                let len = text.chars().count();
                (Just(text), arbitrary_edits(len))
            })
        ) {
            let expected = naive_apply(&text, &normalize(edits.clone(), text.chars().count()).unwrap());
            prop_assert_eq!(apply(&text, edits).unwrap(), expected);
        }

        #[test]
        fn offsets_outside_edits_keep_their_character(
            (text, edits, pick) in "[a-zø]{1,20}".prop_flat_map(|text| {
                let len = text.chars().count();
                (Just(text), arbitrary_edits(len), any::<prop::sample::Index>())
            })
        ) {
            let edits = normalize(edits, text.chars().count()).unwrap();
            let untouched: Vec<usize> = (0..text.chars().count())
                .filter(|&o| {
                    edits.iter().all(|e| {
                        let (start, end) = (e.start().get(), e.end().get());
                        start != o && (o < start || o >= end.max(start + 1))
                    })
                })
                .collect();
            if untouched.is_empty() {
                return Ok(());
            }
            let offset = untouched[pick.index(untouched.len())];
            let mapped = map_offset(at(offset), &edits, Bias::Before);
            let original = text.chars().nth(offset).unwrap();
            let result = naive_apply(&text, &edits);
            prop_assert_eq!(result.chars().nth(mapped.get()), Some(original));
        }
    }
}
