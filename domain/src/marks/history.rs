//! Bookmarks across Undo and Redo of the steps Stet makes itself: bulk edits and the
//! bookmarked-line operations. GTK replays such a step as the replacements it was made of,
//! which collapse the marks inside them and bring deleted lines back without their bookmarks
//! (ADR-003 amendment), so the bookmarks of the lines a step replaced are kept here and put
//! back when the step is undone or redone.
//!
//! A step is recognised by its [`Footprint`]: where its first replacement started and how many
//! characters it deleted and inserted in all. Undoing it deletes what it inserted and inserts
//! what it deleted at the same place, and leaves the text as long as it was before.
//!
//! A step can also keep the Mark and style-token ranges from before and after it, which GTK's
//! replay would drop the same way.

use std::ops::Range;

use crate::text::{LineIndex, TextEdit};

use super::{Bookmarks, Marks};

/// How many steps are kept, as GTK keeps at most this many undo steps
/// ([`MAX_UNDO_LEVELS`](crate::view::MAX_UNDO_LEVELS)).
const MAX_STEPS: usize = crate::view::MAX_UNDO_LEVELS as usize;

/// What a step did to the text, in characters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Footprint {
    /// Where the first replacement started.
    pub start: usize,
    pub deleted: usize,
    pub inserted: usize,
}

impl Footprint {
    /// The footprint of replacements made to one text (sorted, non-overlapping, in its
    /// offsets), as they reached the buffer.
    pub fn of(replacements: &[TextEdit]) -> Self {
        Self {
            start: replacements.first().map_or(0, |edit| edit.start),
            deleted: replacements.iter().map(|edit| edit.end - edit.start).sum(),
            inserted: replacements
                .iter()
                .map(|edit| edit.insert.chars().count())
                .sum(),
        }
    }

    /// The footprint of undoing it.
    pub fn inverse(self) -> Self {
        Self {
            start: self.start,
            deleted: self.inserted,
            inserted: self.deleted,
        }
    }

    /// Adds one replacement seen while GTK undid or redid a step.
    pub fn observe(&mut self, start: usize, deleted: usize, inserted: usize) {
        if self.deleted == 0 && self.inserted == 0 {
            self.start = start;
        } else {
            self.start = self.start.min(start);
        }
        self.deleted += deleted;
        self.inserted += inserted;
    }
}

/// One step: what it did, and the bookmarks of the lines it replaced before and after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub footprint: Footprint,
    /// The text's length in characters before and after the step.
    pub len_before: usize,
    pub len_after: usize,
    /// The lines the step replaced, before and after it.
    pub lines_before: Range<usize>,
    pub lines_after: Range<usize>,
    /// The bookmarks on those lines, before and after.
    pub before: Bookmarks,
    pub after: Bookmarks,
    /// The marks before and after the step, when there were any.
    pub marks: Option<Box<(Marks, Marks)>>,
}

impl Step {
    /// The step that turns the text `index` describes, with `before` bookmarked, into one with
    /// `after` bookmarked through `edits` (sorted, non-overlapping, in the text's offsets),
    /// which reached the buffer as `applied`. `None` when `edits` is empty.
    pub fn new(
        edits: &[TextEdit],
        applied: &[TextEdit],
        index: &LineIndex,
        before: &Bookmarks,
        after: &Bookmarks,
    ) -> Option<Self> {
        let (first, last) = (edits.first()?, edits.last()?);
        let first_line = index.line_of_char(first.start);
        let last_line = index.line_of_char(last.end);
        let removed: usize = edits
            .iter()
            .map(|edit| index.line_of_char(edit.end) - index.line_of_char(edit.start))
            .sum();
        let added: usize = edits
            .iter()
            .map(|edit| memchr::memchr_iter(b'\n', edit.insert.as_bytes()).count())
            .sum();
        let lines_before = first_line..last_line + 1;
        let lines_after = first_line..last_line + 1 + added - removed;
        let inserted: usize = edits.iter().map(|edit| edit.insert.chars().count()).sum();
        let deleted: usize = edits.iter().map(|edit| edit.end - edit.start).sum();
        let len_before = index.len_chars();
        Some(Self {
            footprint: Footprint::of(applied),
            len_before,
            len_after: len_before + inserted - deleted,
            before: within(before, &lines_before),
            after: within(after, &lines_after),
            lines_before,
            lines_after,
            marks: None,
        })
    }
}

fn within(bookmarks: &Bookmarks, lines: &Range<usize>) -> Bookmarks {
    bookmarks.in_lines(lines.clone()).iter().copied().collect()
}

