//! Quick open (Ctrl+P, M8) without a toolkit: what a query asks for, how the open documents,
//! the recent files and the project's files rank, and how a path is shown.

use crate::document::tilde;
use crate::fuzzy;
use crate::location::{LineCol, parse_line_col, split_location};
use std::ops::Range;
use std::path::Path;

/// How many rows the popup shows.
pub const SHOWN_LIMIT: usize = 50;

/// How many files of a project the walk lists at most; a larger tree is cut there and the
/// popup says so.
pub const PROJECT_FILE_CAP: usize = 100_000;

/// What the typed text asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `:line` or `:line:col`: a line of the current document (`None` until it parses).
    Line(Option<LineCol>),
    /// A file whose path matches `pattern`, opened at `position` when one follows it
    /// (`main.rs:40`, `main.rs:40:5`).
    File {
        pattern: String,
        position: Option<LineCol>,
    },
}

impl Request {
    pub fn parse(text: &str) -> Self {
        let text = text.trim();
        if let Some(rest) = text.strip_prefix(':') {
            return Self::Line(parse_line_col(rest));
        }
        match split_location(text) {
            Some((pattern, position)) => Self::File {
                pattern: pattern.to_owned(),
                position: Some(position),
            },
            None => Self::File {
                pattern: text.trim_end_matches(':').to_owned(),
                position: None,
            },
        }
    }
}

/// Where a row comes from, in the order the popup lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Source {
    Open,
    Recent,
    Project,
}

/// A row: an index into its group's candidates and the matched characters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub source: Source,
    pub index: usize,
    pub score: i32,
    /// Character indices of the matched characters.
    pub positions: Vec<usize>,
}

/// The rows for `pattern`: the open documents' matches first, best first, then the recent
/// files', then the project's, at most `limit` in all. An empty pattern lists the groups in
/// their own order (the open documents most recently used first).
pub fn rank_groups<A: AsRef<str>, B: AsRef<str>, C: AsRef<str>>(
    pattern: &str,
    open: &[A],
    recent: &[B],
    project: &[C],
    limit: usize,
) -> Vec<Row> {
    let mut rows = Vec::with_capacity(limit.min(open.len() + recent.len() + project.len()));
    rank_into(&mut rows, Source::Open, pattern, open, limit);
    rank_into(&mut rows, Source::Recent, pattern, recent, limit);
    rank_into(&mut rows, Source::Project, pattern, project, limit);
    rows
}

/// Appends `candidates`' best matches to `rows`, up to `limit` rows in all.
fn rank_into<S: AsRef<str>>(
    rows: &mut Vec<Row>,
    source: Source,
    pattern: &str,
    candidates: &[S],
    limit: usize,
) {
    let room = limit.saturating_sub(rows.len());
    if room == 0 {
        return;
    }
    rows.extend(
        fuzzy::rank(pattern, candidates, room)
            .into_iter()
            .map(|ranked| Row {
                source,
                index: ranked.index,
                score: ranked.score,
                positions: ranked.positions,
            }),
    );
}

/// `path` relative to `root` when it is inside it, else with `~` for the home folder.
pub fn display_path(path: &Path, root: Option<&Path>, home: Option<&Path>) -> String {
    if let Some(relative) = root.and_then(|root| path.strip_prefix(root).ok())
        && !relative.as_os_str().is_empty()
    {
        return relative.to_string_lossy().into_owned();
    }
    tilde(path, home)
}

/// Sorted character indices grouped into runs of neighbours, for highlighting.
pub fn runs(positions: &[usize]) -> Vec<Range<usize>> {
    let mut runs: Vec<Range<usize>> = Vec::new();
    for &position in positions {
        match runs.last_mut() {
            Some(run) if run.end == position => run.end += 1,
            _ => runs.push(position..position + 1),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn queries_name_a_file_and_maybe_a_position() {
        assert_eq!(
            Request::parse("  main.rs "),
            Request::File {
                pattern: "main.rs".into(),
                position: None
            }
        );
        assert_eq!(
            Request::parse("src/main.rs:40"),
            Request::File {
                pattern: "src/main.rs".into(),
                position: Some(LineCol::new(40, None))
            }
        );
        assert_eq!(
            Request::parse("mai:40:5"),
            Request::File {
                pattern: "mai".into(),
                position: Some(LineCol::new(40, Some(5)))
            }
        );
        assert_eq!(
            Request::parse("main.rs:"),
            Request::File {
                pattern: "main.rs".into(),
                position: None
            }
        );
        assert_eq!(
            Request::parse(":12"),
            Request::Line(Some(LineCol::new(12, None)))
        );
        assert_eq!(
            Request::parse(":12:3"),
            Request::Line(Some(LineCol::new(12, Some(3))))
        );
        assert_eq!(Request::parse(":x"), Request::Line(None));
        assert_eq!(
            Request::parse(""),
            Request::File {
                pattern: String::new(),
                position: None
            }
        );
    }

    #[test]
    fn open_documents_come_first_then_recent_then_project_files() {
        let open = ["notes.md", "src/window/mod.rs"];
        let recent = ["~/elsewhere/mod.rs"];
        let project = ["src/main.rs", "src/model.rs", "docs/mod-guide.md"];
        let rows = rank_groups("mod", &open, &recent, &project, 10);
        let found: Vec<(Source, usize)> = rows.iter().map(|row| (row.source, row.index)).collect();
        assert_eq!(
            found,
            [
                (Source::Open, 1),
                (Source::Recent, 0),
                (Source::Project, 1),
                (Source::Project, 2),
            ]
        );
        // The basename match ranks first within its group.
        let rows = rank_groups("model", &open, &recent, &project, 10);
        assert_eq!(rows[0].source, Source::Project);
        assert_eq!(rows[0].index, 1);
        assert_eq!(rows[0].positions, (4..9).collect::<Vec<_>>());
        // The limit cuts across groups.
        assert_eq!(rank_groups("", &open, &recent, &project, 4).len(), 4);
        let all = rank_groups("", &open, &recent, &project, 50);
        assert_eq!(all.len(), 6);
        assert!(all.iter().all(|row| row.positions.is_empty()));
        assert_eq!(all[2].source, Source::Recent);
        assert!(rank_groups("zzz", &open, &recent, &project, 50).is_empty());
    }

    #[test]
    fn paths_show_relative_to_the_project_or_with_a_tilde() {
        let root = PathBuf::from("/home/u/proj");
        let home = PathBuf::from("/home/u");
        let show = |path: &str| display_path(Path::new(path), Some(&root), Some(&home));
        assert_eq!(show("/home/u/proj/src/main.rs"), "src/main.rs");
        assert_eq!(show("/home/u/other/a.txt"), "~/other/a.txt");
        assert_eq!(show("/etc/hosts"), "/etc/hosts");
        assert_eq!(show("/home/u/proj"), "~/proj");
        assert_eq!(
            display_path(Path::new("/home/u/a"), None, Some(&home)),
            "~/a"
        );
    }

    #[test]
    fn positions_group_into_runs() {
        assert_eq!(runs(&[]), Vec::<Range<usize>>::new());
        assert_eq!(runs(&[0, 1, 2, 5, 7, 8]), [0..3, 5..6, 7..9]);
    }
}
