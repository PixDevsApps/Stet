use crate::text::{Bias, TextEdit, map_offset};

/// A caret position: a document and a character offset in it.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Location<D> {
    pub doc: D,
    pub offset: usize,
}

impl<D> Location<D> {
    pub fn new(doc: D, offset: usize) -> Self {
        Self { doc, offset }
    }
}

/// Back and forward navigation between caret positions.
///
/// Record a location for every jump: the place left and the place reached (go to line, search
/// results, bookmarks, differences, quick open, switching documents, a click far away). Moves
/// within `near_lines` lines of the current entry in the same document update that entry instead
/// of adding one. Don't record the caret moves that [`back`](Self::back) and
/// [`forward`](Self::forward) cause themselves.
///
/// `line_of` callbacks give the line of an offset in a document, which the buffer answers
/// cheaply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History<D> {
    entries: Vec<Location<D>>,
    current: usize,
    limit: usize,
    near_lines: usize,
}

impl<D: PartialEq + Clone> Default for History<D> {
    fn default() -> Self {
        Self::new()
    }
}

impl<D: PartialEq + Clone> History<D> {
    pub const DEFAULT_LIMIT: usize = 100;
    pub const DEFAULT_NEAR_LINES: usize = 10;

    pub fn new() -> Self {
        Self::with_limits(Self::DEFAULT_LIMIT, Self::DEFAULT_NEAR_LINES)
    }

    /// Keeps at most `limit` entries (at least one) and merges moves within `near_lines` lines.
    pub fn with_limits(limit: usize, near_lines: usize) -> Self {
        Self {
            entries: Vec::new(),
            current: 0,
            limit: limit.max(1),
            near_lines,
        }
    }

    /// Oldest first.
    pub fn entries(&self) -> &[Location<D>] {
        &self.entries
    }

    pub fn current(&self) -> Option<&Location<D>> {
        self.entries.get(self.current)
    }

    pub fn can_go_back(&self) -> bool {
        self.current > 0
    }

    pub fn can_go_forward(&self) -> bool {
        self.current + 1 < self.entries.len()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.current = 0;
    }

    /// Records a jump source or target. A location near the current entry replaces it; any
    /// other drops the forward entries and becomes the newest one.
    pub fn record(&mut self, location: Location<D>, line_of: impl Fn(&D, usize) -> usize) {
        if let Some(current) = self.entries.get_mut(self.current)
            && current.doc == location.doc
            && line_of(&location.doc, current.offset)
                .abs_diff(line_of(&location.doc, location.offset))
                <= self.near_lines
        {
            current.offset = location.offset;
            return;
        }
        self.entries.truncate(self.current + 1);
        self.entries.push(location);
        if self.entries.len() > self.limit {
            self.entries.remove(0);
        }
        self.current = self.entries.len() - 1;
    }

    /// Go Back from `here`, which is recorded first so that Go Forward returns to it.
    pub fn back(
        &mut self,
        here: Location<D>,
        line_of: impl Fn(&D, usize) -> usize,
    ) -> Option<Location<D>> {
        self.record(here, line_of);
        self.current = self.current.checked_sub(1)?;
        self.current().cloned()
    }

    /// Go Forward from `here`. Moving away from the current entry since going back ends the
    /// forward history, as any new location does.
    pub fn forward(
        &mut self,
        here: Location<D>,
        line_of: impl Fn(&D, usize) -> usize,
    ) -> Option<Location<D>> {
        self.record(here, line_of);
        if !self.can_go_forward() {
            return None;
        }
        self.current += 1;
        self.current().cloned()
    }

    /// Moves the entries of `doc` through `edits` (sorted and non-overlapping).
    pub fn remap(&mut self, doc: &D, edits: &[TextEdit]) {
        for entry in self.entries.iter_mut().filter(|entry| entry.doc == *doc) {
            entry.offset = map_offset(entry.offset, edits, Bias::Before);
        }
        self.dedup();
    }

    /// Forgets a closed document.
    pub fn remove_document(&mut self, doc: &D) {
        let removed_before = self.entries[..self.current.min(self.entries.len())]
            .iter()
            .filter(|entry| entry.doc == *doc)
            .count();
        self.current -= removed_before;
        self.entries.retain(|entry| entry.doc != *doc);
        self.dedup();
    }

