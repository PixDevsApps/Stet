//! The words the find bar and the results panel use to report a search.

use super::{SearchOptions, TemplateWarning};

/// The part of the document that Count and Replace All covered: the rows of
/// [`SearchOptions::scope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    Selection,
    Document,
    FromCaret,
    ToCaret,
}

impl ScopeKind {
    pub const fn describe(self) -> &'static str {
        match self {
            Self::Selection => "in the selection",
            Self::Document => "in the whole document",
            Self::FromCaret => "from the caret to the end",
            Self::ToCaret => "from the start to the caret",
        }
    }
}

impl SearchOptions {
    /// Which row of the scope table applies; `has_selection` means a non-empty selection.
    pub fn scope_kind(&self, has_selection: bool) -> ScopeKind {
        match () {
            _ if self.in_selection && has_selection => ScopeKind::Selection,
            _ if self.wrap => ScopeKind::Document,
            _ if self.backward => ScopeKind::ToCaret,
            _ => ScopeKind::FromCaret,
        }
    }
}

/// `1 match`, `3 matches`.
pub fn counted(count: usize, one: &str, many: &str) -> String {
    format!("{count} {}", if count == 1 { one } else { many })
}

/// The find bar's report after Count.
pub fn count_message(count: usize, scope: ScopeKind) -> String {
    format!(
        "Count: {} {}",
        counted(count, "match", "matches"),
        scope.describe()
    )
}

/// The find bar's report after Replace All.
pub fn replace_message(count: usize, scope: ScopeKind) -> String {
    format!(
        "Replaced {} {}",
        counted(count, "match", "matches"),
        scope.describe()
    )
}

/// The find bar's report after Replace All in Open Documents.
pub fn replace_in_documents_message(count: usize, documents: usize) -> String {
    format!(
        "Replaced {} in {}",
        counted(count, "match", "matches"),
        counted(documents, "document", "documents")
    )
}

/// What the replace field says about a template that is easy to get wrong.
pub fn template_warning(warning: TemplateWarning) -> String {
    match warning {
        TemplateWarning::Grouping => {
            "Parentheses group the replacement and are not inserted; \\( and \\) insert them"
                .to_owned()
        }
        TemplateWarning::Conditional => {
            "? followed by a group number is a condition; \\? inserts a question mark".to_owned()
        }
        TemplateWarning::Truncated { offset } => format!(
            "The ) at character {} closes nothing, so the replacement ends there",
            offset + 1
        ),
    }
}

/// A search's heading in the results panel: `Search "x" (3 hits in 2 files of 10 searched)`.
/// `searched` is the number of files looked at, when it is known.
pub fn results_heading(
    pattern: &str,
    hits: usize,
    files: usize,
    searched: Option<usize>,
) -> String {
    let pattern: String = pattern
        .chars()
        .map(|c| match c {
            '\n' => '⏎',
            '\r' => '␍',
            '\t' => '⇥',
            c => c,
        })
        .collect();
    let mut heading = format!(
        "Search “{pattern}” ({} in {}",
        counted(hits, "hit", "hits"),
        counted(files, "file", "files")
    );
    if let Some(searched) = searched {
        heading.push_str(&format!(" of {searched} searched"));
    }
    heading.push(')');
    heading
}

#[cfg(test)]
mod tests {
    use super::super::SearchMode;
    use super::*;

    #[test]
    fn scope_kinds_follow_the_scope_table() {
        let wrap = SearchOptions::default();
        let plain = SearchOptions {
            wrap: false,
            ..wrap
        };
        let backward = SearchOptions {
            backward: true,
            ..plain
        };
        let in_selection = SearchOptions {
            in_selection: true,
            ..plain
        };
        assert_eq!(wrap.scope_kind(true), ScopeKind::Document);
        assert_eq!(plain.scope_kind(false), ScopeKind::FromCaret);
        assert_eq!(backward.scope_kind(true), ScopeKind::ToCaret);
        assert_eq!(in_selection.scope_kind(true), ScopeKind::Selection);
        assert_eq!(in_selection.scope_kind(false), ScopeKind::FromCaret);
        for (options, has_selection, range) in [
            (wrap, true, 0..10),
            (plain, false, 4..10),
            (backward, false, 0..4),
            (in_selection, true, 2..6),
        ] {
            let selection = has_selection.then_some(2..6);
            assert_eq!(options.scope(4, selection, 10), range, "{options:?}");
        }
        let regex = SearchOptions::new(SearchMode::Regex);
        assert_eq!(regex.scope_kind(false), ScopeKind::Document);
    }

    #[test]
    fn messages() {
        assert_eq!(
            count_message(1, ScopeKind::Document),
            "Count: 1 match in the whole document"
        );
        assert_eq!(
            replace_message(3, ScopeKind::Selection),
            "Replaced 3 matches in the selection"
        );
        assert_eq!(
            replace_in_documents_message(5, 1),
            "Replaced 5 matches in 1 document"
        );
        assert_eq!(
            results_heading("a\tb\n", 3, 2, Some(10)),
            "Search “a⇥b⏎” (3 hits in 2 files of 10 searched)"
        );
        assert_eq!(
            results_heading("x", 1, 1, None),
            "Search “x” (1 hit in 1 file)"
        );
    }

    #[test]
    fn template_warnings_point_at_the_character() {
        let template = super::super::Template::parse("a)b");
        let texts: Vec<String> = template
            .warnings()
            .iter()
            .map(|warning| template_warning(*warning))
            .collect();
        assert_eq!(
            texts,
            ["The ) at character 2 closes nothing, so the replacement ends there"]
        );
        assert!(template_warning(TemplateWarning::Grouping).contains("\\("));
    }
}
