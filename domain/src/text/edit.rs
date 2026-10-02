//! Edits in character offsets, the unit `GtkTextIter` counts, so they apply to the buffer
//! without conversion (ADR-003).

use std::fmt;
use std::ops::Range;

/// Replaces the characters `start..end` with `insert`. Offsets count Unicode scalar values,
/// not bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextEdit {
    pub start: usize,
    pub end: usize,
    pub insert: String,
}

impl TextEdit {
    pub fn new(range: Range<usize>, insert: impl Into<String>) -> Self {
        Self {
            start: range.start,
            end: range.end,
            insert: insert.into(),
        }
    }

    pub fn insert(at: usize, text: impl Into<String>) -> Self {
        Self::new(at..at, text)
    }

    pub fn delete(range: Range<usize>) -> Self {
        Self::new(range, "")
    }

    pub fn range(&self) -> Range<usize> {
        self.start..self.end
    }

    pub fn is_noop(&self) -> bool {
        self.start == self.end && self.insert.is_empty()
    }

    /// How many characters longer the text is after this edit.
    pub fn delta(&self) -> isize {
        self.insert.chars().count() as isize - (self.end - self.start) as isize
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditError {
    Reversed(Range<usize>),
    OutOfBounds {
        range: Range<usize>,
        len: usize,
    },
    Overlap {
        first: Range<usize>,
        second: Range<usize>,
    },
}

impl fmt::Display for EditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reversed(range) => write!(f, "edit range {range:?} ends before it starts"),
            Self::OutOfBounds { range, len } => {
                write!(
                    f,
                    "edit range {range:?} is outside the text of {len} characters"
                )
            }
            Self::Overlap { first, second } => {
                write!(f, "edit ranges {first:?} and {second:?} overlap")
            }
        }
    }
}

impl std::error::Error for EditError {}

/// Sorts `edits` by position, drops no-ops and checks that they neither overlap nor leave a
/// text of `len` characters. Inserts at the same offset keep their order.
pub fn normalize(mut edits: Vec<TextEdit>, len: usize) -> Result<Vec<TextEdit>, EditError> {
    edits.retain(|edit| !edit.is_noop());
    edits.sort_by_key(|edit| (edit.start, edit.end));
    for edit in &edits {
        if edit.start > edit.end {
            return Err(EditError::Reversed(edit.range()));
        }
        if edit.end > len {
            return Err(EditError::OutOfBounds {
                range: edit.range(),
                len,
            });
        }
    }
    for pair in edits.windows(2) {
        if pair[0].end > pair[1].start {
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
    let mut copied = 0;
    for edit in &edits {
        let start = cursor.byte_at(edit.start);
        out.push_str(&text[copied..start]);
        out.push_str(&edit.insert);
        copied = cursor.byte_at(edit.end);
    }
    out.push_str(&text[copied..]);
    Ok(out)
}

/// Converts non-decreasing character offsets to byte offsets in one pass.
pub(crate) struct ByteCursor<'a> {
    text: &'a str,
    chars: usize,
    bytes: usize,
}

impl<'a> ByteCursor<'a> {
    pub(crate) fn new(text: &'a str) -> Self {
        Self {
            text,
            chars: 0,
            bytes: 0,
        }
    }

    /// The byte offset of character `target`; `target` must not be below an earlier call's.
    pub(crate) fn byte_at(&mut self, target: usize) -> usize {
        debug_assert!(target >= self.chars, "ByteCursor moved backwards");
        while self.chars < target {
            let next = self.text[self.bytes..]
                .chars()
                .next()
                .expect("character offset within the text");
            self.bytes += next.len_utf8();
            self.chars += 1;
        }
        self.bytes
    }
}

/// Which side of a replaced range a position inside it moves to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bias {
    Before,
    After,
}

/// Where the character offset `offset` ends up after `edits` (normalized) are applied.
/// Positions inside a replaced range move to its start or, with [`Bias::After`], its end.
pub fn map_offset(offset: usize, edits: &[TextEdit], bias: Bias) -> usize {
    let mut shift: isize = 0;
    for edit in edits {
        if offset < edit.start || (offset == edit.start && bias == Bias::Before) {
            break;
        }
        if offset < edit.end || (offset == edit.end && edit.start == edit.end) {
            let start = (edit.start as isize + shift) as usize;
            return match bias {
                Bias::Before => start,
                Bias::After => start + edit.insert.chars().count(),
            };
        }
        shift += edit.delta();
    }
    (offset as isize + shift) as usize
}