    /// Merges neighbouring entries that became identical.
    fn dedup(&mut self) {
        let mut index = 1;
        while index < self.entries.len() {
            if self.entries[index] == self.entries[index - 1] {
                self.entries.remove(index);
                if self.current >= index {
                    self.current -= 1;
                }
            } else {
                index += 1;
            }
        }
        self.current = self.current.min(self.entries.len().saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every document has 100-character lines.
    fn line_of(_doc: &&str, offset: usize) -> usize {
        offset / 100
    }

    fn at(doc: &'static str, line: usize) -> Location<&'static str> {
        Location::new(doc, line * 100)
    }

    fn lines(history: &History<&'static str>) -> Vec<(&'static str, usize)> {
        history
            .entries()
            .iter()
            .map(|entry| (entry.doc, entry.offset / 100))
            .collect()
    }

    #[test]
    fn goes_back_and_forward_through_jumps() {
        let mut history = History::new();
        history.record(at("a", 1), line_of);
        history.record(at("a", 200), line_of);
        history.record(at("b", 5), line_of);
        assert!(!history.can_go_forward());
        assert_eq!(history.back(at("b", 7), line_of), Some(at("a", 200)));
        assert_eq!(history.back(at("a", 200), line_of), Some(at("a", 1)));
        assert_eq!(history.back(at("a", 1), line_of), None);
        assert_eq!(history.current(), Some(&at("a", 1)));
        assert_eq!(history.forward(at("a", 1), line_of), Some(at("a", 200)));
        assert_eq!(history.forward(at("a", 200), line_of), Some(at("b", 7)));
        assert_eq!(history.forward(at("b", 7), line_of), None);
        assert_eq!(lines(&history), [("a", 1), ("a", 200), ("b", 7)]);
    }

    #[test]
    fn nearby_moves_update_the_current_entry() {
        let mut history = History::new();
        history.record(at("a", 50), line_of);
        history.record(at("a", 58), line_of);
        history.record(at("a", 61), line_of);
        assert_eq!(lines(&history), [("a", 61)]);
        history.record(at("a", 90), line_of);
        history.record(at("b", 90), line_of);
        assert_eq!(lines(&history), [("a", 61), ("a", 90), ("b", 90)]);
    }

    #[test]
    fn a_new_jump_after_going_back_drops_the_forward_entries() {
        let mut history = History::new();
        for line in [0, 100, 200, 300] {
            history.record(at("a", line), line_of);
        }
        history.back(at("a", 300), line_of);
        history.back(at("a", 200), line_of);
        assert_eq!(history.forward(at("a", 104), line_of), Some(at("a", 200)));
        history.back(at("a", 200), line_of);
        history.record(at("a", 500), line_of);
        assert_eq!(lines(&history), [("a", 0), ("a", 104), ("a", 500)]);
        assert!(!history.can_go_forward());
        assert_eq!(history.forward(at("a", 700), line_of), None);
    }

    #[test]
    fn keeps_at_most_the_limit() {
        let mut history = History::with_limits(3, 0);
        for line in 0..6 {
            history.record(at("a", line), line_of);
        }
        assert_eq!(lines(&history), [("a", 3), ("a", 4), ("a", 5)]);
        assert_eq!(history.back(at("a", 5), line_of), Some(at("a", 4)));
    }

    #[test]
    fn entries_follow_edits_to_their_document() {
        let mut history = History::with_limits(10, 0);
        history.record(Location::new("a", 10), line_of);
        history.record(Location::new("b", 10), line_of);
        history.record(Location::new("a", 30), line_of);
        history.remap(
            &"a",
            &[TextEdit::insert(0, "12345"), TextEdit::delete(20..40)],
        );
        let offsets: Vec<(&str, usize)> = history
            .entries()
            .iter()
            .map(|entry| (entry.doc, entry.offset))
            .collect();
        assert_eq!(offsets, [("a", 15), ("b", 10), ("a", 25)]);
    }

    #[test]
    fn closing_a_document_drops_its_entries() {
        let mut history = History::with_limits(10, 0);
        for (doc, line) in [("a", 1), ("b", 1), ("a", 2), ("c", 1), ("a", 3)] {
            history.record(at(doc, line), line_of);
        }
        history.back(at("a", 3), line_of);
        assert_eq!(history.current(), Some(&at("c", 1)));
        history.remove_document(&"a");
        assert_eq!(lines(&history), [("b", 1), ("c", 1)]);
        assert_eq!(history.current(), Some(&at("c", 1)));
        history.remove_document(&"c");
        assert_eq!(history.current(), Some(&at("b", 1)));
        history.remove_document(&"b");
        assert_eq!(history.current(), None);
        assert!(!history.can_go_back());
    }

    #[test]
    fn entries_that_meet_after_a_close_merge() {
        let mut history = History::with_limits(10, 0);
        for (doc, line) in [("a", 1), ("b", 1), ("a", 1)] {
            history.record(at(doc, line), line_of);
        }
        history.remove_document(&"b");
        assert_eq!(lines(&history), [("a", 1)]);
        assert_eq!(history.current(), Some(&at("a", 1)));
    }
}
