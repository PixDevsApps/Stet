//! The find bar's histories: the find and replace fields, and Find in Files' folders and
//! filters. Most recent first, without duplicates, at most [`HISTORY_LIMIT`] entries.
//! `session.json` keeps the find and replace lists in the same order
//! ([`crate::session::Session::find_history`]).

/// Entries kept per history.
pub const HISTORY_LIMIT: usize = 10;

/// One history, most recent first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    entries: Vec<String>,
    limit: usize,
}

impl Default for History {
    fn default() -> Self {
        Self::new(HISTORY_LIMIT)
    }
}

impl History {
    pub fn new(limit: usize) -> Self {
        Self {
            entries: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// A history from stored entries, most recent first. Empty entries and later duplicates
    /// are dropped, and the list is cut to the limit.
    pub fn from_entries(entries: impl IntoIterator<Item = String>, limit: usize) -> Self {
        let mut history = Self::new(limit);
        for entry in entries {
            if !entry.is_empty() && !history.entries.contains(&entry) {
                history.entries.push(entry);
            }
        }
        history.entries.truncate(history.limit);
        history
    }

    /// Most recent first.
    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn get(&self, index: usize) -> Option<&str> {
        self.entries.get(index).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Puts `entry` first, removing an older copy. Empty text is not remembered. Returns
    /// whether the history changed.
    pub fn add(&mut self, entry: &str) -> bool {
        if entry.is_empty() || self.entries.first().is_some_and(|first| first == entry) {
            return false;
        }
        self.entries.retain(|existing| existing != entry);
        self.entries.insert(0, entry.to_owned());
        self.entries.truncate(self.limit);
        true
    }
}

/// Walking a history with Up and Down in a text field, as in a shell. The text typed before the
/// walk started comes back after the newest entry.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Recall {
    position: Option<usize>,
    draft: String,
}

impl Recall {
    /// The next older entry; `current` is the field's text, kept as the draft when the walk
    /// starts. `None` at the oldest entry.
    pub fn older(&mut self, history: &History, current: &str) -> Option<String> {
        let next = match self.position {
            None => {
                let start = usize::from(history.get(0) == Some(current));
                if start >= history.len() {
                    return None;
                }
                self.draft = current.to_owned();
                start
            }
            Some(position) if position + 1 < history.len() => position + 1,
            Some(_) => return None,
        };
        self.position = Some(next);
        history.get(next).map(str::to_owned)
    }

    /// The next newer entry, then the draft. `None` when not walking.
    pub fn newer(&mut self, history: &History) -> Option<String> {
        match self.position? {
            0 => {
                self.position = None;
                Some(std::mem::take(&mut self.draft))
            }
            position => {
                self.position = Some(position - 1);
                history.get(position - 1).map(str::to_owned)
            }
        }
    }

    /// Ends the walk, for example when the user types.
    pub fn reset(&mut self) {
        self.position = None;
        self.draft.clear();
    }
}

/// Every history the find bar keeps.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchHistory {
    pub find: History,
    pub replace: History,
    /// Find in Files' folders.
    pub directories: History,
    /// Find in Files' filters.
    pub filters: History,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entries(history: &History) -> Vec<&str> {
        history.entries().iter().map(String::as_str).collect()
    }

    #[test]
    fn newest_first_without_duplicates() {
        let mut history = History::new(3);
        assert!(history.add("a"));
        assert!(history.add("b"));
        assert!(!history.add("b"));
        assert!(!history.add(""));
        assert!(history.add("a"));
        assert_eq!(entries(&history), ["a", "b"]);
        history.add("c");
        history.add("d");
        assert_eq!(entries(&history), ["d", "c", "a"]);
        assert!(history.add("A"));
        assert_eq!(entries(&history), ["A", "d", "c"]);
    }

    #[test]
    fn stored_entries_are_cleaned() {
        let history = History::from_entries(
            ["x", "", "y", "x", "z", "w"].map(str::to_owned),
            HISTORY_LIMIT,
        );
        assert_eq!(entries(&history), ["x", "y", "z", "w"]);
        let short = History::from_entries(["1", "2", "3"].map(str::to_owned), 2);
        assert_eq!(entries(&short), ["1", "2"]);
        assert_eq!(History::default().entries().len(), 0);
    }

    #[test]
    fn recall_walks_back_and_returns_the_draft() {
        let history = History::from_entries(["new", "mid", "old"].map(str::to_owned), 10);
        let mut recall = Recall::default();
        assert_eq!(recall.newer(&history), None);
        assert_eq!(recall.older(&history, "typed").as_deref(), Some("new"));
        assert_eq!(recall.older(&history, "new").as_deref(), Some("mid"));
        assert_eq!(recall.older(&history, "mid").as_deref(), Some("old"));
        assert_eq!(recall.older(&history, "old"), None);
        assert_eq!(recall.newer(&history).as_deref(), Some("mid"));
        assert_eq!(recall.newer(&history).as_deref(), Some("new"));
        assert_eq!(recall.newer(&history).as_deref(), Some("typed"));
        assert_eq!(recall.newer(&history), None);
    }

    #[test]
    fn recall_skips_the_entry_already_shown() {
        let history = History::from_entries(["new", "old"].map(str::to_owned), 10);
        let mut recall = Recall::default();
        assert_eq!(recall.older(&history, "new").as_deref(), Some("old"));
        assert_eq!(recall.newer(&history).as_deref(), Some("new"));
        recall.reset();
        let empty = History::default();
        assert_eq!(recall.older(&empty, "x"), None);
        let only = History::from_entries(["x".to_owned()], 10);
        assert_eq!(recall.older(&only, "x"), None);
    }
}
