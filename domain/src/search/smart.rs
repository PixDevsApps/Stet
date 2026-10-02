//! Smart highlighting: selecting a word highlights its other occurrences.
//!
//! By default it matches case and whole words only. With whole words, the selection must be
//! exactly one word: word characters only, with no word character just before or after it. A
//! selection that spans lines never qualifies.

use super::{Query, SearchMode, SearchOptions};

/// How smart highlighting matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SmartHighlight {
    pub match_case: bool,
    pub whole_word: bool,
}

impl Default for SmartHighlight {
    fn default() -> Self {
        Self {
            match_case: true,
            whole_word: true,
        }
    }
}

/// A word character for smart highlighting and whole-word matching: a letter, digit or `_`.
pub fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl SmartHighlight {
    /// The query for the selection `selected`, with `before` and `after` the characters next
    /// to it, or `None` when the selection does not qualify.
    pub fn query(
        &self,
        selected: &str,
        before: Option<char>,
        after: Option<char>,
    ) -> Option<Query> {
        if selected.is_empty() || selected.contains(['\n', '\r']) {
            return None;
        }
        if self.whole_word {
            let one_word = selected.chars().all(is_word_char)
                && !before.is_some_and(is_word_char)
                && !after.is_some_and(is_word_char);
            if !one_word {
                return None;
            }
        } else if selected.trim().is_empty() {
            return None;
        }
        let options = SearchOptions::new(SearchMode::Normal)
            .with_match_case(self.match_case)
            .with_whole_word(self.whole_word);
        Some(Query::new(selected, options))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_whole_words_on_one_line_qualify() {
        let smart = SmartHighlight::default();
        let query = smart.query("user", Some(' '), Some('.')).unwrap();
        assert_eq!(query.pattern, "user");
        assert!(query.options.match_case && query.options.whole_word);
        assert_eq!(query.options.mode, SearchMode::Normal);
        assert!(smart.query("user", None, None).is_some());
        assert!(smart.query("snake_case2", Some('('), Some(')')).is_some());
        assert!(smart.query("café", Some(' '), Some(' ')).is_some());
        assert!(smart.query("", None, None).is_none());
        assert!(smart.query("use", None, Some('r')).is_none());
        assert!(smart.query("ser", Some('u'), None).is_none());
        assert!(smart.query("a b", None, None).is_none());
        assert!(smart.query("->", Some(' '), Some(' ')).is_none());
        assert!(smart.query("a\nb", None, None).is_none());
    }

    #[test]
    fn without_whole_word_any_single_line_text_qualifies() {
        let smart = SmartHighlight {
            match_case: false,
            whole_word: false,
        };
        let query = smart.query("a->b", Some('x'), Some('y')).unwrap();
        assert!(!query.options.match_case && !query.options.whole_word);
        assert!(smart.query("  ", None, None).is_none());
        assert!(smart.query("a\r\nb", None, None).is_none());
    }
}
