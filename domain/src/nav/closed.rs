//! The closed tabs that Restore Closed Tab brings back: the most recent first, capped.

use crate::text::Position;
use std::collections::VecDeque;
use std::path::PathBuf;

/// The most closed tabs the window remembers.
const CLOSED_LIMIT: usize = 20;

/// A closed tab that Restore Closed Tab can bring back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClosedTab {
    /// A file's tab: the file opens again with the caret where it was.
    File { path: PathBuf, position: Position },
    /// An untitled tab: its text comes back, with its language and the name it was given.
    Untitled {
        text: String,
        position: Position,
        /// The highlighting language's id.
        language: Option<String>,
        name: Option<String>,
    },
}

/// The closed tabs, the most recent on top. Past twenty the oldest is forgotten.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClosedTabs {
    tabs: VecDeque<ClosedTab>,
}

impl ClosedTabs {
    /// Remembers `tab` as the most recently closed.
    pub fn push(&mut self, tab: ClosedTab) {
        self.tabs.push_back(tab);
        if self.tabs.len() > CLOSED_LIMIT {
            self.tabs.pop_front();
        }
    }

    /// Takes the most recently closed tab.
    pub fn pop(&mut self) -> Option<ClosedTab> {
        self.tabs.pop_back()
    }

    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::Line;

    fn file(number: usize) -> ClosedTab {
        ClosedTab::File {
            path: PathBuf::from(format!("/tmp/{number}.txt")),
            position: Position::new(Line::new(number), 0),
        }
    }

    #[test]
    fn the_most_recent_comes_back_first() {
        let mut closed = ClosedTabs::default();
        assert!(closed.is_empty());
        closed.push(file(1));
        closed.push(file(2));
        assert_eq!(closed.len(), 2);
        assert_eq!(closed.pop(), Some(file(2)));
        assert_eq!(closed.pop(), Some(file(1)));
        assert_eq!(closed.pop(), None);
        assert!(closed.is_empty());
    }

    #[test]
    fn past_the_limit_the_oldest_is_forgotten() {
        let mut closed = ClosedTabs::default();
        for number in 0..=CLOSED_LIMIT {
            closed.push(file(number));
        }
        assert_eq!(closed.len(), CLOSED_LIMIT);
        let mut restored = Vec::new();
        while let Some(tab) = closed.pop() {
            restored.push(tab);
        }
        assert_eq!(restored.first(), Some(&file(CLOSED_LIMIT)));
        assert_eq!(restored.last(), Some(&file(1)));
    }

    #[test]
    fn untitled_tabs_keep_their_text_language_and_name() {
        let mut closed = ClosedTabs::default();
        let untitled = ClosedTab::Untitled {
            text: "notes".to_owned(),
            position: Position::new(Line::ZERO, 3),
            language: Some("markdown".to_owned()),
            name: Some("Ideas".to_owned()),
        };
        closed.push(untitled.clone());
        assert_eq!(closed.pop(), Some(untitled));
    }
}
