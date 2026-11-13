//! The words the find bar and the results panel use to report a search.

use std::fmt::Write as _;
use std::time::Duration;

use super::{SearchOptions, TemplateWarning};
use crate::text::counted;

/// The part of the document that Count and Replace All covered: the rows of
/// [`SearchOptions::scope`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScopeKind {
    /// In the selection.
    Selection,
    /// In the whole text, with Wrap around.
    Document,
    /// From the caret to the end.
    FromCaret,
    /// From the start to the caret, searching backward.
    ToCaret,
}

impl ScopeKind {
    /// The words a report ends with, such as `in the selection`.
    #[must_use]
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
    #[must_use]
    pub fn scope_kind(&self, has_selection: bool) -> ScopeKind {
        match () {
            () if self.in_selection && has_selection => ScopeKind::Selection,
            () if self.wrap => ScopeKind::Document,
            () if self.backward => ScopeKind::ToCaret,
            () => ScopeKind::FromCaret,
        }
    }
}

/// The find bar's report after Count.
#[must_use]
pub fn count_message(count: usize, scope: ScopeKind) -> String {
    format!(
        "Count: {} {}",
        counted(count, "match", "matches"),
        scope.describe()
    )
}

/// The find bar's report after Replace All.
#[must_use]
pub fn replace_message(count: usize, scope: ScopeKind) -> String {
    format!(
        "Replaced {} {}",
        counted(count, "match", "matches"),
        scope.describe()
    )
}

/// The find bar's report after Replace All in Open Documents.
#[must_use]
pub fn replace_in_documents_message(count: usize, documents: usize) -> String {
    format!(
        "Replaced {} in {}",
        counted(count, "match", "matches"),
        counted(documents, "document", "documents")
    )
}

/// What the replace field says about a template that is easy to get wrong.
#[must_use]
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
#[must_use]
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
        let _ = write!(heading, " of {searched} searched");
    }
    heading.push(')');
    heading
}

/// Find in Files' status line while it runs: `Searching ~/src… 3 hits so far`.
#[must_use]
pub fn find_in_files_progress(folder: &str, hits: usize) -> String {
    format!(
        "Searching {folder}… {} so far",
        counted(hits, "hit", "hits")
    )
}

/// Find in Files' status line once it ends: its `heading`, how long it `took`, whether the user
/// stopped it or it stopped at the hit limit (`truncated_at`), and how many files could not be
/// read.
#[must_use]
pub fn find_in_files_status(
    heading: &str,
    took: Duration,
    stopped: bool,
    truncated_at: Option<usize>,
    unreadable: usize,
) -> String {
    let mut status = format!("{heading} in {:.2} s", took.as_secs_f64());
    if stopped {
        status.push_str(" — stopped");
    } else if let Some(hits) = truncated_at {
        let _ = write!(status, " — stopped at {hits} hits");
    }
    if unreadable > 0 {
        let _ = write!(
            status,
            " — {} could not be read",
            counted(unreadable, "file", "files")
        );
    }
    status
}

/// Replace in Files' status line while it runs.
#[must_use]
pub fn replace_in_files_progress(folder: &str, searched: usize, changed: usize) -> String {
    format!(
        "Replacing in {folder}… {} searched, {} changed",
        counted(searched, "file", "files"),
        counted(changed, "file", "files"),
    )
}

/// Replace in Files' status line once it ends: its `heading`, how long it `took`, whether the
/// user stopped it, the files it failed to write and the files listed without their lines.
#[must_use]
pub fn replace_in_files_status(
    heading: &str,
    took: Duration,
    stopped: bool,
    failed: usize,
    unlisted: usize,
) -> String {
    let mut status = format!("{heading} in {:.2} s", took.as_secs_f64());
    if stopped {
        status.push_str(" — stopped");
    }
    if failed > 0 {
        let _ = write!(status, " — {} failed", counted(failed, "file", "files"));
    }
    if unlisted > 0 {
        let _ = write!(
            status,
            " — {} listed without their lines",
            counted(unlisted, "file", "files")
        );
    }
    status
}

#[cfg(test)]
mod tests {
    use super::super::SearchMode;
    use super::*;
    use crate::text::testing::{at, chars};

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
            let selection = has_selection.then_some(chars(2..6));
            assert_eq!(
                options.scope(at(4), selection, at(10)),
                chars(range),
                "{options:?}"
            );
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
    fn find_in_files_status_lines() {
        assert_eq!(
            find_in_files_progress("~/src", 1),
            "Searching ~/src… 1 hit so far"
        );
        assert_eq!(
            find_in_files_progress("/tmp", 12),
            "Searching /tmp… 12 hits so far"
        );
        let heading = "Search “x” (3 hits in 2 files of 10 searched)";
        let took = Duration::from_millis(1234);
        assert_eq!(
            find_in_files_status(heading, took, false, None, 0),
            format!("{heading} in 1.23 s")
        );
        assert_eq!(
            find_in_files_status(heading, took, true, Some(50), 1),
            format!("{heading} in 1.23 s — stopped — 1 file could not be read")
        );
        assert_eq!(
            find_in_files_status(heading, took, false, Some(50), 2),
            format!("{heading} in 1.23 s — stopped at 50 hits — 2 files could not be read")
        );
    }

    #[test]
    fn replace_in_files_status_lines() {
        assert_eq!(
            replace_in_files_progress("~/src", 1, 0),
            "Replacing in ~/src… 1 file searched, 0 files changed"
        );
        let heading = "Replace “a” with “b” (2 replacements in 1 file of 4 searched)";
        let took = Duration::from_millis(50);
        assert_eq!(
            replace_in_files_status(heading, took, false, 0, 0),
            format!("{heading} in 0.05 s")
        );
        assert_eq!(
            replace_in_files_status(heading, took, true, 1, 3),
            format!(
                "{heading} in 0.05 s — stopped — 1 file failed — 3 files listed without their lines"
            )
        );
        assert_eq!(
            replace_in_files_status(heading, took, false, 2, 1),
            format!("{heading} in 0.05 s — 2 files failed — 1 file listed without their lines")
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