/// The bookmarks GTK left after replaying a step (`current`, which GTK moved along with the
/// text outside the step's `lines`), with those on `lines` replaced by the step's own.
fn restore(current: &Bookmarks, lines: &Range<usize>, own: &Bookmarks) -> Bookmarks {
    current
        .lines()
        .iter()
        .copied()
        .filter(|line| !lines.contains(line))
        .chain(own.lines().iter().copied())
        .collect()
}

/// The steps that can still be undone, and those undone that can be redone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BookmarkHistory {
    steps: Vec<Step>,
    /// Steps not undone; `steps[applied..]` can be redone.
    applied: usize,
}

impl BookmarkHistory {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// Steps that can be undone.
    pub fn undoable(&self) -> usize {
        self.applied
    }

    /// A new step: GTK drops what could be redone, and so does this.
    pub fn record(&mut self, step: Step) {
        self.steps.truncate(self.applied);
        self.steps.push(step);
        if self.steps.len() > MAX_STEPS {
            self.steps.remove(0);
        }
        self.applied = self.steps.len();
    }

    /// An edit that is neither an undo nor a redo: nothing can be redone any more.
    pub fn edited(&mut self) {
        self.steps.truncate(self.applied);
    }

    /// Forget everything, as when a document loads again without undo.
    pub fn clear(&mut self) {
        self.steps.clear();
        self.applied = 0;
    }

    /// GTK undid a step that made `seen` and left the text `len` characters long, and the
    /// buffer's bookmarks are `current` now. When it was the last step recorded here, returns
    /// the bookmarks to show instead.
    pub fn undone(
        &mut self,
        seen: Footprint,
        len: usize,
        current: &Bookmarks,
    ) -> Option<Bookmarks> {
        let step = self.steps.get(self.applied.checked_sub(1)?)?;
        if seen != step.footprint.inverse() || len != step.len_before {
            return None;
        }
        self.applied -= 1;
        Some(restore(current, &step.lines_before, &step.before))
    }

    /// The marks from before the step [`Self::undone`] just took back, if it kept them.
    pub fn undone_marks(&self) -> Option<&Marks> {
        let step = self.steps.get(self.applied)?;
        step.marks.as_ref().map(|marks| &marks.0)
    }

    /// The marks from after the step [`Self::redone`] just made again, if it kept them.
    pub fn redone_marks(&self) -> Option<&Marks> {
        let step = self.steps.get(self.applied.checked_sub(1)?)?;
        step.marks.as_ref().map(|marks| &marks.1)
    }