/// The one edit that turns `old` into `new` by replacing only what lies between their common
/// prefix and suffix; `None` when they are equal. A reload from disk applies it, so marks and
/// the view outside the change stay where they were.
pub fn minimal_edit(old: &str, new: &str) -> Option<TextEdit> {
    if old == new {
        return None;
    }
    let (old_bytes, new_bytes) = (old.as_bytes(), new.as_bytes());
    let mismatch = old_bytes
        .iter()
        .zip(new_bytes)
        .position(|(a, b)| a != b)
        .unwrap_or(old.len().min(new.len()));
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
    let start = old[..prefix].chars().count();
    let end = start + old[prefix..old.len() - suffix].chars().count();
    Some(TextEdit::new(start..end, &new[prefix..new.len() - suffix]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn minimal_edits_replace_only_the_changed_middle() {
        assert_eq!(minimal_edit("same", "same"), None);
        assert_eq!(
            minimal_edit("one two three", "one 2 three"),
            Some(TextEdit::new(4..7, "2"))
        );
        assert_eq!(minimal_edit("abc", "abXc"), Some(TextEdit::insert(2, "X")));
        assert_eq!(minimal_edit("aaa", "aa"), Some(TextEdit::delete(2..3)));
        assert_eq!(minimal_edit("", "new"), Some(TextEdit::insert(0, "new")));
        // é (C3 A9) and è (C3 A8) share their first byte; the edit takes whole characters.
        assert_eq!(
            minimal_edit("café!", "cafè!"),
            Some(TextEdit::new(3..4, "è"))
        );
        assert_eq!(minimal_edit("ab", "ba"), Some(TextEdit::new(0..2, "ba")));
    }

    fn apply_ok(text: &str, edits: Vec<TextEdit>) -> String {
        apply(text, edits).unwrap()
    }

    #[test]
    fn applies_inserts_deletes_and_replacements_in_any_order() {
        let edits = vec![
            TextEdit::new(6..11, "Stet"),
            TextEdit::insert(0, ">> "),
            TextEdit::delete(5..6),
        ];
        assert_eq!(apply_ok("hello world", edits), ">> helloStet");
    }

    #[test]
    fn counts_characters_not_bytes() {
        let edits = vec![TextEdit::new(1..2, "o"), TextEdit::insert(4, "!")];
        assert_eq!(apply_ok("pøst", edits), "post!");
        assert_eq!(apply_ok("日本語", vec![TextEdit::delete(1..2)]), "日語");
    }

    #[test]
    fn inserts_at_one_offset_keep_their_order() {
        let edits = vec![TextEdit::insert(1, "a"), TextEdit::insert(1, "b")];
        assert_eq!(apply_ok("xy", edits), "xaby");
    }

    #[test]
    fn rejects_overlap_reversal_and_out_of_bounds() {
        let overlap = vec![TextEdit::delete(0..3), TextEdit::delete(2..4)];
        assert!(matches!(
            apply("abcdef", overlap),
            Err(EditError::Overlap { .. })
        ));
        assert!(matches!(
            apply(
                "abc",
                vec![TextEdit {
                    start: 2,
                    end: 1,
                    insert: "x".into(),
                }]
            ),
            Err(EditError::Reversed(_))
        ));
        assert!(matches!(
            apply("abc", vec![TextEdit::delete(2..4)]),
            Err(EditError::OutOfBounds { .. })
        ));
    }

    #[test]
    fn maps_offsets_through_edits() {
        let edits = normalize(
            vec![TextEdit::new(2..4, "XYZ"), TextEdit::insert(6, "!")],
            8,
        )
        .unwrap();
        assert_eq!(map_offset(1, &edits, Bias::Before), 1);
        assert_eq!(map_offset(3, &edits, Bias::Before), 2);
        assert_eq!(map_offset(3, &edits, Bias::After), 5);
        assert_eq!(map_offset(5, &edits, Bias::Before), 6);
        assert_eq!(map_offset(6, &edits, Bias::Before), 7);
        assert_eq!(map_offset(6, &edits, Bias::After), 8);
        assert_eq!(map_offset(8, &edits, Bias::Before), 10);
    }

    fn naive_apply(text: &str, edits: &[TextEdit]) -> String {
        let mut chars: Vec<char> = text.chars().collect();
        for edit in edits.iter().rev() {
            chars.splice(edit.start..edit.end, edit.insert.chars());
        }
        chars.into_iter().collect()
    }

    fn arbitrary_edits(len: usize) -> impl Strategy<Value = Vec<TextEdit>> {
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
                        TextEdit::new(start..end, insert)
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
                    let kept = old.chars().count() - (edit.end - edit.start);
                    let prefix = old.chars().zip(new.chars()).take_while(|(a, b)| a == b).count();
                    prop_assert!(kept >= prefix.min(edit.start));
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
                    edits
                        .iter()
                        .all(|e| e.start != o && (o < e.start || o >= e.end.max(e.start + 1)))
                })
                .collect();
            if untouched.is_empty() {
                return Ok(());
            }
            let offset = untouched[pick.index(untouched.len())];
            let mapped = map_offset(offset, &edits, Bias::Before);
            let original = text.chars().nth(offset).unwrap();
            let result = naive_apply(&text, &edits);
            prop_assert_eq!(result.chars().nth(mapped), Some(original));
        }
    }
}