    /// GTK redid a step that made `seen` and left the text `len` characters long, and the
    /// buffer's bookmarks are `current` now. When it was the next step recorded here, returns
    /// the bookmarks to show instead.
    pub fn redone(
        &mut self,
        seen: Footprint,
        len: usize,
        current: &Bookmarks,
    ) -> Option<Bookmarks> {
        let step = self.steps.get(self.applied)?;
        if seen != step.footprint || len != step.len_after {
            return None;
        }
        self.applied += 1;
        Some(restore(current, &step.lines_after, &step.after))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::marks::delete_unbookmarked_lines;
    use crate::text::apply;
    use proptest::prelude::*;

    fn bookmarks(lines: &[usize]) -> Bookmarks {
        lines.iter().copied().collect()
    }

    /// A text of `count` lines named after their numbers.
    fn numbered(count: usize) -> String {
        (0..count)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn footprints_add_up_replacements() {
        let edits = [TextEdit::new(4..6, "abc"), TextEdit::delete(10..12)];
        let footprint = Footprint::of(&edits);
        assert_eq!(
            footprint,
            Footprint {
                start: 4,
                deleted: 4,
                inserted: 3
            }
        );
        let mut seen = Footprint::default();
        seen.observe(10, 0, 2);
        seen.observe(4, 3, 2);
        assert_eq!(seen, footprint.inverse());
    }

    #[test]
    fn undo_and_redo_put_the_bookmarks_back() {
        let text = numbered(6);
        let index = LineIndex::new(&text);
        let marked = bookmarks(&[0, 2, 4]);
        let edit = delete_unbookmarked_lines(&index, &marked);
        let after_text = apply(&text, edit.edits.clone()).unwrap();
        assert_eq!(after_text, "line 0\nline 2\nline 4");
        let applied = edit.edits.clone();
        let step = Step::new(&edit.edits, &applied, &index, &marked, &edit.bookmarks).unwrap();
        assert_eq!(step.len_after, after_text.chars().count());
        let mut history = BookmarkHistory::new();
        history.record(step.clone());

        // GTK put the deleted lines back and the bookmarks slid onto the wrong ones.
        let collapsed = bookmarks(&[0, 1]);
        let restored = history
            .undone(step.footprint.inverse(), text.chars().count(), &collapsed)
            .unwrap();
        assert_eq!(restored.lines(), &[0, 2, 4]);
        assert_eq!(history.undoable(), 0);
        assert_eq!(
            history.undone(step.footprint.inverse(), text.chars().count(), &collapsed),
            None
        );

        let redone = history
            .redone(step.footprint, after_text.chars().count(), &bookmarks(&[0]))
            .unwrap();
        assert_eq!(redone.lines(), &[0, 1, 2]);
        assert_eq!(history.undoable(), 1);
    }

    #[test]
    fn other_steps_are_not_mistaken_for_ours() {
        let text = numbered(4);
        let index = LineIndex::new(&text);
        let edits = [TextEdit::new(0..6, "LINE 0")];
        let step = Step::new(&edits, &edits, &index, &bookmarks(&[1]), &bookmarks(&[1])).unwrap();
        let mut history = BookmarkHistory::new();
        history.record(step.clone());
        let len = text.chars().count();
        // Same length, another place: a typed word undone, say.
        let elsewhere = Footprint {
            start: 9,
            ..step.footprint.inverse()
        };
        assert_eq!(history.undone(elsewhere, len, &Bookmarks::new()), None);
        // The right place, but the text is not as long as before the step.
        assert_eq!(
            history.undone(step.footprint.inverse(), len + 1, &Bookmarks::new()),
            None
        );
        assert_eq!(history.undoable(), 1);
        assert!(
            history
                .undone(step.footprint.inverse(), len, &Bookmarks::new())
                .is_some()
        );
    }

    #[test]
    fn bookmarks_outside_the_step_are_kept() {
        // Lines 2..4 became one line. Undoing it, GTK moves the bookmarks after it back (7),
        // with one the user set meanwhile (6), and leaves those of the step collapsed (2).
        let text = numbered(8);
        let index = LineIndex::new(&text);
        let start = index.line_chars(2).start;
        let end = index.line_chars(4).end;
        let edits = [TextEdit::new(start..end, "joined")];
        let step = Step::new(
            &edits,
            &edits,
            &index,
            &bookmarks(&[3, 7]),
            &bookmarks(&[2, 5]),
        )
        .unwrap();
        assert_eq!(step.lines_before, 2..5);
        assert_eq!(step.lines_after, 2..3);
        assert_eq!(step.before.lines(), &[3]);
        assert_eq!(step.after.lines(), &[2]);
        let mut history = BookmarkHistory::new();
        history.record(step.clone());
        let current = bookmarks(&[2, 6, 7]);
        let restored = history
            .undone(step.footprint.inverse(), text.chars().count(), &current)
            .unwrap();
        assert_eq!(restored.lines(), &[3, 6, 7]);
        let redone = history
            .redone(step.footprint, step.len_after, &bookmarks(&[2, 3, 4, 5]))
            .unwrap();
        assert_eq!(redone.lines(), &[2, 3, 4, 5]);
    }

    #[test]
    fn a_new_edit_ends_redo_and_a_new_step_replaces_it() {
        let text = numbered(3);
        let index = LineIndex::new(&text);
        let edits = [TextEdit::insert(0, "x\n")];
        let step = Step::new(&edits, &edits, &index, &bookmarks(&[1]), &bookmarks(&[2])).unwrap();
        let mut history = BookmarkHistory::new();
        history.record(step.clone());
        history.record(step.clone());
        assert_eq!(history.len(), 2);
        let len = step.len_before;
        assert!(
            history
                .undone(step.footprint.inverse(), len, &Bookmarks::new())
                .is_some()
        );
        history.edited();
        assert_eq!(history.len(), 1);
        assert_eq!(
            history.redone(step.footprint, step.len_after, &Bookmarks::new()),
            None
        );
        history.record(step);
        assert_eq!(history.len(), 2);
        history.clear();
        assert!(history.is_empty());
        assert_eq!(history.undoable(), 0);
    }

    /// A deletion or an insertion, as GTK's undo history keeps it: where, which characters,
    /// and whether they were inserted.
    type Change = (usize, Vec<char>, bool);

    /// The changes applying `edits` to `text` makes, last edit first, each as a deletion and
    /// an insertion at its start (`app/src/edits.rs`).
    fn changes(text: &[char], edits: &[TextEdit]) -> Vec<Change> {
        let mut changes = Vec::new();
        for edit in edits.iter().rev() {
            if edit.end > edit.start {
                changes.push((edit.start, text[edit.start..edit.end].to_vec(), false));
            }
            if !edit.insert.is_empty() {
                changes.push((edit.start, edit.insert.chars().collect(), true));
            }
        }
        changes
    }

    /// Plays `changes` on `text` forwards, as a redo does, or inverted from the last, as an
    /// undo does, and adds up what the buffer reports meanwhile.
    fn replay(text: &mut Vec<char>, changes: &[Change], undo: bool) -> Footprint {
        let mut seen = Footprint::default();
        let mut play = |(at, chars, inserted): &Change| {
            if *inserted != undo {
                text.splice(*at..*at, chars.iter().copied());
                seen.observe(*at, 0, chars.len());
            } else {
                text.drain(*at..*at + chars.len());
                seen.observe(*at, chars.len(), 0);
            }
        };
        if undo {
            changes.iter().rev().for_each(&mut play);
        } else {
            changes.iter().for_each(&mut play);
        }
        seen
    }

    proptest! {
        #[test]
        fn replays_are_recognised_and_the_lines_around_a_step_stay(
            text in "[a-cé\n]{0,24}",
            shape in prop::collection::vec((0..4usize, 0..4usize, "[xé\n]{0,3}"), 1..6),
        ) {
            let chars: Vec<char> = text.chars().collect();
            let mut edits = Vec::new();
            let mut at = 0;
            for (gap, len, insert) in shape {
                let (start, end) = (at + gap, at + gap + len);
                if end > chars.len() {
                    break;
                }
                let edit = TextEdit::new(start..end, insert);
                if !edit.is_noop() {
                    edits.push(edit);
                }
                at = end;
            }
            prop_assume!(!edits.is_empty());
            let footprint = Footprint::of(&edits);
            let changes = changes(&chars, &edits);
            let mut buffer = chars.clone();
            prop_assert_eq!(replay(&mut buffer, &changes, false), footprint);
            let after: String = buffer.iter().collect();
            prop_assert_eq!(&after, &apply(&text, edits.clone()).unwrap());

            let index = LineIndex::new(&text);
            let step = Step::new(&edits, &edits, &index, &Bookmarks::new(), &Bookmarks::new())
                .unwrap();
            prop_assert_eq!(step.len_after, after.chars().count());
            let old: Vec<&str> = text.split('\n').collect();
            let new: Vec<&str> = after.split('\n').collect();
            prop_assert_eq!(
                &old[..step.lines_before.start],
                &new[..step.lines_after.start]
            );
            prop_assert_eq!(&old[step.lines_before.end..], &new[step.lines_after.end..]);

            let mut history = BookmarkHistory::new();
            history.record(step.clone());
            let undone = replay(&mut buffer, &changes, true);
            prop_assert_eq!(buffer.iter().collect::<String>(), text.clone());
            prop_assert_eq!(undone, footprint.inverse());
            let len = chars.len();
            prop_assert!(history.undone(undone, len + 1, &Bookmarks::new()).is_none());
            prop_assert!(history.undone(undone, len, &Bookmarks::new()).is_some());
            let redone = replay(&mut buffer, &changes, false);
            prop_assert!(history.redone(redone, step.len_after, &Bookmarks::new()).is_some());
            prop_assert_eq!(history.undoable(), 1);
        }
    }

    #[test]
    fn a_step_gives_back_its_marks() {
        use crate::marks::MarkStyle;
        let text = numbered(4);
        let index = LineIndex::new(&text);
        let edits = [TextEdit::delete(0..7)];
        let mut before = Marks::new();
        before.add(MarkStyle::Mark, std::iter::once(7..13));
        let mut after = before.clone();
        after.remap(&edits);
        let step = Step {
            marks: Some(Box::new((before.clone(), after.clone()))),
            ..Step::new(&edits, &edits, &index, &Bookmarks::new(), &Bookmarks::new()).unwrap()
        };
        let mut history = BookmarkHistory::new();
        history.record(step.clone());
        assert!(
            history
                .undone(step.footprint.inverse(), step.len_before, &Bookmarks::new())
                .is_some()
        );
        assert_eq!(history.undone_marks(), Some(&before));
        assert!(
            history
                .redone(step.footprint, step.len_after, &Bookmarks::new())
                .is_some()
        );
        assert_eq!(history.redone_marks(), Some(&after));
        assert_eq!(after.ranges(MarkStyle::Mark), std::slice::from_ref(&(0..6)));
    }
}
